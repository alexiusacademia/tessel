//! Networking: HTTP requests (`request`), downloads (`download`), giving
//! them up (`cancelRequest`), and WebSockets (`openWebSocket`,
//! `sendWebSocket`, `sendWebSocketData`, `closeWebSocket`).
//!
//! The work happens on background threads; results come back as events
//! (see `post`) and their blocks run on the UI thread, like `fetch`'s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Duration;

use super::{Event, next_id, post};
use crate::ui::Callback;
use crate::{Header, Str, str_text};

/// A finished HTTP request (or, for `onHeaders`, one whose body is still to
/// come), handed to the UI thread.
pub struct HttpResult {
    status: i64,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    error: String,
    /// The URL that answered, after any redirects.
    url: String,
}

/// The response as the compiler's adapter receives it (all owned).
#[repr(C)]
struct ResponseC {
    status: i64,
    text: *mut Str,
    data: *mut Str,
    headers: *mut u8,
    error: *mut Str,
    final_url: *mut Str,
}

enum Outgoing {
    Text(String),
    Bytes(Vec<u8>),
    Close,
}

/// An open WebSocket: its blocks, and the way to send.
struct Socket {
    on_message: Callback,
    /// For binary messages; without it, they arrive as text.
    on_data: Option<Callback>,
    on_close: Callback,
    send: mpsc::Sender<Outgoing>,
}

/// A `request` under way: its blocks.
struct Asked {
    on_headers: Option<Callback>,
    /// With it, the body comes in pieces as it arrives (a stream).
    on_data: Option<Callback>,
    done: Callback,
}

/// A `download` under way: its blocks, and the file it writes to.
struct Loading {
    on_headers: Option<Callback>,
    progress: Option<Callback>,
    done: Callback,
    part: String,
}

/// Requests and downloads that were cancelled while their threads were at
/// work: the threads stop when they see their id here (and take it out).
/// (Held while a download's .part file is made, renamed or removed, so that
/// cancelling and those don't cross.)
static CANCELLED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn cancelled(id: u64) -> bool {
    CANCELLED.lock().unwrap().contains(&id)
}

/// `cancelRequest(id)`: gives up a request or download. Its blocks aren't
/// called any more, and a download's .part file is removed right away.
/// Nothing happens if it has finished already.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_cancel_request(id: i64) {
    let id = id as u64;
    let asked = REQUESTS.with(|r| r.borrow_mut().remove(&id)).is_some();
    let loading = DOWNLOADS.with(|d| d.borrow_mut().remove(&id));
    if !asked && loading.is_none() {
        return;
    }
    {
        let mut cancelled = CANCELLED.lock().unwrap();
        cancelled.push(id);
        if let Some(loading) = &loading {
            let _ = std::fs::remove_file(&loading.part);
        }
    }
    forget_streamed(id);
}

/// A request's or download's thread is done: forget that it was cancelled.
fn finished(id: u64) {
    CANCELLED.lock().unwrap().retain(|c| *c != id);
}

thread_local! {
    /// `request` blocks waiting for their responses, by id.
    static REQUESTS: RefCell<HashMap<u64, Asked>> = RefCell::new(HashMap::new());
    /// `download` blocks, by id.
    static DOWNLOADS: RefCell<HashMap<u64, Loading>> = RefCell::new(HashMap::new());
    /// Open WebSockets: message and close blocks, and the way to send.
    static SOCKETS: RefCell<HashMap<u64, Socket>> = RefCell::new(HashMap::new());
}

/// Is any request, download, stream or WebSocket still going?
pub fn busy() -> bool {
    working() || REQUESTS.with(|r| !r.borrow().is_empty()) || SOCKETS.with(|s| !s.borrow().is_empty())
}

/// Is any request or download still going? (Streams, requests with `onData`,
/// may never end, like open WebSockets, so a headless `wait` doesn't wait
/// for them.)
pub fn working() -> bool {
    REQUESTS.with(|r| r.borrow().values().any(|a| a.on_data.is_none())) || DOWNLOADS.with(|d| !d.borrow().is_empty())
}

fn agent() -> ureq::Agent {
    // 4xx and 5xx are answers, not failures: the app sees their status.
    ureq::Agent::config_builder().http_status_as_error(false).build().into()
}

