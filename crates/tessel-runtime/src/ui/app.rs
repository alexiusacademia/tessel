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

/// The system's file and message dialogs.
#[cfg(not(target_os = "android"))]
mod dialogs {
    fn chooser(title: &str, types: &[String]) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new();
        if !title.is_empty() {
            dialog = dialog.set_title(title);
        }
        if !types.is_empty() {
            let label = types.iter().map(|t| format!(".{t}")).collect::<Vec<_>>().join(", ");
            dialog = dialog.add_filter(label, types);
        }
        dialog
    }

    /// The file chosen to open, or to save as (`name` is suggested).
    pub fn file(save: bool, title: &str, name: &str, types: &[String]) -> Option<String> {
        let mut dialog = chooser(title, types);
        let path = if save {
            if !name.is_empty() {
                dialog = dialog.set_file_name(name);
            }
            dialog.save_file()
        } else {
            dialog.pick_file()
        };
        path.map(|p| p.to_string_lossy().into_owned())
    }

    pub fn folder(title: &str) -> Option<String> {
        chooser(title, &[]).pick_folder().map(|p| p.to_string_lossy().into_owned())
    }

    pub fn files(title: &str, types: &[String]) -> Vec<String> {
        chooser(title, types).pick_files().unwrap_or_default().iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }

    /// Whether the dialog was confirmed. `level`: 0 a note, 1 a warning, 2 an error.
    pub fn message(confirm: bool, title: &str, message: &str, ok: &str, cancel: &str, level: i64) -> bool {
        let mut dialog = rfd::MessageDialog::new().set_title(title).set_description(message).set_level(match level {
            1 => rfd::MessageLevel::Warning,
            2 => rfd::MessageLevel::Error,
            _ => rfd::MessageLevel::Info,
        });
        dialog = if confirm {
            dialog.set_buttons(rfd::MessageButtons::OkCancelCustom(ok.to_string(), cancel.to_string()))
        } else {
            dialog.set_buttons(rfd::MessageButtons::OkCustom(ok.to_string()))
        };
        match dialog.show() {
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => true,
            rfd::MessageDialogResult::Custom(label) => label == ok,
            _ => false,
        }
    }
}

/// On Android, messages, questions and the file chooser are the system's.
/// There are no folders to choose there: a program can't walk the folders
/// of a phone by their paths.
#[cfg(target_os = "android")]
mod dialogs {
    use super::super::android::{self, Choose};

    pub fn file(save: bool, _: &str, name: &str, types: &[String]) -> Option<String> {
        android::choose(if save { Choose::Save } else { Choose::Open }, name, types).into_iter().next()
    }
    pub fn folder(_: &str) -> Option<String> {
        None
    }
    pub fn files(_: &str, types: &[String]) -> Vec<String> {
        android::choose(Choose::OpenSeveral, "", types)
    }
    pub fn message(confirm: bool, title: &str, message: &str, ok: &str, cancel: &str, _: i64) -> bool {
        android::ask(title, message, ok, confirm.then_some(cancel))
    }
}

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

/// The app's clock, in seconds (a script's own, in a headless run).
pub(super) fn clock() -> f64 {
    now()
}

/// Wakes the app up (from a sound that started playing: it looks after it).
pub(super) fn wake_for_media() {
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// Moves a headless run's clock to `t` seconds after it started: the
/// timers' clock, and `now()`'s.
fn set_script_clock(t: f64) {
    CLOCK.with(|c| *c.borrow_mut() = Clock::Virtual(t));
    crate::stdlib::move_script_clock(t);
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
    /// A request's or download's status and headers, before its body.
    Headers(u64, net::HttpResult),
    /// More of a streamed body has arrived (see `net::stream`).
    Chunk(u64),
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
/// The inputs of running commands started with `input: true`, for
/// `sendToCommand`.
static INPUTS: Mutex<Vec<(u64, std::process::ChildStdin)>> = Mutex::new(Vec::new());

thread_local! {
    /// Completion blocks waiting for their loads, by id.
    static WAITING: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
    /// `onOutput` and `onExit` blocks of running commands, by id.
    static COMMANDS: RefCell<HashMap<u64, (Callback, Callback)>> = RefCell::new(HashMap::new());
    /// `background` work still running: the blocks that get its result and
    /// its progress.
    static COMPUTING: RefCell<HashMap<u64, (Callback, Option<Callback>, Discard)>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        n.set(n.get() + 1);
        n.get()
    })
}

/// Wakes the app's loop (something waits for it that isn't an `Event`).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn wake() {
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// Whether a wake for background results is on its way and their
/// delivery hasn't started: one wake at a time is enough.
static WAKE_SENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn post(event: Event) {
    EVENTS.lock().unwrap().push(event);
    // A wake for each result would starve timers and drawing: winit hands
    // the app wakes for as long as more arrive, and a result's block can
    // start work that finishes before the next wake is looked for (#56).
    // So a result that comes while one wake waits is left to `about_to_wait`.
    if WAKE_SENT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// Runs the blocks of background results in `about_to_wait`: from here,
/// results that come wake the app again.
fn deliver_background_and_rearm() -> bool {
    WAKE_SENT.store(false, std::sync::atomic::Ordering::SeqCst);
    deliver_background()
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

/// `runCommand(path:args:input:onOutput:onExit:)`: starts a program. Each
/// line it prints (stdout and stderr) goes to `onOutput` (`void fn(env,
/// String)`), then its exit code to `onExit` (`void fn(env, Int)`), on the
/// UI thread. With `input`, it can be written to (`sendToCommand`);
/// otherwise it has nothing to read. Returns an id for `stopCommand`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_run_command(
    path: *const Str,
    args: *const crate::List,
    input: u8,
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
        .stdin(if input != 0 { std::process::Stdio::piped() } else { std::process::Stdio::null() })
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
    if let Some(stdin) = child.stdin.take() {
        INPUTS.lock().unwrap().push((id, stdin));
    }
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
        INPUTS.lock().unwrap().retain(|(i, _)| *i != id);
        post(Event::Exited(id, code));
    });
    id as i64
}

/// `sendToCommand(id, text:)`: writes a line to a running command started
/// with `input: true`. False if it isn't running, or wasn't.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_send_to_command(id: i64, text: *const Str) -> u8 {
    use std::io::Write;
    let text = unsafe { crate::str_text(text) };
    let mut inputs = INPUTS.lock().unwrap();
    let Some((_, input)) = inputs.iter_mut().find(|(i, _)| *i == id as u64) else { return 0 };
    (writeln!(input, "{text}").is_ok() && input.flush().is_ok()) as u8
}

/// `stopCommand(id)`: stops a running command (its `onExit` still runs).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_stop_command(id: i64) {
    let child = CHILDREN.lock().unwrap().iter().find(|(i, _)| *i == id as u64).map(|(_, c)| c.clone());
    if let Some(c) = child {
        let _ = c.lock().unwrap().kill();
    }
}

/// Frees the result of `background` work that won't be delivered.
type Discard = unsafe extern "C" fn(*mut u8);

/// `background(work:progress:) { result in … }`: runs `worker(input, id)`
/// on another thread, then `done(result)` on the UI thread. The compiler
/// decides what `input` and the result are: the values themselves, marked
/// shared (see `SHARED` in the runtime), or codec trees of copies (for
/// values holding functions). The worker owns (and frees) `input`, and
/// `done` owns the result; `discard` frees a result `done` doesn't get
/// (the job was cancelled). Returns the job's id, for `BackgroundJob`'s
/// methods.
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
    discard: Discard,
) -> i64 {
    let id = next_id();
    let progress = (!progress_f.is_null()).then(|| Callback::new(progress_f, progress_env));
    COMPUTING.with(|c| c.borrow_mut().insert(id, (Callback::new(done_f, done_env), progress, discard)));
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
                let cb = COMPUTING.with(|c| c.borrow().get(&id).and_then(|(_, p, _)| p.clone()));
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
                    Some((done, progress, _)) if !cancelled => {
                        if let (Some(cb), Some((fraction, note))) = (progress, latest) {
                            cb.call_with_float_ptr(fraction, crate::str_new(&note) as *mut u8);
                        }
                        done.call_with(tree as *mut u8);
                    }
                    Some((_, _, discard)) => unsafe { discard(tree as *mut u8) },
                    None => {}
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
        dialogs::file(save != 0, &title, &name, &types)
    };
    // Saving: a name that doesn't end in one of the types gets the first
    // type's extension (for scripted answers too, so tests behave like the
    // real dialog). One that ends in any of them is kept: that's the type
    // the user chose.
    let chosen = chosen.map(|mut p| {
        let lower = p.to_lowercase();
        let typed = types.iter().any(|t| lower.ends_with(&format!(".{}", t.to_lowercase())));
        if let (true, false, Some(ext)) = (save != 0, typed, types.first()) {
            p = format!("{p}.{ext}");
        }
        p
    });
    match chosen {
        Some(p) => crate::str_new(&p),
        None => std::ptr::null_mut(),
    }
}

/// The next scripted answer, if one waits: `Some(None)` for `answer cancel`.
pub fn scripted_answer() -> Option<Option<String>> {
    DIALOG_ANSWERS.with(|a| a.borrow_mut().pop_front())
}

/// `chooseFolder(title:)`: a native dialog for picking a folder; its path,
/// or null if cancelled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_choose_folder(title: *const Str) -> *mut Str {
    let title = unsafe { crate::str_text(title) }.to_string();
    let chosen = if std::env::var_os("TESSEL_SCRIPT").is_some() {
        DIALOG_ANSWERS.with(|a| a.borrow_mut().pop_front()).flatten()
    } else {
        dialogs::folder(&title)
    };
    match chosen {
        Some(p) => crate::str_new(&p),
        None => std::ptr::null_mut(),
    }
}

/// `openFiles(title:types:)`: like `openFile`, for several files at once;
/// the chosen paths (none if cancelled). A scripted answer names them with
/// `|` between.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_open_files(title: *const Str, types: *const crate::List) -> *mut crate::List {
    let title = unsafe { crate::str_text(title) }.to_string();
    let n = unsafe { crate::tsl_list_len(types) };
    let types: Vec<String> = (0..n)
        .map(|i| unsafe {
            let p = crate::tsl_list_get(types, i, std::ptr::null()) as *const *const Str;
            crate::str_text(*p).trim_start_matches('.').to_string()
        })
        .collect();
    let chosen: Vec<String> = if std::env::var_os("TESSEL_SCRIPT").is_some() {
        let answer = DIALOG_ANSWERS.with(|a| a.borrow_mut().pop_front()).flatten();
        answer.map_or(Vec::new(), |a| a.split('|').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
    } else {
        dialogs::files(&title, &types)
    };
    unsafe { crate::str_list(chosen.iter().map(String::as_str)) }
}

/// `alert(title, message:)` (`confirm` = 0) and `confirm(title, message:,
/// confirmLabel:, cancelLabel:)` (`confirm` = 1): a native message dialog,
/// which the program waits for. Whether it was confirmed (always, for an
/// alert). `level` is 0 for a note, 1 for a warning, 2 for an error.
///
/// In a scripted run the dialog is printed instead, and `answer` says how
/// a `confirm` ends: `answer ok` (or any text) confirms the next one,
/// `answer cancel` cancels it; with no answer waiting, it's cancelled.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_message_dialog(
    confirm: u8,
    title: *const Str,
    message: *const Str,
    confirm_label: *const Str,
    cancel_label: *const Str,
    level: i64,
) -> u8 {
    let text = |s: *const Str| unsafe { crate::str_text(s) }.to_string();
    let (title, message, ok, cancel) = (text(title), text(message), text(confirm_label), text(cancel_label));
    if std::env::var_os("TESSEL_SCRIPT").is_some() {
        let kind = if confirm != 0 { "Confirm" } else { "Alert" };
        let detail = if message.is_empty() { String::new() } else { format!(" {message:?}") };
        let buttons = if confirm != 0 { format!(" [{cancel}] [{ok}]") } else { String::new() };
        println!("{kind} {title:?}{detail}{buttons}");
        if confirm == 0 {
            return 1;
        }
        return DIALOG_ANSWERS.with(|a| a.borrow_mut().pop_front()).flatten().is_some() as u8;
    }
    dialogs::message(confirm != 0, &title, &message, &ok, &cancel, level) as u8
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
    // (A command that's written to goes on for as long as it's told to: a
    // script doesn't wait for it to end, but for what it does, with `waitfor`.)
    let written_to: Vec<u64> = INPUTS.lock().unwrap().iter().map(|(id, _)| *id).collect();
    WAITING.with(|w| !w.borrow().is_empty())
        || COMMANDS.with(|c| c.borrow().keys().any(|id| !written_to.contains(id)))
        || COMPUTING.with(|c| !c.borrow().is_empty())
        || net::working()
}

/// Anything running at all: background work, open WebSockets, commands
/// (also ones that are written to). A headless `waitfor` moves the clock
/// on only when there's none.
fn anything_running() -> bool {
    background_busy() || COMMANDS.with(|c| !c.borrow().is_empty())
}

/// How far a headless `waitfor` moves the clock, at most, looking for its
/// label (an hour: an app with a repeating timer would go on for ever).
const WAITFOR_SPAN: f64 = 3600.0;

/// A headless run: lets background work in progress finish (until
/// `deadline`), running the blocks of its results, and of work they start.
fn settle(app: &mut App, deadline: Instant) {
    while jobs_busy() && Instant::now() < deadline {
        if deliver_background() {
            app.dirty = true;
            app.update();
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
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
    /// A ribbon's menu item (`RibbonMenu`): it opens its menu.
    RibbonMenu(u64),
    /// A collapsed ribbon group's button, which opens (or closes) its
    /// panel; and the open panel, besides its items.
    RibbonGroup(u64),
    RibbonPanel(u64),
    /// A table's header over a column, or the edge between a column and
    /// the next (to resize it); a row (by its place among the rows), or the
    /// arrow that opens a tree's node. All by the table.
    TableHeader(u64, usize),
    TableResize(u64, usize),
    TableRow(u64, usize),
    TreeArrow(u64, usize),
    /// The divider of a split view, which is dragged.
    Split(u64),
    /// A search field's button that clears it.
    SearchClear(u64),
    /// A menu picker's box: it opens (or closes) its list.
    MenuPicker(u64),
    /// A video: its play button (0), its track (1), or elsewhere on it (2).
    Video(u64, u8),
}

impl Hit {
    fn key(&self) -> u64 {
        match *self {
            Hit::Button(k) | Hit::Toggle(k) | Hit::Field(k) | Hit::Segment(k, _) | Hit::Tap(k) | Hit::Link(k, _) | Hit::Drag(k) => k,
            Hit::Pointer(k) | Hit::Slider(k) | Hit::Stepper(k, _) | Hit::ColorPicker(k) | Hit::Swatch(k) | Hit::Palette(k) => k,
            Hit::RibbonMenu(k) | Hit::RibbonGroup(k) | Hit::RibbonPanel(k) => k,
            Hit::Disclosure(k) => k,
            Hit::Tab(k, _) | Hit::TabClose(k, _) => k,
            Hit::TableHeader(k, _) | Hit::TableResize(k, _) | Hit::TableRow(k, _) | Hit::TreeArrow(k, _) => k,
            Hit::Split(k) | Hit::SearchClear(k) | Hit::MenuPicker(k) | Hit::Video(k, _) => k,
        }
    }
}

/// Can the keyboard go to this view with Tab (besides text fields)?
fn focusable(n: &Node) -> bool {
    matches!(
        n.kind,
        Kind::Slider { .. }
            | Kind::Stepper { .. }
            | Kind::ColorPicker { .. }
            | Kind::Disclosure { .. }
            | Kind::Button { .. }
            | Kind::Toggle { .. }
            | Kind::Picker { .. }
            | Kind::TabView { .. }
            | Kind::Table(_)
    )
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
    /// The app's `.trayIcon`, as the tree last described it.
    tray: Option<super::tray::TrayInfo>,
    /// The main window is to be hidden (true: closed while the app goes on
    /// in its tray icon) or shown again (`showWindow()`, the tray icon).
    show_main: Option<bool>,
    /// Whether the main window is hidden that way.
    main_hidden: bool,
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
    /// The menu of a ribbon's `RibbonMenu`.
    Item(u64),
}

thread_local! {
    /// The window events are handled for (`closeWindow()` closes it).
    static CURRENT_WINDOW: Cell<u64> = const { Cell::new(0) };
    /// Windows `closeWindow()` closed, and whether `quit()` was called.
    static CLOSING: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static QUITTING: Cell<bool> = const { Cell::new(false) };
    /// `showWindow()` was called.
    static SHOWING: Cell<bool> = const { Cell::new(false) };
    /// `showNotification`s not yet shown: the title and the message.
    pub(super) static NOTIFYING: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// `showWindow()`: shows the main window again (after it was closed with a
/// tray icon keeping the app running), in front.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_show_window() {
    SHOWING.with(|s| s.set(true));
}

/// `showNotification(title, message:)`: a notification from the system. A
/// headless run only says so.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_show_notification(title: *const Str, message: *const Str) {
    let (title, message) = unsafe { (crate::str_text(title).to_string(), crate::str_text(message).to_string()) };
    if std::env::var_os("TESSEL_SCRIPT").is_some() {
        println!("(notification {title:?} {message:?})");
        return;
    }
    NOTIFYING.with(|n| n.borrow_mut().push((title, message)));
    if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
        let _ = proxy.send_event(());
    }
}

/// `closeWindow()`: closes the window whose event is handled (for the main
/// window, the app ends), without asking `.onCloseRequest`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_close_window() {
    let key = CURRENT_WINDOW.with(Cell::get);
    CLOSING.with(|c| c.borrow_mut().push(key));
}

/// `scrollTo(id, align:)`: the scroll views around the view with that
/// `.id` (a `String`'s hash) scroll so it shows. `align` is a `ScrollAlign`
/// tag: 0 nearest, 1 top, 2 center, 3 bottom. It's done once the views are
/// next laid out, so a view the same action adds is there to scroll to.
/// With `scoped` (`in:`), only the view with `.id` `within` (a list, or a
/// view around one) is looked in.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_scroll_to(id: u64, align: i32, within: u64, scoped: u8) {
    let target = ScrollTarget { id, align: align.clamp(0, 3) as u8, within: (scoped != 0).then_some(within) };
    SCROLL_TARGET.with(|t| t.set(Some(target)));
}

/// What `scrollTo` asked for.
#[derive(Clone, Copy)]
struct ScrollTarget {
    id: u64,
    align: u8,
    /// `in:`: the `.id` of the view to look in.
    within: Option<u64>,
}

