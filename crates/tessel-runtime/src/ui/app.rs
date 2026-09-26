//! The app: window, input events, timers, and the rebuild → layout → draw cycle.
//!
//! Setting `TESSEL_SCRIPT` runs the app without a window instead, driven by a
//! script (used by tests). Commands, separated by `;` or newlines:
//!
//! ```text
//! size 320 200          window size in points
//! click "Add"           click the button / toggle / text / field with this label
//! click 40 60           click at a point
//! type hello world      type into the focused text field
//! key backspace         backspace | enter | escape
//! scroll 0 120          scroll the view under the pointer by (dx, dy) points
//! wait 1.5              let 1.5 seconds pass (fires timers)
//! snapshot out.png      render to a PNG (at 2x)
//! dump                  print the laid-out view tree, with positions
//! tree                  print the view tree without positions
//! ```

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use vello::util::{RenderContext, RenderSurface};
use vello::{wgpu, Renderer, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use super::layout::{read_string, Layouter};
use super::render::{self, Interaction, Painter, TextEngine};
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
            self.ui.focused = None;
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
            (Hit::Field(key), _) => self.ui.focused = Some(*key),
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

    /// Changes the focused text field's contents.
    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        let Some(key) = self.ui.focused else { return };
        let Some(Kind::TextField { text, .. }) = self.node(key).map(|n| &n.kind) else { return };
        let mut s = read_string(text);
        change(&mut s);
        let slot = text.locate() as *mut *mut Str;
        unsafe {
            let old = *slot;
            *slot = crate::str_new(&s);
            crate::tsl_release(old as *mut Header);
        }
        self.dirty = true;
    }

    fn key(&mut self, name: &str) {
        match name {
            "backspace" => self.edit(|s| {
                s.pop();
            }),
            "enter" | "escape" => {
                self.ui.focused = None;
                self.dirty = true;
            }
            _ => {}
        }
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
    };
    let code = match std::env::var("TESSEL_SCRIPT") {
        Ok(script) => run_script(app, &script),
        Err(_) => run_window(app),
    };
    TIMERS.with(|t| t.borrow_mut().clear());
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
            "type" => app.edit(|s| s.push_str(rest)),
            "key" => app.key(rest),
            "scroll" => {
                let n: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [_, dy] = n[..] {
                    let (x, y) = (app.width / 2.0, app.height / 2.0);
                    app.scroll(x, y, dy);
                }
            }
            "wait" => {
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
}

fn run_window(app: App) -> i32 {
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("error: can't open a window: {e}");
            return 1;
        }
    };
    let mut w = WindowApp { app, cx: RenderContext::new(), renderer: None, surface: None, window: None, cursor: (0.0, 0.0) };
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
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale();
                self.cursor = (position.x / scale, position.y / scale);
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
                    ElementState::Pressed => self.app.ui.pressed = self.app.hit(x, y).map(|h| h.key()),
                    ElementState::Released => {
                        let pressed = self.app.ui.pressed.take();
                        let here = self.app.hit(x, y).map(|h| h.key());
                        if pressed.is_some() && pressed == here {
                            self.app.click(x, y);
                        } else if here.is_none() && self.app.ui.focused.is_some() {
                            self.app.ui.focused = None;
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
                match logical_key {
                    Key::Named(NamedKey::Backspace) => self.app.key("backspace"),
                    Key::Named(NamedKey::Enter) => self.app.key("enter"),
                    Key::Named(NamedKey::Escape) => self.app.key("escape"),
                    _ => {
                        if let Some(t) = text.filter(|t| !t.chars().any(char::is_control)) {
                            self.app.edit(|s| s.push_str(&t));
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