/// A block given as `adapter` and `box`, or none if `adapter` is null.
fn optional(f: *const std::ffi::c_void, env: *mut Header) -> Option<Callback> {
    (!f.is_null()).then(|| Callback::new(f, env))
}

// ===== streamed bodies =====

/// The most of a stream's body that waits for its `onData` block: past it,
/// reading pauses until the app has taken what's there.
const STREAM_LIMIT: usize = 1 << 20;

/// Pieces of streamed bodies waiting for their `onData` blocks, by id.
/// What arrives while some waits is added to it, so the app gets it in one.
static STREAMED: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());
/// Signalled when the app takes what waits (or gives a stream up).
static ROOM: Condvar = Condvar::new();

/// Hands a piece of a streamed body to the UI thread, waiting while a lot
/// is there already. False if the request was cancelled.
fn stream(id: u64, piece: &[u8]) -> bool {
    let mut streamed = STREAMED.lock().unwrap();
    loop {
        if cancelled(id) {
            return false;
        }
        let i = match streamed.iter().position(|(s, _)| *s == id) {
            Some(i) => i,
            None => {
                streamed.push((id, Vec::new()));
                streamed.len() - 1
            }
        };
        let waiting = &mut streamed[i].1;
        if waiting.len() < STREAM_LIMIT {
            let first = waiting.is_empty();
            waiting.extend_from_slice(piece);
            drop(streamed);
            // (One event for what waits: more pieces join it.)
            if first {
                post(Event::Chunk(id));
            }
            return true;
        }
        streamed = ROOM.wait_timeout(streamed, Duration::from_millis(50)).unwrap().0;
    }
}

/// What of a stream's body waits for the app (taking it).
fn take_streamed(id: u64) -> Vec<u8> {
    let mut streamed = STREAMED.lock().unwrap();
    let out = streamed.iter_mut().find(|(s, _)| *s == id).map(|(_, b)| std::mem::take(b)).unwrap_or_default();
    ROOM.notify_all();
    out
}

fn forget_streamed(id: u64) {
    STREAMED.lock().unwrap().retain(|(s, _)| *s != id);
    ROOM.notify_all();
}

/// Gives `onData` what of the stream `id` has arrived.
fn deliver_streamed(id: u64) {
    let Some(on_data) = REQUESTS.with(|r| r.borrow().get(&id).and_then(|a| a.on_data.clone())) else {
        forget_streamed(id);
        return;
    };
    let bytes = take_streamed(id);
    if !bytes.is_empty() {
        on_data.call_with(crate::codec::data_new(&bytes) as *mut u8);
    }
}

// ===== requests =====

/// `request(url:method:headers:body:onHeaders:onData:done:)`. `headers` is
/// a `[String: String]`; `onHeaders` and `done` are `adapter(box,
/// *ResponseC)` (`onHeaders`' adapter null for none); `onData` is a
/// `fn(Data)` closure, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_request(
    url: *const Str,
    method: *const Str,
    headers: *const crate::Dict,
    body: *const Str,
    headers_f: *const std::ffi::c_void,
    headers_env: *mut Header,
    data_f: *const std::ffi::c_void,
    data_env: *mut Header,
    f: *const std::ffi::c_void,
    env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let method = unsafe { str_text(method) }.to_uppercase();
    let body = unsafe { str_text(body) }.as_bytes().to_vec();
    let headers = unsafe { pairs(headers) };
    let id = next_id();
    let asked = Asked {
        on_headers: optional(headers_f, headers_env),
        on_data: optional(data_f, data_env),
        done: Callback::new(f, env),
    };
    let (tell_headers, streaming) = (asked.on_headers.is_some(), asked.on_data.is_some());
    REQUESTS.with(|r| r.borrow_mut().insert(id, asked));
    std::thread::spawn(move || {
        let result = run_request(id, &url, &method, &headers, body, tell_headers, streaming);
        finished(id);
        post(Event::Responded(id, result));
    });
    id as i64
}

