//! Databases (`openDatabase`, see the prelude): SQLite, built into Tessel
//! (its C source, compiled with the runtime), so they work the same on every
//! system; only programs that open a database carry it.
//!
//! A database is known to the program by a number. Values given for `?`s
//! come as the JSON of a `[SqlValue]`, and a query's rows go back as the
//! JSON of the prelude's `__DatabaseTable`: `SqlValue`s as Tessel writes
//! enums (`"null"`, `{"int":{"value":1}}`, …), `Data` in base64.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::Mutex;

use libsqlite3_sys as ffi;
use serde_json::{json, Value};

use crate::{set_error, str_new, str_text, List, Str};

type Db = *mut ffi::sqlite3;
type Stmt = *mut ffi::sqlite3_stmt;

/// The open databases, by number (their pointers, as numbers).
static OPEN: Mutex<Option<HashMap<i64, usize>>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn db_of(id: i64) -> Result<Db, String> {
    OPEN.lock().unwrap().as_ref().and_then(|m| m.get(&id)).map(|p| *p as Db).ok_or_else(|| "the database is closed".to_string())
}

unsafe fn message(db: Db) -> String {
    unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(db)) }.to_string_lossy().into_owned()
}

/// `openDatabase(path)`: the database's number, or -1 (and the reason).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_db_open(path: *const Str) -> i64 {
    let path = unsafe { str_text(path) }.to_string();
    let result = (|| -> Result<i64, String> {
        let c = std::ffi::CString::new(path.clone()).map_err(|_| "a database's path can't have a zero byte".to_string())?;
        let mut db: Db = std::ptr::null_mut();
        let flags = ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE | ffi::SQLITE_OPEN_FULLMUTEX;
        let rc = unsafe { ffi::sqlite3_open_v2(c.as_ptr(), &mut db, flags, std::ptr::null()) };
        if rc != ffi::SQLITE_OK {
            let why = if db.is_null() { format!("error {rc}") } else { unsafe { message(db) } };
            if !db.is_null() {
                unsafe { ffi::sqlite3_close(db) };
            }
            return Err(format!("can't open the database {path}: {why}"));
        }
        // Wait a little for another program that's writing to it.
        unsafe { ffi::sqlite3_busy_timeout(db, 2000) };
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        OPEN.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, db as usize);
        Ok(id)
    })();
    result.unwrap_or_else(|e| {
        set_error(e);
        -1
    })
}

/// `db.close()`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_db_close(id: i64) {
    let db = OPEN.lock().unwrap().as_mut().and_then(|m| m.remove(&id));
    if let Some(db) = db {
        unsafe { ffi::sqlite3_close(db as Db) };
    }
}

/// Binds `values` (a `[SqlValue]`'s JSON) to a statement's `?`s.
unsafe fn bind(stmt: Stmt, values: &[Value]) -> Result<(), String> {
    let wanted = unsafe { ffi::sqlite3_bind_parameter_count(stmt) } as usize;
    if wanted != values.len() {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let given = if values.len() == 1 { "1 was".to_string() } else { format!("{} were", values.len()) };
        return Err(format!("the SQL has {wanted} value{} to fill in (`?`), but {given} given", s(wanted)));
    }
    for (i, v) in values.iter().enumerate() {
        let at = i as c_int + 1;
        let field = |case: &str| v.get(case).and_then(|c| c.get("value"));
        let rc = unsafe {
            if let Some(n) = field("int").and_then(Value::as_i64) {
                ffi::sqlite3_bind_int64(stmt, at, n)
            } else if let Some(x) = field("float").and_then(Value::as_f64) {
                ffi::sqlite3_bind_double(stmt, at, x)
            } else if let Some(t) = field("text").and_then(Value::as_str) {
                ffi::sqlite3_bind_text(stmt, at, t.as_ptr() as *const c_char, t.len() as c_int, ffi::SQLITE_TRANSIENT())
            } else if let Some(b) = field("data").and_then(Value::as_str) {
                let bytes = crate::codec::base64_decode(b).unwrap_or_default();
                ffi::sqlite3_bind_blob(stmt, at, bytes.as_ptr() as *const c_void, bytes.len() as c_int, ffi::SQLITE_TRANSIENT())
            } else {
                ffi::sqlite3_bind_null(stmt, at)
            }
        };
        if rc != ffi::SQLITE_OK {
            return Err(format!("can't use value {}: error {rc}", i + 1));
        }
    }
    Ok(())
}

