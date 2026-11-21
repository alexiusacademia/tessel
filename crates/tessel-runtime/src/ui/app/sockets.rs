//! Sockets: TCP connections and listeners (`connectTCP`, `listenTCP`,
//! `sendTCP`, `tcpQueued`, `pauseTCP`, `closeTCP`, `tcpPort`, `tcpPeer`),
//! UDP (`openUDP`, `sendUDP`, `joinMulticast`, `udpPort`, `closeUDP`), an
//! HTTP server (`serveHTTP`, `respondHTTP`, `stopHTTP`), and `resolveHost`
//! and `localAddresses`.
//!
//! One thread (`io_thread`) waits on every socket at once with mio (kqueue
//! on macOS, IOCP on Windows, epoll on Linux and Android), so thousands of
//! connections cost no threads. The UI thread tells it what to do with
//! commands (`Cmd`, which wake it), and it answers with notes (`Note`),
//! posted as events whose blocks run on the UI thread like every other
//! callback's. Listeners and UDP sockets are bound on the UI thread, so
//! their ports are known at once (and -1 says it failed); host names are
//! resolved on short-lived threads. Each read makes one `Data`, on the I/O
//! thread, and hands it over.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use mio::net::{TcpListener, TcpStream, UdpSocket};
use mio::{Events, Interest, Poll, Token, Waker};

use super::{Event, post};
use crate::ui::Callback;
use crate::{Header, Str, str_new, str_text};

/// How much a socket reads at once: each read is one `Data`.
const READ_SIZE: usize = 64 * 1024;
/// Bytes read from a connection but not yet given to its block: past this,
/// reading waits for the block to catch up, so a fast peer can't fill memory.
const UNREAD_LIMIT: i64 = 4 << 20;
/// How long a closing connection goes on sending what's queued.
const CLOSE_GRACE: Duration = Duration::from_secs(5);
/// The most an HTTP request's line and headers may take.
const HEAD_LIMIT: usize = 64 * 1024;
/// The most an HTTP request's body may be.
const BODY_LIMIT: usize = 64 << 20;
/// The waker's token (ids are the other tokens).
const WAKE: Token = Token(usize::MAX);


/// A new socket or request id (unique across threads: accepted connections
/// get theirs on the I/O thread).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    super::threads::own_socket(id);
    id
}

/// What the UI thread reads about a socket without asking the I/O thread.
pub struct Shared {
    /// Bytes queued to send and not yet written (`tcpQueued`).
    queued: AtomicI64,
    /// Bytes read and not yet handed to the block (see `UNREAD_LIMIT`).
    unread: AtomicI64,
    /// Reading stopped for `unread` to come down; the UI thread pokes it.
    throttled: AtomicBool,
    /// The bound port, or a connection's own port once connected.
    port: AtomicI64,
    /// The other end, "ip:port", once connected.
    peer: Mutex<String>,
}

impl Shared {
    fn new(port: i64) -> Arc<Shared> {
        Arc::new(Shared {
            queued: AtomicI64::new(0),
            unread: AtomicI64::new(0),
            throttled: AtomicBool::new(false),
            port: AtomicI64::new(port),
            peer: Mutex::new(String::new()),
        })
    }

    fn connected(&self, stream: &TcpStream) {
        if let Ok(peer) = stream.peer_addr() {
            *self.peer.lock().unwrap() = peer.to_string();
        }
        if let Ok(local) = stream.local_addr() {
            self.port.store(local.port() as i64, Ordering::Relaxed);
        }
    }
}

/// What the UI thread asks the I/O thread to do.
enum Cmd {
    /// Connect to the first address that answers, by `deadline`.
    Connect { id: u64, addrs: Vec<SocketAddr>, deadline: Instant, shared: Arc<Shared> },
    Listen { id: u64, listener: TcpListener, http: bool },
    Udp { id: u64, socket: UdpSocket },
    Send { id: u64, bytes: Vec<u8> },
    Pause { id: u64, paused: bool },
    /// Reading may go on (`unread` came down).
    Poke(u64),
    Close(u64),
    /// The answer to an HTTP request, ready to send.
    Respond { request: u64, bytes: Vec<u8> },
    /// The status and headers of an answer sent in pieces (`startHTTP`).
    Start { request: u64, status: i64, headers: Vec<(String, String)>, head: bool },
    /// A piece of it (`sendHTTP`).
    Piece { request: u64, bytes: Vec<u8> },
    /// Its end (`endHTTP`).
    End { request: u64 },
    /// An answer that can't be finished: its connection closes.
    Abort { request: u64 },
    /// A request becomes a WebSocket (`acceptWebSocket`): the 101 answer.
    Upgrade { request: u64, bytes: Vec<u8> },
    /// A message on a server's WebSocket (a frame, ready to send).
    Frame { id: u64, bytes: Vec<u8> },
}

/// What the I/O thread tells the UI thread.
pub enum Note {
    Connected(u64),
    Accepted { listener: u64, id: u64, peer: String, shared: Arc<Shared> },
    Data(u64, Bytes),
    /// A connection, listener or UDP socket is gone (the reason for
    /// connections: "closed", or what went wrong).
    Closed(u64, String),
    Packet { id: u64, data: Bytes, from: String, port: i64 },
    Request { listener: u64, request: Box<Request> },
    /// The connection of a request ended before it was answered.
    Unanswered(u64),
    Resolved(u64, Vec<String>),
    /// A text message on a server's WebSocket (by its connection's id).
    WsText(u64, String),
    /// A binary message on a server's WebSocket.
    WsBinary(u64, Bytes),
}

/// A `Data` made on the I/O thread, for the UI thread (which owns it then).
pub struct Bytes(*mut Str);
unsafe impl Send for Bytes {}

/// An HTTP request, parsed on the I/O thread.
pub struct Request {
    pub(super) id: u64,
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    remote: String,
    /// Its connection, and what the app reads about it (`httpQueued`).
    conn: u64,
    shared: Arc<Shared>,
}

/// The request as the compiler's adapter receives it (all owned).
#[repr(C)]
struct RequestC {
    id: i64,
    method: *mut Str,
    path: *mut Str,
    query: *mut Str,
    headers: *mut u8,
    body: *mut Str,
    remote: *mut Str,
}

// ===== the UI thread's side =====

/// The blocks of an open socket.
enum Kind {
    Client { on_connect: Callback, on_data: Callback, on_close: Callback },
    /// A connection a listener accepted: its blocks are the listener's.
    Accepted { listener: u64 },
    Listener { on_connection: Callback, on_data: Callback, on_close: Callback },
    /// `socket` is a second handle on the same socket, to send with.
    Udp { on_packet: Callback, socket: std::net::UdpSocket },
    Http { on_request: Callback },
    /// A server whose requests go to worker threads (see `threads`): the
    /// main thread keeps it only to stop it.
    Pool,
}

struct Open {
    kind: Kind,
    shared: Arc<Shared>,
    /// Closed by the app; its blocks don't run any more.
    closing: bool,
}

thread_local! {
    /// Open sockets, by id.
    static OPEN: RefCell<HashMap<u64, Open>> = RefCell::new(HashMap::new());
    /// HTTP requests waiting for their answers.
    static ASKED: RefCell<HashMap<u64, Asked>> = RefCell::new(HashMap::new());
    /// Server WebSockets (`acceptWebSocket`), by id, and their ids by
    /// connection.
    static SERVER_WS: RefCell<HashMap<u64, ServerWs>> = RefCell::new(HashMap::new());
    static WS_OF_CONN: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
    /// `resolveHost` blocks waiting for their addresses.
    static RESOLVING: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
}

/// A request waiting for its answer.
struct Asked {
    head: bool,
    /// Its connection, and what's known about it without asking the I/O
    /// thread.
    conn: u64,
    shared: Arc<Shared>,
    /// The answer is being sent in pieces (`startHTTP`).
    started: bool,
    /// The client's `Sec-WebSocket-Key`, if it asked for a WebSocket.
    ws_key: Option<String>,
}

/// A server's WebSocket.
struct ServerWs {
    conn: u64,
    on_message: Callback,
    on_data: Option<Callback>,
    on_close: Callback,
}

/// Is any socket open, or a name being resolved? (A program with only `fn
/// main` keeps running while one is.)
pub fn busy() -> bool {
    OPEN.with(|o| !o.borrow().is_empty())
        || RESOLVING.with(|r| !r.borrow().is_empty())
        || SERVER_WS.with(|w| !w.borrow().is_empty())
}

/// The I/O thread: started by the first socket.
struct Io {
    tx: mpsc::Sender<Cmd>,
    waker: Waker,
}

static IO: OnceLock<Io> = OnceLock::new();

fn send(cmd: Cmd) {
    let io = IO.get_or_init(|| {
        let poll = Poll::new().expect("can't start the socket loop");
        let waker = Waker::new(poll.registry(), WAKE).expect("can't start the socket loop");
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("tessel-sockets".into())
            .spawn(move || {
                raise_fd_limit();
                io_thread(poll, rx);
            })
            .expect("can't start the socket thread");
        Io { tx, waker }
    });
    let _ = io.tx.send(cmd);
    let _ = io.waker.wake();
}