unsafe fn pairs(dict: *const crate::Dict) -> Vec<(String, String)> {
    unsafe {
        let (keys, values) = (crate::tsl_dict_keys(dict), crate::tsl_dict_values(dict));
        let get = |list: *const crate::List, i: i64| {
            let p = crate::tsl_list_get(list, i, std::ptr::null()) as *const *const Str;
            str_text(*p).to_string()
        };
        let out = (0..crate::tsl_list_len(keys)).map(|i| (get(keys, i), get(values, i))).collect();
        crate::tsl_release(keys as *mut Header);
        crate::tsl_release(values as *mut Header);
        out
    }
}

fn failed(error: String) -> HttpResult {
    HttpResult { status: 0, headers: Vec::new(), body: Vec::new(), error, url: String::new() }
}

/// An answer before its body: status, headers and final URL.
struct Answer<'a> {
    status: i64,
    headers: Vec<(String, String)>,
    url: String,
    body: Box<dyn Read + 'a>,
    /// The body's size, if known.
    size: Option<u64>,
}

impl Answer<'_> {
    fn head(&self) -> HttpResult {
        HttpResult { status: self.status, headers: self.headers.clone(), body: Vec::new(), error: String::new(), url: self.url.clone() }
    }
}

/// Sends a request (or opens a `file://` URL), up to the start of the body.
fn ask<'a>(response: &'a mut Option<ureq::http::Response<ureq::Body>>, url: &str, method: &str, headers: &[(String, String)], body: Vec<u8>) -> Result<Answer<'a>, String> {
    use ureq::ResponseExt;
    if let Some(path) = url.strip_prefix("file://") {
        return read_file(path, url, headers);
    }
    let mut builder = ureq::http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let request = builder.body(body).map_err(|e| format!("invalid request: {e}"))?;
    let r = response.insert(agent().run(request).map_err(|e| e.to_string())?);
    let status = r.status().as_u16() as i64;
    let headers = r.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect();
    let url = r.get_uri().to_string();
    let size = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    Ok(Answer { status, headers, url, body: Box::new(r.body_mut().with_config().limit(u64::MAX).reader()), size })
}

fn run_request(id: u64, url: &str, method: &str, headers: &[(String, String)], body: Vec<u8>, tell_headers: bool, streaming: bool) -> HttpResult {
    let mut response = None;
    let mut answer = match ask(&mut response, url, method, headers, body) {
        Ok(a) => a,
        Err(e) => return failed(e),
    };
    if tell_headers {
        post(Event::Headers(id, answer.head()));
    }
    // In pieces, so a cancelled request stops reading.
    let mut result = answer.head();
    let mut piece = vec![0u8; 64 * 1024];
    loop {
        match answer.body.read(&mut piece) {
            Ok(0) => break result,
            Ok(_) if cancelled(id) => break failed("cancelled".into()),
            Ok(n) if streaming => {
                if !stream(id, &piece[..n]) {
                    break failed("cancelled".into());
                }
            }
            Ok(n) => {
                if result.body.len() as u64 + n as u64 > 1 << 31 {
                    break HttpResult { error: "the body is too big (over 2 GB)".into(), ..answer.head() };
                }
                result.body.extend_from_slice(&piece[..n]);
            }
            Err(e) => break HttpResult { error: e.to_string(), ..answer.head() },
        }
    }
}

/// A `file://` request, up to its body: the whole file (200), or the part a
/// `Range: bytes=a-b` header asks for (206), as an HTTP server would
/// answer. Only that part is read, as the body is read.
fn read_file(path: &str, url: &str, headers: &[(String, String)]) -> Result<Answer<'static>, String> {
    use std::io::{Seek, SeekFrom};
    let range = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("range")).map(|(_, v)| v.as_str());
    let (mut file, size) = match std::fs::File::open(path).and_then(|f| f.metadata().map(|m| (f, m))) {
        Ok((_, m)) if m.is_dir() => return Err(std::io::Error::from(std::io::ErrorKind::IsADirectory).to_string()),
        Ok((f, m)) => (f, m.len()),
        Err(e) => return Err(e.to_string()),
    };
    let header = |k: &str, v: String| (k.to_string(), v);
    let answer = |status, headers, body: Box<dyn Read>, size| Answer { status, headers, url: url.to_string(), body, size: Some(size) };
    let Some(range) = range.and_then(|r| byte_range(r, size)) else {
        let headers = vec![header("Accept-Ranges", "bytes".into()), header("Content-Length", size.to_string())];
        return Ok(answer(200, headers, Box::new(file), size));
    };
    let Some((start, end)) = range else {
        let headers = vec![header("Content-Range", format!("bytes */{size}")), header("Content-Length", "0".into())];
        return Ok(answer(416, headers, Box::new(std::io::empty()), 0));
    };
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let len = end - start + 1;
    let headers = vec![
        header("Accept-Ranges", "bytes".into()),
        header("Content-Range", format!("bytes {start}-{end}/{size}")),
        header("Content-Length", len.to_string()),
    ];
    Ok(answer(206, headers, Box::new(file.take(len)), len))
}

