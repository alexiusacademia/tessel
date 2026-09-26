//! The Tessel runtime: memory, strings, lists, dictionaries, printing and
//! (later) the UI.
//!
//! Compiled Tessel programs link against this static library and call its C
//! ABI (`tsl_*` functions). The runtime also provides the process entry point,
//! which calls the program's `tsl_main`.
//!
//! Conventions shared with the code generator:
//! - Every heap object starts with a [`Header`]. A negative reference count
//!   marks an immortal object (string literals), which is never freed.
//! - Runtime functions *borrow* their object arguments, except where a
//!   comment says an element is *moved* in. Returned objects are owned (+1).
//! - Elements of lists and dictionaries are stored inline; a [`TslType`]
//!   describes their size and how to retain, release, compare and hash them.

#![allow(clippy::missing_safety_doc)]

use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::io::Write;
use std::ptr;

// ===== objects =====

#[repr(C)]
pub struct Header {
    rc: i64,
    drop: unsafe extern "C" fn(*mut Header),
}

/// Describes a value type stored inside a list, dictionary or box.
#[repr(C)]
pub struct TslType {
    pub size: i64,
    pub align: i64,
    pub retain: Option<unsafe extern "C" fn(*mut u8)>,
    pub release: Option<unsafe extern "C" fn(*mut u8)>,
    pub eq: Option<unsafe extern "C" fn(*const u8, *const u8) -> bool>,
    pub hash: Option<unsafe extern "C" fn(*const u8) -> u64>,
    pub cmp: Option<unsafe extern "C" fn(*const u8, *const u8) -> i32>,
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(p: *mut u8);
    fn realloc(p: *mut u8, size: usize) -> *mut u8;
}

unsafe fn alloc(size: usize) -> *mut u8 {
    let p = unsafe { malloc(size.max(1)) };
    if p.is_null() {
        fatal("out of memory");
    }
    p
}

unsafe fn new_object(size: usize, drop: unsafe extern "C" fn(*mut Header)) -> *mut Header {
    let h = unsafe { alloc(size) } as *mut Header;
    unsafe { h.write(Header { rc: 1, drop }) };
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_retain(obj: *mut Header) {
    if !obj.is_null() && unsafe { (*obj).rc } >= 0 {
        unsafe { (*obj).rc += 1 };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_release(obj: *mut Header) {
    if obj.is_null() || unsafe { (*obj).rc } < 0 {
        return;
    }
    unsafe {
        (*obj).rc -= 1;
        if (*obj).rc == 0 {
            ((*obj).drop)(obj);
        }
    }
}

unsafe fn elem_retain(ty: *const TslType, p: *mut u8) {
    if let Some(f) = unsafe { (*ty).retain } {
        unsafe { f(p) };
    }
}

unsafe fn elem_release(ty: *const TslType, p: *mut u8) {
    if let Some(f) = unsafe { (*ty).release } {
        unsafe { f(p) };
    }
}

// ===== boxes: enum payloads, closure environments, captured variables =====

/// Data starts this many bytes into a box (16-byte aligned).
pub const BOX_DATA: usize = 32;

#[repr(C)]
struct BoxObj {
    h: Header,
    ty: *const TslType,
    _pad: u64,
}

unsafe extern "C" fn box_drop(h: *mut Header) {
    let b = h as *mut BoxObj;
    unsafe {
        elem_release((*b).ty, (b as *mut u8).add(BOX_DATA));
        free(b as *mut u8);
    }
}

/// A new box whose data (uninitialized, at offset [`BOX_DATA`]) is released
/// with `ty` when the box is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_box_new(ty: *const TslType) -> *mut Header {
    let size = BOX_DATA + unsafe { (*ty).size } as usize;
    let b = unsafe { new_object(size, box_drop) } as *mut BoxObj;
    unsafe {
        (*b).ty = ty;
        (*b)._pad = 0;
    }
    b as *mut Header
}

/// Compares the data of two boxes of the same type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_box_eq(a: *mut Header, b: *mut Header) -> bool {
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    let ty = unsafe { (*(a as *mut BoxObj)).ty };
    match unsafe { (*ty).eq } {
        Some(eq) => unsafe { eq((a as *mut u8).add(BOX_DATA), (b as *mut u8).add(BOX_DATA)) },
        None => true,
    }
}

// ===== source locations and runtime errors =====

#[repr(C)]
pub struct Loc {
    file: *const c_char,
    line: i32,
    col: i32,
}

fn fatal(msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("error: {msg}");
    std::process::exit(101);
}

unsafe fn panic_at(loc: *const Loc, msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("error: {msg}");
    if !loc.is_null() {
        let loc = unsafe { &*loc };
        let file = unsafe { CStr::from_ptr(loc.file) }.to_string_lossy();
        eprintln!(" --> {file}:{}:{}", loc.line, loc.col);
    }
    std::process::exit(101);
}

/// Stops the program with a message (a NUL-terminated string) and location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_panic(msg: *const c_char, loc: *const Loc) -> ! {
    let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    unsafe { panic_at(loc, &msg) }
}

