//! Standard library functions that the compiler calls directly (see the
//! `RtCall` table in the compiler's builtins.rs): strings, numbers, dates,
//! files and paths, and the system.
//!
//! Conventions: strings, data and lists are borrowed; results that are
//! objects are new references. An optional object is null for nil; an
//! optional number comes back through `out`, with 1 if present.

use std::ptr;

use crate::codec::{base64_decode, base64_encode, data_bytes, data_new};
use crate::{List, Str, str_list, str_new, str_text};

unsafe fn text<'a>(s: *const Str) -> &'a str {
    unsafe { str_text(s) }
}

fn opt_str(s: Option<String>) -> *mut Str {
    s.map_or(ptr::null_mut(), |s| str_new(&s))
}

unsafe fn put<T>(out: *mut T, v: Option<T>) -> u8 {
    match v {
        Some(v) => {
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

// ===== strings =====

/// `text.characters`: each character (grapheme-ish: each Unicode scalar).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_characters(s: *const Str) -> *mut List {
    let t = unsafe { text(s) };
    let chars: Vec<String> = t.chars().map(String::from).collect();
    unsafe { str_list(chars.iter().map(String::as_str)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_reversed(s: *const Str) -> *mut Str {
    str_new(&unsafe { text(s) }.chars().rev().collect::<String>())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_repeated(s: *const Str, times: i64) -> *mut Str {
    str_new(&unsafe { text(s) }.repeat(times.max(0) as usize))
}

/// `padStart(length, with:)`: adds `pad` before the text until it has `length` characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_pad_start(s: *const Str, length: i64, pad: *const Str) -> *mut Str {
    let (t, pad) = unsafe { (text(s), text(pad)) };
    str_new(&(padding(t, length, pad) + t))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_pad_end(s: *const Str, length: i64, pad: *const Str) -> *mut Str {
    let (t, pad) = unsafe { (text(s), text(pad)) };
    str_new(&(t.to_string() + &padding(t, length, pad)))
}

fn padding(t: &str, length: i64, pad: &str) -> String {
    let missing = (length.max(0) as usize).saturating_sub(t.chars().count());
    if pad.is_empty() {
        return String::new();
    }
    pad.chars().cycle().take(missing).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_trim_start(s: *const Str) -> *mut Str {
    str_new(unsafe { text(s) }.trim_start())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_trim_end(s: *const Str) -> *mut Str {
    str_new(unsafe { text(s) }.trim_end())
}

/// `findLast(text)`: the character index of the last occurrence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_find_last(s: *const Str, needle: *const Str, out: *mut i64) -> u8 {
    let (t, n) = unsafe { (text(s), text(needle)) };
    let found = t.rfind(n).map(|byte| t[..byte].chars().count() as i64);
    unsafe { put(out, found) }
}

/// `capitalized()`: the first letter of each word in upper case.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_capitalized(s: *const Str) -> *mut Str {
    let mut out = String::new();
    let mut start = true;
    for c in unsafe { text(s) }.chars() {
        if start && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.push(c);
            start = c.is_whitespace();
        }
    }
    str_new(&out)
}

/// `character(at:)`: the character at an index, or nil.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_character_at(s: *const Str, i: i64) -> *mut Str {
    if i < 0 {
        return ptr::null_mut();
    }
    opt_str(unsafe { text(s) }.chars().nth(i as usize).map(String::from))
}

/// `words()`: the text split at runs of spaces, tabs and new lines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_words(s: *const Str) -> *mut List {
    unsafe { str_list(text(s).split_whitespace()) }
}

/// `occurrences(of:)`: how many times `needle` appears (not overlapping).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_occurrences(s: *const Str, needle: *const Str) -> i64 {
    let (t, n) = unsafe { (text(s), text(needle)) };
    if n.is_empty() { 0 } else { t.matches(n).count() as i64 }
}

/// `urlEncoded`: percent-encoding for a URL part (a query value, a path segment).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_url_encoded(s: *const Str) -> *mut Str {
    let mut out = String::new();
    for b in unsafe { text(s) }.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    str_new(&out)
}

/// `urlDecoded`: undoes percent-encoding (and `+` as a space); nil if it isn't valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_url_decoded(s: *const Str) -> *mut Str {
    let bytes = unsafe { text(s) }.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let Some(hex) = bytes.get(i + 1..i + 3) else { return ptr::null_mut() };
                let Ok(v) = u8::from_str_radix(std::str::from_utf8(hex).unwrap_or("x"), 16) else { return ptr::null_mut() };
                out.push(v);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    opt_str(String::from_utf8(out).ok())
}

// ===== paths =====

fn path_parts(p: &str) -> (&str, &str) {
    let cut = p.rfind(['/', '\\']);
    match cut {
        Some(i) => (&p[..i], &p[i + 1..]),
        None => ("", p),
    }
}

