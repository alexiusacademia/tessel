//! The app: window, input events, timers, and the rebuild → layout → draw cycle.
//!
//! Setting `TESSEL_SCRIPT` runs the app without a window instead, driven by a
//! script (used by tests). Commands, separated by `;` or newlines:
//!
//! ```text
//! size 320 200          window size in points
//! click "Add"           click the button / toggle / text / field with this label
//! click 40 60           click at a point
//! close "main.tsl"      click the close button of the tab with this title
//! click toggle 2        click the 2nd view of a kind (toggle, button, field,
//!                       text, editor, slider, stepper, colorpicker)
//! type hello world      type into the text field or code editor being edited
//! type "two words "     quoted, to keep leading or trailing spaces
//! key backspace         a key, optionally with modifiers: `key shift+left`,
//!                       `key alt+backspace`, `key cmd+a` (cmd means Ctrl on
//!                       Windows; `control` is the Ctrl key itself, and
//!                       `repeat` makes it a key held down). Keys: backspace
//!                       delete left right up down home end pageup pagedown
//!                       enter tab escape space f1 … f12, and characters
//!                       (`key a` types "a" in a field, `key shift+a` "A")
//! hover "Add"           move the pointer over a labelled view (or `hover 40 60`)
//! answer "/tmp/a.och"   what the next openFile/saveFile dialog returns
//!                       (`answer cancel` for cancelling it)
//! drag 300 40 -80 0     press at a point, and move by (dx, dy) with the
//!                       button held (for views with `.onDrag`, and sliders)
//! dragto "A" "B"        drag the view with `.draggable` at A onto B (labels,
//!                       or `dragto 40 60 40 200`), and let go
//! scroll 0 120          scroll the view under the middle of the window by
//!                       (dx, dy) points
//! wheel 40 60 0 120     the same, at a point
//! press 40 60           press the mouse on a view with `.onPointer` (add
//!                       `right` or `middle` for another button) or
//!                       `.draggable` (or `press "Label"`), then
//! move 80 60            move it there (a drag while pressed), and
//! release               let go; `click`, `drag` and `hover` work on such
//!                       views too, and so do
//! rightclick 40 60      a click with the right button,
//! doubleclick 40 60     two clicks in a row, and
//! pinch 40 60 0.2       a trackpad pinch (0.2: a fifth larger)
//! mods shift+alt        the modifier keys held for these (`mods none`)
//! drop "/tmp/a.shp"     drop a file on the window (for `.onDrop`)
//! wait 1.5              let 1.5 seconds pass (fires timers); first waits
//!                       for loads, commands and background work to finish
//! repeat 20 key down    run a command several times
//! snapshot out.png      render to a PNG (at 2x)
//! compose "ni"          text an input method is composing (not yet typed);
//!                       `type` then commits what it became
//! reload                with `tessel run --hot`: take the program's new code
//!                       now, if its files changed
//! appearance dark       switch to the dark (or `light`) appearance
//! animations on         animate as a window does (headless runs don't,
//!                       unless asked); `wait` then shows views on their way
//! dump                  print the laid-out view tree, with positions
//! tree                  print the view tree without positions
//! ```

mod net;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use super::edit::{self, Clipboard, Edit, Editing, Unit};
use super::layout::{read_string, Layouter, Rect};
use super::menu::{self, Command, Entry, Std};
use super::render::{self, code_origin, field_origin, Interaction, Painter, TextEngine};
use super::theme::{self, theme};
use super::{build_all, Built, Callback, Kind, Node, Rgba};
use crate::{Header, Str};

// ===== timers =====

struct Timer {
    due: f64,
    action: Callback,
    /// `every(seconds:)`: the interval it repeats at.
    repeat: Option<f64>,
    id: i64,
}

thread_local! {
    static TIMERS: RefCell<Vec<Timer>> = const { RefCell::new(Vec::new()) };
    /// Repeating timers stopped while timers are firing.
    static STOPPED: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    static CLOCK: RefCell<Clock> = RefCell::new(Clock::Real(Instant::now()));
}

enum Clock {
    Real(Instant),
    /// Headless runs advance time only with `wait`.
    Virtual(f64),
}

fn now() -> f64 {
    CLOCK.with(|c| match &*c.borrow() {
        Clock::Real(start) => start.elapsed().as_secs_f64(),
        Clock::Virtual(t) => *t,
    })
}

/// `after(seconds:) { … }`: runs `action` once, after a delay.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_after(seconds: f64, f: *const c_void, env: *mut Header) {
    let action = Callback::new(f, env);
    let due = now() + seconds.max(0.0);
    TIMERS.with(|t| t.borrow_mut().push(Timer { due, action, repeat: None, id: 0 }));
}

/// `every(seconds:) { … }`: runs `action` again and again; returns an id for `stopTimer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_every(seconds: f64, f: *const c_void, env: *mut Header) -> i64 {
    let action = Callback::new(f, env);
    let interval = seconds.max(0.001);
    let id = next_id() as i64;
    TIMERS.with(|t| t.borrow_mut().push(Timer { due: now() + interval, action, repeat: Some(interval), id }));
    id
}

/// `stopTimer(id)`: stops a repeating timer (nothing happens if it's gone).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_stop_timer(id: i64) {
    TIMERS.with(|t| t.borrow_mut().retain(|x| x.id != id || x.repeat.is_none()));
    STOPPED.with(|s| s.borrow_mut().push(id));
}

/// Runs timers that are due. Returns whether any ran.
fn fire_timers() -> bool {
    let t = now();
    let due: Vec<Timer> = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        let (due, later): (Vec<Timer>, Vec<Timer>) = timers.drain(..).partition(|x| x.due <= t);
        *timers = later;
        due
    });
    let fired = !due.is_empty();
    STOPPED.with(|s| s.borrow_mut().clear());
    for timer in due {
        timer.action.call();
        // A repeating timer comes back, unless its own action stopped it.
        if let Some(interval) = timer.repeat {
            if !STOPPED.with(|s| s.borrow().contains(&timer.id)) {
                let next = Timer { due: timer.due + interval, action: timer.action, repeat: timer.repeat, id: timer.id };
                TIMERS.with(|t| t.borrow_mut().push(next));
            }
        }
    }
    fired
}

fn next_timer() -> Option<f64> {
    TIMERS.with(|t| t.borrow().iter().map(|x| x.due).fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.min(d)))))
}

// ===== background work: fetch and running commands =====

/// Results of background work, handed from worker threads to the UI thread.
enum Event {
    Fetched(u64, Option<String>),
    Line(u64, String),
    Exited(u64, i32),
    Responded(u64, net::HttpResult),
    Progress(u64, f64),
    Downloaded(u64, bool),
    Message(u64, String),
    /// A binary WebSocket message.
    Bytes(u64, Vec<u8>),
    Closed(u64, String),
    /// `background` work finished: its result (a codec tree, as an address).
    Computed(u64, usize),
    /// `background` work reported progress (see `JobShared::latest`).
    JobProgress(u64),
    /// A function to call on the UI thread (its address): see `run_on_ui`.
    Call(usize),
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
/// Wakes the window's event loop when background work produces something.
static WAKE: Mutex<Option<EventLoopProxy<()>>> = Mutex::new(None);
/// Running commands, so `stopCommand` can stop them.
static CHILDREN: Mutex<Vec<(u64, Arc<Mutex<std::process::Child>>)>> = Mutex::new(Vec::new());

thread_local! {
    /// Completion blocks waiting for their loads, by id.
    static WAITING: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
    /// `onOutput` and `onExit` blocks of running commands, by id.
    static COMMANDS: RefCell<HashMap<u64, (Callback, Callback)>> = RefCell::new(HashMap::new());
    /// `background` work still running: the blocks that get its result and
    /// its progress.
    static COMPUTING: RefCell<HashMap<u64, (Callback, Option<Callback>)>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        n.set(n.get() + 1);
        n.get()
    })
}

fn post(event: Event) {
    EVENTS.lock().unwrap().push(event);
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// Loads `http(s)://…` or `file://…` as text; `None` if it fails.
fn load(url: &str) -> Option<String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read_to_string(path).ok();
    }
    let mut response = ureq::get(url).call().ok()?;
    let mut body = String::new();
    response.body_mut().as_reader().read_to_string(&mut body).ok()?;
    Some(body)
}

/// `fetch(url:) { text in … }`: loads `url` in the background, then calls
/// `done(text or null)` on the UI thread. `done` is `void fn(env, ptr)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_fetch(url: *const Str, f: *const c_void, env: *mut Header) {
    let url = unsafe { crate::str_text(url) }.to_string();
    let id = next_id();
    WAITING.with(|w| w.borrow_mut().insert(id, Callback::new(f, env)));
    std::thread::spawn(move || post(Event::Fetched(id, load(&url))));
}

/// `runCommand(path:args:onOutput:onExit:)`: starts a program. Each line it
/// prints (stdout and stderr) goes to `onOutput` (`void fn(env, String)`),
/// then its exit code to `onExit` (`void fn(env, Int)`), on the UI thread.
/// Returns an id for `stopCommand`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_run_command(
    path: *const Str,
    args: *const crate::List,
    out_f: *const c_void,
    out_env: *mut Header,
    exit_f: *const c_void,
    exit_env: *mut Header,
) -> i64 {
    let path = unsafe { crate::str_text(path) }.to_string();
    let n = unsafe { crate::tsl_list_len(args) };
    let args: Vec<String> = (0..n)
        .map(|i| unsafe {
            let p = crate::tsl_list_get(args, i, std::ptr::null()) as *const *const Str;
            crate::str_text(*p).to_string()
        })
        .collect();
    let id = next_id();
    COMMANDS.with(|c| c.borrow_mut().insert(id, (Callback::new(out_f, out_env), Callback::new(exit_f, exit_env))));
    let spawned = std::process::Command::new(&path)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            post(Event::Line(id, format!("error: can't start `{path}`: {e}")));
            post(Event::Exited(id, -1));
            return id as i64;
        }
    };
    let readers: Vec<_> = [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|stream| {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stream);
            let mut line = Vec::new();
            while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                let text = String::from_utf8_lossy(&line);
                post(Event::Line(id, text.trim_end_matches(['\n', '\r']).to_string()));
                line.clear();
            }
        })
    })
    .collect();
    let child = Arc::new(Mutex::new(child));
    CHILDREN.lock().unwrap().push((id, child.clone()));
    std::thread::spawn(move || {
        // All output first, then the exit code.
        for r in readers {
            let _ = r.join();
        }
        let code = child.lock().unwrap().wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        CHILDREN.lock().unwrap().retain(|(i, _)| *i != id);
        post(Event::Exited(id, code));
    });
    id as i64
}

/// `stopCommand(id)`: stops a running command (its `onExit` still runs).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_stop_command(id: i64) {
    let child = CHILDREN.lock().unwrap().iter().find(|(i, _)| *i == id as u64).map(|(_, c)| c.clone());
    if let Some(c) = child {
        let _ = c.lock().unwrap().kill();
    }
}

/// `background(work:progress:) { result in … }`: runs `worker(input, id)`
/// on another thread, then `done(result)` on the UI thread. `input` and the
/// result are codec trees, so the two threads share no values: the worker
/// owns (and frees) `input`, and `done` owns the result. Returns the job's
/// id, for `BackgroundJob`'s methods.
///
/// The work reports progress with `tsl_job_progress`; the `progress` block
/// (`void fn(env, Float, String)`, or null) gets the latest on the UI thread,
/// at most once per pass of the event loop, and always before `done`. After
/// `tsl_job_cancel`, neither block runs again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_background(
    worker: extern "C" fn(*mut u8, i64) -> *mut u8,
    input: *mut u8,
    done_f: *const std::ffi::c_void,
    done_env: *mut crate::Header,
    progress_f: *const std::ffi::c_void,
    progress_env: *mut crate::Header,
) -> i64 {
    let id = next_id();
    let progress = (!progress_f.is_null()).then(|| Callback::new(progress_f, progress_env));
    COMPUTING.with(|c| c.borrow_mut().insert(id, (Callback::new(done_f, done_env), progress)));
    JOBS.lock().unwrap().insert(id, JobShared::default());
    let input = input as usize;
    std::thread::spawn(move || {
        crate::stack_guard::thread_init();
        let result = worker(input as *mut u8, id as i64);
        post(Event::Computed(id, result as usize));
    });
    id as i64
}

/// What the UI thread and a `background` job's thread share about it.
#[derive(Default)]
struct JobShared {
    cancelled: bool,
    /// The latest progress the work reported, not yet given to its block.
    latest: Option<(f64, String)>,
}

/// Running `background` jobs, by id.
static JOBS: std::sync::LazyLock<Mutex<HashMap<u64, JobShared>>> = std::sync::LazyLock::new(Default::default);

/// `job.progress(fraction, note:)`: called by the work, on its thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_job_progress(id: i64, fraction: f64, note: *const Str) {
    let note = unsafe { crate::str_text(note) }.to_string();
    let mut jobs = JOBS.lock().unwrap();
    let Some(job) = jobs.get_mut(&(id as u64)) else { return };
    // One event at a time: until the UI thread takes it, newer progress
    // just replaces what waits.
    let first = job.latest.is_none();
    job.latest = Some((if fraction.is_finite() { fraction } else { 0.0 }, note));
    drop(jobs);
    if first {
        post(Event::JobProgress(id as u64));
    }
}

/// `job.isCancelled()`: whether `job.cancel()` was called.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_job_is_cancelled(id: i64) -> u8 {
    JOBS.lock().unwrap().get(&(id as u64)).is_some_and(|j| j.cancelled) as u8
}

/// `job.cancel()`: the work sees it with `isCancelled()`, and its blocks
/// (progress and done) don't run any more. Nothing happens if it's finished.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_job_cancel(id: i64) {
    if let Some(job) = JOBS.lock().unwrap().get_mut(&(id as u64)) {
        job.cancelled = true;
        job.latest = None;
    }
}

/// The progress a job reported and its block hasn't had yet (none once
/// it's cancelled).
fn take_progress(id: u64) -> Option<(f64, String)> {
    let mut jobs = JOBS.lock().unwrap();
    let job = jobs.get_mut(&id)?;
    let latest = job.latest.take();
    latest.filter(|_| !job.cancelled)
}

/// Runs the blocks for finished background work. Returns whether any ran.
fn deliver_background() -> bool {
    let events: Vec<Event> = std::mem::take(&mut *EVENTS.lock().unwrap());
    let ran = !events.is_empty();
    for event in events {
        match event {
            Event::Fetched(id, body) => {
                let Some(cb) = WAITING.with(|w| w.borrow_mut().remove(&id)) else { continue };
                let text = body.map_or(std::ptr::null_mut(), |b| crate::str_new(&b));
                cb.call_with(text as *mut u8);
            }
            Event::Line(id, line) => {
                let cb = COMMANDS.with(|c| c.borrow().get(&id).map(|(out, _)| out.clone()));
                if let Some(cb) = cb {
                    cb.call_with(crate::str_new(&line) as *mut u8);
                }
            }
            Event::Exited(id, code) => {
                if let Some((_, on_exit)) = COMMANDS.with(|c| c.borrow_mut().remove(&id)) {
                    on_exit.call_with_int(code as i64);
                }
            }
            Event::JobProgress(id) => {
                let cb = COMPUTING.with(|c| c.borrow().get(&id).and_then(|(_, p)| p.clone()));
                if let (Some(cb), Some((fraction, note))) = (cb, take_progress(id)) {
                    cb.call_with_float_ptr(fraction, crate::str_new(&note) as *mut u8);
                }
            }
            Event::Computed(id, tree) => {
                // The last progress first, then the result (unless it was cancelled).
                let latest = take_progress(id);
                // (A cancelled job is remembered, so `isCancelled()` stays true.)
                let cancelled = {
                    let mut jobs = JOBS.lock().unwrap();
                    let cancelled = jobs.get(&id).is_none_or(|j| j.cancelled);
                    if !cancelled {
                        jobs.remove(&id);
                    }
                    cancelled
                };
                match COMPUTING.with(|c| c.borrow_mut().remove(&id)) {
                    Some((done, progress)) if !cancelled => {
                        if let (Some(cb), Some((fraction, note))) = (progress, latest) {
                            cb.call_with_float_ptr(fraction, crate::str_new(&note) as *mut u8);
                        }
                        done.call_with(tree as *mut u8);
                    }
                    _ => unsafe { crate::codec::tsl_tree_free(tree as *mut _) },
                }
            }
            Event::Call(address) => {
                let f: extern "C" fn() = unsafe { std::mem::transmute(address) };
                f();
            }
            other => net::deliver(other),
        }
    }
    ran
}

// ===== hot reload =====
//
// `tessel run --hot` runs the program inside `tessel`, and compiles it again
// when its files change. The new code's entry point calls `tsl_ui_hot`, which
// hands the running app its new root view; the next rebuild uses it. View
// state is kept by the runtime, so it carries over (see `tsl_ui_view_begin`).

thread_local! {
    /// Whether this thread's app is running.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    /// A reloaded program's root view, until the app takes it.
    static NEW_ROOT: RefCell<Option<Callback>> = const { RefCell::new(None) };
}

/// Compiles the program again if its files changed; the address of the new
/// entry point, or 0.
static RELOAD: std::sync::OnceLock<fn() -> usize> = std::sync::OnceLock::new();

/// For the `reload` script command: how to get the program's new code.
pub fn on_reload(reload: fn() -> usize) {
    let _ = RELOAD.set(reload);
}

/// Calls the function at `address` (an `extern "C" fn()`) on the UI thread,
/// soon. Can be called from any thread.
pub fn run_on_ui(address: usize) {
    post(Event::Call(address));
}

/// Like `tsl_ui_run`, for a program run with hot reload: the first time it
/// starts the app (and never returns); when the app is running already, it
/// gives it `build` as its new root view, and returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_hot(
    title: *const Str,
    width: f64,
    height: f64,
    appearance: i32,
    f: *const c_void,
    env: *mut Header,
) {
    if RUNNING.with(Cell::get) {
        NEW_ROOT.with(|r| *r.borrow_mut() = Some(Callback::new(f, env)));
        return;
    }
    HOT.with(|h| h.set(true));
    unsafe { tsl_ui_run(title, width, height, appearance, f, env) }
}

// ===== file dialogs =====

thread_local! {
    /// Headless runs: what the next dialogs return (`answer` script command).
    static DIALOG_ANSWERS: RefCell<std::collections::VecDeque<Option<String>>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
}

/// `openFile(title:types:)` (`save` = 0) and `saveFile(title:name:types:)`
/// (`save` = 1): a native dialog; the chosen path, or null if cancelled.
/// `types` are file extensions without the dot, like `["och"]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_file_dialog(
    save: u8,
    title: *const Str,
    name: *const Str,
    types: *const crate::List,
) -> *mut Str {
    let title = unsafe { crate::str_text(title) }.to_string();
    let name = unsafe { crate::str_text(name) }.to_string();
    let n = unsafe { crate::tsl_list_len(types) };
    let types: Vec<String> = (0..n)
        .map(|i| unsafe {
            let p = crate::tsl_list_get(types, i, std::ptr::null()) as *const *const Str;
            crate::str_text(*p).trim_start_matches('.').to_string()
        })
        .collect();
    let chosen = if std::env::var_os("TESSEL_SCRIPT").is_some() {
        DIALOG_ANSWERS.with(|a| a.borrow_mut().pop_front()).flatten()
    } else {
        let mut dialog = rfd::FileDialog::new();
        if !title.is_empty() {
            dialog = dialog.set_title(&title);
        }
        if !types.is_empty() {
            let label = types.iter().map(|t| format!(".{t}")).collect::<Vec<_>>().join(", ");
            dialog = dialog.add_filter(label, &types);
        }
        let path = if save == 0 {
            dialog.pick_file()
        } else {
            if !name.is_empty() {
                dialog = dialog.set_file_name(&name);
            }
            dialog.save_file()
        };
        path.map(|p| p.to_string_lossy().into_owned())
    };
    // Saving: add the first file type's extension if it's missing (for
    // scripted answers too, so tests behave like the real dialog).
    let chosen = chosen.map(|mut p| {
        if let (true, Some(ext)) = (save != 0, types.first()) {
            if !p.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
                p = format!("{p}.{ext}");
            }
        }
        p
    });
    match chosen {
        Some(p) => crate::str_new(&p),
        None => std::ptr::null_mut(),
    }
}

/// For programs without an `app` (only `fn main`): after `main` returns,
/// runs `after` timers and the blocks of `fetch` and `runCommand` until none
/// are left, as an app's event loop would.
pub fn run_until_idle() {
    loop {
        let ran = fire_timers() | deliver_background();
        if !ran && next_timer().is_none() && !background_busy() {
            return;
        }
        // Sleep until the next timer, but check for background results often.
        let wait = next_timer().map_or(0.005, |t| (t - now()).clamp(0.0, 0.005));
        std::thread::sleep(Duration::from_secs_f64(wait));
    }
}

/// Is background work still going (loads or commands)?
fn background_busy() -> bool {
    jobs_busy() || net::busy()
}

/// Background work that finishes by itself (not open WebSockets).
fn jobs_busy() -> bool {
    WAITING.with(|w| !w.borrow().is_empty())
        || COMMANDS.with(|c| !c.borrow().is_empty())
        || COMPUTING.with(|c| !c.borrow().is_empty())
        || net::working()
}

// ===== the app model =====

/// What a click landed on.
enum Hit {
    Tab(u64, usize),
    TabClose(u64, usize),
    Button(u64),
    Toggle(u64),
    Field(u64),
    Segment(u64, usize),
    Tap(u64),
    /// A link in a Markdown view: its number.
    Link(u64, usize),
    /// A view with `.onDrag`.
    Drag(u64),
    /// A view with `.onPointer`.
    Pointer(u64),
    Slider(u64),
    /// A stepper's − (-1) or + (1) button, or its value (0).
    Stepper(u64, i8),
    ColorPicker(u64),
    /// A color in an open color picker's palette.
    Swatch(u64),
    /// Somewhere else on the palette.
    Palette(u64),
    /// A disclosure group's title.
    Disclosure(u64),
}

impl Hit {
    fn key(&self) -> u64 {
        match *self {
            Hit::Button(k) | Hit::Toggle(k) | Hit::Field(k) | Hit::Segment(k, _) | Hit::Tap(k) | Hit::Link(k, _) | Hit::Drag(k) => k,
            Hit::Pointer(k) | Hit::Slider(k) | Hit::Stepper(k, _) | Hit::ColorPicker(k) | Hit::Swatch(k) | Hit::Palette(k) => k,
            Hit::Disclosure(k) => k,
            Hit::Tab(k, _) | Hit::TabClose(k, _) => k,
        }
    }
}