/// The bytes a `Range` header asks for in a file of `size` bytes: `None`
/// if the header is ignored (not one `bytes=` range), `Some(None)` if the
/// range starts past the end (416), else the first and last byte.
fn byte_range(header: &str, size: u64) -> Option<Option<(u64, u64)>> {
    let spec = header.trim().strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (a.trim(), b.trim());
    let parse = |s: &str| if s.bytes().all(|c| c.is_ascii_digit()) { s.parse::<u64>().ok() } else { None };
    let (start, end) = if a.is_empty() {
        // The last `n` bytes.
        let n = parse(b)?;
        if n == 0 || size == 0 {
            return Some(None);
        }
        (size.saturating_sub(n), size - 1)
    } else {
        let start = parse(a)?;
        let end = if b.is_empty() { u64::MAX } else { parse(b)? };
        if end < start {
            return None;
        }
        if start >= size {
            return Some(None);
        }
        (start, end.min(size - 1))
    };
    Some(Some((start, end)))
}

/// The `HttpResponse` for `r` (its parts owned by the receiver).
fn response(r: &HttpResult) -> ResponseC {
    let text = String::from_utf8_lossy(&r.body).into_owned();
    let dict = unsafe { crate::tsl_dict_new(&crate::TSL_TYPE_STR, &crate::TSL_TYPE_STR) };
    let mut slot = dict;
    for (k, v) in &r.headers {
        let (mut k, mut v) = (crate::str_new(&k.to_lowercase()), crate::str_new(v));
        unsafe {
            crate::tsl_dict_set(
                &mut slot,
                &mut k as *mut *mut Str as *mut u8,
                &mut v as *mut *mut Str as *mut u8,
            )
        };
    }
    ResponseC {
        status: r.status,
        text: crate::str_new(&text),
        data: crate::codec::data_new(&r.body),
        headers: slot as *mut u8,
        error: crate::str_new(&r.error),
        final_url: crate::str_new(&r.url),
    }
}

fn deliver_response(id: u64, r: HttpResult) {
    // What's left of a stream first.
    if REQUESTS.with(|m| m.borrow().get(&id).is_some_and(|a| a.on_data.is_some())) {
        deliver_streamed(id);
    }
    forget_streamed(id);
    let Some(asked) = REQUESTS.with(|m| m.borrow_mut().remove(&id)) else { return };
    let mut c = response(&r);
    asked.done.call_with(&mut c as *mut ResponseC as *mut u8);
}

/// Gives `onHeaders` (of a request or a download) its answer.
fn deliver_headers(id: u64, r: HttpResult) {
    let block = REQUESTS
        .with(|m| m.borrow().get(&id).and_then(|a| a.on_headers.clone()))
        .or_else(|| DOWNLOADS.with(|d| d.borrow().get(&id).and_then(|l| l.on_headers.clone())));
    if let Some(block) = block {
        let mut c = response(&r);
        block.call_with(&mut c as *mut ResponseC as *mut u8);
    }
}

// ===== downloads =====

