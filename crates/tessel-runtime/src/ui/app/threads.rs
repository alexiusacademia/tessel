//! Threads that run Tessel code besides the main one: the workers of a
//! `serveHTTP(…, workers:)` server.
//!
//! Each thread that runs Tessel code has a mailbox: the results of the
//! work it started (timers it set, requests it sent, sockets it opened,
//! `background` work, commands) come to it there, and its own loop runs
//! their blocks. The main thread's mailbox is the app's (or `fn main`'s)
//! event queue. An id made on a worker is remembered as that worker's, so
//! the event about it goes to its mailbox; everything else goes to the
//! main thread, as before workers existed.
//!
//! A worker that hits a runtime error doesn't stop the program: its
//! requests get a 500, the locks of `Shared` values it held are let go,
//! another worker takes its place, and the thread waits for ever (a
//! runtime error doesn't unwind, so what it held stays held; the program
//! goes on without it).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Duration;

use super::sockets::{self, Note};
use super::Event;
use crate::ui::Callback;

/// Where a thread's events wait for its loop.
pub(super) struct Mailbox {
    events: Mutex<Vec<Event>>,
    posted: Mutex<bool>,
    cv: Condvar,
}

impl Mailbox {
    fn new() -> Arc<Mailbox> {
        Arc::new(Mailbox { events: Mutex::new(Vec::new()), posted: Mutex::new(false), cv: Condvar::new() })
    }

    /// Adds an event, and wakes the thread if it waits.
    pub(super) fn put(&self, event: Event) {
        self.events.lock().unwrap().push(event);
        *self.posted.lock().unwrap() = true;
        self.cv.notify_all();
    }