/// Thousands of connections need thousands of file descriptors: the soft
/// limit is often 256 on macOS.
#[cfg(unix)]
fn raise_fd_limit() {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return;
    }
    for want in [65536u64, 10240] {
        let want = (want as libc::rlim_t).min(limit.rlim_max);
        if limit.rlim_cur >= want {
            return;
        }
        let raised = libc::rlimit { rlim_cur: want, rlim_max: limit.rlim_max };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raised) } == 0 {
            return;
        }
    }
}

#[cfg(not(unix))]
fn raise_fd_limit() {}

fn text(s: *const Str) -> String {
    unsafe { str_text(s) }.to_string()
}

fn port_number(port: i64) -> Option<u16> {
    u16::try_from(port).ok()
}

/// The address to bind: `host` is an IP ("0.0.0.0", "::", "127.0.0.1"…)
/// or empty for every interface.
fn bind_address(host: &str, port: i64) -> Result<SocketAddr, String> {
    let Some(port) = port_number(port) else { return Err(format!("{port} isn't a port number (0 to 65535)")) };
    let host = host.trim();
    let ip = if host.is_empty() { IpAddr::V4(Ipv4Addr::UNSPECIFIED) } else {
        host.parse().map_err(|_| format!("`{host}` isn't an IP address (give one of this computer's, or \"0.0.0.0\" for all)"))?
    };
    Ok(SocketAddr::new(ip, port))
}

/// Why binding failed, in a sentence.
fn bind_error(what: &str, addr: SocketAddr, e: &io::Error) -> String {
    let why = match e.kind() {
        io::ErrorKind::AddrInUse => "the port is in use".to_string(),
        io::ErrorKind::PermissionDenied => "no permission (ports below 1024 need it)".to_string(),
        io::ErrorKind::AddrNotAvailable => "this computer doesn't have that address".to_string(),
        _ => e.to_string(),
    };
    format!("can't {what} on {addr}: {why}")
}

/// Why a connection ended, in a sentence.
fn sentence(e: &io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => "connection refused".into(),
        TimedOut => "timed out".into(),
        ConnectionReset | BrokenPipe => "connection reset".into(),
        ConnectionAborted => "connection aborted".into(),
        HostUnreachable => "host unreachable".into(),
        NetworkUnreachable => "network unreachable".into(),
        NetworkDown => "network down".into(),
        AddrNotAvailable => "address not available".into(),
        _ => e.to_string(),
    }
}

/// `connectTCP(host:port:timeout:onConnect:onData:onClose:)`: the blocks
/// are `(fn, env)` pairs (moved to the runtime).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_connect_tcp(
    host: *const Str,
    port: i64,
    timeout: f64,
    connect_f: *const c_void,
    connect_env: *mut Header,
    data_f: *const c_void,
    data_env: *mut Header,
    close_f: *const c_void,
    close_env: *mut Header,
) -> i64 {
    let host = text(host);
    let kind = Kind::Client {
        on_connect: Callback::new(connect_f, connect_env),
        on_data: Callback::new(data_f, data_env),
        on_close: Callback::new(close_f, close_env),
    };
    let Some(port) = port_number(port) else {
        crate::set_error(format!("can't connect to {host}: {port} isn't a port number (0 to 65535)"));
        return -1;
    };
    let id = new_id();
    let shared = Shared::new(-1);
    OPEN.with(|o| o.borrow_mut().insert(id, Open { kind, shared: shared.clone(), closing: false }));
    let timeout = if timeout > 0.0 { timeout } else { 10.0 };
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    match host.trim().parse::<IpAddr>() {
        Ok(ip) => send(Cmd::Connect { id, addrs: vec![SocketAddr::new(ip, port)], deadline, shared }),
        Err(_) => {
            // Resolved in the background (it can take a while).
            std::thread::spawn(move || {
                let addrs: Vec<SocketAddr> = (host.trim(), port).to_socket_addrs().map(|a| a.collect()).unwrap_or_default();
                if addrs.is_empty() {
                    post(Event::Socket(Note::Closed(id, format!("can't resolve {}", host.trim()))));
                } else {
                    send(Cmd::Connect { id, addrs, deadline, shared });
                }
            });
        }
    }
    id as i64
}

/// Binds a listener (the UI thread's half of `listenTCP` and `serveHTTP`).
fn listen(what: &str, host: &str, port: i64) -> Result<(TcpListener, i64), String> {
    let addr = bind_address(host, port)?;
    let listener = bind_listener(addr).map_err(|e| bind_error(what, addr, &e))?;
    let port = listener.local_addr().map(|a| a.port() as i64).unwrap_or(port);
    Ok((listener, port))
}

/// A listener with a long queue of connections waiting to be accepted
/// (where the system allows one: macOS caps it at `kern.ipc.somaxconn`,
/// 128 unless raised), for bursts of connections.
fn bind_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    // (So the port can be taken again right after the program ends; on
    // Windows that would let another program take it while it's in use.)
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(4096)?;
    socket.set_nonblocking(true)?;
    Ok(TcpListener::from_std(socket.into()))
}

/// `listenTCP(port:host:onConnection:onData:onClose:)`: -1 if it can't listen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_listen_tcp(
    port: i64,
    host: *const Str,
    conn_f: *const c_void,
    conn_env: *mut Header,
    data_f: *const c_void,
    data_env: *mut Header,
    close_f: *const c_void,
    close_env: *mut Header,
) -> i64 {
    let kind = Kind::Listener {
        on_connection: Callback::new(conn_f, conn_env),
        on_data: Callback::new(data_f, data_env),
        on_close: Callback::new(close_f, close_env),
    };
    let (listener, port) = match listen("listen", &text(host), port) {
        Ok(l) => l,
        Err(e) => {
            crate::set_error(e);
            return -1;
        }
    };
    let id = new_id();
    OPEN.with(|o| o.borrow_mut().insert(id, Open { kind, shared: Shared::new(port), closing: false }));
    send(Cmd::Listen { id, listener, http: false });
    id as i64
}

/// `sendTCP(id, data:)`: queues the bytes; false if the connection isn't open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_tcp(id: i64, data: *const Str) -> u8 {
    let bytes = unsafe { crate::codec::data_bytes(data) };
    let id = id as u64;
    let accepted = OPEN.with(|o| {
        let o = o.borrow();
        match o.get(&id) {
            Some(open) if !open.closing && matches!(open.kind, Kind::Client { .. } | Kind::Accepted { .. }) => {
                open.shared.queued.fetch_add(bytes.len() as i64, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    });
    if accepted && !bytes.is_empty() {
        send(Cmd::Send { id, bytes: bytes.to_vec() });
    }
    accepted as u8
}

/// `tcpQueued(id)`: bytes given to `sendTCP` and not yet sent.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_tcp_queued(id: i64) -> i64 {
    OPEN.with(|o| o.borrow().get(&(id as u64)).map_or(0, |open| open.shared.queued.load(Ordering::Relaxed).max(0)))
}

/// `pauseTCP(id, paused:)`: stops (or resumes) reading from a connection.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_pause_tcp(id: i64, paused: u8) {
    let id = id as u64;
    if OPEN.with(|o| o.borrow().get(&id).is_some_and(|open| !open.closing)) {
        send(Cmd::Pause { id, paused: paused != 0 });
    }
}

/// `closeTCP(id)` (also `closeUDP` and `stopHTTP`): closes a connection (after
/// sending what's queued), a listener and its connections, or a UDP socket.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_close_tcp(id: i64) {
    let id = id as u64;
    super::threads::stop_pool(id);
    let open = OPEN.with(|o| {
        o.borrow_mut().get_mut(&id).is_some_and(|open| {
            let was = open.closing;
            open.closing = true;
            !was
        })
    });
    if open {
        send(Cmd::Close(id));
    }
}

/// `tcpPort(id)` (also `udpPort`): the port a listener or UDP socket is
/// bound to, or a connection's own port; -1 if unknown.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_tcp_port(id: i64) -> i64 {
    OPEN.with(|o| o.borrow().get(&(id as u64)).map_or(-1, |open| open.shared.port.load(Ordering::Relaxed)))
}

/// `tcpPeer(id)`: "ip:port" of the other end, or "" before it's connected.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_tcp_peer(id: i64) -> *mut Str {
    let peer = OPEN.with(|o| o.borrow().get(&(id as u64)).map(|open| open.shared.peer.lock().unwrap().clone()));
    str_new(&peer.unwrap_or_default())
}

/// `openUDP(port:host:onPacket:)`: -1 if it can't bind. The socket can
/// broadcast, and shares its port with other programs (for multicast groups
/// like SSDP's, which several programs listen to at once).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_open_udp(port: i64, host: *const Str, packet_f: *const c_void, packet_env: *mut Header) -> i64 {
    let on_packet = Callback::new(packet_f, packet_env);
    let bound = bind_address(&text(host), port).and_then(|addr| bind_udp(addr).map_err(|e| bind_error("open a UDP socket", addr, &e)));
    let socket = match bound {
        Ok(s) => s,
        Err(e) => {
            crate::set_error(e);
            return -1;
        }
    };
    let port = socket.local_addr().map(|a| a.port() as i64).unwrap_or(port);
    let Ok(sender) = socket.try_clone() else {
        crate::set_error("can't open a UDP socket".into());
        return -1;
    };
    let id = new_id();
    let kind = Kind::Udp { on_packet, socket: sender };
    OPEN.with(|o| o.borrow_mut().insert(id, Open { kind, shared: Shared::new(port), closing: false }));
    send(Cmd::Udp { id, socket: UdpSocket::from_std(socket) });
    id as i64
}