/// Can the keyboard go to this view with Tab (besides text fields)?
fn focusable(n: &Node) -> bool {
    matches!(n.kind, Kind::Slider { .. } | Kind::Stepper { .. } | Kind::ColorPicker { .. } | Kind::Disclosure { .. })
}

struct App {
    root: Callback,
    title: String,
    /// The window whose surface is in the fields below (0: the main window;
    /// a `Window`'s key otherwise). The others are parked in `others`.
    current: u64,
    others: Vec<Surface>,
    /// The menu bar (with the standard menus), as the tree last described it.
    bar: Vec<Entry>,
    /// Whether the app has a `.menuBar` (otherwise `bar` has only the
    /// standard menus, which show only on macOS).
    has_menu_bar: bool,
    /// Whether the menu bar is the system's (macOS) or drawn in the window.
    native_bar: bool,
    /// Files the system asked the app to open, until a view with
    /// `.onOpenFile` takes them.
    open_files: Vec<String>,
    /// The app is to end (`quit()`, or its main window was closed).
    quit: bool,
    /// Standard menu commands only the window can carry out (Minimize, …).
    window_commands: Vec<Std>,
    /// How many times the trees were built (the menu bar may have changed).
    builds: u64,
    // ----- the current window's surface (see `Surface`) -----
    width: f64,
    height: f64,
    /// What the app asked for: 0 follows the system, 1 light, 2 dark.
    appearance: i32,
    tree: Option<Node>,
    text: TextEngine,
    ui: Interaction,
    dirty: bool,
    clipboard: Clipboard,
    /// Keys of the views in the current tree, to find ones that just appeared.
    keys: std::collections::HashSet<u64>,
    /// The caret the view was last scrolled to: the view edited, the
    /// selection, and the length of its text (see `follow_caret`).
    followed: Option<(u64, usize, usize, usize)>,
    completing: Completing,
    /// The view with `.onPointer` the mouse is over (it's told when it leaves).
    pointer_over: Option<u64>,
    /// The view with a `.tooltip` the mouse rests on.
    tip: Option<Tip>,
    /// The sizes views with `.onResize` were last told.
    sizes: HashMap<u64, (f64, f64)>,
    /// The menu that's open (a context menu, or one of the drawn menu bar's).
    menu: Option<OpenMenu>,
}

/// What a window shows and what's going on in it: the App's fields from
/// `width` on, for a window that isn't the current one.
struct Surface {
    key: u64,
    tree: Option<Node>,
    width: f64,
    height: f64,
    ui: Interaction,
    followed: Option<(u64, usize, usize, usize)>,
    completing: Completing,
    pointer_over: Option<u64>,
    tip: Option<Tip>,
    sizes: HashMap<u64, (f64, f64)>,
    menu: Option<OpenMenu>,
}

/// An open menu.
struct OpenMenu {
    source: MenuSource,
    /// Where its top left corner is.
    at: (f64, f64),
    /// For each menu shown (the menu, then its open submenus): the item
    /// that's selected. Only the last one's selection may be `None`.
    sel: Vec<Option<usize>>,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuSource {
    /// The `.contextMenu` of a view.
    Context(u64),
    /// A menu of the drawn menu bar.
    Bar(usize),
}

thread_local! {
    /// The window events are handled for (`closeWindow()` closes it).
    static CURRENT_WINDOW: Cell<u64> = const { Cell::new(0) };
    /// Windows `closeWindow()` closed, and whether `quit()` was called.
    static CLOSING: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static QUITTING: Cell<bool> = const { Cell::new(false) };
}

/// `closeWindow()`: closes the window whose event is handled (for the main
/// window, the app ends), without asking `.onCloseRequest`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_close_window() {
    let key = CURRENT_WINDOW.with(Cell::get);
    CLOSING.with(|c| c.borrow_mut().push(key));
}

/// `quit()`: ends the app, without asking `.onCloseRequest`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_quit() {
    QUITTING.with(|q| q.set(true));
}

/// What the system tells the app outside its windows: a menu item of the
/// macOS menu bar chosen (its path), a file to open, or Quit.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) enum Shell {
    Menu(Vec<usize>),
    OpenFile(String),
    Quit,
}

pub(super) static SHELL: Mutex<Vec<Shell>> = Mutex::new(Vec::new());

/// Hands the app something from the system (on any thread) and wakes it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn shell_post(event: Shell) {
    SHELL.lock().unwrap().push(event);
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// Whether the app has a view with `.onCloseRequest` (so quitting asks it).
pub(super) static ASKS_TO_CLOSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A tooltip waiting to show, or showing.
struct Tip {
    key: u64,
    /// When the mouse came to rest on the view (on the app's clock).
    since: f64,
    /// Showing, or not to be shown again until the mouse leaves the view
    /// (after a click).
    done: bool,
}

/// How long the mouse rests on a view before its tooltip shows.
const TIP_DELAY: f64 = 0.6;

thread_local! {
    /// `setWindowTitle(…)`: the title the window takes at its next update.
    static NEW_TITLE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// `undoTextEdit()` (false) or `redoTextEdit()` (true), done at the next update.
    static UNDO: Cell<Option<bool>> = const { Cell::new(None) };
}

/// `undoTextEdit()`: as Cmd+Z.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_undo_text_edit() {
    UNDO.with(|u| u.set(Some(false)));
}

/// `redoTextEdit()`: as Cmd+Shift+Z.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_redo_text_edit() {
    UNDO.with(|u| u.set(Some(true)));
}

/// `setWindowTitle(title)`: changes the text in the window's title bar.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_set_window_title(title: *const Str) {
    let title = unsafe { crate::str_text(title) }.to_string();
    NEW_TITLE.with(|t| *t.borrow_mut() = Some(title));
}

// `PointerKind` cases, in order.
const DOWN: i32 = 0;
const UP: i32 = 1;
const MOVE: i32 = 2;
const DRAG: i32 = 3;
const SCROLL: i32 = 4;
const PINCH: i32 = 5;
const LEAVE: i32 = 6;

/// Code completion in a `CodeEditor`.
#[derive(Default)]
struct Completing {
    /// The editor and the word start (byte offset) suggestions were last
    /// asked for. Suggestions for another start are stale and don't show.
    asked: Option<(u64, usize)>,
    /// The typed part of the word, which the selection belongs to.
    prefix: String,
    selected: usize,
}

/// The suggestions that match the word at the caret.
struct Matches {
    key: u64,
    /// The word being completed: from `start` to `end` (it may go on after the caret).
    start: usize,
    end: usize,
    items: Vec<super::Suggestion>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the word ending at `caret` starts.
fn word_start(text: &str, caret: usize) -> usize {
    text[..caret].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(caret, |(i, _)| i)
}

/// Suggestions matching `prefix`: those starting with it (exact case first),
/// then those containing it.
fn filter(all: &[super::Suggestion], prefix: &str) -> Vec<super::Suggestion> {
    let lower = prefix.to_lowercase();
    let mut out: Vec<super::Suggestion> = all.iter().filter(|s| s.label.starts_with(prefix)).cloned().collect();
    for s in all {
        let l = s.label.to_lowercase();
        if !out.contains(s) && l.starts_with(&lower) {
            out.push(s.clone());
        }
    }
    if !prefix.is_empty() {
        for s in all {
            if !out.contains(s) && s.label.to_lowercase().contains(&lower) {
                out.push(s.clone());
            }
        }
    }
    out
}

fn badge(kind: &str) -> (&'static str, Rgba) {
    match kind {
        "function" => ("f", Rgba([142, 68, 173, 255])),
        "method" => ("m", Rgba([142, 68, 173, 255])),
        "variable" => ("v", Rgba([0, 122, 255, 255])),
        "field" => ("p", Rgba([48, 176, 199, 255])),
        "view" => ("V", Rgba([255, 149, 0, 255])),
        "modifier" => (".", Rgba([52, 199, 89, 255])),
        "type" => ("T", Rgba([162, 132, 94, 255])),
        "case" => ("c", Rgba([204, 160, 0, 255])),
        _ => ("k", Rgba([142, 142, 147, 255])),
    }
}

/// Keyboard modifiers, with the platform's shortcut key as `primary`
/// (Cmd on macOS, Ctrl elsewhere).
#[derive(Clone, Copy, Default)]
struct Mods {
    shift: bool,
    alt: bool,
    primary: bool,
    /// The Ctrl key itself (on Windows and Linux it's also `primary`).
    control: bool,
}

/// Keys that edit text or move in it, which `App::key` handles.
const EDIT_KEYS: &[&str] =
    &["backspace", "delete", "left", "right", "up", "down", "home", "end", "enter", "escape", "tab"];

impl App {
    /// Rebuilds the trees if state may have changed, then lays them out.
    fn update(&mut self) {
        if let Some(redo) = UNDO.with(|u| u.take()) {
            self.undo(redo);
        }
        // `.onAppear` blocks can change state, which rebuilds again (a few times at most).
        if let Some(root) = NEW_ROOT.with(|r| r.borrow_mut().take()) {
            self.root = root;
            self.dirty = true;
        }
        self.take_requests();
        // `.onResize` blocks run after layout, and can change state too:
        // then the tree is built and laid out again (a few times at most).
        for _ in 0..4 {
            for _ in 0..8 {
                if !(self.dirty || self.tree.is_none()) {
                    break;
                }
                self.dirty = false;
                let built = build_all(&self.root);
                self.distribute(built);
                self.run_on_appear();
                self.each_surface(|a| {
                    a.sync_editor_lines();
                    a.track_editor_texts();
                });
                self.take_requests();
            }
            let mut again = false;
            self.each_surface(|a| {
                a.lay_out();
                again |= a.run_on_resize();
                again |= a.lazy_rows();
            });
            if !again {
                break;
            }
        }
        let mut picker_shown = false;
        self.each_surface(|a| {
            // The keyboard leaves a control that's gone.
            if a.ui.editing.is_some() || a.ui.focused.is_some_and(|k| a.node(k).is_none()) {
                a.ui.focused = None;
            }
            picker_shown |= super::controls::OPEN_PICKER.get().is_some_and(|k| a.node(k).is_some());
            a.follow_caret();
            a.scroll_to_offset();
            a.layout_popup();
            a.layout_menus();
            if let Some(tree) = a.tree.as_ref() {
                a.ui.anim.step(tree, now(), (a.width, a.height), &a.ui.scroll);
            }
        });
        // A palette whose picker is gone closes.
        if !picker_shown {
            super::controls::OPEN_PICKER.set(None);
        }
        if self.deliver_open_files() {
            self.update();
        }
    }

    /// Lays out the current window's tree.
    fn lay_out(&mut self) {
        if self.ui.editing.is_none() {
            self.ui.composing = None;
        }
        let composing =
            self.ui.editing.as_ref().zip(self.ui.composing.as_ref()).map(|(e, c)| (e.key, e.range(), c.text.as_str()));
        let top = if self.bar_drawn() { menu::BAR_H } else { 0.0 };
        let Some(tree) = self.tree.as_mut() else { return };
        Layouter { text: &mut self.text, scroll: &self.ui.scroll, composing }.run_below(tree, self.width, self.height, top);
    }

    /// Whether the current window shows a menu bar drawn in it.
    fn bar_drawn(&self) -> bool {
        self.current == 0 && self.has_menu_bar && !self.native_bar
    }

    // ===== windows =====

    /// Swaps the current window's surface with `s`.
    fn swap_surface(&mut self, s: &mut Surface) {
        std::mem::swap(&mut self.current, &mut s.key);
        std::mem::swap(&mut self.tree, &mut s.tree);
        std::mem::swap(&mut self.width, &mut s.width);
        std::mem::swap(&mut self.height, &mut s.height);
        std::mem::swap(&mut self.ui, &mut s.ui);
        std::mem::swap(&mut self.followed, &mut s.followed);
        std::mem::swap(&mut self.completing, &mut s.completing);
        std::mem::swap(&mut self.pointer_over, &mut s.pointer_over);
        std::mem::swap(&mut self.tip, &mut s.tip);
        std::mem::swap(&mut self.sizes, &mut s.sizes);
        std::mem::swap(&mut self.menu, &mut s.menu);
    }

    /// Makes window `key` the current one. Returns whether it's open.
    fn enter(&mut self, key: u64) -> bool {
        if self.current != key {
            let Some(i) = self.others.iter().position(|s| s.key == key) else { return false };
            let mut s = self.others.remove(i);
            self.swap_surface(&mut s);
            self.others.push(s);
        }
        CURRENT_WINDOW.with(|c| c.set(key));
        true
    }

    /// Runs `f` with each window current in turn.
    fn each_surface(&mut self, mut f: impl FnMut(&mut App)) {
        let back = self.current;
        let keys: Vec<u64> = std::iter::once(back).chain(self.others.iter().map(|s| s.key)).collect();
        for key in keys {
            if self.enter(key) {
                f(self);
            }
        }
        if !self.enter(back) {
            self.enter(0);
        }
    }

    /// The open windows other than the main one: key, title and size.
    fn windows(&self) -> Vec<(u64, String, f64, f64)> {
        let mut out = Vec::new();
        let all = std::iter::once((self.current, &self.tree, self.width, self.height))
            .chain(self.others.iter().map(|s| (s.key, &s.tree, s.width, s.height)));
        for (key, tree, w, h) in all {
            if let (true, Some(Node { kind: Kind::Window { title, .. }, .. })) = (key != 0, tree) {
                out.push((key, title.clone(), w, h));
            }
        }
        out.sort_by_key(|w| w.0);
        out
    }

    /// The tree of the main window.
    fn main_tree(&self) -> Option<&Node> {
        if self.current == 0 {
            self.tree.as_ref()
        } else {
            self.others.iter().find(|s| s.key == 0)?.tree.as_ref()
        }
    }

    /// Gives each window its new tree: windows that just opened get a
    /// surface, and those that closed lose theirs.
    fn distribute(&mut self, built: Built) {
        let Built { root, windows } = built;
        self.builds += 1;
        let back = self.current;
        let open: Vec<u64> = windows.iter().map(|w| w.key).collect();
        if self.enter(0) {
            self.tree = Some(root);
        }
        for w in windows {
            if !self.enter(w.key) {
                let Kind::Window { width, height, .. } = w.kind else { continue };
                let mut ui = Interaction::default();
                ui.anim.enabled = self.ui.anim.enabled;
                self.others.push(Surface {
                    key: w.key,
                    tree: None,
                    width,
                    height,
                    ui,
                    followed: None,
                    completing: Completing::default(),
                    pointer_over: None,
                    tip: None,
                    sizes: HashMap::new(),
                    menu: None,
                });
                self.enter(w.key);
            }
            self.tree = Some(w);
        }
        if back == 0 || !open.contains(&back) || !self.enter(back) {
            self.enter(0);
        }
        self.others.retain(|s| s.key == 0 || open.contains(&s.key));
        // The menu bar is the main window's.
        fn find(n: &Node) -> Option<&Node> {
            if n.mods.hidden {
                return None;
            }
            if let Some(b) = &n.mods.menu_bar {
                return Some(b);
            }
            n.children.iter().find_map(find)
        }
        fn shortcuts(n: &Node, out: &mut Vec<menu::Shortcut>) {
            if n.mods.hidden || n.mods.disabled {
                return;
            }
            for s in &n.mods.shortcuts {
                out.push(menu::Shortcut { cmd: true, shift: s.shift, alt: s.alt, key: s.key.clone() });
            }
            n.visible_children().iter().for_each(|c| shortcuts(c, out));
        }
        let mut keys = Vec::new();
        if let Some(t) = self.main_tree() {
            shortcuts(t, &mut keys);
        }
        let user = self.main_tree().and_then(find);
        let has_menu_bar = user.is_some();
        let bar = menu::bar(user, &self.title, self.native_bar, &keys);
        self.has_menu_bar = has_menu_bar;
        self.bar = bar;
        let asks = self.find_in_main(|n| n.mods.on_close_request.is_some()).is_some();
        ASKS_TO_CLOSE.store(asks, std::sync::atomic::Ordering::Relaxed);
    }

