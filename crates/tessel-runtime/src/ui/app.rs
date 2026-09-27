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
//! scroll 0 120          scroll the view under the pointer by (dx, dy) points
//! wait 1.5              let 1.5 seconds pass (fires timers)
//! repeat 20 key down    run a command several times
//! snapshot out.png      render to a PNG (at 2x)
//! dump                  print the laid-out view tree, with positions
//! tree                  print the view tree without positions
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vello::util::{RenderContext, RenderSurface};
use vello::{wgpu, Renderer, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use super::edit::{self, Clipboard, Edit, Editing, Unit};
use super::layout::{read_string, Layouter};
use super::render::{self, code_origin, field_origin, Interaction, Painter, TextEngine};
use super::{build, Callback, Kind, Node};
use crate::{Header, Str};

// ===== timers =====

struct Timer {
    due: f64,
    action: Callback,
}

thread_local! {
    static TIMERS: RefCell<Vec<Timer>> = const { RefCell::new(Vec::new()) };
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
    TIMERS.with(|t| t.borrow_mut().push(Timer { due, action }));
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
    for timer in due {
        timer.action.call();
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
        }
    }
    ran
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
        path.map(|p| {
            let mut p = p.to_string_lossy().into_owned();
            // Saving: add the first file type's extension if it's missing.
            if save != 0 {
                if let Some(ext) = types.first() {
                    if !p.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
                        p = format!("{p}.{ext}");
                    }
                }
            }
            p
        })
    };
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
    WAITING.with(|w| !w.borrow().is_empty()) || COMMANDS.with(|c| !c.borrow().is_empty())
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
}

impl Hit {
    fn key(&self) -> u64 {
        match *self {
            Hit::Button(k) | Hit::Toggle(k) | Hit::Field(k) | Hit::Segment(k, _) | Hit::Tap(k) => k,
            Hit::Tab(k, _) | Hit::TabClose(k, _) => k,
        }
    }
}

struct App {
    root: Callback,
    title: String,
    width: f64,
    height: f64,
    tree: Option<Node>,
    text: TextEngine,
    ui: Interaction,
    dirty: bool,
    clipboard: Clipboard,
    /// Keys of the views in the current tree, to find ones that just appeared.
    keys: std::collections::HashSet<u64>,
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
        for _ in 0..8 {
            if !(self.dirty || self.tree.is_none()) {
                break;
            }
            self.dirty = false;
            self.tree = Some(build(&self.root));
            self.run_on_appear();
            self.sync_editor_lines();
        }
        let tree = self.tree.as_mut().unwrap();
        Layouter { text: &mut self.text, scroll: &self.ui.scroll }.run(tree, self.width, self.height);
        self.follow_caret();
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
        let (caret, _) = edit::geometry(layout, e);
        let content = node.layout.rect.inset_by(node.mods.padding);
        // Scroll so the caret stays visible.
        let keep = |offset: &mut f64, lo: f64, hi: f64, visible: f64, total: f64| {
            if hi - *offset > visible {
                *offset = hi - visible;
            } else if lo < *offset {
                *offset = lo;
            }
            *offset = offset.min((total - visible).max(0.0)).max(0.0);
        };
        match node.kind {
            Kind::CodeEditor { .. } => {
                let (cx0, cx1, cy0, cy1) = caret.map_or((0.0, 0.0, 0.0, 16.0), |c| (c.x0, c.x1, c.y0, c.y1));
                let visible_w = (content.w - node.layout.gutter - 16.0).max(1.0);
                let visible_h = (content.h - 12.0).max(1.0);
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
    }

    fn scene(&mut self, scale: f64) -> Scene {
        self.update();
        let mut scene = Scene::new();
        let tree = self.tree.as_ref().unwrap();
        Painter { scene: &mut scene, ui: &self.ui, scale }.paint(tree);
        scene
    }

    fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        fn visit(n: &Node, x: f64, y: f64) -> Option<Hit> {
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
            match n.kind {
                Kind::Button { .. } => Some(Hit::Button(n.key)),
                Kind::Toggle { .. } => Some(Hit::Toggle(n.key)),
                Kind::TextField { .. } | Kind::CodeEditor { .. } => Some(Hit::Field(n.key)),
                _ if n.mods.on_tap.is_some() => Some(Hit::Tap(n.key)),
                _ => None,
            }
        }
        visit(self.tree.as_ref()?, x, y)
    }