/// `download(url:to:headers:onHeaders:progress:done:)`: `onHeaders` is
/// `adapter(box, *ResponseC)` and `progress` a closure, each null for none;
/// `done` is `adapter(box, i64 ok)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_download(
    url: *const Str,
    to: *const Str,
    headers: *const crate::Dict,
    headers_f: *const std::ffi::c_void,
    headers_env: *mut Header,
    progress_f: *const std::ffi::c_void,
    progress_env: *mut Header,
    done_f: *const std::ffi::c_void,
    done_env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let to = unsafe { str_text(to) }.to_string();
    let headers = unsafe { pairs(headers) };
    let id = next_id();
    // Write to a temporary name, so a failed download leaves no half file.
    let part = format!("{to}.part");
    let loading = Loading {
        on_headers: optional(headers_f, headers_env),
        progress: optional(progress_f, progress_env),
        done: Callback::new(done_f, done_env),
        part: part.clone(),
    };
    let tell_headers = loading.on_headers.is_some();
    DOWNLOADS.with(|d| d.borrow_mut().insert(id, loading));
    std::thread::spawn(move || {
        let ok = run_download(id, &url, &to, &part, &headers, tell_headers);
        finished(id);
        post(Event::Downloaded(id, ok));
    });
    id as i64
}

fn run_download(id: u64, url: &str, to: &str, part: &str, headers: &[(String, String)], tell_headers: bool) -> bool {
    // A local file (`file://`), or the body of a successful GET.
    let mut response = None;
    let Ok(answer) = ask(&mut response, url, "GET", headers, Vec::new()) else { return false };
    if tell_headers {
        post(Event::Headers(id, answer.head()));
    }
    if !(200..300).contains(&answer.status) {
        return false;
    }
    let ok = save(id, answer.body, answer.size, part) && {
        // (Not if it was cancelled meanwhile: then the .part is gone.)
        let cancelled = CANCELLED.lock().unwrap();
        !cancelled.contains(&id) && crate::wrote(to, std::fs::rename(part, to).is_ok())
    };
    if !ok {
        // (A cancelled download's .part was removed by `cancelRequest`, and
        // the name may be another download's by now.)
        let cancelled = CANCELLED.lock().unwrap();
        if !cancelled.contains(&id) {
            let _ = std::fs::remove_file(part);
        }
    }
    ok
}

/// Copies `reader` to the file `path`, reporting progress when `total` is known.
fn save(id: u64, mut reader: Box<dyn Read + '_>, total: Option<u64>, path: &str) -> bool {
    use std::io::Write;
    let file = {
        let cancelled = CANCELLED.lock().unwrap();
        if cancelled.contains(&id) {
            return false;
        }
        std::fs::File::create(path)
    };
    let Ok(mut file) = file else { return false };
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut reported = -1.0f64;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(_) if cancelled(id) => return false,
            Ok(n) => {
                if file.write_all(&buf[..n]).is_err() {
                    return false;
                }
                done += n as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let fraction = (done as f64 / total as f64).min(1.0);
                    if fraction - reported >= 0.01 {
                        reported = fraction;
                        post(Event::Progress(id, fraction));
                    }
                }
            }
            Err(_) => return false,
        }
    }
    if file.flush().is_err() {
        return false;
    }
    // Always end at 1.0, once.
    if reported < 1.0 {
        post(Event::Progress(id, 1.0));
    }
    true
}

// ===== WebSockets =====

/// `openWebSocket(url:onMessage:onData:onClose:)`: returns an id for sending
/// and closing. `data_f` is null when there's no block for binary messages.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_open_web_socket(
    url: *const Str,
    msg_f: *const std::ffi::c_void,
    msg_env: *mut Header,
    data_f: *const std::ffi::c_void,
    data_env: *mut Header,
    close_f: *const std::ffi::c_void,
    close_env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let id = next_id();
    let (tx, rx) = mpsc::channel();
    let socket = Socket {
        on_message: Callback::new(msg_f, msg_env),
        on_data: (!data_f.is_null()).then(|| Callback::new(data_f, data_env)),
        on_close: Callback::new(close_f, close_env),
        send: tx,
    };
    SOCKETS.with(|s| s.borrow_mut().insert(id, socket));
    std::thread::spawn(move || {
        let reason = run_socket(id, &url, rx);
        post(Event::Closed(id, reason));
    });
    id as i64
}

