//! The app: window, input events, timers, and the rebuild → layout → draw cycle.
//!
//! Setting `TESSEL_SCRIPT` runs the app without a window instead, driven by a
//! script (used by tests). Commands, separated by `;` or newlines:
//!
//! ```text
//! size 320 200          window size in points
//! click "Add"           click the button / toggle / text / field with this label
//! click 40 60           click at a point
//! type hello world      type into the text field being edited
//! type "two words "     quoted, to keep leading or trailing spaces
//! key backspace         a key, optionally with modifiers: `key shift+left`,
//!                       `key alt+backspace`, `key cmd+a` (cmd means Ctrl on
//!                       Windows). Keys: backspace delete left right home end
//!                       enter escape, and letters for shortcuts (a c x v)
//! scroll 0 120          scroll the view under the pointer by (dx, dy) points
//! wait 1.5              let 1.5 seconds pass (fires timers)
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
use super::render::{self, field_origin, Interaction, Painter, TextEngine};
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

// ===== fetch =====

/// Finished loads, handed from worker threads to the UI thread.
static FETCHED: Mutex<Vec<(u64, Option<String>)>> = Mutex::new(Vec::new());
/// Wakes the window's event loop when a load finishes.
static WAKE: Mutex<Option<EventLoopProxy<()>>> = Mutex::new(None);

thread_local! {
    /// Completion blocks waiting for their loads, by id.
    static WAITING: RefCell<HashMap<u64, Callback>> = RefCell::new(HashMap::new());
    static NEXT_FETCH: Cell<u64> = const { Cell::new(0) };
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
    let id = NEXT_FETCH.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    WAITING.with(|w| w.borrow_mut().insert(id, Callback::new(f, env)));
    std::thread::spawn(move || {
        let body = load(&url);
        FETCHED.lock().unwrap().push((id, body));
        if let Some(proxy) = WAKE.lock().unwrap().as_ref() {
            let _ = proxy.send_event(());
        }
    });
}

/// Runs completion blocks for finished loads. Returns whether any ran.
fn deliver_fetches() -> bool {
    let done: Vec<(u64, Option<String>)> = std::mem::take(&mut *FETCHED.lock().unwrap());
    let ran = !done.is_empty();
    for (id, body) in done {
        let Some(cb) = WAITING.with(|w| w.borrow_mut().remove(&id)) else { continue };
        let text = body.map_or(std::ptr::null_mut(), |b| crate::str_new(&b));
        cb.call_with(text as *mut u8);
    }
    ran
}

fn fetches_waiting() -> usize {
    WAITING.with(|w| w.borrow().len())
}

// ===== the app model =====

