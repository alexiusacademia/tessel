//! Networking: HTTP requests (`request`), downloads (`download`), giving
//! them up (`cancelRequest`), and WebSockets (`openWebSocket`,
//! `sendWebSocket`, `sendWebSocketData`, `closeWebSocket`).
//!
//! The work happens on background threads; results come back as events
//! (see `post`) and their blocks run on the UI thread, like `fetch`'s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{mpsc, Mutex};

use super::{Event, next_id, post};
use crate::ui::Callback;
use crate::{Header, Str, str_text};

/// A finished HTTP request, handed to the UI thread.
pub struct HttpResult {
    status: i64,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    error: String,
}

/// The response as the compiler's adapter receives it (all owned).
#[repr(C)]
struct ResponseC {
    status: i64,
    text: *mut Str,
    data: *mut Str,
    headers: *mut u8,
    error: *mut Str,
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

/// Requests and downloads that were cancelled while their threads were at
/// work: the threads stop when they see their id here (and take it out).
static CANCELLED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn cancelled(id: u64) -> bool {
    CANCELLED.lock().unwrap().contains(&id)
}

/// `cancelRequest(id)`: gives up a request or download. Its `done` block
/// isn't called (nor `progress`), and a download leaves no file. Nothing
/// happens if it has finished already.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_cancel_request(id: i64) {
    let id = id as u64;
    let going = REQUESTS.with(|r| r.borrow_mut().remove(&id).is_some()) | DOWNLOADS.with(|d| d.borrow_mut().remove(&id).is_some());
    if going {
        CANCELLED.lock().unwrap().push(id);
    }
}

/// A request's or download's thread is done: forget that it was cancelled.
fn finished(id: u64) {
    CANCELLED.lock().unwrap().retain(|c| *c != id);
}

thread_local! {
    /// `request` blocks waiting for their responses, by id.
    static REQUESTS: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
    /// `download` progress and done blocks, by id.
    static DOWNLOADS: RefCell<HashMap<u64, (Option<Callback>, Callback)>> = RefCell::new(HashMap::new());
    /// Open WebSockets: message and close blocks, and the way to send.
    static SOCKETS: RefCell<HashMap<u64, Socket>> = RefCell::new(HashMap::new());
}

/// Is any request, download or WebSocket still going?
pub fn busy() -> bool {
    working() || SOCKETS.with(|s| !s.borrow().is_empty())
}

/// Is any request or download still going? (Open WebSockets don't finish
/// by themselves, so a headless `wait` doesn't wait for them to close.)
pub fn working() -> bool {
    REQUESTS.with(|r| !r.borrow().is_empty()) || DOWNLOADS.with(|d| !d.borrow().is_empty())
}

fn agent() -> ureq::Agent {
    // 4xx and 5xx are answers, not failures: the app sees their status.
    ureq::Agent::config_builder().http_status_as_error(false).build().into()
}

// ===== requests =====

/// `request(url:method:headers:body:done:)`. `headers` is a `[String: String]`;
/// `done` is `adapter(box, *ResponseC)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_request(
    url: *const Str,
    method: *const Str,
    headers: *const crate::Dict,
    body: *const Str,
    f: *const std::ffi::c_void,
    env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let method = unsafe { str_text(method) }.to_uppercase();
    let body = unsafe { str_text(body) }.as_bytes().to_vec();
    let headers = unsafe { pairs(headers) };
    let id = next_id();
    REQUESTS.with(|r| r.borrow_mut().insert(id, Callback::new(f, env)));
    std::thread::spawn(move || {
        let result = run_request(id, &url, &method, &headers, body);
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
    HttpResult { status: 0, headers: Vec::new(), body: Vec::new(), error }
}

fn run_request(id: u64, url: &str, method: &str, headers: &[(String, String)], body: Vec<u8>) -> HttpResult {
    if let Some(path) = url.strip_prefix("file://") {
        return match std::fs::read(path) {
            Ok(b) => HttpResult { status: 200, headers: Vec::new(), body: b, error: String::new() },
            Err(e) => failed(e.to_string()),
        };
    }
    let mut builder = ureq::http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let request = match builder.body(body) {
        Ok(r) => r,
        Err(e) => return failed(format!("invalid request: {e}")),
    };
    match agent().run(request) {
        Ok(mut response) => {
            let status = response.status().as_u16() as i64;
            let headers = response
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            // In pieces, so a cancelled request stops reading.
            let mut reader = response.body_mut().with_config().limit(1 << 31).reader();
            let mut body = Vec::new();
            let mut piece = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut piece) {
                    Ok(0) => break HttpResult { status, headers, body, error: String::new() },
                    Ok(_) if cancelled(id) => break failed("cancelled".into()),
                    Ok(n) => body.extend_from_slice(&piece[..n]),
                    Err(e) => break HttpResult { status, headers, body: Vec::new(), error: e.to_string() },
                }
            }
        }
        Err(e) => failed(e.to_string()),
    }
}