fn run_socket(id: u64, url: &str, rx: mpsc::Receiver<Outgoing>) -> String {
    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::{Error, Message};
    let mut socket = match tungstenite::connect(url) {
        Ok((s, _)) => s,
        Err(e) => return format!("couldn't connect: {e}"),
    };
    // Short reads, so messages to send go out promptly.
    let timeout = Some(std::time::Duration::from_millis(30));
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_read_timeout(timeout);
        }
        _ => {}
    }
    loop {
        loop {
            match rx.try_recv() {
                Ok(Outgoing::Text(t)) => {
                    if let Err(e) = socket.send(Message::text(t)) {
                        return format!("send failed: {e}");
                    }
                }
                Ok(Outgoing::Bytes(b)) => {
                    if let Err(e) = socket.send(Message::binary(b)) {
                        return format!("send failed: {e}");
                    }
                }
                Ok(Outgoing::Close) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return "closed".into();
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        match socket.read() {
            Ok(Message::Text(t)) => post(Event::Message(id, t.to_string())),
            Ok(Message::Binary(b)) => post(Event::Bytes(id, b.to_vec())),
            Ok(Message::Close(frame)) => {
                return frame.map_or("closed by the server".into(), |f| {
                    if f.reason.is_empty() { "closed by the server".into() } else { f.reason.to_string() }
                });
            }
            Ok(_) => {}
            Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(Error::ConnectionClosed) | Err(Error::AlreadyClosed) => return "closed".into(),
            Err(e) => return format!("connection lost: {e}"),
        }
    }
}

/// `sendWebSocket(id, text:)`: false if it isn't open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_web_socket(id: i64, text: *const Str) -> u8 {
    let text = unsafe { str_text(text) }.to_string();
    SOCKETS.with(|s| s.borrow().get(&(id as u64)).is_some_and(|socket| socket.send.send(Outgoing::Text(text)).is_ok())) as u8
}

/// `sendWebSocketData(id, data:)`: a binary message; false if it isn't open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_web_socket_data(id: i64, data: *const Str) -> u8 {
    let bytes = unsafe { crate::codec::data_bytes(data) }.to_vec();
    SOCKETS.with(|s| s.borrow().get(&(id as u64)).is_some_and(|socket| socket.send.send(Outgoing::Bytes(bytes)).is_ok())) as u8
}

/// `closeWebSocket(id)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_close_web_socket(id: i64) {
    SOCKETS.with(|s| {
        if let Some(socket) = s.borrow().get(&(id as u64)) {
            let _ = socket.send.send(Outgoing::Close);
        }
    });
}

// ===== delivering results on the UI thread =====

/// Runs the blocks for a networking event.
pub fn deliver(event: Event) {
    match event {
        Event::Responded(id, r) => deliver_response(id, r),
        Event::Headers(id, r) => deliver_headers(id, r),
        Event::Chunk(id) => deliver_streamed(id),
        Event::Progress(id, fraction) => {
            let cb = DOWNLOADS.with(|d| d.borrow().get(&id).and_then(|l| l.progress.clone()));
            if let Some(cb) = cb {
                cb.call_with_float(fraction);
            }
        }
        Event::Downloaded(id, ok) => {
            if let Some(loading) = DOWNLOADS.with(|d| d.borrow_mut().remove(&id)) {
                loading.done.call_with_int(ok as i64);
            }
        }
        Event::Message(id, text) => {
            let cb = SOCKETS.with(|s| s.borrow().get(&id).map(|socket| socket.on_message.clone()));
            if let Some(cb) = cb {
                cb.call_with(crate::str_new(&text) as *mut u8);
            }
        }
        Event::Bytes(id, bytes) => {
            // To the block for binary messages; without one, as text.
            let blocks = SOCKETS.with(|s| s.borrow().get(&id).map(|socket| (socket.on_data.clone(), socket.on_message.clone())));
            match blocks {
                Some((Some(on_data), _)) => on_data.call_with(crate::codec::data_new(&bytes) as *mut u8),
                Some((None, on_message)) => on_message.call_with(crate::str_new(&String::from_utf8_lossy(&bytes)) as *mut u8),
                None => {}
            }
        }
        Event::Closed(id, reason) => {
            if let Some(socket) = SOCKETS.with(|s| s.borrow_mut().remove(&id)) {
                socket.on_close.call_with(crate::str_new(&reason) as *mut u8);
            }
        }
        _ => {}
    }
}