/// What a click landed on.
enum Hit {
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
        if self.dirty || self.tree.is_none() {
            self.dirty = false;
            self.tree = Some(build(&self.root));
        }
        let tree = self.tree.as_mut().unwrap();
        Layouter { text: &mut self.text, scroll: &self.ui.scroll }.run(tree, self.width, self.height);
        self.follow_caret();
    }

    /// Stops editing a field that disappeared, keeps the selection valid,
    /// and scrolls the field so the caret stays visible.
    fn follow_caret(&mut self) {
        let Some(e) = self.ui.editing.as_mut() else { return };
        let node = self.tree.as_ref().and_then(|t| t.find(e.key));
        let Some(node) = node.filter(|n| matches!(n.kind, Kind::TextField { .. })) else {
            self.ui.editing = None;
            return;
        };
        let Kind::TextField { text, .. } = &node.kind else { unreachable!() };
        e.clamp(&read_string(text));
        let Some(layout) = &node.layout.text else { return };
        let (caret, _) = edit::geometry(layout, e);
        let caret_x = caret.map_or(0.0, |c| c.x0);
        let visible = (node.layout.rect.w - 16.0).max(1.0);
        if caret_x - e.scroll > visible {
            e.scroll = caret_x - visible;
        } else if caret_x < e.scroll {
            e.scroll = caret_x;
        }
        e.scroll = e.scroll.min((layout.width() as f64 - visible).max(0.0)).max(0.0);
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
                Kind::TextField { .. } => Some(Hit::Field(n.key)),
                _ if n.mods.on_tap.is_some() => Some(Hit::Tap(n.key)),
                _ => None,
            }
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
            (Hit::Field(key), Kind::TextField { text, .. }) => {
                if self.ui.editing.as_ref().is_none_or(|e| e.key != *key) {
                    self.ui.editing = Some(Editing::new(*key, &read_string(text)));
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
        let (Kind::TextField { text, .. }, Some(layout)) = (&node.kind, &node.layout.text) else { return };
        let current = read_string(text);
        if let Some(new) = edit::apply(&current, layout, &mut e, change, &mut self.clipboard) {
            let slot = text.locate() as *mut *mut Str;
            unsafe {
                let old = *slot;
                *slot = crate::str_new(&new);
                crate::tsl_release(old as *mut Header);
            }
            self.dirty = true;
        }
        self.ui.editing = Some(e);
        // Lay out again so the caret position is current.
        if !self.dirty {
            self.follow_caret();
        }
    }

    /// A key press, by name (see the script commands).
    fn key(&mut self, name: &str, mods: Mods) {
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
            "a" if mods.primary => Edit::SelectAll,
            "c" if mods.primary => Edit::Copy,
            "x" if mods.primary => Edit::Cut,
            "v" if mods.primary => Edit::Paste,
            "enter" | "escape" => {
                self.ui.editing = None;
                self.dirty = true;
                return;
            }
            _ => return,
        };
        self.edit(change);
    }

    /// A mouse press on a text field: place the caret or select.
    fn press_field(&mut self, key: u64, x: f64, y: f64, shift: bool, clicks: u32) {
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(key)) else { return };
        let Kind::TextField { text, .. } = &node.kind else { return };
        let (x0, y0) = field_origin(node);
        let current = read_string(text);
        if self.ui.editing.as_ref().is_none_or(|e| e.key != key) {
            self.ui.editing = Some(Editing::new(key, &current));
        }
        let scroll = self.ui.editing.as_ref().map_or(0.0, |e| e.scroll);
        self.edit(Edit::Click { x: x - x0 + scroll, y: y - y0, extend: shift, clicks });
    }

    fn drag_field(&mut self, x: f64, y: f64) {
        let Some(key) = self.ui.editing.as_ref().map(|e| e.key) else { return };
        let Some(node) = self.tree.as_ref().and_then(|t| t.find(key)) else { return };
        let (x0, y0) = field_origin(node);
        let scroll = self.ui.editing.as_ref().map_or(0.0, |e| e.scroll);
        self.edit(Edit::Drag { x: x - x0 + scroll, y: y - y0 });
    }

    fn scroll(&mut self, x: f64, y: f64, dy: f64) {
        fn find(n: &Node, x: f64, y: f64) -> Option<u64> {
            if !n.layout.rect.contains(x, y) || n.mods.hidden {
                return None;
            }
            let inner = n.children.iter().rev().find_map(|c| find(c, x, y));
            inner.or(matches!(n.kind, Kind::Scroll).then_some(n.key))
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
            let matches = match &n.kind {
                Kind::Text(s) | Kind::Button { label: s, .. } | Kind::Toggle { label: s, .. } => s == label,
                Kind::TextField { placeholder, .. } => placeholder == label,
                _ => false,
            };
            if matches && !n.mods.hidden {
                let r = n.layout.rect;
                return Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
            }
            n.children.iter().find_map(|c| visit(c, label))
        }
        visit(self.tree.as_ref()?, label)
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
                Kind::VStack(_) => "VStack".to_string(),
                Kind::HStack(_) => "HStack".to_string(),
                Kind::ZStack => "ZStack".to_string(),
                Kind::Scroll => "Scroll".to_string(),
                Kind::Spacer => "Spacer".to_string(),
                Kind::Image(p) => format!("Image {p:?}"),
            };
            let flags = [(n.mods.hidden, " hidden"), (n.mods.disabled, " disabled")]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, s)| *s)
                .collect::<String>();
            let place = if positions { format!(" [{:.0},{:.0} {:.0}x{:.0}]", r.x, r.y, r.w, r.h) } else { String::new() };
            out.push_str(&format!("{}{what}{place}{flags}\n", "  ".repeat(depth)));
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
    for cmd in script.split([';', '\n']).map(str::trim).filter(|c| !c.is_empty()) {
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
                let point = if rest.starts_with('"') {
                    app.find_label(rest.trim_matches('"'))
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
            "type" => {
                let text = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(rest);
                app.edit(Edit::Insert(text.to_string()));
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
                // Loads in progress finish first (tests shouldn't depend on timing).
                let started = Instant::now();
                while fetches_waiting() > 0 && started.elapsed() < Duration::from_secs(30) {
                    if deliver_fetches() {
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
                if hovered != self.app.ui.hovered {
                    self.app.ui.hovered = hovered;
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.ui.hovered = None;
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
                    _ => None,
                };
                match (named, &logical_key) {
                    (Some(name), _) => self.app.key(name, mods),
                    (None, Key::Character(c)) if mods.primary => self.app.key(&c.to_lowercase(), mods),
                    _ => {
                        if let Some(t) = text.filter(|t| !t.chars().any(char::is_control)) {
                            self.app.edit(Edit::Insert(t.to_string()));
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
        if deliver_fetches() {
            self.app.dirty = true;
            self.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if deliver_fetches() {
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