    /// The events that came, oldest first.
    pub(super) fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    fn waiting(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Waits until an event comes, or `limit` passes.
    pub(super) fn wait(&self, limit: Duration) {
        let mut p = self.posted.lock().unwrap();
        if !*p {
            p = self.cv.wait_timeout(p, limit).unwrap().0;
        }
        *p = false;
    }
}

/// The main thread's mailbox.
pub(super) static MAIN: LazyLock<Arc<Mailbox>> = LazyLock::new(Mailbox::new);

thread_local! {
    /// A worker's own mailbox (none on the main thread).
    static THIS: RefCell<Option<Arc<Mailbox>>> = const { RefCell::new(None) };
    /// A worker: its server and its place in the pool.
    static WORKER: RefCell<Option<(u64, Arc<Worker>)>> = const { RefCell::new(None) };
}

/// The calling thread's mailbox.
pub(super) fn this_box() -> Arc<Mailbox> {
    THIS.with(|t| t.borrow().clone()).unwrap_or_else(|| MAIN.clone())
}

/// Ids made on workers, and the mailbox of the worker each belongs to:
/// timers, loads, commands and jobs here, sockets (which count on their
/// own) in `SOCKET_OWNERS`.
static OWNERS: LazyLock<Mutex<HashMap<u64, Arc<Mailbox>>>> = LazyLock::new(Default::default);
static SOCKET_OWNERS: LazyLock<Mutex<HashMap<u64, Arc<Mailbox>>>> = LazyLock::new(Default::default);

/// Remembers `id` as the calling thread's, if it's a worker.
pub(super) fn own(id: u64) {
    if let Some(mb) = THIS.with(|t| t.borrow().clone()) {
        OWNERS.lock().unwrap().insert(id, mb);
    }
}

/// Remembers the socket `id` as the calling thread's, if it's a worker.
pub(super) fn own_socket(id: u64) {
    if let Some(mb) = THIS.with(|t| t.borrow().clone()) {
        SOCKET_OWNERS.lock().unwrap().insert(id, mb);
    }
}

fn owner(id: u64) -> Option<Arc<Mailbox>> {
    OWNERS.lock().unwrap().get(&id).cloned()
}

fn disown(id: u64) {
    OWNERS.lock().unwrap().remove(&id);
}

fn socket_owner(id: u64) -> Option<Arc<Mailbox>> {
    SOCKET_OWNERS.lock().unwrap().get(&id).cloned()
}

fn socket_disown(id: u64) {
    SOCKET_OWNERS.lock().unwrap().remove(&id);
}

/// Sends an event to the thread it belongs to. Returns it if it's the
/// main thread's.
pub(super) fn route(event: Event) -> Option<Event> {
    // Nothing runs on workers: everything is the main thread's.
    if OWNERS.lock().unwrap().is_empty() && SOCKET_OWNERS.lock().unwrap().is_empty() && POOLS.lock().unwrap().is_empty() {
        return Some(event);
    }
    if let Event::Socket(_) = event {
        let Event::Socket(note) = event else { unreachable!() };
        return route_note(note);
    }
    let (id, last) = match &event {
        Event::Fetched(id, _) | Event::Exited(id, _) | Event::Computed(id, _) | Event::Responded(id, _) => (*id, true),
        Event::Downloaded(id, _) | Event::Closed(id, _) => (*id, true),
        Event::Line(id, _)
        | Event::Headers(id, _)
        | Event::Chunk(id)
        | Event::Progress(id, _)
        | Event::Message(id, _)
        | Event::Bytes(id, _)
        | Event::JobProgress(id) => (*id, false),
        Event::Socket(_) | Event::Call(_) | Event::Stop => return Some(event),
    };
    let Some(mb) = owner(id) else { return Some(event) };
    if last {
        disown(id);
    }
    mb.put(event);
    None
}

fn route_note(note: Note) -> Option<Event> {
    let target = match &note {
        Note::Request { listener, request } => {
            // A server with workers: to the one that's freest.
            if let Some(mb) = pick_worker(*listener) {
                SOCKET_OWNERS.lock().unwrap().insert(request.id, mb.clone());
                mb.put(Event::Socket(note));
                return None;
            }
            let mb = socket_owner(*listener);
            if let Some(mb) = &mb {
                SOCKET_OWNERS.lock().unwrap().insert(request.id, mb.clone());
            }
            mb
        }
        Note::Accepted { listener, id, .. } => {
            let mb = socket_owner(*listener);
            if let Some(mb) = &mb {
                SOCKET_OWNERS.lock().unwrap().insert(*id, mb.clone());
            }
            mb
        }
        Note::Unanswered(request) => {
            let mb = socket_owner(*request);
            socket_disown(*request);
            mb
        }
        Note::Closed(id, _) => {
            let mb = socket_owner(*id);
            socket_disown(*id);
            mb
        }
        Note::Connected(id) | Note::Data(id, _) | Note::Resolved(id, _) => socket_owner(*id),
        Note::WsText(id, _) | Note::WsBinary(id, _) => socket_owner(*id),
        Note::Packet { id, .. } => socket_owner(*id),
    };
    match target {
        Some(mb) => {
            mb.put(Event::Socket(note));
            None
        }
        None => Some(Event::Socket(note)),
    }
}

// ===== worker pools =====

/// Makes a worker's request block, on the worker's thread: the compiled
/// `start:` block, given its values (which it reads, and leaves for the
/// next worker), sets the block's function and environment (a
/// `fn(ServerRequest)`, adapted for the runtime).
pub type StartFn = unsafe extern "C" fn(input: *mut u8, f: *mut *const std::ffi::c_void, env: *mut *mut crate::Header);

/// One worker thread.
pub(super) struct Worker {
    mailbox: Arc<Mailbox>,
    /// Running a block now.
    busy: AtomicBool,
}

struct Pool {
    workers: Vec<Arc<Worker>>,
    start: StartFn,
    /// The `start:` block's values (never freed: a worker that fails is
    /// replaced from them).
    input: usize,
    /// For taking turns among idle workers.
    turn: usize,
}

/// Servers with workers, by listener id.
static POOLS: LazyLock<Mutex<HashMap<u64, Pool>>> = LazyLock::new(Default::default);

/// Starts `count` workers for the server `listener`.
pub(super) fn start_pool(listener: u64, count: usize, start: StartFn, input: *mut u8) {
    let input = input as usize;
    let workers = (0..count).map(|_| spawn_worker(listener, start, input)).collect();
    POOLS.lock().unwrap().insert(listener, Pool { workers, start, input, turn: 0 });
}

/// Stops the workers of `listener`'s server (their loops end when they've
/// run what they have).
pub(super) fn stop_pool(listener: u64) {
    if let Some(pool) = POOLS.lock().unwrap().remove(&listener) {
        for w in pool.workers {
            w.mailbox.put(Event::Stop);
        }
    }
}

/// The mailbox of the worker a new request of `listener` goes to: an idle
/// one in turn, or else the one with the fewest waiting.
fn pick_worker(listener: u64) -> Option<Arc<Mailbox>> {
    let mut pools = POOLS.lock().unwrap();
    let pool = pools.get_mut(&listener)?;
    let n = pool.workers.len();
    if n == 0 {
        return None;
    }
    for k in 0..n {
        let w = &pool.workers[(pool.turn + k) % n];
        if !w.busy.load(Ordering::Relaxed) && w.mailbox.waiting() == 0 {
            pool.turn = (pool.turn + k + 1) % n;
            return Some(w.mailbox.clone());
        }
    }
    pool.workers.iter().min_by_key(|w| w.mailbox.waiting()).map(|w| w.mailbox.clone())
}

fn spawn_worker(listener: u64, start: StartFn, input: usize) -> Arc<Worker> {
    let worker = Arc::new(Worker { mailbox: Mailbox::new(), busy: AtomicBool::new(true) });
    let w = worker.clone();
    std::thread::Builder::new()
        .name("tessel-worker".into())
        .spawn(move || run_worker(listener, w, start, input))
        .expect("can't start a worker thread");
    worker
}

thread_local! {
    /// The worker's loop ends (its server stopped).
    static STOPPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `Event::Stop`: the worker's server stopped.
pub(super) fn stop_this() {
    STOPPING.with(|s| s.set(true));
}

fn run_worker(listener: u64, worker: Arc<Worker>, start: StartFn, input: usize) {
    crate::stack_guard::thread_init();
    THIS.with(|t| *t.borrow_mut() = Some(worker.mailbox.clone()));
    WORKER.with(|w| *w.borrow_mut() = Some((listener, worker.clone())));
    let mut f: *const std::ffi::c_void = std::ptr::null();
    let mut env: *mut crate::Header = std::ptr::null_mut();
    unsafe { start(input as *mut u8, &mut f, &mut env) };
    sockets::serve_here(listener, Callback::new(f, env));
    loop {
        worker.busy.store(true, Ordering::Relaxed);
        super::fire_timers();
        super::deliver_background();
        worker.busy.store(false, Ordering::Relaxed);
        if STOPPING.with(|s| s.get()) {
            break;
        }
        let wait = super::next_timer().map_or(0.05, |t| (t - super::now()).clamp(0.0, 0.05));
        worker.mailbox.wait(Duration::from_secs_f64(wait));
    }
    WORKER.with(|w| *w.borrow_mut() = None);
}

/// A runtime error on a worker (called after its message is printed):
/// the worker's requests get a 500, the `Shared` locks it holds are let
/// go, another worker takes its place, and this thread waits for ever.
/// Returns at once on a thread that isn't a worker.
pub(crate) fn worker_failed() {
    let Some((listener, worker)) = WORKER.with(|w| w.borrow().clone()) else { return };
    sockets::fail_waiting();
    crate::shared::let_go();
    // Its place, and what waited for it, go to a new worker.
    let undelivered = worker.mailbox.take();
    {
        let mut pools = POOLS.lock().unwrap();
        if let Some(pool) = pools.get_mut(&listener) {
            pool.workers.retain(|w| !Arc::ptr_eq(w, &worker));
            let fresh = spawn_worker(listener, pool.start, pool.input);
            pool.workers.push(fresh);
        }
    }
    OWNERS.lock().unwrap().retain(|_, mb| !Arc::ptr_eq(mb, &worker.mailbox));
    SOCKET_OWNERS.lock().unwrap().retain(|_, mb| !Arc::ptr_eq(mb, &worker.mailbox));
    for event in undelivered {
        if let Event::Socket(note @ Note::Request { .. }) = event {
            if let Some(e) = route_note(note) {
                super::post_main(e);
            }
        }
    }
    let _ = std::io::Write::flush(&mut std::io::stderr());
    loop {
        std::thread::park();
    }
}
