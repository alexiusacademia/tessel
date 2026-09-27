//! JSON and binary (MessagePack) encoding, and the `Data` type.
//!
//! The compiler generates an encoder and a decoder for each type that is
//! saved or loaded (`toJson`, `fromJson`, `toBinary`, `fromBinary`). They build
//! or read a tree of values through the `tsl_tree_*` functions here; the tree
//! is then written as JSON or MessagePack, or parsed from them.
//!
//! Tree handles are `*mut Value`. Builders return owned handles, which
//! `tsl_tree_push`/`tsl_tree_set`/`tsl_tree_to_*` take over; readers return
//! borrowed handles into a tree the caller frees with `tsl_tree_free`.
//!
//! `Data` values are `Str` objects holding arbitrary bytes (not UTF-8).

use serde_json::{Map, Number, Value};

use crate::{List, Loc, Str, TslType, new_object, str_text};

// ===== Data =====

pub(crate) fn data_new(bytes: &[u8]) -> *mut Str {
    unsafe {
        let s = new_object(size_of::<Str>() + bytes.len(), crate::str_drop_fn()) as *mut Str;
        (*s).len = bytes.len() as i64;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), (s as *mut u8).add(size_of::<Str>()), bytes.len());
        s
    }
}

pub(crate) unsafe fn data_bytes<'a>(s: *const Str) -> &'a [u8] {
    unsafe { std::slice::from_raw_parts((s as *const u8).add(size_of::<Str>()), (*s).len as usize) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_len(d: *const Str) -> i64 {
    unsafe { (*d).len }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_is_empty(d: *const Str) -> bool {
    unsafe { (*d).len == 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_eq(a: *const Str, b: *const Str) -> bool {
    unsafe { data_bytes(a) == data_bytes(b) }
}

/// `data[i]`: the byte as an Int (0–255); stops the program if out of range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_get(d: *const Str, i: i64, loc: *const Loc) -> i64 {
    let bytes = unsafe { data_bytes(d) };
    if i < 0 || i as usize >= bytes.len() {
        let msg = format!("index {i} is out of range for data with {} bytes", bytes.len());
        unsafe { crate::panic_at(loc, &msg) };
    }
    bytes[i as usize] as i64
}

/// `Data(bytes: [Int])`: each Int is taken modulo 256.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_from_ints(list: *const List) -> *mut Str {
    let n = unsafe { crate::tsl_list_len(list) };
    let bytes: Vec<u8> = (0..n)
        .map(|i| unsafe { *(crate::tsl_list_get(list, i, std::ptr::null()) as *const i64) as u8 })
        .collect();
    data_new(&bytes)
}

/// `Data()`: no bytes.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_data_empty() -> *mut Str {
    data_new(&[])
}

/// `Data(text: String)`: the text's UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_from_str(s: *const Str) -> *mut Str {
    data_new(unsafe { str_text(s) }.as_bytes())
}

/// `data.text`: the bytes as UTF-8 text, or null if they aren't valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_text(d: *const Str) -> *mut Str {
    match std::str::from_utf8(unsafe { data_bytes(d) }) {
        Ok(t) => crate::str_new(t),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `data.bytes`: the bytes as a list of Ints.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_ints(d: *const Str) -> *mut List {
    let bytes = unsafe { data_bytes(d) };
    let list = unsafe { crate::tsl_list_new(&INT_TYPE, bytes.len() as i64) };
    let mut slot = list;
    for &b in bytes {
        let v = b as i64;
        unsafe { crate::tsl_list_push(&mut slot, &v as *const i64 as *const u8) };
    }
    slot
}

/// `readData(path)`: the file's bytes, or null if it can't be read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_read_data(path: *const Str) -> *mut Str {
    match std::fs::read(unsafe { str_text(path) }) {
        Ok(bytes) => data_new(&bytes),
        Err(_) => std::ptr::null_mut(),
    }
}

/// `writeData(path:data:)`: whether it worked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_write_data(path: *const Str, data: *const Str) -> bool {
    std::fs::write(unsafe { str_text(path) }, unsafe { data_bytes(data) }).is_ok()
}

unsafe extern "C" fn int_eq(a: *const u8, b: *const u8) -> bool {
    unsafe { *(a as *const i64) == *(b as *const i64) }
}

unsafe extern "C" fn int_hash(a: *const u8) -> u64 {
    unsafe { *(a as *const i64) as u64 }
}

unsafe extern "C" fn int_cmp(a: *const u8, b: *const u8) -> i32 {
    unsafe { (*(a as *const i64)).cmp(&*(b as *const i64)) as i32 }
}

/// Element type of `[Int]`, for lists the runtime creates.
static INT_TYPE: TslType = TslType {
    size: 8,
    align: 8,
    retain: None,
    release: None,
    eq: Some(int_eq),
    hash: Some(int_hash),
    cmp: Some(int_cmp),
};

// ===== building a tree =====

fn owned(v: Value) -> *mut Value {
    Box::into_raw(Box::new(v))
}

