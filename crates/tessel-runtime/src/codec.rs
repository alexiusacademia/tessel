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

/// `data[i] = byte`: sets the byte to `byte` modulo 256 (like
/// `Data(bytes:)`); stops the program if `i` is out of range. In place if
/// nothing else holds the `Data`, otherwise in a copy (the old value released).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_set(slot: *mut *mut Str, i: i64, byte: i64, loc: *const Loc) {
    unsafe {
        let d = *slot;
        let len = (*d).len;
        if i < 0 || i >= len {
            let msg = format!("index {i} is out of range for data with {len} bytes");
            crate::panic_at(loc, &msg);
        }
        let d = if crate::unique(d as *mut crate::Header) {
            d
        } else {
            let bytes = data_bytes(d);
            let copy = if is_growable(d) { growable_new(bytes) } else { data_new(bytes) };
            crate::tsl_release(d as *mut crate::Header);
            *slot = copy;
            copy
        };
        *(d as *mut u8).add(size_of::<Str>() + i as usize) = byte as u8;
    }
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

/// The room a growable `Data` of `len` bytes has: a power of two, at least
/// 16 bytes. (So appending to one copies its bytes O(log n) times.)
fn grow_capacity(len: usize) -> usize {
    len.next_power_of_two().max(16)
}

/// How many growable `Data` values have been freed. Counting them also keeps
/// `grow_drop`'s code unlike any other drop function's: `is_growable` tells
/// growable values apart by that function's address, and the compiler or
/// linker would make identical functions one (on Windows it does, and every
/// `Data` then looked growable, so appending wrote past its end).
static GROWN_FREED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn grow_drop(h: *mut crate::Header) {
    GROWN_FREED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    unsafe { crate::free(h as *mut u8) };
}

/// Is `d` a growable `Data` (one with room after its bytes, see
/// `grow_capacity`)? They're the ones whose drop function is `grow_drop`.
unsafe fn is_growable(d: *const Str) -> bool {
    let drop: unsafe extern "C" fn(*mut crate::Header) = grow_drop;
    unsafe { std::ptr::fn_addr_eq((*d).h.drop, drop) }
}

/// A new growable `Data` holding `bytes`.
fn growable_new(bytes: &[u8]) -> *mut Str {
    unsafe {
        let s = new_object(size_of::<Str>() + grow_capacity(bytes.len()), grow_drop) as *mut Str;
        (*s).len = bytes.len() as i64;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), (s as *mut u8).add(size_of::<Str>()), bytes.len());
        s
    }
}

/// `a + b` for `Data`: a new value with the bytes of both. It's growable,
/// since a sum is often added to again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_concat(a: *const Str, b: *const Str) -> *mut Str {
    let (a, b) = unsafe { (data_bytes(a), data_bytes(b)) };
    let s = growable_new(&[]);
    let mut slot = s;
    unsafe { append_bytes(&mut slot, a) };
    unsafe { append_bytes(&mut slot, b) };
    slot
}

/// Adds `bytes` to the end of the `Data` in `*slot`: in place if it's
/// growable, has room, and nothing else holds it; otherwise into a new
/// growable copy (released old value), which later appends then fill in place.
unsafe fn append_bytes(slot: *mut *mut Str, bytes: &[u8]) {
    unsafe {
        let d = *slot;
        let len = (*d).len as usize;
        let new_len = len + bytes.len();
        if is_growable(d) && crate::unique(d as *mut crate::Header) {
            let cap = grow_capacity(len);
            let d = if new_len > cap {
                let grown = crate::realloc(d as *mut u8, size_of::<Str>() + grow_capacity(new_len)) as *mut Str;
                if grown.is_null() {
                    crate::fatal("out of memory");
                }
                *slot = grown;
                grown
            } else {
                d
            };
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), (d as *mut u8).add(size_of::<Str>() + len), bytes.len());
            (*d).len = new_len as i64;
            return;
        }
        let mut all = Vec::with_capacity(new_len);
        all.extend_from_slice(data_bytes(d));
        all.extend_from_slice(bytes);
        *slot = growable_new(&all);
        crate::tsl_release(d as *mut crate::Header);
    }
}

/// `data += other` and `data.append(other)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_append(slot: *mut *mut Str, other: *const Str) {
    // (`data += data` copies the bytes first: they may move.)
    if std::ptr::eq(unsafe { *slot }, other) {
        let bytes = unsafe { data_bytes(other) }.to_vec();
        unsafe { append_bytes(slot, &bytes) };
    } else {
        unsafe { append_bytes(slot, data_bytes(other)) };
    }
}

/// `text += more`: a `String` is laid out as `Data` is, and its UTF-8
/// bytes join the same way, in place when there's room.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_str_append(slot: *mut *mut Str, other: *const Str) {
    unsafe { tsl_data_append(slot, other) };
}

/// `data.append(byte: n)`: one byte, `n` modulo 256 (like `Data(bytes:)`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_append_byte(slot: *mut *mut Str, byte: i64) {
    unsafe { append_bytes(slot, &[byte as u8]) };
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