thread_local! {
    static SCROLL_TARGET: Cell<Option<ScrollTarget>> = const { Cell::new(None) };
    static DISMISSING: Cell<bool> = const { Cell::new(false) };
    /// `showNotice`: the text that shows, and until when (on the app's clock).
    static NOTICE: RefCell<Option<(String, f64)>> = const { RefCell::new(None) };
}

/// `showNotice(text, seconds:)`: a short message near the bottom of the
/// window, which goes by itself after a moment. A new one takes the place
/// of the one that shows; no text takes it away.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_notice(text: *const Str, seconds: f64) {
    let text = unsafe { crate::str_text(text) }.to_string();
    let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 2.5 };
    NOTICE.with(|n| *n.borrow_mut() = (!text.is_empty()).then(|| (text, now() + seconds)));
}

/// When the notice that shows is to go, if one shows.
fn notice_time() -> Option<f64> {
    NOTICE.with(|n| n.borrow().as_ref().map(|(_, until)| *until))
}

/// `dismiss()`: closes the sheet or popover that's on top (its
/// `isPresented` becomes false), as Escape does.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_dismiss() {
    DISMISSING.with(|d| d.set(true));
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
    /// An item of the tray icon's menu (its path), or a click on the icon
    /// itself (which shows the main window).
    Tray(Vec<usize>),
    #[cfg_attr(not(windows), allow(dead_code))]
    TrayClick,
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
    /// `setFullScreen(on)`, done when the main window is next drawn; and
    /// whether it fills the screen.
    static FULL_SCREEN: Cell<Option<bool>> = const { Cell::new(None) };
    static IS_FULL_SCREEN: Cell<bool> = const { Cell::new(false) };
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

/// `setFullScreen(on)`: the main window fills the screen (or doesn't).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_set_full_screen(on: u8) {
    FULL_SCREEN.with(|f| f.set(Some(on != 0)));
    // (Said at once; the window says how it is once it's drawn again.)
    IS_FULL_SCREEN.with(|f| f.set(on != 0));
}

/// `isFullScreen()`: whether the main window fills the screen.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_is_full_screen() -> u8 {
    IS_FULL_SCREEN.with(Cell::get) as u8
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

/// How far either side of a split view's divider it can be caught.
const SPLIT_GRAB: f64 = 3.0;

/// Keys that edit text or move in it, which `App::key` handles.
const EDIT_KEYS: &[&str] =
    &["backspace", "delete", "left", "right", "up", "down", "home", "end", "enter", "escape", "tab"];