fn bind_udp(addr: SocketAddr) -> io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    socket.set_reuse_port(true)?;
    socket.set_broadcast(true)?;
    socket.bind(&addr.into())?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

/// `sendUDP(id, data:host:port:)`: false if the socket isn't open, the port
/// is wrong, or the system wouldn't take the packet. A host name is resolved
/// in the background, and the packet sent then.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_udp(id: i64, data: *const Str, host: *const Str, port: i64) -> u8 {
    let bytes = unsafe { crate::codec::data_bytes(data) };
    let host = text(host);
    let Some(port) = port_number(port) else { return 0 };
    OPEN.with(|o| {
        let o = o.borrow();
        let Some(Open { kind: Kind::Udp { socket, .. }, closing: false, .. }) = o.get(&(id as u64)) else { return 0 };
        match host.trim().parse::<IpAddr>() {
            Ok(ip) => socket.send_to(bytes, SocketAddr::new(ip, port)).is_ok() as u8,
            Err(_) => {
                let Ok(socket) = socket.try_clone() else { return 0 };
                let bytes = bytes.to_vec();
                // (An address of the socket's own kind: "localhost" is ::1
                // first, which an IPv4 socket can't send to.)
                let v4 = socket.local_addr().is_ok_and(|a| a.is_ipv4());
                std::thread::spawn(move || {
                    let found = (host.trim(), port).to_socket_addrs().map(|a| a.collect::<Vec<_>>()).unwrap_or_default();
                    if let Some(addr) = found.iter().find(|a| a.is_ipv4() == v4).or(found.first()) {
                        let _ = socket.send_to(&bytes, addr);
                    }
                });
                1
            }
        }
    })
}

/// `joinMulticast(id, group:)`: receives packets sent to a multicast group.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_join_multicast(id: i64, group: *const Str) -> u8 {
    let group = text(group);
    OPEN.with(|o| {
        let o = o.borrow();
        let Some(Open { kind: Kind::Udp { socket, .. }, closing: false, .. }) = o.get(&(id as u64)) else { return 0 };
        match group.trim().parse::<IpAddr>() {
            Ok(IpAddr::V4(g)) => socket.join_multicast_v4(&g, &Ipv4Addr::UNSPECIFIED).is_ok() as u8,
            Ok(IpAddr::V6(g)) => socket.join_multicast_v6(&g, 0).is_ok() as u8,
            Err(_) => 0,
        }
    })
}

/// `serveHTTP(port:host:onRequest:)`: `request_f` is `adapter(box,
/// *RequestC)`; -1 if it can't listen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_serve_http(port: i64, host: *const Str, request_f: *const c_void, request_env: *mut Header) -> i64 {
    let kind = Kind::Http { on_request: Callback::new(request_f, request_env) };
    let (listener, port) = match listen("serve", &text(host), port) {
        Ok(l) => l,
        Err(e) => {
            crate::set_error(e);
            return -1;
        }
    };
    let id = new_id();
    OPEN.with(|o| o.borrow_mut().insert(id, Open { kind, shared: Shared::new(port), closing: false }));
    send(Cmd::Listen { id, listener, http: true });
    id as i64
}

/// `serveHTTP(port:host:workers:start:)`: like `tsl_serve_http`, but each
/// request is handled on one of `workers` threads, each with its own block
/// made by `start` (see `threads`). Returns the server's id, or -1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_serve_http_workers(port: i64, host: *const Str, workers: i64, start: super::StartFn, input: *mut u8) -> i64 {
    let (listener, port) = match listen("serve", &text(host), port) {
        Ok(l) => l,
        Err(e) => {
            crate::set_error(e);
            return -1;
        }
    };
    let id = new_id();
    OPEN.with(|o| o.borrow_mut().insert(id, Open { kind: Kind::Pool, shared: Shared::new(port), closing: false }));
    super::threads::start_pool(id, workers.clamp(1, 1024) as usize, start, input);
    send(Cmd::Listen { id, listener, http: true });
    id as i64
}

/// The client's `Sec-WebSocket-Key`, if the request asks for a WebSocket.
fn ws_key(r: &Request) -> Option<String> {
    let header = |n: &str| r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
    let upgrade = header("upgrade").is_some_and(|v| v.to_ascii_lowercase().contains("websocket"));
    let connection = header("connection").is_some_and(|v| v.to_ascii_lowercase().contains("upgrade"));
    if r.method != "GET" || !upgrade || !connection || header("sec-websocket-version") != Some("13") {
        return None;
    }
    header("sec-websocket-key").map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

fn server_ws<T>(conn: u64, f: impl FnOnce(&ServerWs) -> T) -> Option<T> {
    let id = WS_OF_CONN.with(|m| m.borrow().get(&conn).copied())?;
    SERVER_WS.with(|w| w.borrow().get(&id).map(f))
}

/// `startHTTP(request, status:headers:)`: sends an answer's status and
/// headers; its body follows with `sendHTTP`, and ends with `endHTTP`.
/// Without a Content-Length it goes in chunks. False if the request was
/// answered already, or its connection is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_start_http(request: i64, status: i64, headers: *const crate::Dict) -> u8 {
    let request = request as u64;
    let head = ASKED.with(|a| match a.borrow_mut().get_mut(&request) {
        Some(asked) if !asked.started => {
            asked.started = true;
            Some(asked.head)
        }
        _ => None,
    });
    let Some(head) = head else { return 0 };
    let headers = unsafe { super::net::pairs(headers) };
    let status = if (100..1000).contains(&status) { status } else { 500 };
    send(Cmd::Start { request, status, headers, head });
    if head {
        // (A HEAD's answer has no body: it's finished.)
        ASKED.with(|a| a.borrow_mut().remove(&request));
    }
    1
}

/// `sendHTTP(request, data:)`: the next piece of an answer `startHTTP`
/// began. False if it wasn't started, or its connection is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_http(request: i64, data: *const Str) -> u8 {
    let request = request as u64;
    if !ASKED.with(|a| a.borrow().get(&request).is_some_and(|x| x.started)) {
        return 0;
    }
    let bytes = unsafe { crate::codec::data_bytes(data) }.to_vec();
    send(Cmd::Piece { request, bytes });
    1
}

/// `endHTTP(request)`: ends an answer sent in pieces.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_end_http(request: i64) -> u8 {
    let request = request as u64;
    let started = ASKED.with(|a| {
        let mut a = a.borrow_mut();
        if a.get(&request).is_some_and(|x| x.started) {
            a.remove(&request);
            true
        } else {
            false
        }
    });
    if started {
        send(Cmd::End { request });
    }
    started as u8
}

/// `httpQueued(request)`: bytes of the answer not yet sent (0 once it's
/// answered or its connection is gone).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_http_queued(request: i64) -> i64 {
    ASKED.with(|a| a.borrow().get(&(request as u64)).map_or(0, |x| x.shared.queued.load(Ordering::Relaxed).max(0)))
}

/// `acceptWebSocket(request, onMessage:onData:onClose:)`: answers a
/// request that asks for a WebSocket, which stays open: messages from the
/// client come to the blocks, and `sendWebSocket` sends to it. Returns its
/// id (from the same series as `openWebSocket`'s), or -1 if the request
/// isn't asking for a WebSocket, or was answered already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_accept_web_socket(
    request: i64,
    msg_f: *const c_void,
    msg_env: *mut Header,
    data_f: *const c_void,
    data_env: *mut Header,
    close_f: *const c_void,
    close_env: *mut Header,
) -> i64 {
    let on_message = Callback::new(msg_f, msg_env);
    let on_data = (!data_f.is_null()).then(|| Callback::new(data_f, data_env));
    let on_close = Callback::new(close_f, close_env);
    let request = request as u64;
    let asked = ASKED.with(|a| {
        let mut a = a.borrow_mut();
        if a.get(&request).is_some_and(|x| !x.started && x.ws_key.is_some()) { a.remove(&request) } else { None }
    });
    let Some(asked) = asked else { return -1 };
    let key = asked.ws_key.unwrap_or_default();
    use sha1::Digest;
    let accept = crate::codec::base64_encode(&sha1::Sha1::digest(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()));
    let bytes = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )
    .into_bytes();
    let id = super::next_id();
    // (The connection's events come to this thread from now on.)
    super::threads::own_socket(asked.conn);
    SERVER_WS.with(|w| w.borrow_mut().insert(id, ServerWs { conn: asked.conn, on_message, on_data, on_close }));
    WS_OF_CONN.with(|m| m.borrow_mut().insert(asked.conn, id));
    send(Cmd::Upgrade { request, bytes });
    id as i64
}