    /// Moves the pointer to a point (as the window does on mouse moves).
    pub fn hover(&mut self, x: f64, y: f64) {
        self.ui.pointer = Some((x, y));
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

    /// A completed click at (x, y).
    fn click(&mut self, x: f64, y: f64) {
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
        // Text editing keeps Cmd+A/C/X/V/Z while a field is being edited.
        let editing_key = self.ui.editing.is_some() && matches!(name, "a" | "c" | "x" | "v" | "z");
        if mods.primary && !editing_key && self.run_shortcut(name) {
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

    fn scroll(&mut self, x: f64, y: f64, dy: f64) {
        fn find(n: &Node, x: f64, y: f64) -> Option<u64> {
            if !n.layout.rect.contains(x, y) || n.mods.hidden {
                return None;
            }
            let inner = n.children.iter().rev().find_map(|c| find(c, x, y));
            inner.or(matches!(n.kind, Kind::Scroll | Kind::CodeEditor { .. }).then_some(n.key))
        }
        let Some(key) = self.tree.as_ref().and_then(|t| find(t, x, y)) else { return };
        let offset = self.ui.scroll.entry(key).or_insert(0.0);
        *offset = (*offset + dy).max(0.0);
        if let Some(n) = self.node(key) {
            let max = (n.layout.content_h - n.layout.rect.h).max(0.0);
            let offset = self.ui.scroll.get_mut(&key).unwrap();
            *offset = offset.min(max);
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
        fn visit(n: &Node, depth: usize, out: &mut String, positions: bool) {
            let r = n.layout.rect;
            let what = match &n.kind {
                Kind::Root => "Root".to_string(),
                Kind::Group => "View".to_string(),
                Kind::Text(s) => format!("Text {s:?}"),
                Kind::Button { label, .. } => format!("Button {label:?}"),
                Kind::TextField { text, placeholder } => {
                    format!("TextField {placeholder:?} = {:?}", read_string(text))
                }
                Kind::Toggle { label, is_on } => format!("Toggle {label:?} = {}", unsafe { *is_on.locate() } & 1 != 0),
                Kind::Picker { .. } => "Picker".to_string(),
                Kind::VStack(..) => "VStack".to_string(),
                Kind::HStack(..) => "HStack".to_string(),
                Kind::ZStack(_) => "ZStack".to_string(),
                Kind::Scroll => "Scroll".to_string(),
                Kind::Spacer => "Spacer".to_string(),
                Kind::Image(p) => format!("Image {p:?}"),
                Kind::TabView { .. } => {
                    let titles: Vec<&str> = n.children.iter().map(|c| c.mods.tab_title.as_deref().unwrap_or("")).collect();
                    format!("TabView {titles:?} selected {}", n.selected_tab())
                }
                Kind::Divider => "Divider".to_string(),
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
                    visit(c, depth + 1, out, positions);
                }
                return;
            }
            for c in &n.children {
                visit(c, depth + 1, out, positions);
            }
        }
        let mut out = String::new();
        if let Some(t) = &self.tree {
            visit(t, 0, &mut out, positions);
        }
        out
    }
}

// ===== entry point =====

/// Starts the app: opens a window (or runs `TESSEL_SCRIPT` without one) and
/// calls `build` (a `{f, env}` closure; ownership is taken) whenever the UI
/// needs to be described again. Never returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_run(title: *const Str, width: f64, height: f64, f: *const c_void, env: *mut Header) -> ! {
    let title = unsafe { crate::str_text(title) }.to_string();
    let app = App {
        root: Callback::new(f, env),
        title,
        width,
        height,
        tree: None,
        text: TextEngine::new(),
        ui: Interaction::default(),
        dirty: true,
        clipboard: Clipboard::Memory(String::new()),
        keys: Default::default(),
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
                    Some((x, y)) => app.click(x, y),
                    None => {
                        eprintln!("script: nothing to click for `{rest}`");
                        return 2;
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
                    Some((x, y)) => app.hover(x, y),
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
            "scroll" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [_, dy] = n[..] {
                    let (x, y) = (app.width / 2.0, app.height / 2.0);
                    app.scroll(x, y, dy);
                }
            }
            "wait" => {
                // Loads and commands in progress finish first (tests shouldn't depend on timing).
                let started = Instant::now();
                while background_busy() && started.elapsed() < Duration::from_secs(60) {
                    if deliver_background() {
                        app.dirty = true;
                        app.update();
                    } else {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                let secs: f64 = rest.parse().unwrap_or(0.0);
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
            }
            "snapshot" => {
                let scale = 2.0;
                let scene = app.scene(scale);
                let (w, h) = ((app.width * scale) as u32, (app.height * scale) as u32);
                render::render_png(&mut cx, &mut renderer, &scene, w, h, rest);
            }
            "dump" | "tree" => {
                app.update();
                print!("{}", app.dump(word == "dump"));
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
    /// For double and triple clicks: when and where the last click was, and the count.
    last_click: Option<(Instant, (f64, f64), u32)>,
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
        last_click: None,
    };
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
            .with_inner_size(LogicalSize::new(self.app.width, self.app.height));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("error: can't open a window: {e}");
                event_loop.exit();
                return;
            }
        };
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
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(s) = self.surface.as_mut() {
                    self.cx.resize_surface(s, size.width.max(1), size.height.max(1));
                }
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => {
                let m = m.state();
                let primary = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
                self.mods = Mods { shift: m.shift_key(), alt: m.alt_key(), primary };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale();
                self.cursor = (position.x / scale, position.y / scale);
                if self.selecting {
                    self.app.drag_field(self.cursor.0, self.cursor.1);
                    self.request_redraw();
                }
                let hovered = self.app.hit(self.cursor.0, self.cursor.1).map(|h| h.key());
                let hover_bg = self.app.hover_background_at(self.cursor.0, self.cursor.1);
                self.app.ui.pointer = Some(self.cursor);
                if hovered != self.app.ui.hovered || hovered.is_some() || hover_bg != self.app.ui.hover_background {
                    self.app.ui.hovered = hovered;
                    self.app.ui.hover_background = hover_bg;
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.ui.hovered = None;
                self.app.ui.hover_background = None;
                self.app.ui.pointer = None;
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let (x, y) = self.cursor;
                match state {
                    ElementState::Pressed => {
                        let hit = self.app.hit(x, y);
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
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y as f64 * 40.0,
                    MouseScrollDelta::PixelDelta(p) => -p.y / self.scale(),
                };
                self.app.scroll(self.cursor.0, self.cursor.1, dy);
                self.request_redraw();
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
        match next_timer() {
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