impl App {
    /// Rebuilds the trees if state may have changed, then lays them out.
    fn update(&mut self) {
        if let Some(redo) = UNDO.with(|u| u.take()) {
            self.undo(redo);
        }
        self.each_surface(|a| a.finish_cell_edit());
        let mut playing = false;
        self.each_surface(|a| {
            playing |= a.tick_videos().0;
        });
        let _ = playing;
        let commands = super::rich::COMMANDS.with(|q| std::mem::take(&mut *q.borrow_mut()));
        if !commands.is_empty() {
            self.each_surface(|a| a.rich_commands(&commands));
        }
        self.each_surface(|a| a.rich_marks());
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
                self.each_surface(|a| a.sync_videos());
                self.each_surface(|a| {
                    a.sync_editor_lines();
                    a.track_editor_texts();
                });
                self.take_requests();
            }
            let mut again = false;
            let mut lazy_seen = Vec::new();
            self.each_surface(|a| {
                a.lay_out();
                again |= a.scroll_to_target();
                again |= a.run_on_resize();
                again |= a.lazy_rows(&mut lazy_seen);
            });
            // (Only once every window has said which lists it has: one
            // window's lists aren't another's to forget.)
            super::controls::LAZY.with(|l| l.borrow_mut().retain(|k, _| lazy_seen.contains(k)));
            if !again {
                break;
            }
        }
        // (A view that isn't there isn't scrolled to later.)
        SCROLL_TARGET.with(|t| t.set(None));
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
                a.ui.anim.step(tree, now(), (a.width, a.height), (&a.ui.scroll, &a.ui.scroll_x));
            }
        });
        // A palette whose picker is gone closes, and so does the panel of a
        // ribbon group that's gone or has room again.
        if !picker_shown {
            super::controls::OPEN_PICKER.set(None);
        }
        if let Some(k) = super::controls::OPEN_GROUP.get() {
            let mut shown = false;
            self.each_surface(|a| shown |= a.node(k).is_some_and(|n| n.layout.collapsed));
            if !shown {
                super::controls::OPEN_GROUP.set(None);
            }
        }
        if self.deliver_open_files() {
            self.update();
        }
    }

    /// `scrollTo`: when the view asked for is in this window, its scroll
    /// views scroll so it shows (true: lay out again).
    fn scroll_to_target(&mut self) -> bool {
        let Some(ScrollTarget { id, align, within }) = SCROLL_TARGET.with(Cell::get) else { return false };
        // The view's rectangle, and the scroll views around it (outermost
        // first). Views with that `.id` first (`rows`: then a list's row).
        // With `in:`, only inside the view with that `.id` (`inside`: in it).
        fn find(n: &Node, id: u64, rows: bool, within: Option<u64>, inside: bool, around: &mut Vec<u64>) -> Option<Rect> {
            if n.mods.hidden {
                return None;
            }
            let inside = inside || (within.is_some() && n.id == within);
            if n.id == Some(id) && !rows && inside {
                return Some(n.layout.rect);
            }
            let scrolls = matches!(n.kind, Kind::Scroll(_));
            if scrolls {
                around.push(n.key);
            }
            for c in n.visible_children() {
                if let Some(r) = find(c, id, rows, within, inside, around) {
                    return Some(r);
                }
            }
            // A `LazyVStack`'s row (a `LazyHStack`'s column), by its number (built or not).
            if let (Kind::Lazy { count, row_height, across, .. }, true) = (&n.kind, rows && inside)
                && id < *count as u64
            {
                let (row_height, along) = (*row_height, id as f64 * *row_height);
                let c = n.layout.rect.inset_by(n.mods.padding);
                return Some(if *across {
                    Rect { x: c.x + along, y: c.y, w: row_height, h: c.h }
                } else {
                    Rect { x: c.x, y: c.y + along, w: c.w, h: row_height }
                });
            }
            if scrolls {
                around.pop();
            }
            None
        }
        let mut around = Vec::new();
        let tree = self.tree.as_ref();
        let inside = within.is_none();
        let found = tree
            .and_then(|t| find(t, id, false, within, inside, &mut around))
            .or_else(|| tree.and_then(|t| find(t, id, true, within, inside, &mut around)));
        let Some(mut target) = found else { return false };
        SCROLL_TARGET.with(|t| t.set(None));
        // Innermost first: scrolling one moves the view within the next.
        for &key in around.iter().rev() {
            let Some(n) = self.node(key) else { continue };
            let Kind::Scroll(axes) = n.kind else { continue };
            let (down, across) = super::scroll_axes(axes);
            let content = n.layout.rect.inset_by(n.mods.padding);
            let (dy, dx) = super::layout::scroll_offsets(n, content, &self.ui.scroll, &self.ui.scroll_x);
            let (max_y, max_x) = ((n.layout.content_h - content.h).max(0.0), (n.layout.content_w - content.w).max(0.0));
            // How far the view's start is to move, to start `start` along,
            // in a window of `size`.
            let shift = |start: f64, len: f64, from: f64, size: f64| -> f64 {
                let (top, end) = (start - from, start + len - (from + size));
                match align {
                    1 => top,
                    2 => start + len / 2.0 - (from + size / 2.0),
                    3 => end,
                    // Nearest: no further than it takes to show it (its start
                    // when it's larger than the view).
                    _ if top < 0.0 || len > size => top,
                    _ if end > 0.0 => end,
                    _ => 0.0,
                }
            };
            if down {
                let new = (dy + shift(target.y, target.h, content.y, content.h)).clamp(0.0, max_y);
                self.ui.scroll.insert(key, new);
                target.y -= new - dy;
            }
            if across {
                let new = (dx + shift(target.x, target.w, content.x, content.w)).clamp(0.0, max_x);
                self.ui.scroll_x.insert(key, new);
                target.x -= new - dx;
            }
        }
        true
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
        Layouter { text: &mut self.text, scroll: &self.ui.scroll, scroll_x: &self.ui.scroll_x, columns: Vec::new(), composing }.run_below(tree, self.width, self.height, top);
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
        self.tray = self.find_in_main(|n| n.mods.tray.is_some()).and_then(|n| n.mods.tray.as_deref()).map(|t| super::tray::TrayInfo {
            icon: t.icon,
            title: t.title.clone(),
            keep_running: t.keep_running,
            items: menu::entries(&t.items),
        });
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
        // `dismiss()`: the topmost sheet or popover closes.
        if DISMISSING.with(|d| d.replace(false)) {
            self.close_top_popover();
        }
        if QUITTING.with(|q| q.replace(false)) {
            self.quit = true;
        }
        for key in CLOSING.with(|c| std::mem::take(&mut *c.borrow_mut())) {
            self.close_window(key);
        }
        if SHOWING.with(|s| s.replace(false)) {
            self.show_main = Some(true);
            self.main_hidden = false;
        }
    }

    /// Closes a window (the main one: the app ends).
    fn close_window(&mut self, key: u64) {
        if key == 0 {
            // With a tray icon that keeps the app running: it's hidden.
            if self.tray.as_ref().is_some_and(|t| t.keep_running) {
                self.show_main = Some(false);
                self.main_hidden = true;
                return;
            }
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
            MenuSource::Item(key) => self.node(key).map(menu::entries),
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
        let bar = self.bar.clone();
        self.choose_item(&bar, labels)
    }

    /// Chooses the item of `menus` that `labels` name (menu by menu).
    fn choose_item(&mut self, menus: &[Entry], labels: &[String]) -> Result<(), String> {
        let mut items: &[Entry] = menus;
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
        visit(self.point_root(x, y)?, x, y)
    }

    /// The popovers that are open, the topmost last.
    fn popovers(&self) -> Vec<&Node> {
        let Some(tree) = &self.tree else { return Vec::new() };
        tree.children.iter().filter(|c| matches!(c.kind, Kind::Popover { .. })).collect()
    }

    /// Where to look for what's at a point: the topmost popover it's in,
    /// or the sheet on top (which keeps everything under it out of reach),
    /// or else the whole window.
    fn point_root(&self, x: f64, y: f64) -> Option<&Node> {
        for p in self.popovers().into_iter().rev() {
            let sheet = matches!(p.kind, Kind::Popover { edge, .. } if edge == super::SHEET);
            if sheet || p.layout.rect.contains(x, y) {
                return Some(p);
            }
        }
        self.tree.as_ref()
    }


    /// point (all of them, for a point outside). Returns whether any closed.
    fn close_popovers_outside(&mut self, x: f64, y: f64) -> bool {
        let mut closed = false;
        // (A click outside a sheet does nothing: it's taken, and the sheet stays.)
        let mut blocked = false;
        for p in self.popovers().into_iter().rev() {
            if p.layout.rect.contains(x, y) {
                break;
            }
            if let Kind::Popover { presented, edge, .. } = &p.kind {
                if *edge == super::SHEET {
                    blocked = true;
                    break;
                }
                unsafe { *presented.locate() = 0 };
                closed = true;
            }
        }
        if blocked && !closed {
            return true;
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
            MenuSource::Item(key) => {
                let label = match self.node(key).map(|n| &n.kind) {
                    Some(Kind::RibbonItem { label, .. }) => label.as_str(),
                    _ => "",
                };
                out.push_str(&format!("RibbonMenu {label:?}\n"));
            }
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
            if typable(n) || focusable(n) {
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
        let text = shown_text(node).unwrap_or_default();
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
        let Some(text) = shown_text(node) else {
            self.ui.editing = None;
            return;
        };
        e.clamp(&text);
        // (Text that's only selected doesn't scroll.)
        if selectable_text(node) {
            return;
        }
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
            Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. } => {
                let (cy0, cy1) = caret.map_or((0.0, 16.0), |c| (c.y0, c.y1));
                let visible_h = (content.h - 12.0).max(1.0);
                let sy = self.ui.scroll.entry(e.key).or_insert(0.0);
                keep(sy, cy0, cy1, visible_h, layout.height());
            }
            _ => {
                let caret_x = caret.map_or(0.0, |c| c.x0);
                let room = if matches!(node.kind, Kind::TextField { search: true, .. }) { super::layout::SEARCH_LEFT + super::layout::SEARCH_RIGHT } else { 16.0 };
                let visible = (node.layout.rect.w - room).max(1.0);
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
    /// whether any is. Adds the lists it has to `seen` (the caller forgets
    /// the rows of lists no window has).
    fn lazy_rows(&mut self, seen: &mut Vec<u64>) -> bool {
        fn visit(n: &Node, clip: Rect, out: &mut Vec<(u64, usize, usize)>, seen: &mut Vec<u64>) {
            if n.mods.hidden {
                return;
            }
            let r = n.layout.rect;
            let mut clip = clip;
            if matches!(n.kind, Kind::Scroll(_)) || n.mods.clipped {
                let c = r.inset_by(n.mods.padding);
                let (x0, y0) = (clip.x.max(c.x), clip.y.max(c.y));
                let (x1, y1) = ((clip.x + clip.w).min(c.x + c.w), (clip.y + clip.h).min(c.y + c.h));
                clip = Rect { x: x0, y: y0, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0) };
            }
            if let Kind::Lazy { count, row_height, first, across } = n.kind {
                seen.push(n.key);
                let c = r.inset_by(n.mods.padding);
                // (Columns: the same, across.)
                let (top, bottom, c) = if across {
                    let c = Rect { x: c.y, y: c.x, w: c.h, h: c.w };
                    ((clip.x).max(c.y), (clip.x + clip.w).min(c.y + c.h), c)
                } else {
                    (clip.y.max(c.y), (clip.y + clip.h).min(c.y + c.h), c)
                };
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
        let mut wanted = Vec::new();
        if let Some(t) = &self.tree {
            visit(t, Rect { x: 0.0, y: 0.0, w: self.width, h: self.height }, &mut wanted, seen);
        }
        super::controls::LAZY.with(|l| {
            let mut l = l.borrow_mut();
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
            if let (true, true, true) = (n.mods.autofocus, new, typable(n)) {
                *focus = Some((n.key, shown_text(n).unwrap_or_default()));
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
        super::scene3d::keep_only(&self.keys);
        super::table::keep_only(&self.keys);
        super::rich::keep_only(&self.keys);
        let back = self.current;
        for (window, (key, text)) in focus {
            if self.enter(window) {
                let mut e = Editing::new(key, &text);
                // A color picker's hex field, and a table's cell, start with
                // their text selected, to type over.
                let cell = super::table::CELL.with(|c| c.borrow().as_ref().and_then(|c| c.field)) == Some(key);
                if Some(key) == super::controls::HEX_FIELD.get() || cell {
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
            Kind::Text(_) => (node.layout.content.x, node.layout.content.y, 0.0, 0.0),
            Kind::CodeEditor { .. } => {
                let (x0, y0) = code_origin(node);
                let sx = self.ui.scroll_x.get(&node.key).copied().unwrap_or(0.0);
                let sy = self.ui.scroll.get(&node.key).copied().unwrap_or(0.0);
                (x0, y0, sx, sy)
            }
            Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. } => {
                let (x0, y0) = field_origin(node);
                (x0, y0, 0.0, self.ui.scroll.get(&node.key).copied().unwrap_or(0.0))
            }
            _ => {
                let (x0, y0) = field_origin(node);
                let sx = self.ui.editing.as_ref().filter(|e| e.key == node.key).map_or(0.0, |e| e.scroll);
                (x0, y0, sx, 0.0)
            }
        }
    }

    /// Whether the text being edited is a text field of several lines.
    fn editing_lines(&self) -> bool {
        let key = self.ui.editing.as_ref().map(|e| e.key);
        key.and_then(|k| self.node(k)).is_some_and(|n| matches!(n.kind, Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. }))
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

    /// The text being edited, with its selection.
    #[cfg(target_os = "android")]
    fn edited(&self) -> Option<super::android::Text> {
        let e = self.ui.editing.as_ref()?;
        let node = self.node(e.key)?;
        let text = shown_text(node).filter(|_| typable(node))?;
        Some(super::android::Text { anchor: e.anchor.min(text.len()), focus: e.focus.min(text.len()), text })
    }

    /// A phone's keyboard changes the text being edited: what's in `range`
    /// becomes `text`, and then `select` is selected. All are counted as
    /// Java does (see `android::byte_at`).
    #[cfg(target_os = "android")]
    fn replace(&mut self, range: std::ops::Range<usize>, text: &str, select: (usize, usize)) {
        use super::android::byte_at;
        let Some(now) = self.edited() else { return };
        let (start, end) = (byte_at(&now.text, range.start), byte_at(&now.text, range.end));
        if start < end || !text.is_empty() {
            if let Some(e) = self.ui.editing.as_mut() {
                (e.anchor, e.focus) = (start, end.max(start));
            }
            self.edit(if text.is_empty() { Edit::Backspace(edit::Unit::Char) } else { Edit::Insert(text.to_string()) });
        }
        let Some(now) = self.edited() else { return };
        if let Some(e) = self.ui.editing.as_mut() {
            (e.anchor, e.focus) = (byte_at(&now.text, select.0), byte_at(&now.text, select.1));
        }
        self.dirty = true;
    }

    /// What a screen reader is told is in the window (titled `title`), and
    /// which of it the keyboard is on.
    fn accessible(&self, title: &str) -> (Vec<super::access::Item>, Option<u64>) {
        let focus = self.ui.editing.as_ref().map(|e| e.key).or(self.ui.focused);
        let items = match self.tree.as_ref() {
            Some(tree) => super::access::describe(tree, title, (self.width, self.height)),
            None => Vec::new(),
        };
        (items, focus)
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
        self.notice_sync();
        let mut list = super::draw::DrawList::new();
        let tree = self.tree.as_ref().unwrap();
        // The views (kept as drawn, for the ones that are gone by the next
        // drawing and fade out), then the ones fading out now.
        let at = now();
        self.ui.anim.leave(at);
        let mut views = super::draw::DrawList::new();
        let mut painter = Painter { scene: &mut views, ui: &self.ui, scale, offset: (0.0, 0.0), turn: kurbo::Affine::IDENTITY, clip: None, marks: Vec::new() };
        painter.paint(tree);
        let marks = std::mem::take(&mut painter.marks);
        let views = std::rc::Rc::new(views);
        list.append(views.clone(), kurbo::Affine::IDENTITY);
        let window = kurbo::Rect::new(0.0, 0.0, self.width * scale, self.height * scale);
        for (ghost, shows) in self.ui.anim.leaving(at) {
            list.push(super::draw::FillRule::NonZero, shows as f32, kurbo::Affine::IDENTITY, &window);
            list.append(ghost, kurbo::Affine::IDENTITY);
            list.pop();
        }
        self.ui.anim.drew(views, marks);
        let mut painter = Painter { scene: &mut list, ui: &self.ui, scale, offset: (0.0, 0.0), turn: kurbo::Affine::IDENTITY, clip: None, marks: Vec::new() };
        painter.paint_overlay(tree);
        painter.paint_dragged(tree);
        painter.paint_popup();
        painter.paint_menu_bar();
        painter.paint_menus();
        painter.paint_notice();
        painter.paint_tooltip();
        list
    }

    fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<Hit> {
            // In a view that's turned or scaled: the point as it is in the view.
            let (x, y) = match super::layout::turn_of(n) {
                Some(t) => {
                    let p = t.inverse() * kurbo::Point::new(x, y);
                    (p.x, p.y)
                }
                None => (x, y),
            };
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
            // A split view's divider, a little wider than it looks, to be
            // easy to catch (over the panes' edges).
            if let (Kind::Split { .. }, Some(line)) = (&n.kind, n.layout.segments.first())
                && line.inset(-SPLIT_GRAB).contains(x, y)
            {
                return Some(Hit::Split(n.key));
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
            if let Kind::Ribbon { .. } = n.kind {
                if let Some(i) = n.layout.tabs.iter().position(|t| t.rect.contains(x, y)) {
                    return Some(Hit::Tab(n.key, i));
                }
                return n.children.get(n.selected_ribbon_tab()).and_then(|c| visit(c, x, y));
            }
            // A collapsed group is its button (its items are in its panel,
            // found first, below).
            if matches!(n.kind, Kind::RibbonGroup { .. }) && n.layout.collapsed {
                return n.layout.rect.contains(x, y).then_some(Hit::RibbonGroup(n.key));
            }
            if let Kind::RibbonItem { role, .. } = &n.kind {
                return n.layout.rect.contains(x, y).then_some(match role {
                    super::RibbonRole::Button(_) => Hit::Button(n.key),
                    super::RibbonRole::Toggle(..) => Hit::Toggle(n.key),
                    super::RibbonRole::Menu => Hit::RibbonMenu(n.key),
                });
            }
            if let Kind::Picker { style: 2, .. } = n.kind {
                // (Its list's options are found first, above.)
                return n.layout.rect.contains(x, y).then_some(Hit::MenuPicker(n.key));
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
            // A table's header: its columns, and the edges between them.
            if let Kind::Table(_) = n.kind {
                let last = n.layout.tabs.len().saturating_sub(1);
                for (i, t) in n.layout.tabs.iter().enumerate() {
                    if t.rect.h <= 0.0 || !t.rect.contains(x, y) {
                        continue;
                    }
                    if x >= t.rect.x + t.rect.w - 4.0 && i < last {
                        return Some(Hit::TableResize(n.key, i));
                    }
                    if x <= t.rect.x + 4.0 && i > 0 {
                        return Some(Hit::TableResize(n.key, i - 1));
                    }
                    return Some(Hit::TableHeader(n.key, i));
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
                Kind::TextField { search: true, .. } if render::search_clear(n).is_some_and(|r| r.inset(-3.0).contains(x, y)) => {
                    Some(Hit::SearchClear(n.key))
                }
                Kind::TextField { .. } | Kind::CodeEditor { .. } | Kind::RichEditor { .. } => Some(Hit::Field(n.key)),
                Kind::Text(_) if selectable_text(n) && n.mods.on_tap.is_none() && n.mods.on_pointer.is_none() => Some(Hit::Field(n.key)),
                Kind::Slider { .. } => Some(Hit::Slider(n.key)),
                Kind::Stepper { .. } => {
                    let r = n.layout.rect.inset_by(n.mods.padding);
                    let b = super::layout::STEPPER_BUTTON;
                    let dir = if x < r.x + b { -1 } else if x >= r.x + r.w - b { 1 } else { 0 };
                    Some(Hit::Stepper(n.key, dir))
                }
                Kind::Swatch(_) => Some(Hit::Swatch(n.key)),
                Kind::Video { controls, .. } => {
                    let (bar, button, track) = render::video_parts(n);
                    let part = if !controls || !bar.contains(x, y) {
                        2
                    } else if button.inset(-4.0).contains(x, y) {
                        0
                    } else if (Rect { y: track.y - 10.0, h: track.h + 20.0, ..track }).contains(x, y) {
                        1
                    } else {
                        2
                    };
                    Some(Hit::Video(n.key, part))
                }
                Kind::TableRow(ref row) => {
                    let c = n.layout.content;
                    let arrow = c.x + super::table::CELL_PAD + row.depth as f64 * super::table::INDENT;
                    if row.expandable && x >= arrow - 2.0 && x < arrow + super::table::INDENT {
                        Some(Hit::TreeArrow(row.table, row.index))
                    } else {
                        Some(Hit::TableRow(row.table, row.index))
                    }
                }
                _ if n.mods.on_pointer.is_some() => Some(Hit::Pointer(n.key)),
                _ if n.mods.on_drag.is_some() => Some(Hit::Drag(n.key)),
                _ if n.mods.on_tap.is_some() => Some(Hit::Tap(n.key)),
                _ => None,
            }
        }
        // An open ribbon group's panel is over everything else.
        if let Some(n) = super::controls::OPEN_GROUP.get().and_then(|k| self.node(k)) {
            if n.layout.collapsed && n.layout.panel.contains(x, y) {
                let item = n.children.iter().rev().find_map(|c| visit(c, x, y));
                return Some(item.unwrap_or(Hit::RibbonPanel(n.key)));
            }
        }
        // An open menu picker's list is over everything else.
        if let Some(n) = super::controls::OPEN_MENU.get().and_then(|k| self.node(k)) {
            if let Some(i) = n.layout.segments.iter().position(|s| s.contains(x, y)) {
                return Some(Hit::Segment(n.key, i));
            }
        }
        // An open color picker's palette is over everything else.
        if let Some(palette) = self.palette() {
            if palette.layout.rect.contains(x, y) {
                return Some(visit(palette, x, y).unwrap_or(Hit::Palette(palette.key)));
            }
        }
        visit(self.point_root(x, y)?, x, y)
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
        visit(self.point_root(x, y)?, x, y)
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

    /// The innermost shown view with `.onKey` or `.onKeyEvent` that a point
    /// is in: a click there gives it the keyboard.
    fn on_key_at(&self, x: f64, y: f64) -> Option<u64> {
        fn find(n: &Node, x: f64, y: f64, found: &mut Option<u64>) {
            if n.mods.hidden || n.layout.style.disabled {
                return;
            }
            let r = n.layout.rect;
            if x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h {
                return;
            }
            if n.mods.on_key.is_some() || n.mods.on_key_event.is_some() {
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

    /// The view with `.onPointer` at a point.
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
        // keyboard to a view with `.onKey` or `.onKeyEvent` it's in (or is).
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
            // (Over text that can be typed in or selected: the text cursor.)
            let text = (typable(n) && !matches!(n.kind, Kind::CodeEditor { .. }) || selectable_text(n)) && !n.layout.style.disabled;
            inner.or(n.mods.cursor.filter(|_| n.layout.rect.contains(x, y))).or((text && n.layout.rect.contains(x, y)).then_some(5))
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
        let style = visit(self.point_root(x, y)?, x, y)?;
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
        visit(self.point_root(x, y)?, x, y)
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

    /// Brings the notice that shows in line with `showNotice`: a new one
    /// comes up, one whose time is over goes. Returns whether that changed
    /// what shows.
    fn notice_sync(&mut self) -> bool {
        let wanted = NOTICE.with(|n| {
            let mut n = n.borrow_mut();
            if n.as_ref().is_some_and(|(_, until)| now() >= *until) {
                *n = None;
            }
            n.as_ref().map(|(text, _)| text.clone())
        });
        let shown = self.ui.notice.as_ref().map(|n| n.text.clone());
        let Some(text) = wanted else {
            self.ui.notice = None;
            return shown.is_some();
        };
        let layout = Rc::new(self.text.layout(&text, 13.0, 500.0, Rgba([255, 255, 255, 255]), Some((self.width - 80.0).max(80.0))));
        let (w, h) = (layout.width() + 32.0, layout.height() + 16.0);
        // In the middle, near the bottom of the window.
        let rect = Rect { x: ((self.width - w) / 2.0).round(), y: (self.height - h - 24.0).max(4.0).round(), w, h };
        let changed = shown.as_deref() != Some(text.as_str());
        self.ui.notice = Some(render::Tooltip { rect, text, layout });
        changed
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
            let has = if target { n.mods.drop_target.is_some() } else { n.mods.draggable.is_some() || n.mods.drag_file.is_some() };
            inner.or_else(|| (has && n.layout.rect.contains(x, y)).then_some(n.key))
        }
        if self.palette().is_some_and(|p| p.layout.rect.contains(x, y)) {
            return None;
        }
        visit(self.point_root(x, y)?, x, y, target)
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
    fn run_on_key_event(&mut self, name: &str, text: &str, mods: Mods, repeated: bool, up: bool) -> bool {
        fn find(n: &Node) -> Option<(Callback, bool)> {
            if n.mods.hidden || n.layout.style.disabled {
                return None;
            }
            if let Some(cb) = &n.mods.on_key_event {
                return Some((cb.clone(), n.mods.key_releases));
            }
            n.visible_children().iter().find_map(find)
        }
        if name.is_empty() {
            return false;
        }
        // The view with the keyboard first, then the first one with `.onKeyEvent`.
        let to = self.ui.keys_to.and_then(|k| self.node(k)).filter(|n| !n.mods.hidden && !n.layout.style.disabled);
        let Some((cb, releases)) = to
            .and_then(|n| n.mods.on_key_event.as_ref().map(|cb| (cb.clone(), n.mods.key_releases)))
            .or_else(|| self.tree.as_ref().and_then(find))
        else {
            return false;
        };
        // (A key let go, or a modifier on its own, only where it's asked for.)
        if (up || MODIFIER_KEYS.contains(&name)) && !releases {
            return false;
        }
        let mut event = super::KeyC {
            key: crate::str_new(name),
            text: crate::str_new(text),
            shift: mods.shift as u8,
            alt: mods.alt as u8,
            command: mods.primary as u8,
            control: mods.control as u8,
            repeated: repeated as u8,
            up: up as u8,
        };
        cb.call_with(&mut event as *mut super::KeyC as *mut u8);
        self.dirty = true;
        true
    }

    /// A key pressed, by its name (`"a"` for the A key whatever the
    /// modifiers, `"left"`, `"f5"`, `"space"`…) and the text it types
    /// (`"A"` with Shift; empty for keys that type nothing).
    /// A phone's Back button: puts away what's on top (what's dragged, an
    /// open palette or menu, text being edited, a popover or sheet). False
    /// if there was nothing to put away.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn back(&mut self) -> bool {
        if self.ui.dragging.is_some() {
            self.end_drag(None);
        } else if super::controls::OPEN_PICKER.get().is_some() {
            self.close_picker();
        } else if self.menu.is_some() {
            self.menu = None;
        } else if self.ui.editing.is_some() {
            self.ui.editing = None;
        } else if !self.close_top_popover() {
            return false;
        }
        self.dirty = true;
        true
    }

    /// A key let go: only `.onKeyEvent` is told (while no text is edited),
    /// for what's done while a key is held.
    fn release_key(&mut self, name: &str, mods: Mods) {
        if self.ui.editing.is_none() && self.menu.is_none() {
            self.run_on_key_event(name, "", mods, false, true);
        }
    }

    fn press(&mut self, name: &str, text: &str, mods: Mods, repeated: bool) {
        use super::controls::{hex_field_text, parse_hex, HEX_FIELD, OPEN_PICKER};
        // A modifier on its own (Shift, Alt…) only goes to `.onKeyEvent`.
        if MODIFIER_KEYS.contains(&name) {
            if self.ui.editing.is_none() && self.menu.is_none() {
                self.run_on_key_event(name, "", mods, repeated, false);
            }
            return;
        }
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
        if name == "escape" && super::controls::OPEN_GROUP.take().is_some() {
            self.dirty = true;
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
        // An open menu, and the menu bar's shortcuts, come before the views.
        if self.menu.is_some() || self.bar_takes(name, mods) {
            self.key(name, mods);
            return;
        }
        // Shortcuts first, then the control the keyboard is on, then
        // `.onKeyEvent` (while no text is edited).
        if self.ui.editing.is_none() {
            // Escape closes the popover or sheet on top, before the app's
            // own `.onKeyEvent` could take the key.
            if name == "escape" && !mods.primary && self.close_top_popover() {
                return;
            }
            if mods.primary && self.run_shortcut(name, mods) {
                return;
            }
            self.ui.key_shift = mods.shift;
            if plain && self.control_key(name) {
                return;
            }
            if self.run_on_key_event(name, text, mods, repeated, false) {
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

    // ===== tables and trees =====

    /// A table's spec, and its scroll view's identity.
    fn table_parts(&self, table: u64) -> Option<(std::rc::Rc<super::table::Spec>, Option<u64>)> {
        let n = self.node(table)?;
        let Kind::Table(spec) = &n.kind else { return None };
        Some((spec.clone(), n.children.first().map(|c| c.key)))
    }

    /// Selects a table's row (a tree's node, by its place among those that
    /// show): only it, or with Shift the rows from where selecting started,
    /// or with Cmd (Ctrl) it too, or not any more. It's scrolled into view.
    fn select_row(&mut self, table: u64, row: usize, shift: bool, toggle: bool) {
        let Some((spec, scroll)) = self.table_parts(table) else { return };
        if spec.count == 0 {
            return;
        }
        let row = row.min(spec.count - 1);
        let anchor = self.ui.table_at.get(&table).map_or(row, |a| a.0);
        let anchor = if shift && spec.multiple { anchor } else { row };
        self.ui.table_at.insert(table, (anchor, row));
        if let Some(selection) = &spec.selection {
            if spec.tree.is_some() {
                let path = super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.get(row)).map(|p| p.0.clone()));
                if let Some(path) = path {
                    super::table::write_ints(selection, &path);
                }
            } else {
                let mut chosen = super::table::read_ints(selection);
                if spec.multiple && toggle {
                    match chosen.iter().position(|&r| r == row as i64) {
                        Some(i) => {
                            chosen.remove(i);
                        }
                        None => chosen.push(row as i64),
                    }
                } else if spec.multiple && shift {
                    chosen = (anchor.min(row)..=anchor.max(row)).map(|r| r as i64).collect();
                } else {
                    chosen = vec![row as i64];
                }
                chosen.sort_unstable();
                chosen.dedup();
                super::table::write_ints(selection, &chosen);
            }
        }
        // Into view: the scroll view moves as little as it takes.
        if let (Some(scroll), Some(view)) = (scroll, scroll.and_then(|s| self.node(s)).map(|n| n.layout.rect.h)) {
            let (top, bottom) = (row as f64 * spec.row_height, (row + 1) as f64 * spec.row_height);
            let at = self.ui.scroll.entry(scroll).or_insert(0.0);
            if top < *at {
                *at = top;
            } else if bottom > *at + view {
                *at = bottom - view;
            }
        }
        self.dirty = true;
    }

    // ===== web views =====

    /// The web views showing in this window (by key): what they show, and where.
    fn web_views(&self) -> HashMap<u64, super::webview::Wanted> {
        fn visit(n: &Node, clip: Rect, out: &mut HashMap<u64, super::webview::Wanted>) {
            if n.mods.hidden {
                return;
            }
            if let Kind::WebView { url, html } = &n.kind {
                let r = n.layout.rect.inset_by(n.mods.padding);
                let visible = r.w > 0.0 && r.h > 0.0 && r.x < clip.x + clip.w && r.y < clip.y + clip.h && r.x + r.w > clip.x && r.y + r.h > clip.y;
                out.insert(n.key, super::webview::Wanted { url: url.clone(), html: html.clone(), rect: r, visible });
            }
            for c in n.visible_children() {
                visit(c, clip, out);
            }
        }
        let mut out = HashMap::new();
        if let Some(t) = &self.tree {
            visit(t, Rect { x: 0.0, y: 0.0, w: self.width, h: self.height }, &mut out);
        }
        out
    }

    // ===== video =====

    /// The video views of this window: each one's key, path, bindings, and
    /// whether it loops and how loud it is.
    fn videos(&self) -> Vec<(u64, String, Option<Callback>, Option<Callback>, bool, f64)> {
        fn visit(n: &Node, out: &mut Vec<(u64, String, Option<Callback>, Option<Callback>, bool, f64)>) {
            if let Kind::Video { path, playing, position, looping, volume, .. } = &n.kind {
                out.push((n.key, path.clone(), playing.clone(), position.clone(), *looping, *volume));
            }
            n.children.iter().for_each(|c| visit(c, out));
        }
        let mut out = Vec::new();
        if let Some(t) = &self.tree {
            visit(t, &mut out);
        }
        out
    }

    /// After a build: each video view has its player (of its file), which
    /// plays, pauses and goes where its bindings say.
    fn sync_videos(&mut self) {
        for (key, path, playing, position, looping, volume) in self.videos() {
            super::media::VIDEOS.with(|v| {
                let mut v = v.borrow_mut();
                if v.get(&key).is_none_or(|(p, _)| *p != path) {
                    match super::media::Player::open(&path, true) {
                        Some(p) => {
                            v.insert(key, (path.clone(), p));
                        }
                        None => {
                            v.remove(&key);
                            return;
                        }
                    }
                }
                let Some((_, p)) = v.get_mut(&key) else { return };
                p.looping = looping;
                p.set_volume(volume);
                if let Some(cb) = &position {
                    let want = unsafe { *(cb.locate() as *const f64) };
                    if (want - p.told).abs() > 0.01 {
                        p.seek(want);
                        p.told = want;
                    }
                }
                if let Some(cb) = &playing {
                    let want = unsafe { *cb.locate() } & 1 != 0;
                    if want != p.playing() {
                        if want {
                            p.play();
                        } else {
                            p.pause();
                        }
                    }
                }
            });
        }
        super::media::keep_only(&self.keys);
    }

    /// As time goes by: each video's newest picture, and its bindings told
    /// where it is (and that it stopped, at its end). Whether any plays, and
    /// whether the app is to be built again (its time or bindings changed).
    fn tick_videos(&mut self) -> (bool, bool) {
        let (mut playing_any, mut changed) = (false, false);
        let mut fresh = false;
        for (key, _, playing, position, _, _) in self.videos() {
            super::media::VIDEOS.with(|v| {
                let mut v = v.borrow_mut();
                let Some((_, p)) = v.get_mut(&key) else { return };
                let now = p.tick();
                playing_any |= now;
                fresh |= p.take_picture();
                // It came to its end (and doesn't loop): `playing` says so.
                if let (Some(cb), true, false) = (&playing, p.was_playing, now) {
                    if unsafe { *cb.locate() } & 1 != 0 {
                        unsafe { *cb.locate() = 0 };
                        changed = true;
                    }
                }
                p.was_playing = now;
                let at = p.position();
                if let Some(cb) = &position {
                    if (at - p.told).abs() >= 0.25 || (!now && (at - p.told).abs() > 0.01) {
                        unsafe { *(cb.locate() as *mut f64) = at };
                        p.told = at;
                        changed = true;
                    }
                }
                // (Its time shows the seconds: laid out again when they change.)
                if at.floor() as i64 != p.shown {
                    p.shown = at.floor() as i64;
                    changed = true;
                }
            });
        }
        if changed {
            self.dirty = true;
        }
        (playing_any || fresh, changed)
    }

    // ===== rich text editors =====

    /// A rich text editor's text changed: the program is told (with the
    /// text's JSON), and the step before it can be undone.
    fn rich_commit(&mut self, key: u64, doc: super::rich::Doc, before: (super::rich::Doc, usize, usize), typing: bool) {
        use super::rich::{HISTORY, TYPED};
        let coalesce = typing && TYPED.with(|t| t.get()) == Some(key);
        HISTORY.with(|h| {
            let mut h = h.borrow_mut();
            let steps = h.entry(key).or_default();
            if !coalesce {
                steps.0.push(before);
            }
            steps.1.clear();
        });
        TYPED.with(|t| t.set(typing.then_some(key)));
        self.rich_set(key, doc);
    }

    /// Puts a new text in a rich text editor, and tells the program.
    fn rich_set(&mut self, key: u64, doc: super::rich::Doc) {
        let json = doc.to_json();
        let mut told = None;
        if let Some(Kind::RichEditor { doc: d, on_change, .. }) = self.tree.as_mut().and_then(|t| t.find_mut(key)).map(|n| &mut n.kind) {
            *d = Rc::new(doc);
            told = Some(on_change.clone());
        }
        if let Some(cb) = told {
            cb.call_with(crate::str_new(&json) as *mut u8);
        }
        self.ui.last_edited = Some(key);
        self.dirty = true;
    }

    /// Undo (or redo) in a rich text editor.
    fn rich_undo(&mut self, key: u64, redo: bool) {
        use super::rich::HISTORY;
        let Some(Kind::RichEditor { doc, .. }) = self.node(key).map(|n| &n.kind) else { return };
        let (a, f) = self.ui.editing.as_ref().filter(|e| e.key == key).map_or((doc.text.len(), doc.text.len()), |e| (e.anchor, e.focus));
        let now = ((**doc).clone(), a, f);
        let step = HISTORY.with(|h| {
            let mut h = h.borrow_mut();
            let steps = h.entry(key).or_default();
            let (from, to) = if redo { (&mut steps.1, &mut steps.0) } else { (&mut steps.0, &mut steps.1) };
            let step = from.pop()?;
            to.push(now);
            Some(step)
        });
        super::rich::TYPED.with(|t| t.set(None));
        let Some((doc, anchor, focus)) = step else { return };
        self.rich_set(key, doc);
        self.ui.editing = Some(Editing { key, anchor, focus, ..Default::default() });
    }

    /// Changes the style of what's selected in a rich text editor (with
    /// nothing selected: of what's typed next), and goes back to typing in it.
    fn rich_command(&mut self, key: u64, command: &super::rich::Command) {
        let Some(n) = self.node(key) else { return };
        let Kind::RichEditor { doc, .. } = &n.kind else { return };
        let (doc, base) = ((**doc).clone(), n.layout.style.size);
        let len = doc.text.len();
        let fix = |i: usize| {
            let mut i = i.min(len);
            while !doc.text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let (anchor, focus) = match self.ui.editing.as_ref().filter(|e| e.key == key) {
            Some(e) => (e.anchor, e.focus),
            None => super::rich::SELECTED.with(|s| s.borrow().get(&key).copied()).unwrap_or((len, len)),
        };
        let (anchor, focus) = (fix(anchor), fix(focus));
        let selection = anchor.min(focus)..anchor.max(focus);
        if let Some(d) = super::rich::apply(key, &doc, selection, command, base) {
            self.rich_commit(key, d, (doc, anchor, focus), false);
        }
        self.ui.editing = Some(Editing { key, anchor, focus, ..Default::default() });
        self.ui.focused = None;
        self.dirty = true;
    }

    /// What toolbars asked of their editors (by id), for those in this window.
    fn rich_commands(&mut self, commands: &[(i64, super::rich::Command)]) {
        fn find(n: &Node, id: i64) -> Option<u64> {
            match n.kind {
                Kind::RichEditor { id: i, .. } if i == id => Some(n.key),
                _ => n.children.iter().find_map(|c| find(c, id)),
            }
        }
        for (id, command) in commands {
            if let Some(key) = self.tree.as_ref().and_then(|t| find(t, *id)) {
                self.rich_command(key, command);
            }
        }
    }

    /// Keeps where each rich text editor's selection is, and tells its
    /// toolbar what's selected (building again when that changed).
    fn rich_marks(&mut self) {
        fn editors(n: &Node, out: &mut Vec<(u64, i64, Rc<super::rich::Doc>)>) {
            if let Kind::RichEditor { id, doc, .. } = &n.kind {
                out.push((n.key, *id, doc.clone()));
            }
            for c in &n.children {
                editors(c, out);
            }
        }
        let mut found = Vec::new();
        if let Some(t) = &self.tree {
            editors(t, &mut found);
        }
        for (key, id, doc) in found {
            let selected = match self.ui.editing.as_ref().filter(|e| e.key == key) {
                Some(e) => {
                    super::rich::SELECTED.with(|s| s.borrow_mut().insert(key, (e.anchor, e.focus)));
                    (e.anchor, e.focus)
                }
                None => super::rich::SELECTED.with(|s| s.borrow().get(&key).copied()).unwrap_or((0, 0)),
            };
            let len = doc.text.len();
            let (a, f) = (selected.0.min(len), selected.1.min(len));
            if !doc.text.is_char_boundary(a) || !doc.text.is_char_boundary(f) {
                continue;
            }
            let bits = super::rich::marks_bits(key, &doc, a.min(f)..a.max(f));
            let changed = super::rich::MARKS.with(|m| m.borrow_mut().insert(id, bits) != Some(bits));
            if changed {
                self.dirty = true;
            }
        }
    }

    /// Starts editing a table's cell: a text field takes its place, with
    /// the text it showed (its first `Text`'s).
    fn edit_cell(&mut self, table: u64, row: usize, column: usize) {
        fn first_text(n: &Node) -> Option<String> {
            match &n.kind {
                Kind::Text(s) => Some(s.clone()),
                _ => n.children.iter().find_map(first_text),
            }
        }
        fn find_row(n: &Node, table: u64, index: usize) -> Option<&Node> {
            match &n.kind {
                Kind::TableRow(r) if r.table == table && r.index == index => Some(n),
                _ => n.children.iter().find_map(|c| find_row(c, table, index)),
            }
        }
        let text = self.node(table).and_then(|t| find_row(t, table, row)).and_then(|r| r.children.get(column)).and_then(first_text);
        super::table::begin_edit(table, row, column, &text.unwrap_or_default());
        self.ui.focused = None;
        self.dirty = true;
    }

    /// Once the cell being edited is left (Enter, a click elsewhere, Tab):
    /// the column's `onEdit` gets what was typed, unless Escape dropped it.
    /// Tab goes on to edit the next cell.
    fn finish_cell_edit(&mut self) {
        let Some((table, row, column, field, cancel, next)) =
            super::table::CELL.with(|c| c.borrow().as_ref().map(|e| (e.table, e.row, e.column, e.field, e.cancel, e.next)))
        else {
            return;
        };
        if self.node(table).is_none() {
            return;
        }
        let editing = field.is_some() && self.ui.editing.as_ref().map(|e| e.key) == field;
        if editing {
            super::table::CELL.with(|c| c.borrow_mut().as_mut().map(|e| e.seen = true));
            return;
        }
        let seen = super::table::CELL.with(|c| c.borrow().as_ref().is_some_and(|e| e.seen));
        if !seen {
            return;
        }
        super::table::CELL.with(|c| c.borrow_mut().take());
        if !cancel {
            let on_edit = self.table_parts(table).and_then(|(s, _)| s.columns.get(column).and_then(|c| c.on_edit.clone()));
            if let Some(cb) = on_edit {
                cb.call_with_int_ptr(row as i64, crate::str_new(&super::table::typed()) as *mut u8);
            }
        }
        self.dirty = true;
        if let Some((row, column)) = next {
            self.edit_cell(table, row, column);
            self.select_row(table, row, false, false);
        } else {
            // The keyboard goes back to the table.
            self.ui.focused = Some(table);
        }
    }

    /// The cell Tab (or Shift+Tab: `back`) goes to from one being edited:
    /// the next one that's edited, on to the next row.
    fn next_cell(&self, table: u64, row: usize, column: usize, back: bool) -> Option<(usize, usize)> {
        let (spec, _) = self.table_parts(table)?;
        let edited: Vec<usize> = (0..spec.columns.len()).filter(|&c| spec.columns[c].on_edit.is_some()).collect();
        let at = row * spec.columns.len() + column;
        let cells = (0..spec.count).flat_map(|r| edited.iter().map(move |&c| (r, c)));
        if back {
            cells.filter(|&(r, c)| r * spec.columns.len() + c < at).last()
        } else {
            cells.into_iter().find(|&(r, c)| r * spec.columns.len() + c > at)
        }
    }

    /// `onOpen` with a row (of a tree, with its node's path).
    fn open_row(&mut self, table: u64, row: usize) {
        let Some((spec, _)) = self.table_parts(table) else { return };
        let Some(open) = &spec.on_open else { return };
        if spec.tree.is_some() {
            let path = super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.get(row)).map(|p| p.0.clone()));
            if let Some(path) = path {
                open.call_with(super::table::ints(&path) as *mut u8);
            }
        } else if row < spec.count {
            open.call_with_int(row as i64);
        }
        self.dirty = true;
    }

    /// Opens a tree's node (by its place among those that show), or closes
    /// it; `open` says which (none: the other way from how it is).
    fn toggle_node(&mut self, table: u64, row: usize, open: Option<bool>) {
        let Some(node) = super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.get(row)).cloned()) else { return };
        let (path, _, has) = node;
        if !has {
            return;
        }
        super::table::OPEN.with(|o| {
            let mut o = o.borrow_mut();
            let set = o.entry(table).or_default();
            let now = set.contains(&path);
            if open.unwrap_or(!now) {
                set.insert(path);
            } else {
                set.remove(&path);
            }
        });
        self.dirty = true;
    }

    /// The keys on a table the keyboard is on: the arrows (with Shift, to
    /// select more), Home, End, Page Up and Down, Enter to open; in a tree,
    /// right and left open and close (or go to the first child, and the parent).
    fn table_key(&mut self, table: u64, name: &str) -> bool {
        let Some((spec, scroll)) = self.table_parts(table) else { return false };
        let tree = spec.tree.is_some();
        let selected = spec.selection.as_ref().map(super::table::read_ints).unwrap_or_default();
        let current = match self.ui.table_at.get(&table) {
            Some(&(_, at)) => Some(at),
            None if tree => {
                super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.iter().position(|p| p.0 == selected)))
            }
            None => selected.first().map(|&r| r.max(0) as usize),
        };
        let page = scroll.and_then(|s| self.node(s)).map_or(10, |n| ((n.layout.rect.h / spec.row_height).floor() as usize).max(1));
        let last = spec.count.saturating_sub(1);
        let shift = self.ui.key_shift;
        let to = match (name, current) {
            ("down", None) | ("up", None) | ("home", _) => Some(0),
            ("end", _) => Some(last),
            ("down", Some(c)) => Some((c + 1).min(last)),
            ("up", Some(c)) => Some(c.saturating_sub(1)),
            ("pagedown", c) => Some((c.unwrap_or(0) + page).min(last)),
            ("pageup", c) => Some(c.unwrap_or(0).saturating_sub(page)),
            ("enter", Some(c)) => {
                self.ui.focus_ring = true;
                // Without `onOpen`, Enter edits the row's first cell that's edited.
                match spec.columns.iter().position(|c| c.on_edit.is_some()).filter(|_| spec.on_open.is_none()) {
                    Some(column) => self.edit_cell(table, c, column),
                    None => self.open_row(table, c),
                }
                return true;
            }
            ("right" | "left", Some(c)) if tree => {
                self.ui.focus_ring = true;
                let node = super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.get(c)).cloned());
                let Some((path, _, has)) = node else { return true };
                let open = super::table::OPEN.with(|o| o.borrow().get(&table).is_some_and(|s| s.contains(&path)));
                match (name, has, open) {
                    ("right", true, false) => self.toggle_node(table, c, Some(true)),
                    ("right", true, true) => self.select_row(table, c + 1, false, false),
                    ("left", true, true) => self.toggle_node(table, c, Some(false)),
                    ("left", _, _) if path.len() > 1 => {
                        let parent = &path[..path.len() - 1];
                        let at = super::table::VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.iter().position(|p| p.0 == parent)));
                        if let Some(at) = at {
                            self.select_row(table, at, false, false);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            _ => None,
        };
        let Some(to) = to else { return false };
        if spec.count > 0 {
            self.ui.focus_ring = true;
            self.select_row(table, to, shift, false);
        }
        true
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
        // An open menu picker's list closes at any click (on it, once its option is chosen).
        let menu_open = super::controls::OPEN_MENU.get();
        if menu_open.is_some() && !matches!(hit, Some(Hit::Segment(k, _) | Hit::MenuPicker(k)) if Some(k) == menu_open) {
            super::controls::OPEN_MENU.set(None);
            self.dirty = true;
        }
        // An open ribbon group's panel closes at a click outside it, and
        // once one of its items is chosen (other views in it keep it open).
        if let Some(open) = super::controls::OPEN_GROUP.get() {
            let inside = self.node(open).is_some_and(|n| n.layout.panel.contains(x, y));
            let item = matches!(hit, Some(Hit::Button(k) | Hit::Toggle(k) | Hit::RibbonMenu(k))
                if self.node(k).is_some_and(|n| matches!(n.kind, Kind::RibbonItem { .. })));
            let button = matches!(hit, Some(Hit::RibbonGroup(k)) if k == open);
            if !button && (!inside || item) {
                super::controls::OPEN_GROUP.set(None);
                self.dirty = true;
            }
        }
        let on_palette = self.close_palette_unless(&hit);
        // The keyboard goes to the control clicked.
        self.ui.focused = match &hit {
            Some(Hit::Slider(k) | Hit::Stepper(k, _) | Hit::ColorPicker(k) | Hit::Disclosure(k) | Hit::MenuPicker(k)) => Some(*k),
            Some(Hit::TableHeader(k, _) | Hit::TableResize(k, _) | Hit::TableRow(k, _) | Hit::TreeArrow(k, _)) => Some(*k),
            _ if on_palette => self.ui.focused,
            _ => None,
        };
        self.ui.focus_ring = false;
        // A click in a view with `.onKey` or `.onKeyEvent` gives it the keyboard.
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
            // A column's title sorts by it; again, the other way.
            (Hit::TableHeader(_, column), Kind::Table(spec)) => {
                if let Some((by, up)) = &spec.sort {
                    unsafe {
                        let (by, up) = (by.locate() as *mut i64, up.locate());
                        if *by == *column as i64 {
                            *up = (*up & 1) ^ 1;
                        } else {
                            *by = *column as i64;
                            *up = 1;
                        }
                    }
                    if let Some(sorted) = spec.on_sort.clone() {
                        sorted.call();
                    }
                }
            }
            (Hit::TableRow(table, row), Kind::Table(_)) => {
                let (table, row) = (*table, *row);
                let (shift, toggle) = self.ui.click_mods;
                let now = std::time::Instant::now();
                let again = self.ui.last_row_click.is_some_and(|(t, k, r)| k == table && r == row && now.duration_since(t).as_millis() < 450);
                self.ui.last_row_click = if again { None } else { Some((now, table, row)) };
                self.select_row(table, row, shift, toggle);
                if again {
                    // In a column whose cells are edited: edit the cell.
                    let column = self.node(table).and_then(|n| n.layout.tabs.iter().position(|t| x >= t.rect.x && x < t.rect.x + t.rect.w));
                    match column.filter(|&c| self.table_parts(table).is_some_and(|(s, _)| s.columns.get(c).is_some_and(|c| c.on_edit.is_some()))) {
                        Some(column) => self.edit_cell(table, row, column),
                        None => self.open_row(table, row),
                    }
                }
            }
            (Hit::TreeArrow(table, row), Kind::Table(_)) => {
                let (table, row) = (*table, *row);
                self.toggle_node(table, row, None);
            }
            (Hit::Button(_), Kind::Button { action, .. } | Kind::RibbonItem { role: super::RibbonRole::Button(action), .. }) => {
                let action = action.clone();
                action.call();
            }
            (Hit::Toggle(_), Kind::RibbonItem { role: super::RibbonRole::Toggle(is_on, on_change), .. }) => {
                let p = is_on.locate();
                unsafe { *p = (*p & 1) ^ 1 };
                if let Some(cb) = on_change.clone() {
                    cb.call();
                }
            }
            (Hit::RibbonGroup(key), _) => {
                let key = *key;
                let open = super::controls::OPEN_GROUP.get() == Some(key);
                super::controls::OPEN_GROUP.set((!open).then_some(key));
            }
            (Hit::RibbonMenu(key), _) => {
                // Below the item.
                let (key, r) = (*key, node.layout.rect);
                self.open_menu(MenuSource::Item(key), (r.x, r.y + r.h + 1.0));
                return;
            }
            (Hit::Tab(_, i), Kind::Ribbon { selection }) => {
                if let Some(Kind::RibbonTab { title }) = node.children.get(*i).map(|c| &c.kind) {
                    let slot = selection.locate() as *mut *mut Str;
                    unsafe {
                        let old = *slot;
                        *slot = crate::str_new(title);
                        crate::tsl_release(old as *mut Header);
                    }
                }
            }
            (Hit::Toggle(_), Kind::Toggle { is_on, on_change, .. }) => {
                let p = is_on.locate();
                unsafe { *p = (*p & 1) ^ 1 };
                if let Some(cb) = on_change.clone() {
                    cb.call();
                }
            }
            // A click in a code editor's margin, when that sets breakpoints.
            (Hit::Field(_), Kind::CodeEditor { on_breakpoint: Some(toggle), .. })
                if x < node.layout.rect.inset_by(node.mods.padding).x + node.layout.gutter =>
            {
                let toggle = toggle.clone();
                let at = y - render::code_origin(node).1 + self.ui.scroll.get(&node.key).copied().unwrap_or(0.0);
                let lines = node.layout.text.as_ref().map(|t| t.lines.iter().map(|l| (l.top, l.bottom)).collect::<Vec<_>>());
                if let Some(line) = lines.unwrap_or_default().iter().position(|(top, bottom)| at >= *top && at < *bottom) {
                    toggle.call_with_int(line as i64 + 1);
                    self.dirty = true;
                }
            }
            (Hit::Field(key), Kind::TextField { text, .. } | Kind::CodeEditor { text, .. }) => {
                if self.ui.editing.as_ref().is_none_or(|e| e.key != *key) {
                    self.ui.editing = Some(Editing::new(*key, &read_string(text)));
                }
            }
            // A search field's clear button: no text, and on to typing.
            (Hit::SearchClear(key), Kind::TextField { text, on_change, .. }) => {
                let key = *key;
                let had = !read_string(text).is_empty();
                set_string(text, "");
                let told = on_change.clone().filter(|_| had);
                self.ui.editing = Some(Editing::new(key, ""));
                self.dirty = true;
                if let Some(cb) = told {
                    cb.call();
                }
            }
            // Selectable text: the caret goes where it's clicked (unseen).
            (Hit::Field(key), Kind::Text(_) | Kind::RichEditor { .. }) => {
                let key = *key;
                if self.ui.editing.as_ref().is_none_or(|e| e.key != key) {
                    self.press_field(key, x, y, false, 1);
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
            // A video plays or pauses, or goes where its track is clicked.
            (Hit::Video(key, part), Kind::Video { playing, position, .. }) => {
                let (key, part) = (*key, *part);
                let (playing, position) = (playing.clone(), position.clone());
                let track = render::video_parts(node).2;
                super::media::VIDEOS.with(|v| {
                    let mut v = v.borrow_mut();
                    let Some((_, p)) = v.get_mut(&key) else { return };
                    if part == 1 {
                        let to = ((x - track.x) / track.w.max(1.0)).clamp(0.0, 1.0) * p.duration();
                        p.seek(to);
                        p.told = to;
                        if let Some(cb) = &position {
                            unsafe { *(cb.locate() as *mut f64) = to };
                        }
                    } else {
                        let now = !p.playing();
                        if now {
                            p.play();
                        } else {
                            p.pause();
                        }
                        if let Some(cb) = &playing {
                            unsafe { *cb.locate() = now as u8 };
                        }
                    }
                });
                self.dirty = true;
            }
            (Hit::MenuPicker(key), Kind::Picker { .. }) => {
                let key = *key;
                let open = super::controls::OPEN_MENU.get() == Some(key);
                super::controls::OPEN_MENU.set((!open).then_some(key));
                self.ui.focused = Some(key);
            }
            (Hit::Segment(_, i), Kind::Picker { selection, style, on_change }) => {
                if *style == 2 {
                    super::controls::OPEN_MENU.set(None);
                }
                if let Some(tag) = node.children.get(*i).and_then(|c| c.mods.tag.as_ref()) {
                    // (Choosing the option that's chosen changes nothing.)
                    let same = tag.equals(selection.locate());
                    unsafe { tag.store_to(selection.locate()) };
                    if let Some(cb) = on_change.clone().filter(|_| !same) {
                        cb.call();
                    }
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
    /// Moves a split view's divider: its first pane is to be `size` (no
    /// pane less than its minimum).
    fn set_split(&mut self, key: u64, size: f64) {
        let Some(n) = self.node(key) else { return };
        let Kind::Split { vertical, size: bound } = &n.kind else { return };
        let c = n.layout.rect.inset_by(n.mods.padding);
        let along = if *vertical { c.h } else { c.w };
        // Where it may go, as layout has it: the size tried, then clamped.
        let clamped = match bound {
            Some(cb) => {
                super::controls::write_number(cb, false, size);
                super::layout::split_first(n, along)
            }
            None => {
                super::layout::SPLITS.with(|s| s.borrow_mut().insert(key, size));
                super::layout::split_first(n, along)
            }
        }
        .round();
        match bound {
            Some(cb) => {
                super::controls::write_number(cb, false, clamped);
            }
            None => {
                super::layout::SPLITS.with(|s| s.borrow_mut().insert(key, clamped));
            }
        }
        self.dirty = true;
    }

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
        if let Kind::Table(_) = n.kind {
            return self.table_key(key, name);
        }
        // Using the keyboard on it shows which control it's on.
        let took = matches!(
                (&n.kind, name),
                (Kind::Slider { .. } | Kind::Stepper { .. }, "right" | "up" | "left" | "down")
                    | (Kind::ColorPicker { .. } | Kind::Button { .. } | Kind::Toggle { .. }, "enter" | "space")
                    | (Kind::Disclosure { .. }, "enter" | "space" | "left" | "right")
                    | (Kind::Picker { .. } | Kind::TabView { .. }, "left" | "right" | "up" | "down")
                    | (Kind::Picker { style: 2, .. }, "enter" | "space" | "escape")
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
            // Enter or Space presses a button, and turns a toggle.
            (Kind::Button { action, .. }, "enter" | "space") => {
                let action = action.clone();
                action.call();
                self.dirty = true;
                true
            }
            (Kind::Toggle { is_on, on_change, .. }, "enter" | "space") => {
                let p = is_on.locate();
                unsafe { *p = (*p & 1) ^ 1 };
                let told = on_change.clone();
                self.dirty = true;
                if let Some(cb) = told {
                    cb.call();
                }
                true
            }
            // Enter or Space opens a menu picker's list (or closes it); Escape closes it.
            (Kind::Picker { style: 2, .. }, "enter" | "space" | "escape") => {
                let open = super::controls::OPEN_MENU.get() == Some(key);
                super::controls::OPEN_MENU.set((!open && name != "escape").then_some(key));
                self.dirty = true;
                true
            }
            // The arrows choose the option, or the tab, before or after.
            (Kind::Picker { selection, .. } | Kind::TabView { selection, .. }, "left" | "right" | "up" | "down") => {
                let current = selection.locate();
                let tagged: Vec<usize> = (0..n.children.len()).filter(|&i| n.children[i].mods.tag.is_some()).collect();
                let at = tagged.iter().position(|&i| n.children[i].mods.tag.as_ref().is_some_and(|t| t.equals(current)));
                let to = match (at, matches!(name, "right" | "down")) {
                    (Some(i), true) => (i + 1).min(tagged.len().saturating_sub(1)),
                    (Some(i), false) => i.saturating_sub(1),
                    (None, _) => 0,
                };
                if let Some(tag) = tagged.get(to).and_then(|&i| n.children[i].mods.tag.as_ref()) {
                    let same = tag.equals(current);
                    unsafe { tag.store_to(selection.locate()) };
                    // (A `TabView` has no `onChange`.)
                    let told = match &n.kind {
                        Kind::Picker { on_change, .. } => on_change.clone().filter(|_| !same),
                        _ => None,
                    };
                    self.dirty = true;
                    if let Some(cb) = told {
                        cb.call();
                    }
                }
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
        let Some(layout) = &node.layout.text else { return };
        if let Kind::RichEditor { doc, .. } = &node.kind {
            let doc = (**doc).clone();
            let marks = super::rich::typing_marks(e.key, &doc, e.range());
            let before = (doc.clone(), e.anchor, e.focus);
            let typing = matches!(&change, Edit::Insert(t) if t.chars().all(|c| c.is_alphanumeric() || c == '_'));
            // Copying takes the styles along; pasting text with styles keeps them.
            let range = e.range();
            match change {
                Edit::Copy | Edit::Cut if !range.is_empty() => {
                    let part = doc.slice(range.clone());
                    self.clipboard.set_rich(&part.text, &part.to_html());
                    let key = e.key;
                    if matches!(change, Edit::Cut) {
                        let mut d = doc;
                        d.replace(range.clone(), "", &marks);
                        e.collapse_to(range.start);
                        self.ui.editing = Some(e);
                        self.rich_commit(key, d, before, false);
                    } else {
                        self.ui.editing = Some(e);
                    }
                    return;
                }
                Edit::Paste => {
                    if let Some(html) = self.clipboard.html() {
                        let part = super::rich::Doc::from_html(&html);
                        if !part.text.is_empty() {
                            let mut d = doc;
                            d.splice(range.clone(), &part);
                            e.collapse_to(range.start + part.text.len());
                            let key = e.key;
                            self.ui.editing = Some(e);
                            self.rich_commit(key, d, before, false);
                            return;
                        }
                    }
                }
                _ => {}
            }
            let new = edit::apply(&doc.text, layout, &mut e, change, &mut self.clipboard, true);
            let key = e.key;
            self.ui.editing = Some(e);
            if let Some(new) = new {
                let (start, old_end, new_end) = super::rich::changed(&doc.text, &new);
                let mut d = doc;
                d.replace(start..old_end, &new[start..new_end], &marks);
                self.rich_commit(key, d, before, typing);
            } else {
                self.follow_caret();
            }
            return;
        }
        // Text that's only selected: it's selected and copied, never changed.
        if selectable_text(node) {
            let change = match change {
                Edit::Cut => Edit::Copy,
                Edit::Insert(_) | Edit::Backspace(_) | Edit::Delete(_) | Edit::Paste => return,
                other => other,
            };
            let current = shown_text(node).unwrap_or_default();
            let _ = edit::apply(&current, layout, &mut e, change, &mut self.clipboard, true);
            self.ui.editing = Some(e);
            self.dirty = true;
            return;
        }
        let Some(text) = editable_text(node) else { return };
        let current = read_string(text);
        let typing = matches!(&change, Edit::Insert(t) if t.chars().all(|c| c.is_alphanumeric() || c == '_'));
        let before = edit::Snapshot { text: current.clone(), anchor: e.anchor, focus: e.focus };
        let multiline = in_lines(&node.kind);
        // (What's typed in a secure field stays in it.)
        if matches!(node.kind, Kind::TextField { secure: true, .. }) && matches!(change, Edit::Copy | Edit::Cut) {
            return;
        }
        let on_change = text_on_change(node);
        let mut changed = false;
        if let Some(new) = edit::apply(&current, layout, &mut e, change, &mut self.clipboard, multiline) {
            changed = new != current;
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
        if let Some(cb) = on_change.filter(|_| changed) {
            cb.call();
        }
    }

    /// Undo (or redo) the last change in the view being edited, or, while
    /// nothing is, in the one whose text changed last. That one is then
    /// edited, with the caret where the change was.
    fn undo(&mut self, redo: bool) {
        let Some(key) = self.ui.editing.as_ref().map(|e| e.key).or(self.ui.last_edited) else { return };
        if self.node(key).is_some_and(|n| matches!(n.kind, Kind::RichEditor { .. })) {
            self.rich_undo(key, redo);
            return;
        }
        let Some(text) = self.node(key).and_then(editable_text).cloned() else { return };
        let on_change = self.node(key).and_then(text_on_change);
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
        if let Some(cb) = on_change {
            cb.call();
        }
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

    /// Whether a menu bar item runs on this key (`key` runs it): the app's
    /// own items, and the standard ones that aren't about text (Copy and
    /// the like reach the views, which do them).
    fn bar_takes(&self, name: &str, mods: Mods) -> bool {
        let editing = self.ui.editing.is_some() && mods.primary && !mods.alt && matches!(name, "a" | "c" | "x" | "v" | "z");
        match menu::find_shortcut(&self.bar, name, mods.primary, mods.shift, mods.alt) {
            Some(Entry::Item { command: Command::User(_), .. }) => !editing,
            Some(Entry::Item { command: Command::Std(Std::Quit | Std::Hide | Std::HideOthers | Std::Minimize | Std::CloseWindow), .. }) => true,
            _ => false,
        }
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
        // In a table's cell being edited, Escape drops what's typed, and Tab
        // goes on to the next cell.
        let in_cell = super::table::CELL.with(|c| c.borrow().as_ref().and_then(|c| c.field.map(|f| (f, c.table, c.row, c.column))));
        if let Some((field, table, row, column)) = in_cell.filter(|c| self.ui.editing.as_ref().map(|e| e.key) == Some(c.0)) {
            let _ = field;
            match name {
                "escape" => {
                    super::table::CELL.with(|c| c.borrow_mut().as_mut().map(|e| e.cancel = true));
                    self.ui.editing = None;
                    self.dirty = true;
                    return;
                }
                "tab" => {
                    let next = self.next_cell(table, row, column, mods.shift);
                    super::table::CELL.with(|c| c.borrow_mut().as_mut().map(|e| e.next = next));
                    self.ui.editing = None;
                    self.dirty = true;
                    return;
                }
                _ => {}
            }
        }
        if !mods.primary && !mods.shift && self.completion_key(name) {
            return;
        }
        // In a rich text editor, Cmd+B, Cmd+I and Cmd+U change the style.
        let rich = self.ui.editing.as_ref().map(|e| e.key).filter(|k| self.node(*k).is_some_and(|n| matches!(n.kind, Kind::RichEditor { .. })));
        if let (Some(key), true, false) = (rich, mods.primary, mods.alt) {
            let command = match name {
                "b" => Some(super::rich::Command::Bold),
                "i" => Some(super::rich::Command::Italic),
                "u" => Some(super::rich::Command::Underline),
                _ => None,
            };
            if let Some(c) = command {
                self.rich_command(key, &c);
                return;
            }
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
            // In a field of several lines, Enter is a new line (and
            // Command-Enter, or Control-Enter, submits).
            "enter" if !mods.primary && self.editing_lines() => {
                self.edit(Edit::Insert("\n".into()));
                return;
            }
            // Escape in a search field with text clears it first.
            "escape" if self.ui.editing.as_ref().and_then(|e| self.node(e.key)).is_some_and(|n| render::search_clear(n).is_some()) => {
                let Some(key) = self.ui.editing.as_ref().map(|e| e.key) else { return };
                let mut told = None;
                if let Some(Kind::TextField { text, on_change, .. }) = self.node(key).map(|n| &n.kind) {
                    // (The clear button shows only while there's text.)
                    set_string(text, "");
                    told = on_change.clone();
                }
                self.ui.editing = Some(Editing::new(key, ""));
                self.dirty = true;
                if let Some(cb) = told {
                    cb.call();
                }
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
        let Some(current) = shown_text(node) else { return };
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
            inner.or((matches!(n.kind, Kind::Scroll(_)) || in_lines(&n.kind)).then_some(n.key))
        }
        self.point_root(x, y).and_then(|t| find(t, x, y))
    }

    /// What the wheel turns at a point: the innermost scroll view or code
    /// editor there that scrolls the way it turns (by `dx`, `dy`), or view
    /// with `.onPointer` (true: it is told instead).
    fn wheel_target(&self, x: f64, y: f64, dx: f64, dy: f64) -> Option<(u64, bool)> {
        fn find(n: &Node, x: f64, y: f64, dx: f64, dy: f64) -> Option<(u64, bool)> {
            if !n.layout.rect.contains(x, y) || n.mods.hidden {
                return None;
            }
            let inner = n.visible_children().iter().rev().find_map(|c| find(c, x, y, dx, dy));
            inner.or_else(|| {
                if n.mods.on_pointer.is_some() && !n.layout.style.disabled {
                    return Some((n.key, true));
                }
                let turns = match n.kind {
                    // (Sideways only: it takes a wheel that turns down too,
                    // when no scroll view around it goes that way.)
                    Kind::Scroll(axes) => {
                        let (down, across) = super::scroll_axes(axes);
                        (down && dy != 0.0) || (across && dx != 0.0) || (dx == 0.0 && dy == 0.0)
                    }
                    _ => in_lines(&n.kind),
                };
                turns.then_some((n.key, false))
            })
        }
        let root = self.point_root(x, y)?;
        find(root, x, y, dx, dy).or_else(|| {
            // A wheel turned down over a view that only scrolls across.
            fn across(n: &Node, x: f64, y: f64) -> Option<u64> {
                if !n.layout.rect.contains(x, y) || n.mods.hidden {
                    return None;
                }
                let inner = n.visible_children().iter().rev().find_map(|c| across(c, x, y));
                inner.or(matches!(n.kind, Kind::Scroll(1)).then_some(n.key))
            }
            across(root, x, y).map(|k| (k, false))
        })
    }

    /// Scrolls the view under (x, y) by (dx, dy): as far as its content
    /// goes. A view with `.onPointer` is told of the scroll instead.
    fn scroll(&mut self, x: f64, y: f64, dx: f64, dy: f64, mods: Mods) {
        let Some((key, pointer)) = self.wheel_target(x, y, dx, dy) else { return };
        if pointer {
            self.pointer(key, SCROLL, (x, y), (dx, dy), 0, 0, mods);
            return;
        }
        let Some(n) = self.node(key) else { return };
        let content = n.layout.rect.inset_by(n.mods.padding);
        let mut max_y = (n.layout.content_h - content.h).max(0.0);
        let (mut dx, mut dy) = (dx, dy);
        let max_x = match &n.kind {
            Kind::CodeEditor { .. } => {
                let width = n.layout.text.as_ref().map_or(0.0, |t| t.width()) + 2.0 * render::CODE_PAD_X;
                (width - (content.w - n.layout.gutter)).max(0.0)
            }
            Kind::Scroll(axes) => {
                let (down, across) = super::scroll_axes(*axes);
                if !down {
                    // Sideways only: a wheel turned down moves it across.
                    if dx == 0.0 {
                        (dx, dy) = (dy, 0.0);
                    }
                    max_y = 0.0;
                }
                if across { (n.layout.content_w - content.w).max(0.0) } else { 0.0 }
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
        // `named`: a view's accessibility label or tooltip counts too (only
        // when no view says the label: those come first).
        fn visit(n: &Node, label: &str, named: bool) -> Option<(f64, f64)> {
            // A table's column, by its title.
            if let (Kind::Table(spec), false) = (&n.kind, n.mods.hidden) {
                let found = spec.columns.iter().position(|c| c.title == label).and_then(|i| n.layout.tabs.get(i)).filter(|t| t.rect.h > 0.0);
                if let Some(t) = found {
                    return Some((t.rect.x + t.rect.w / 2.0, t.rect.y + t.rect.h / 2.0));
                }
            }
            if let Kind::TabView { .. } = n.kind {
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    if n.children.get(i).and_then(|c| c.mods.tab_title.as_deref()) == Some(label) {
                        let r = tab.rect;
                        return Some((r.x + r.w / 2.0 - 10.0, r.y + r.h / 2.0));
                    }
                }
                return n.children.get(n.selected_tab()).and_then(|c| visit(c, label, named));
            }
            // A ribbon's tab by its title.
            if let (Kind::Ribbon { .. }, false) = (&n.kind, n.mods.hidden) {
                for (tab, c) in n.layout.tabs.iter().zip(&n.children) {
                    if matches!(&c.kind, Kind::RibbonTab { title } if title == label) {
                        return Some((tab.rect.x + tab.rect.w / 2.0, tab.rect.y + tab.rect.h / 2.0));
                    }
                }
            }
            // A collapsed ribbon group by its title (its button); its items
            // only while its panel is open.
            if let (Kind::RibbonGroup { title, .. }, true, false) = (&n.kind, n.layout.collapsed, n.mods.hidden) {
                if title == label {
                    let r = n.layout.rect;
                    return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
                }
                if super::controls::OPEN_GROUP.get() != Some(n.key) {
                    return None;
                }
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
                Kind::Text(s) | Kind::Button { label: s, .. } | Kind::Toggle { label: s, .. } | Kind::RibbonItem { label: s, .. } => s == label,
                Kind::TextField { placeholder, .. } => placeholder == label,
                Kind::Svg { icon: Some(name), .. } => *name == label,
                Kind::Swatch(c) => super::controls::hex(*c) == label,
                _ => false,
            };
            // Any view by what it's called to a screen reader, or by its
            // tooltip (a toolbar of pictures has nothing else).
            let matches = matches || named && (n.mods.access_label.as_deref() == Some(label) || n.mods.tooltip.as_deref() == Some(label));
            if matches && !n.mods.hidden {
                let r = n.layout.rect;
                return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
            }
            n.visible_children().iter().find_map(|c| visit(c, label, named))
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
        for named in [false, true] {
            for p in self.popovers().into_iter().rev() {
                if let Some(found) = visit(p, label, named) {
                    return Some(found);
                }
            }
            if let Some(found) = visit(tree, label, named) {
                return Some(found);
            }
        }
        None
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
                Kind::Text(s) => {
                    // Cut short by its line limit: what shows; selected: what is.
                    let shown = n.layout.cut.then(|| text_shown(n, s));
                    let mut out = match &shown {
                        Some(shown) => format!("Text {s:?} shows {shown:?}"),
                        None => format!("Text {s:?}"),
                    };
                    if let Some(e) = ui.editing.as_ref().filter(|e| e.key == n.key) {
                        let text = shown.unwrap_or_else(|| s.clone());
                        out += &format!(" selected {:?}", text.get(e.range()).unwrap_or(""));
                    }
                    out
                }
                Kind::Button { label, .. } => format!("Button {label:?}"),
                Kind::TextField { text, placeholder, secure, search, .. } => {
                    let composing = ui.composing_in(n.key).map(|(_, c)| format!(" composing {:?}", c.text)).unwrap_or_default();
                    let mut value = read_string(text);
                    if *secure {
                        value = value.chars().map(|_| '•').collect();
                    }
                    let kind = if *search { "SearchField" } else { "TextField" };
                    format!("{kind} {placeholder:?} = {value:?}{composing}")
                }
                Kind::Toggle { label, is_on, .. } => format!("Toggle {label:?} = {}", unsafe { *is_on.locate() } & 1 != 0),
                Kind::RichEditor { doc, .. } => {
                    // Its runs, by the characters they're on, and how they look.
                    let at = |b: usize| doc.text[..b].chars().count();
                    let runs: Vec<String> = doc
                        .runs
                        .iter()
                        .map(|(r, m)| {
                            let mut looks = Vec::new();
                            for (on, name) in [(m.bold, "bold"), (m.italic, "italic"), (m.underline, "underline"), (m.strike, "strikethrough")] {
                                if on {
                                    looks.push(name.to_string());
                                }
                            }
                            if let Some(s) = m.size {
                                looks.push(format!("size {s}"));
                            }
                            if let Some(c) = &m.color {
                                looks.push(format!("color {c}"));
                            }
                            if let Some(f) = &m.font {
                                looks.push(format!("font {f:?}"));
                            }
                            format!("{}..{} {}", at(r.start), at(r.end), looks.join(" "))
                        })
                        .collect();
                    let mut out = format!("RichTextEditor {:?} runs {runs:?}", doc.text);
                    if let Some(e) = ui.editing.as_ref().filter(|e| e.key == n.key) {
                        out += &format!(" selected {}..{}", at(e.range().start.min(doc.text.len())), at(e.range().end.min(doc.text.len())));
                    }
                    out
                }
                Kind::WebView { url, html } => {
                    if url.is_empty() { format!("WebView html {:?}", html.chars().take(40).collect::<String>()) } else { format!("WebView {url:?}") }
                }
                Kind::Video { path, .. } => {
                    let state = super::media::VIDEOS.with(|v| {
                        v.borrow().get(&n.key).map(|(_, p)| format!("{} {:.1}/{:.1}", if p.playing() { "playing" } else { "paused" }, p.position(), p.duration()))
                    });
                    format!("VideoPlayer {path:?} {}", state.unwrap_or_else(|| "can't be played".into()))
                }
                Kind::Picker { style: 1, .. } => "Picker radio".to_string(),
                Kind::Picker { style: 2, .. } => {
                    let open = if super::controls::OPEN_MENU.get() == Some(n.key) { " open" } else { "" };
                    format!("Picker menu{open}")
                }
                Kind::Picker { .. } => "Picker".to_string(),
                Kind::Grid { .. } => {
                    let columns = n.layout.grid.as_ref().map_or(0, |g| g.widths.len());
                    format!("Grid {columns} columns")
                }
                Kind::GridRow => "GridRow".to_string(),
                Kind::VStack(..) => "VStack".to_string(),
                Kind::HStack(..) => "HStack".to_string(),
                Kind::ZStack(_) => "ZStack".to_string(),
                Kind::Scroll(0) => "Scroll".to_string(),
                Kind::Scroll(1) => "Scroll horizontal".to_string(),
                Kind::Scroll(_) => "Scroll both".to_string(),
                Kind::Spacer => "Spacer".to_string(),
                Kind::Image { path, .. } => format!("Image {path:?}"),
                Kind::TabView { .. } => {
                    let titles: Vec<&str> = n.children.iter().map(|c| c.mods.tab_title.as_deref().unwrap_or("")).collect();
                    format!("TabView {titles:?} selected {}", n.selected_tab())
                }
                Kind::Divider => "Divider".to_string(),
                Kind::Split { vertical, .. } => {
                    let c = n.layout.rect.inset_by(n.mods.padding);
                    let first = super::layout::split_first(n, if *vertical { c.h } else { c.w });
                    format!("{} {first}", if *vertical { "VSplit" } else { "HSplit" })
                }
                Kind::Canvas { draw, redraw } => {
                    let c = n.layout.rect.inset_by(n.mods.padding);
                    let drawing = super::canvas::drawing(n.key, draw, *redraw, c.w, c.h);
                    format!("Canvas ({})", drawing.stats.describe())
                }
                Kind::Scene3D { draw, redraw } => {
                    let c = n.layout.rect.inset_by(n.mods.padding);
                    let shot = super::scene3d::shot(n.key, draw, *redraw, c.w, c.h, 1.0, false);
                    format!("Scene3D ({})", shot.describe())
                }
                Kind::Markdown { .. } => {
                    let laid = n.layout.markdown.as_ref();
                    let links: Vec<&str> = laid.iter().flat_map(|l| &l.links).map(|l| l.text.as_str()).collect();
                    format!("Markdown {:?} links {links:?}", laid.map_or("", |l| l.title.as_str()))
                }
                Kind::Svg { icon: Some(name), .. } => format!("Icon {name}"),
                Kind::Svg { .. } => "Svg".to_string(),
                Kind::CodeEditor { text, error_lines, breakpoints, stopped_line, .. } => {
                    // (A debugger's marks, when there are any.)
                    let mut marks = String::new();
                    if !breakpoints.is_empty() {
                        marks += &format!(" breakpoints {breakpoints:?}");
                    }
                    if *stopped_line > 0 {
                        marks += &format!(" stopped {stopped_line}");
                    }
                    format!("CodeEditor errors {error_lines:?}{marks} = {:?}", read_string(text))
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
                Kind::Lazy { count, first, across: false, .. } => {
                    format!("LazyVStack {count} rows, built {}..{}", first, first + n.children.len())
                }
                Kind::Lazy { count, first, .. } => {
                    format!("LazyHStack {count} columns, built {}..{}", first, first + n.children.len())
                }
                Kind::Table(spec) => {
                    let mut out = match spec.tree {
                        Some(_) => format!("Tree {} rows", spec.count),
                        None => {
                            let titles: Vec<&str> = spec.columns.iter().map(|c| c.title.as_str()).collect();
                            format!("Table {titles:?} {} rows", spec.count)
                        }
                    };
                    if let Some((column, up)) = &spec.sort {
                        let (c, u) = unsafe { (*(column.locate() as *const i64), *up.locate() & 1 != 0) };
                        if let Some(title) = usize::try_from(c).ok().and_then(|c| spec.columns.get(c)) {
                            out += &format!(", sorted by {:?} {}", title.title, if u { "up" } else { "down" });
                        }
                    }
                    if let Some(s) = &spec.selection {
                        out += &format!(", selected {:?}", super::table::read_ints(s));
                    }
                    out
                }
                Kind::TableRow(row) => {
                    let mut out = match (&row.path, row.expandable) {
                        (Some(p), true) => format!("Row {p:?} {}", if row.expanded { "open" } else { "closed" }),
                        (Some(p), false) => format!("Row {p:?}"),
                        (None, _) => format!("Row {}", row.index),
                    };
                    if row.selected {
                        out += " selected";
                    }
                    out
                }
                Kind::MenuItem { label, .. } => format!("MenuItem {label:?}"),
                Kind::MenuDivider => "MenuDivider".to_string(),
                Kind::Menu { title } => format!("Menu {title:?}"),
                Kind::MenuList => "MenuList".to_string(),
                Kind::Popover { edge, .. } => if *edge == super::SHEET { "Sheet" } else { "Popover" }.to_string(),
                Kind::Window { title, .. } => format!("Window {title:?}"),
                Kind::StatusBar => "StatusBar".to_string(),
                Kind::Ribbon { .. } => {
                    let titles: Vec<&str> = n.children.iter().filter_map(|c| match &c.kind {
                        Kind::RibbonTab { title } => Some(title.as_str()),
                        _ => None,
                    }).collect();
                    format!("Ribbon {titles:?} selected {:?}", titles.get(n.selected_ribbon_tab()).copied().unwrap_or(""))
                }
                Kind::RibbonTab { title } => format!("RibbonTab {title:?}"),
                Kind::RibbonGroup { title, .. } => {
                    let shown = if n.layout.collapsed {
                        if super::controls::OPEN_GROUP.get() == Some(n.key) { " collapsed open" } else { " collapsed" }
                    } else if n.layout.compact {
                        " small"
                    } else {
                        ""
                    };
                    format!("RibbonGroup {title:?}{shown}")
                }
                Kind::RibbonItem { label, large, role, .. } => {
                    let size = if *large && !n.layout.compact { " large" } else { "" };
                    match role {
                        super::RibbonRole::Button(_) => format!("RibbonButton {label:?}{size}"),
                        super::RibbonRole::Toggle(is_on, _) => {
                            let on = unsafe { *is_on.locate() } & 1 != 0;
                            format!("RibbonToggle {label:?}{size} = {on}")
                        }
                        super::RibbonRole::Menu => format!("RibbonMenu {label:?}{size} [{}]", menu::describe_inline(&menu::entries(n))),
                    }
                }
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
            // A tab view shows only its selected page (and a ribbon its
            // selected tab; a ribbon's menu shows its items above).
            if let Kind::TabView { .. } | Kind::Ribbon { .. } | Kind::RibbonItem { .. } = n.kind {
                for c in n.visible_children() {
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
            // (The main window, hidden while the app goes on in its tray icon.)
            if self.main_hidden && self.current == 0 {
                out = out.replacen("Root", "Root (hidden)", 1);
            }
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
                Kind::TextField { search: true, .. } => "SearchField",
                Kind::TextField { .. } => "TextField",
                Kind::Toggle { .. } => "Toggle",
                Kind::RichEditor { .. } => "RichTextEditor",
                Kind::Video { .. } => "VideoPlayer",
                Kind::WebView { .. } => "WebView",
                Kind::Picker { .. } => "Picker",
                Kind::VStack(..) => "VStack",
                Kind::HStack(..) => "HStack",
                Kind::ZStack(_) => "ZStack",
                Kind::Scroll(_) => "Scroll",
                Kind::Spacer => "Spacer",
                Kind::Image { .. } => "Image",
                Kind::TabView { .. } => "TabView",
                Kind::Divider => "Divider",
                Kind::Split { vertical: false, .. } => "HSplit",
                Kind::Split { vertical: true, .. } => "VSplit",
                Kind::Canvas { .. } => "Canvas",
                Kind::Scene3D { .. } => "Scene3D",
                Kind::Markdown { .. } => "Markdown",
                Kind::Svg { icon: Some(_), .. } => "Icon",
                Kind::Svg { .. } => "Svg",
                Kind::CodeEditor { .. } => "CodeEditor",
                Kind::Slider { .. } => "Slider",
                Kind::Stepper { .. } => "Stepper",
                Kind::ColorPicker { .. } => "ColorPicker",
                Kind::Swatch(_) => "Swatch",
                Kind::Disclosure { .. } => "DisclosureGroup",
                Kind::Lazy { across: false, .. } => "LazyVStack",
                Kind::Lazy { .. } => "LazyHStack",
                Kind::Grid { .. } => "Grid",
                Kind::GridRow => "GridRow",
                Kind::Table(spec) => {
                    if spec.tree.is_some() {
                        "Tree"
                    } else {
                        "Table"
                    }
                }
                Kind::TableRow(_) => "TableRow",
                Kind::MenuItem { .. } => "MenuItem",
                Kind::MenuDivider => "MenuDivider",
                Kind::Menu { .. } => "Menu",
                Kind::MenuList => "MenuList",
                Kind::Popover { .. } => "Popover",
                Kind::Window { .. } => "Window",
                Kind::StatusBar => "StatusBar",
                Kind::Ribbon { .. } => "Ribbon",
                Kind::RibbonTab { .. } => "RibbonTab",
                Kind::RibbonGroup { .. } => "RibbonGroup",
                Kind::RibbonItem { role: super::RibbonRole::Button(_), .. } => "RibbonButton",
                Kind::RibbonItem { role: super::RibbonRole::Toggle(..), .. } => "RibbonToggle",
                Kind::RibbonItem { role: super::RibbonRole::Menu, .. } => "RibbonMenu",
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
            if matches!(n.kind, Kind::Scroll(_)) || n.mods.clipped {
                clip = Rect { x: x0, y: y0, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0) };
            }
            // A tab view shows only its selected page, a ribbon its selected tab.
            if let Kind::TabView { .. } | Kind::Ribbon { .. } | Kind::RibbonItem { .. } = n.kind {
                for c in n.visible_children() {
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
        tray: None,
        show_main: None,
        main_hidden: false,
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
    set_script_clock(0.0);
    crate::stdlib::start_script_clock();
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
                // `click shift "…"`, `click cmd "…"`: with the key held (a table's rows).
                let mut rest = rest;
                let mut held = (false, false);
                loop {
                    if let Some(r) = rest.strip_prefix("shift ") {
                        (held.0, rest) = (true, r);
                    } else if let Some(r) = rest.strip_prefix("cmd ") {
                        (held.1, rest) = (true, r);
                    } else {
                        break;
                    }
                }
                app.ui.click_mods = held;
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
                app.ui.click_mods = (false, false);
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
                // A double click in text selects a word; on a table's row,
                // it opens it (or edits the cell).
                if let (None, [x, y], "doubleclick") = (key, &n[..], word) {
                    match app.hit(*x, *y) {
                        Some(Hit::Field(k)) => {
                            app.press_field(k, *x, *y, false, 1);
                            app.press_field(k, *x, *y, false, 2);
                            mouse = (*x, *y);
                            continue;
                        }
                        Some(Hit::TableRow(..)) => {
                            app.click(*x, *y);
                            app.click(*x, *y);
                            mouse = (*x, *y);
                            continue;
                        }
                        _ => {}
                    }
                }
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
                // (A file dragged out of the app: a headless run only says so.)
                if let Some(path) = key.and_then(|k| app.node(k)).and_then(|n| n.mods.drag_file.clone()) {
                    println!("(drag file {path:?} out)");
                    continue;
                }
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
                if app.ui.editing.is_some() {
                    app.type_text(text);
                } else {
                    // Nothing is edited: each character is a key press, as
                    // typing it would be (for `.onKeyEvent` and `.onKey`).
                    for c in text.chars() {
                        let (name, shift) = unshifted(c);
                        let mods = Mods { shift, ..Mods::default() };
                        app.press(&name, &c.to_string(), mods, false);
                        app.update();
                    }
                }
            }
            "keyup" => {
                let mut mods = Mods::default();
                let mut name = rest;
                while let Some((m, tail)) = name.split_once('+').filter(|(_, tail)| !tail.is_empty()) {
                    match m {
                        "shift" => mods.shift = true,
                        "alt" => mods.alt = true,
                        "cmd" | "ctrl" => mods.primary = true,
                        "control" => mods.control = true,
                        _ => {}
                    }
                    name = tail;
                }
                app.release_key(name, mods);
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
                // A modifier pressed on its own is held down.
                match name {
                    "shift" => mods.shift = true,
                    "alt" => mods.alt = true,
                    "control" => mods.control = true,
                    "command" => mods.primary = true,
                    _ => {}
                }
                // What the key types: a character (shifted with Shift, as on
                // a US keyboard), or a space.
                let text = match name {
                    "space" => " ".to_string(),
                    _ if name.chars().count() == 1 && mods.shift => shifted(name).to_string(),
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
                // A split view's divider follows the pointer.
                if let ([x, y, dx, dy], Some(Hit::Split(key))) = (&n[..], n.get(..2).and_then(|p| app.hit(p[0], p[1])))
                    && let Some(node) = app.node(key)
                    && let Kind::Split { vertical, .. } = node.kind
                {
                    let c = node.layout.rect.inset_by(node.mods.padding);
                    let size = super::layout::split_first(node, if vertical { c.h } else { c.w });
                    let _ = (x, y);
                    app.set_split(key, size + if vertical { *dy } else { *dx });
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
            // `waitfor "label"`: until a view with that label is there (what
            // a running command, a load or a timer brings about). When
            // nothing is running, the clock moves on to the next timer.
            "waitfor" => {
                let label = rest.trim_matches('"');
                let started = Instant::now();
                let until = now() + WAITFOR_SPAN;
                loop {
                    app.update();
                    if app.find_label(label).is_some() {
                        break;
                    }
                    if started.elapsed() > Duration::from_secs(60) {
                        eprintln!("script: `{label}` never appeared");
                        return 2;
                    }
                    if deliver_background() | fire_timers() {
                        app.dirty = true;
                    } else if !anything_running() {
                        // Nothing to wait for but timers: on to the next one.
                        // (An hour of the app's time is enough looking.)
                        match next_timer() {
                            Some(t) if t <= until => set_script_clock(t.max(now())),
                            Some(_) => {
                                eprintln!("script: `{label}` never appeared (in an hour of the app's time)");
                                return 2;
                            }
                            None => std::thread::sleep(Duration::from_millis(5)),
                        }
                    } else {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            "wait" => {
                // Background work in progress finishes first, and its results
                // arrive (tests shouldn't depend on timing): for at most a
                // minute of real time in all.
                let deadline = Instant::now() + Duration::from_secs(60);
                settle(app, deadline);
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
                // What they start finishes before the clock moves on, as
                // what its results start does (#52).
                while let Some(t) = next_timer().filter(|t| *t <= end) {
                    set_script_clock(t);
                    if fire_timers() {
                        app.dirty = true;
                        app.update();
                    }
                    settle(app, deadline);
                }
                set_script_clock(end);
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
            // `tray`: what the tray icon shows; `tray "Tick"`: chooses an item of its menu.
            "tray" => {
                let Some(tray) = app.tray.clone() else {
                    eprintln!("script: the app has no `.trayIcon`");
                    return 2;
                };
                let labels = quoted(rest);
                if labels.is_empty() {
                    print!("Tray {}", tray.describe());
                } else if let Err(e) = app.choose_item(&tray.items, &labels) {
                    eprintln!("script: {e}");
                    return 2;
                }
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
            // What a screen reader is told is in the window (see `access`).
            "accessibility" => {
                app.update();
                print!("{}", super::access::listing(&app.accessible("").0));
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
                app.notice_sync();
                if let Some(notice) = &app.ui.notice {
                    println!("Notice {:?}", notice.text);
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
/// What a phone's screen adds: the system's bars to stay clear of, and a
/// finger, which scrolls where a mouse would turn its wheel.
#[derive(Default)]
struct Phone {
    /// How far the system's bars reach into the window, in pixels: left,
    /// top, right, bottom. The app is laid out and drawn inside them.
    insets: [f64; 4],
    /// Whether `insets` are the window's as it is now.
    insets_known: bool,
    finger: Option<Finger>,
    /// A second finger, and how far it was from the first: the two pinch
    /// (for a view with `.onPointer`).
    pinch: Option<(u64, (f64, f64), f64)>,
    /// A lifted finger whose release the app hasn't been given yet. It's
    /// given once the touch itself is dealt with (see `about_to_wait`):
    /// what it clicks may wait for a dialog, and Android takes an app that
    /// holds on to a touch for seconds to have stopped responding.
    lifted: Option<(WindowId, winit::event::DeviceId)>,
    /// The field the keyboard is shown for.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    typing_in: Option<u64>,
    /// The field's text and selection as the keyboard was last told (see
    /// `android::keyboard`); none if the keyboard only types into it.
    #[cfg(target_os = "android")]
    told: Option<super::android::Text>,
    /// The number the keyboard knows the field by, and how many of the
    /// keyboard's changes of it were taken in.
    #[cfg(target_os = "android")]
    field: i32,
    #[cfg(target_os = "android")]
    serial: i32,
    /// The caret is to be brought into view (the keyboard came up over it,
    /// or another field is edited).
    reveal: bool,
    /// Until when to keep drawing after the screen came back: what's drawn
    /// to a new screen before the system shows it is lost. Also while the
    /// on-screen keyboard slides in or out, which moves the bottom inset.
    fresh: Option<Instant>,
    /// A scroll that goes on after the finger let go.
    fling: Option<Fling>,
}

/// The finger on the screen.
struct Finger {
    id: u64,
    start: (f64, f64),
    /// When it came down (held long enough in one place, it's a long press).
    down_at: Instant,
    last: (f64, f64),
    moved_at: Instant,
    /// Points per second, smoothed.
    velocity: (f64, f64),
    doing: Touching,
}

#[derive(PartialEq)]
enum Touching {
    /// On something that scrolls: a tap if it lifts, a scroll if it moves.
    Waiting,
    Scrolling,
    /// Like a mouse button held down.
    Pressing,
    /// A long press that opened a menu: nothing more until it lifts.
    Held,
}

/// How long a finger stays in one place before it's a long press (which
/// opens a context menu, like a right click).
const LONG_PRESS: Duration = Duration::from_millis(500);

struct Fling {
    at: (f64, f64),
    velocity: (f64, f64),
    stepped: Instant,
}

/// How far a finger moves before it's a scroll and not a tap, in points.
const TOUCH_SLOP: f64 = 8.0;

struct WindowApp {
    app: App,
    mods: Mods,
    phone: Phone,
    /// The other open windows (`Window`s, or the main one), parked while
    /// another one's events are handled (see `focus`).
    parked: Vec<Win>,
    /// The tray icon the system shows, as `TrayInfo::describe` says it.
    tray_shown: Option<String>,
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
    /// The table column being widened or narrowed: the table, the column,
    /// where the mouse was pressed and how wide the column was then.
    resizing: Option<(u64, usize, f64, f64)>,
    /// The split view whose divider is being dragged: where the mouse was
    /// pressed (along it) and the first pane's size then.
    splitting: Option<(u64, f64, f64)>,
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
    /// What tells screen readers what's in each window (see `access`).
    #[cfg(any(target_os = "macos", windows))]
    access: HashMap<WindowId, super::access::system::Adapter>,
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
    resizing: Option<(u64, usize, f64, f64)>,
    /// The split view whose divider is being dragged: where the mouse was
    /// pressed (along it) and the first pane's size then.
    splitting: Option<(u64, f64, f64)>,
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
            resizing: None,
            splitting: None,
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
    #[cfg(not(target_os = "android"))]
    let made = EventLoop::new();
    // (On Android the loop is the activity's.)
    #[cfg(target_os = "android")]
    let made = {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        EventLoop::builder().with_android_app(super::android::app()).build()
    };
    let event_loop = match made {
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
        phone: Phone::default(),
        parked: Vec::new(),
        native_menu: (None, u64::MAX),
        tray_shown: None,
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
        resizing: None,
        splitting: None,
        drag_start: None,
        pointer: winit::window::CursorIcon::Default,
        last_click: None,
        ime: false,
        ime_caret: None,
        swallow: false,
        #[cfg(any(target_os = "macos", windows))]
        access: HashMap::new(),
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
    super::tray::shutdown();
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
        std::mem::swap(&mut self.resizing, &mut w.resizing);
        std::mem::swap(&mut self.splitting, &mut w.splitting);
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
        // (Shown once the system's hold on it for screen readers is there:
        // it has to be there first.)
        #[cfg(any(target_os = "macos", windows))]
        let attrs = attrs.with_visible(false);
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
        #[cfg(any(target_os = "macos", windows))]
        {
            self.access.insert(window.id(), super::access::system::adapter(event_loop, &window));
            window.set_visible(true);
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
        #[cfg(any(target_os = "macos", windows))]
        {
            let there: Vec<WindowId> = self.parked.iter().filter_map(|w| w.window.as_ref()).chain(&self.window).map(|w| w.id()).collect();
            self.access.retain(|id, _| there.contains(id));
        }
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
                #[cfg(target_os = "macos")]
                Std::About => super::native::show_about(),
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
        self.sync_tray();
    }

    /// Gives the system the app's tray icon when it changed, hides or shows
    /// the main window, and shows the notifications asked for.
    fn sync_tray(&mut self) {
        let main = if self.key == 0 { self.window.clone() } else { self.parked.iter().find(|w| w.key == 0).and_then(|w| w.window.clone()) };
        if let Some(show) = self.app.show_main.take() {
            if let Some(w) = &main {
                w.set_visible(show);
                if show {
                    w.focus_window();
                }
            }
        }
        let wanted = self.app.tray.as_ref().map(super::tray::TrayInfo::describe);
        if wanted != self.tray_shown {
            super::tray::set(main.as_deref(), self.app.tray.as_ref());
            self.tray_shown = wanted;
        }
        for (title, message) in NOTIFYING.with(|n| std::mem::take(&mut *n.borrow_mut())) {
            super::tray::notify(main.as_deref(), &self.app.title, &title, &message);
        }
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
                Shell::Tray(path) => {
                    let item = self.app.tray.as_ref().and_then(|t| menu::at(&t.items, &path).cloned());
                    if let Some(e @ Entry::Item { enabled: true, .. }) = item {
                        self.app.run_entry(&e);
                    }
                }
                Shell::TrayClick => {
                    self.app.show_main = Some(true);
                    self.app.main_hidden = false;
                }
            }
        }
        any
    }

    /// A point of the window (in pixels) as the app sees it: in points,
    /// from the corner it's drawn from.
    fn point(&self, at: winit::dpi::PhysicalPosition<f64>) -> (f64, f64) {
        let scale = self.scale();
        ((at.x - self.phone.insets[0]) / scale, (at.y - self.phone.insets[1]) / scale)
    }

    /// Moves a scroll that goes on after the finger let go, slowing down.
    fn step_fling(&mut self) {
        let Some(fling) = &mut self.phone.fling else { return };
        let now = Instant::now();
        let dt = (now - fling.stepped).as_secs_f64().min(0.05);
        fling.stepped = now;
        let (at, (vx, vy)) = (fling.at, fling.velocity);
        let slower = (-4.0 * dt).exp();
        fling.velocity = (vx * slower, vy * slower);
        if fling.velocity.0.hypot(fling.velocity.1) < 20.0 {
            self.phone.fling = None;
        }
        self.app.scroll(at.0, at.1, vx * dt, vy * dt, self.mods);
    }

    /// A finger on the screen. It's the mouse with its button held, except
    /// on something that scrolls (and isn't itself dragged, like a slider):
    /// there, moving it scrolls, and only lifting it where it landed is a
    /// click.
    fn touch(&mut self, event_loop: &ActiveEventLoop, id: WindowId, touch: &winit::event::Touch) {
        use winit::event::TouchPhase;
        let device_id = touch.device_id;
        let moved = WindowEvent::CursorMoved { device_id, position: touch.location };
        let button = |state| WindowEvent::MouseInput { device_id, state, button: MouseButton::Left };
        let p = self.point(touch.location);
        // A second finger: with the first, it pinches (others are ignored).
        if let Some(first) = self.phone.finger.as_mut().filter(|f| f.id != touch.id) {
            let apart = (p.0 - first.last.0).hypot(p.1 - first.last.1).max(1.0);
            let middle = ((p.0 + first.last.0) / 2.0, (p.1 + first.last.1) / 2.0);
            match (touch.phase, self.phone.pinch) {
                (TouchPhase::Started, None) => {
                    // (What the first finger pressed isn't clicked.)
                    first.doing = Touching::Held;
                    self.app.ui.pressed = None;
                    self.selecting = false;
                    self.drag_start = None;
                    self.phone.pinch = Some((touch.id, p, apart));
                }
                (TouchPhase::Moved, Some((id, _, before))) if id == touch.id => {
                    self.phone.pinch = Some((id, p, apart));
                    if let Some(key) = self.app.pointer_at(middle.0, middle.1) {
                        self.app.pointer(key, PINCH, middle, (0.0, apart / before - 1.0), 0, 0, self.mods);
                        self.request_redraw();
                    }
                }
                (TouchPhase::Ended | TouchPhase::Cancelled, Some((id, ..))) if id == touch.id => self.phone.pinch = None,
                _ => {}
            }
            return;
        }
        match touch.phase {
            TouchPhase::Started => {
                self.release_lifted(event_loop);
                self.phone.fling = None;
                self.window_event(event_loop, id, moved);
                let dragged = matches!(self.app.hit(p.0, p.1), Some(Hit::Slider(_) | Hit::Drag(_) | Hit::Pointer(_) | Hit::Palette(_)));
                let scrolls = self.app.wheel_target(p.0, p.1, 0.0, 0.0).is_some_and(|(_, pointer)| !pointer);
                let waits = scrolls && !dragged && self.app.menu.is_none();
                if !waits {
                    self.window_event(event_loop, id, button(ElementState::Pressed));
                }
                let doing = if waits { Touching::Waiting } else { Touching::Pressing };
                let now = Instant::now();
                self.phone.finger =
                    Some(Finger { id: touch.id, start: p, down_at: now, last: p, moved_at: now, velocity: (0.0, 0.0), doing });
            }
            TouchPhase::Moved => {
                let Some(f) = &mut self.phone.finger else { return };
                if f.doing == Touching::Held {
                    // (Pinching: the first finger moves too.)
                    f.last = p;
                    if let Some((id, second, before)) = self.phone.pinch {
                        let apart = (p.0 - second.0).hypot(p.1 - second.1).max(1.0);
                        let middle = ((p.0 + second.0) / 2.0, (p.1 + second.1) / 2.0);
                        self.phone.pinch = Some((id, second, apart));
                        if let Some(key) = self.app.pointer_at(middle.0, middle.1) {
                            self.app.pointer(key, PINCH, middle, (0.0, apart / before - 1.0), 0, 0, self.mods);
                            self.request_redraw();
                        }
                    }
                    return;
                }
                if f.doing == Touching::Pressing {
                    f.last = p;
                    self.window_event(event_loop, id, moved);
                    return;
                }
                if f.doing == Touching::Waiting && (p.0 - f.start.0).hypot(p.1 - f.start.1) > TOUCH_SLOP {
                    f.doing = Touching::Scrolling;
                }
                let now = Instant::now();
                let (dx, dy) = (f.last.0 - p.0, f.last.1 - p.1);
                let dt = (now - f.moved_at).as_secs_f64().max(0.001);
                f.velocity = (0.6 * f.velocity.0 + 0.4 * dx / dt, 0.6 * f.velocity.1 + 0.4 * dy / dt);
                (f.last, f.moved_at) = (p, now);
                if f.doing == Touching::Scrolling {
                    let at = f.start;
                    self.app.scroll(at.0, at.1, dx, dy, self.mods);
                    self.request_redraw();
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(f) = self.phone.finger.take() else { return };
                self.phone.pinch = None;
                let lifted = touch.phase == TouchPhase::Ended;
                match f.doing {
                    Touching::Waiting if lifted => {
                        self.window_event(event_loop, id, button(ElementState::Pressed));
                        self.phone.lifted = Some((id, device_id));
                    }
                    Touching::Waiting | Touching::Held => {}
                    Touching::Pressing => self.phone.lifted = Some((id, device_id)),
                    // Let go while moving: the scroll goes on by itself.
                    Touching::Scrolling => {
                        let moving = f.moved_at.elapsed().as_secs_f64() < 0.1 && f.velocity.0.hypot(f.velocity.1) > 60.0;
                        if lifted && moving {
                            self.phone.fling = Some(Fling { at: f.start, velocity: f.velocity, stepped: Instant::now() });
                            self.request_redraw();
                        }
                    }
                }
                // (A lifted finger points at nothing: no view stays hovered.)
                if self.phone.lifted.is_none() {
                    self.window_event(event_loop, id, WindowEvent::CursorLeft { device_id });
                }
            }
        }
    }

    /// Gives the app the release of a finger that lifted (see `Phone::lifted`).
    fn release_lifted(&mut self, event_loop: &ActiveEventLoop) {
        let Some((id, device_id)) = self.phone.lifted.take() else { return };
        self.window_event(event_loop, id, WindowEvent::MouseInput { device_id, state: ElementState::Released, button: MouseButton::Left });
        self.window_event(event_loop, id, WindowEvent::CursorLeft { device_id });
    }

    /// A finger held in one place long enough opens the context menu of
    /// what's under it, as a right click does. Returns when to look again.
    fn long_press(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let f = self.phone.finger.as_ref()?;
        let still = (f.last.0 - f.start.0).hypot(f.last.1 - f.start.1) <= TOUCH_SLOP;
        if !still || !matches!(f.doing, Touching::Waiting | Touching::Pressing) {
            return None;
        }
        let due = f.down_at + LONG_PRESS;
        if Instant::now() < due {
            return Some(due);
        }
        let (x, y) = f.start;
        if !matches!(self.app.context_target(x, y), Some((_, true))) {
            return None;
        }
        // (What the finger pressed isn't clicked when it lifts.)
        self.app.ui.pressed = None;
        self.selecting = false;
        self.drag_start = None;
        if let Some(f) = &mut self.phone.finger {
            f.doing = Touching::Held;
        }
        if self.app.mouse_press(x, y, true) {
            self.after_app(event_loop);
            self.request_all();
        }
        None
    }

    fn redraw(&mut self) {
        self.step_fling();
        // The system's bars may have changed with the window.
        // (While text is edited, the keyboard may come and go at any time.)
        #[cfg(target_os = "android")]
        if (!self.phone.insets_known || self.phone.fresh.is_some() || self.ime)
            && let Some(insets) = super::android::insets()
        {
            if insets != self.phone.insets {
                self.app.dirty = true;
            }
            self.phone.insets = insets;
            self.phone.insets_known = true;
            if let Some(screen) = self.screen.as_mut() {
                screen.origin = (insets[0], insets[1]);
            }
            // The phone's theme, for an app that follows it.
            if self.app.appearance == 0
                && let Some(dark) = super::android::dark()
                && theme::set_dark(dark)
            {
                self.app.dirty = true;
                super::android::bar_icons(!dark);
            }
        }
        let [left, top, right, bottom] = self.phone.insets;
        self.phone.fresh = self.phone.fresh.filter(|until| Instant::now() < *until);
        let again = self.phone.fling.is_some() || self.phone.fresh.is_some();
        let (Some(screen), Some(window)) = (self.screen.as_mut(), self.window.as_ref()) else { return };
        if self.key == 0 {
            if let Some(title) = NEW_TITLE.with(|t| t.borrow_mut().take()) {
                window.set_title(&title);
                self.app.title = title;
            }
            if let Some(on) = FULL_SCREEN.with(Cell::take) {
                window.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
            }
            IS_FULL_SCREEN.with(|f| f.set(window.fullscreen().is_some()));
        }
        let scale = window.scale_factor();
        let size = window.inner_size();
        self.app.width = (size.width as f64 - left - right).max(1.0) / scale;
        self.app.height = (size.height as f64 - top - bottom).max(1.0) / scale;
        // The system's web views, where theirs are laid out.
        super::webview::sync(window, scale, &self.app.web_views());
        let list = self.app.scene(scale);
        // The caret, brought back above the keyboard if it's under it now
        // (where what it's in can scroll).
        if std::mem::take(&mut self.phone.reveal)
            && let Some(caret) = self.app.caret_rect()
        {
            let below = caret.y + caret.h + 12.0 - self.app.height;
            if below > 0.0 {
                self.app.scroll(caret.x, caret.y.min(self.app.height - 1.0), 0.0, below, self.mods);
                window.request_redraw();
            }
        }
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
        if self.app.ui.anim.running || again {
            window.request_redraw();
        }
        // A screen reader, if one's listening, is told what the window shows now.
        #[cfg(any(target_os = "macos", windows))]
        if let Some(adapter) = self.access.get_mut(&window.id()) {
            let app = &self.app;
            adapter.update_if_active(|| {
                let (items, focus) = app.accessible(&self.title);
                super::access::system::update(&items, focus, scale)
            });
        }
    }

    /// Does what screen readers asked for (see `access::system::asked`): a
    /// press where the view is, or a step of a slider or stepper.
    #[cfg(any(target_os = "macos", windows))]
    fn accessible_asks(&mut self) -> bool {
        let asked = super::access::system::asked();
        for (window, id, action) in &asked {
            let Some(key) = self.key_of(*window) else { continue };
            if !self.focus(key) {
                continue;
            }
            let (items, _) = self.app.accessible(&self.title);
            let Some(item) = items.iter().find(|i| i.id == *id) else { continue };
            let middle = (item.rect.x + item.rect.w / 2.0, item.rect.y + item.rect.h / 2.0);
            match action {
                accesskit::Action::Click | accesskit::Action::Focus if item.press => self.app.click(middle.0, middle.1),
                accesskit::Action::Increment => {
                    self.app.step(*id, 1.0);
                }
                accesskit::Action::Decrement => {
                    self.app.step(*id, -1.0);
                }
                _ => {}
            }
            self.app.dirty = true;
        }
        !asked.is_empty()
    }

    /// Input methods are on while a text field or code editor is edited.
    fn sync_ime(&mut self) {
        // (Text that's only selected takes no typing.)
        let editing = self.app.ui.editing.as_ref().is_some_and(|e| self.app.node(e.key).is_some_and(typable));
        if editing != self.ime {
            self.ime = editing;
            // (A phone's keyboard takes a moment to slide in or out.)
            if cfg!(target_os = "android") {
                self.phone.fresh = Some(Instant::now() + Duration::from_millis(900));
                self.request_redraw();
            }
            #[cfg(not(target_os = "android"))]
            if let Some(w) = &self.window {
                w.set_ime_allowed(editing);
            }
            if !editing {
                self.app.ui.composing = None;
            }
        }
        // A phone's keyboard is shown for the field that's edited: what it
        // offers depends on the field, and it starts over for another.
        #[cfg(target_os = "android")]
        {
            use super::android::Typing;
            // (Not for text that's only selected.)
            let key = self.app.ui.editing.as_ref().map(|e| e.key).filter(|k| self.app.node(*k).is_some_and(typable));
            let now = self.app.edited();
            if key != self.phone.typing_in {
                self.phone.typing_in = key;
                self.phone.reveal = key.is_some();
                let typing = key.map(|key| match self.app.node(key).map(|n| &n.kind) {
                    Some(Kind::TextField { secure: true, .. }) => Typing::Secret,
                    Some(Kind::TextField { lines: 2.., .. }) => Typing::Lines,
                    Some(Kind::CodeEditor { .. }) => Typing::Code,
                    _ => Typing::Text,
                });
                // The keyboard reads and corrects a field's text. Code is
                // only typed (and a very long text: it's passed whole).
                self.phone.told = now.clone().filter(|t| typing != Some(Typing::Code) && t.text.len() < 20_000);
                self.phone.field += 1;
                self.phone.serial = 0;
                super::android::keyboard(typing, self.phone.field, self.phone.told.as_ref());
            } else if let (Some(now), true) = (now, self.phone.told.is_some()) {
                if self.phone.told.as_ref() != Some(&now) {
                    super::android::typed(&now, self.phone.serial);
                    self.phone.told = Some(now);
                }
            }
        }
    }

    /// What the phone's keyboard and system said since the last look (see
    /// `android::Input`). Whether anything was said.
    #[cfg(target_os = "android")]
    fn phone_input(&mut self) -> bool {
        let said = super::android::take_input();
        for input in &said {
            match input {
                super::android::Input::Typed(text) => {
                    self.app.compose(String::new(), None);
                    self.app.type_text(text);
                }
                super::android::Input::Composing(text) => self.app.compose(text.clone(), Some(text.len())),
                super::android::Input::Edit { field, serial, start, end, text, select } => {
                    if *field == self.phone.field && self.phone.told.is_some() {
                        self.phone.serial = *serial;
                        self.app.compose(String::new(), None);
                        self.app.replace(*start..*end, text, *select);
                    }
                }
                super::android::Input::Delete(before, after) => {
                    for (count, key) in [(*before, "backspace"), (*after, "delete")] {
                        for _ in 0..count.min(64) {
                            self.app.press(key, "", Mods::default(), false);
                        }
                    }
                }
                // (The keyboard slides: its edge is read until it rests.)
                super::android::Input::Insets => {
                    self.phone.insets_known = false;
                    self.phone.reveal = self.phone.typing_in.is_some();
                    self.phone.fresh = Some(Instant::now() + Duration::from_millis(500));
                }
            }
        }
        if !said.is_empty() {
            self.sync_ime();
            self.request_redraw();
        }
        !said.is_empty()
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl WindowApp {
    /// The file chooser took the screen away while the program waited for
    /// it, where the event loop couldn't see (`android::choose`): what
    /// `suspended` and `resumed` would have done is done now.
    fn screen_lost(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "android")]
        {
            let became = super::android::screen();
            if became != 0 && self.key == 0 {
                self.screen = None;
                self.window = None;
                if became == 2 {
                    self.resumed(_event_loop);
                }
            }
        }
    }
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.screen_lost(event_loop);
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
        self.phone.insets_known = false;
        self.phone.fresh = cfg!(target_os = "android").then(|| Instant::now() + Duration::from_millis(700));
        self.request_redraw();
        // (Replaces winit's menu bar, which is made at launch.)
        self.app.update();
        self.after_app(event_loop);
    }

    /// A phone takes the screen away when the app goes to the background,
    /// and gives a new one when it's back (`resumed`).
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if cfg!(target_os = "android") && self.key == 0 {
            self.screen = None;
            self.window = None;
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if std::env::var_os("TESSEL_DEBUG_EVENTS").is_some() && !matches!(event, WindowEvent::RedrawRequested) {
            eprintln!("event: {event:?}");
        }
        self.screen_lost(event_loop);
        #[cfg(any(target_os = "macos", windows))]
        if let Some(adapter) = self.access.get_mut(&id) {
            let window = self.window.iter().chain(self.parked.iter().filter_map(|w| w.window.as_ref())).find(|w| w.id() == id);
            if let Some(window) = window {
                adapter.process_event(window, &event);
            }
        }
        if let WindowEvent::Touch(touch) = &event {
            self.touch(event_loop, id, touch);
            return;
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
            WindowEvent::Focused(true) => {
                self.phone.insets_known = false;
                self.request_redraw();
            }
            WindowEvent::Resized(size) => {
                self.phone.insets_known = false;
                if let Some(s) = self.screen.as_mut() {
                    s.resize(size.width, size.height);
                }
                // On macOS, the frame of the new size at once, while the
                // window is being resized: not after it has shown the
                // last one's (which looks like blinking).
                if cfg!(target_os = "macos") && self.screen.is_some() {
                    self.redraw();
                } else {
                    self.request_redraw();
                }
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
                let before = self.cursor;
                self.cursor = self.point(position);
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
                // A split view's divider follows the mouse.
                if let Some((key, from, size)) = self.splitting {
                    let vertical = self.app.node(key).is_some_and(|n| matches!(n.kind, Kind::Split { vertical: true, .. }));
                    let at = if vertical { self.cursor.1 } else { self.cursor.0 };
                    self.app.set_split(key, size + at - from);
                    self.request_redraw();
                    return;
                }
                // A table's column follows the mouse as it's widened or narrowed.
                if let Some((table, column, x0, w0)) = self.resizing {
                    let width = (w0 + self.cursor.0 - x0).max(super::table::MIN_WIDTH);
                    let count = self.app.table_parts(table).map_or(0, |(s, _)| s.columns.len());
                    super::table::WIDTHS.with(|w| {
                        let mut w = w.borrow_mut();
                        let set = w.entry(table).or_default();
                        set.resize(count.max(set.len()), None);
                        if let Some(slot) = set.get_mut(column) {
                            *slot = Some(width);
                        }
                    });
                    self.app.dirty = true;
                    self.request_redraw();
                    return;
                }
                // A view with `.draggable` starts moving once the mouse has
                // gone a few points (less is a click).
                if let (Some((key, (sx, sy))), None) = (self.drag_start, &self.app.ui.dragging) {
                    if (self.cursor.0 - sx).hypot(self.cursor.1 - sy) > 4.0 {
                        self.drag_start = None;
                        // A file goes out of the app, as the system drags files.
                        let file = self.app.node(key).and_then(|n| n.mods.drag_file.clone());
                        match (file, self.window.clone()) {
                            (Some(path), Some(window)) => {
                                self.app.ui.pressed = None;
                                super::desktop::drag_file(&window, &path);
                            }
                            _ => self.app.begin_drag(key, sx, sy),
                        }
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
                    let resize = match self.app.hit(x, y) {
                        Some(Hit::TableResize(..)) => Some(winit::window::CursorIcon::ColResize),
                        Some(Hit::Split(key)) => Some(match self.app.node(key).map(|n| &n.kind) {
                            Some(Kind::Split { vertical: true, .. }) => winit::window::CursorIcon::RowResize,
                            _ => winit::window::CursorIcon::ColResize,
                        }),
                        _ => None,
                    };
                    let pointer = resize.or_else(|| self.app.cursor_at(x, y)).or_else(|| self.app.drag_cursor(x, y)).unwrap_or_default();
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
                        if let Some(Hit::TableResize(table, column)) = hit {
                            let width = self.app.node(table).and_then(|n| n.layout.tabs.get(column)).map_or(100.0, |t| t.rect.w);
                            self.resizing = Some((table, column, x, width));
                        }
                        if let Some(Hit::Split(key)) = hit
                            && let Some(n) = self.app.node(key)
                            && let Kind::Split { vertical, .. } = n.kind
                        {
                            let c = n.layout.rect.inset_by(n.mods.padding);
                            let size = super::layout::split_first(n, if vertical { c.h } else { c.w });
                            self.splitting = Some((key, if vertical { y } else { x }, size));
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
                        self.resizing = None;
                        self.splitting = None;
                        self.drag_start = None;
                        self.app.ui.click_mods = (self.mods.shift, self.mods.primary);
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
            // A phone's Back button: closes what's open on top, and with
            // nothing open, steps out of the app.
            #[cfg(target_os = "android")]
            #[cfg(target_os = "android")]
            WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::BrowserBack) => {
                if event.state == ElementState::Pressed && !self.app.back() {
                    super::android::leave();
                }
                self.request_all();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(name) = key_name(&event) else { return };
                // (A modifier's own press or release says how it is now,
                // whichever of it and the change of modifiers comes first.)
                let mut mods = self.mods;
                let down = event.state == ElementState::Pressed;
                let mac = cfg!(target_os = "macos");
                match name.as_str() {
                    "shift" => mods.shift = down,
                    "alt" => mods.alt = down,
                    "control" => {
                        mods.control = down;
                        if !mac {
                            mods.primary = down;
                        }
                    }
                    "command" if mac => mods.primary = down,
                    _ => {}
                }
                if down {
                    let text = event.text.as_deref().unwrap_or("").to_string();
                    self.app.press(&name, &text, mods, event.repeat);
                } else {
                    self.app.release_key(&name, mods);
                }
                self.request_redraw();
            }
            _ => {}
        }
        self.sync_ime();
        self.after_app(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        #[cfg(target_os = "android")]
        if self.phone_input() {
            self.app.dirty = true;
        }
        #[cfg(any(target_os = "macos", windows))]
        if self.accessible_asks() | super::access::system::wanted() {
            self.request_all();
        }
        let shell = self.take_shell_events();
        if deliver_background() || shell {
            self.app.dirty = true;
        }
        self.after_app(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.screen_lost(event_loop);
        self.release_lifted(event_loop);
        // The activity is gone, and the system waits for the app to stop.
        #[cfg(target_os = "android")]
        if self.window.is_none() && super::android::destroyed() {
            let _ = std::io::Write::flush(&mut std::io::stdout());
            unsafe { libc::_exit(0) };
        }
        if self.take_shell_events() | deliver_background_and_rearm() {
            self.app.dirty = true;
        }
        if fire_timers() {
            self.app.dirty = true;
        }
        // Sounds and videos playing: the next picture, soon.
        let mut media = super::media::tick_sounds();
        let mut frames = false;
        self.app.each_surface(|a| {
            let (playing, _) = a.tick_videos();
            frames |= playing;
        });
        if frames {
            self.request_all();
        }
        media |= frames;
        if self.app.tip_due() | self.app.notice_sync() {
            self.request_redraw();
        }
        self.after_app(event_loop);
        // Wake for the next timer, for a tooltip that's waiting to show, or
        // for a notice that's to go.
        let soon = media.then(|| now() + 1.0 / 60.0);
        let next = [next_timer(), self.app.tip_time(), notice_time(), soon].into_iter().flatten().fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))));
        let next = next.map(|t| Instant::now() + Duration::from_secs_f64((t - now()).max(0.0)));
        let held = self.long_press(event_loop);
        match [next, held].into_iter().flatten().min() {
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// A key's name, as `.onKeyEvent` and the script's `key` command name it:
/// the key without the modifiers (`"a"` for Shift+A, `"1"` for Shift+1),
/// or a special key's name. `None` for keys that are only modifiers.
/// Keys that only change what other keys do.
const MODIFIER_KEYS: [&str; 4] = ["shift", "alt", "control", "command"];

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
                NamedKey::Shift => "shift",
                NamedKey::Alt => "alt",
                NamedKey::Control => "control",
                NamedKey::Super | NamedKey::Meta => "command",
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
        // (Android gives Enter and Tab as the characters they type.)
        (Key::Character(c), _) if matches!(c.as_str(), "\n" | "\r") => Some("enter".to_string()),
        (Key::Character(c), _) if c.as_str() == "\t" => Some("tab".to_string()),
        (_, Key::Character(c)) | (Key::Character(c), _) => Some(c.to_lowercase()),
        _ => None,
    }
}

/// The keys of a US keyboard and what they type with Shift (for the `key`
/// and `type` script commands).
const SHIFTED: &[(char, char)] = &[
    ('1', '!'), ('2', '@'), ('3', '#'), ('4', '$'), ('5', '%'), ('6', '^'), ('7', '&'), ('8', '*'), ('9', '('),
    ('0', ')'), ('-', '_'), ('=', '+'), ('[', '{'), (']', '}'), ('\\', '|'), (';', ':'), ('\'', '"'), (',', '<'),
    ('.', '>'), ('/', '?'), ('`', '~'),
];

/// What a key (one character) types with Shift held.
fn shifted(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => SHIFTED.iter().find(|(k, _)| *k == c).map_or(c.to_uppercase().to_string(), |(_, s)| s.to_string()),
        _ => key.to_string(),
    }
}

/// The key that types `c`, and whether it takes Shift.
fn unshifted(c: char) -> (String, bool) {
    if c == ' ' {
        return ("space".into(), false);
    }
    if let Some((k, _)) = SHIFTED.iter().find(|(_, s)| *s == c) {
        return (k.to_string(), true);
    }
    if c.is_uppercase() {
        return (c.to_lowercase().to_string(), true);
    }
    (c.to_string(), false)
}

/// The locator of an editable view's text.
/// How many bytes two texts start with in common (at a character boundary).
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices().zip(b.chars()).find(|((_, x), y)| x != y).map_or(a.len().min(b.len()), |((i, _), _)| i)
}

/// Whether a view's text is edited in lines: a code editor, or a text field
/// of several.
fn in_lines(kind: &Kind) -> bool {
    matches!(kind, Kind::CodeEditor { .. } | Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. })
}

/// Whether a view's text can be typed in (a text field, code editor or
/// rich text editor).
fn typable(n: &Node) -> bool {
    editable_text(n).is_some() || matches!(n.kind, Kind::RichEditor { .. })
}

/// Puts `value` where a `String` locator points (letting go of what was there).
fn set_string(locator: &Callback, value: &str) {
    let slot = locator.locate() as *mut *mut Str;
    unsafe {
        let old = *slot;
        *slot = crate::str_new(value);
        crate::tsl_release(old as *mut Header);
    }
}

/// What a text field or code editor calls after its text is edited
/// (its `onChange`).
fn text_on_change(n: &Node) -> Option<Callback> {
    match &n.kind {
        Kind::TextField { on_change, .. } | Kind::CodeEditor { on_change, .. } => on_change.clone(),
        _ => None,
    }
}

fn editable_text(n: &Node) -> Option<&Callback> {
    match &n.kind {
        Kind::TextField { text, .. } | Kind::CodeEditor { text, .. } => Some(text),
        _ => None,
    }
}

/// Whether a view is text that can be selected but not changed: a
/// `Text` with `.selectable()`.
fn selectable_text(n: &Node) -> bool {
    matches!(n.kind, Kind::Text(_)) && n.layout.style.selectable
}

/// The text of a view whose text is selected or edited: a text field's or
/// code editor's, or a selectable `Text`'s (as it shows: cut short by its
/// `.lineLimit`, it's what shows that's selected).
fn shown_text(n: &Node) -> Option<String> {
    match &n.kind {
        Kind::Text(s) if selectable_text(n) => Some(text_shown(n, s)),
        Kind::RichEditor { doc, .. } => Some(doc.text.clone()),
        _ => editable_text(n).map(read_string),
    }
}

/// What shows of a `Text`'s `s`: all of it, or (cut short by its
/// `.lineLimit`) its start, without the spaces at its end, then "…".
fn text_shown(n: &Node, s: &str) -> String {
    match &n.layout.text {
        Some(t) if n.layout.cut => {
            let start = t.len.saturating_sub('…'.len_utf8()).min(s.len());
            format!("{}…", s.get(..start).unwrap_or(s))
        }
        _ => s.to_string(),
    }
}
