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

mod codec;
mod stdlib;
pub mod ui;

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
    /// Keys the runtime hashes and compares itself, without calling `hash`
    /// and `eq`: 1 for `Int`, 2 for `String`, 0 for anything else.
    pub kind: i64,
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

/// Makes the box in `slot` uniquely owned before a change (copy-on-write):
/// a box shared with other values is replaced by a copy of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_box_unique(slot: *mut *mut Header) -> *mut Header {
    unsafe {
        let b = *slot;
        if (*b).rc == 1 {
            return b;
        }
        let ty = (*(b as *mut BoxObj)).ty;
        let copy = tsl_box_new(ty);
        let (from, to) = ((b as *mut u8).add(BOX_DATA), (copy as *mut u8).add(BOX_DATA));
        std::ptr::copy_nonoverlapping(from, to, (*ty).size as usize);
        if let Some(retain) = (*ty).retain {
            retain(to);
        }
        tsl_release(b);
        *slot = copy;
        copy
    }
}

/// `==` on interface values (method table, box): the same type, and equal
/// values. A type that can't be compared (it holds functions) is only equal
/// to the very same copy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_iface_eq(ta: *const u8, a: *mut Header, tb: *const u8, b: *mut Header) -> bool {
    if ta != tb {
        return false;
    }
    if a == b {
        return true;
    }
    let ty = unsafe { (*(a as *mut BoxObj)).ty };
    match unsafe { (*ty).eq } {
        Some(eq) => unsafe { eq((a as *mut u8).add(BOX_DATA), (b as *mut u8).add(BOX_DATA)) },
        None => false,
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

/// The drop function of `Str` objects (also used for `Data`).
pub(crate) fn str_drop_fn() -> unsafe extern "C" fn(*mut Header) {
    str_drop
}

pub(crate) unsafe fn str_text<'a>(s: *const Str) -> &'a str {
    unsafe {
        let len = (*s).len as usize;
        let bytes = std::slice::from_raw_parts((s as *const u8).add(size_of::<Str>()), len);
        std::str::from_utf8_unchecked(bytes)
    }
}

pub(crate) fn str_new(text: &str) -> *mut Str {
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

/// Lines of text, without their line endings (`\n` or `\r\n`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_lines(s: *const Str) -> *mut List {
    let text = unsafe { str_text(s) };
    unsafe { str_list(text.lines()) }
}

/// Characters `from..<to` (clamped to the text), by character index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_substring(s: *const Str, from: i64, to: i64) -> *mut Str {
    let text = unsafe { str_text(s) };
    let from = from.max(0) as usize;
    let to = to.max(0) as usize;
    if to <= from {
        return str_new("");
    }
    str_new(&text.chars().skip(from).take(to - from).collect::<String>())
}

/// Character index of the first occurrence of `part`, or -1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_find(s: *const Str, part: *const Str) -> i64 {
    let (text, part) = unsafe { (str_text(s), str_text(part)) };
    match text.find(part) {
        Some(byte) => text[..byte].chars().count() as i64,
        None => -1,
    }
}

/// A new `[String]` from an iterator of text.
pub(crate) unsafe fn str_list<'a>(items: impl Iterator<Item = &'a str>) -> *mut List {
    let mut list = unsafe { tsl_list_new(&TSL_TYPE_STR, 0) };
    for item in items {
        let obj = str_new(item);
        unsafe { tsl_list_push(&mut list, &obj as *const *mut Str as *const u8) };
    }
    list
}

// ===== files and the environment =====

/// The file's text, or null if it can't be read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_read_file(path: *const Str) -> *mut Str {
    match std::fs::read_to_string(unsafe { str_text(path) }) {
        Ok(text) => str_new(&text),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_write_file(path: *const Str, text: *const Str) -> bool {
    std::fs::write(unsafe { str_text(path) }, unsafe { str_text(text) }).is_ok()
}

/// Names in a folder, sorted; null if it can't be read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_folder(path: *const Str) -> *mut List {
    let Ok(entries) = std::fs::read_dir(unsafe { str_text(path) }) else { return ptr::null_mut() };
    let mut names: Vec<String> = entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
    names.sort();
    unsafe { str_list(names.iter().map(String::as_str)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_file_exists(path: *const Str) -> bool {
    std::path::Path::new(unsafe { str_text(path) }).exists()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_is_folder(path: *const Str) -> bool {
    std::path::Path::new(unsafe { str_text(path) }).is_dir()
}

/// Creates a folder (and any missing parents). Returns whether it worked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_create_folder(path: *const Str) -> bool {
    std::fs::create_dir_all(unsafe { str_text(path) }).is_ok()
}

/// Renames or moves a file or folder. Returns whether it worked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_rename_file(from: *const Str, to: *const Str) -> bool {
    std::fs::rename(unsafe { str_text(from) }, unsafe { str_text(to) }).is_ok()
}

/// Deletes a file (not a folder). Returns whether it worked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_delete_file(path: *const Str) -> bool {
    std::fs::remove_file(unsafe { str_text(path) }).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_current_folder() -> *mut Str {
    let dir = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
    str_new(&dir)
}

/// Math functions, by number (the order of MATH_FNS in the compiler's codegen).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_math(op: i32, a: f64, b: f64) -> f64 {
    match op {
        0 => a.sqrt(),
        1 => a.powf(b),
        2 => a.exp(),
        3 => a.ln(),
        4 => a.log10(),
        5 => a.sin(),
        6 => a.cos(),
        7 => a.tan(),
        8 => a.asin(),
        9 => a.acos(),
        10 => a.atan(),
        11 => a.atan2(b),
        12 => a.floor(),
        13 => a.ceil(),
        14 => a.round(),
        15 => a.abs(),
        16 => a.min(b),
        17 => a.max(b),
        _ => f64::NAN,
    }
}

/// `x.formatted(decimals: n)`: the number with exactly `n` decimals (0–15).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_float_format(x: f64, decimals: i64) -> *mut Str {
    let text = format!("{:.*}", decimals.clamp(0, 15) as usize, x);
    // "-0.00" reads oddly; show "0.00".
    let text = if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') { text[1..].to_string() } else { text };
    str_new(&text)
}