    /// The first visible, enabled view in the main window's tree for which
    /// `pick` is true.
    fn find_in_main(&self, pick: impl Fn(&Node) -> bool) -> Option<&Node> {
        fn find<'n>(n: &'n Node, pick: &dyn Fn(&Node) -> bool) -> Option<&'n Node> {
            if n.mods.hidden || n.mods.disabled {
                return None;
            }
            if pick(n) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, pick))
        }
        find(self.main_tree()?, &pick)
    }

    /// Carries out `closeWindow()` and `quit()`.
    fn take_requests(&mut self) {
        if QUITTING.with(|q| q.replace(false)) {
            self.quit = true;
        }
        for key in CLOSING.with(|c| std::mem::take(&mut *c.borrow_mut())) {
            self.close_window(key);
        }
    }

    /// Closes a window (the main one: the app ends).
    fn close_window(&mut self, key: u64) {
        if key == 0 {
            self.quit = true;
            return;
        }
        let back = self.current;
        if self.enter(key) {
            if let Some(Node { kind: Kind::Window { is_open, .. }, .. }) = &self.tree {
                unsafe { *is_open.locate() = 0 };
                self.dirty = true;
            }
            self.enter(back);
        }
    }

    /// The user asks to close the current window: its `.onCloseRequest`
    /// decides, if it has one; otherwise it closes.
    fn request_close(&mut self) {
        fn find(n: &Node) -> Option<Callback> {
            if n.mods.hidden || n.mods.disabled {
                return None;
            }
            if let Some(cb) = &n.mods.on_close_request {
                return Some(cb.clone());
            }
            n.children.iter().find_map(find)
        }
        match self.tree.as_ref().and_then(find) {
            Some(cb) => {
                cb.call();
                self.dirty = true;
            }
            None => self.close_window(self.current),
        }
        self.take_requests();
    }

    /// The user asks to quit: the main window's `.onCloseRequest` decides.
    fn request_quit(&mut self) {
        let back = self.current;
        self.enter(0);
        self.request_close();
        if !self.enter(back) {
            self.enter(0);
        }
    }

    /// Gives files waiting to be opened to the first view with
    /// `.onOpenFile`, if there's one. Returns whether it got any.
    fn deliver_open_files(&mut self) -> bool {
        if self.open_files.is_empty() {
            return false;
        }
        let Some(cb) = self.find_in_main(|n| n.mods.on_open_file.is_some()).and_then(|n| n.mods.on_open_file.clone()) else {
            return false;
        };
        for path in std::mem::take(&mut self.open_files) {
            cb.call_with(crate::str_new(&path) as *mut u8);
        }
        self.dirty = true;
        self.take_requests();
        true
    }

    // ===== menus =====

    /// The items of the open menu (its first level).
    fn menu_items(&self) -> Option<Vec<Entry>> {
        match self.menu.as_ref()?.source {
            MenuSource::Context(key) => self.node(key)?.mods.context_menu.as_deref().map(menu::entries),
            MenuSource::Bar(i) => match self.bar.get(i)? {
                Entry::Sub { items, .. } => Some(items.clone()),
                _ => None,
            },
        }
    }

    /// Lays out the open menu and its submenus, and the drawn menu bar.
    fn layout_menus(&mut self) {
        self.ui.menus.clear();
        self.ui.menu_bar = self.bar_drawn().then(|| menu::Bar::layout(&mut self.text, &self.bar, self.width));
        let Some(items) = self.menu_items() else {
            self.menu = None;
            return;
        };
        let Some(m) = self.menu.as_ref() else { return };
        if let (Some(bar), MenuSource::Bar(i)) = (self.ui.menu_bar.as_mut(), m.source) {
            bar.open = Some(i);
        }
        let (mut at, mut beside) = (m.at, None);
        let mut path = Vec::new();
        for (level, sel) in m.sel.iter().enumerate() {
            let Some(shown) = menu::items_at(&items, &path) else { break };
            let mut panel = menu::Panel::layout(&mut self.text, shown, at, (self.width, self.height), beside);
            panel.selected = sel.filter(|i| *i < panel.rows.len());
            let deeper = level + 1 < m.sel.len();
            if let (Some(i), true) = (panel.selected, deeper) {
                let row = panel.rows[i].rect;
                at = (panel.rect.x + panel.rect.w - 4.0, row.y - menu::PAD);
                beside = Some(panel.rect.x);
                path.push(i);
            }
            self.ui.menus.push(panel);
            if path.len() <= level {
                break;
            }
        }
    }

    fn open_menu(&mut self, source: MenuSource, at: (f64, f64)) {
        self.menu = Some(OpenMenu { source, at, sel: vec![None] });
        self.ui.editing = None;
        self.tip_hide();
        self.layout_menus();
    }

    fn open_bar_menu(&mut self, i: usize) {
        let x = self.ui.menu_bar.as_ref().and_then(|b| b.titles.get(i)).map_or(0.0, |(_, r)| r.x);
        self.open_menu(MenuSource::Bar(i), (x, menu::BAR_H));
    }

    /// Selects row `row` of the menu shown at `level`; a submenu there opens.
    fn menu_select(&mut self, level: usize, row: usize) {
        let Some(items) = self.menu_items() else { return };
        let Some(m) = self.menu.as_mut() else { return };
        if m.sel.len() <= level {
            return;
        }
        m.sel.truncate(level + 1);
        m.sel[level] = Some(row);
        let path: Vec<usize> = m.sel.iter().map_while(|s| *s).collect();
        if let Some(Entry::Sub { enabled: true, .. }) = menu::at(&items, &path) {
            m.sel.push(None);
        }
        self.layout_menus();
    }

    /// The menu row at a point: the level of its menu, and the row.
    fn menu_row_at(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        self.ui.menus.iter().enumerate().rev().find_map(|(level, p)| p.row_at(x, y).map(|row| (level, row)))
    }

    /// The mouse is over a point: the menu item there is selected (and, on
    /// the drawn menu bar, another menu opens in place of the open one).
    fn menu_hover(&mut self, x: f64, y: f64) {
        if self.menu.is_none() {
            return;
        }
        if let (Some(i), Some(MenuSource::Bar(open))) =
            (self.ui.menu_bar.as_ref().and_then(|b| b.title_at(x, y)), self.menu.as_ref().map(|m| m.source))
        {
            if i != open {
                self.open_bar_menu(i);
            }
            return;
        }
        let Some((level, row)) = self.menu_row_at(x, y) else { return };
        if self.ui.menus[level].rows[row].label.is_some() {
            self.menu_select(level, row);
        }
    }

    /// A press while a menu is open, or on the drawn menu bar. Returns
    /// whether it was taken (a press outside an open menu closes it).
    fn menu_press(&mut self, x: f64, y: f64) -> bool {
        if let Some(i) = self.ui.menu_bar.as_ref().and_then(|b| b.title_at(x, y)) {
            if self.menu.as_ref().is_some_and(|m| m.source == MenuSource::Bar(i)) {
                self.menu = None;
                self.layout_menus();
            } else {
                self.open_bar_menu(i);
            }
            return true;
        }
        if self.menu.is_none() {
            return false;
        }
        match self.menu_row_at(x, y) {
            Some((level, row)) => {
                let mut path: Vec<usize> = self.menu.as_ref().map_or(Vec::new(), |m| m.sel.iter().take(level).map_while(|s| *s).collect());
                path.push(row);
                self.menu_choose(&path);
            }
            None => {
                self.menu = None;
                self.layout_menus();
            }
        }
        true
    }

    /// Chooses the item at `path` of the open menu: a submenu opens; an item
    /// closes the menu and runs.
    fn menu_choose(&mut self, path: &[usize]) {
        let Some(items) = self.menu_items() else { return };
        let Some(entry) = menu::at(&items, path).cloned() else { return };
        match entry {
            Entry::Sub { enabled: true, .. } => {
                if let Some(m) = self.menu.as_mut() {
                    m.sel = path.iter().map(|i| Some(*i)).collect();
                    m.sel.push(None);
                }
                self.layout_menus();
            }
            Entry::Item { enabled: true, .. } => {
                self.menu = None;
                self.layout_menus();
                self.run_entry(&entry);
            }
            _ => {}
        }
    }

    /// Runs a menu item.
    fn run_entry(&mut self, entry: &Entry) {
        let Entry::Item { command, shortcut, .. } = entry else { return };
        match command {
            // An item with Cmd+C (or another editing key) edits the text
            // that's being edited instead.
            Command::User(_) if self.ui.editing.is_some() && shortcut.as_ref().is_some_and(|s| s.is_editing()) => {
                let s = shortcut.as_ref().unwrap();
                let mods = Mods { shift: s.shift, alt: s.alt, primary: true, control: false };
                self.key_plain(&s.key, mods);
            }
            Command::User(action) => {
                action.call();
                self.dirty = true;
            }
            Command::Std(std) => self.run_std(*std),
        }
        self.take_requests();
    }

    /// Carries out a command of the standard menus.
    fn run_std(&mut self, std: Std) {
        let primary = Mods { primary: true, ..Mods::default() };
        match std {
            Std::Quit => self.request_quit(),
            Std::Undo => self.key_plain("z", primary),
            Std::Redo => self.key_plain("z", Mods { shift: true, ..primary }),
            Std::Cut => self.key_plain("x", primary),
            Std::Copy => self.key_plain("c", primary),
            Std::Paste => self.key_plain("v", primary),
            Std::SelectAll => self.key_plain("a", primary),
            Std::CloseWindow => self.request_close(),
            other => self.window_commands.push(other),
        }
    }

    /// Chooses an item of the menu bar by its titles (`menu` script command).
    fn choose_bar_item(&mut self, labels: &[String]) -> Result<(), String> {
        let mut items: &[Entry] = &self.bar;
        let mut found = None;
        for (i, label) in labels.iter().enumerate() {
            let Some(e) = items.iter().find(|e| e.label() == label) else {
                return Err(format!("no menu item {:?}", labels[..=i].join(" > ")));
            };
            match e {
                Entry::Sub { items: inner, .. } if i + 1 < labels.len() => items = inner,
                _ if i + 1 == labels.len() => found = Some(e.clone()),
                _ => return Err(format!("{label:?} has no items")),
            }
        }
        match found {
            Some(e @ Entry::Item { enabled: true, .. }) => {
                self.run_entry(&e);
                Ok(())
            }
            Some(Entry::Item { .. }) => Ok(()),
            _ => Err("name a menu, then an item in it".into()),
        }
    }

    /// Keys while a menu is open: arrows move, Enter chooses, Escape closes.
    /// Returns whether the key was taken (all are, but shortcuts).
    fn menu_key(&mut self, name: &str, mods: Mods) -> bool {
        if mods.primary {
            return false;
        }
        let Some(items) = self.menu_items() else { return false };
        let bar_len = self.bar.len().max(1);
        let Some(m) = self.menu.as_mut() else { return false };
        let level = m.sel.len() - 1;
        let path: Vec<usize> = m.sel.iter().take(level).map_while(|s| *s).collect();
        let shown = menu::items_at(&items, &path).unwrap_or(&[]);
        let step = |from: Option<usize>, down: bool| -> Option<usize> {
            let n = shown.len();
            let mut i = from;
            for _ in 0..n {
                let next = match (i, down) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                };
                if shown[next].enabled() {
                    return Some(next);
                }
                i = Some(next);
            }
            from
        };
        match name {
            "down" | "up" => m.sel[level] = step(m.sel[level], name == "down"),
            "right" => match m.sel[level].and_then(|i| shown.get(i)) {
                Some(Entry::Sub { enabled: true, items: sub, .. }) => {
                    let first = sub.iter().position(Entry::enabled);
                    m.sel.push(first);
                }
                _ => {
                    // On the menu bar: the next menu.
                    if let MenuSource::Bar(i) = m.source {
                        self.open_bar_menu((i + 1) % bar_len);
                        return true;
                    }
                }
            },
            "left" => {
                if level > 0 {
                    m.sel.pop();
                } else if let MenuSource::Bar(i) = m.source {
                    self.open_bar_menu((i + bar_len - 1) % bar_len);
                    return true;
                }
            }
            "escape" => {
                if level > 0 {
                    m.sel.pop();
                } else {
                    self.menu = None;
                }
            }
            "enter" | "space" => {
                if let Some(i) = m.sel[level] {
                    let mut path = path.clone();
                    path.push(i);
                    self.menu_choose(&path);
                    return true;
                }
            }
            _ => {}
        }
        self.layout_menus();
        true
    }

    /// The innermost view at a point with a `.contextMenu` or `.onPointer`:
    /// its key, and whether it's a context menu.
    fn context_target(&self, x: f64, y: f64) -> Option<(u64, bool)> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<(u64, bool)> {
            if n.mods.hidden || n.layout.style.disabled || ((n.mods.clipped || n.children.is_empty()) && !n.layout.rect.contains(x, y)) {
                return None;
            }
            if let Some(found) = n.visible_children().iter().rev().find_map(|c| visit(c, x, y)) {
                return Some(found);
            }
            if !n.layout.rect.contains(x, y) {
                return None;
            }
            if n.mods.context_menu.is_some() {
                return Some((n.key, true));
            }
            n.mods.on_pointer.is_some().then_some((n.key, false))
        }
        visit(self.tree.as_ref()?, x, y)
    }

    /// The popovers that are open, the topmost last.
    fn popovers(&self) -> Vec<&Node> {
        let Some(tree) = &self.tree else { return Vec::new() };
        tree.children.iter().filter(|c| matches!(c.kind, Kind::Popover { .. })).collect()
    }

    /// Closes popovers (from the topmost) down to the one containing the
    /// point (all of them, for a point outside). Returns whether any closed.
    fn close_popovers_outside(&mut self, x: f64, y: f64) -> bool {
        let mut closed = false;
        for p in self.popovers().into_iter().rev() {
            if p.layout.rect.contains(x, y) {
                break;
            }
            if let Kind::Popover { presented, .. } = &p.kind {
                unsafe { *presented.locate() = 0 };
                closed = true;
            }
        }
        if closed {
            self.ui.editing = None;
            self.dirty = true;
        }
        closed
    }

    /// Escape: closes the topmost popover. Returns whether there was one.
    fn close_top_popover(&mut self) -> bool {
        let Some(Kind::Popover { presented, .. }) = self.popovers().last().map(|p| &p.kind) else { return false };
        unsafe { *presented.locate() = 0 };
        self.dirty = true;
        true
    }

    /// A mouse press, before views get it: open menus, the drawn menu bar
    /// and popovers take it first, and a right press (`right`) opens a
    /// view's context menu. Returns whether it was taken.
    fn mouse_press(&mut self, x: f64, y: f64, right: bool) -> bool {
        if self.menu_press(x, y) {
            return true;
        }
        if self.close_popovers_outside(x, y) {
            return true;
        }
        if let (true, Some((key, true))) = (right, self.context_target(x, y)) {
            self.open_menu(MenuSource::Context(key), (x, y));
            return true;
        }
        false
    }

    /// What `tree` prints after the view tree: the open menu, with its
    /// selected item marked and its open submenu under that item.
    fn describe_menu(&self) -> String {
        fn level(items: &[Entry], sel: &[Option<usize>], depth: usize, out: &mut String) {
            let Some((here, deeper)) = sel.split_first() else { return };
            for (i, e) in items.iter().enumerate() {
                let flat = match e {
                    Entry::Sub { title, enabled, .. } => Entry::Sub { title: title.clone(), enabled: *enabled, items: Vec::new() },
                    e => e.clone(),
                };
                let mut line = String::new();
                menu::describe(std::slice::from_ref(&flat), 0, &mut line);
                let mark = if *here == Some(i) { " [selected]" } else { "" };
                out.push_str(&format!("{}{}{mark}\n", "  ".repeat(depth), line.trim_end()));
                if let (true, false, Entry::Sub { items: sub, .. }) = (*here == Some(i), deeper.is_empty(), e) {
                    level(sub, deeper, depth + 1, out);
                }
            }
        }
        let mut out = String::new();
        let (Some(m), Some(items)) = (self.menu.as_ref(), self.menu_items()) else { return out };
        match m.source {
            MenuSource::Context(_) => out.push_str("ContextMenu\n"),
            MenuSource::Bar(i) => out.push_str(&format!("Menu {:?}\n", self.bar.get(i).map_or("", |e| e.label()))),
        }
        level(&items, &m.sel, 1, &mut out);
        out
    }

    /// Text an input method is composing (empty when it's done or given up).
    fn compose(&mut self, text: String, caret: Option<usize>) {
        self.ui.composing = (self.ui.editing.is_some() && !text.is_empty()).then_some(render::Composing { text, caret });
    }

    /// Where the caret is in the window, for the input method's own window.
    fn caret_rect(&self) -> Option<Rect> {
        let e = self.ui.editing.as_ref()?;
        let node = self.node(e.key)?;
        let layout = node.layout.text.as_ref()?;
        let at = self.ui.shown_editing(e.key)?;
        let (x0, y0, sx, sy) = self.text_origin(node);
        let c = edit::geometry(layout, &at).0?;
        Some(Rect { x: x0 - sx + c.x0, y: y0 - sy + c.y0, w: c.x1 - c.x0, h: c.y1 - c.y0 })
    }

    /// Tab and Shift+Tab: edits the next (or previous) text field or code
    /// editor, with a field's text selected, or puts the keyboard on the next
    /// slider, stepper, color picker or disclosure group.
    fn focus_next(&mut self, back: bool) {
        fn fields<'n>(n: &'n Node, out: &mut Vec<&'n Node>) {
            if n.mods.hidden || n.layout.style.disabled {
                return;
            }
            if editable_text(n).is_some() || focusable(n) {
                out.push(n);
            }
            // (Not into a color picker's palette.)
            if !matches!(n.kind, Kind::ColorPicker { .. }) {
                for c in n.visible_children() {
                    fields(c, out);
                }
            }
        }
        let mut found = Vec::new();
        // In a popover, Tab stays in it.
        if let Some(t) = self.popovers().last().copied().or(self.tree.as_ref()) {
            fields(t, &mut found);
        }
        let count = found.len();
        if count == 0 {
            return;
        }
        let at = self.ui.editing.as_ref().map(|e| e.key).or(self.ui.focused);
        let current = at.and_then(|key| found.iter().position(|n| n.key == key));
        let next = match (current, back) {
            (Some(i), false) => (i + 1) % count,
            (Some(i), true) => (i + count - 1) % count,
            (None, false) => 0,
            (None, true) => count - 1,
        };
        let node = found[next];
        let (key, control, field) = (node.key, focusable(node), matches!(node.kind, Kind::TextField { .. }));
        let text = editable_text(node).map(read_string).unwrap_or_default();
        self.ui.composing = None;
        self.completing.asked = None;
        if control {
            self.ui.focused = Some(key);
            self.ui.focus_ring = true;
            self.ui.editing = None;
            return;
        }
        let mut e = Editing::new(key, &text);
        if field {
            e.anchor = 0;
        }
        self.ui.editing = Some(e);
        self.ui.focused = None;
    }

    // ===== code completion =====

    /// The suggestions for the word at the caret, if the list should show.
    fn matches(&self) -> Option<Matches> {
        let e = self.ui.editing.as_ref()?;
        let (key, start) = self.completing.asked?;
        if key != e.key || !e.range().is_empty() {
            return None;
        }
        let Kind::CodeEditor { text, completions, completion_start, .. } = &self.node(key)?.kind else { return None };
        if *completion_start != start as i64 || completions.is_empty() {
            return None;
        }
        let text = read_string(text);
        let caret = e.focus.min(text.len());
        if start > caret || word_start(&text, caret) != start {
            return None;
        }
        let end = caret + text[caret..].chars().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
        let prefix = &text[start..caret];
        let items = filter(completions, prefix);
        // Nothing left to complete.
        if items.is_empty() || (items.len() == 1 && items[0].label == text[start..end]) {
            return None;
        }
        Some(Matches { key, start, end, items })
    }

    /// After typing in a code editor: ask for suggestions when a new word
    /// starts (or after `.`). While the word goes on, the list just filters.
    fn after_typing(&mut self, typed: &str) {
        let Some(e) = self.ui.editing.as_ref() else { return };
        let key = e.key;
        let caret = e.focus;
        let Some(Kind::CodeEditor { text, on_complete: Some(ask), .. }) = self.node(key).map(|n| &n.kind) else { return };
        let (text, ask) = (read_string(text), ask.clone());
        let last = typed.chars().last().unwrap_or(' ');
        if !(is_word(last) || last == '.') || caret > text.len() {
            self.completing.asked = None;
            return;
        }
        let start = word_start(&text, caret);
        let before = text[..start].chars().next_back();
        // Not in numbers (`12`, `3.5`).
        if text[start..].starts_with(|c: char| c.is_ascii_digit())
            || (start == caret && before == Some('.') && text[..start - 1].ends_with(|c: char| c.is_ascii_digit()))
        {
            self.completing.asked = None;
            return;
        }
        if self.completing.asked == Some((key, start)) {
            return;
        }
        self.completing.asked = Some((key, start));
        self.completing.selected = 0;
        ask.call_with_int(caret as i64);
        self.dirty = true;
    }

    /// Replaces the word being completed with suggestion `index` of `m`.
    fn accept(&mut self, m: &Matches, index: usize) {
        let Some(s) = m.items.get(index) else { return };
        let text = self.node(m.key).and_then(editable_text).map(read_string).unwrap_or_default();
        // Calls get their `(`, or `()` when they take nothing.
        let mut insert = s.label.clone();
        let callable = matches!(s.kind.as_str(), "function" | "method" | "view" | "modifier");
        if callable && !text[m.end.min(text.len())..].starts_with('(') {
            insert.push('(');
            if s.detail.trim_start_matches("view ").starts_with("()") {
                insert.push(')');
            }
        }
        if let Some(e) = self.ui.editing.as_mut().filter(|e| e.key == m.key) {
            e.anchor = m.start;
            e.focus = m.end;
        }
        self.completing.asked = None;
        self.edit(Edit::Insert(insert));
    }

    /// Keys the completion list takes while it shows. Returns whether it took the key.
    fn completion_key(&mut self, name: &str) -> bool {
        let Some(m) = self.matches() else { return false };
        match name {
            "up" | "down" => {
                let n = m.items.len();
                let s = &mut self.completing.selected;
                *s = if name == "down" { (*s + 1) % n } else { (*s + n - 1) % n };
            }
            "enter" | "tab" => {
                let i = self.completing.selected.min(m.items.len() - 1);
                let word = self.node(m.key).and_then(editable_text).map(read_string).unwrap_or_default();
                if name == "enter" && word.get(m.start..m.end) == Some(m.items[i].label.as_str()) {
                    // Already typed in full: Enter is a new line.
                    self.completing.asked = None;
                    return false;
                }
                self.accept(&m, i);
            }
            "escape" => self.completing.asked = None,
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// A click on the completion list accepts the suggestion under it.
    fn popup_click(&mut self, x: f64, y: f64) -> bool {
        let Some(index) = self.ui.popup.as_ref().and_then(|p| p.row_at(x, y)) else { return false };
        if let Some(m) = self.matches() {
            self.accept(&m, index);
            self.dirty = true;
        }
        true
    }

    /// Lays out the completion list under the word being completed.
    fn layout_popup(&mut self) {
        self.ui.popup = None;
        let Some(m) = self.matches() else { return };
        let prefix = self.node(m.key).and_then(editable_text).map(read_string).unwrap_or_default();
        let caret = self.ui.editing.as_ref().map_or(m.start, |e| e.focus.min(prefix.len()));
        let prefix = prefix[m.start..caret].to_string();
        if prefix != self.completing.prefix {
            self.completing.prefix = prefix;
            self.completing.selected = 0;
        }
        let selected = self.completing.selected.min(m.items.len() - 1);
        self.completing.selected = selected;
        let Some(node) = self.node(m.key) else { return };
        let Some(layout) = &node.layout.text else { return };
        let mut at = Editing::new(m.key, "");
        at.anchor = m.start;
        at.focus = m.start;
        let (x0, y0, sx, sy) = self.text_origin(node);
        let (cx, top, bottom) = match edit::geometry(layout, &at).0 {
            Some(c) => (x0 - sx + c.x0, y0 - sy + c.y0, y0 - sy + c.y1),
            None => (x0, y0, y0 + 16.0),
        };
        const VISIBLE: usize = 10;
        let first = selected.saturating_sub(VISIBLE - 1);
        let mut rows = Vec::new();
        let mut widest: f64 = 0.0;
        for (index, s) in m.items.iter().enumerate().skip(first).take(VISIBLE) {
            let (letter, badge_color) = badge(&s.kind);
            let detail: String = if s.detail.chars().count() > 48 {
                s.detail.chars().take(47).chain(std::iter::once('…')).collect()
            } else {
                s.detail.clone()
            };
            let row = render::PopupRow {
                index,
                badge: Rc::new(self.text.mono_line(letter, 11.0, 700.0, Rgba([255, 255, 255, 255]))),
                badge_color,
                label: Rc::new(self.text.mono_line(&s.label, 13.0, 400.0, theme().colors[theme::PRIMARY])),
                detail: Rc::new(self.text.layout(&detail, 11.5, 400.0, theme().colors[theme::GRAY], None)),
            };
            widest = widest.max(row.label.width() + row.detail.width());
            rows.push(row);
        }
        let row_h = 22.0;
        let w = (widest + 64.0).clamp(220.0, 560.0).min(self.width - 8.0);
        let h = rows.len() as f64 * row_h + 8.0;
        let x = (cx - 32.0).min(self.width - w - 4.0).max(4.0);
        let y = if bottom + 2.0 + h > self.height - 4.0 && top - h - 2.0 > 4.0 { top - h - 2.0 } else { bottom + 2.0 };
        self.ui.popup = Some(render::Popup {
            rect: Rect { x, y, w, h },
            row_h,
            selected: Some(selected - first),
            rows,
        });
    }

    /// Stops editing a field that disappeared, keeps the selection valid,
    /// and scrolls the field so the caret stays visible.
    fn follow_caret(&mut self) {
        let Some(e) = self.ui.editing.as_mut() else { return };
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(e.key)) else {
            self.ui.editing = None;
            return;
        };
        let Some(text) = editable_text(node) else {
            self.ui.editing = None;
            return;
        };
        e.clamp(&read_string(text));
        let Some(layout) = &node.layout.text else { return };
        // While text is composed, the caret is inside it.
        let mut at = e.clone();
        if let Some(c) = &self.ui.composing {
            let caret = e.range().start + c.caret.unwrap_or(c.text.len()).min(c.text.len());
            (at.anchor, at.focus) = (caret, caret);
        }
        let (caret, _) = edit::geometry(layout, &at);
        let content = node.layout.rect.inset_by(node.mods.padding);
        // Scroll so the caret shows, when it (or the text) changed: not all
        // the time, or the view couldn't be scrolled away from it.
        let now = (e.key, at.anchor, at.focus, layout.len);
        let moved = self.followed != Some(now);
        self.followed = Some(now);
        let keep = |offset: &mut f64, lo: f64, hi: f64, visible: f64, total: f64| {
            if moved {
                if hi - *offset > visible {
                    *offset = hi - visible;
                } else if lo < *offset {
                    *offset = lo;
                }
            }
            *offset = offset.min((total - visible).max(0.0)).max(0.0);
        };
        match node.kind {
            Kind::CodeEditor { .. } => {
                let (cx0, cx1, cy0, cy1) = caret.map_or((0.0, 0.0, 0.0, 16.0), |c| (c.x0, c.x1, c.y0, c.y1));
                let visible_w = (content.w - node.layout.gutter - 2.0 * render::CODE_PAD_X).max(1.0);
                let visible_h = (content.h - 2.0 * render::CODE_PAD_Y).max(1.0);
                let sx = self.ui.scroll_x.entry(e.key).or_insert(0.0);
                keep(sx, cx0, cx1, visible_w, layout.width() + 2.0);
                let sy = self.ui.scroll.entry(e.key).or_insert(0.0);
                keep(sy, cy0, cy1, visible_h, layout.height());
            }
            _ => {
                let caret_x = caret.map_or(0.0, |c| c.x0);
                let visible = (node.layout.rect.w - 16.0).max(1.0);
                keep(&mut e.scroll, caret_x, caret_x, visible, layout.width());
            }
        }
    }

    /// Runs `.onResize` for views whose size isn't what they were last told
    /// (or that were never told). Returns whether any ran.
    fn run_on_resize(&mut self) -> bool {
        fn visit(n: &Node, sizes: &HashMap<u64, (f64, f64)>, out: &mut Vec<(u64, f64, f64, Callback)>) {
            if n.mods.hidden {
                return;
            }
            if let Some(cb) = &n.mods.on_resize {
                let c = n.layout.rect.inset_by(n.mods.padding);
                if sizes.get(&n.key) != Some(&(c.w, c.h)) {
                    out.push((n.key, c.w, c.h, cb.clone()));
                }
            }
            for c in n.visible_children() {
                visit(c, sizes, out);
            }
        }
        let mut changed = Vec::new();
        if let Some(t) = &self.tree {
            visit(t, &self.sizes, &mut changed);
        }
        self.sizes.retain(|key, _| self.keys.contains(key));
        let ran = !changed.is_empty();
        for (key, w, h, cb) in changed {
            self.sizes.insert(key, (w, h));
            cb.call_with_floats(w, h);
            self.dirty = true;
        }
        ran
    }

    /// After layout: the rows each `LazyVStack` should build, from the part
    /// of it that shows (inside the window and the scroll views around it).
    /// When those aren't the ones it built, it's to be built again: returns
    /// whether any is.
    fn lazy_rows(&mut self) -> bool {
        fn visit(n: &Node, clip: Rect, out: &mut Vec<(u64, usize, usize)>, seen: &mut Vec<u64>) {
            if n.mods.hidden {
                return;
            }
            let r = n.layout.rect;
            let mut clip = clip;
            if matches!(n.kind, Kind::Scroll) || n.mods.clipped {
                let c = r.inset_by(n.mods.padding);
                let (x0, y0) = (clip.x.max(c.x), clip.y.max(c.y));
                let (x1, y1) = ((clip.x + clip.w).min(c.x + c.w), (clip.y + clip.h).min(c.y + c.h));
                clip = Rect { x: x0, y: y0, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0) };
            }
            if let Kind::Lazy { count, row_height, first } = n.kind {
                seen.push(n.key);
                let c = r.inset_by(n.mods.padding);
                let (top, bottom) = (clip.y.max(c.y), (clip.y + clip.h).min(c.y + c.h));
                let built = (first, first + n.children.len());
                if bottom > top {
                    let from = ((top - c.y) / row_height).floor().max(0.0) as usize;
                    let to = (((bottom - c.y) / row_height).ceil() as usize).min(count);
                    let screen = (to - from).max(10);
                    // Rebuild when what shows isn't built, or far more is built than shows.
                    let missing = from < built.0 || to > built.1;
                    let wasteful = built.1 - built.0 > 5 * screen;
                    if missing || wasteful {
                        out.push((n.key, from.saturating_sub(screen), (to + screen).min(count)));
                    }
                }
            }
            for c in n.visible_children() {
                visit(c, clip, out, seen);
            }
        }
        let (mut wanted, mut seen) = (Vec::new(), Vec::new());
        if let Some(t) = &self.tree {
            visit(t, Rect { x: 0.0, y: 0.0, w: self.width, h: self.height }, &mut wanted, &mut seen);
        }
        super::controls::LAZY.with(|l| {
            let mut l = l.borrow_mut();
            l.retain(|k, _| seen.contains(k));
            for &(key, from, to) in &wanted {
                l.insert(key, (from, to));
            }
        });
        if !wanted.is_empty() {
            self.dirty = true;
        }
        !wanted.is_empty()
    }

    /// Runs `.onAppear` for views that weren't in the previous tree.
    fn run_on_appear(&mut self) {
        type Keys = std::collections::HashSet<u64>;
        fn visit(n: &Node, keys: &mut Keys, old: &Keys, out: &mut Vec<Callback>, focus: &mut Option<(u64, String)>) {
            keys.insert(n.key);
            let new = !old.contains(&n.key);
            if let (Some(cb), true) = (&n.mods.on_appear, new) {
                out.push(cb.clone());
            }
            if let (true, true, Some(text)) = (n.mods.autofocus, new, editable_text(n)) {
                *focus = Some((n.key, read_string(text)));
            }
            for c in &n.children {
                visit(c, keys, old, out, focus);
            }
        }
        let mut keys = std::collections::HashSet::new();
        let mut appeared = Vec::new();
        // In every window: the view to focus in each.
        let mut focus = Vec::new();
        let trees = std::iter::once((self.current, &self.tree)).chain(self.others.iter().map(|s| (s.key, &s.tree)));
        for (window, tree) in trees {
            let mut found = None;
            if let Some(t) = tree {
                visit(t, &mut keys, &self.keys, &mut appeared, &mut found);
            }
            focus.extend(found.map(|f| (window, f)));
        }
        self.keys = keys;
        super::canvas::keep_only(&self.keys);
        let back = self.current;
        for (window, (key, text)) in focus {
            if self.enter(window) {
                let mut e = Editing::new(key, &text);
                // A color picker's hex field starts with its text selected, to type over.
                if Some(key) == super::controls::HEX_FIELD.get() {
                    e.anchor = 0;
                }
                self.ui.editing = Some(e);
            }
        }
        self.enter(back);
        for cb in appeared {
            cb.call();
            self.dirty = true;
        }
    }

    /// Two-way sync of code editors' `line:` bindings: when the program sets
    /// a new line, move the caret there.
    /// Changes the program makes to a code editor's text can be undone too,
    /// like typing: each becomes a step of the editor's undo history.
    fn track_editor_texts(&mut self) {
        fn editors(n: &Node, out: &mut Vec<(u64, String)>) {
            if let Kind::CodeEditor { text, .. } = &n.kind {
                out.push((n.key, read_string(text)));
            }
            n.children.iter().for_each(|c| editors(c, out));
        }
        let mut found = Vec::new();
        if let Some(t) = &self.tree {
            editors(t, &mut found);
        }
        for (key, text) in found {
            let Some(h) = self.ui.history.get_mut(&key) else {
                // (First seen: what it has is where its history starts.)
                self.ui.history.insert(key, edit::History { last_text: text, ..Default::default() });
                continue;
            };
            if h.last_text != text {
                // Undoing puts the caret where the change began.
                let at = common_prefix(&h.last_text, &text);
                let before = std::mem::replace(&mut h.last_text, text);
                h.undo.push(edit::Snapshot { text: before, anchor: at, focus: at });
                h.redo.clear();
                h.typing = false;
                self.ui.last_edited = Some(key);
            }
        }
    }

    fn sync_editor_lines(&mut self) {
        fn editors(n: &Node, out: &mut Vec<(u64, Callback)>) {
            if let Kind::CodeEditor { line: Some(line), .. } = &n.kind {
                out.push((n.key, line.clone()));
            }
            n.children.iter().for_each(|c| editors(c, out));
        }
        let mut found = Vec::new();
        if let Some(t) = &self.tree {
            editors(t, &mut found);
        }
        for (key, line) in found {
            let wanted = unsafe { *(line.locate() as *const i64) };
            match self.ui.editor_line.insert(key, wanted) {
                Some(known) if known != wanted => self.go_to_line(key, wanted),
                _ => {}
            }
        }
    }

    /// Puts the caret at the start of `line` (from 1) in a code editor.
    fn go_to_line(&mut self, key: u64, line: i64) {
        let Some(text) = self.node(key).and_then(editable_text).map(read_string) else { return };
        let offset = if line <= 1 {
            0
        } else {
            text.match_indices('\n').nth((line - 2) as usize).map_or(text.len(), |(i, _)| i + 1)
        };
        // (While a view with `.onKey` has the keyboard, it keeps it.)
        if self.ui.keys_to.is_some() {
            self.ui.scroll_to = Some((key, offset));
            return;
        }
        let mut e = Editing::new(key, &text);
        e.anchor = offset;
        e.focus = offset;
        self.ui.editing = Some(e);
    }

    /// Scrolls a code editor so that a character shows, a third of the way
    /// down (see `go_to_line`).
    fn scroll_to_offset(&mut self) {
        let Some((key, offset)) = self.ui.scroll_to.take() else { return };
        let Some(node) = self.node(key) else { return };
        let Some(layout) = &node.layout.text else { return };
        let at = Editing { key, anchor: offset, focus: offset, ..Default::default() };
        let (caret, _) = edit::geometry(layout, &at);
        let Some(c) = caret else { return };
        let content = node.layout.rect.inset_by(node.mods.padding);
        let visible_h = (content.h - 2.0 * render::CODE_PAD_Y).max(1.0);
        let total = layout.height();
        let y = (c.y0 - visible_h / 3.0).min((total - visible_h).max(0.0)).max(0.0);
        self.ui.scroll.insert(key, y);
        self.ui.scroll_x.insert(key, 0.0);
    }

    /// After the caret moved: report its line to the editor's `line:` binding.
    fn report_line(&mut self) {
        let Some(e) = self.ui.editing.as_ref() else { return };
        let key = e.key;
        let focus = e.focus;
        let Some(Kind::CodeEditor { text, line: Some(line), .. }) = self.node(key).map(|n| &n.kind) else { return };
        let text = read_string(text);
        let current = text[..focus.min(text.len())].matches('\n').count() as i64 + 1;
        let slot = line.locate() as *mut i64;
        if unsafe { *slot } != current {
            unsafe { *slot = current };
            self.ui.editor_line.insert(key, current);
            self.dirty = true;
        }
    }

    /// Where the text of an editable view starts, and its (x, y) scroll.
    fn text_origin(&self, node: &Node) -> (f64, f64, f64, f64) {
        match node.kind {
            Kind::CodeEditor { .. } => {
                let (x0, y0) = code_origin(node);
                let sx = self.ui.scroll_x.get(&node.key).copied().unwrap_or(0.0);
                let sy = self.ui.scroll.get(&node.key).copied().unwrap_or(0.0);
                (x0, y0, sx, sy)
            }
            _ => {
                let (x0, y0) = field_origin(node);
                let sx = self.ui.editing.as_ref().filter(|e| e.key == node.key).map_or(0.0, |e| e.scroll);
                (x0, y0, sx, 0.0)
            }
        }
    }

    fn editing_code(&self) -> bool {
        let key = self.ui.editing.as_ref().map(|e| e.key);
        key.and_then(|k| self.node(k)).is_some_and(|n| matches!(n.kind, Kind::CodeEditor { .. }))
    }

    /// The edited text, and the text of the caret's line up to the caret.
    fn line_before_caret(&self) -> Option<String> {
        let e = self.ui.editing.as_ref()?;
        let text = read_string(editable_text(self.node(e.key)?)?);
        let caret = e.range().start.min(text.len());
        let line_start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
        Some(text[line_start..caret].to_string())
    }

    /// Types text. In code, Enter keeps the indentation (one more level after
    /// `{`) and `}` at the start of a line removes one level.
    fn type_text(&mut self, text: &str) {
        self.ui.composing = None;
        // An open menu takes the keys.
        if self.menu.is_some() {
            if text == " " {
                self.menu_key("space", Mods::default());
            }
            return;
        }
        // With no text being edited, a typed character is a key for `.onKey`.
        if self.ui.editing.is_none() {
            self.run_on_key(text);
            return;
        }
        if self.editing_code() {
            let before = self.line_before_caret().unwrap_or_default();
            if text == "\n" {
                let indent: String = before.chars().take_while(|c| *c == ' ').collect();
                let extra = if before.trim_end().ends_with('{') { "    " } else { "" };
                self.edit(Edit::Insert(format!("\n{indent}{extra}")));
                return;
            }
            if text == "}" && before.len() >= 4 && before.chars().all(|c| c == ' ') {
                if let Some(e) = self.ui.editing.as_mut() {
                    let caret = e.range().start;
                    e.anchor = caret - 4;
                    e.focus = caret;
                }
            }
        }
        self.edit(Edit::Insert(text.to_string()));
        if self.editing_code() {
            self.after_typing(text);
        }
    }

    /// The window's drawing, in pixels (`scale` to the point).
    fn scene(&mut self, scale: f64) -> super::draw::DrawList {
        self.update();
        let mut list = super::draw::DrawList::new();
        let tree = self.tree.as_ref().unwrap();
        let mut painter = Painter { scene: &mut list, ui: &self.ui, scale, offset: (0.0, 0.0), clip: None };
        painter.paint(tree);
        painter.paint_overlay(tree);
        painter.paint_dragged(tree);
        painter.paint_popup();
        painter.paint_menu_bar();
        painter.paint_menus();
        painter.paint_tooltip();
        list
    }

    fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<Hit> {
            // Nothing of a clipped view shows outside it.
            if n.mods.clipped && !n.layout.rect.contains(x, y) {
                return None;
            }
            if n.mods.hidden || n.layout.style.disabled || !n.layout.rect.contains(x, y) {
                // Scroll content can extend outside; only stop at our own bounds for leaves.
                if n.mods.hidden || n.layout.style.disabled || n.children.is_empty() {
                    return None;
                }
            }
            if let Kind::TabView { .. } = n.kind {
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    if tab.close.is_some_and(|c| c.contains(x, y)) {
                        return Some(Hit::TabClose(n.key, i));
                    }
                    if tab.rect.contains(x, y) {
                        return Some(Hit::Tab(n.key, i));
                    }
                }
                return n.children.get(n.selected_tab()).and_then(|c| visit(c, x, y));
            }
            if let Kind::Picker { .. } = n.kind {
                if let Some(i) = n.layout.segments.iter().position(|s| s.contains(x, y)) {
                    return Some(Hit::Segment(n.key, i));
                }
                return None;
            }
            // (Its palette is found first, by `overlay_hit`.)
            if let Kind::ColorPicker { .. } = n.kind {
                return n.layout.rect.contains(x, y).then_some(Hit::ColorPicker(n.key));
            }
            if let Kind::Disclosure { .. } = n.kind {
                if n.layout.segments.first().is_some_and(|h| h.contains(x, y)) {
                    return Some(Hit::Disclosure(n.key));
                }
            }
            for c in n.children.iter().rev() {
                if let Some(h) = visit(c, x, y) {
                    return Some(h);
                }
            }
            if !n.layout.rect.contains(x, y) {
                return None;
            }
            if let (Kind::Markdown { .. }, Some(laid)) = (&n.kind, &n.layout.markdown) {
                let c = n.layout.content;
                if let Some(link) = laid.link_at(x - c.x, y - c.y) {
                    return Some(Hit::Link(n.key, link));
                }
            }
            match n.kind {
                Kind::Button { .. } => Some(Hit::Button(n.key)),
                Kind::Toggle { .. } => Some(Hit::Toggle(n.key)),
                Kind::TextField { .. } | Kind::CodeEditor { .. } => Some(Hit::Field(n.key)),
                Kind::Slider { .. } => Some(Hit::Slider(n.key)),
                Kind::Stepper { .. } => {
                    let r = n.layout.rect.inset_by(n.mods.padding);
                    let b = super::layout::STEPPER_BUTTON;
                    let dir = if x < r.x + b { -1 } else if x >= r.x + r.w - b { 1 } else { 0 };
                    Some(Hit::Stepper(n.key, dir))
                }
                Kind::Swatch(_) => Some(Hit::Swatch(n.key)),
                _ if n.mods.on_pointer.is_some() => Some(Hit::Pointer(n.key)),
                _ if n.mods.on_drag.is_some() => Some(Hit::Drag(n.key)),
                _ if n.mods.on_tap.is_some() => Some(Hit::Tap(n.key)),
                _ => None,
            }
        }
        // An open color picker's palette is over everything else.
        if let Some(palette) = self.palette() {
            if palette.layout.rect.contains(x, y) {
                return Some(visit(palette, x, y).unwrap_or(Hit::Palette(palette.key)));
            }
        }
        visit(self.tree.as_ref()?, x, y)
    }

    /// The open color picker's palette, if one is open.
    fn palette(&self) -> Option<&Node> {
        self.node(super::controls::OPEN_PICKER.get()?)?.children.first()
    }

    /// Moves the pointer to a point (as the window does on mouse moves).
    pub fn hover(&mut self, x: f64, y: f64) {
        self.ui.pointer = Some((x, y));
        self.menu_hover(x, y);
        self.tip_move(x, y);
        self.ui.hovered = self.hit(x, y).map(|h| h.key());
        self.ui.hover_background = self.hover_background_at(x, y);
    }

    /// The innermost visible view with `.hoverBackground` containing the point.
    pub fn hover_background_at(&self, x: f64, y: f64) -> Option<u64> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<u64> {
            if n.mods.hidden {
                return None;
            }
            let children: Vec<&Node> = match n.kind {
                Kind::TabView { .. } => n.children.get(n.selected_tab()).into_iter().collect(),
                _ => n.children.iter().collect(),
            };
            if let Some(k) = children.iter().rev().find_map(|c| visit(c, x, y)) {
                return Some(k);
            }
            (n.mods.hover_background.is_some() && n.layout.rect.contains(x, y)).then_some(n.key)
        }
        visit(self.tree.as_ref()?, x, y)
    }

    fn node(&self, key: u64) -> Option<&Node> {
        self.tree.as_ref()?.find(key)
    }

    /// The view with `.onDrag` at a point, if a drag can start there.
    fn drag_at(&self, x: f64, y: f64) -> Option<u64> {
        match self.hit(x, y) {
            Some(Hit::Drag(key)) => Some(key),
            _ => None,
        }
    }

    /// The mouse moved by (dx, dy) while dragging view `key`.
    fn drag(&mut self, key: u64, dx: f64, dy: f64) {
        if let Some(on_drag) = self.node(key).and_then(|n| n.mods.on_drag.clone()) {
            on_drag.call_with_floats(dx, dy);
            self.dirty = true;
        }
    }

    /// The view with `.onPointer` at a point.
    /// The innermost shown view with `.onKey` that a point is in: a click
    /// there gives it the keyboard.
    fn on_key_at(&self, x: f64, y: f64) -> Option<u64> {
        fn find(n: &Node, x: f64, y: f64, found: &mut Option<u64>) {
            if n.mods.hidden || n.layout.style.disabled {
                return;
            }
            let r = n.layout.rect;
            if x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h {
                return;
            }
            if n.mods.on_key.is_some() {
                *found = Some(n.key);
            }
            n.visible_children().iter().for_each(|c| find(c, x, y, found));
        }
        let mut found = None;
        if let Some(t) = &self.tree {
            find(t, x, y, &mut found);
        }
        found
    }

    fn pointer_at(&self, x: f64, y: f64) -> Option<u64> {
        match self.hit(x, y) {
            Some(Hit::Pointer(key)) => Some(key),
            _ => None,
        }
    }

    /// Tells view `key` what the mouse did, at `at` in the window (the view
    /// gets it from its own top left corner, inside its padding).
    fn pointer(&mut self, key: u64, kind: i32, at: (f64, f64), delta: (f64, f64), button: i32, clicks: i64, mods: Mods) {
        let Some(node) = self.node(key) else { return };
        let Some(on_pointer) = node.mods.on_pointer.clone() else { return };
        let origin = node.layout.rect.inset_by(node.mods.padding);
        // Pressing a view like a canvas ends text editing, and gives the
        // keyboard to a view with `.onKey` it's in (or is).
        if kind == DOWN {
            self.ui.editing = None;
            self.ui.keys_to = self.on_key_at(at.0, at.1);
        }
        let mut event = super::PointerC {
            kind,
            button,
            clicks,
            x: at.0 - origin.x,
            y: at.1 - origin.y,
            dx: delta.0,
            dy: delta.1,
            shift: mods.shift as u8,
            alt: mods.alt as u8,
            command: mods.primary as u8,
            window_x: at.0,
            window_y: at.1,
        };
        on_pointer.call_with(&mut event as *mut super::PointerC as *mut u8);
        self.dirty = true;
    }

    /// The mouse moved (no button held) to a point: the view with
    /// `.onPointer` there is told, and the one it left. Returns whether any was.
    fn pointer_move(&mut self, x: f64, y: f64, delta: (f64, f64), mods: Mods) -> bool {
        let now = self.pointer_at(x, y);
        let before = self.pointer_over;
        if let Some(old) = before.filter(|old| Some(*old) != now) {
            self.pointer(old, LEAVE, (x, y), (0.0, 0.0), 0, 0, mods);
        }
        if let Some(key) = now {
            self.pointer(key, MOVE, (x, y), delta, 0, 0, mods);
        }
        self.pointer_over = now;
        now.is_some() || before.is_some()
    }

    /// The mouse left the window.
    fn pointer_leave(&mut self, mods: Mods) {
        if let Some(old) = self.pointer_over.take() {
            let at = self.ui.pointer.unwrap_or((0.0, 0.0));
            self.pointer(old, LEAVE, at, (0.0, 0.0), 0, 0, mods);
        }
    }

    /// The pointer a view asks for with `.cursor(…)`: the innermost such
    /// view at the point.
    fn cursor_at(&self, x: f64, y: f64) -> Option<winit::window::CursorIcon> {
        use winit::window::CursorIcon;
        fn visit(n: &Node, x: f64, y: f64) -> Option<u8> {
            if n.mods.hidden || !n.layout.rect.contains(x, y) && n.children.is_empty() {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| visit(c, x, y));
            inner.or(n.mods.cursor.filter(|_| n.layout.rect.contains(x, y)))
        }
        // In the order of the `CursorStyle` enum.
        const STYLES: [CursorIcon; 13] = [
            CursorIcon::Default,
            CursorIcon::Pointer,
            CursorIcon::Crosshair,
            CursorIcon::Grab,
            CursorIcon::Grabbing,
            CursorIcon::Text,
            CursorIcon::Move,
            CursorIcon::Wait,
            CursorIcon::NotAllowed,
            CursorIcon::EwResize,
            CursorIcon::NsResize,
            CursorIcon::ZoomIn,
            CursorIcon::ZoomOut,
        ];
        let style = visit(self.tree.as_ref()?, x, y)?;
        STYLES.get(style as usize).copied()
    }

    /// The innermost view with a `.tooltip` at a point: its key and text.
    fn tooltip_at(&self, x: f64, y: f64) -> Option<(u64, String)> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<(u64, String)> {
            if n.mods.hidden || (n.mods.clipped || n.children.is_empty()) && !n.layout.rect.contains(x, y) {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| visit(c, x, y));
            inner.or_else(|| n.mods.tooltip.clone().filter(|_| n.layout.rect.contains(x, y)).map(|t| (n.key, t)))
        }
        visit(self.tree.as_ref()?, x, y)
    }

    /// The mouse moved to a point: a tooltip starts waiting when it comes
    /// onto a view that has one, and goes when it leaves.
    fn tip_move(&mut self, x: f64, y: f64) {
        let over = self.tooltip_at(x, y).map(|(key, _)| key);
        if over != self.tip.as_ref().map(|t| t.key) {
            self.tip = over.map(|key| Tip { key, since: now(), done: false });
            self.ui.tooltip = None;
        }
    }

    /// A click, a key or the wheel: the tooltip goes, until the mouse has
    /// left the view.
    fn tip_hide(&mut self) {
        if let Some(t) = self.tip.as_mut() {
            t.done = true;
        }
        self.ui.tooltip = None;
    }

    /// When the waiting tooltip is to show (on the app's clock), if one waits.
    fn tip_time(&self) -> Option<f64> {
        self.tip.as_ref().filter(|t| !t.done).map(|t| t.since + TIP_DELAY)
    }

    /// Shows the tooltip if the mouse has rested long enough. Returns
    /// whether it just came up.
    fn tip_due(&mut self) -> bool {
        let Some(tip) = self.tip.as_ref().filter(|t| !t.done && now() >= t.since + TIP_DELAY) else { return false };
        let Some(text) = self.node(tip.key).and_then(|n| n.mods.tooltip.clone()) else {
            self.tip = None;
            return false;
        };
        let layout = Rc::new(self.text.layout(&text, 12.0, 400.0, theme().colors[theme::PRIMARY], Some(280.0)));
        let (w, h) = (layout.width() + 16.0, layout.height() + 10.0);
        // Below and to the right of the pointer, inside the window.
        let (px, py) = self.ui.pointer.unwrap_or((0.0, 0.0));
        let x = (px + 12.0).min(self.width - w - 4.0).max(4.0);
        let y = if py + 20.0 + h > self.height - 4.0 { (py - h - 8.0).max(4.0) } else { py + 20.0 };
        self.ui.tooltip = Some(render::Tooltip { rect: Rect { x, y, w, h }, text, layout });
        if let Some(t) = self.tip.as_mut() {
            t.done = true;
        }
        true
    }

    /// A file dropped on the window: the first visible view with `.onDrop`
    /// gets its path. Returns whether one did.
    fn drop_file(&mut self, path: &str) -> bool {
        fn find(n: &Node) -> Option<Callback> {
            if n.mods.hidden || n.layout.style.disabled {
                return None;
            }
            if let Some(cb) = &n.mods.on_drop {
                return Some(cb.clone());
            }
            n.visible_children().iter().find_map(find)
        }
        let Some(cb) = self.tree.as_ref().and_then(find) else { return false };
        cb.call_with(crate::str_new(path) as *mut u8);
        self.dirty = true;
        true
    }

    // ===== dragging views onto drop targets =====

    /// The innermost visible, enabled view with `.draggable` (or
    /// `.dropTarget`, with `target`) at a point.
    fn drag_view_at(&self, x: f64, y: f64, target: bool) -> Option<u64> {
        fn visit(n: &Node, x: f64, y: f64, target: bool) -> Option<u64> {
            if n.mods.hidden || n.layout.style.disabled || (n.mods.clipped || n.children.is_empty()) && !n.layout.rect.contains(x, y) {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| visit(c, x, y, target));
            let has = if target { n.mods.drop_target.is_some() } else { n.mods.draggable.is_some() };
            inner.or_else(|| (has && n.layout.rect.contains(x, y)).then_some(n.key))
        }
        if self.palette().is_some_and(|p| p.layout.rect.contains(x, y)) {
            return None;
        }
        visit(self.tree.as_ref()?, x, y, target)
    }

    /// Starts dragging view `key` (which has `.draggable`), held at (x, y).
    fn begin_drag(&mut self, key: u64, x: f64, y: f64) {
        let Some(n) = self.node(key) else { return };
        let Some(payload) = n.mods.draggable.clone() else { return };
        let r = n.layout.rect;
        self.ui.dragging = Some(render::Dragging { key, payload, at: (x, y), grab: (x - r.x, y - r.y), target: None });
        self.ui.pressed = None;
        self.ui.editing = None;
        self.tip_hide();
        self.drag_to(x, y);
    }

    /// The view being dragged moves with the pointer to (x, y): the drop
    /// target under it (if any) is told it's targeted, and the one it left that it isn't.
    fn drag_to(&mut self, x: f64, y: f64) {
        let target = self.drag_view_at(x, y, true);
        let Some(d) = self.ui.dragging.as_mut() else { return };
        d.at = (x, y);
        let old = std::mem::replace(&mut d.target, target);
        if old != target {
            if let Some(old) = old {
                self.set_targeted(old, false);
            }
            if let Some(new) = target {
                self.set_targeted(new, true);
            }
        }
        self.dirty = true;
    }

    /// Sets a drop target's `isTargeted:` binding.
    fn set_targeted(&mut self, key: u64, on: bool) {
        let Some(t) = self.node(key).and_then(|n| n.mods.drop_target.as_ref()) else { return };
        if let Some(b) = &t.targeted {
            unsafe { *b.locate() = on as u8 };
        }
    }

    /// The drag ends: let go at `at` (the target under it gets the
    /// payload), or cancelled (`None`).
    fn end_drag(&mut self, at: Option<(f64, f64)>) {
        if let Some((x, y)) = at {
            self.drag_to(x, y);
        }
        let Some(d) = self.ui.dragging.take() else { return };
        self.dirty = true;
        let Some(target) = d.target else { return };
        self.set_targeted(target, false);
        let (Some((x, y)), Some(n)) = (at, self.node(target)) else { return };
        let Some(t) = n.mods.drop_target.as_ref() else { return };
        let origin = n.layout.rect.inset_by(n.mods.padding);
        let action = t.action.clone();
        action.call_with_ptr_floats(crate::str_new(&d.payload) as *mut u8, x - origin.x, y - origin.y);
    }

    /// Runs `.onKey` with a key pressed while no text is edited: the first
    /// visible, enabled view with one gets it. Returns whether one did.
    fn run_on_key(&mut self, key: &str) -> bool {
        fn find(n: &Node) -> Option<Callback> {
            if n.mods.hidden || n.layout.style.disabled {
                return None;
            }
            if let Some(cb) = &n.mods.on_key {
                return Some(cb.clone());
            }
            n.visible_children().iter().find_map(find)
        }
        if key.is_empty() {
            return false;
        }
        // The view with the keyboard first, then the first one with `.onKey`.
        let to = self.ui.keys_to.and_then(|k| self.node(k)).filter(|n| !n.mods.hidden && !n.layout.style.disabled);
        let Some(cb) = to.and_then(|n| n.mods.on_key.clone()).or_else(|| self.tree.as_ref().and_then(find)) else { return false };
        cb.call_with(crate::str_new(key) as *mut u8);
        self.dirty = true;
        true
    }

    /// Runs `.onKeyEvent` with a key pressed while no text is edited: the
    /// first visible, enabled view with one gets it. Returns whether one did.
    fn run_on_key_event(&mut self, name: &str, text: &str, mods: Mods, repeated: bool) -> bool {
        fn find(n: &Node) -> Option<Callback> {
            if n.mods.hidden || n.layout.style.disabled {
                return None;
            }
            if let Some(cb) = &n.mods.on_key_event {
                return Some(cb.clone());
            }
            n.visible_children().iter().find_map(find)
        }
        if name.is_empty() {
            return false;
        }
        let Some(cb) = self.tree.as_ref().and_then(find) else { return false };
        let mut event = super::KeyC {
            key: crate::str_new(name),
            text: crate::str_new(text),
            shift: mods.shift as u8,
            alt: mods.alt as u8,
            command: mods.primary as u8,
            control: mods.control as u8,
            repeated: repeated as u8,
        };
        cb.call_with(&mut event as *mut super::KeyC as *mut u8);
        self.dirty = true;
        true
    }

    /// A key pressed, by its name (`"a"` for the A key whatever the
    /// modifiers, `"left"`, `"f5"`, `"space"`…) and the text it types
    /// (`"A"` with Shift; empty for keys that type nothing).
    fn press(&mut self, name: &str, text: &str, mods: Mods, repeated: bool) {
        use super::controls::{hex_field_text, parse_hex, HEX_FIELD, OPEN_PICKER};
        let plain = !(mods.primary || mods.alt || mods.control);
        // Escape puts away what's being dragged, then an open palette.
        if name == "escape" && self.ui.dragging.is_some() {
            self.end_drag(None);
            return;
        }
        if name == "escape" && OPEN_PICKER.get().is_some() {
            self.close_picker();
            return;
        }
        // Enter in the palette's hex field picks that color.
        let editing = self.ui.editing.as_ref().map(|e| e.key);
        if name == "enter" && editing.is_some() && editing == HEX_FIELD.get() {
            match parse_hex(&hex_field_text()) {
                Some(c) => self.pick(c),
                None => self.dirty = true,
            }
            return;
        }
        // Shortcuts first, then the control the keyboard is on, then
        // `.onKeyEvent` (while no text is edited).
        if self.ui.editing.is_none() {
            if mods.primary && self.run_shortcut(name, mods) {
                return;
            }
            if plain && self.control_key(name) {
                return;
            }
            if self.run_on_key_event(name, text, mods, repeated) {
                return;
            }
        }
        if EDIT_KEYS.contains(&name) || mods.primary {
            self.key(name, mods);
        } else if !text.is_empty() && !text.chars().any(char::is_control) {
            self.type_text(text);
        } else if self.ui.editing.is_none() {
            // Keys that type nothing (F5, Page Down…) go to `.onKey` by name.
            self.run_on_key(name);
        }
    }

    /// The pointer for a view with `.onDrag` under it: by the view's shape,
    /// since a tall thin one is dragged sideways and a wide flat one up and
    /// down.
    fn drag_cursor(&self, x: f64, y: f64) -> Option<winit::window::CursorIcon> {
        use winit::window::CursorIcon;
        let r = self.node(self.drag_at(x, y)?)?.layout.rect;
        Some(if r.h >= 2.0 * r.w {
            CursorIcon::ColResize
        } else if r.w >= 2.0 * r.h {
            CursorIcon::RowResize
        } else {
            CursorIcon::Grab
        })
    }

    /// A completed click at (x, y).
    fn click(&mut self, x: f64, y: f64) {
        if self.popup_click(x, y) {
            return;
        }
        let hit = self.hit(x, y);
        if !matches!(hit, Some(Hit::Field(_))) {
            self.ui.editing = None;
        }
        let on_palette = self.close_palette_unless(&hit);
        // The keyboard goes to the control clicked.
        self.ui.focused = match &hit {
            Some(Hit::Slider(k) | Hit::Stepper(k, _) | Hit::ColorPicker(k) | Hit::Disclosure(k)) => Some(*k),
            _ if on_palette => self.ui.focused,
            _ => None,
        };
        self.ui.focus_ring = false;
        // A click in a view with `.onKey` gives it the keyboard.
        self.ui.keys_to = if matches!(hit, Some(Hit::Field(_))) { None } else { self.on_key_at(x, y) };
        let Some(hit) = hit else { return };
        let Some(node) = self.node(hit.key()) else { return };
        match (&hit, &node.kind) {
            (Hit::Slider(key), Kind::Slider { .. }) => {
                let key = *key;
                self.slide(key, x);
            }
            (Hit::Stepper(key, dir), Kind::Stepper { .. }) => {
                let (key, dir) = (*key, *dir);
                self.step(key, dir as f64);
            }
            (Hit::ColorPicker(key), _) => {
                let key = *key;
                self.toggle_picker(key, false);
            }
            (Hit::Swatch(_), Kind::Swatch(c)) => {
                let c = *c;
                self.pick(c);
            }
            (Hit::Disclosure(_), Kind::Disclosure { expanded, .. }) => {
                let p = expanded.locate();
                unsafe { *p = (*p & 1) ^ 1 };
            }
            (Hit::Button(_), Kind::Button { action, .. }) => {
                let action = action.clone();
                action.call();
            }
            (Hit::Toggle(_), Kind::Toggle { is_on, .. }) => {
                let p = is_on.locate();
                unsafe { *p = (*p & 1) ^ 1 };
            }
            (Hit::Field(key), Kind::TextField { text, .. } | Kind::CodeEditor { text, .. }) => {
                if self.ui.editing.as_ref().is_none_or(|e| e.key != *key) {
                    self.ui.editing = Some(Editing::new(*key, &read_string(text)));
                }
            }
            (Hit::Tab(_, i), Kind::TabView { selection, .. }) => {
                if let Some(tag) = node.children.get(*i).and_then(|c| c.mods.tag.as_ref()) {
                    unsafe { tag.store_to(selection.locate()) };
                }
            }
            (Hit::TabClose(_, i), Kind::TabView { close: Some(close), .. }) => {
                if let Some(tag) = node.children.get(*i).and_then(|c| c.mods.tag.as_ref()) {
                    // The adapter copies (and retains) the value it's given.
                    let mut bytes = tag.bytes.clone();
                    let close = close.clone();
                    close.call_with(bytes.as_mut_ptr());
                }
            }
            (Hit::Segment(_, i), Kind::Picker { selection }) => {
                if let Some(tag) = node.children.get(*i).and_then(|c| c.mods.tag.as_ref()) {
                    unsafe { tag.store_to(selection.locate()) };
                }
            }
            (Hit::Tap(_), _) => {
                if let Some(tap) = node.mods.on_tap.clone() {
                    tap.call();
                }
            }
            // A link goes to the view's `onLink`; without one, links to the
            // web open in the browser.
            (Hit::Link(_, i), Kind::Markdown { on_link, .. }) => {
                let url = node.layout.markdown.as_ref().and_then(|laid| laid.links.get(*i)).map(|l| l.url.clone());
                match (url, on_link.clone()) {
                    (Some(url), Some(on_link)) => on_link.call_with(crate::str_new(&url) as *mut u8),
                    (Some(url), None) if url.starts_with("http://") || url.starts_with("https://") => {
                        let s = crate::str_new(&url);
                        unsafe {
                            crate::stdlib::tsl_open_url(s);
                            crate::tsl_release(s as *mut Header);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    // ===== sliders, steppers, color pickers =====

    /// Moves a slider's knob to `x` in the window.
    fn slide(&mut self, key: u64, x: f64) {
        let Some(n) = self.node(key) else { return };
        let Kind::Slider { value, from, to, step, on_change } = &n.kind else { return };
        let c = n.layout.rect.inset_by(n.mods.padding);
        let knob = super::layout::KNOB;
        let t = (x - c.x - knob) / (c.w - 2.0 * knob).max(1.0);
        let v = super::controls::slider_value(*from, *to, *step, t);
        if super::controls::write_number(value, false, v) {
            if let Some(cb) = on_change.clone() {
                cb.call();
            }
        }
        self.dirty = true;
    }

    /// Moves a slider (by a twentieth of its range, or a step) or a stepper
    /// (by its step) up (`dir` 1) or down (-1). Returns whether it's one.
    fn step(&mut self, key: u64, dir: f64) -> bool {
        let Some(n) = self.node(key) else { return false };
        let (value, int, step, from, to, on_change) = match &n.kind {
            Kind::Stepper { value, int, step, from, to, on_change, .. } => (value, *int, *step, *from, *to, on_change),
            Kind::Slider { value, from, to, step, on_change } => {
                let by = if *step > 0.0 { *step } else { (to - from) / 20.0 };
                (value, false, by, *from, *to, on_change)
            }
            _ => return false,
        };
        let v = super::controls::stepped(super::controls::read_number(value, int), step, dir, from, to);
        if super::controls::write_number(value, int, v) {
            if let Some(cb) = on_change.clone() {
                cb.call();
            }
        }
        self.dirty = true;
        true
    }

    /// Opens a color picker's palette, or closes it. From the keyboard, its
    /// hex field starts out being edited.
    fn toggle_picker(&mut self, key: u64, keyboard: bool) {
        use super::controls::{hex, read_color, set_hex_field_text, FOCUS_HEX, OPEN_PICKER};
        if OPEN_PICKER.get() == Some(key) {
            self.close_picker();
            return;
        }
        let Some(Kind::ColorPicker { color, .. }) = self.node(key).map(|n| &n.kind) else { return };
        set_hex_field_text(&hex(read_color(color)));
        OPEN_PICKER.set(Some(key));
        FOCUS_HEX.set(keyboard);
        self.dirty = true;
    }

    /// A press outside an open palette (and its picker) closes it. Returns
    /// whether it's on the palette.
    fn close_palette_unless(&mut self, hit: &Option<Hit>) -> bool {
        let open = super::controls::OPEN_PICKER.get();
        let on_palette = matches!(hit, Some(Hit::Swatch(_) | Hit::Palette(_)))
            || matches!(hit, Some(Hit::Field(k)) if Some(*k) == super::controls::HEX_FIELD.get());
        if open.is_some() && !on_palette && !matches!(hit, Some(Hit::ColorPicker(k)) if Some(*k) == open) {
            self.close_picker();
        }
        on_palette
    }

    fn close_picker(&mut self) {
        use super::controls::{HEX_FIELD, OPEN_PICKER};
        if OPEN_PICKER.take().is_some() {
            if self.ui.editing.as_ref().is_some_and(|e| Some(e.key) == HEX_FIELD.get()) {
                self.ui.editing = None;
            }
            HEX_FIELD.set(None);
            self.dirty = true;
        }
    }

    /// A color chosen in the open palette (a swatch, or the hex field):
    /// the picker gets it, and the palette closes.
    fn pick(&mut self, c: Rgba) {
        let Some(key) = super::controls::OPEN_PICKER.get() else { return };
        if let Some(Kind::ColorPicker { color, on_change }) = self.node(key).map(|n| &n.kind) {
            if super::controls::write_color(color, c) {
                if let Some(cb) = on_change.clone() {
                    cb.call();
                }
            }
        }
        self.close_picker();
        self.dirty = true;
    }

    /// Keys for the control the keyboard is on: arrows move a slider or
    /// stepper, open or close a disclosure group; Enter or Space opens a
    /// color picker or a disclosure group. Returns whether it took the key.
    fn control_key(&mut self, name: &str) -> bool {
        let Some(key) = self.ui.focused else { return false };
        let Some(n) = self.node(key) else {
            self.ui.focused = None;
            return false;
        };
        // Using the keyboard on it shows which control it's on.
        let took = matches!(
                (&n.kind, name),
                (Kind::Slider { .. } | Kind::Stepper { .. }, "right" | "up" | "left" | "down")
                    | (Kind::ColorPicker { .. }, "enter" | "space")
                    | (Kind::Disclosure { .. }, "enter" | "space" | "left" | "right")
            );
        if took {
            self.ui.focus_ring = true;
        }
        let Some(n) = self.node(key) else { return false };
        match (&n.kind, name) {
            (Kind::Slider { .. } | Kind::Stepper { .. }, "right" | "up") => self.step(key, 1.0),
            (Kind::Slider { .. } | Kind::Stepper { .. }, "left" | "down") => self.step(key, -1.0),
            (Kind::ColorPicker { .. }, "enter" | "space") => {
                self.toggle_picker(key, true);
                true
            }
            (Kind::Disclosure { expanded, .. }, "enter" | "space" | "left" | "right") => {
                let p = expanded.locate();
                let open = unsafe { *p } & 1 != 0;
                let want = match name {
                    "left" => false,
                    "right" => true,
                    _ => !open,
                };
                unsafe { *p = want as u8 };
                self.dirty = true;
                true
            }
            (_, "escape") => {
                self.ui.focused = None;
                true
            }
            _ => false,
        }
    }

    /// Applies an edit to the text field being edited.
    fn edit(&mut self, change: Edit) {
        let Some(mut e) = self.ui.editing.clone() else { return };
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(e.key)) else { return };
        let (Some(text), Some(layout)) = (editable_text(node), &node.layout.text) else { return };
        let current = read_string(text);
        let typing = matches!(&change, Edit::Insert(t) if t.chars().all(|c| c.is_alphanumeric() || c == '_'));
        let before = edit::Snapshot { text: current.clone(), anchor: e.anchor, focus: e.focus };
        let multiline = matches!(node.kind, Kind::CodeEditor { .. });
        if let Some(new) = edit::apply(&current, layout, &mut e, change, &mut self.clipboard, multiline) {
            let slot = text.locate() as *mut *mut Str;
            unsafe {
                let old = *slot;
                *slot = crate::str_new(&new);
                crate::tsl_release(old as *mut Header);
            }
            let h = self.ui.history.entry(e.key).or_default();
            if h.last_text != current {
                // Changed from outside since our last edit: old steps don't apply.
                h.undo.clear();
                h.redo.clear();
                h.typing = false;
            }
            if !(typing && h.typing) {
                h.undo.push(before);
            }
            h.typing = typing;
            h.redo.clear();
            h.last_text = new;
            self.ui.last_edited = Some(e.key);
            self.dirty = true;
        }
        self.ui.editing = Some(e);
        self.report_line();
        // Lay out again so the caret position is current.
        if !self.dirty {
            self.follow_caret();
        }
    }

    /// Undo (or redo) the last change in the view being edited, or, while
    /// nothing is, in the one whose text changed last. That one is then
    /// edited, with the caret where the change was.
    fn undo(&mut self, redo: bool) {
        let Some(key) = self.ui.editing.as_ref().map(|e| e.key).or(self.ui.last_edited) else { return };
        let Some(text) = self.node(key).and_then(editable_text).cloned() else { return };
        let current = read_string(&text);
        let editing = self.ui.editing.clone().filter(|e| e.key == key);
        let h = self.ui.history.entry(key).or_default();
        if h.last_text != current {
            h.undo.clear();
            h.redo.clear();
            return;
        }
        let (from, to) = if redo { (&mut h.redo, &mut h.undo) } else { (&mut h.undo, &mut h.redo) };
        let Some(step) = from.pop() else { return };
        let (anchor, focus) = match &editing {
            Some(e) => (e.anchor, e.focus),
            None => {
                let at = common_prefix(&current, &step.text);
                (at, at)
            }
        };
        to.push(edit::Snapshot { text: current, anchor, focus });
        h.last_text = step.text.clone();
        h.typing = false;
        let slot = text.locate() as *mut *mut Str;
        unsafe {
            let old = *slot;
            *slot = crate::str_new(&step.text);
            crate::tsl_release(old as *mut Header);
        }
        let mut e = editing.unwrap_or_else(|| Editing::new(key, &step.text));
        let valid = |at: usize| {
            let mut at = at.min(step.text.len());
            while !step.text.is_char_boundary(at) {
                at -= 1;
            }
            at
        };
        e.anchor = valid(step.anchor);
        e.focus = valid(step.focus);
        self.ui.editing = Some(e);
        self.ui.last_edited = Some(key);
        self.dirty = true;
    }

    /// Runs the `.shortcut` for Cmd/Ctrl + `key`, if a visible, enabled view
    /// has one. A shortcut asking for exactly the Shift and Alt keys held
    /// wins; otherwise the first whose keys are all held (so `"s"` also runs
    /// with Shift+Cmd+S, unless there's a `"shift+s"`).
    fn run_shortcut(&mut self, key: &str, mods: Mods) -> bool {
        fn find(n: &Node, key: &str, mods: Mods, exact: &mut Option<Callback>, loose: &mut Option<Callback>) {
            if n.mods.hidden || n.layout.style.disabled || exact.is_some() {
                return;
            }
            for s in n.mods.shortcuts.iter().filter(|s| s.key == key) {
                if s.shift == mods.shift && s.alt == mods.alt {
                    *exact = Some(s.action.clone());
                    return;
                }
                if loose.is_none() && (!s.shift || mods.shift) && (!s.alt || mods.alt) {
                    *loose = Some(s.action.clone());
                }
            }
            for c in n.visible_children() {
                find(c, key, mods, exact, loose);
            }
        }
        let (mut exact, mut loose) = (None, None);
        if let Some(t) = &self.tree {
            find(t, key, mods, &mut exact, &mut loose);
        }
        let Some(cb) = exact.or(loose) else { return false };
        cb.call();
        self.dirty = true;
        true
    }

    /// A key press, by name (see the script commands): an open menu takes
    /// it first, then the menu bar's shortcuts, then the views.
    fn key(&mut self, name: &str, mods: Mods) {
        if self.menu.is_some() {
            if !self.menu_key(name, mods) && mods.primary {
                self.menu = None;
                self.layout_menus();
                self.key(name, mods);
            }
            return;
        }
        // The editing keys edit text while it's edited.
        let editing = self.ui.editing.is_some() && mods.primary && !mods.alt && matches!(name, "a" | "c" | "x" | "v" | "z");
        let item = menu::find_shortcut(&self.bar, name, mods.primary, mods.shift, mods.alt).cloned();
        match item {
            Some(e @ Entry::Item { command: Command::User(_), .. }) if !editing => {
                self.run_entry(&e);
                return;
            }
            Some(Entry::Item {
                command: Command::Std(std @ (Std::Quit | Std::Hide | Std::HideOthers | Std::Minimize | Std::CloseWindow)),
                ..
            }) => {
                self.run_std(std);
                return;
            }
            _ => {}
        }
        if name == "escape" && self.ui.editing.is_none() && !mods.primary && self.close_top_popover() {
            return;
        }
        self.key_plain(name, mods);
    }

    /// A key press for the views (and the text being edited).
    fn key_plain(&mut self, name: &str, mods: Mods) {
        if !mods.primary && !mods.shift && self.completion_key(name) {
            return;
        }
        // Text editing keeps Cmd+A/C/X/V/Z while a field is being edited.
        let editing_key = self.ui.editing.is_some() && matches!(name, "a" | "c" | "x" | "v" | "z");
        if mods.primary && !editing_key && self.run_shortcut(name, mods) {
            return;
        }
        if self.ui.editing.is_none() && !mods.primary && self.run_on_key(name) {
            return;
        }
        // The view with the keyboard gets Cmd keys the shortcuts don't take,
        // as "cmd+c" (Cmd+Z stays undo).
        if self.ui.editing.is_none() && mods.primary && name != "z" && self.ui.keys_to.is_some() {
            let shift = if mods.shift { "shift+" } else { "" };
            if self.run_on_key(&format!("cmd+{shift}{name}")) {
                return;
            }
        }
        // (While nothing is being edited: in the view whose text changed last.)
        if self.ui.editing.is_none() && mods.primary && name == "z" {
            self.undo(mods.shift);
            return;
        }
        let unit = if mods.primary {
            Unit::Line
        } else if mods.alt {
            Unit::Word
        } else {
            Unit::Char
        };
        let extend = mods.shift;
        let change = match name {
            "backspace" => Edit::Backspace(unit),
            "delete" => Edit::Delete(unit),
            "left" => Edit::Left { unit, extend },
            "right" => Edit::Right { unit, extend },
            "home" => Edit::Left { unit: Unit::Line, extend },
            "end" => Edit::Right { unit: Unit::Line, extend },
            "up" => Edit::Up { extend },
            "down" => Edit::Down { extend },
            "enter" if self.editing_code() => {
                self.type_text("\n");
                return;
            }
            "tab" if self.editing_code() => {
                self.type_text("    ");
                return;
            }
            "tab" => {
                self.focus_next(mods.shift);
                return;
            }
            "a" if mods.primary => Edit::SelectAll,
            "c" if mods.primary => Edit::Copy,
            "x" if mods.primary => Edit::Cut,
            "v" if mods.primary => Edit::Paste,
            "z" if mods.primary => {
                self.undo(mods.shift);
                return;
            }
            "enter" | "escape" => {
                let submit = (name == "enter")
                    .then(|| self.ui.editing.as_ref().and_then(|e| self.node(e.key)).and_then(|n| n.mods.on_submit.clone()))
                    .flatten();
                self.ui.editing = None;
                self.dirty = true;
                if let Some(cb) = submit {
                    cb.call();
                }
                return;
            }
            _ => return,
        };
        self.edit(change);
    }

    /// A mouse press on a text field or code editor: place the caret or select.
    fn press_field(&mut self, key: u64, x: f64, y: f64, shift: bool, clicks: u32) {
        // Another field than an open palette's own closes the palette.
        if super::controls::OPEN_PICKER.get().is_some() && Some(key) != super::controls::HEX_FIELD.get() {
            self.close_picker();
        }
        self.ui.focused = None;
        self.ui.keys_to = None;
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(key)) else { return };
        let Some(text) = editable_text(node) else { return };
        let current = read_string(text);
        if self.ui.editing.as_ref().is_none_or(|e| e.key != key) {
            self.ui.editing = Some(Editing::new(key, &current));
        }
        let (x0, y0, sx, sy) = self.text_origin(node);
        self.edit(Edit::Click { x: x - x0 + sx, y: y - y0 + sy, extend: shift, clicks });
    }

    fn drag_field(&mut self, x: f64, y: f64) {
        let Some(key) = self.ui.editing.as_ref().map(|e| e.key) else { return };
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(key)) else { return };
        let (x0, y0, sx, sy) = self.text_origin(node);
        self.edit(Edit::Drag { x: x - x0 + sx, y: y - y0 + sy });
    }

    /// The scroll view or code editor at a point (the innermost one).
    fn scrollable_at(&self, x: f64, y: f64) -> Option<u64> {
        fn find(n: &Node, x: f64, y: f64) -> Option<u64> {
            if !n.layout.rect.contains(x, y) || n.mods.hidden {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| find(c, x, y));
            inner.or(matches!(n.kind, Kind::Scroll | Kind::CodeEditor { .. }).then_some(n.key))
        }
        self.tree.as_ref().and_then(|t| find(t, x, y))
    }

    /// What the wheel turns at a point: the innermost scroll view or code
    /// editor there, or view with `.onPointer` (true: it is told instead).
    fn wheel_target(&self, x: f64, y: f64) -> Option<(u64, bool)> {
        fn find(n: &Node, x: f64, y: f64) -> Option<(u64, bool)> {
            if !n.layout.rect.contains(x, y) || n.mods.hidden {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| find(c, x, y));
            inner.or_else(|| {
                if n.mods.on_pointer.is_some() && !n.layout.style.disabled {
                    Some((n.key, true))
                } else {
                    matches!(n.kind, Kind::Scroll | Kind::CodeEditor { .. }).then_some((n.key, false))
                }
            })
        }
        self.tree.as_ref().and_then(|t| find(t, x, y))
    }

    /// Scrolls the view under (x, y) by (dx, dy): as far as its content
    /// goes. A view with `.onPointer` is told of the scroll instead.
    fn scroll(&mut self, x: f64, y: f64, dx: f64, dy: f64, mods: Mods) {
        let Some((key, pointer)) = self.wheel_target(x, y) else { return };
        if pointer {
            self.pointer(key, SCROLL, (x, y), (dx, dy), 0, 0, mods);
            return;
        }
        let Some(n) = self.node(key) else { return };
        let content = n.layout.rect.inset_by(n.mods.padding);
        let max_y = (n.layout.content_h - content.h).max(0.0);
        let max_x = match &n.kind {
            Kind::CodeEditor { .. } => {
                let width = n.layout.text.as_ref().map_or(0.0, |t| t.width()) + 2.0 * render::CODE_PAD_X;
                (width - (content.w - n.layout.gutter)).max(0.0)
            }
            _ => 0.0,
        };
        let down = self.ui.scroll.entry(key).or_insert(0.0);
        *down = (*down + dy).clamp(0.0, max_y);
        if max_x > 0.0 || dx < 0.0 {
            let across = self.ui.scroll_x.entry(key).or_insert(0.0);
            *across = (*across + dx).clamp(0.0, max_x);
        }
    }

    /// The scroll bar at a point: its view, whether it's the one down the
    /// side, and how far along the thumb the point is (the middle of the
    /// thumb, for a point on the track beside it: the thumb jumps there).
    fn scroll_bar_at(&self, x: f64, y: f64) -> Option<(u64, bool, f64)> {
        let n = self.node(self.scrollable_at(x, y)?)?;
        let (down, across) = render::scroll_bars(n, &self.ui);
        // (A little wider than it looks, to be easy to catch.)
        let on = |r: Rect| r.inset(-3.0).contains(x, y);
        if let Some(bar) = down.filter(|b| on(b.track)) {
            let grab = if on(bar.thumb) { y - bar.thumb.y } else { bar.thumb.h / 2.0 };
            return Some((n.key, true, grab));
        }
        let bar = across.filter(|b| on(b.track))?;
        let grab = if on(bar.thumb) { x - bar.thumb.x } else { bar.thumb.w / 2.0 };
        Some((n.key, false, grab))
    }

    /// Scrolls so that the thumb of a scroll bar (held `grab` along it) is
    /// under the pointer at (x, y).
    fn drag_scroll_bar(&mut self, key: u64, vertical: bool, grab: f64, x: f64, y: f64) {
        let Some(n) = self.node(key) else { return };
        let (down, across) = render::scroll_bars(n, &self.ui);
        if let (true, Some(bar)) = (vertical, &down) {
            let offset = bar.offset_for(y - grab - bar.track.y);
            self.ui.scroll.insert(key, offset);
        } else if let (false, Some(bar)) = (vertical, &across) {
            let offset = bar.offset_for(x - grab - bar.track.x);
            self.ui.scroll_x.insert(key, offset);
        }
    }

    /// Center of the first view labelled `label`.
    fn find_label(&self, label: &str) -> Option<(f64, f64)> {
        fn visit(n: &Node, label: &str) -> Option<(f64, f64)> {
            if let Kind::TabView { .. } = n.kind {
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    if n.children.get(i).and_then(|c| c.mods.tab_title.as_deref()) == Some(label) {
                        let r = tab.rect;
                        return Some((r.x + r.w / 2.0 - 10.0, r.y + r.h / 2.0));
                    }
                }
                return n.children.get(n.selected_tab()).and_then(|c| visit(c, label));
            }
            // A link in Markdown, by what it says.
            if let (Kind::Markdown { .. }, Some(laid), false) = (&n.kind, &n.layout.markdown, n.mods.hidden) {
                let c = n.layout.content;
                if let Some(r) = laid.links.iter().find(|l| l.text == label).and_then(|l| l.rects.first()) {
                    return Some((c.x + r.x + r.w / 2.0, c.y + r.y + r.h / 2.0));
                }
            }
            // A disclosure group by its title (which is what's clicked).
            if let (Kind::Disclosure { title, .. }, Some(h), false) = (&n.kind, n.layout.segments.first(), n.mods.hidden) {
                if title == label {
                    return Some((h.x + h.w.min(40.0) / 2.0, h.y + h.h / 2.0));
                }
            }
            let matches = match &n.kind {
                Kind::Text(s) | Kind::Button { label: s, .. } | Kind::Toggle { label: s, .. } => s == label,
                Kind::TextField { placeholder, .. } => placeholder == label,
                Kind::Svg { icon: Some(name), .. } => *name == label,
                Kind::Swatch(c) => super::controls::hex(*c) == label,
                _ => false,
            };
            if matches && !n.mods.hidden {
                let r = n.layout.rect;
                return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
            }
            n.visible_children().iter().find_map(|c| visit(c, label))
        }
        // An open menu's items (the deepest menu first), then the drawn
        // menu bar's titles, then the views (the topmost popover's first).
        if let Some(items) = self.menu_items() {
            let menu = self.menu.as_ref()?;
            let mut path = Vec::new();
            let mut levels = Vec::new();
            for (level, sel) in menu.sel.iter().enumerate() {
                levels.push((level, menu::items_at(&items, &path).map(<[Entry]>::to_vec).unwrap_or_default()));
                match sel {
                    Some(i) => path.push(*i),
                    None => break,
                }
            }
            for (level, shown) in levels.into_iter().rev() {
                let panel = self.ui.menus.get(level)?;
                if let Some(i) = shown.iter().position(|e| e.label() == label && !matches!(e, Entry::Divider)) {
                    let r = panel.rows.get(i)?.rect;
                    return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
                }
            }
        }
        if let Some(bar) = &self.ui.menu_bar {
            if let Some(i) = self.bar.iter().position(|e| e.label() == label) {
                let r = bar.titles.get(i)?.1;
                return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
            }
        }
        let tree = self.tree.as_ref()?;
        for p in self.popovers().into_iter().rev() {
            if let Some(found) = visit(p, label) {
                return Some(found);
            }
        }
        visit(tree, label)
    }

    /// Center of the close button of the tab titled `title`.
    fn find_tab_close(&self, title: &str) -> Option<(f64, f64)> {
        fn visit(n: &Node, title: &str) -> Option<(f64, f64)> {
            if let Kind::TabView { .. } = n.kind {
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    if n.children.get(i).and_then(|c| c.mods.tab_title.as_deref()) == Some(title) {
                        let c = tab.close?;
                        return Some((c.x + c.w / 2.0, c.y + c.h / 2.0));
                    }
                }
            }
            n.visible_children().iter().find_map(|c| visit(c, title))
        }
        visit(self.tree.as_ref()?, title)
    }

    /// Center of the `n`th (from 1) view of a kind, in tree order.
    fn find_nth(&self, kind: &str, n: usize) -> Option<(f64, f64)> {
        fn visit<'n>(node: &'n Node, kind: &str, out: &mut Vec<&'n Node>) {
            let matches = matches!(
                (&node.kind, kind),
                (Kind::Toggle { .. }, "toggle") | (Kind::Button { .. }, "button") | (Kind::TextField { .. }, "field") | (Kind::Text(_), "text")
                    | (Kind::CodeEditor { .. }, "editor")
                    | (Kind::Slider { .. }, "slider")
                    | (Kind::Stepper { .. }, "stepper")
                    | (Kind::ColorPicker { .. }, "colorpicker")
            );
            if matches && !node.mods.hidden {
                out.push(node);
            }
            for c in node.visible_children() {
                visit(c, kind, out);
            }
        }
        let mut found = Vec::new();
        visit(self.tree.as_ref()?, kind, &mut found);
        let r = found.get(n.checked_sub(1)?)?.layout.rect;
        Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    fn dump(&self, positions: bool) -> String {
        fn visit(n: &Node, depth: usize, out: &mut String, positions: bool, ui: &Interaction) {
            // Where it's drawn now, which is on the way to its place while it's animated.
            let r = ui.anim.shown(n.key).map_or(n.layout.rect, |s| s.rect);
            let what = match &n.kind {
                Kind::Root => "Root".to_string(),
                Kind::Group => "View".to_string(),
                Kind::Text(s) => format!("Text {s:?}"),
                Kind::Button { label, .. } => format!("Button {label:?}"),
                Kind::TextField { text, placeholder } => {
                    let composing = ui.composing_in(n.key).map(|(_, c)| format!(" composing {:?}", c.text)).unwrap_or_default();
                    format!("TextField {placeholder:?} = {:?}{composing}", read_string(text))
                }
                Kind::Toggle { label, is_on } => format!("Toggle {label:?} = {}", unsafe { *is_on.locate() } & 1 != 0),
                Kind::Picker { .. } => "Picker".to_string(),
                Kind::VStack(..) => "VStack".to_string(),
                Kind::HStack(..) => "HStack".to_string(),
                Kind::ZStack(_) => "ZStack".to_string(),
                Kind::Scroll => "Scroll".to_string(),
                Kind::Spacer => "Spacer".to_string(),
                Kind::Image { path, .. } => format!("Image {path:?}"),
                Kind::TabView { .. } => {
                    let titles: Vec<&str> = n.children.iter().map(|c| c.mods.tab_title.as_deref().unwrap_or("")).collect();
                    format!("TabView {titles:?} selected {}", n.selected_tab())
                }
                Kind::Divider => "Divider".to_string(),
                Kind::Canvas { draw, redraw } => {
                    let c = n.layout.rect.inset_by(n.mods.padding);
                    let drawing = super::canvas::drawing(n.key, draw, *redraw, c.w, c.h);
                    format!("Canvas ({})", drawing.stats.describe())
                }
                Kind::Markdown { .. } => {
                    let laid = n.layout.markdown.as_ref();
                    let links: Vec<&str> = laid.iter().flat_map(|l| &l.links).map(|l| l.text.as_str()).collect();
                    format!("Markdown {:?} links {links:?}", laid.map_or("", |l| l.title.as_str()))
                }
                Kind::Svg { icon: Some(name), .. } => format!("Icon {name}"),
                Kind::Svg { .. } => "Svg".to_string(),
                Kind::CodeEditor { text, error_lines, .. } => {
                    format!("CodeEditor errors {error_lines:?} = {:?}", read_string(text))
                }
                Kind::Slider { value, .. } => format!("Slider = {}", super::controls::read_number(value, false)),
                Kind::Stepper { value, int, decimals, .. } => {
                    let v = super::controls::read_number(value, *int);
                    format!("Stepper = {}", super::controls::format_number(v, *decimals))
                }
                Kind::ColorPicker { color, .. } => {
                    let open = if super::controls::OPEN_PICKER.get() == Some(n.key) { " open" } else { "" };
                    format!("ColorPicker = {}{open}", super::controls::hex(super::controls::read_color(color)))
                }
                Kind::Swatch(c) => format!("Swatch {}", super::controls::hex(*c)),
                Kind::Disclosure { title, expanded } => {
                    let open = unsafe { *expanded.locate() } & 1 != 0;
                    format!("DisclosureGroup {title:?} {}", if open { "open" } else { "closed" })
                }
                Kind::Lazy { count, first, .. } => {
                    format!("LazyVStack {count} rows, built {}..{}", first, first + n.children.len())
                }
                Kind::MenuItem { label, .. } => format!("MenuItem {label:?}"),
                Kind::MenuDivider => "MenuDivider".to_string(),
                Kind::Menu { title } => format!("Menu {title:?}"),
                Kind::MenuList => "MenuList".to_string(),
                Kind::Popover { .. } => "Popover".to_string(),
                Kind::Window { title, .. } => format!("Window {title:?}"),
            };
            let what = if ui.focused == Some(n.key) { format!("{what} focused") } else { what };
            let what = match &ui.dragging {
                Some(d) if d.key == n.key => format!("{what} dragged"),
                Some(d) if d.target == Some(n.key) => format!("{what} drop target"),
                _ => what,
            };
            // A view's context menu is shown as one, with its items.
            let what = match (&n.mods.context_menu, positions) {
                (Some(m), false) => format!("{what} contextMenu [{}]", menu::describe_inline(&menu::entries(m))),
                _ => what,
            };
            let flags = [(n.mods.hidden, " hidden"), (n.mods.disabled, " disabled")]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, s)| *s)
                .collect::<String>();
            let place = if positions { format!(" [{:.0},{:.0} {:.0}x{:.0}]", r.x, r.y, r.w, r.h) } else { String::new() };
            out.push_str(&format!("{}{what}{place}{flags}\n", "  ".repeat(depth)));
            // A tab view shows only its selected page.
            if let Kind::TabView { .. } = n.kind {
                if let Some(c) = n.children.get(n.selected_tab()) {
                    visit(c, depth + 1, out, positions, ui);
                }
                return;
            }
            for c in &n.children {
                visit(c, depth + 1, out, positions, ui);
            }
        }
        let mut out = String::new();
        if let Some(t) = &self.tree {
            visit(t, 0, &mut out, positions, &self.ui);
        }
        out
    }

    /// The views on screen as JSON, for `tessel preview`: each one's depth
    /// in the tree, kind, place in the window, and which view expression of
    /// the source it is (see `Mods::source`; -1 when that isn't known). A
    /// view that doesn't show has no width or height.
    fn layout_json(&self) -> String {
        fn visit(n: &Node, depth: usize, shift: (f64, f64), clip: Rect, out: &mut Vec<String>) {
            if n.mods.hidden {
                return;
            }
            let shift = (shift.0 + n.mods.offset.0, shift.1 + n.mods.offset.1);
            let r = n.layout.rect;
            let (x, y) = (r.x + shift.0, r.y + shift.1);
            let kind = match &n.kind {
                Kind::Root => "Root",
                Kind::Group => "View",
                Kind::Text(_) => "Text",
                Kind::Button { .. } => "Button",
                Kind::TextField { .. } => "TextField",
                Kind::Toggle { .. } => "Toggle",
                Kind::Picker { .. } => "Picker",
                Kind::VStack(..) => "VStack",
                Kind::HStack(..) => "HStack",
                Kind::ZStack(_) => "ZStack",
                Kind::Scroll => "Scroll",
                Kind::Spacer => "Spacer",
                Kind::Image { .. } => "Image",
                Kind::TabView { .. } => "TabView",
                Kind::Divider => "Divider",
                Kind::Canvas { .. } => "Canvas",
                Kind::Markdown { .. } => "Markdown",
                Kind::Svg { icon: Some(_), .. } => "Icon",
                Kind::Svg { .. } => "Svg",
                Kind::CodeEditor { .. } => "CodeEditor",
                Kind::Slider { .. } => "Slider",
                Kind::Stepper { .. } => "Stepper",
                Kind::ColorPicker { .. } => "ColorPicker",
                Kind::Swatch(_) => "Swatch",
                Kind::Disclosure { .. } => "DisclosureGroup",
                Kind::Lazy { .. } => "LazyVStack",
                Kind::MenuItem { .. } => "MenuItem",
                Kind::MenuDivider => "MenuDivider",
                Kind::Menu { .. } => "Menu",
                Kind::MenuList => "MenuList",
                Kind::Popover { .. } => "Popover",
                Kind::Window { .. } => "Window",
            };
            // What can be seen of it: the part inside the scroll views around it.
            let (x0, y0) = (x.max(clip.x), y.max(clip.y));
            let (x1, y1) = ((x + r.w).min(clip.x + clip.w), (y + r.h).min(clip.y + clip.h));
            let view = n.mods.source.map_or(-1, |v| v as i64);
            out.push(format!(
                "{{\"depth\":{depth},\"kind\":\"{kind}\",\"x\":{x0:.1},\"y\":{y0:.1},\"width\":{:.1},\"height\":{:.1},\"source\":{view}}}",
                (x1 - x0).max(0.0),
                (y1 - y0).max(0.0),
            ));
            // (Layout already puts a scroll view's content where it's scrolled to.)
            let mut clip = clip;
            if matches!(n.kind, Kind::Scroll) || n.mods.clipped {
                clip = Rect { x: x0, y: y0, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0) };
            }
            // A tab view shows only its selected page.
            if let Kind::TabView { .. } = n.kind {
                if let Some(c) = n.children.get(n.selected_tab()) {
                    visit(c, depth + 1, shift, clip, out);
                }
                return;
            }
            for c in &n.children {
                visit(c, depth + 1, shift, clip, out);
            }
        }
        let mut out = Vec::new();
        if let Some(t) = &self.tree {
            let all = Rect { x: 0.0, y: 0.0, w: self.width, h: self.height };
            visit(t, 0, (0.0, 0.0), all, &mut out);
        }
        format!("[\n{}\n]\n", out.join(",\n"))
    }
}