/// A message on a server's WebSocket (`sendWebSocket`): `None` if `id`
/// isn't one, else whether it's open.
pub(super) fn ws_send(id: u64, binary: bool, payload: &[u8]) -> Option<bool> {
    let conn = SERVER_WS.with(|w| w.borrow().get(&id).map(|x| x.conn))?;
    send(Cmd::Frame { id: conn, bytes: ws_frame_bytes(if binary { 0x2 } else { 0x1 }, payload) });
    Some(true)
}

/// `closeWebSocket(id)` for a server's WebSocket: false if `id` isn't one.
pub(super) fn ws_close(id: u64) -> bool {
    let Some(conn) = SERVER_WS.with(|w| w.borrow().get(&id).map(|x| x.conn)) else { return false };
    send(Cmd::Frame { id: conn, bytes: ws_frame_bytes(0x8, &1000u16.to_be_bytes()) });
    send(Cmd::Close(conn));
    true
}

/// On a worker: its block for the requests of `listener`.
pub(super) fn serve_here(listener: u64, on_request: Callback) {
    OPEN.with(|o| o.borrow_mut().insert(listener, Open { kind: Kind::Http { on_request }, shared: Shared::new(0), closing: false }));
}

/// On a worker that failed: a 500 for each request it was given and
/// hasn't answered.
pub(super) fn fail_waiting() {
    let waiting: Vec<(u64, Asked)> = ASKED.with(|a| a.borrow_mut().drain().collect());
    for (request, asked) in waiting {
        if asked.started {
            // (Half an answer is out: its connection closes.)
            send(Cmd::Abort { request });
            continue;
        }
        let body = b"Internal Server Error";
        let headers = vec![("Content-Type".to_string(), "text/plain; charset=utf-8".to_string())];
        send(Cmd::Respond { request, bytes: response_bytes(500, &headers, body, asked.head) });
    }
}

/// The reason phrase of an HTTP status.
fn reason(status: i64) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// Queues bytes to send on a connection, counted for `httpQueued`.
fn queue(conn: &mut Conn, bytes: Vec<u8>) {
    conn.shared.queued.fetch_add(bytes.len() as i64, Ordering::Relaxed);
    conn.out.push_back(bytes);
}

/// The status line and headers of an answer sent in pieces.
fn head_bytes(status: i64, headers: &[(String, String)]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status)).into_bytes();
    for (k, v) in headers {
        let k = k.replace(['\r', '\n', ':'], "");
        for line in v.split('\n') {
            let line = line.replace('\r', "");
            out.extend_from_slice(format!("{k}: {line}\r\n").as_bytes());
        }
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// A WebSocket frame from a client, read from the start of `input`.
struct WsFrame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
    consumed: usize,
}

/// The frame at the start of `input`: `None` if it isn't all here yet.
/// A client's frames are masked; one that isn't, or is too big, is an error.
fn ws_frame(input: &[u8], limit: usize) -> Result<Option<WsFrame>, &'static str> {
    if input.len() < 2 {
        return Ok(None);
    }
    let fin = input[0] & 0x80 != 0;
    if input[0] & 0x70 != 0 {
        return Err("unknown extension");
    }
    let opcode = input[0] & 0x0f;
    if !matches!(opcode, 0x0 | 0x1 | 0x2 | 0x8 | 0x9 | 0xA) {
        return Err("unknown frame");
    }
    if input[1] & 0x80 == 0 {
        return Err("a client's frames must be masked");
    }
    let mut at = 2;
    let mut len = (input[1] & 0x7f) as u64;
    if len == 126 {
        if input.len() < 4 {
            return Ok(None);
        }
        len = u16::from_be_bytes([input[2], input[3]]) as u64;
        at = 4;
    } else if len == 127 {
        if input.len() < 10 {
            return Ok(None);
        }
        len = u64::from_be_bytes(input[2..10].try_into().unwrap());
        at = 10;
    }
    if len > limit as u64 {
        return Err("message too big");
    }
    if input.len() < at + 4 + len as usize {
        return Ok(None);
    }
    let mask = [input[at], input[at + 1], input[at + 2], input[at + 3]];
    at += 4;
    let payload: Vec<u8> = input[at..at + len as usize].iter().enumerate().map(|(i, b)| b ^ mask[i % 4]).collect();
    Ok(Some(WsFrame { fin, opcode, payload, consumed: at + len as usize }))
}

/// A frame from the server (not masked), with the whole message.
pub(super) fn ws_frame_bytes(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    let n = payload.len();
    if n < 126 {
        out.push(n as u8);
    } else if n <= 0xffff {
        out.push(126);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(n as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn ws_close_frame(code: u16, why: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(why.as_bytes());
    ws_frame_bytes(0x8, &payload)
}

/// A response with `Connection: keep-alive` after its status line, unless
/// it has a Connection header of its own.
fn with_keep_alive(bytes: Vec<u8>) -> Vec<u8> {
    let Some(line_end) = find(&bytes, b"\r\n", 0) else { return bytes };
    let head_end = find(&bytes, b"\r\n\r\n", 0).unwrap_or(bytes.len());
    let has_own = bytes[line_end..head_end]
        .split(|&b| b == b'\n')
        .any(|line| line.len() >= 11 && line[..11].eq_ignore_ascii_case(b"connection:"));
    if has_own {
        return bytes;
    }
    let mut out = Vec::with_capacity(bytes.len() + 24);
    out.extend_from_slice(&bytes[..line_end + 2]);
    out.extend_from_slice(b"Connection: keep-alive\r\n");
    out.extend_from_slice(&bytes[line_end + 2..]);
    out
}

/// An HTTP/1.1 response: the status line, `headers` (with a Content-Length
/// unless one is given), and the body (none for a HEAD, 204 or 304).
fn response_bytes(status: i64, headers: &[(String, String)], body: &[u8], head: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status)).into_bytes();
    let mut has_length = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_length = true;
        }
        // A value with line breaks is several headers of the same name, one
        // for each line (two `Set-Cookie`s, say); nothing else can make a
        // header (#83).
        let k = k.replace(['\r', '\n', ':'], "");
        for line in v.split('\n') {
            let line = line.replace('\r', "");
            out.extend_from_slice(format!("{k}: {line}\r\n").as_bytes());
        }
    }
    if !has_length {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if !head && status != 204 && status != 304 {
        out.extend_from_slice(body);
    }
    out
}

/// `respondHTTP(request, status:headers:body:)`: false if the request was
/// answered already, or its connection is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_respond_http(request: i64, status: i64, headers: *const crate::Dict, body: *const Str) -> u8 {
    let request = request as u64;
    // (An answer being sent in pieces ends with `endHTTP`.)
    if ASKED.with(|a| a.borrow().get(&request).is_some_and(|x| x.started)) {
        return 0;
    }
    let Some(Asked { head, .. }) = ASKED.with(|a| a.borrow_mut().remove(&request)) else { return 0 };
    let headers = unsafe { super::net::pairs(headers) };
    let body = unsafe { crate::codec::data_bytes(body) };
    let status = if (100..1000).contains(&status) { status } else { 500 };
    send(Cmd::Respond { request, bytes: response_bytes(status, &headers, body, head) });
    1
}

/// `resolveHost(name, done:)`: `done` gets the addresses (none if the name
/// isn't known).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_resolve_host(name: *const Str, f: *const c_void, env: *mut Header) {
    let name = text(name);
    let id = new_id();
    RESOLVING.with(|r| r.borrow_mut().insert(id, Callback::new(f, env)));
    std::thread::spawn(move || {
        let mut addrs: Vec<String> = Vec::new();
        if let Ok(found) = (name.trim(), 0).to_socket_addrs() {
            for a in found {
                let a = a.ip().to_string();
                if !addrs.contains(&a) {
                    addrs.push(a);
                }
            }
        }
        post(Event::Socket(Note::Resolved(id, addrs)));
    });
}

/// `localAddresses()`: this computer's IP addresses (not loopback or
/// link-local ones), IPv4 first.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_local_addresses() -> *mut crate::List {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        match iface.ip() {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => v4.push(ip.to_string()),
            IpAddr::V6(ip) if !ip.is_loopback() && !ip.is_unicast_link_local() => v6.push(ip.to_string()),
            _ => {}
        }
    }
    v4.append(&mut v6);
    v4.dedup();
    unsafe { crate::str_list(v4.iter().map(String::as_str)) }
}

/// A `[String: String]` of `pairs` (a new reference).
fn string_dict(pairs: &[(String, String)]) -> *mut u8 {
    let dict = unsafe { crate::tsl_dict_new(&crate::TSL_TYPE_STR, &crate::TSL_TYPE_STR) };
    let mut slot = dict;
    for (k, v) in pairs {
        let (mut k, mut v) = (str_new(k), str_new(v));
        unsafe { crate::tsl_dict_set(&mut slot, &mut k as *mut *mut Str as *mut u8, &mut v as *mut *mut Str as *mut u8) };
    }
    slot as *mut u8
}

fn release_data(data: Bytes) {
    unsafe { crate::tsl_release(data.0 as *mut Header) };
}