/// An environment variable, or null if it isn't set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_environment(name: *const Str) -> *mut Str {
    match std::env::var(unsafe { str_text(name) }) {
        Ok(v) => str_new(&v),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_print(s: *const Str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(unsafe { str_text(s) }.as_bytes());
    let _ = out.write_all(b"\n");
}

/// `print(value, terminator:)` (`to_err` = 0) and `printError` (1): the text,
/// then the terminator, flushed so a prompt without a line break shows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_print_to(to_err: u8, s: *const Str, end: *const Str) {
    let (s, end) = unsafe { (str_text(s), str_text(end)) };
    if to_err != 0 {
        let mut out = std::io::stderr().lock();
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(end.as_bytes());
    } else {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(end.as_bytes());
        if !end.ends_with('\n') {
            let _ = out.flush();
        }
    }
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
    kind: 2,
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

/// Items `from` up to, not including, `to` (both clamped to the list), as a new list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_slice(l: *const List, from: i64, to: i64) -> *mut List {
    unsafe {
        let n = (*l).len;
        let (from, to) = (from.clamp(0, n), to.clamp(0, n));
        let mut out = tsl_list_new((*l).elem, (to - from).max(0));
        for i in from..to {
            let p = elem_at(l as *mut List, i);
            if let Some(retain) = (*(*l).elem).retain {
                retain(p);
            }
            tsl_list_push(&mut out, p);
        }
        out
    }
}