/// `path.fileName`: the last part of a path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_path_file_name(s: *const Str) -> *mut Str {
    str_new(path_parts(unsafe { text(s) }).1)
}

/// `path.fileExtension`: after the last dot of the file name, without the dot ("" if none).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_path_extension(s: *const Str) -> *mut Str {
    let name = path_parts(unsafe { text(s) }).1;
    str_new(match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    })
}

/// `path.folder`: the folder a path is in ("" if it has none).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_path_folder(s: *const Str) -> *mut Str {
    str_new(path_parts(unsafe { text(s) }).0)
}

/// `joinPath(folder, name)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_join_path(folder: *const Str, name: *const Str) -> *mut Str {
    let (f, n) = unsafe { (text(folder), text(name)) };
    if f.is_empty() {
        return str_new(n);
    }
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let f = f.trim_end_matches(['/', '\\']);
    str_new(&format!("{f}{sep}{}", n.trim_start_matches(['/', '\\'])))
}

// ===== numbers =====

#[unsafe(no_mangle)]
pub extern "C" fn tsl_int_is_even(n: i64) -> u8 {
    (n % 2 == 0) as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_int_is_odd(n: i64) -> u8 {
    (n % 2 != 0) as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_int_clamped(n: i64, lo: i64, hi: i64) -> i64 {
    n.max(lo).min(hi)
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_float_is_nan(x: f64) -> u8 {
    x.is_nan() as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_float_is_finite(x: f64) -> u8 {
    x.is_finite() as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_float_clamped(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

// ===== random numbers =====

thread_local! {
    static RNG: std::cell::Cell<u64> = std::cell::Cell::new({
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        (t ^ 0x9E37_79B9_7F4A_7C15) | 1
    });
}

/// xorshift64*: fast, and plenty for apps (not for secrets).
pub(crate) fn random_u64() -> u64 {
    RNG.with(|r| {
        let mut x = r.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        r.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// `random(lo..hi)`: an Int from `lo` up to, not including, `hi`; stops
/// the program if the range is empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_random_int(lo: i64, hi: i64, loc: *const crate::Loc) -> i64 {
    if hi <= lo {
        let msg = format!("can't pick a random number from the empty range {lo}..{hi}");
        unsafe { crate::panic_at(loc, &msg) };
    }
    let span = (hi as i128 - lo as i128) as u128;
    (lo as i128 + (random_u64() as u128 % span) as i128) as i64
}

/// `randomFloat()`: from 0 up to, not including, 1.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_random_float() -> f64 {
    (random_u64() >> 11) as f64 / (1u64 << 53) as f64
}

// ===== dates and time =====

/// `now()`: seconds since 1 January 1970 (UTC), with fractions.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

// (Calendar dates and time zones are in `dates.rs`.)

// ===== files and folders =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_append_file(path: *const Str, s: *const Str) -> u8 {
    use std::io::Write;
    let path = unsafe { text(path) };
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    file.and_then(|mut f| f.write_all(unsafe { text(s) }.as_bytes())).map_err(|e| crate::file_error("write", path, &e)).is_ok() as u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_copy_file(from: *const Str, to: *const Str) -> u8 {
    let from = unsafe { text(from) };
    std::fs::copy(from, unsafe { text(to) }).map_err(|e| crate::file_error("copy", from, &e)).is_ok() as u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_file_size(path: *const Str, out: *mut i64) -> u8 {
    let v = std::fs::metadata(unsafe { text(path) }).ok().map(|m| m.len() as i64);
    unsafe { put(out, v) }
}

/// `modifiedTime(path)`: when the file last changed, in seconds since 1970.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_modified_time(path: *const Str, out: *mut f64) -> u8 {
    let v = std::fs::metadata(unsafe { text(path) })
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64());
    unsafe { put(out, v) }
}

fn home() -> String {
    std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_home_folder() -> *mut Str {
    str_new(&home())
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_documents_folder() -> *mut Str {
    str_new(&std::path::Path::new(&home()).join("Documents").to_string_lossy())
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_temp_folder() -> *mut Str {
    str_new(std::env::temp_dir().to_string_lossy().trim_end_matches(['/', '\\']))
}

/// `appDataFolder(name)`: a folder for the app's own files (settings, caches),
/// created if needed: ~/Library/Application Support/<name> on macOS,
/// %APPDATA%\<name> on Windows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_app_data_folder(name: *const Str) -> *mut Str {
    let base = if cfg!(windows) {
        std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| home()))
    } else if cfg!(target_os = "macos") {
        std::path::Path::new(&home()).join("Library").join("Application Support")
    } else {
        std::path::Path::new(&home()).join(".local").join("share")
    };
    let dir = base.join(unsafe { text(name) });
    let _ = std::fs::create_dir_all(&dir);
    str_new(&dir.to_string_lossy())
}

// ===== the system =====

/// `arguments()`: what the program was started with, after its own path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_arguments() -> *mut List {
    let args: Vec<String> = std::env::args().skip(1).collect();
    unsafe { str_list(args.iter().map(String::as_str)) }
}

/// `exit(code)`: ends the program now.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_exit(code: i64) {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(code as i32);
}

/// `platform()`: "macos" or "windows".
#[unsafe(no_mangle)]
pub extern "C" fn tsl_platform() -> *mut Str {
    str_new(if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    })
}

/// `openURL(url)`: opens a web page, a file or a folder with its usual app.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_open_url(url: *const Str) -> u8 {
    let target = unsafe { text(url) }.to_string();
    if std::env::var_os("TESSEL_SCRIPT").is_some() {
        // Headless runs (tests) don't open anything.
        println!("(open {target})");
        return 1;
    }
    let status = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", &target]).status()
    } else {
        std::process::Command::new("open").arg(&target).status()
    };
    status.is_ok_and(|s| s.success()) as u8
}

// ===== the clipboard =====

thread_local! {
    /// Headless runs use a clipboard of their own, never the user's.
    static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn headless() -> bool {
    std::env::var_os("TESSEL_SCRIPT").is_some()
}

pub(crate) fn headless_clipboard() -> Option<String> {
    TEST_CLIPBOARD.with(|c| c.borrow().clone())
}

pub(crate) fn set_headless_clipboard(text: &str) {
    TEST_CLIPBOARD.with(|c| *c.borrow_mut() = Some(text.to_string()));
}

/// `copyToClipboard(text)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_copy_to_clipboard(s: *const Str) -> u8 {
    let t = unsafe { text(s) }.to_string();
    if headless() {
        set_headless_clipboard(&t);
        return 1;
    }
    arboard::Clipboard::new().and_then(|mut c| c.set_text(t)).is_ok() as u8
}

// ===== standard input =====

/// `readLine()`: the next line of input without its line break, or nil at
/// the end of the input.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_read_line() -> *mut Str {
    use std::io::BufRead;
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => std::ptr::null_mut(),
        Ok(_) => {
            let trimmed = line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(&line);
            str_new(trimmed)
        }
    }
}

