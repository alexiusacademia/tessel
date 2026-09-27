//! Networking: HTTP requests (`request`), downloads (`download`) and
//! WebSockets (`openWebSocket`, `sendWebSocket`, `closeWebSocket`).
//!
//! The work happens on background threads; results come back as events
//! (see `post`) and their blocks run on the UI thread, like `fetch`'s.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::mpsc;

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
    Close,
}

/// An open WebSocket: its message and close blocks, and the way to send.
struct Socket(Callback, Callback, mpsc::Sender<Outgoing>);

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
) {
    let url = unsafe { str_text(url) }.to_string();
    let method = unsafe { str_text(method) }.to_uppercase();
    let body = unsafe { str_text(body) }.as_bytes().to_vec();
    let headers = unsafe { pairs(headers) };
    let id = next_id();
    REQUESTS.with(|r| r.borrow_mut().insert(id, Callback::new(f, env)));
    std::thread::spawn(move || post(Event::Responded(id, run_request(&url, &method, &headers, body))));
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

fn run_request(url: &str, method: &str, headers: &[(String, String)], body: Vec<u8>) -> HttpResult {
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
            match response.body_mut().with_config().limit(1 << 31).read_to_vec() {
                Ok(body) => HttpResult { status, headers, body, error: String::new() },
                Err(e) => HttpResult { status, headers, body: Vec::new(), error: e.to_string() },
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
) {
    let url = unsafe { str_text(url) }.to_string();
    let to = unsafe { str_text(to) }.to_string();
    let id = next_id();
    let progress = (!progress_f.is_null()).then(|| Callback::new(progress_f, progress_env));
    DOWNLOADS.with(|d| d.borrow_mut().insert(id, (progress, Callback::new(done_f, done_env))));
    std::thread::spawn(move || {
        let ok = run_download(id, &url, &to);
        post(Event::Downloaded(id, ok));
    });
}

fn run_download(id: u64, url: &str, to: &str) -> bool {
    use std::io::Write;
    let mut response = match agent().get(url).call() {
        Ok(r) if r.status().is_success() => r,
        _ => return false,
    };
    let total: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    // Write to a temporary name, so a failed download leaves no half file.
    let partial = format!("{to}.part");
    let Ok(mut file) = std::fs::File::create(&partial) else { return false };
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut reported = -1.0f64;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if file.write_all(&buf[..n]).is_err() {
                    return false;
                }
                done += n as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let fraction = done as f64 / total as f64;
                    if fraction - reported >= 0.01 {
                        reported = fraction;
                        post(Event::Progress(id, fraction.min(1.0)));
                    }
                }
            }
            Err(_) => {
                let _ = std::fs::remove_file(&partial);
                return false;
            }
        }
    }
    drop(file);
    post(Event::Progress(id, 1.0));
    std::fs::rename(&partial, to).is_ok()
}

// ===== WebSockets =====

/// `openWebSocket(url:onMessage:onClose:)`: returns an id for sending and closing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_open_web_socket(
    url: *const Str,
    msg_f: *const std::ffi::c_void,
    msg_env: *mut Header,
    close_f: *const std::ffi::c_void,
    close_env: *mut Header,
) -> i64 {
    let url = unsafe { str_text(url) }.to_string();
    let id = next_id();
    let (tx, rx) = mpsc::channel();
    SOCKETS.with(|s| s.borrow_mut().insert(id, Socket(Callback::new(msg_f, msg_env), Callback::new(close_f, close_env), tx)));
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
            Ok(Message::Binary(b)) => post(Event::Message(id, String::from_utf8_lossy(&b).into_owned())),
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
    SOCKETS.with(|s| s.borrow().get(&(id as u64)).is_some_and(|Socket(_, _, tx)| tx.send(Outgoing::Text(text)).is_ok())) as u8
}

/// `closeWebSocket(id)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_close_web_socket(id: i64) {
    SOCKETS.with(|s| {
        if let Some(Socket(_, _, tx)) = s.borrow().get(&(id as u64)) {
            let _ = tx.send(Outgoing::Close);
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
            let cb = SOCKETS.with(|s| s.borrow().get(&id).map(|Socket(m, _, _)| m.clone()));
            if let Some(cb) = cb {
                cb.call_with(crate::str_new(&text) as *mut u8);
            }
        }
        Event::Closed(id, reason) => {
            if let Some(Socket(_, close, _)) = SOCKETS.with(|s| s.borrow_mut().remove(&id)) {
                close.call_with(crate::str_new(&reason) as *mut u8);
            }
        }
        _ => {}
    }
}
