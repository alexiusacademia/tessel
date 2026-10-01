//! JSON and binary (MessagePack) encoding, and the `Data` type.
//!
//! The compiler generates an encoder and a decoder for each type that is
//! saved or loaded (`toJson`, `fromJson`, `toBinary`, `fromBinary`). They build
//! or read a tree of values through the `tsl_tree_*` functions here; the tree
//! is then written as JSON or MessagePack, or parsed from them.
//!
//! The tree is Tessel's own (`Tree`), richer than JSON: it keeps bytes, NaN
//! and infinite floats, and non-text dictionary keys, which MessagePack can
//! store as they are. Writing JSON turns them into what JSON has: base64
//! text, `null`, and keys as text.
//!
//! Tree handles are `*mut Tree`. Builders return owned handles, which
//! `tsl_tree_push`/`tsl_tree_set`/`tsl_tree_to_*` take over; readers return
//! borrowed handles into a tree the caller frees with `tsl_tree_free`.
//!
//! `Data` values are `Str` objects holding arbitrary bytes (not UTF-8).

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

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
    let path = unsafe { str_text(path) };
    match std::fs::read(path) {
        Ok(bytes) => data_new(&bytes),
        Err(e) => {
            crate::file_error("read", path, &e);
            std::ptr::null_mut()
        }
    }
}

/// `writeData(path:data:)`: whether it worked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_write_data(path: *const Str, data: *const Str) -> bool {
    let path = unsafe { str_text(path) };
    std::fs::write(path, unsafe { data_bytes(data) }).map_err(|e| crate::file_error("write", path, &e)).is_ok()
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
pub(crate) static INT_TYPE: TslType = TslType {
    size: 8,
    align: 8,
    retain: None,
    release: None,
    eq: Some(int_eq),
    hash: Some(int_hash),
    cmp: Some(int_cmp),
    kind: 1,
};

// ===== the tree =====

pub enum Tree {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Tree>),
    /// Keys in order; struct fields and enum cases have `Str` keys.
    Map(Vec<(Tree, Tree)>),
}

impl Tree {
    /// A key as text: text as it is, numbers and booleans as they print.
    fn key_text(&self) -> String {
        match self {
            Tree::Str(s) => s.clone(),
            Tree::Int(n) => n.to_string(),
            Tree::Float(x) => crate::format_float(*x),
            Tree::Bool(b) => b.to_string(),
            Tree::Bytes(b) => base64_encode(b),
            Tree::Null => "null".to_string(),
            Tree::Array(_) | Tree::Map(_) => String::new(),
        }
    }

    /// As JSON: bytes as base64 text, NaN and infinity as null, keys as text.
    fn to_json(&self) -> serde_json::Value {
        use serde_json::Value;
        match self {
            Tree::Null => Value::Null,
            Tree::Bool(b) => Value::Bool(*b),
            Tree::Int(n) => Value::Number((*n).into()),
            Tree::Float(x) => serde_json::Number::from_f64(*x).map_or(Value::Null, Value::Number),
            Tree::Str(s) => Value::String(s.clone()),
            Tree::Bytes(b) => Value::String(base64_encode(b)),
            Tree::Array(a) => Value::Array(a.iter().map(Tree::to_json).collect()),
            Tree::Map(m) => Value::Object(m.iter().map(|(k, v)| (k.key_text(), v.to_json())).collect()),
        }
    }
}