/// A copy in random order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_list_shuffled(l: *const List) -> *mut List {
    unsafe {
        let out = list_clone(l);
        let (n, size) = ((*out).len as usize, elem_size(out));
        // Fisher–Yates.
        for i in (1..n).rev() {
            let j = (stdlib::random_u64() % (i as u64 + 1)) as usize;
            if i != j {
                ptr::swap_nonoverlapping(elem_at(out, i as i64), elem_at(out, j as i64), size);
            }
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

/// Entries in insertion order, and an open-addressing index into them.
///
/// Removing a key marks its entry dead (the index still points at it, and
/// lookups skip it); dead entries are dropped when the index is rebuilt.
struct DictInner {
    keys: Vec<u8>,
    values: Vec<u8>,
    hashes: Vec<u64>,
    live: Vec<bool>,
    /// Live entries.
    len: usize,
    /// Entry numbers (`EMPTY` for a free slot); its length is a power of two.
    index: Vec<u32>,
}

const EMPTY: u32 = u32::MAX;

impl DictInner {
    fn new() -> DictInner {
        DictInner { keys: Vec::new(), values: Vec::new(), hashes: Vec::new(), live: Vec::new(), len: 0, index: Vec::new() }
    }

    /// Entries, live and dead.
    fn used(&self) -> usize {
        self.hashes.len()
    }
}

/// Mixes a hash so that its low bits (which pick the index slot) vary.
fn mix(h: u64) -> u64 {
    let h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^ (h >> 29)
}

fn str_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ bytes.len() as u64;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(c.try_into().unwrap())).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    for &b in chunks.remainder() {
        h = (h.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    h
}

unsafe fn str_bytes<'a>(s: *const Str) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts((s as *const u8).add(size_of::<Str>()), (*s).len as usize) }
}

/// The hash of the key at `key` (Ints and Strings directly, others through their type).
unsafe fn key_hash(ty: *const TslType, key: *const u8) -> u64 {
    unsafe {
        mix(match (*ty).kind {
            1 => *(key as *const i64) as u64,
            2 => str_hash(str_bytes(*(key as *const *const Str))),
            _ => ((*ty).hash.expect("key type has no hash"))(key),
        })
    }
}

unsafe fn key_eq(ty: *const TslType, a: *const u8, b: *const u8) -> bool {
    unsafe {
        match (*ty).kind {
            1 => *(a as *const i64) == *(b as *const i64),
            2 => {
                let (x, y) = (*(a as *const *const Str), *(b as *const *const Str));
                x == y || str_bytes(x) == str_bytes(y)
            }
            _ => ((*ty).eq.expect("key type has no eq"))(a, b),
        }
    }
}

unsafe extern "C" fn dict_drop(h: *mut Header) {
    let d = h as *mut Dict;
    unsafe {
        let inner = Box::from_raw((*d).inner);
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        for i in (0..inner.used()).filter(|&i| inner.live[i]) {
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
        (*d).inner = Box::into_raw(Box::new(DictInner::new()));
        d
    }
}

/// The entry holding `key` (with hash `hash`), if any.
unsafe fn dict_find_hashed(d: *const Dict, key: *const u8, hash: u64) -> Option<usize> {
    unsafe {
        let inner = &*(*d).inner;
        if inner.index.is_empty() {
            return None;
        }
        let ks = (*(*d).key).size as usize;
        let mask = inner.index.len() - 1;
        let mut pos = hash as usize & mask;
        loop {
            let e = inner.index[pos];
            if e == EMPTY {
                return None;
            }
            let e = e as usize;
            if inner.hashes[e] == hash && inner.live[e] && key_eq((*d).key, inner.keys.as_ptr().add(e * ks), key) {
                return Some(e);
            }
            pos = (pos + 1) & mask;
        }
    }
}

unsafe fn dict_find(d: *const Dict, key: *const u8) -> Option<usize> {
    unsafe { dict_find_hashed(d, key, key_hash((*d).key, key)) }
}

/// Drops dead entries and rebuilds the index with room for `extra` more.
unsafe fn dict_rebuild(d: *mut Dict, extra: usize) {
    unsafe {
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        let inner = &mut *(*d).inner;
        if inner.len < inner.used() {
            let mut w = 0;
            for r in 0..inner.used() {
                if inner.live[r] {
                    if w != r {
                        inner.keys.copy_within(r * ks..(r + 1) * ks, w * ks);
                        inner.values.copy_within(r * vs..(r + 1) * vs, w * vs);
                        inner.hashes[w] = inner.hashes[r];
                    }
                    w += 1;
                }
            }
            inner.keys.truncate(w * ks);
            inner.values.truncate(w * vs);
            inner.hashes.truncate(w);
            inner.live.clear();
            inner.live.resize(w, true);
        }
        // At most 3/4 full after adding `extra`.
        let want = ((inner.len + extra) * 4 / 3 + 1).next_power_of_two().max(8);
        inner.index.clear();
        inner.index.resize(want, EMPTY);
        let mask = want - 1;
        for (e, &h) in inner.hashes.iter().enumerate() {
            let mut pos = h as usize & mask;
            while inner.index[pos] != EMPTY {
                pos = (pos + 1) & mask;
            }
            inner.index[pos] = e as u32;
        }
    }
}

/// The dictionary in `slot`, not shared with other values (copy-on-write).
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
        dst.hashes = src.hashes.clone();
        dst.live = src.live.clone();
        dst.len = src.len;
        dst.index = src.index.clone();
        let (ks, vs) = ((*(*d).key).size as usize, (*(*d).value).size as usize);
        for i in (0..dst.used()).filter(|&i| dst.live[i]) {
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
        let hash = key_hash((*d).key, key);
        if let Some(i) = dict_find_hashed(d, key, hash) {
            let old = (*(*d).inner).values.as_mut_ptr().add(i * vs);
            elem_release((*d).value, old);
            ptr::copy_nonoverlapping(value, old, vs);
            elem_release((*d).key, key);
            return;
        }
        let inner = &mut *(*d).inner;
        if (inner.used() + 1) * 4 > inner.index.len() * 3 {
            dict_rebuild(d, 1);
        }
        let inner = &mut *(*d).inner;
        let e = inner.used();
        inner.keys.extend_from_slice(std::slice::from_raw_parts(key, ks));
        inner.values.extend_from_slice(std::slice::from_raw_parts(value, vs));
        inner.hashes.push(hash);
        inner.live.push(true);
        inner.len += 1;
        let mask = inner.index.len() - 1;
        let mut pos = hash as usize & mask;
        while inner.index[pos] != EMPTY {
            pos = (pos + 1) & mask;
        }
        inner.index[pos] = e as u32;
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
        let inner = &mut *(*d).inner;
        ptr::copy_nonoverlapping(inner.values.as_ptr().add(i * vs), out, vs);
        elem_release((*d).key, inner.keys.as_mut_ptr().add(i * ks));
        inner.live[i] = false;
        inner.len -= 1;
        // Many dead entries: drop them.
        if inner.used() > 16 && inner.len < inner.used() / 2 {
            dict_rebuild(d, 0);
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
        for i in (0..inner.used()).filter(|&i| inner.live[i]) {
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
        (0..ia.used()).filter(|&i| ia.live[i]).all(|i| {
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
    // Apps never get here (their event loop ends the process); a program
    // with only `fn main` still runs its timers and background callbacks.
    ui::run_until_idle();
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