// ===== entry point =====

/// Starts the app: opens a window (or runs `TESSEL_SCRIPT` without one) and
/// calls `build` (a `{f, env}` closure; ownership is taken) whenever the UI
/// needs to be described again. Never returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_run(
    title: *const Str,
    width: f64,
    height: f64,
    appearance: i32,
    f: *const c_void,
    env: *mut Header,
) -> ! {
    let title = unsafe { crate::str_text(title) }.to_string();
    RUNNING.with(|r| r.set(true));
    theme::set_dark(appearance == 2);
    let app = App {
        root: Callback::new(f, env),
        title,
        current: 0,
        others: Vec::new(),
        bar: Vec::new(),
        has_menu_bar: false,
        native_bar: cfg!(target_os = "macos"),
        open_files: Vec::new(),
        quit: false,
        window_commands: Vec::new(),
        builds: 0,
        menu: None,
        width,
        height,
        appearance,
        tree: None,
        text: TextEngine::new(),
        ui: Interaction::default(),
        dirty: true,
        clipboard: Clipboard::Headless,
        keys: Default::default(),
        followed: None,
        completing: Completing::default(),
        pointer_over: None,
        tip: None,
        sizes: HashMap::new(),
    };
    let code = match std::env::var("TESSEL_SCRIPT") {
        Ok(script) => run_script(app, &script),
        Err(_) => {
            let mut app = app;
            app.clipboard = Clipboard::system();
            run_window(app)
        }
    };
    TIMERS.with(|t| t.borrow_mut().clear());
    WAITING.with(|w| w.borrow_mut().clear());
    COMMANDS.with(|c| c.borrow_mut().clear());
    super::release_all_state();
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

/// The quoted words of a script command: `"File" "Open…"`.
fn quoted(text: &str) -> Vec<String> {
    text.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

fn run_script(app: App, script: &str) -> i32 {
    let mut app = app;
    let code = script_steps(&mut app, script);
    drop(app);
    code
}

fn script_steps(app: &mut App, script: &str) -> i32 {
    CLOCK.with(|c| *c.borrow_mut() = Clock::Virtual(0.0));
    let mut offscreen: Option<super::backend::Offscreen> = None;
    let mut commands: Vec<String> = Vec::new();
    for cmd in script.split([';', '\n']).map(str::trim).filter(|c| !c.is_empty()) {
        match cmd.strip_prefix("repeat ").and_then(|r| r.split_once(' ')) {
            Some((n, inner)) => {
                for _ in 0..n.parse::<usize>().unwrap_or(1) {
                    commands.push(inner.to_string());
                }
            }
            None => commands.push(cmd.to_string()),
        }
    }
    // The mouse, for views with `.onPointer`: the view it's pressed on (and
    // the button), where it is, and the modifier keys held.
    let mut held: Option<(u64, i32)> = None;
    let mut mouse = (0.0, 0.0);
    let mut mods = Mods::default();
    // A view with `.draggable` pressed on (and where), not yet moved.
    let mut drag_pending: Option<(u64, (f64, f64))> = None;
    let numbers = |text: &str| -> Vec<f64> { text.split_whitespace().filter_map(|x| x.parse().ok()).collect() };
    // A point: `40 60` (and anything after), or a view's label in quotes.
    let point_of = |app: &App, text: &str| -> Option<(f64, f64)> {
        if text.starts_with('"') {
            return app.find_label(text.split('"').nth(1)?);
        }
        let n: Vec<f64> = text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (n.len() >= 2).then(|| (n[0], n[1]))
    };
    for cmd in commands.iter().map(String::as_str) {
        app.update();
        if app.quit {
            break;
        }
        let (word, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
        let rest = rest.trim();
        match word {
            "size" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [w, h] = n[..] {
                    app.width = w;
                    app.height = h;
                }
            }
            "click" => {
                app.tip_hide();
                let words: Vec<&str> = rest.split_whitespace().collect();
                let point = if rest.starts_with('"') {
                    app.find_label(rest.trim_matches('"'))
                } else if let [kind @ ("toggle" | "button" | "field" | "text" | "editor" | "slider" | "stepper" | "colorpicker"), n] =
                    words[..]
                {
                    app.find_nth(kind, n.parse().unwrap_or(1))
                } else {
                    let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    (n.len() == 2).then(|| (n[0], n[1]))
                };
                match point {
                    // An open menu, the drawn menu bar or a popover takes it first.
                    Some((x, y)) if app.mouse_press(x, y, false) => mouse = (x, y),
                    Some((x, y)) => match app.pointer_at(x, y) {
                        Some(key) => {
                            app.ui.editing = None;
                            app.pointer(key, DOWN, (x, y), (0.0, 0.0), 0, 1, mods);
                            app.pointer(key, UP, (x, y), (0.0, 0.0), 0, 1, mods);
                            mouse = (x, y);
                        }
                        None => app.click(x, y),
                    },
                    None => {
                        eprintln!("script: nothing to click for `{rest}`");
                        return 2;
                    }
                }
            }
            "rightclick" | "doubleclick" => {
                let n = match rest.starts_with('"') {
                    true => app.find_label(rest.trim_matches('"')).map_or(Vec::new(), |(x, y)| vec![x, y]),
                    false => numbers(rest),
                };
                if let [x, y] = n[..] {
                    // A context menu opens (or an open menu takes the click).
                    if app.mouse_press(x, y, word == "rightclick") {
                        mouse = (x, y);
                        continue;
                    }
                }
                let key = match n[..] {
                    [x, y] => app.pointer_at(x, y),
                    _ => None,
                };
                let (Some(key), [x, y]) = (key, &n[..]) else {
                    eprintln!("script: no view with `.onPointer` for `{cmd}`");
                    return 2;
                };
                let at = (*x, *y);
                app.ui.editing = None;
                let (button, presses) = if word == "rightclick" { (1, 1) } else { (0, 2) };
                for clicks in 1..=presses {
                    app.pointer(key, DOWN, at, (0.0, 0.0), button, clicks, mods);
                    app.pointer(key, UP, at, (0.0, 0.0), button, clicks, mods);
                }
                mouse = at;
            }
            "press" => {
                let words: Vec<&str> = rest.split_whitespace().collect();
                let button = match words.get(2).copied() {
                    Some("right") => 1,
                    Some("middle") => 2,
                    _ => 0,
                };
                let point = point_of(app, rest);
                let key = point.and_then(|(x, y)| app.pointer_at(x, y));
                // Not on a view with `.onPointer`: maybe on one with `.draggable`.
                if let (None, Some((x, y))) = (key, point) {
                    if let Some(key) = app.drag_view_at(x, y, false) {
                        mouse = (x, y);
                        drag_pending = Some((key, mouse));
                        continue;
                    }
                }
                let (Some(key), Some(at)) = (key, point) else {
                    eprintln!("script: no view with `.onPointer` or `.draggable` for `{cmd}`");
                    return 2;
                };
                mouse = at;
                app.ui.editing = None;
                app.pointer(key, DOWN, mouse, (0.0, 0.0), button, 1, mods);
                held = Some((key, button));
            }
            "move" => {
                let Some((x, y)) = point_of(app, rest) else {
                    eprintln!("script: write it as `move 80 60` (or `move \"Label\"`)");
                    return 2;
                };
                let delta = (x - mouse.0, y - mouse.1);
                mouse = (x, y);
                if let Some((key, (sx, sy))) = drag_pending.take() {
                    app.begin_drag(key, sx, sy);
                }
                match held {
                    _ if app.ui.dragging.is_some() => app.drag_to(x, y),
                    Some((key, button)) => app.pointer(key, DRAG, mouse, delta, button, 0, mods),
                    None => {
                        app.hover(x, y);
                        app.pointer_move(x, y, delta, mods);
                    }
                }
            }
            "release" => {
                if let Some(at) = point_of(app, rest) {
                    mouse = at;
                }
                drag_pending = None;
                if app.ui.dragging.is_some() {
                    app.end_drag(Some(mouse));
                } else if let Some((key, button)) = held.take() {
                    app.pointer(key, UP, mouse, (0.0, 0.0), button, 1, mods);
                }
            }
            "dragto" => {
                // `dragto "Row 1" "Row 3"` or `dragto 40 60 40 200`: drags the
                // view with `.draggable` at the first place to the second.
                let n = numbers(rest);
                let labels: Vec<&str> = rest.split('"').skip(1).step_by(2).collect();
                let (from, to) = match (&n[..], &labels[..]) {
                    ([x1, y1, x2, y2], _) => (Some((*x1, *y1)), Some((*x2, *y2))),
                    (_, [a, b]) => (app.find_label(a), app.find_label(b)),
                    _ => (None, None),
                };
                let key = from.and_then(|(x, y)| app.drag_view_at(x, y, false));
                let (Some(key), Some(from), Some(to)) = (key, from, to) else {
                    eprintln!("script: nothing to drag for `{cmd}` (it needs a view with `.draggable`, and a place to drag it to)");
                    return 2;
                };
                app.begin_drag(key, from.0, from.1);
                app.update();
                app.drag_to((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
                app.update();
                app.end_drag(Some(to));
                mouse = to;
            }
            "pinch" => {
                let n = numbers(rest);
                let key = match n[..] {
                    [x, y, _] => app.pointer_at(x, y),
                    _ => None,
                };
                let (Some(key), [x, y, delta]) = (key, &n[..]) else {
                    eprintln!("script: no view with `.onPointer` for `{cmd}`");
                    return 2;
                };
                app.pointer(key, PINCH, (*x, *y), (0.0, *delta), 0, 0, mods);
            }
            "mods" => {
                mods = Mods::default();
                for m in rest.split('+') {
                    match m.trim() {
                        "shift" => mods.shift = true,
                        "alt" => mods.alt = true,
                        "cmd" | "ctrl" => mods.primary = true,
                        "control" => mods.control = true,
                        _ => {}
                    }
                }
            }
            "answer" => {
                // What the next file dialog returns: a path, or `cancel`.
                let answer = (rest != "cancel").then(|| rest.trim_matches('"').to_string());
                DIALOG_ANSWERS.with(|a| a.borrow_mut().push_back(answer));
            }
            "hover" => {
                app.update();
                let point = if rest.starts_with('"') {
                    app.find_label(rest.trim_matches('"'))
                } else {
                    let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    (n.len() == 2).then(|| (n[0], n[1]))
                };
                match point {
                    Some((x, y)) => {
                        app.hover(x, y);
                        app.pointer_move(x, y, (x - mouse.0, y - mouse.1), mods);
                        mouse = (x, y);
                    }
                    None => {
                        eprintln!("script: nothing to hover over for `{rest}`");
                        return 2;
                    }
                }
            }
            // The user closes the window (`.onCloseRequest` may keep it open).
            "close" if rest == "window" => app.request_close(),
            "quit" => app.request_quit(),
            "close" => match app.find_tab_close(rest.trim_matches('"')) {
                Some((x, y)) => app.click(x, y),
                None => {
                    eprintln!("script: no closable tab `{rest}`");
                    return 2;
                }
            },
            "type" => {
                let text = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
                app.type_text(text);
            }
            "key" => {
                let mut mods = Mods::default();
                let mut repeated = false;
                let mut name = rest;
                while let Some((m, tail)) = name.split_once('+').filter(|(_, tail)| !tail.is_empty()) {
                    match m {
                        "shift" => mods.shift = true,
                        "alt" => mods.alt = true,
                        "cmd" | "ctrl" => mods.primary = true,
                        "control" => mods.control = true,
                        "repeat" => repeated = true,
                        _ => {}
                    }
                    name = tail;
                }
                // What the key types: a character (in capitals with Shift), or a space.
                let text = match name {
                    "space" => " ".to_string(),
                    _ if name.chars().count() == 1 && mods.shift => name.to_uppercase(),
                    _ if name.chars().count() == 1 => name.to_string(),
                    _ => String::new(),
                };
                app.press(name, &text, mods, repeated);
            }
            "drag" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                // A slider's knob follows the pointer.
                if let ([x, _, dx, _], Some(Hit::Slider(key))) = (&n[..], n.get(..2).and_then(|p| app.hit(p[0], p[1]))) {
                    app.ui.focused = Some(key);
                    app.slide(key, *x);
                    app.update();
                    app.slide(key, x + dx);
                    continue;
                }
                // On a view with `.onPointer`: press, move and let go.
                if let [x, y, dx, dy] = n[..] {
                    if let Some(key) = app.pointer_at(x, y) {
                        app.ui.editing = None;
                        app.pointer(key, DOWN, (x, y), (0.0, 0.0), 0, 1, mods);
                        mouse = (x + dx, y + dy);
                        app.pointer(key, DRAG, mouse, (dx, dy), 0, 0, mods);
                        app.pointer(key, UP, mouse, (0.0, 0.0), 0, 1, mods);
                        continue;
                    }
                }
                let key = match n[..] {
                    [x, y, _, _] => app.drag_at(x, y),
                    _ => None,
                };
                // A scroll bar, or a view with `.onDrag`.
                let bar = match n[..] {
                    [x, y, _, _] => app.scroll_bar_at(x, y),
                    _ => None,
                };
                match (bar, key, &n[..]) {
                    (Some((key, vertical, grab)), _, [x, y, dx, dy]) => {
                        app.drag_scroll_bar(key, vertical, grab, *x, *y);
                        app.update();
                        app.drag_scroll_bar(key, vertical, grab, x + dx, y + dy);
                    }
                    (None, Some(key), [_, _, dx, dy]) => app.drag(key, *dx, *dy),
                    _ => {
                        eprintln!("script: nothing to drag for `{rest}`");
                        return 2;
                    }
                }
            }
            "scroll" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [dx, dy] = n[..] {
                    let (x, y) = (app.width / 2.0, app.height / 2.0);
                    app.scroll(x, y, dx, dy, mods);
                }
            }
            "wheel" => {
                let [x, y, dx, dy] = numbers(rest)[..] else {
                    eprintln!("script: write it as `wheel 40 60 0 120`");
                    return 2;
                };
                app.scroll(x, y, dx, dy, mods);
            }
            "wait" => {
                // Loads and commands in progress finish first (tests shouldn't depend on timing).
                let started = Instant::now();
                while jobs_busy() && started.elapsed() < Duration::from_secs(60) {
                    if deliver_background() {
                        app.dirty = true;
                        app.update();
                    } else {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                let secs: f64 = rest.parse().unwrap_or(0.0);
                // Open WebSockets: let their messages arrive for a moment
                // (at least half a second, or as long as the wait).
                if net::busy() {
                    let until = Instant::now() + Duration::from_secs_f64(secs.max(0.5));
                    while Instant::now() < until {
                        if deliver_background() {
                            app.dirty = true;
                            app.update();
                        } else {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
                let end = now() + secs;
                // Step through timers in order, so ones they add also fire.
                while let Some(t) = next_timer().filter(|t| *t <= end) {
                    CLOCK.with(|c| *c.borrow_mut() = Clock::Virtual(t));
                    if fire_timers() {
                        app.dirty = true;
                        app.update();
                    }
                }
                CLOCK.with(|c| *c.borrow_mut() = Clock::Virtual(end));
                app.tip_due();
            }
            "menu" => {
                let labels = quoted(rest);
                if let Err(e) = app.choose_bar_item(&labels) {
                    eprintln!("script: {e}");
                    return 2;
                }
            }
            "menus" => {
                let mut out = String::new();
                menu::describe(&app.bar, 0, &mut out);
                print!("{out}");
            }
            "menubar" => {
                app.native_bar = rest == "native";
                app.menu = None;
                app.dirty = true;
            }
            "open" => {
                app.open_files.push(rest.trim_matches('"').to_string());
                if !app.deliver_open_files() {
                    eprintln!("script: no view with `.onOpenFile` for `{cmd}`");
                    return 2;
                }
            }
            "window" => {
                let title = rest.trim_matches('"');
                let key = if rest == "main" { Some(0) } else { app.windows().into_iter().find(|w| w.1 == title).map(|w| w.0) };
                match key {
                    Some(key) if app.enter(key) => {}
                    _ => {
                        eprintln!("script: no open window `{title}`");
                        return 2;
                    }
                }
            }
            "drop" => {
                let path = rest.trim_matches('"');
                if !app.drop_file(path) {
                    eprintln!("script: no view with `.onDrop` for `{cmd}`");
                    return 2;
                }
            }
            "compose" => {
                let text = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
                app.compose(text.to_string(), None);
            }
            "animations" => app.ui.anim.enabled = rest == "on",
            "reload" => {
                let entry = RELOAD.get().map_or(0, |reload| reload());
                if entry != 0 {
                    let f: extern "C" fn() = unsafe { std::mem::transmute(entry) };
                    f();
                }
            }
            "appearance" => {
                if theme::set_dark(rest == "dark") {
                    app.dirty = true;
                }
            }
            "snapshot" => {
                let scale = 2.0;
                let list = app.scene(scale);
                let (w, h) = ((app.width * scale) as u32, (app.height * scale) as u32);
                let background = theme().colors[theme::BACKGROUND];
                let pixels = offscreen.get_or_insert_with(Default::default).rgba(&list, w, h, Some(background));
                if !super::backend::save_rgba(rest, w, h, pixels) {
                    eprintln!("script: can't write `{rest}`");
                    return 2;
                }
            }
            "layout" => {
                app.update();
                if let Err(e) = std::fs::write(rest, app.layout_json()) {
                    eprintln!("script: can't write `{rest}`: {e}");
                    return 2;
                }
            }
            "dump" | "tree" => {
                app.update();
                if app.has_menu_bar && app.current == 0 {
                    let titles: Vec<String> = app.bar.iter().map(|m| format!("{:?}", m.label())).collect();
                    let how = if app.native_bar { "native" } else { "drawn" };
                    println!("MenuBar ({how}) {}", titles.join(" "));
                }
                print!("{}", app.dump(word == "dump"));
                print!("{}", app.describe_menu());
                if app.current == 0 {
                    for (_, title, w, h) in app.windows() {
                        println!("Window {title:?} {w:.0}x{h:.0}");
                    }
                }
                if let Some(tip) = &app.ui.tooltip {
                    println!("Tooltip {:?}", tip.text);
                }
                // The completion list, with the selected suggestion marked.
                if let Some(m) = app.matches() {
                    let items: Vec<String> = m
                        .items
                        .iter()
                        .enumerate()
                        .map(|(i, s)| if i == app.completing.selected { format!("[{}]", s.label) } else { s.label.clone() })
                        .collect();
                    println!("Completions: {}", items.join(" "));
                }
            }
            _ => {
                eprintln!("script: unknown command `{word}`");
                return 2;
            }
        }
    }
    0
}

// ===== window =====

/// An open window, and what the mouse and keyboard are doing in it.
struct WindowApp {
    app: App,
    mods: Mods,
    /// The other open windows (`Window`s, or the main one), parked while
    /// another one's events are handled (see `focus`).
    parked: Vec<Win>,
    /// The menu bar the system shows (macOS), as `menu::describe` lists it,
    /// and the app's build it was last compared with.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    native_menu: (Option<String>, u64),
    // ----- the window whose events are handled (see `Win`) -----
    /// Its key in the app (0: the main window).
    key: u64,
    screen: Option<super::backend::Screen>,
    window: Option<Arc<Window>>,
    /// The title it shows.
    title: String,
    cursor: (f64, f64),
    /// Dragging to select text in a field.
    selecting: bool,
    /// The view with `.onDrag` being dragged.
    dragging: Option<u64>,
    /// The scroll bar being dragged: its view, whether it's the one down
    /// the side, and where on its thumb it's held.
    scrolling: Option<(u64, bool, f64)>,
    /// The view with `.onPointer` the mouse is pressed on, and the button.
    pointing: Option<(u64, i32)>,
    /// The slider whose knob follows the mouse.
    sliding: Option<u64>,
    /// A view with `.draggable` the mouse is pressed on, and where: it's
    /// dragged once the mouse moves far enough.
    drag_start: Option<(u64, (f64, f64))>,
    /// The pointer that's showing (a resize one over a drag handle).
    pointer: winit::window::CursorIcon,
    /// For double and triple clicks: when and where the last click was, and the count.
    last_click: Option<(Instant, (f64, f64), u32)>,
    /// Whether input methods are on (they are while text is edited), and
    /// where the caret was last reported to them.
    ime: bool,
    ime_caret: Option<Rect>,
    /// A press a menu or popover took: its release is ignored too.
    swallow: bool,
}

/// The fields of `WindowApp` from `key` on, for a parked window.
struct Win {
    key: u64,
    screen: Option<super::backend::Screen>,
    window: Option<Arc<Window>>,
    title: String,
    cursor: (f64, f64),
    selecting: bool,
    dragging: Option<u64>,
    scrolling: Option<(u64, bool, f64)>,
    pointing: Option<(u64, i32)>,
    sliding: Option<u64>,
    drag_start: Option<(u64, (f64, f64))>,
    pointer: winit::window::CursorIcon,
    last_click: Option<(Instant, (f64, f64), u32)>,
    ime: bool,
    ime_caret: Option<Rect>,
    swallow: bool,
}

impl Win {
    fn new(key: u64, title: String) -> Win {
        Win {
            key,
            screen: None,
            window: None,
            title,
            cursor: (0.0, 0.0),
            selecting: false,
            dragging: None,
            scrolling: None,
            pointing: None,
            sliding: None,
            drag_start: None,
            pointer: winit::window::CursorIcon::Default,
            last_click: None,
            ime: false,
            ime_caret: None,
            swallow: false,
        }
    }
}

thread_local! {
    /// Whether the program runs with `tessel run --hot` (inside `tessel`,
    /// whose arguments aren't the program's).
    static HOT: Cell<bool> = const { Cell::new(false) };
}

fn run_window(app: App) -> i32 {
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("error: can't open a window: {e}");
            return 1;
        }
    };
    *WAKE.lock().unwrap() = Some(event_loop.create_proxy());
    #[cfg(target_os = "macos")]
    super::native::install();
    let mut w = WindowApp {
        app,
        mods: Mods::default(),
        parked: Vec::new(),
        native_menu: (None, u64::MAX),
        key: 0,
        screen: None,
        window: None,
        title: String::new(),
        cursor: (0.0, 0.0),
        selecting: false,
        dragging: None,
        scrolling: None,
        pointing: None,
        sliding: None,
        drag_start: None,
        pointer: winit::window::CursorIcon::Default,
        last_click: None,
        ime: false,
        ime_caret: None,
        swallow: false,
    };
    w.app.ui.anim.enabled = true;
    // Files the app is started with go to `.onOpenFile` (as files opened
    // with it later do).
    if !HOT.with(Cell::get) {
        let files = std::env::args().skip(1).filter(|a| !a.starts_with('-') && std::path::Path::new(a).is_file());
        w.app.open_files.extend(files);
    }
    let code = match event_loop.run_app(&mut w) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    w.app.tree = None;
    for s in &mut w.app.others {
        s.tree = None;
    }
    code
}

impl WindowApp {
    /// Swaps the handled window's fields with `w`'s.
    fn swap(&mut self, w: &mut Win) {
        std::mem::swap(&mut self.key, &mut w.key);
        std::mem::swap(&mut self.screen, &mut w.screen);
        std::mem::swap(&mut self.window, &mut w.window);
        std::mem::swap(&mut self.title, &mut w.title);
        std::mem::swap(&mut self.cursor, &mut w.cursor);
        std::mem::swap(&mut self.selecting, &mut w.selecting);
        std::mem::swap(&mut self.dragging, &mut w.dragging);
        std::mem::swap(&mut self.scrolling, &mut w.scrolling);
        std::mem::swap(&mut self.pointing, &mut w.pointing);
        std::mem::swap(&mut self.sliding, &mut w.sliding);
        std::mem::swap(&mut self.drag_start, &mut w.drag_start);
        std::mem::swap(&mut self.pointer, &mut w.pointer);
        std::mem::swap(&mut self.last_click, &mut w.last_click);
        std::mem::swap(&mut self.ime, &mut w.ime);
        std::mem::swap(&mut self.ime_caret, &mut w.ime_caret);
        std::mem::swap(&mut self.swallow, &mut w.swallow);
    }

    /// Makes the window with app key `key` the handled one (and its surface
    /// the app's current one). Returns whether it's open.
    fn focus(&mut self, key: u64) -> bool {
        if self.key != key {
            let Some(i) = self.parked.iter().position(|w| w.key == key) else { return false };
            let mut w = self.parked.remove(i);
            self.swap(&mut w);
            self.parked.push(w);
        }
        self.app.enter(key)
    }

    /// The app key of the window with this id.
    fn key_of(&self, id: WindowId) -> Option<u64> {
        if self.window.as_ref().is_some_and(|w| w.id() == id) {
            return Some(self.key);
        }
        self.parked.iter().find(|w| w.window.as_ref().is_some_and(|w| w.id() == id)).map(|w| w.key)
    }

    fn scale(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    /// Opens a window for `key` (its size and title from the app), with a
    /// screen to draw on. The main window's title bar follows `appearance`.
    fn open(&mut self, event_loop: &ActiveEventLoop, key: u64, title: &str, size: (f64, f64)) -> Option<(Arc<Window>, super::backend::Screen)> {
        let attrs = Window::default_attributes()
            .with_title(title.to_string())
            .with_inner_size(LogicalSize::new(size.0, size.1))
            // The title bar's appearance; `None` follows the system.
            .with_theme(match self.app.appearance {
                1 => Some(winit::window::Theme::Light),
                2 => Some(winit::window::Theme::Dark),
                _ => None,
            });
        // The app's icon is icon group 1 in its .exe (`tessel build` puts it
        // there): show it in the title bar and the taskbar.
        #[cfg(windows)]
        let attrs = {
            use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
            match winit::window::Icon::from_resource(1, None) {
                Ok(icon) => attrs.with_window_icon(Some(icon.clone())).with_taskbar_icon(Some(icon)),
                Err(_) => attrs,
            }
        };
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("error: can't open a window: {e}");
                return None;
            }
        };
        if key == 0 && self.app.appearance == 0 && theme::set_dark(window.theme() == Some(winit::window::Theme::Dark)) {
            self.app.dirty = true;
        }
        match super::backend::Screen::new(window.clone()) {
            Ok(s) => Some((window, s)),
            Err(e) => {
                eprintln!("error: can't draw to the window: {e}");
                None
            }
        }
    }

    /// Opens a window for each `Window` the app has open, and closes those
    /// it closed. Titles follow the app's.
    fn sync_windows(&mut self, event_loop: &ActiveEventLoop) {
        let wanted = self.app.windows();
        // Closed: the main window takes over.
        let open = |key: u64| key == 0 || wanted.iter().any(|w| w.0 == key);
        if !open(self.key) {
            self.focus(0);
        }
        self.parked.retain(|w| open(w.key));
        for (key, title, w, h) in wanted {
            let known = self.key == key || self.parked.iter().any(|p| p.key == key);
            if !known {
                let mut win = Win::new(key, title.clone());
                if let Some((window, screen)) = self.open(event_loop, key, &title, (w, h)) {
                    win.window = Some(window.clone());
                    win.screen = Some(screen);
                    window.request_redraw();
                    self.parked.push(win);
                }
                continue;
            }
            let win = if self.key == key { None } else { self.parked.iter_mut().find(|p| p.key == key) };
            let (current_title, window) = match win {
                Some(p) => (&mut p.title, p.window.as_ref()),
                None => (&mut self.title, self.window.as_ref()),
            };
            if *current_title != title {
                if let Some(window) = window {
                    window.set_title(&title);
                }
                *current_title = title;
            }
        }
    }

    /// Redraws every window (state changed: any of them may show it).
    fn request_all(&self) {
        self.request_redraw();
        for w in &self.parked {
            if let Some(w) = &w.window {
                w.request_redraw();
            }
        }
    }

    /// Carries out what the app asked of its windows, and of the event loop.
    fn after_app(&mut self, event_loop: &ActiveEventLoop) {
        for command in std::mem::take(&mut self.app.window_commands) {
            let Some(w) = &self.window else { continue };
            match command {
                Std::Minimize => w.set_minimized(true),
                Std::Zoom => w.set_maximized(!w.is_maximized()),
                _ => {}
            }
        }
        self.app.take_requests();
        if self.app.quit {
            event_loop.exit();
            return;
        }
        // State changed: every window may show it, and windows may have
        // opened or closed.
        if self.app.dirty {
            self.app.update();
            self.request_all();
            if self.app.quit {
                event_loop.exit();
                return;
            }
        }
        self.sync_windows(event_loop);
        self.sync_native_menu();
    }

    /// Gives the system the app's menu bar (macOS) when it changed.
    fn sync_native_menu(&mut self) {
        #[cfg(target_os = "macos")]
        {
            if self.native_menu.1 == self.app.builds {
                return;
            }
            self.native_menu.1 = self.app.builds;
            let mut listed = String::new();
            menu::describe(&self.app.bar, 0, &mut listed);
            if self.native_menu.0.as_deref() != Some(listed.as_str()) && !self.app.bar.is_empty() {
                super::native::set_menu_bar(&self.app.bar);
                self.native_menu.0 = Some(listed);
            }
        }
    }

    /// What the system sent the app: menu items chosen, files, Quit.
    fn take_shell_events(&mut self) -> bool {
        let events: Vec<Shell> = std::mem::take(&mut *SHELL.lock().unwrap());
        let any = !events.is_empty();
        // They're for the window that had the last event (the menu bar's
        // Close Window closes it).
        if any && !self.focus(self.key) {
            self.focus(0);
        }
        for event in events {
            match event {
                Shell::Menu(path) => {
                    let item = menu::at(&self.app.bar, &path).cloned();
                    if let Some(e @ Entry::Item { enabled: true, .. }) = item {
                        self.app.run_entry(&e);
                    }
                }
                Shell::OpenFile(path) => {
                    self.app.open_files.push(path);
                    self.app.dirty = true;
                }
                Shell::Quit => self.app.request_quit(),
            }
        }
        any
    }

    fn redraw(&mut self) {
        let (Some(screen), Some(window)) = (self.screen.as_mut(), self.window.as_ref()) else { return };
        if self.key == 0 {
            if let Some(title) = NEW_TITLE.with(|t| t.borrow_mut().take()) {
                window.set_title(&title);
                self.app.title = title;
            }
        }
        let scale = window.scale_factor();
        let size = window.inner_size();
        self.app.width = size.width as f64 / scale;
        self.app.height = size.height as f64 / scale;
        let list = self.app.scene(scale);
        if !screen.present(&list, theme().colors[theme::BACKGROUND]) {
            window.request_redraw();
            return;
        }
        // The input method shows its candidates next to the caret.
        let caret = self.app.caret_rect();
        if let (Some(c), true) = (caret, caret != self.ime_caret) {
            window.set_ime_cursor_area(LogicalPosition::new(c.x, c.y), LogicalSize::new(c.w.max(1.0), c.h));
        }
        self.ime_caret = caret;
        if self.app.ui.anim.running {
            window.request_redraw();
        }
    }

    /// Input methods are on while a text field or code editor is edited.
    fn sync_ime(&mut self) {
        let editing = self.app.ui.editing.is_some();
        if editing != self.ime {
            self.ime = editing;
            if let Some(w) = &self.window {
                w.set_ime_allowed(editing);
            }
            if !editing {
                self.app.ui.composing = None;
            }
        }
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.key != 0 {
            return;
        }
        let (title, size) = (self.app.title.clone(), (self.app.width, self.app.height));
        let Some((window, screen)) = self.open(event_loop, 0, &title, size) else {
            event_loop.exit();
            return;
        };
        self.screen = Some(screen);
        self.window = Some(window);
        self.title = title;
        self.request_redraw();
        // (Replaces winit's menu bar, which is made at launch.)
        self.app.update();
        self.after_app(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if std::env::var_os("TESSEL_DEBUG_EVENTS").is_some() && !matches!(event, WindowEvent::RedrawRequested) {
            eprintln!("event: {event:?}");
        }
        // The event's window, with its surface in the app.
        let Some(key) = self.key_of(id) else { return };
        if !self.focus(key) {
            return;
        }
        // A click, a key or the wheel puts the tooltip away.
        if matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::KeyboardInput { .. } | WindowEvent::MouseWheel { .. }) {
            if self.app.ui.tooltip.is_some() {
                self.request_redraw();
            }
            self.app.tip_hide();
        }
        // Open menus, the drawn menu bar and popovers take presses first; a
        // right press (or a Ctrl-click, on macOS) opens a context menu.
        if let WindowEvent::MouseInput { state, button, .. } = &event {
            let (x, y) = self.cursor;
            match state {
                ElementState::Pressed if matches!(button, MouseButton::Left | MouseButton::Right) => {
                    let right = *button == MouseButton::Right || (cfg!(target_os = "macos") && self.mods.control);
                    if self.app.mouse_press(x, y, right) {
                        self.swallow = true;
                        self.after_app(event_loop);
                        self.request_all();
                        return;
                    }
                }
                ElementState::Released if self.swallow => {
                    self.swallow = false;
                    return;
                }
                _ => {}
            }
        }
        match event {
            WindowEvent::CloseRequested => self.app.request_close(),
            WindowEvent::Focused(true) => {}
            WindowEvent::Resized(size) => {
                if let Some(s) = self.screen.as_mut() {
                    s.resize(size.width, size.height);
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::ThemeChanged(t) => {
                if self.app.appearance == 0 && theme::set_dark(t == winit::window::Theme::Dark) {
                    self.app.dirty = true;
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => {
                let m = m.state();
                let primary = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
                self.mods = Mods { shift: m.shift_key(), alt: m.alt_key(), primary, control: m.control_key() };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale();
                let before = self.cursor;
                self.cursor = (position.x / scale, position.y / scale);
                // An open menu: the item under the pointer is selected.
                if self.app.menu.is_some() {
                    self.app.ui.pointer = Some(self.cursor);
                    self.app.menu_hover(self.cursor.0, self.cursor.1);
                    self.request_redraw();
                    return;
                }
                if let Some((key, vertical, grab)) = self.scrolling {
                    self.app.drag_scroll_bar(key, vertical, grab, self.cursor.0, self.cursor.1);
                    self.request_redraw();
                    return;
                }
                let delta = (self.cursor.0 - before.0, self.cursor.1 - before.1);
                if let Some(key) = self.sliding {
                    self.app.slide(key, self.cursor.0);
                    self.request_redraw();
                    return;
                }
                // A view with `.draggable` starts moving once the mouse has
                // gone a few points (less is a click).
                if let (Some((key, (sx, sy))), None) = (self.drag_start, &self.app.ui.dragging) {
                    if (self.cursor.0 - sx).hypot(self.cursor.1 - sy) > 4.0 {
                        self.drag_start = None;
                        self.app.begin_drag(key, sx, sy);
                    }
                }
                if self.app.ui.dragging.is_some() {
                    self.app.drag_to(self.cursor.0, self.cursor.1);
                    self.request_redraw();
                    return;
                }
                if let Some((key, button)) = self.pointing {
                    self.app.pointer(key, DRAG, self.cursor, delta, button, 0, self.mods);
                    self.request_redraw();
                } else if let Some(key) = self.dragging {
                    self.app.drag(key, delta.0, delta.1);
                    self.request_redraw();
                } else {
                    if !self.selecting && self.app.pointer_move(self.cursor.0, self.cursor.1, delta, self.mods) {
                        self.request_redraw();
                    }
                    let (x, y) = self.cursor;
                    let pointer = self.app.cursor_at(x, y).or_else(|| self.app.drag_cursor(x, y)).unwrap_or_default();
                    if pointer != self.pointer {
                        self.pointer = pointer;
                        if let Some(w) = &self.window {
                            w.set_cursor(pointer);
                        }
                    }
                }
                if self.selecting {
                    self.app.drag_field(self.cursor.0, self.cursor.1);
                    self.request_redraw();
                }
                let hovered = self.app.hit(self.cursor.0, self.cursor.1).map(|h| h.key());
                let hover_bg = self.app.hover_background_at(self.cursor.0, self.cursor.1);
                let over_bar = self.app.ui.menu_bar.as_ref().is_some_and(|b| self.cursor.1 < menu::BAR_H || b.open.is_some());
                self.app.ui.pointer = Some(self.cursor);
                let had_tip = self.app.ui.tooltip.is_some();
                self.app.tip_move(self.cursor.0, self.cursor.1);
                if had_tip && self.app.ui.tooltip.is_none() {
                    self.request_redraw();
                }
                if hovered != self.app.ui.hovered || hovered.is_some() || hover_bg != self.app.ui.hover_background || over_bar {
                    self.app.ui.hovered = hovered;
                    self.app.ui.hover_background = hover_bg;
                    self.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if self.app.drop_file(&path.to_string_lossy()) {
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.pointing.is_none() {
                    self.app.pointer_leave(self.mods);
                }
                self.app.tip = None;
                self.app.ui.tooltip = None;
                self.app.ui.hovered = None;
                self.app.ui.hover_background = None;
                self.app.ui.pointer = None;
                self.request_redraw();
            }
            // A view with `.onPointer` gets every button, pressed and let go.
            WindowEvent::MouseInput { state, button, .. }
                if matches!(button, MouseButton::Left | MouseButton::Right | MouseButton::Middle)
                    && match state {
                        ElementState::Pressed => {
                            self.pointing.is_none()
                                && self.app.ui.popup.as_ref().is_none_or(|p| !p.rect.contains(self.cursor.0, self.cursor.1))
                                && self.app.pointer_at(self.cursor.0, self.cursor.1).is_some()
                        }
                        ElementState::Released => self.pointing.is_some(),
                    } =>
            {
                let (x, y) = self.cursor;
                let pressed = match button {
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => 0,
                };
                match (state, self.pointing) {
                    (ElementState::Pressed, _) => {
                        let Some(key) = self.app.pointer_at(x, y) else { return };
                        let now = Instant::now();
                        let clicks = match self.last_click {
                            Some((t, (px, py), n))
                                if now.duration_since(t) < Duration::from_millis(400) && (px - x).abs() < 4.0 && (py - y).abs() < 4.0 =>
                            {
                                n + 1
                            }
                            _ => 1,
                        };
                        self.last_click = Some((now, (x, y), clicks));
                        self.app.ui.editing = None;
                        self.app.ui.focused = None;
                        self.app.close_palette_unless(&None);
                        self.app.pointer(key, DOWN, (x, y), (0.0, 0.0), pressed, clicks as i64, self.mods);
                        self.pointing = Some((key, pressed));
                    }
                    (ElementState::Released, Some((key, held))) if held == pressed => {
                        let clicks = self.last_click.map_or(1, |(_, _, n)| n);
                        self.app.pointer(key, UP, (x, y), (0.0, 0.0), pressed, clicks as i64, self.mods);
                        self.pointing = None;
                    }
                    _ => {}
                }
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let (x, y) = self.cursor;
                match state {
                    ElementState::Pressed if self.app.popup_click(x, y) => {}
                    // On a scroll bar: the thumb comes to the pointer, and
                    // follows it until the button is released.
                    ElementState::Pressed if self.app.scroll_bar_at(x, y).is_some() => {
                        self.scrolling = self.app.scroll_bar_at(x, y);
                        if let Some((key, vertical, grab)) = self.scrolling {
                            self.app.drag_scroll_bar(key, vertical, grab, x, y);
                        }
                    }
                    ElementState::Released if self.scrolling.is_some() => self.scrolling = None,
                    ElementState::Pressed => {
                        let hit = self.app.hit(x, y);
                        if hit.is_none() {
                            self.app.close_palette_unless(&hit);
                            self.app.ui.focused = None;
                        }
                        if let Some(Hit::Drag(key)) = hit {
                            self.dragging = Some(key);
                        }
                        if let Some(Hit::Slider(key)) = hit {
                            self.sliding = Some(key);
                            self.app.ui.focused = Some(key);
                            self.app.slide(key, x);
                        }
                        if !matches!(hit, Some(Hit::Field(_) | Hit::Slider(_) | Hit::Drag(_))) {
                            self.drag_start = self.app.drag_view_at(x, y, false).map(|key| (key, (x, y)));
                        }
                        if let Some(Hit::Field(key)) = hit {
                            let now = Instant::now();
                            let clicks = match self.last_click {
                                Some((t, (px, py), n))
                                    if now.duration_since(t) < Duration::from_millis(400)
                                        && (px - x).abs() < 4.0
                                        && (py - y).abs() < 4.0 =>
                                {
                                    n + 1
                                }
                                _ => 1,
                            };
                            self.last_click = Some((now, (x, y), clicks));
                            self.app.press_field(key, x, y, self.mods.shift, clicks);
                            self.selecting = true;
                        } else {
                            self.app.ui.pressed = hit.map(|h| h.key());
                        }
                    }
                    ElementState::Released if self.app.ui.dragging.is_some() => {
                        self.drag_start = None;
                        self.app.end_drag(Some((x, y)));
                    }
                    ElementState::Released => {
                        self.selecting = false;
                        self.dragging = None;
                        self.sliding = None;
                        self.drag_start = None;
                        let pressed = self.app.ui.pressed.take();
                        let here = self.app.hit(x, y).map(|h| h.key());
                        if pressed.is_some() && pressed == here {
                            self.app.click(x, y);
                        } else if here.is_none() && self.app.ui.editing.is_some() {
                            self.app.ui.editing = None;
                        }
                    }
                }
                self.request_redraw();
            }
            WindowEvent::Ime(Ime::Preedit(text, caret)) => {
                self.app.compose(text, caret.map(|(_, end)| end));
                self.request_redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.app.type_text(&text);
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x as f64 * 40.0, -y as f64 * 40.0),
                    MouseScrollDelta::PixelDelta(p) => (-p.x / self.scale(), -p.y / self.scale()),
                };
                self.app.scroll(self.cursor.0, self.cursor.1, dx, dy, self.mods);
                self.request_redraw();
            }
            // A trackpad pinch (macOS): for a view with `.onPointer`.
            WindowEvent::PinchGesture { delta, .. } => {
                if let Some(key) = self.app.pointer_at(self.cursor.0, self.cursor.1) {
                    self.app.pointer(key, PINCH, self.cursor, (0.0, delta), 0, 0, self.mods);
                    self.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let Some(name) = key_name(&event) else { return };
                let text = event.text.as_deref().unwrap_or("").to_string();
                self.app.press(&name, &text, self.mods, event.repeat);
                self.request_redraw();
            }
            _ => {}
        }
        self.sync_ime();
        self.after_app(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        let shell = self.take_shell_events();
        if deliver_background() || shell {
            self.app.dirty = true;
        }
        self.after_app(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.take_shell_events() | deliver_background() {
            self.app.dirty = true;
        }
        if fire_timers() {
            self.app.dirty = true;
        }
        if self.app.tip_due() {
            self.request_redraw();
        }
        self.after_app(event_loop);
        // Wake for the next timer, or for a tooltip that's waiting to show.
        let next = [next_timer(), self.app.tip_time()].into_iter().flatten().fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))));
        match next {
            Some(t) => {
                let wait = Duration::from_secs_f64((t - now()).max(0.0));
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// A key's name, as `.onKeyEvent` and the script's `key` command name it:
/// the key without the modifiers (`"a"` for Shift+A, `"1"` for Shift+1),
/// or a special key's name. `None` for keys that are only modifiers.
fn key_name(event: &KeyEvent) -> Option<String> {
    let named = |k: &NamedKey| -> Option<String> {
        Some(
            match k {
                NamedKey::Backspace => "backspace",
                NamedKey::Delete => "delete",
                NamedKey::ArrowLeft => "left",
                NamedKey::ArrowRight => "right",
                NamedKey::ArrowUp => "up",
                NamedKey::ArrowDown => "down",
                NamedKey::Home => "home",
                NamedKey::End => "end",
                NamedKey::PageUp => "pageup",
                NamedKey::PageDown => "pagedown",
                NamedKey::Enter => "enter",
                NamedKey::Escape => "escape",
                NamedKey::Tab => "tab",
                NamedKey::Space => "space",
                NamedKey::Insert => "insert",
                NamedKey::F1 => "f1",
                NamedKey::F2 => "f2",
                NamedKey::F3 => "f3",
                NamedKey::F4 => "f4",
                NamedKey::F5 => "f5",
                NamedKey::F6 => "f6",
                NamedKey::F7 => "f7",
                NamedKey::F8 => "f8",
                NamedKey::F9 => "f9",
                NamedKey::F10 => "f10",
                NamedKey::F11 => "f11",
                NamedKey::F12 => "f12",
                _ => return None,
            }
            .to_string(),
        )
    };
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    let plain = {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        event.key_without_modifiers()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let plain = event.logical_key.clone();
    match (&event.logical_key, &plain) {
        (Key::Named(k), _) => named(k),
        (_, Key::Character(c)) | (Key::Character(c), _) => Some(c.to_lowercase()),
        _ => None,
    }
}

/// The locator of an editable view's text.
/// How many bytes two texts start with in common (at a character boundary).
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices().zip(b.chars()).find(|((_, x), y)| x != y).map_or(a.len().min(b.len()), |((i, _), _)| i)
}

fn editable_text(n: &Node) -> Option<&Callback> {
    match &n.kind {
        Kind::TextField { text, .. } | Kind::CodeEditor { text, .. } => Some(text),
        _ => None,
    }
}