/// Runs each statement of `sql` (the values go to the only one, when
/// there are values), calling `row` for each row of the last.
unsafe fn run(id: i64, sql: &str, values: &str, row: &mut dyn FnMut(Stmt)) -> Result<(), String> {
    let db = db_of(id)?;
    let values: Vec<Value> = serde_json::from_str(values).unwrap_or_default();
    let c = std::ffi::CString::new(sql).map_err(|_| "SQL can't have a zero byte".to_string())?;
    let mut rest: *const c_char = c.as_ptr();
    let mut first = true;
    loop {
        let text = unsafe { CStr::from_ptr(rest) }.to_bytes();
        if text.iter().all(|b| b.is_ascii_whitespace() || *b == b';') {
            return Ok(());
        }
        let mut stmt: Stmt = std::ptr::null_mut();
        let mut tail: *const c_char = std::ptr::null();
        let rc = unsafe { ffi::sqlite3_prepare_v2(db, rest, -1, &mut stmt, &mut tail) };
        if rc != ffi::SQLITE_OK {
            return Err(unsafe { message(db) });
        }
        if stmt.is_null() {
            // (Only a comment was left.)
            return Ok(());
        }
        let bound = if first { unsafe { bind(stmt, &values) } } else { Ok(()) };
        if !first && !values.is_empty() {
            unsafe { ffi::sqlite3_finalize(stmt) };
            return Err("values (`?`) can only be given for SQL of one statement".into());
        }
        if let Err(e) = bound {
            unsafe { ffi::sqlite3_finalize(stmt) };
            return Err(e);
        }
        loop {
            match unsafe { ffi::sqlite3_step(stmt) } {
                ffi::SQLITE_ROW => row(stmt),
                ffi::SQLITE_DONE => break,
                _ => {
                    let e = unsafe { message(db) };
                    unsafe { ffi::sqlite3_finalize(stmt) };
                    return Err(e);
                }
            }
        }
        unsafe { ffi::sqlite3_finalize(stmt) };
        first = false;
        rest = tail;
    }
}

/// `db.execute(sql, values)`: how many rows it changed, or -1 (and the reason).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_db_execute(id: i64, sql: *const Str, values: *const Str) -> i64 {
    let (sql, values) = unsafe { (str_text(sql), str_text(values)) };
    match unsafe { run(id, sql, values, &mut |_| {}) } {
        Ok(()) => {
            db_of(id).map_or(0, |db| unsafe { ffi::sqlite3_changes(db) as i64 })
        }
        Err(e) => {
            set_error(e);
            -1
        }
    }
}

/// `db.query(sql, values)`: the rows, as the JSON of `__DatabaseTable`, or
/// null (and the reason).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_db_query(id: i64, sql: *const Str, values: *const Str) -> *mut Str {
    let (sql, values) = unsafe { (str_text(sql), str_text(values)) };
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    let result = unsafe {
        run(id, sql, values, &mut |stmt| {
            let n = ffi::sqlite3_column_count(stmt);
            if columns.is_empty() {
                columns = (0..n).map(|i| CStr::from_ptr(ffi::sqlite3_column_name(stmt, i)).to_string_lossy().into_owned()).collect();
            }
            let cells: Vec<Value> = (0..n)
                .map(|i| match ffi::sqlite3_column_type(stmt, i) {
                    ffi::SQLITE_INTEGER => json!({ "int": { "value": ffi::sqlite3_column_int64(stmt, i) } }),
                    ffi::SQLITE_FLOAT => json!({ "float": { "value": ffi::sqlite3_column_double(stmt, i) } }),
                    ffi::SQLITE_NULL => json!("null"),
                    ffi::SQLITE_BLOB => {
                        let len = ffi::sqlite3_column_bytes(stmt, i) as usize;
                        let p = ffi::sqlite3_column_blob(stmt, i) as *const u8;
                        let bytes = if p.is_null() { &[][..] } else { std::slice::from_raw_parts(p, len) };
                        json!({ "data": { "value": crate::codec::base64_encode(bytes) } })
                    }
                    _ => {
                        let p = ffi::sqlite3_column_text(stmt, i);
                        let len = ffi::sqlite3_column_bytes(stmt, i) as usize;
                        let bytes = if p.is_null() { &[][..] } else { std::slice::from_raw_parts(p, len) };
                        json!({ "text": { "value": String::from_utf8_lossy(bytes) } })
                    }
                })
                .collect();
            rows.push(Value::Array(cells));
        })
    };
    match result {
        Ok(()) => str_new(&json!({ "columns": columns, "rows": rows }).to_string()),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

/// `db.lastInsertId()`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_db_last_insert_id(id: i64) -> i64 {
    db_of(id).map_or(0, |db| unsafe { ffi::sqlite3_last_insert_rowid(db) })
}

/// A row asked for a column it doesn't have: the program stops.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_db_no_column(column: *const Str, columns: *const List) -> i64 {
    let column = unsafe { str_text(column) };
    let n = unsafe { crate::tsl_list_len(columns) };
    let names: Vec<String> = (0..n)
        .map(|i| unsafe { str_text(*(crate::tsl_list_get(columns, i, std::ptr::null()) as *const *const Str)) }.to_string())
        .collect();
    let msg = format!("the row has no column `{column}`; its columns are {}", names.join(", "));
    let msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe { crate::tsl_panic(msg.as_ptr(), std::ptr::null()) }
}