/// Runs the blocks for a note, on the UI thread.
pub fn deliver(note: Note) {
    match note {
        Note::Connected(id) => {
            let cb = OPEN.with(|o| match o.borrow().get(&id) {
                Some(Open { kind: Kind::Client { on_connect, .. }, closing: false, .. }) => Some(on_connect.clone()),
                _ => None,
            });
            if let Some(cb) = cb {
                cb.call();
            }
        }
        Note::Accepted { listener, id, peer, shared } => {
            let cb = OPEN.with(|o| match o.borrow().get(&listener) {
                Some(Open { kind: Kind::Listener { on_connection, .. }, closing: false, .. }) => Some(on_connection.clone()),
                _ => None,
            });
            // (A listener the app has closed: the I/O thread closes this
            // connection too, and nothing is said about it.)
            if let Some(cb) = cb {
                OPEN.with(|o| o.borrow_mut().insert(id, Open { kind: Kind::Accepted { listener }, shared, closing: false }));
                cb.call_with_int_ptr(id as i64, str_new(&peer) as *mut u8);
            }
        }
        Note::Data(id, data) => {
            let len = unsafe { (*data.0).len };
            // The block to call: a client's own, or the listener's with the id.
            let target = OPEN.with(|o| {
                let o = o.borrow();
                let open = o.get(&id)?;
                let shared = &open.shared;
                if shared.unread.fetch_sub(len, Ordering::Relaxed) - len < UNREAD_LIMIT / 2 && shared.throttled.swap(false, Ordering::Relaxed) {
                    send(Cmd::Poke(id));
                }
                if open.closing {
                    return None;
                }
                match &open.kind {
                    Kind::Client { on_data, .. } => Some((on_data.clone(), None)),
                    Kind::Accepted { listener } => match o.get(listener) {
                        Some(Open { kind: Kind::Listener { on_data, .. }, closing: false, .. }) => Some((on_data.clone(), Some(id))),
                        _ => None,
                    },
                    _ => None,
                }
            });
            match target {
                Some((cb, None)) => cb.call_with(data.0 as *mut u8),
                Some((cb, Some(id))) => cb.call_with_int_ptr(id as i64, data.0 as *mut u8),
                None => release_data(data),
            }
        }
        Note::Closed(id, reason) => {
            // A server's WebSocket: its `onClose`.
            if let Some(ws_id) = WS_OF_CONN.with(|m| m.borrow_mut().remove(&id)) {
                if let Some(ws) = SERVER_WS.with(|w| w.borrow_mut().remove(&ws_id)) {
                    ws.on_close.call_with(str_new(&reason) as *mut u8);
                }
                return;
            }
            let Some(open) = OPEN.with(|o| o.borrow_mut().remove(&id)) else { return };
            match open.kind {
                Kind::Client { on_close, .. } => on_close.call_with(str_new(&reason) as *mut u8),
                Kind::Accepted { listener } => {
                    let cb = OPEN.with(|o| match o.borrow().get(&listener) {
                        Some(Open { kind: Kind::Listener { on_close, .. }, .. }) => Some(on_close.clone()),
                        _ => None,
                    });
                    if let Some(cb) = cb {
                        cb.call_with_int_ptr(id as i64, str_new(&reason) as *mut u8);
                    }
                }
                Kind::Listener { .. } | Kind::Udp { .. } | Kind::Http { .. } | Kind::Pool => {}
            }
        }
        Note::Packet { id, data, from, port } => {
            let cb = OPEN.with(|o| match o.borrow().get(&id) {
                Some(Open { kind: Kind::Udp { on_packet, .. }, closing: false, .. }) => Some(on_packet.clone()),
                _ => None,
            });
            match cb {
                Some(cb) => cb.call_with_ptrs_int(data.0 as *mut u8, str_new(&from) as *mut u8, port),
                None => release_data(data),
            }
        }
        Note::Request { listener, request } => {
            let cb = OPEN.with(|o| match o.borrow().get(&listener) {
                Some(Open { kind: Kind::Http { on_request }, closing: false, .. }) => Some(on_request.clone()),
                _ => None,
            });
            let Some(cb) = cb else { return };
            let ws_key = ws_key(&request);
            ASKED.with(|a| {
                a.borrow_mut().insert(
                    request.id,
                    Asked { head: request.method == "HEAD", conn: request.conn, shared: request.shared.clone(), started: false, ws_key },
                )
            });
            let mut c = RequestC {
                id: request.id as i64,
                method: str_new(&request.method),
                path: str_new(&request.path),
                query: str_new(&request.query),
                headers: string_dict(&request.headers),
                body: crate::codec::data_new(&request.body),
                remote: str_new(&request.remote),
            };
            cb.call_with(&mut c as *mut RequestC as *mut u8);
        }
        Note::Unanswered(request) => {
            ASKED.with(|a| a.borrow_mut().remove(&request));
        }
        Note::WsText(conn, text) => {
            let cb = server_ws(conn, |w| w.on_message.clone());
            if let Some(cb) = cb {
                cb.call_with(str_new(&text) as *mut u8);
            }
        }
        Note::WsBinary(conn, data) => {
            match server_ws(conn, |w| (w.on_data.clone(), w.on_message.clone())) {
                Some((Some(on_data), _)) => on_data.call_with(data.0 as *mut u8),
                Some((None, on_message)) => {
                    let text = String::from_utf8_lossy(unsafe { crate::codec::data_bytes(data.0) }).into_owned();
                    release_data(data);
                    on_message.call_with(str_new(&text) as *mut u8);
                }
                None => release_data(data),
            }
        }
        Note::Resolved(id, addrs) => {
            if let Some(cb) = RESOLVING.with(|r| r.borrow_mut().remove(&id)) {
                cb.call_with(unsafe { crate::str_list(addrs.iter().map(String::as_str)) } as *mut u8);
            }
        }
    }
}

// ===== the I/O thread =====

enum State {
    Connecting { addrs: Vec<SocketAddr>, deadline: Instant },
    Open,
    /// Closed by the app: sends what's queued (until `deadline`), then ends.
    Closing { deadline: Instant },
}

/// An HTTP server connection's parsing state.
struct Http {
    inbuf: Vec<u8>,
    /// The request handed to the app and not yet answered.
    pending: Option<u64>,
    /// Close once the pending request is answered (HTTP/1.0, or
    /// `Connection: close`).
    close_after: bool,
    /// An HTTP/1.0 client asked to keep the connection: the answer must
    /// say it's kept (#82).
    say_kept: bool,
    /// An HTTP/1.1 client (it takes an answer in chunks).
    version_11: bool,
    /// An answer being sent in pieces: in chunks (HTTP/1.1, no length
    /// given), or as they are.
    streaming: Option<bool>,
    /// "100 Continue" was sent for the request being read.
    continued: bool,
}

struct Conn {
    stream: TcpStream,
    shared: Arc<Shared>,
    state: State,
    out: VecDeque<Vec<u8>>,
    /// How much of `out`'s first piece has been written.
    sent: usize,
    paused: bool,
    /// What it's registered for (none if it isn't).
    interest: Option<Interest>,
    /// The listener it came from.
    listener: Option<u64>,
    /// Its remote address.
    peer: String,
    http: Option<Http>,
    /// A server's WebSocket (an HTTP connection that was upgraded).
    ws: Option<Ws>,
}

/// A server's WebSocket: frames being read.
#[derive(Default)]
struct Ws {
    inbuf: Vec<u8>,
    /// A message in pieces: its kind (1 text, 2 binary) and what came.
    kind: u8,
    message: Vec<u8>,
    /// Why it's closing, for `onClose`.
    reason: Option<String>,
}

enum Entry {
    Conn(Conn),
    Listener { listener: TcpListener, http: bool },
    Udp(UdpSocket),
}

struct Loop {
    poll: Poll,
    entries: HashMap<u64, Entry>,
    /// HTTP requests waiting for their answers: the connection of each.
    requests: HashMap<u64, u64>,
    /// Ids closed before their connection arrived (a close while the host
    /// was being resolved).
    cancelled: HashSet<u64>,
    buf: Vec<u8>,
}

fn io_thread(poll: Poll, rx: mpsc::Receiver<Cmd>) {
    let mut lp = Loop { poll, entries: HashMap::new(), requests: HashMap::new(), cancelled: HashSet::new(), buf: vec![0; READ_SIZE] };
    let mut events = Events::with_capacity(1024);
    loop {
        let timeout = lp.next_deadline().map(|d| d.saturating_duration_since(Instant::now()));
        if let Err(e) = lp.poll.poll(&mut events, timeout) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        while let Ok(cmd) = rx.try_recv() {
            lp.command(cmd);
        }
        for event in events.iter() {
            if event.token() == WAKE {
                continue;
            }
            lp.ready(event.token().0 as u64, event.is_readable() || event.is_read_closed() || event.is_error(), event.is_writable() || event.is_error());
        }
        while let Ok(cmd) = rx.try_recv() {
            lp.command(cmd);
        }
        lp.expire();
    }
}