unsafe fn take(h: *mut Value) -> Value {
    *unsafe { Box::from_raw(h) }
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_null() -> *mut Value {
    owned(Value::Null)
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_bool(b: u8) -> *mut Value {
    owned(Value::Bool(b != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_int(n: i64) -> *mut Value {
    owned(Value::Number(n.into()))
}

/// Infinity and NaN have no JSON form: they become null.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_float(x: f64) -> *mut Value {
    owned(Number::from_f64(x).map_or(Value::Null, Value::Number))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_str(s: *const Str) -> *mut Value {
    owned(Value::String(unsafe { str_text(s) }.to_string()))
}

/// Data inside JSON: a base64 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_data(d: *const Str) -> *mut Value {
    owned(Value::String(base64_encode(unsafe { data_bytes(d) })))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_array() -> *mut Value {
    owned(Value::Array(Vec::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_object() -> *mut Value {
    owned(Value::Object(Map::new()))
}

/// Appends `item` (taken over) to the array `arr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_push(arr: *mut Value, item: *mut Value) {
    let item = unsafe { take(item) };
    if let Value::Array(a) = unsafe { &mut *arr } {
        a.push(item);
    }
}

/// Sets `key` to `item` (taken over) in the object `obj`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_set(obj: *mut Value, key: *const Str, item: *mut Value) {
    let item = unsafe { take(item) };
    if let Value::Object(m) = unsafe { &mut *obj } {
        m.insert(unsafe { str_text(key) }.to_string(), item);
    }
}

/// Writes the tree (taken over) as JSON text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_to_json(h: *mut Value, pretty: u8) -> *mut Str {
    let v = unsafe { take(h) };
    let text = if pretty != 0 { serde_json::to_string_pretty(&v) } else { serde_json::to_string(&v) };
    crate::str_new(&text.unwrap_or_default())
}

/// Writes the tree (taken over) as MessagePack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_to_binary(h: *mut Value) -> *mut Str {
    let v = unsafe { take(h) };
    data_new(&rmp_serde::to_vec(&v).unwrap_or_default())
}

// ===== reading a tree =====

/// Parses JSON text; null if it isn't valid JSON. Free with `tsl_tree_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_json_parse(s: *const Str) -> *mut Value {
    match serde_json::from_str::<Value>(unsafe { str_text(s) }) {
        Ok(v) => owned(v),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Parses MessagePack; null if it isn't valid. Free with `tsl_tree_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_binary_parse(d: *const Str) -> *mut Value {
    match rmp_serde::from_slice::<Value>(unsafe { data_bytes(d) }) {
        Ok(v) => owned(v),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_free(h: *mut Value) {
    if !h.is_null() {
        drop(unsafe { take(h) });
    }
}

/// 0 null, 1 bool, 2 number, 3 string, 4 array, 5 object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_kind(h: *const Value) -> i32 {
    match unsafe { &*h } {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_bool(h: *const Value, out: *mut u8) -> u8 {
    match unsafe { &*h } {
        Value::Bool(b) => {
            unsafe { *out = *b as u8 };
            1
        }
        _ => 0,
    }
}

/// An Int: a whole number (`3` or `3.0`) that fits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_int(h: *const Value, out: *mut i64) -> u8 {
    let Value::Number(n) = (unsafe { &*h }) else { return 0 };
    let v = n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.2e18).map(|f| f as i64));
    match v {
        Some(v) => {
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

/// A Float: any number (null reads as NaN, since that's how NaN is written).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_float(h: *const Value, out: *mut f64) -> u8 {
    let v = match unsafe { &*h } {
        Value::Number(n) => n.as_f64(),
        Value::Null => Some(f64::NAN),
        _ => None,
    };
    match v {
        Some(v) => {
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

/// A new String, or null if it isn't one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_str(h: *const Value) -> *mut Str {
    match unsafe { &*h } {
        Value::String(s) => crate::str_new(s),
        _ => std::ptr::null_mut(),
    }
}

/// New Data from a base64 string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_data(h: *const Value) -> *mut Str {
    match unsafe { &*h } {
        Value::String(s) => base64_decode(s).map_or(std::ptr::null_mut(), |b| data_new(&b)),
        _ => std::ptr::null_mut(),
    }
}

/// The number of items of an array or object; -1 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_len(h: *const Value) -> i64 {
    match unsafe { &*h } {
        Value::Array(a) => a.len() as i64,
        Value::Object(m) => m.len() as i64,
        _ => -1,
    }
}

/// Item `i` of an array, or value `i` of an object (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_at(h: *const Value, i: i64) -> *const Value {
    match unsafe { &*h } {
        Value::Array(a) => a.get(i as usize).map_or(std::ptr::null(), |v| v as *const Value),
        Value::Object(m) => m.values().nth(i as usize).map_or(std::ptr::null(), |v| v as *const Value),
        _ => std::ptr::null(),
    }
}

/// Key `i` of an object, as a new String (null if not an object).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_key_at(h: *const Value, i: i64) -> *mut Str {
    match unsafe { &*h } {
        Value::Object(m) => m.keys().nth(i as usize).map_or(std::ptr::null_mut(), |k| crate::str_new(k)),
        _ => std::ptr::null_mut(),
    }
}

/// The value of `key` in an object (borrowed), or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_field(h: *const Value, key: *const Str) -> *const Value {
    match unsafe { &*h } {
        Value::Object(m) => m.get(unsafe { str_text(key) }).map_or(std::ptr::null(), |v| v as *const Value),
        _ => std::ptr::null(),
    }
}

// ===== base64 (for Data inside JSON) =====

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = B64.iter().position(|&x| x == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7]] {
            let e = base64_encode(bytes);
            assert_eq!(base64_decode(&e).unwrap(), bytes, "{e}");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }
}