fn deliver_response(id: u64, r: HttpResult) {
    let Some(cb) = REQUESTS.with(|m| m.borrow_mut().remove(&id)) else { return };
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
    let mut c = ResponseC {
        status: r.status,
        text: crate::str_new(&text),
        data: crate::codec::data_new(&r.body),
        headers: slot as *mut u8,
        error: crate::str_new(&r.error),
    };
    cb.call_with(&mut c as *mut ResponseC as *mut u8);
}

// ===== downloads =====

/// `download(url:to:progress:done:)`: `progress` may be null (no block);
/// `done` is `adapter(box, i64 ok)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_download(
    url: *const Str,
    to: *const Str,
    progress_f: *const std::ffi::c_void,
    progress_env: *mut Header,
    done_f: *const std::ffi::c_void,
    done_env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let to = unsafe { str_text(to) }.to_string();
    let id = next_id();
    let progress = (!progress_f.is_null()).then(|| Callback::new(progress_f, progress_env));
    DOWNLOADS.with(|d| d.borrow_mut().insert(id, (progress, Callback::new(done_f, done_env))));
    std::thread::spawn(move || {
        let ok = run_download(id, &url, &to);
        finished(id);
        post(Event::Downloaded(id, ok));
    });
    id as i64
}

fn run_download(id: u64, url: &str, to: &str) -> bool {
    // A local file (`file://`), or the body of a successful GET.
    let mut response;
    let (reader, total): (Box<dyn Read + '_>, Option<u64>) = if let Some(path) = url.strip_prefix("file://") {
        let Ok(file) = std::fs::File::open(path) else { return false };
        let total = file.metadata().ok().map(|m| m.len());
        (Box::new(file), total)
    } else {
        response = match agent().get(url).call() {
            Ok(r) if r.status().is_success() => r,
            _ => return false,
        };
        let total = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        (Box::new(response.body_mut().with_config().limit(u64::MAX).reader()), total)
    };
    // Write to a temporary name, so a failed download leaves no half file.
    let partial = format!("{to}.part");
    let ok = save(id, reader, total, &partial) && !cancelled(id) && crate::wrote(to, std::fs::rename(&partial, to).is_ok());
    if !ok {
        let _ = std::fs::remove_file(&partial);
    }
    ok
}

/// Copies `reader` to the file `path`, reporting progress when `total` is known.
fn save(id: u64, mut reader: Box<dyn Read + '_>, total: Option<u64>, path: &str) -> bool {
    use std::io::Write;
    let Ok(mut file) = std::fs::File::create(path) else { return false };
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
        Event::Progress(id, fraction) => {
            let cb = DOWNLOADS.with(|d| d.borrow().get(&id).and_then(|(p, _)| p.clone()));
            if let Some(cb) = cb {
                cb.call_with_float(fraction);
            }
        }
        Event::Downloaded(id, ok) => {
            if let Some((_, done)) = DOWNLOADS.with(|d| d.borrow_mut().remove(&id)) {
                done.call_with_int(ok as i64);
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
