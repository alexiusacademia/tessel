//! `Shared<T>`: one value that every thread may read and change, under a
//! lock.
//!
//! The object holds a lock and a cell with the value (its layout is the
//! compiler's: it reads and writes the cell between `tsl_shared_lock` and
//! `tsl_shared_unlock`). Values put in the cell are marked shared first
//! (their counts change atomically), so any thread may hold what it reads.
//! The lock is taken again freely by the thread that holds it (`get()`
//! inside an `update` block), and a worker that fails lets go of the locks
//! it held (see `ui::worker_failed`).

use std::cell::RefCell;
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

use crate::Header;

/// Who holds a lock, and how many times over.
struct Lock {
    state: Mutex<(Option<ThreadId>, usize)>,
    freed: Condvar,
}

#[repr(C)]
pub struct SharedObj {
    h: Header,
    lock: *mut Lock,
    /// Releases what the cell holds (the compiler's, for its type).
    drop_value: Option<unsafe extern "C" fn(*mut u8)>,
    // followed by the cell
}

thread_local! {
    /// The `Shared` objects whose lock this thread holds, once per taking.
    static HELD: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn shared_drop(h: *mut Header) {
    unsafe {
        let s = h as *mut SharedObj;
        if let Some(drop_value) = (*s).drop_value {
            drop_value(cell(s));
        }
        drop(Box::from_raw((*s).lock));
        crate::free(h as *mut u8);
    }
}

unsafe fn cell(s: *mut SharedObj) -> *mut u8 {
    unsafe { (s as *mut u8).add(size_of::<SharedObj>()) }
}

/// A new `Shared` with a cell of `size` bytes (zeros, for the compiler to
/// store the first value in).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_shared_new(size: i64, drop_value: Option<unsafe extern "C" fn(*mut u8)>) -> *mut SharedObj {
    unsafe {
        let total = size_of::<SharedObj>() + size.max(0) as usize;
        let s = crate::new_object(total, shared_drop) as *mut SharedObj;
        let lock = Box::into_raw(Box::new(Lock { state: Mutex::new((None, 0)), freed: Condvar::new() }));
        (&raw mut (*s).lock).write(lock);
        (&raw mut (*s).drop_value).write(drop_value);
        std::ptr::write_bytes(cell(s), 0, size.max(0) as usize);
        s
    }
}

/// The cell of a new `Shared`, for its first value (no lock: no other
/// thread has it yet).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_shared_cell(s: *mut SharedObj) -> *mut u8 {
    unsafe { cell(s) }
}

/// A function going to another thread: it mustn't use values from where
/// it was made (they'd be counted from two threads at once).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_fn_portable(env: *mut Header) {
    if !env.is_null() {
        crate::fatal(
            "a function given to another thread uses values from where it was made; give it a top-level function (`fn` at the top of a file), and pass values separately",
        );
    }
}

/// Takes the lock (waiting for another thread to let go of it) and gives
/// the cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_shared_lock(s: *mut SharedObj) -> *mut u8 {
    unsafe {
        let lock = &*(*s).lock;
        let me = std::thread::current().id();
        let mut st = lock.state.lock().unwrap();
        while st.0.is_some_and(|t| t != me) {
            st = lock.freed.wait(st).unwrap();
        }
        st.0 = Some(me);
        st.1 += 1;
        drop(st);
        HELD.with(|h| h.borrow_mut().push(s as usize));
        cell(s)
    }
}

/// Lets go of the lock (once, for each `tsl_shared_lock`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_shared_unlock(s: *mut SharedObj) {
    unsafe {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(i) = h.iter().rposition(|&x| x == s as usize) {
                h.remove(i);
            }
        });
        release_once(s);
    }
}

unsafe fn release_once(s: *mut SharedObj) {
    unsafe {
        let lock = &*(*s).lock;
        let mut st = lock.state.lock().unwrap();
        if st.1 > 0 {
            st.1 -= 1;
        }
        if st.1 == 0 {
            st.0 = None;
            lock.freed.notify_all();
        }
    }
}

/// A worker failed: the locks it held are let go (what the cells hold is
/// what was there before its `update`, which hadn't stored its result).
pub(crate) fn let_go() {
    let held: Vec<usize> = HELD.with(|h| std::mem::take(&mut *h.borrow_mut()));
    for s in held {
        unsafe { release_once(s as *mut SharedObj) };
    }
}