impl Loop {
    fn next_deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|e| match e {
                Entry::Conn(Conn { state: State::Connecting { deadline, .. } | State::Closing { deadline }, .. }) => Some(*deadline),
                _ => None,
            })
            .min()
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect { id, addrs, deadline, shared } => {
                if self.cancelled.remove(&id) {
                    return;
                }
                // The first address that starts connecting; the rest wait
                // in case it fails (`connect_next`).
                let mut addrs = addrs;
                let mut stream = None;
                let mut error = None;
                while !addrs.is_empty() && stream.is_none() {
                    match TcpStream::connect(addrs.remove(0)) {
                        Ok(s) => stream = Some(s),
                        Err(e) => error = Some(e),
                    }
                }
                let Some(stream) = stream else {
                    let why = error.as_ref().map_or("can't connect".to_string(), sentence);
                    post(Event::Socket(Note::Closed(id, why)));
                    return;
                };
                let mut conn = Conn {
                    stream,
                    shared,
                    state: State::Connecting { addrs, deadline },
                    out: VecDeque::new(),
                    sent: 0,
                    paused: false,
                    interest: None,
                    listener: None,
                    peer: String::new(),
                    http: None,
                    ws: None,
                };
                if self.poll.registry().register(&mut conn.stream, Token(id as usize), Interest::READABLE | Interest::WRITABLE).is_err() {
                    post(Event::Socket(Note::Closed(id, "can't connect".into())));
                    return;
                }
                conn.interest = Some(Interest::READABLE | Interest::WRITABLE);
                self.entries.insert(id, Entry::Conn(conn));
            }
            Cmd::Listen { id, mut listener, http } => {
                if self.poll.registry().register(&mut listener, Token(id as usize), Interest::READABLE).is_err() {
                    post(Event::Socket(Note::Closed(id, "closed".into())));
                    return;
                }
                self.entries.insert(id, Entry::Listener { listener, http });
            }
            Cmd::Udp { id, mut socket } => {
                if self.poll.registry().register(&mut socket, Token(id as usize), Interest::READABLE).is_err() {
                    post(Event::Socket(Note::Closed(id, "closed".into())));
                    return;
                }
                self.entries.insert(id, Entry::Udp(socket));
            }
            Cmd::Send { id, bytes } => {
                if let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) {
                    conn.out.push_back(bytes);
                    if matches!(conn.state, State::Open) {
                        self.flush(id);
                    }
                }
            }
            Cmd::Pause { id, paused } => {
                if let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) {
                    conn.paused = paused;
                    if !paused {
                        self.read(id);
                    } else {
                        self.update_interest(id);
                    }
                }
            }
            Cmd::Poke(id) => self.read(id),
            Cmd::Close(id) => self.close(id),
            Cmd::Respond { request, bytes } => {
                let Some(&id) = self.requests.get(&request) else { return };
                let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
                let say_kept = conn.http.as_ref().is_some_and(|h| h.say_kept && h.pending == Some(request));
                queue(conn, if say_kept { with_keep_alive(bytes) } else { bytes });
                self.answered(request);
            }
            Cmd::Start { request, status, mut headers, head } => {
                let Some(&id) = self.requests.get(&request) else { return };
                let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
                let Some(http) = &mut conn.http else { return };
                let has_length = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
                headers.retain(|(k, _)| !k.eq_ignore_ascii_case("transfer-encoding"));
                // In chunks if the client takes them and no length is
                // given; otherwise as they come, and an answer without a
                // length ends with the connection.
                let chunked = !has_length && http.version_11;
                if chunked {
                    headers.push(("Transfer-Encoding".into(), "chunked".into()));
                } else if !has_length {
                    http.close_after = true;
                    http.say_kept = false;
                    headers.push(("Connection".into(), "close".into()));
                }
                http.streaming = Some(chunked);
                let mut bytes = head_bytes(status, &headers);
                if http.say_kept && has_length {
                    bytes = with_keep_alive(bytes);
                }
                queue(conn, bytes);
                if head {
                    self.answered(request);
                } else {
                    self.flush(id);
                }
            }
            Cmd::Piece { request, bytes } => {
                let Some(&id) = self.requests.get(&request) else { return };
                let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
                let Some(chunked) = conn.http.as_ref().and_then(|h| h.streaming) else { return };
                if bytes.is_empty() {
                    return;
                }
                if chunked {
                    let mut piece = format!("{:x}\r\n", bytes.len()).into_bytes();
                    piece.extend_from_slice(&bytes);
                    piece.extend_from_slice(b"\r\n");
                    queue(conn, piece);
                } else {
                    queue(conn, bytes);
                }
                self.flush(id);
            }
            Cmd::End { request } => {
                let Some(&id) = self.requests.get(&request) else { return };
                let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
                if conn.http.as_ref().and_then(|h| h.streaming) == Some(true) {
                    queue(conn, b"0\r\n\r\n".to_vec());
                }
                self.answered(request);
            }
            Cmd::Abort { request } => {
                let Some(id) = self.requests.remove(&request) else { return };
                if let Some(Entry::Conn(conn)) = self.entries.get_mut(&id)
                    && let Some(http) = &mut conn.http
                {
                    http.pending = None;
                }
                self.finish(id, "closed".into());
            }
            Cmd::Upgrade { request, bytes } => {
                let Some(id) = self.requests.remove(&request) else { return };
                let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
                // What came after the request is the first frames.
                let rest = conn.http.take().map(|h| h.inbuf).unwrap_or_default();
                conn.ws = Some(Ws { inbuf: rest, ..Ws::default() });
                queue(conn, bytes);
                self.flush(id);
                self.read_ws(id, Vec::new());
            }
            Cmd::Frame { id, bytes } => {
                if let Some(Entry::Conn(conn)) = self.entries.get_mut(&id)
                    && conn.ws.is_some()
                    && matches!(conn.state, State::Open)
                {
                    queue(conn, bytes);
                    self.flush(id);
                }
            }
        }
    }

    /// `request` has had all of its answer: the connection closes, or goes
    /// on to the next request.
    fn answered(&mut self, request: u64) {
        let Some(id) = self.requests.remove(&request) else { return };
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let close_after = match &mut conn.http {
            Some(http) if http.pending == Some(request) => {
                http.pending = None;
                http.streaming = None;
                http.close_after
            }
            _ => false,
        };
        if close_after {
            conn.state = State::Closing { deadline: Instant::now() + CLOSE_GRACE };
        }
        self.flush(id);
        if !close_after {
            self.parse_http(id);
        }
    }

    /// Reads the frames that have come on a server's WebSocket: messages go
    /// to the app; pings are answered, and a close is answered and ends it.
    fn read_ws(&mut self, id: u64, input: Vec<u8>) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let Some(ws) = &mut conn.ws else { return };
        ws.inbuf.extend_from_slice(&input);
        let mut replies: Vec<Vec<u8>> = Vec::new();
        let mut closing = false;
        loop {
            let frame = match ws_frame(&ws.inbuf, BODY_LIMIT) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(why) => {
                    replies.push(ws_close_frame(1002, why));
                    ws.reason = Some(format!("closed: {why}"));
                    closing = true;
                    break;
                }
            };
            ws.inbuf.drain(..frame.consumed);
            match frame.opcode {
                0x1 | 0x2 | 0x0 => {
                    if frame.opcode != 0 {
                        ws.kind = frame.opcode;
                        ws.message.clear();
                    }
                    ws.message.extend_from_slice(&frame.payload);
                    if ws.message.len() > BODY_LIMIT {
                        replies.push(ws_close_frame(1009, "message too big"));
                        ws.reason = Some("closed: message too big".into());
                        closing = true;
                        break;
                    }
                    if frame.fin {
                        let message = std::mem::take(&mut ws.message);
                        if ws.kind == 0x2 {
                            post(Event::Socket(Note::WsBinary(id, Bytes(crate::codec::data_new(&message)))));
                        } else {
                            post(Event::Socket(Note::WsText(id, String::from_utf8_lossy(&message).into_owned())));
                        }
                    }
                }
                0x8 => {
                    let reason = if frame.payload.len() > 2 {
                        String::from_utf8_lossy(&frame.payload[2..]).into_owned()
                    } else {
                        "closed by the client".into()
                    };
                    ws.reason.get_or_insert(reason);
                    let code = if frame.payload.len() >= 2 { u16::from_be_bytes([frame.payload[0], frame.payload[1]]) } else { 1000 };
                    replies.push(ws_close_frame(code, ""));
                    closing = true;
                    break;
                }
                0x9 => replies.push(ws_frame_bytes(0xA, &frame.payload)),
                _ => {}
            }
        }
        for r in replies {
            queue(conn, r);
        }
        if closing {
            conn.state = State::Closing { deadline: Instant::now() + CLOSE_GRACE };
        }
        self.flush(id);
    }

    /// The app closes `id`: a connection (once what's queued is sent), a
    /// listener (and its connections), or a UDP socket.
    fn close(&mut self, id: u64) {
        match self.entries.get_mut(&id) {
            Some(Entry::Conn(conn)) => {
                if matches!(conn.state, State::Open) && !conn.out.is_empty() {
                    conn.state = State::Closing { deadline: Instant::now() + CLOSE_GRACE };
                    self.update_interest(id);
                } else {
                    self.finish(id, "closed".into());
                }
            }
            Some(Entry::Listener { .. }) => {
                let conns: Vec<u64> = self
                    .entries
                    .iter()
                    .filter_map(|(cid, e)| match e {
                        Entry::Conn(c) if c.listener == Some(id) => Some(*cid),
                        _ => None,
                    })
                    .collect();
                for cid in conns {
                    self.finish(cid, "closed".into());
                }
                if let Some(Entry::Listener { mut listener, .. }) = self.entries.remove(&id) {
                    let _ = self.poll.registry().deregister(&mut listener);
                }
                post(Event::Socket(Note::Closed(id, "closed".into())));
            }
            Some(Entry::Udp(_)) => {
                if let Some(Entry::Udp(mut socket)) = self.entries.remove(&id) {
                    let _ = self.poll.registry().deregister(&mut socket);
                }
                post(Event::Socket(Note::Closed(id, "closed".into())));
            }
            None => {
                // Closed before its connection arrived, or already gone.
                self.cancelled.insert(id);
                post(Event::Socket(Note::Closed(id, "closed".into())));
            }
        }
    }

    /// Ends a connection, telling the UI thread why.
    fn finish(&mut self, id: u64, reason: String) {
        let Some(Entry::Conn(mut conn)) = self.entries.remove(&id) else { return };
        let _ = self.poll.registry().deregister(&mut conn.stream);
        let _ = conn.stream.shutdown(std::net::Shutdown::Both);
        if let Some(http) = &conn.http {
            if let Some(request) = http.pending {
                self.requests.remove(&request);
                post(Event::Socket(Note::Unanswered(request)));
            }
        } else {
            let reason = conn.ws.as_mut().and_then(|w| w.reason.take()).unwrap_or(reason);
            post(Event::Socket(Note::Closed(id, reason)));
        }
    }

    /// Connections past their deadlines.
    fn expire(&mut self) {
        let now = Instant::now();
        let due: Vec<(u64, bool)> = self
            .entries
            .iter()
            .filter_map(|(id, e)| match e {
                Entry::Conn(Conn { state: State::Connecting { deadline, .. }, .. }) if *deadline <= now => Some((*id, true)),
                Entry::Conn(Conn { state: State::Closing { deadline }, .. }) if *deadline <= now => Some((*id, false)),
                _ => None,
            })
            .collect();
        for (id, connecting) in due {
            self.finish(id, if connecting { "timed out".into() } else { "closed".into() });
        }
    }

    /// Registers `id` for what it needs now: reading unless paused, throttled
    /// or closing; writing while connecting or something is queued.
    fn update_interest(&mut self, id: u64) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let reading = match conn.state {
            State::Connecting { .. } => true,
            State::Open => !conn.paused && !conn.shared.throttled.load(Ordering::Relaxed),
            State::Closing { .. } => false,
        };
        let writing = matches!(conn.state, State::Connecting { .. }) || !conn.out.is_empty();
        let want = match (reading, writing) {
            (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
            (true, false) => Some(Interest::READABLE),
            (false, true) => Some(Interest::WRITABLE),
            (false, false) => None,
        };
        if want == conn.interest {
            return;
        }
        let token = Token(id as usize);
        let registry = self.poll.registry();
        let ok = match (conn.interest, want) {
            (Some(_), Some(w)) => registry.reregister(&mut conn.stream, token, w),
            (None, Some(w)) => registry.register(&mut conn.stream, token, w),
            (Some(_), None) => registry.deregister(&mut conn.stream),
            (None, None) => Ok(()),
        };
        if ok.is_ok() {
            conn.interest = want;
        }
    }

    /// A socket is ready.
    fn ready(&mut self, id: u64, readable: bool, writable: bool) {
        match self.entries.get_mut(&id) {
            Some(Entry::Listener { .. }) => self.accept(id),
            Some(Entry::Udp(_)) => self.receive(id),
            Some(Entry::Conn(conn)) => {
                if let State::Connecting { .. } = conn.state {
                    if readable || writable {
                        self.connected(id);
                    }
                    return;
                }
                if writable {
                    self.flush(id);
                }
                if readable {
                    self.read(id);
                }
            }
            None => {}
        }
    }

    /// A connecting stream is ready: connected, or on to the next address.
    fn connected(&mut self, id: u64) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let error = match conn.stream.take_error() {
            Ok(Some(e)) => Some(e),
            Ok(None) => match conn.stream.peer_addr() {
                Ok(_) => None,
                Err(e) if e.kind() == io::ErrorKind::NotConnected || e.raw_os_error() == Some(EINPROGRESS) => return,
                Err(e) => Some(e),
            },
            Err(e) => Some(e),
        };
        match error {
            None => {
                conn.shared.connected(&conn.stream);
                conn.state = State::Open;
                post(Event::Socket(Note::Connected(id)));
                self.flush(id);
                self.read(id);
            }
            Some(e) => self.connect_next(id, e),
        }
    }

    /// Tries the next address of a connecting stream, or gives up with `e`.
    fn connect_next(&mut self, id: u64, e: io::Error) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let State::Connecting { addrs, .. } = &mut conn.state else { return };
        let _ = self.poll.registry().deregister(&mut conn.stream);
        conn.interest = None;
        while !addrs.is_empty() {
            let addr = addrs.remove(0);
            if let Ok(stream) = TcpStream::connect(addr) {
                conn.stream = stream;
                if self.poll.registry().register(&mut conn.stream, Token(id as usize), Interest::READABLE | Interest::WRITABLE).is_ok() {
                    conn.interest = Some(Interest::READABLE | Interest::WRITABLE);
                    return;
                }
            }
        }
        self.finish(id, sentence(&e));
    }

    fn accept(&mut self, id: u64) {
        loop {
            let Some(Entry::Listener { listener, http }) = self.entries.get_mut(&id) else { return };
            let http = *http;
            let (mut stream, peer) = match listener.accept() {
                Ok(a) => a,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                // (Out of descriptors, or a connection that went away.)
                Err(_) => return,
            };
            let cid = new_id();
            if self.poll.registry().register(&mut stream, Token(cid as usize), Interest::READABLE).is_err() {
                continue;
            }
            let shared = Shared::new(stream.local_addr().map(|a| a.port() as i64).unwrap_or(-1));
            *shared.peer.lock().unwrap() = peer.to_string();
            let conn = Conn {
                stream,
                shared: shared.clone(),
                state: State::Open,
                out: VecDeque::new(),
                sent: 0,
                paused: false,
                interest: Some(Interest::READABLE),
                listener: Some(id),
                peer: peer.to_string(),
                http: http.then(|| Http {
                    inbuf: Vec::new(),
                    pending: None,
                    close_after: false,
                    say_kept: false,
                    version_11: true,
                    streaming: None,
                    continued: false,
                }),
                ws: None,
            };
            self.entries.insert(cid, Entry::Conn(conn));
            if !http {
                post(Event::Socket(Note::Accepted { listener: id, id: cid, peer: peer.to_string(), shared }));
            }
        }
    }

    fn receive(&mut self, id: u64) {
        loop {
            let Some(Entry::Udp(socket)) = self.entries.get_mut(&id) else { return };
            match socket.recv_from(&mut self.buf) {
                Ok((n, from)) => {
                    let data = Bytes(crate::codec::data_new(&self.buf[..n]));
                    post(Event::Socket(Note::Packet { id, data, from: from.ip().to_string(), port: from.port() as i64 }));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                // (Windows reports a port unreachable answer to an earlier
                // send as a reset: not the end of the socket.)
                Err(e) if matches!(e.kind(), io::ErrorKind::Interrupted | io::ErrorKind::ConnectionReset) => {}
                Err(_) => return,
            }
        }
    }

    /// Writes what's queued, as far as the system takes it.
    fn flush(&mut self, id: u64) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        if matches!(conn.state, State::Connecting { .. }) {
            return;
        }
        let mut failed = None;
        while let Some(front) = conn.out.front() {
            match conn.stream.write(&front[conn.sent..]) {
                Ok(n) => {
                    conn.sent += n;
                    conn.shared.queued.fetch_sub(n as i64, Ordering::Relaxed);
                    if conn.sent >= front.len() {
                        conn.out.pop_front();
                        conn.sent = 0;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            self.finish(id, sentence(&e));
            return;
        }
        if matches!(conn.state, State::Closing { .. }) && conn.out.is_empty() {
            self.finish(id, "closed".into());
            return;
        }
        self.update_interest(id);
    }

    /// Reads what has arrived, a `Data` per read, until there's no more or
    /// the block has enough waiting.
    fn read(&mut self, id: u64) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        if !matches!(conn.state, State::Open) || conn.paused || conn.shared.throttled.load(Ordering::Relaxed) {
            self.update_interest(id);
            return;
        }
        let mut ended = None;
        let mut http_input = Vec::new();
        loop {
            match conn.stream.read(&mut self.buf) {
                Ok(0) => {
                    ended = Some("closed".to_string());
                    break;
                }
                Ok(n) => {
                    if conn.http.is_some() || conn.ws.is_some() {
                        http_input.extend_from_slice(&self.buf[..n]);
                        if http_input.len() > HEAD_LIMIT + BODY_LIMIT {
                            break;
                        }
                        continue;
                    }
                    let unread = conn.shared.unread.fetch_add(n as i64, Ordering::Relaxed) + n as i64;
                    // (Throttled before the piece is posted: the UI thread
                    // sees the flag when it takes the piece, and pokes.)
                    let throttle = unread >= UNREAD_LIMIT;
                    if throttle {
                        conn.shared.throttled.store(true, Ordering::Relaxed);
                    }
                    post(Event::Socket(Note::Data(id, Bytes(crate::codec::data_new(&self.buf[..n])))));
                    if throttle {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    ended = Some(sentence(&e));
                    break;
                }
            }
        }
        if !http_input.is_empty() {
            if conn.ws.is_some() {
                self.read_ws(id, http_input);
            } else {
                if let Some(http) = &mut conn.http {
                    http.inbuf.extend_from_slice(&http_input);
                }
                self.parse_http(id);
            }
        }
        if let Some(reason) = ended {
            self.finish(id, reason);
            return;
        }
        self.update_interest(id);
    }

    /// Hands the app the next complete request on an HTTP connection (one
    /// at a time: the next waits until it's answered).
    fn parse_http(&mut self, id: u64) {
        let Some(Entry::Conn(conn)) = self.entries.get_mut(&id) else { return };
        let Some(http) = &mut conn.http else { return };
        if http.pending.is_some() || !matches!(conn.state, State::Open) {
            return;
        }
        let parsed = match parse_request(&http.inbuf, HEAD_LIMIT, BODY_LIMIT) {
            Ok(Some(p)) => p,
            Ok(None) => {
                // Not all here yet. A client waiting to be told to send its
                // body ("Expect: 100-continue") is told once.
                if !http.continued && expects_continue(&http.inbuf) {
                    http.continued = true;
                    queue(conn, b"HTTP/1.1 100 Continue\r\n\r\n".to_vec());
                    self.flush(id);
                }
                return;
            }
            Err(status) => {
                conn.out.push_back(response_bytes(status, &[("Connection".into(), "close".into())], b"", false));
                conn.state = State::Closing { deadline: Instant::now() + CLOSE_GRACE };
                self.flush(id);
                return;
            }
        };
        http.inbuf.drain(..parsed.consumed);
        http.continued = false;
        let request = new_id();
        http.pending = Some(request);
        http.close_after = parsed.close;
        http.say_kept = parsed.keep_alive_10;
        http.version_11 = parsed.version_11;
        http.streaming = None;
        self.requests.insert(request, id);
        let (path, query) = match parsed.target.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (parsed.target, String::new()),
        };
        let listener = conn.listener.unwrap_or(0);
        let remote = conn.peer.clone();
        let shared = conn.shared.clone();
        post(Event::Socket(Note::Request {
            listener,
            request: Box::new(Request {
                id: request,
                method: parsed.method,
                path,
                query,
                headers: parsed.headers,
                body: parsed.body,
                remote,
                conn: id,
                shared,
            }),
        }));
    }
}

#[cfg(unix)]
const EINPROGRESS: i32 = libc::EINPROGRESS;
#[cfg(windows)]
const EINPROGRESS: i32 = 10036; // WSAEINPROGRESS
#[cfg(not(any(unix, windows)))]
const EINPROGRESS: i32 = -1;

// ===== HTTP parsing =====

#[derive(Debug)]
struct Parsed {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// How much of the input the request took.
    consumed: usize,
    /// The connection closes after the answer.
    close: bool,
    /// An HTTP/1.0 request that asked to keep the connection.
    keep_alive_10: bool,
    /// An HTTP/1.1 request.
    version_11: bool,
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Does the request being read ask for "100 Continue"?
fn expects_continue(input: &[u8]) -> bool {
    let Some(end) = find(input, b"\r\n\r\n", 0) else { return false };
    let head = String::from_utf8_lossy(&input[..end]).to_ascii_lowercase();
    head.lines().skip(1).any(|l| l.trim_start().starts_with("expect:") && l.contains("100-continue"))
}

/// A complete request at the start of `input`: `None` if it isn't all there
/// yet; the status to answer with (400, 413, 431) if it's wrong.
fn parse_request(input: &[u8], head_limit: usize, body_limit: usize) -> Result<Option<Parsed>, i64> {
    let Some(end) = find(input, b"\r\n\r\n", 0) else {
        return if input.len() > head_limit { Err(431) } else { Ok(None) };
    };
    if end > head_limit {
        return Err(431);
    }
    let head = std::str::from_utf8(&input[..end]).map_err(|_| 400)?;
    let mut lines = head.split("\r\n");
    let line = lines.next().ok_or(400)?;
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next().ok_or(400)?, parts.next().ok_or(400)?, parts.next().ok_or(400)?);
    if method.is_empty() || target.is_empty() || !version.starts_with("HTTP/1") {
        return Err(400);
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400)?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name.is_empty() {
            return Err(400);
        }
        match headers.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                v.push_str(", ");
                v.push_str(&value);
            }
            None => headers.push((name, value)),
        }
    }
    let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    let body_start = end + 4;
    let chunked = header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let (body, consumed) = if chunked {
        match chunked_body(&input[body_start..], body_limit)? {
            Some((body, used)) => (body, body_start + used),
            None => return Ok(None),
        }
    } else if let Some(length) = header("content-length") {
        let length: usize = length.trim().parse().map_err(|_| 400)?;
        if length > body_limit {
            return Err(413);
        }
        if input.len() < body_start + length {
            return Ok(None);
        }
        (input[body_start..body_start + length].to_vec(), body_start + length)
    } else {
        (Vec::new(), body_start)
    };
    let connection = header("connection").map(|v| v.to_ascii_lowercase()).unwrap_or_default();
    let close = connection.contains("close") || (version == "HTTP/1.0" && !connection.contains("keep-alive"));
    let keep_alive_10 = version == "HTTP/1.0" && !close;
    let version_11 = version != "HTTP/1.0";
    Ok(Some(Parsed { method: method.to_string(), target: target.to_string(), headers, body, consumed, close, keep_alive_10, version_11 }))
}

