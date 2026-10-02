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
//!                       text, editor)
//! type hello world      type into the text field or code editor being edited
//! type "two words "     quoted, to keep leading or trailing spaces
//! key backspace         a key, optionally with modifiers: `key shift+left`,
//!                       `key alt+backspace`, `key cmd+a` (cmd means Ctrl on
//!                       Windows). Keys: backspace delete left right up down
//!                       home end enter tab escape, and letters for shortcuts
//! hover "Add"           move the pointer over a labelled view (or `hover 40 60`)
//! answer "/tmp/a.och"   what the next openFile/saveFile dialog returns
//!                       (`answer cancel` for cancelling it)
//! drag 300 40 -80 0     press at a point, and move by (dx, dy) with the
//!                       button held (for views with `.onDrag`)
//! scroll 0 120          scroll the view under the middle of the window by
//!                       (dx, dy) points
//! wheel 40 60 0 120     the same, at a point
//! press 40 60           press the mouse on a view with `.onPointer` (add
//!                       `right` or `middle` for another button), then
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
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vello::util::{RenderContext, RenderSurface};
use vello::{wgpu, Renderer, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use super::edit::{self, Clipboard, Edit, Editing, Unit};
use super::layout::{read_string, Layouter, Rect};
use super::render::{self, code_origin, field_origin, Interaction, Painter, TextEngine};
use super::theme::{self, theme};
use super::{build, Callback, Kind, Node, Rgba};
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
    /// `background` work still running: the blocks that get its results.
    static COMPUTING: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
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

/// `background(work:) { result in … }`: runs `worker(input)` on another
/// thread, then `done(result)` on the UI thread. `input` and the result are
/// codec trees, so the two threads share no values: the worker owns (and
/// frees) `input`, and `done` owns the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_background(
    worker: extern "C" fn(*mut u8) -> *mut u8,
    input: *mut u8,
    done_f: *const std::ffi::c_void,
    done_env: *mut crate::Header,
) {
    let id = next_id();
    COMPUTING.with(|c| c.borrow_mut().insert(id, Callback::new(done_f, done_env)));
    let input = input as usize;
    std::thread::spawn(move || {
        crate::stack_guard::thread_init();
        let result = worker(input as *mut u8);
        post(Event::Computed(id, result as usize));
    });
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
            Event::Computed(id, tree) => {
                if let Some(cb) = COMPUTING.with(|c| c.borrow_mut().remove(&id)) {
                    cb.call_with(tree as *mut u8);
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
}

impl Hit {
    fn key(&self) -> u64 {
        match *self {
            Hit::Button(k) | Hit::Toggle(k) | Hit::Field(k) | Hit::Segment(k, _) | Hit::Tap(k) | Hit::Link(k, _) | Hit::Drag(k) => k,
            Hit::Pointer(k) => k,
            Hit::Tab(k, _) | Hit::TabClose(k, _) => k,
        }
    }
}

struct App {
    root: Callback,
    title: String,
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
}

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
}

impl App {
    /// Rebuilds the tree if state may have changed, then lays it out.
    fn update(&mut self) {
        // `.onAppear` blocks can change state, which rebuilds again (a few times at most).
        if let Some(root) = NEW_ROOT.with(|r| r.borrow_mut().take()) {
            self.root = root;
            self.dirty = true;
        }
        // `.onResize` blocks run after layout, and can change state too:
        // then the tree is built and laid out again (a few times at most).
        for _ in 0..4 {
            for _ in 0..8 {
                if !(self.dirty || self.tree.is_none()) {
                    break;
                }
                self.dirty = false;
                self.tree = Some(build(&self.root));
                self.run_on_appear();
                self.sync_editor_lines();
            }
            if self.ui.editing.is_none() {
                self.ui.composing = None;
            }
            let composing =
                self.ui.editing.as_ref().zip(self.ui.composing.as_ref()).map(|(e, c)| (e.key, e.range(), c.text.as_str()));
            let tree = self.tree.as_mut().unwrap();
            Layouter { text: &mut self.text, scroll: &self.ui.scroll, composing }.run(tree, self.width, self.height);
            if !self.run_on_resize() {
                break;
            }
        }
        self.follow_caret();
        self.layout_popup();
        let tree = self.tree.as_ref().unwrap();
        self.ui.anim.step(tree, now(), (self.width, self.height), &self.ui.scroll);
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
    /// editor, with a field's text selected.
    fn focus_next(&mut self, back: bool) {
        fn fields<'n>(n: &'n Node, out: &mut Vec<&'n Node>) {
            if n.mods.hidden || n.layout.style.disabled {
                return;
            }
            if editable_text(n).is_some() {
                out.push(n);
            }
            for c in n.visible_children() {
                fields(c, out);
            }
        }
        let mut found = Vec::new();
        if let Some(t) = &self.tree {
            fields(t, &mut found);
        }
        let count = found.len();
        if count == 0 {
            return;
        }
        let current = self.ui.editing.as_ref().and_then(|e| found.iter().position(|n| n.key == e.key));
        let next = match (current, back) {
            (Some(i), false) => (i + 1) % count,
            (Some(i), true) => (i + count - 1) % count,
            (None, false) => 0,
            (None, true) => count - 1,
        };
        let node = found[next];
        let mut e = Editing::new(node.key, &editable_text(node).map(read_string).unwrap_or_default());
        if matches!(node.kind, Kind::TextField { .. }) {
            e.anchor = 0;
        }
        self.ui.editing = Some(e);
        self.ui.composing = None;
        self.completing.asked = None;
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
                badge: self.text.mono_line(letter, 11.0, 700.0, Rgba([255, 255, 255, 255])),
                badge_color,
                label: self.text.mono_line(&s.label, 13.0, 400.0, theme().colors[theme::PRIMARY]),
                detail: self.text.layout(&detail, 11.5, 400.0, theme().colors[theme::GRAY], None),
            };
            widest = widest.max(row.label.width() as f64 + row.detail.width() as f64);
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
        let now = (e.key, at.anchor, at.focus, layout.len());
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
                keep(sx, cx0, cx1, visible_w, layout.width() as f64 + 2.0);
                let sy = self.ui.scroll.entry(e.key).or_insert(0.0);
                keep(sy, cy0, cy1, visible_h, layout.height() as f64);
            }
            _ => {
                let caret_x = caret.map_or(0.0, |c| c.x0);
                let visible = (node.layout.rect.w - 16.0).max(1.0);
                keep(&mut e.scroll, caret_x, caret_x, visible, layout.width() as f64);
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
        let mut focus = None;
        if let Some(t) = &self.tree {
            visit(t, &mut keys, &self.keys, &mut appeared, &mut focus);
        }
        self.keys = keys;
        super::canvas::keep_only(&self.keys);
        if let Some((key, text)) = focus {
            self.ui.editing = Some(Editing::new(key, &text));
        }
        for cb in appeared {
            cb.call();
            self.dirty = true;
        }
    }

    /// Two-way sync of code editors' `line:` bindings: when the program sets
    /// a new line, move the caret there.
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
        let mut e = Editing::new(key, &text);
        e.anchor = offset;
        e.focus = offset;
        self.ui.editing = Some(e);
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

    fn scene(&mut self, scale: f64) -> Scene {
        self.update();
        let mut scene = Scene::new();
        let tree = self.tree.as_ref().unwrap();
        let mut painter = Painter { scene: &mut scene, ui: &self.ui, scale, offset: (0.0, 0.0), clip: None };
        painter.paint(tree);
        painter.paint_popup();
        painter.paint_tooltip();
        scene
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
                _ if n.mods.on_pointer.is_some() => Some(Hit::Pointer(n.key)),
                _ if n.mods.on_drag.is_some() => Some(Hit::Drag(n.key)),
                _ if n.mods.on_tap.is_some() => Some(Hit::Tap(n.key)),
                _ => None,
            }
        }
        visit(self.tree.as_ref()?, x, y)
    }

    /// Moves the pointer to a point (as the window does on mouse moves).
    pub fn hover(&mut self, x: f64, y: f64) {
        self.ui.pointer = Some((x, y));
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
        let layout = self.text.layout(&text, 12.0, 400.0, theme().colors[theme::PRIMARY], Some(280.0));
        let (w, h) = (layout.width() as f64 + 16.0, layout.height() as f64 + 10.0);
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
        let Some(cb) = self.tree.as_ref().and_then(find) else { return false };
        cb.call_with(crate::str_new(key) as *mut u8);
        self.dirty = true;
        true
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
        let Some(hit) = hit else { return };
        let Some(node) = self.node(hit.key()) else { return };
        match (&hit, &node.kind) {
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
            self.dirty = true;
        }
        self.ui.editing = Some(e);
        self.report_line();
        // Lay out again so the caret position is current.
        if !self.dirty {
            self.follow_caret();
        }
    }

    /// Undo (or redo) the last change in the view being edited.
    fn undo(&mut self, redo: bool) {
        let Some(mut e) = self.ui.editing.clone() else { return };
        let Some(text) = self.node(e.key).and_then(editable_text).cloned() else { return };
        let current = read_string(&text);
        let h = self.ui.history.entry(e.key).or_default();
        if h.last_text != current {
            h.undo.clear();
            h.redo.clear();
            return;
        }
        let (from, to) = if redo { (&mut h.redo, &mut h.undo) } else { (&mut h.undo, &mut h.redo) };
        let Some(step) = from.pop() else { return };
        to.push(edit::Snapshot { text: current, anchor: e.anchor, focus: e.focus });
        h.last_text = step.text.clone();
        h.typing = false;
        let slot = text.locate() as *mut *mut Str;
        unsafe {
            let old = *slot;
            *slot = crate::str_new(&step.text);
            crate::tsl_release(old as *mut Header);
        }
        e.anchor = step.anchor;
        e.focus = step.focus;
        self.ui.editing = Some(e);
        self.dirty = true;
    }

    /// Runs the `.shortcut` for Cmd/Ctrl + `key`, if a visible, enabled view has one.
    fn run_shortcut(&mut self, key: &str) -> bool {
        fn find(n: &Node, key: &str) -> Option<Callback> {
            if n.mods.hidden || n.layout.style.disabled {
                return None;
            }
            if let Some((k, cb)) = &n.mods.shortcut {
                if k == key {
                    return Some(cb.clone());
                }
            }
            n.visible_children().iter().find_map(|c| find(c, key))
        }
        let Some(cb) = self.tree.as_ref().and_then(|t| find(t, key)) else { return false };
        cb.call();
        self.dirty = true;
        true
    }

    /// A key press, by name (see the script commands).
    fn key(&mut self, name: &str, mods: Mods) {
        if !mods.primary && !mods.shift && self.completion_key(name) {
            return;
        }
        // Text editing keeps Cmd+A/C/X/V/Z while a field is being edited.
        let editing_key = self.ui.editing.is_some() && matches!(name, "a" | "c" | "x" | "v" | "z");
        if mods.primary && !editing_key && self.run_shortcut(name) {
            return;
        }
        if self.ui.editing.is_none() && !mods.primary && self.run_on_key(name) {
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
                let width = n.layout.text.as_ref().map_or(0.0, |t| t.width() as f64) + 2.0 * render::CODE_PAD_X;
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
            let matches = match &n.kind {
                Kind::Text(s) | Kind::Button { label: s, .. } | Kind::Toggle { label: s, .. } => s == label,
                Kind::TextField { placeholder, .. } => placeholder == label,
                Kind::Svg { icon: Some(name), .. } => *name == label,
                _ => false,
            };
            if matches && !n.mods.hidden {
                let r = n.layout.rect;
                return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
            }
            n.visible_children().iter().find_map(|c| visit(c, label))
        }
        visit(self.tree.as_ref()?, label)
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
    /// the source it is (see `Mods::source`; -1 when that isn't known, or
    /// when nothing of the view shows).
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
            };
            // What can be seen of it: the part inside the scroll views around it.
            let (x0, y0) = (x.max(clip.x), y.max(clip.y));
            let (x1, y1) = ((x + r.w).min(clip.x + clip.w), (y + r.h).min(clip.y + clip.h));
            let view = match n.mods.source {
                Some(view) if x1 > x0 && y1 > y0 => view as i64,
                _ => -1,
            };
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