/// `readInput()`: everything left on standard input (for piped input).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_read_input() -> *mut Str {
    use std::io::Read;
    let mut text = String::new();
    let mut bytes = Vec::new();
    if std::io::stdin().lock().read_to_end(&mut bytes).is_ok() {
        text = String::from_utf8_lossy(&bytes).into_owned();
    }
    str_new(&text)
}

/// `clipboardText()`: nil if the clipboard has no text.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_clipboard_text() -> *mut Str {
    if headless() {
        return opt_str(headless_clipboard());
    }
    opt_str(arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok())
}

// ===== encodings =====

/// `data.base64`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_base64(d: *const Str) -> *mut Str {
    str_new(&base64_encode(unsafe { data_bytes(d) }))
}

/// `decodeBase64(text)`: nil if it isn't base64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_decode_base64(s: *const Str) -> *mut Str {
    match base64_decode(unsafe { text(s) }.trim()) {
        Some(b) => data_new(&b),
        None => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> *mut Str {
        str_new(t)
    }

    #[test]
    fn strings() {
        unsafe {
            assert_eq!(text(tsl_str_pad_start(s("7"), 3, s("0"))), "007");
            assert_eq!(text(tsl_str_pad_end(s("ab"), 5, s(".-"))), "ab.-.");
            assert_eq!(text(tsl_str_capitalized(s("hello big world"))), "Hello Big World");
            assert_eq!(text(tsl_str_url_encoded(s("a b&c=é"))), "a%20b%26c%3D%C3%A9");
            assert_eq!(text(tsl_str_url_decoded(s("a%20b+c%C3%A9"))), "a b cé");
            assert_eq!(text(tsl_path_extension(s("/a/b/file.tar.gz"))), "gz");
            assert_eq!(text(tsl_path_folder(s("/a/b/file.txt"))), "/a/b");
            let mut i = 0;
            assert_eq!(tsl_str_find_last(s("aébé"), s("é"), &mut i), 1);
            assert_eq!(i, 3);
        }
    }

    #[test]
    fn random_numbers_stay_in_range() {
        for _ in 0..1000 {
            let n = unsafe { tsl_random_int(-3, 4, std::ptr::null()) };
            assert!((-3..4).contains(&n));
            let f = tsl_random_float();
            assert!((0.0..1.0).contains(&f));
        }
    }
}