/// A chunked body at the start of `input`, and how much it took: `None`
/// if it isn't complete.
fn chunked_body(input: &[u8], limit: usize) -> Result<Option<(Vec<u8>, usize)>, i64> {
    let mut body = Vec::new();
    let mut at = 0;
    loop {
        let Some(line_end) = find(input, b"\r\n", at) else { return Ok(None) };
        let line = std::str::from_utf8(&input[at..line_end]).map_err(|_| 400)?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| 400)?;
        at = line_end + 2;
        if size == 0 {
            // Trailers (if any) end with an empty line.
            loop {
                let Some(e) = find(input, b"\r\n", at) else { return Ok(None) };
                let empty = e == at;
                at = e + 2;
                if empty {
                    return Ok(Some((body, at)));
                }
            }
        }
        if body.len() + size > limit {
            return Err(413);
        }
        if input.len() < at + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&input[at..at + size]);
        at += size + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        let input = b"GET /a?b=1 HTTP/1.1\r\nHost: x\r\nX-A: 1\r\nx-a: 2\r\n\r\nGET / HTTP/1.1\r\n\r\n";
        let p = parse_request(input, 1 << 16, 1 << 20).unwrap().unwrap();
        assert_eq!(p.method, "GET");
        assert_eq!(p.target, "/a?b=1");
        assert_eq!(p.headers, vec![("host".to_string(), "x".to_string()), ("x-a".to_string(), "1, 2".to_string())]);
        assert!(p.body.is_empty());
        assert!(!p.close);
        assert_eq!(&input[p.consumed..], b"GET / HTTP/1.1\r\n\r\n");

        let input = b"POST /p HTTP/1.0\r\nContent-Length: 5\r\n\r\nhel";
        assert!(parse_request(input, 1 << 16, 1 << 20).unwrap().is_none());
        let input = b"POST /p HTTP/1.0\r\nContent-Length: 5\r\n\r\nhello!";
        let p = parse_request(input, 1 << 16, 1 << 20).unwrap().unwrap();
        assert_eq!(p.body, b"hello");
        assert!(p.close);
        assert_eq!(p.consumed, input.len() - 1);

        let input = b"POST /c HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\nX";
        let p = parse_request(input, 1 << 16, 1 << 20).unwrap().unwrap();
        assert_eq!(p.body, b"abcde");
        assert_eq!(&input[p.consumed..], b"X");
        let input = b"POST /c HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n";
        assert!(parse_request(input, 1 << 16, 1 << 20).unwrap().is_none());

        assert_eq!(parse_request(b"nonsense\r\n\r\n", 1 << 16, 1 << 20).unwrap_err(), 400);
        assert_eq!(parse_request(b"POST / HTTP/1.1\r\nContent-Length: 99\r\n\r\n", 1 << 16, 10).unwrap_err(), 413);
        assert_eq!(parse_request(&vec![b'a'; 100], 50, 10).unwrap_err(), 431);
    }

    #[test]
    fn responses() {
        let r = response_bytes(200, &[("X-A".into(), "1".into())], b"hi", false);
        assert_eq!(r, b"HTTP/1.1 200 OK\r\nX-A: 1\r\nContent-Length: 2\r\n\r\nhi");
        let r = response_bytes(404, &[], b"gone", true);
        assert_eq!(r, b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\n");
        let r = response_bytes(204, &[("Content-Length".into(), "0".into())], b"", false);
        assert_eq!(r, b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    }
}