/// `data.slice(from:to:)`: the bytes from index `from` up to `to`, both
/// brought into range (like a list's `slice`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_slice(d: *const Str, from: i64, to: i64) -> *mut Str {
    let bytes = unsafe { data_bytes(d) };
    let len = bytes.len() as i64;
    let from = from.clamp(0, len);
    let to = to.clamp(from, len);
    data_new(&bytes[from as usize..to as usize])
}

/// `data.latin1`: each byte as the character with that number (the text of
/// old files that aren't UTF-8), ending at the first zero byte if there is one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_data_latin1(d: *const Str) -> *mut Str {
    let bytes = unsafe { data_bytes(d) };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let text: String = bytes[..end].iter().map(|b| *b as char).collect();
    crate::str_new(&text)
}

/// The `N` bytes at `at`, in the order numbers are stored on this machine
/// (`big`: the file has the most significant byte first). Stops the program
/// if they aren't all there.
unsafe fn number_at<const N: usize>(d: *const Str, at: i64, big: u8, loc: *const Loc) -> [u8; N] {
    let bytes = unsafe { data_bytes(d) };
    if at < 0 || at as usize + N > bytes.len() {
        let msg = format!("bytes {at} to {} are out of range for data with {} bytes", at + N as i64 - 1, bytes.len());
        unsafe { crate::panic_at(loc, &msg) };
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[at as usize..at as usize + N]);
    if big != 0 {
        out.reverse();
    }
    out
}

/// `data.int16(at:bigEndian:)` and the others: a number stored in the bytes
/// starting at `at`.
macro_rules! data_number {
    ($name:ident, $t:ty, $out:ty) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(d: *const Str, at: i64, big: u8, loc: *const Loc) -> $out {
            <$t>::from_le_bytes(unsafe { number_at(d, at, big, loc) }) as $out
        }
    };
}

data_number!(tsl_data_int8, i8, i64);
data_number!(tsl_data_int16, i16, i64);
data_number!(tsl_data_uint16, u16, i64);
data_number!(tsl_data_int32, i32, i64);
data_number!(tsl_data_uint32, u32, i64);
data_number!(tsl_data_int64, i64, i64);
data_number!(tsl_data_float32, f32, f64);
data_number!(tsl_data_float64, f64, f64);

/// A list of Ints, one for each byte.
fn byte_list(bytes: &[u8]) -> *mut List {
    let mut slot = unsafe { crate::tsl_list_new(&INT_TYPE, bytes.len() as i64) };
    for &b in bytes {
        let v = b as i64;
        unsafe { crate::tsl_list_push(&mut slot, &v as *const i64 as *const u8) };
    }
    slot
}

/// `n.bytes(count:bigEndian:)`: the number as `count` bytes (1 to 8), the
/// least significant first unless `big`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_int_bytes(n: i64, count: i64, big: u8) -> *mut List {
    let count = count.clamp(1, 8) as usize;
    let mut bytes = n.to_le_bytes()[..count].to_vec();
    if big != 0 {
        bytes.reverse();
    }
    byte_list(&bytes)
}

/// `x.bytes(count:bigEndian:)`: the number as 8 bytes (or 4, with less
/// precision), as files store decimal numbers.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_float_bytes(x: f64, count: i64, big: u8) -> *mut List {
    let mut bytes = if count == 4 { (x as f32).to_le_bytes().to_vec() } else { x.to_le_bytes().to_vec() };
    if big != 0 {
        bytes.reverse();
    }
    byte_list(&bytes)
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
    crate::wrote(path, std::fs::write(path, unsafe { data_bytes(data) }).map_err(|e| crate::file_error("write", path, &e)).is_ok())
}

/// `appendData(path:data:)`: the bytes added to the end of a file (made if
/// it isn't there).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_append_data(path: *const Str, data: *const Str) -> bool {
    use std::io::Write;
    let path = unsafe { str_text(path) };
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    crate::wrote(path, file.and_then(|mut f| f.write_all(unsafe { data_bytes(data) })).map_err(|e| crate::file_error("write", path, &e)).is_ok())
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

unsafe extern "C" fn float_eq(a: *const u8, b: *const u8) -> bool {
    unsafe { *(a as *const f64) == *(b as *const f64) }
}

unsafe extern "C" fn float_hash(a: *const u8) -> u64 {
    // (0 and -0 are equal, so they hash the same.)
    let v = unsafe { *(a as *const f64) };
    if v == 0.0 { 0 } else { v.to_bits() }
}

unsafe extern "C" fn float_cmp(a: *const u8, b: *const u8) -> i32 {
    let (a, b) = unsafe { (*(a as *const f64), *(b as *const f64)) };
    a.partial_cmp(&b).map_or(0, |o| o as i32)
}

/// Element type of `[Float]`, for lists the runtime creates.
pub(crate) static FLOAT_TYPE: TslType = TslType {
    size: 8,
    align: 8,
    retain: None,
    release: None,
    eq: Some(float_eq),
    hash: Some(float_hash),
    cmp: Some(float_cmp),
    kind: 0,
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