// ===== strings =====

#[repr(C)]
pub struct Str {
    h: Header,
    len: i64,
    // followed by `len` bytes of UTF-8
}

unsafe extern "C" fn str_drop(h: *mut Header) {
    unsafe { free(h as *mut u8) };
}

unsafe fn str_text<'a>(s: *const Str) -> &'a str {
    unsafe {
        let len = (*s).len as usize;
        let bytes = std::slice::from_raw_parts((s as *const u8).add(size_of::<Str>()), len);
        std::str::from_utf8_unchecked(bytes)
    }
}

fn str_new(text: &str) -> *mut Str {
    unsafe {
        let s = new_object(size_of::<Str>() + text.len(), str_drop) as *mut Str;
        (*s).len = text.len() as i64;
        ptr::copy_nonoverlapping(text.as_ptr(), (s as *mut u8).add(size_of::<Str>()), text.len());
        s
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_concat(parts: *const *const Str, n: i64) -> *mut Str {
    let parts = unsafe { std::slice::from_raw_parts(parts, n as usize) };
    let mut out = String::new();
    for &p in parts {
        out.push_str(unsafe { str_text(p) });
    }
    str_new(&out)
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_str_from_int(n: i64) -> *mut Str {
    str_new(&n.to_string())
}

/// Floats always show a decimal point (`3.0`), so they read differently from Ints.
pub fn format_float(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e16 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_str_from_float(x: f64) -> *mut Str {
    str_new(&format_float(x))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_str_from_bool(b: bool) -> *mut Str {
    str_new(if b { "true" } else { "false" })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_eq(a: *const Str, b: *const Str) -> bool {
    unsafe { str_text(a) == str_text(b) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_cmp(a: *const Str, b: *const Str) -> i32 {
    unsafe { str_text(a).cmp(str_text(b)) as i32 }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_hash(s: *const Str) -> u64 {
    fnv(unsafe { str_text(s) }.as_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_count(s: *const Str) -> i64 {
    unsafe { str_text(s) }.chars().count() as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_contains(s: *const Str, part: *const Str) -> bool {
    unsafe { str_text(s).contains(str_text(part)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_has_prefix(s: *const Str, part: *const Str) -> bool {
    unsafe { str_text(s).starts_with(str_text(part)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_has_suffix(s: *const Str, part: *const Str) -> bool {
    unsafe { str_text(s).ends_with(str_text(part)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_trim(s: *const Str) -> *mut Str {
    str_new(unsafe { str_text(s) }.trim())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_uppercase(s: *const Str) -> *mut Str {
    str_new(&unsafe { str_text(s) }.to_uppercase())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_lowercase(s: *const Str) -> *mut Str {
    str_new(&unsafe { str_text(s) }.to_lowercase())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_replace(s: *const Str, old: *const Str, new: *const Str) -> *mut Str {
    let (s, old, new) = unsafe { (str_text(s), str_text(old), str_text(new)) };
    if old.is_empty() { str_new(s) } else { str_new(&s.replace(old, new)) }
}

/// Splits into a `[String]`. An empty separator splits into characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_split(s: *const Str, sep: *const Str) -> *mut List {
    let (s, sep) = unsafe { (str_text(s), str_text(sep)) };
    let parts: Vec<String> =
        if sep.is_empty() { s.chars().map(String::from).collect() } else { s.split(sep).map(String::from).collect() };
    let list = unsafe { tsl_list_new(&TSL_TYPE_STR, parts.len() as i64) };
    let mut slot = list;
    for p in parts {
        let obj = str_new(&p);
        unsafe { tsl_list_push(&mut slot, &obj as *const *mut Str as *const u8) };
    }
    slot
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_to_int(s: *const Str, out: *mut i64) -> bool {
    match unsafe { str_text(s) }.trim().parse::<i64>() {
        Ok(n) => {
            unsafe { *out = n };
            true
        }
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_to_float(s: *const Str, out: *mut f64) -> bool {
    match unsafe { str_text(s) }.trim().parse::<f64>() {
        Ok(x) => {
            unsafe { *out = x };
            true
        }
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_print(s: *const Str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(unsafe { str_text(s) }.as_bytes());
    let _ = out.write_all(b"\n");
}

unsafe extern "C" fn str_retain(p: *mut u8) {
    unsafe { tsl_retain(*(p as *mut *mut Header)) }
}

unsafe extern "C" fn str_release(p: *mut u8) {
    unsafe { tsl_release(*(p as *mut *mut Header)) }
}

unsafe extern "C" fn str_eq_ptr(a: *const u8, b: *const u8) -> bool {
    unsafe { tsl_str_eq(*(a as *const *const Str), *(b as *const *const Str)) }
}

unsafe extern "C" fn str_hash_ptr(a: *const u8) -> u64 {
    unsafe { tsl_str_hash(*(a as *const *const Str)) }
}

unsafe extern "C" fn str_cmp_ptr(a: *const u8, b: *const u8) -> i32 {
    unsafe { tsl_str_cmp(*(a as *const *const Str), *(b as *const *const Str)) }
}

/// Element type of `[String]`, used for lists the runtime creates.
#[unsafe(no_mangle)]
pub static TSL_TYPE_STR: TslType = TslType {
    size: 8,
    align: 8,
    retain: Some(str_retain),
    release: Some(str_release),
    eq: Some(str_eq_ptr),
    hash: Some(str_hash_ptr),
    cmp: Some(str_cmp_ptr),
};

// ===== lists =====

#[repr(C)]
pub struct List {
    h: Header,
    len: i64,
    cap: i64,
    elem: *const TslType,
    data: *mut u8,
}

unsafe fn elem_size(l: *const List) -> usize {
    unsafe { (*(*l).elem).size as usize }
}

unsafe fn elem_at(l: *const List, i: i64) -> *mut u8 {
    unsafe { (*l).data.add(i as usize * elem_size(l)) }
}

unsafe extern "C" fn list_drop(h: *mut Header) {
    let l = h as *mut List;
    unsafe {
        for i in 0..(*l).len {
            elem_release((*l).elem, elem_at(l, i));
        }
        free((*l).data);
        free(l as *mut u8);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_new(elem: *const TslType, cap: i64) -> *mut List {
    unsafe {
        let l = new_object(size_of::<List>(), list_drop) as *mut List;
        let cap = cap.max(4);
        (*l).len = 0;
        (*l).cap = cap;
        (*l).elem = elem;
        (*l).data = alloc(cap as usize * (*elem).size.max(1) as usize);
        l
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_len(l: *const List) -> i64 {
    unsafe { (*l).len }
}

/// A copy of `l` whose elements are retained.
unsafe fn list_clone(l: *const List) -> *mut List {
    unsafe {
        let n = (*l).len;
        let copy = tsl_list_new((*l).elem, n);
        ptr::copy_nonoverlapping((*l).data, (*copy).data, n as usize * elem_size(l));
        (*copy).len = n;
        for i in 0..n {
            elem_retain((*l).elem, elem_at(copy, i));
        }
        copy
    }
}

/// Makes the list in `slot` uniquely owned (copy-on-write) before a change.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_unique(slot: *mut *mut List) -> *mut List {
    unsafe {
        let l = *slot;
        if (*l).h.rc != 1 {
            let copy = list_clone(l);
            tsl_release(l as *mut Header);
            *slot = copy;
        }
        *slot
    }
}

unsafe fn reserve(l: *mut List, extra: i64) {
    unsafe {
        let need = (*l).len + extra;
        if need > (*l).cap {
            let cap = need.max((*l).cap * 2);
            (*l).data = realloc((*l).data, cap as usize * elem_size(l).max(1));
            if (*l).data.is_null() {
                fatal("out of memory");
            }
            (*l).cap = cap;
        }
    }
}

unsafe fn index_error(loc: *const Loc, i: i64, len: i64) -> ! {
    let items = if len == 1 { "item" } else { "items" };
    unsafe { panic_at(loc, &format!("index {i} is out of range for a list of {len} {items}")) }
}

/// Pointer to element `i`, for reading. Stops the program if out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_get(l: *const List, i: i64, loc: *const Loc) -> *mut u8 {
    unsafe {
        if i < 0 || i >= (*l).len {
            index_error(loc, i, (*l).len);
        }
        elem_at(l, i)
    }
}

/// Pointer to element `i` of the list in `slot`, for writing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_elem_mut(slot: *mut *mut List, i: i64, loc: *const Loc) -> *mut u8 {
    unsafe {
        let len = (**slot).len;
        if i < 0 || i >= len {
            index_error(loc, i, len);
        }
        elem_at(tsl_list_unique(slot), i)
    }
}

/// Appends the value at `value` (moved in).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_push(slot: *mut *mut List, value: *const u8) {
    unsafe {
        let l = tsl_list_unique(slot);
        reserve(l, 1);
        ptr::copy_nonoverlapping(value, elem_at(l, (*l).len), elem_size(l));
        (*l).len += 1;
    }
}

/// Inserts the value at `value` (moved in) before index `i`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_insert(slot: *mut *mut List, value: *const u8, i: i64, loc: *const Loc) {
    unsafe {
        let len = (**slot).len;
        if i < 0 || i > len {
            index_error(loc, i, len);
        }
        let l = tsl_list_unique(slot);
        reserve(l, 1);
        let size = elem_size(l);
        ptr::copy(elem_at(l, i), elem_at(l, i + 1), (len - i) as usize * size);
        ptr::copy_nonoverlapping(value, elem_at(l, i), size);
        (*l).len += 1;
    }
}

/// Removes element `i`, moving it to `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_remove(slot: *mut *mut List, i: i64, out: *mut u8, loc: *const Loc) {
    unsafe {
        let len = (**slot).len;
        if i < 0 || i >= len {
            index_error(loc, i, len);
        }
        let l = tsl_list_unique(slot);
        let size = elem_size(l);
        ptr::copy_nonoverlapping(elem_at(l, i), out, size);
        ptr::copy(elem_at(l, i + 1), elem_at(l, i), (len - i - 1) as usize * size);
        (*l).len -= 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_concat(a: *const List, b: *const List) -> *mut List {
    unsafe {
        let out = list_clone(a);
        let mut slot = out;
        reserve(slot, (*b).len);
        for i in 0..(*b).len {
            let p = elem_at(b, i);
            elem_retain((*b).elem, p);
            tsl_list_push(&mut slot, p);
        }
        slot
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_eq(a: *const List, b: *const List) -> bool {
    unsafe {
        if (*a).len != (*b).len {
            return false;
        }
        let Some(eq) = (*(*a).elem).eq else { return true };
        (0..(*a).len).all(|i| eq(elem_at(a, i), elem_at(b, i)))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_contains(l: *const List, value: *const u8) -> bool {
    unsafe {
        let Some(eq) = (*(*l).elem).eq else { return false };
        (0..(*l).len).any(|i| eq(elem_at(l, i), value))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_reversed(l: *const List) -> *mut List {
    unsafe {
        let out = list_clone(l);
        let (n, size) = ((*out).len as usize, elem_size(out));
        for i in 0..n / 2 {
            ptr::swap_nonoverlapping(elem_at(out, i as i64), elem_at(out, (n - 1 - i) as i64), size);
        }
        out
    }
}

/// A sorted copy. With `less` (a closure adapter: `less(ctx, a, b)`), uses
/// it; otherwise uses the element type's natural order. The sort is stable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_sorted(
    l: *const List,
    less: Option<unsafe extern "C" fn(*mut u8, *const u8, *const u8) -> bool>,
    ctx: *mut u8,
) -> *mut List {
    unsafe {
        let out = list_clone(l);
        let (n, size) = ((*out).len as usize, elem_size(out));
        let mut order: Vec<usize> = (0..n).collect();
        let cmp = (*(*l).elem).cmp;
        order.sort_by(|&x, &y| {
            let (a, b) = (elem_at(l, x as i64), elem_at(l, y as i64));
            match less {
                Some(less) => {
                    if less(ctx, a, b) {
                        std::cmp::Ordering::Less
                    } else if less(ctx, b, a) {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                None => cmp.map_or(0, |c| c(a, b)).cmp(&0),
            }
        });
        let mut data = Vec::with_capacity(n * size);
        for i in order {
            data.extend_from_slice(std::slice::from_raw_parts(elem_at(out, i as i64), size));
        }
        ptr::copy_nonoverlapping(data.as_ptr(), (*out).data, n * size);
        out
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_joined(l: *const List, sep: *const Str) -> *mut Str {
    unsafe {
        let parts: Vec<&str> = (0..(*l).len).map(|i| str_text(*(elem_at(l, i) as *const *const Str))).collect();
        str_new(&parts.join(str_text(sep)))
    }
}

// ===== dictionaries =====

#[repr(C)]
pub struct Dict {
    h: Header,
    key: *const TslType,
    value: *const TslType,
    inner: *mut DictInner,
}

/// Entries in insertion order, plus a hash index into them.
struct DictInner {
    keys: Vec<u8>,
    values: Vec<u8>,
    len: usize,
    index: HashMap<u64, Vec<usize>>,
}

unsafe extern "C" fn dict_drop(h: *mut Header) {
    let d = h as *mut Dict;
    unsafe {
        let inner = Box::from_raw((*d).inner);
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        for i in 0..inner.len {
            elem_release((*d).key, inner.keys.as_ptr().add(i * ks) as *mut u8);
            elem_release((*d).value, inner.values.as_ptr().add(i * vs) as *mut u8);
        }
        free(d as *mut u8);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_new(key: *const TslType, value: *const TslType) -> *mut Dict {
    unsafe {
        let d = new_object(size_of::<Dict>(), dict_drop) as *mut Dict;
        (*d).key = key;
        (*d).value = value;
        let inner = DictInner { keys: Vec::new(), values: Vec::new(), len: 0, index: HashMap::new() };
        (*d).inner = Box::into_raw(Box::new(inner));
        d
    }
}

unsafe fn dict_find(d: *const Dict, key: *const u8) -> Option<usize> {
    unsafe {
        let inner = &*(*d).inner;
        let ks = (*(*d).key).size as usize;
        let hash = ((*(*d).key).hash.expect("key type has no hash"))(key);
        let eq = (*(*d).key).eq.expect("key type has no eq");
        inner.index.get(&hash)?.iter().copied().find(|&i| eq(inner.keys.as_ptr().add(i * ks), key))
    }
}

unsafe fn dict_unique(slot: *mut *mut Dict) -> *mut Dict {
    unsafe {
        let d = *slot;
        if (*d).h.rc == 1 {
            return d;
        }
        let copy = tsl_dict_new((*d).key, (*d).value);
        let src = &*(*d).inner;
        let dst = &mut *(*copy).inner;
        dst.keys = src.keys.clone();
        dst.values = src.values.clone();
        dst.len = src.len;
        dst.index = src.index.clone();
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        for i in 0..dst.len {
            elem_retain((*d).key, dst.keys.as_mut_ptr().add(i * ks));
            elem_retain((*d).value, dst.values.as_mut_ptr().add(i * vs));
        }
        tsl_release(d as *mut Header);
        *slot = copy;
        copy
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_len(d: *const Dict) -> i64 {
    unsafe { (*(*d).inner).len as i64 }
}

/// Copies the value for `key` (retained) to `out`; returns false if absent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_get(d: *const Dict, key: *const u8, out: *mut u8) -> bool {
    unsafe {
        let Some(i) = dict_find(d, key) else { return false };
        let vs = (*(*d).value).size as usize;
        let src = (*(*d).inner).values.as_ptr().add(i * vs);
        ptr::copy_nonoverlapping(src, out, vs);
        elem_retain((*d).value, out);
        true
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_contains(d: *const Dict, key: *const u8) -> bool {
    unsafe { dict_find(d, key).is_some() }
}

/// Sets `key` to `value`; both are moved in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_set(slot: *mut *mut Dict, key: *mut u8, value: *mut u8) {
    unsafe {
        let d = dict_unique(slot);
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        let inner = &mut *(*d).inner;
        if let Some(i) = dict_find(d, key) {
            let old = inner.values.as_mut_ptr().add(i * vs);
            elem_release((*d).value, old);
            ptr::copy_nonoverlapping(value, old, vs);
            elem_release((*d).key, key);
            return;
        }
        let hash = ((*(*d).key).hash.unwrap())(key);
        inner.keys.extend_from_slice(std::slice::from_raw_parts(key, ks));
        inner.values.extend_from_slice(std::slice::from_raw_parts(value, vs));
        inner.index.entry(hash).or_default().push(inner.len);
        inner.len += 1;
    }
}

/// Removes `key`, moving its value to `out`; returns false if absent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_remove(slot: *mut *mut Dict, key: *const u8, out: *mut u8) -> bool {
    unsafe {
        if dict_find(*slot, key).is_none() {
            return false;
        }
        let d = dict_unique(slot);
        let i = dict_find(d, key).unwrap();
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        let hash_of = (*(*d).key).hash.unwrap();
        let inner = &mut *(*d).inner;
        ptr::copy_nonoverlapping(inner.values.as_ptr().add(i * vs), out, vs);
        elem_release((*d).key, inner.keys.as_mut_ptr().add(i * ks));
        inner.keys.drain(i * ks..(i + 1) * ks);
        inner.values.drain(i * vs..(i + 1) * vs);
        inner.len -= 1;
        // Rebuild the index: positions after `i` shifted down by one.
        inner.index.clear();
        for j in 0..inner.len {
            let h = hash_of(inner.keys.as_ptr().add(j * ks));
            inner.index.entry(h).or_default().push(j);
        }
        true
    }
}

unsafe fn dict_column(d: *const Dict, keys: bool) -> *mut List {
    unsafe {
        let ty = if keys { (*d).key } else { (*d).value };
        let size = (*ty).size as usize;
        let inner = &*(*d).inner;
        let mut out = tsl_list_new(ty, inner.len as i64);
        let src = if keys { &inner.keys } else { &inner.values };
        for i in 0..inner.len {
            let p = src.as_ptr().add(i * size) as *mut u8;
            elem_retain(ty, p);
            tsl_list_push(&mut out, p);
        }
        out
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_keys(d: *const Dict) -> *mut List {
    unsafe { dict_column(d, true) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_values(d: *const Dict) -> *mut List {
    unsafe { dict_column(d, false) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_dict_eq(a: *const Dict, b: *const Dict) -> bool {
    unsafe {
        let (ia, ib) = (&*(*a).inner, &*(*b).inner);
        if ia.len != ib.len {
            return false;
        }
        let (ks, vs) = ((*(*a).key).size as usize, (*(*a).value).size as usize);
        let veq = (*(*a).value).eq;
        (0..ia.len).all(|i| {
            let key = ia.keys.as_ptr().add(i * ks);
            match dict_find(b, key) {
                Some(j) => veq.is_none_or(|eq| eq(ia.values.as_ptr().add(i * vs), ib.values.as_ptr().add(j * vs))),
                None => false,
            }
        })
    }
}

// ===== entry point =====

#[cfg(not(test))]
unsafe extern "C" {
    fn tsl_main();
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    unsafe { tsl_main() };
    let _ = std::io::stdout().flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn text(s: *const Str) -> String {
        unsafe { str_text(s) }.to_string()
    }

    #[test]
    fn floats_show_a_decimal_point() {
        assert_eq!(format_float(3.0), "3.0");
        assert_eq!(format_float(0.1 + 0.2), "0.30000000000000004");
    }

    #[test]
    fn lists_are_copy_on_write() {
        unsafe {
            let mut a = tsl_list_new(&TSL_TYPE_STR, 0);
            let s = str_new("x");
            tsl_list_push(&mut a, &s as *const _ as *const u8);
            let b = a;
            tsl_retain(b as *mut Header); // a second owner
            let mut a2 = a;
            let t = str_new("y");
            tsl_list_push(&mut a2, &t as *const _ as *const u8);
            assert_ne!(a2, b, "push on a shared list must copy it");
            assert_eq!(tsl_list_len(b), 1);
            assert_eq!(tsl_list_len(a2), 2);
            let joined = tsl_list_joined(a2, str_new(","));
            assert_eq!(text(joined), "x,y");
        }
    }

    #[test]
    fn dictionaries_set_get_remove() {
        unsafe {
            let mut d = tsl_dict_new(&TSL_TYPE_STR, &TSL_TYPE_STR);
            for (k, v) in [("a", "1"), ("b", "2"), ("a", "3")] {
                let (mut k, mut v) = (str_new(k), str_new(v));
                tsl_dict_set(&mut d, &mut k as *mut _ as *mut u8, &mut v as *mut _ as *mut u8);
            }
            assert_eq!(tsl_dict_len(d), 2);
            let key = str_new("a");
            let mut out: *mut Str = ptr::null_mut();
            assert!(tsl_dict_get(d, &key as *const _ as *const u8, &mut out as *mut _ as *mut u8));
            assert_eq!(text(out), "3");
            assert!(tsl_dict_remove(&mut d, &key as *const _ as *const u8, &mut out as *mut _ as *mut u8));
            assert_eq!(tsl_dict_len(d), 1);
            let keys = tsl_dict_keys(d);
            assert_eq!(text(tsl_list_joined(keys, str_new(""))), "b");
        }
    }
}