fn run_script(app: App, script: &str) -> i32 {
    let mut app = app;
    let code = script_steps(&mut app, script);
    drop(app);
    code
}

fn script_steps(app: &mut App, script: &str) -> i32 {
    CLOCK.with(|c| *c.borrow_mut() = Clock::Virtual(0.0));
    let mut cx = RenderContext::new();
    let mut renderer: Option<Renderer> = None;
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
    let numbers = |text: &str| -> Vec<f64> { text.split_whitespace().filter_map(|x| x.parse().ok()).collect() };
    for cmd in commands.iter().map(String::as_str) {
        app.update();
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
                } else if let [kind @ ("toggle" | "button" | "field" | "text" | "editor"), n] = words[..] {
                    app.find_nth(kind, n.parse().unwrap_or(1))
                } else {
                    let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    (n.len() == 2).then(|| (n[0], n[1]))
                };
                match point {
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
                let n = numbers(rest);
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
                let n = numbers(rest);
                let key = match n[..] {
                    [x, y] => app.pointer_at(x, y),
                    _ => None,
                };
                let (Some(key), [x, y]) = (key, &n[..]) else {
                    eprintln!("script: no view with `.onPointer` for `{cmd}`");
                    return 2;
                };
                mouse = (*x, *y);
                app.ui.editing = None;
                app.pointer(key, DOWN, mouse, (0.0, 0.0), button, 1, mods);
                held = Some((key, button));
            }
            "move" => {
                let n = numbers(rest);
                let [x, y] = n[..] else {
                    eprintln!("script: write it as `move 80 60`");
                    return 2;
                };
                let delta = (x - mouse.0, y - mouse.1);
                mouse = (x, y);
                match held {
                    Some((key, button)) => app.pointer(key, DRAG, mouse, delta, button, 0, mods),
                    None => {
                        app.hover(x, y);
                        app.pointer_move(x, y, delta, mods);
                    }
                }
            }
            "release" => {
                if let [x, y] = numbers(rest)[..] {
                    mouse = (x, y);
                }
                if let Some((key, button)) = held.take() {
                    app.pointer(key, UP, mouse, (0.0, 0.0), button, 1, mods);
                }
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
                let mut name = rest;
                while let Some((m, tail)) = name.split_once('+') {
                    match m {
                        "shift" => mods.shift = true,
                        "alt" => mods.alt = true,
                        "cmd" | "ctrl" => mods.primary = true,
                        _ => {}
                    }
                    name = tail;
                }
                app.key(name, mods);
            }
            "drag" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
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
                let scene = app.scene(scale);
                let (w, h) = ((app.width * scale) as u32, (app.height * scale) as u32);
                render::render_png(&mut cx, &mut renderer, &scene, w, h, rest);
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
                print!("{}", app.dump(word == "dump"));
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

struct WindowApp {
    app: App,
    cx: RenderContext,
    renderer: Option<Renderer>,
    surface: Option<RenderSurface<'static>>,
    window: Option<Arc<Window>>,
    cursor: (f64, f64),
    mods: Mods,
    /// Dragging to select text in a field.
    selecting: bool,
    /// The view with `.onDrag` being dragged.
    dragging: Option<u64>,
    /// The scroll bar being dragged: its view, whether it's the one down
    /// the side, and where on its thumb it's held.
    scrolling: Option<(u64, bool, f64)>,
    /// The view with `.onPointer` the mouse is pressed on, and the button.
    pointing: Option<(u64, i32)>,
    /// The pointer that's showing (a resize one over a drag handle).
    pointer: winit::window::CursorIcon,
    /// For double and triple clicks: when and where the last click was, and the count.
    last_click: Option<(Instant, (f64, f64), u32)>,
    /// Whether input methods are on (they are while text is edited), and
    /// where the caret was last reported to them.
    ime: bool,
    ime_caret: Option<Rect>,
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
    let mut w = WindowApp {
        app,
        cx: RenderContext::new(),
        renderer: None,
        surface: None,
        window: None,
        cursor: (0.0, 0.0),
        mods: Mods::default(),
        selecting: false,
        dragging: None,
        scrolling: None,
        pointing: None,
        pointer: winit::window::CursorIcon::Default,
        last_click: None,
        ime: false,
        ime_caret: None,
    };
    w.app.ui.anim.enabled = true;
    let code = match event_loop.run_app(&mut w) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    w.app.tree = None;
    code
}

impl WindowApp {
    fn scale(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    fn redraw(&mut self) {
        let (Some(surface), Some(window)) = (self.surface.as_ref(), self.window.as_ref()) else { return };
        if let Some(title) = NEW_TITLE.with(|t| t.borrow_mut().take()) {
            window.set_title(&title);
            self.app.title = title;
        }
        let scale = window.scale_factor();
        let size = window.inner_size();
        self.app.width = size.width as f64 / scale;
        self.app.height = size.height as f64 / scale;
        let scene = self.app.scene(scale);
        let dev = &self.cx.devices[surface.dev_id];
        let renderer = self.renderer.get_or_insert_with(|| render::renderer(&dev.device));
        let (w, h) = (surface.config.width, surface.config.height);
        if renderer.render_to_texture(&dev.device, &dev.queue, &scene, &surface.target_view, &render::params(w, h)).is_err() {
            return;
        }
        let frame = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                // Outdated or lost: reconfigure and try on the next frame.
                self.cx.configure_surface(surface);
                self.request_redraw();
                return;
            }
        };
        let mut encoder = dev.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        surface.blitter.copy(&dev.device, &mut encoder, &surface.target_view, &view);
        dev.queue.submit([encoder.finish()]);
        frame.present();
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
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.app.title.clone())
            .with_inner_size(LogicalSize::new(self.app.width, self.app.height))
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
                event_loop.exit();
                return;
            }
        };
        if self.app.appearance == 0 && theme::set_dark(window.theme() == Some(winit::window::Theme::Dark)) {
            self.app.dirty = true;
        }
        let size = window.inner_size();
        let surface = pollster::block_on(self.cx.create_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ));
        match surface {
            Ok(s) => self.surface = Some(s),
            Err(e) => {
                eprintln!("error: can't draw to the window: {e}");
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        self.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if std::env::var_os("TESSEL_DEBUG_EVENTS").is_some() && !matches!(event, WindowEvent::RedrawRequested) {
            eprintln!("event: {event:?}");
        }
        // A click, a key or the wheel puts the tooltip away.
        if matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::KeyboardInput { .. } | WindowEvent::MouseWheel { .. }) {
            if self.app.ui.tooltip.is_some() {
                self.request_redraw();
            }
            self.app.tip_hide();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(s) = self.surface.as_mut() {
                    self.cx.resize_surface(s, size.width.max(1), size.height.max(1));
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
                self.mods = Mods { shift: m.shift_key(), alt: m.alt_key(), primary };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale();
                let before = self.cursor;
                self.cursor = (position.x / scale, position.y / scale);
                if let Some((key, vertical, grab)) = self.scrolling {
                    self.app.drag_scroll_bar(key, vertical, grab, self.cursor.0, self.cursor.1);
                    self.request_redraw();
                    return;
                }
                let delta = (self.cursor.0 - before.0, self.cursor.1 - before.1);
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
                self.app.ui.pointer = Some(self.cursor);
                let had_tip = self.app.ui.tooltip.is_some();
                self.app.tip_move(self.cursor.0, self.cursor.1);
                if had_tip && self.app.ui.tooltip.is_none() {
                    self.request_redraw();
                }
                if hovered != self.app.ui.hovered || hovered.is_some() || hover_bg != self.app.ui.hover_background {
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
                        if let Some(Hit::Drag(key)) = hit {
                            self.dragging = Some(key);
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
                    ElementState::Released => {
                        self.selecting = false;
                        self.dragging = None;
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
            WindowEvent::KeyboardInput { event: KeyEvent { state: ElementState::Pressed, logical_key, text, .. }, .. } => {
                let mods = self.mods;
                let named = match &logical_key {
                    Key::Named(NamedKey::Backspace) => Some("backspace"),
                    Key::Named(NamedKey::Delete) => Some("delete"),
                    Key::Named(NamedKey::ArrowLeft) => Some("left"),
                    Key::Named(NamedKey::ArrowRight) => Some("right"),
                    Key::Named(NamedKey::Home) => Some("home"),
                    Key::Named(NamedKey::End) => Some("end"),
                    Key::Named(NamedKey::Enter) => Some("enter"),
                    Key::Named(NamedKey::Escape) => Some("escape"),
                    Key::Named(NamedKey::ArrowUp) => Some("up"),
                    Key::Named(NamedKey::ArrowDown) => Some("down"),
                    Key::Named(NamedKey::Tab) => Some("tab"),
                    _ => None,
                };
                match (named, &logical_key) {
                    (Some(name), _) => self.app.key(name, mods),
                    (None, Key::Character(c)) if mods.primary => self.app.key(&c.to_lowercase(), mods),
                    _ => {
                        if let Some(t) = text.filter(|t| !t.chars().any(char::is_control)) {
                            self.app.type_text(&t);
                        }
                    }
                }
                self.request_redraw();
            }
            _ => {}
        }
        self.sync_ime();
        if self.app.dirty {
            self.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        if deliver_background() {
            self.app.dirty = true;
            self.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if deliver_background() {
            self.app.dirty = true;
            self.request_redraw();
        }
        if fire_timers() {
            self.app.dirty = true;
            self.request_redraw();
        }
        if self.app.tip_due() {
            self.request_redraw();
        }
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

/// The locator of an editable view's text.
fn editable_text(n: &Node) -> Option<&Callback> {
    match &n.kind {
        Kind::TextField { text, .. } | Kind::CodeEditor { text, .. } => Some(text),
        _ => None,
    }
}