/// MessagePack keeps everything: bytes as `bin`, floats as they are, and
/// keys of any kind.
impl Serialize for Tree {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Tree::Null => s.serialize_unit(),
            Tree::Bool(b) => s.serialize_bool(*b),
            Tree::Int(n) => s.serialize_i64(*n),
            Tree::Float(x) => s.serialize_f64(*x),
            Tree::Str(t) => s.serialize_str(t),
            Tree::Bytes(b) => s.serialize_bytes(b),
            Tree::Array(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for item in a {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Tree::Map(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

/// Parsing JSON or MessagePack.
impl<'de> Deserialize<'de> for Tree {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Tree, D::Error> {
        d.deserialize_any(TreeVisitor)
    }
}

struct TreeVisitor;

impl<'de> Visitor<'de> for TreeVisitor {
    type Value = Tree;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a value")
    }
    fn visit_unit<E>(self) -> Result<Tree, E> {
        Ok(Tree::Null)
    }
    fn visit_none<E>(self) -> Result<Tree, E> {
        Ok(Tree::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Tree, D::Error> {
        Tree::deserialize(d)
    }
    fn visit_bool<E>(self, b: bool) -> Result<Tree, E> {
        Ok(Tree::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Tree, E> {
        Ok(Tree::Int(n))
    }
    fn visit_u64<E>(self, n: u64) -> Result<Tree, E> {
        Ok(i64::try_from(n).map_or(Tree::Float(n as f64), Tree::Int))
    }
    fn visit_f64<E>(self, x: f64) -> Result<Tree, E> {
        Ok(Tree::Float(x))
    }
    fn visit_str<E>(self, s: &str) -> Result<Tree, E> {
        Ok(Tree::Str(s.to_string()))
    }
    fn visit_string<E>(self, s: String) -> Result<Tree, E> {
        Ok(Tree::Str(s))
    }
    fn visit_bytes<E>(self, b: &[u8]) -> Result<Tree, E> {
        Ok(Tree::Bytes(b.to_vec()))
    }
    fn visit_byte_buf<E>(self, b: Vec<u8>) -> Result<Tree, E> {
        Ok(Tree::Bytes(b))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Tree, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Tree::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Tree, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry()? {
            entries.push(entry);
        }
        Ok(Tree::Map(entries))
    }
}

// ===== building a tree =====

fn owned(v: Tree) -> *mut Tree {
    Box::into_raw(Box::new(v))
}

unsafe fn take(h: *mut Tree) -> Tree {
    *unsafe { Box::from_raw(h) }
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_null() -> *mut Tree {
    owned(Tree::Null)
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_bool(b: u8) -> *mut Tree {
    owned(Tree::Bool(b != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_int(n: i64) -> *mut Tree {
    owned(Tree::Int(n))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_float(x: f64) -> *mut Tree {
    owned(Tree::Float(x))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_str(s: *const Str) -> *mut Tree {
    owned(Tree::Str(unsafe { str_text(s) }.to_string()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_data(d: *const Str) -> *mut Tree {
    owned(Tree::Bytes(unsafe { data_bytes(d) }.to_vec()))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_array() -> *mut Tree {
    owned(Tree::Array(Vec::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_tree_object() -> *mut Tree {
    owned(Tree::Map(Vec::new()))
}

/// Appends `item` (taken over) to the array `arr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_push(arr: *mut Tree, item: *mut Tree) {
    let item = unsafe { take(item) };
    if let Tree::Array(a) = unsafe { &mut *arr } {
        a.push(item);
    }
}

/// Sets the text `key` to `item` (taken over) in the object `obj`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_set(obj: *mut Tree, key: *const Str, item: *mut Tree) {
    let key = owned(Tree::Str(unsafe { str_text(key) }.to_string()));
    unsafe { tsl_tree_set_key(obj, key, item) };
}

/// Sets `key` to `item` (both taken over) in the object `obj`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_set_key(obj: *mut Tree, key: *mut Tree, item: *mut Tree) {
    let (key, item) = unsafe { (take(key), take(item)) };
    if let Tree::Map(m) = unsafe { &mut *obj } {
        m.push((key, item));
    }
}

/// Writes the tree (taken over) as JSON text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_to_json(h: *mut Tree, pretty: u8) -> *mut Str {
    let v = unsafe { take(h) }.to_json();
    let text = if pretty != 0 { serde_json::to_string_pretty(&v) } else { serde_json::to_string(&v) };
    crate::str_new(&text.unwrap_or_default())
}

/// Writes the tree (taken over) as MessagePack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_to_binary(h: *mut Tree) -> *mut Str {
    let v = unsafe { take(h) };
    data_new(&rmp_serde::to_vec(&v).unwrap_or_default())
}

// ===== reading a tree =====

/// Parses JSON text; null if it isn't valid JSON. Free with `tsl_tree_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_json_parse(s: *const Str) -> *mut Tree {
    match serde_json::from_str::<Tree>(unsafe { str_text(s) }) {
        Ok(v) => owned(v),
        Err(e) => {
            crate::set_error(format!("this isn't valid JSON: {e}"));
            std::ptr::null_mut()
        }
    }
}

/// Parses MessagePack; null if it isn't valid. Free with `tsl_tree_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_binary_parse(d: *const Str) -> *mut Tree {
    match rmp_serde::from_slice::<Tree>(unsafe { data_bytes(d) }) {
        Ok(v) => owned(v),
        Err(e) => {
            crate::set_error(format!("this isn't valid binary data: {e}"));
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_free(h: *mut Tree) {
    if !h.is_null() {
        drop(unsafe { take(h) });
    }
}

/// 0 null, 1 bool, 2 number, 3 string, 4 array, 5 object, 6 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_kind(h: *const Tree) -> i32 {
    match unsafe { &*h } {
        Tree::Null => 0,
        Tree::Bool(_) => 1,
        Tree::Int(_) | Tree::Float(_) => 2,
        Tree::Str(_) => 3,
        Tree::Array(_) => 4,
        Tree::Map(_) => 5,
        Tree::Bytes(_) => 6,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_bool(h: *const Tree, out: *mut u8) -> u8 {
    match unsafe { &*h } {
        Tree::Bool(b) => {
            unsafe { *out = *b as u8 };
            1
        }
        _ => 0,
    }
}

/// An Int: a whole number (`3` or `3.0`) that fits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_int(h: *const Tree, out: *mut i64) -> u8 {
    let v = match unsafe { &*h } {
        Tree::Int(n) => Some(*n),
        Tree::Float(f) if f.fract() == 0.0 && f.abs() < 9.2e18 => Some(*f as i64),
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

/// A Float: any number. In JSON, NaN and infinity are written as null, so a
/// null reads as NaN (but as nil for a `Float?`, which the decoder checks first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_float(h: *const Tree, out: *mut f64) -> u8 {
    let v = match unsafe { &*h } {
        Tree::Int(n) => Some(*n as f64),
        Tree::Float(x) => Some(*x),
        Tree::Null => Some(f64::NAN),
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
pub unsafe extern "C" fn tsl_tree_as_str(h: *const Tree) -> *mut Str {
    match unsafe { &*h } {
        Tree::Str(s) => crate::str_new(s),
        _ => std::ptr::null_mut(),
    }
}

/// New Data from bytes (binary) or a base64 string (JSON), or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_as_data(h: *const Tree) -> *mut Str {
    match unsafe { &*h } {
        Tree::Bytes(b) => data_new(b),
        Tree::Str(s) => base64_decode(s).map_or(std::ptr::null_mut(), |b| data_new(&b)),
        _ => std::ptr::null_mut(),
    }
}

/// The number of items of an array or object; -1 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_len(h: *const Tree) -> i64 {
    match unsafe { &*h } {
        Tree::Array(a) => a.len() as i64,
        Tree::Map(m) => m.len() as i64,
        _ => -1,
    }
}

/// Item `i` of an array, or value `i` of an object (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_at(h: *const Tree, i: i64) -> *const Tree {
    match unsafe { &*h } {
        Tree::Array(a) => a.get(i as usize).map_or(std::ptr::null(), |v| v as *const Tree),
        Tree::Map(m) => m.get(i as usize).map_or(std::ptr::null(), |(_, v)| v as *const Tree),
        _ => std::ptr::null(),
    }
}

/// Key `i` of an object, as a new String (null if not an object). Keys that
/// aren't text come as they print, so the decoder parses all keys the same way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_key_at(h: *const Tree, i: i64) -> *mut Str {
    match unsafe { &*h } {
        Tree::Map(m) => m.get(i as usize).map_or(std::ptr::null_mut(), |(k, _)| crate::str_new(&k.key_text())),
        _ => std::ptr::null_mut(),
    }
}

/// The value of the text `key` in an object (borrowed), or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_tree_field(h: *const Tree, key: *const Str) -> *const Tree {
    let key = unsafe { str_text(key) };
    match unsafe { &*h } {
        Tree::Map(m) => m
            .iter()
            .find(|(k, _)| matches!(k, Tree::Str(s) if s == key))
            .map_or(std::ptr::null(), |(_, v)| v as *const Tree),
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
