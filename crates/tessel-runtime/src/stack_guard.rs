//! Reports running out of stack as a Tessel runtime error.
//!
//! Too many nested calls (usually a function that calls itself without a
//! case that stops) run past the end of the stack into a guard page, and the
//! operating system stops the program with a signal. Without a handler that
//! shows up as a silent crash, so each thread gets a small separate stack
//! for signal handlers, and the handler prints what happened and exits with
//! status 101, like the other runtime errors. On Windows the same is done
//! with an exception handler, and stack kept back for it on each thread.


#[cfg(unix)]
mod imp {
    use std::ptr;

    const ALT_STACK: usize = 64 * 1024;

    /// Installs the handler, and the signal stack for the calling thread.
    pub fn install() {
        unsafe {
            thread_init();
            for sig in [libc::SIGSEGV, libc::SIGBUS] {
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = handler as *const () as usize;
                sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                libc::sigemptyset(&mut sa.sa_mask);
                libc::sigaction(sig, &sa, ptr::null_mut());
            }
        }
    }

    /// Gives the calling thread its own signal stack (each thread needs one).
    pub fn thread_init() {
        unsafe {
            let mem = libc::mmap(
                ptr::null_mut(),
                ALT_STACK,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            );
            if mem == libc::MAP_FAILED {
                return;
            }
            let st = libc::stack_t { ss_sp: mem, ss_size: ALT_STACK, ss_flags: 0 };
            libc::sigaltstack(&st, ptr::null_mut());
        }
    }

    /// The lowest address of the calling thread's stack.
    unsafe fn stack_bottom() -> usize {
        unsafe {
            #[cfg(target_os = "macos")]
            {
                let t = libc::pthread_self();
                libc::pthread_get_stackaddr_np(t) as usize - libc::pthread_get_stacksize_np(t)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let mut attr: libc::pthread_attr_t = std::mem::zeroed();
                if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
                    return 0;
                }
                let (mut addr, mut size) = (ptr::null_mut(), 0usize);
                libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
                libc::pthread_attr_destroy(&mut attr);
                addr as usize
            }
        }
    }

    fn say(text: &str) {
        unsafe { libc::write(2, text.as_ptr().cast(), text.len()) };
    }

    extern "C" fn handler(_sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
        unsafe {
            let addr = (*info).si_addr() as usize;
            let bottom = stack_bottom();
            // Within the guard area just below the stack (or its last page).
            let overflow = bottom != 0 && addr < bottom + 64 * 1024 && addr + 16 * 1024 * 1024 >= bottom;
            if overflow {
                say(
                    "error: stack overflow: too many function calls inside each other\n = help: a function that \
                     calls itself needs a case where it stops calling itself, like `if n == 0 { return 1 }`\n",
                );
            } else {
                say("error: the program crashed (invalid memory access); this is a bug in Tessel, please report it\n");
            }
            // (The REPL goes on; see `on_error`.)
            if let Some(hook) = crate::ON_ERROR.get() {
                hook();
            }
            libc::_exit(101);
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    /// `EXCEPTION_STACK_OVERFLOW`.
    const STACK_OVERFLOW: u32 = 0xC000_00FD;
    /// Stack kept for the handler when the rest runs out.
    const GUARANTEE: u32 = 64 * 1024;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        // (The rest isn't read.)
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *const ExceptionRecord,
        context: *mut c_void,
    }

    unsafe extern "system" {
        fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32) -> *mut c_void;
        fn SetThreadStackGuarantee(size: *mut u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn WriteFile(file: *mut c_void, data: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
        fn ExitProcess(code: u32) -> !;
    }

    /// Installs the handler (once), and stack for it on the calling thread.
    pub fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| unsafe {
            // First, before Rust's own (which only says the thread overflowed).
            AddVectoredExceptionHandler(1, handler);
        });
        thread_init();
    }

    /// Keeps stack for the handler on the calling thread (each thread needs it).
    pub fn thread_init() {
        let mut size = GUARANTEE;
        unsafe { SetThreadStackGuarantee(&mut size) };
    }

    fn say(text: &str) {
        unsafe {
            // STD_ERROR_HANDLE
            let err = GetStdHandle((-12i32) as u32);
            let mut written = 0;
            WriteFile(err, text.as_ptr(), text.len() as u32, &mut written, std::ptr::null_mut());
        }
    }

    unsafe extern "system" fn handler(info: *mut ExceptionPointers) -> i32 {
        unsafe {
            if info.is_null() || (*info).record.is_null() || (*(*info).record).code != STACK_OVERFLOW {
                // EXCEPTION_CONTINUE_SEARCH: someone else's to handle.
                return 0;
            }
            say(
                "error: stack overflow: too many function calls inside each other\n = help: a function that \
                 calls itself needs a case where it stops calling itself, like `if n == 0 { return 1 }`\n",
            );
            // (The REPL goes on; see `on_error`.)
            if let Some(hook) = crate::ON_ERROR.get() {
                hook();
            }
            ExitProcess(101);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn install() {}
    pub fn thread_init() {}
}

pub use imp::{install, thread_init};
