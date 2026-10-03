//! The UI runtime: a tree of views built by compiled code, laid out and
//! drawn by the runtime itself.
//!
//! Every time state changes, the program's build function runs again and
//! describes the whole UI through the `tsl_ui_*` calls below. The runtime
//! keeps what must survive rebuilds (view `state`, focus, scroll offsets)
//! keyed by each view's position in the tree.

mod anim;
mod app;
mod canvas;
mod draw;
mod edit;
mod highlight;
mod image;
mod layout;
mod markdown;
mod render;
mod svg;
mod text;

// The renderer: what draws, lays text out and reads pictures. On macOS and
// Windows the system's (`mac`, `win`), when the `native` feature is on;
// otherwise the GPU's.
#[cfg(all(feature = "gpu", not(all(feature = "native", any(target_os = "macos", windows)))))]
pub(crate) mod gpu;
#[cfg(all(feature = "gpu", not(all(feature = "native", any(target_os = "macos", windows)))))]
pub(crate) use gpu as backend;
#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) mod mac;
#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) use mac as backend;
#[cfg(all(feature = "native", windows))]
pub(crate) mod win;
#[cfg(all(feature = "native", windows))]
pub(crate) use win as backend;
#[cfg(all(feature = "native", any(target_os = "macos", windows)))]
mod svg_list;
mod theme;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};

use crate::{tsl_release, tsl_retain, Header, Str, TslType};

pub use app::{on_reload, run_on_ui, run_until_idle, tsl_ui_hot, tsl_ui_run};
pub use theme::tsl_is_dark_mode;

// ===== callbacks =====

/// A compiled closure `{ fn, env }`. Owns one reference to `env`.
pub struct Callback {
    f: *const c_void,
    env: *mut Header,
}

impl Callback {
    /// Takes ownership of one reference to `env`.
    pub(crate) fn new(f: *const c_void, env: *mut Header) -> Callback {
        Callback { f, env }
    }

    /// Runs an action: `void fn(env)`.
    pub fn call(&self) {
        let f: extern "C" fn(*mut Header) = unsafe { std::mem::transmute(self.f) };
        f(self.env);
    }

    /// Runs a closure taking one pointer argument: `void fn(env, ptr)`.
    pub fn call_with(&self, arg: *mut u8) {
        let f: extern "C" fn(*mut Header, *mut u8) = unsafe { std::mem::transmute(self.f) };
        f(self.env, arg);
    }

    /// Runs a closure taking one `Float`: `void fn(env, f64)`.
    pub fn call_with_float(&self, arg: f64) {
        let f: extern "C" fn(*mut Header, f64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, arg);
    }

    /// Runs a closure taking one `Int`: `void fn(env, i64)`.
    pub fn call_with_int(&self, arg: i64) {
        let f: extern "C" fn(*mut Header, i64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, arg);
    }

    /// Runs a closure taking two `Float`s: `void fn(env, f64, f64)`.
    pub fn call_with_floats(&self, a: f64, b: f64) {
        let f: extern "C" fn(*mut Header, f64, f64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, a, b);
    }

    /// Runs a locator: `ptr fn(env)`, giving a pointer to a value's storage.
    pub fn locate(&self) -> *mut u8 {
        let f: extern "C" fn(*mut Header) -> *mut u8 = unsafe { std::mem::transmute(self.f) };
        f(self.env)
    }
}

impl Clone for Callback {
    fn clone(&self) -> Callback {
        unsafe { tsl_retain(self.env) };
        Callback { f: self.f, env: self.env }
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        unsafe { tsl_release(self.env) };
    }
}

/// A copy of a value of any type (a Picker option's tag).
pub struct Value {
    pub bytes: Vec<u8>,
    ty: *const TslType,
}

impl Value {
    /// Copies the value at `p`, retaining it.
    unsafe fn copy_from(p: *const u8, ty: *const TslType) -> Value {
        let size = unsafe { (*ty).size } as usize;
        let mut bytes = unsafe { std::slice::from_raw_parts(p, size) }.to_vec();
        if let Some(retain) = unsafe { (*ty).retain } {
            unsafe { retain(bytes.as_mut_ptr()) };
        }
        Value { bytes, ty }
    }

    pub(crate) fn equals(&self, p: *const u8) -> bool {
        match unsafe { (*self.ty).eq } {
            Some(eq) => unsafe { eq(self.bytes.as_ptr(), p) },
            None => false,
        }
    }

    /// Replaces the value stored at `p` with a copy of this one.
    pub(crate) unsafe fn store_to(&self, p: *mut u8) {
        let ty = self.ty;
        unsafe {
            if let Some(release) = (*ty).release {
                release(p);
            }
            std::ptr::copy_nonoverlapping(self.bytes.as_ptr(), p, self.bytes.len());
            if let Some(retain) = (*ty).retain {
                retain(p);
            }
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        if let Some(release) = unsafe { (*self.ty).release } {
            unsafe { release(self.bytes.as_mut_ptr()) };
        }
    }
}

// ===== the view tree =====

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub [u8; 4]);

/// Font weights in the order of the `Weight` enum.
pub const WEIGHTS: [f32; 4] = [400.0, 500.0, 600.0, 700.0];

pub enum Kind {
    Root,
    /// A custom view's body; lays out like a `VStack`.
    Group,
    Text(String),
    Button { label: String, action: Callback },
    TextField { placeholder: String, text: Callback },
    Toggle { label: String, is_on: Callback },
    /// A multi-line code editor with line numbers and Tessel syntax colors.
    CodeEditor {
        text: Callback,
        error_lines: Vec<i64>,
        line: Option<Callback>,
        /// Suggestions for the word starting at byte `completion_start`.
        completions: Vec<Suggestion>,
        completion_start: i64,
        /// Asks for suggestions, with the caret's byte offset.
        on_complete: Option<Callback>,
    },
    /// Tabs: each child is a page, titled by `.tabItem` and identified by `.tag`.
    TabView { selection: Callback, close: Option<Callback> },
    /// A hairline separator across its stack.
    Divider,
    /// An SVG drawing: a built-in icon (with its name), `Svg("…")`, or an SVG file.
    Svg { source: std::rc::Rc<str>, icon: Option<&'static str> },
    Picker { selection: Callback },
    /// Spacing, and alignment across the stack (0 leading, 1 center, 2 trailing).
    VStack(f64, u8),
    HStack(f64, u8),
    /// Its `Alignment` tag.
    ZStack(u8),
    Scroll,
    Spacer,
    /// Text written in Markdown. Pictures are looked for in `folder`;
    /// `on_link` gets the address of a link that's clicked.
    Markdown { source: std::rc::Rc<str>, folder: String, on_link: Option<Callback> },
    /// A PNG or JPEG file; without a picture (a missing file), a placeholder box.
    Image { path: String, picture: Option<std::rc::Rc<image::Picture>> },
    /// A drawing of the program's own: `draw` is called with the canvas's
    /// width and height when it's painted. With `redraw`, the drawing is kept
    /// until that number changes.
    Canvas { draw: Callback, redraw: Option<i64> },
}

#[derive(Default)]
pub struct Mods {
    /// `.padding(…)`; repeated paddings add up.
    pub padding: layout::Insets,
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// `.frame(minWidth:minHeight:)`: never smaller than this.
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    /// `.frame(maxWidth:maxHeight:)`: grow to the space offered, up to this (may be infinite).
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
    /// `.textAlignment(…)`: how lines of text line up (0 leading, 1 center, 2 trailing); inherited.
    pub text_align: Option<u8>,
    /// `.frame(alignment:)`: where the content sits in the frame (an `Alignment` tag).
    pub frame_align: Option<u8>,
    pub font_size: Option<f64>,
    pub weight: Option<f32>,
    pub color: Option<Rgba>,
    pub background: Option<(Rgba, f64)>,
    pub border: Option<(Rgba, f64, f64)>,
    pub opacity: Option<f64>,
    pub hidden: bool,
    pub disabled: bool,
    pub on_tap: Option<Callback>,
    /// `.onDrag { dx, dy in … }`: called as the mouse moves with its button
    /// held down on the view, with how far it moved since the last call.
    pub on_drag: Option<Callback>,
    pub tag: Option<Value>,
    /// `.shortcut("s") { … }`: runs with Cmd+S (Ctrl+S on Windows).
    pub shortcut: Option<(String, Callback)>,
    /// `.onAppear { … }`: runs when the view first appears.
    pub on_appear: Option<Callback>,
    /// `.onSubmit { … }`: runs when Enter is pressed in a text field.
    pub on_submit: Option<Callback>,
    /// `.autofocus()`: a text field or code editor that starts editing when it appears.
    pub autofocus: bool,
    /// `.tabItem("Title")`: the title of a TabView page.
    pub tab_title: Option<String>,
    /// `.hoverBackground(color, radius:)`: drawn while the mouse is over the view.
    pub hover_background: Option<(Rgba, f64)>,
    /// `.animation(seconds)`: how long changes to the view and the views
    /// inside it take (0 for at once); inherited.
    pub animation: Option<f64>,
    /// `.onPointer { event in … }`: called with what the mouse does on the
    /// view (see `PointerC`).
    pub on_pointer: Option<Callback>,
    /// `.onKey { key in … }`: called with keys pressed while no text is edited.
    pub on_key: Option<Callback>,
    /// `.cursor(…)`: the pointer shown over the view (a `CursorStyle` tag).
    pub cursor: Option<u8>,
    /// `.offset(x:y:)`: the view (and what's in it) is shown this far from
    /// where layout put it, without moving anything else.
    pub offset: (f64, f64),
    /// `.clipped()`: nothing of the view's content shows outside its frame.
    pub clipped: bool,
    /// `.onDrop { path in … }`: called with each file dropped on the window.
    pub on_drop: Option<Callback>,
    /// `.tooltip("…")`: shown next to the pointer when it rests on the view.
    pub tooltip: Option<String>,
    /// `.onResize { width, height in … }`: called with the view's size
    /// (inside its padding) when it's first laid out, and when it changes.
    pub on_resize: Option<Callback>,
    /// Which view expression of the source this is, for `tessel preview`:
    /// its number among the ones the compiler listed.
    pub source: Option<u32>,
}

/// A `PointerEvent`, as the adapter the compiler generates reads it.
#[repr(C)]
pub struct PointerC {
    /// The `PointerKind` case: 0 down, 1 up, 2 move, 3 drag, 4 scroll, 5 pinch, 6 leave.
    pub kind: i32,
    /// The `PointerButton` case: 0 left, 1 right, 2 middle.
    pub button: i32,
    pub clicks: i64,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub shift: u8,
    pub alt: u8,
    pub command: u8,
}

pub struct Node {
    pub kind: Kind,
    pub children: Vec<Node>,
    pub mods: Mods,
    /// Identity across rebuilds: a hash of the path from the root.
    pub key: u64,
    /// Filled in by layout.
    pub layout: layout::Box,
}

impl Node {
    /// For a TabView: the index of the page whose tag equals the selection.
    pub fn selected_tab(&self) -> usize {
        let Kind::TabView { selection, .. } = &self.kind else { return 0 };
        let current = selection.locate();
        self.children.iter().position(|c| c.mods.tag.as_ref().is_some_and(|t| t.equals(current))).unwrap_or(0)
    }

    fn new(kind: Kind, key: u64) -> Node {
        Node { kind, children: Vec::new(), mods: Mods::default(), key, layout: layout::Box::default() }
    }

    /// Children that are showing: a tab view shows only its selected page.
    pub fn visible_children(&self) -> &[Node] {
        match self.kind {
            Kind::TabView { .. } => {
                let i = self.selected_tab();
                self.children.get(i..=i).unwrap_or(&[])
            }
            _ => &self.children,
        }
    }

    /// Finds a node by key.
    pub fn find(&self, key: u64) -> Option<&Node> {
        if self.key == key {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(key))
    }
}

/// Part of a view's identity: a different kind of view in the same position
/// is a different view (it doesn't inherit focus, undo history, …).
fn kind_salt(kind: &Kind) -> u64 {
    match kind {
        Kind::Root => 1,
        Kind::Group => 2,
        Kind::Text(_) => 3,
        Kind::Button { .. } => 4,
        Kind::TextField { .. } => 5,
        Kind::Toggle { .. } => 6,
        Kind::CodeEditor { .. } => 7,
        Kind::TabView { .. } => 8,
        Kind::Divider => 9,
        Kind::Picker { .. } => 10,
        Kind::VStack(..) => 11,
        Kind::HStack(..) => 12,
        Kind::ZStack(_) => 13,
        Kind::Scroll => 14,
        Kind::Spacer => 15,
        Kind::Image { .. } => 16,
        Kind::Svg { .. } => 17,
        Kind::Markdown { .. } => 18,
        Kind::Canvas { .. } => 19,
    }
}

fn child_key(parent: u64, index: usize, salt: u64) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (parent, index, salt).hash(&mut h);
    h.finish()
}

// ===== building =====

struct Builder {
    /// Open containers, innermost last. The root is at the bottom.
    stack: Vec<Node>,
    /// Set by `.id(…)`: the identity of the next view, instead of its position.
    pending_id: Option<u64>,
}

impl Builder {
    fn parent(&mut self) -> &mut Node {
        self.stack.last_mut().expect("no open view")
    }

    fn next_key(&mut self, salt: u64) -> u64 {
        if let Some(id) = self.pending_id.take() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.parent().key, "id", id, salt).hash(&mut h);
            return h.finish();
        }
        let p = self.parent();
        child_key(p.key, p.children.len(), salt)
    }

    fn leaf(&mut self, kind: Kind) {
        let key = self.next_key(kind_salt(&kind));
        self.parent().children.push(Node::new(kind, key));
    }

    fn begin(&mut self, kind: Kind, salt: u64) {
        let key = self.next_key(salt ^ kind_salt(&kind));
        self.stack.push(Node::new(kind, key));
    }

    fn end(&mut self) {
        let node = self.stack.pop().expect("unbalanced end");
        self.parent().children.push(node);
    }

    /// The most recently finished node, which modifiers apply to.
    fn last(&mut self) -> Option<&mut Node> {
        self.parent().children.last_mut()
    }
}

/// Persistent state of one custom view instance (a box of its `state` values).
struct ViewState {
    record: *mut Header,
    seen: u64,
    /// What the record's memory holds (see `tsl_ui_view_begin`).
    layout: u64,
}

thread_local! {
    static BUILDER: RefCell<Option<Builder>> = const { RefCell::new(None) };
    static STATES: RefCell<HashMap<u64, ViewState>> = RefCell::new(HashMap::new());
    static GENERATION: RefCell<u64> = const { RefCell::new(0) };
}

fn with_builder(f: impl FnOnce(&mut Builder)) {
    BUILDER.with(|b| {
        if let Some(b) = b.borrow_mut().as_mut() {
            f(b);
        }
    });
}

/// Runs the program's build function and returns the new tree. View states
/// that weren't used in this build (their views disappeared) are released.
pub fn build(root: &Callback) -> Node {
    let generation = GENERATION.with(|g| {
        *g.borrow_mut() += 1;
        *g.borrow()
    });
    BUILDER.with(|b| *b.borrow_mut() = Some(Builder { stack: vec![Node::new(Kind::Root, 1)], pending_id: None }));
    root.call();
    let mut builder = BUILDER.with(|b| b.borrow_mut().take()).expect("builder");
    while builder.stack.len() > 1 {
        builder.end();
    }
    let gone: Vec<*mut Header> = STATES.with(|s| {
        let mut s = s.borrow_mut();
        let dead: Vec<u64> = s.iter().filter(|(_, v)| v.seen != generation).map(|(k, _)| *k).collect();
        dead.into_iter().filter_map(|k| s.remove(&k)).map(|v| v.record).collect()
    });
    for record in gone {
        unsafe { tsl_release(record) };
    }
    builder.stack.pop().unwrap()
}

/// Releases the pictures, drawings and fonts kept for reuse. At exit this
/// must happen before the process ends: Windows may have unloaded the
/// libraries they belong to by the time thread-local values are dropped.
pub fn release_caches() {
    canvas::forget_all();
    image::forget_all();
}

/// Releases all view state (at exit, so leak checkers see a clean heap).
pub fn release_all_state() {
    release_caches();
    let records: Vec<*mut Header> = STATES.with(|s| s.borrow_mut().drain().map(|(_, v)| v.record).collect());
    for r in records {
        unsafe { tsl_release(r) };
    }
}

unsafe fn text_of(s: *const Str) -> String {
    unsafe { crate::str_text(s) }.to_string()
}

// ===== C ABI: views =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_text(s: *const Str) {
    let text = unsafe { text_of(s) };
    with_builder(|b| b.leaf(Kind::Text(text)));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_button(label: *const Str, f: *const c_void, env: *mut Header) {
    let label = unsafe { text_of(label) };
    let action = Callback::new(f, env);
    with_builder(|b| b.leaf(Kind::Button { label, action }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_text_field(placeholder: *const Str, f: *const c_void, env: *mut Header) {
    let placeholder = unsafe { text_of(placeholder) };
    let text = Callback::new(f, env);
    with_builder(|b| b.leaf(Kind::TextField { placeholder, text }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_toggle(label: *const Str, f: *const c_void, env: *mut Header) {
    let label = unsafe { text_of(label) };
    let is_on = Callback::new(f, env);
    with_builder(|b| b.leaf(Kind::Toggle { label, is_on }));
}

/// A `Completion` value (label, detail, kind).
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub detail: String,
    pub kind: String,
}

/// `CodeEditor(text:errorLines:line:completions:completionStart:onComplete:)`.
/// The lists are borrowed; `line_f` and `complete_f` are null when not given.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_code_editor(
    f: *const c_void,
    env: *mut Header,
    error_lines: *const crate::List,
    line_f: *const c_void,
    line_env: *mut Header,
    completions: *const crate::List,
    completion_start: i64,
    complete_f: *const c_void,
    complete_env: *mut Header,
) {
    let text = Callback::new(f, env);
    let line = (!line_f.is_null()).then(|| Callback::new(line_f, line_env));
    let on_complete = (!complete_f.is_null()).then(|| Callback::new(complete_f, complete_env));
    let n = unsafe { crate::tsl_list_len(error_lines) };
    let error_lines =
        (0..n).map(|i| unsafe { *(crate::tsl_list_get(error_lines, i, std::ptr::null()) as *const i64) }).collect();
    let n = unsafe { crate::tsl_list_len(completions) };
    let completions = (0..n)
        .map(|i| unsafe {
            // A `Completion` is three Strings.
            let p = crate::tsl_list_get(completions, i, std::ptr::null()) as *const *const Str;
            Suggestion { label: text_of(*p), detail: text_of(*p.add(1)), kind: text_of(*p.add(2)) }
        })
        .collect();
    with_builder(|b| {
        b.leaf(Kind::CodeEditor { text, error_lines, line, completions, completion_start, on_complete })
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_spacer() {
    with_builder(|b| b.leaf(Kind::Spacer));
}

/// `Image("path")`: an SVG, PNG or JPEG file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_image(path: *const Str) {
    let path = unsafe { text_of(path) };
    let svg = path.to_lowercase().ends_with(".svg").then(|| std::fs::read_to_string(&path).ok()).flatten();
    svg::enable();
    image::enable();
    let kind = match svg {
        Some(source) => Kind::Svg { source: source.into(), icon: None },
        None => {
            let picture = image::load(&path);
            Kind::Image { path, picture }
        }
    };
    with_builder(|b| b.leaf(kind));
}

/// `Markdown(text, folder:onLink:)`. `f` is null when there's no `onLink`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_markdown(source: *const Str, folder: *const Str, f: *const c_void, env: *mut Header) {
    let (source, folder) = unsafe { (text_of(source), text_of(folder)) };
    let on_link = (!f.is_null()).then(|| Callback::new(f, env));
    markdown::enable();
    with_builder(|b| b.leaf(Kind::Markdown { source: source.into(), folder, on_link }));
}

/// `Icon(.name)`: a built-in icon, by its index in `IconName`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_icon(index: i32) {
    svg::enable_icons();
    let Some(&(name, source)) = svg::ICONS.get(index as usize) else { return };
    with_builder(|b| b.leaf(Kind::Svg { source: source.into(), icon: Some(name) }));
}

/// `Svg("<svg …>")`: SVG source written in the program.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_svg(source: *const Str) {
    let source = unsafe { text_of(source) };
    svg::enable();
    with_builder(|b| b.leaf(Kind::Svg { source: source.into(), icon: None }));
}

/// `Canvas(redraw:) { g in … }`: `f` is called as `f(env, width, height)`
/// each time the canvas is painted (or, with `redraw`, when it changed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_canvas(f: *const c_void, env: *mut Header, has_redraw: u8, redraw: i64) {
    let draw = Callback::new(f, env);
    let redraw = (has_redraw != 0).then_some(redraw);
    with_builder(|b| b.leaf(Kind::Canvas { draw, redraw }));
}

/// An `Alignment` tag (the compiler's `ALIGNMENTS` order) as horizontal and
/// vertical positions: 0 start, 1 center, 2 end.
pub fn align_2d(tag: u8) -> (u8, u8) {
    match tag {
        0 => (0, 1), // leading
        2 => (2, 1), // trailing
        3 => (1, 0), // top
        4 => (1, 2), // bottom
        5 => (0, 0), // topLeading
        6 => (2, 0), // topTrailing
        7 => (0, 2), // bottomLeading
        8 => (2, 2), // bottomTrailing
        _ => (1, 1), // center
    }
}

/// Opens a container: 0 = VStack, 1 = HStack, 2 = ZStack, 3 = Scroll.
/// `align` is an `Alignment` tag.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin(kind: i32, spacing: f64, align: i32) {
    let align = align.clamp(0, 8) as u8;
    let (h, v) = align_2d(align);
    let kind = match kind {
        // Stacks store the position across their axis: 0 start, 1 center, 2 end.
        0 => Kind::VStack(spacing, h),
        // In an HStack, `.leading`/`.trailing` mean top/bottom.
        1 => Kind::HStack(spacing, if align == 0 || align == 2 { align } else { v }),
        2 => Kind::ZStack(align),
        _ => Kind::Scroll,
    };
    with_builder(|b| b.begin(kind, 0));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_picker(f: *const c_void, env: *mut Header) {
    let selection = Callback::new(f, env);
    with_builder(|b| b.begin(Kind::Picker { selection }, 0));
}

/// Opens a TabView. `close_f` is null when tabs can't be closed; otherwise
/// it's called as `close_f(close_env, pointer to the closed tab's tag)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_tabs(f: *const c_void, env: *mut Header, close_f: *const c_void, close_env: *mut Header) {
    let selection = Callback::new(f, env);
    let close = (!close_f.is_null()).then(|| Callback::new(close_f, close_env));
    // (The close buttons are an icon.)
    if close.is_some() {
        svg::enable_icons();
    }
    with_builder(|b| b.begin(Kind::TabView { selection, close }, 0));
}

/// `.textAlignment(…)`: only the horizontal part of the `Alignment` counts.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_text_alignment(align: i32) {
    let (h, _) = align_2d(align.clamp(0, 8) as u8);
    modify(|m| m.text_align = Some(h));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_animation(seconds: f64) {
    modify(|m| m.animation = Some(seconds.max(0.0)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_autofocus() {
    modify(|m| m.autofocus = true);
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_divider() {
    with_builder(|b| b.leaf(Kind::Divider));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_end() {
    with_builder(|b| b.end());
}

/// Opens a custom view instance. Returns its persistent state record (the
/// data of a box of type `ty`), and sets `*fresh` if it was just created and
/// must be initialized. `ty` may be null for views without state.
///
/// `layout` stands for what the record's memory holds (its values' names and
/// types, all the way down). It only ever differs after a hot reload: when
/// the new code keeps other state in the view, the view starts afresh,
/// rather than read the old record as something it isn't.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_view_begin(view_id: u64, ty: *const TslType, layout: u64, fresh: *mut u8) -> *mut u8 {
    let mut key = 0;
    with_builder(|b| {
        b.begin(Kind::Group, view_id);
        key = b.parent().key;
    });
    unsafe { *fresh = 0 };
    if ty.is_null() {
        return std::ptr::null_mut();
    }
    let generation = GENERATION.with(|g| *g.borrow());
    let stale = STATES.with(|s| {
        let mut s = s.borrow_mut();
        match s.get(&key) {
            Some(v) if v.layout != layout => s.remove(&key).map(|v| v.record),
            _ => None,
        }
    });
    if let Some(record) = stale {
        unsafe { tsl_release(record) };
    }
    let record = STATES.with(|s| {
        let mut s = s.borrow_mut();
        let entry = s.entry(key).or_insert_with(|| {
            unsafe { *fresh = 1 };
            ViewState { record: unsafe { crate::tsl_box_new(ty) }, seen: generation, layout }
        });
        entry.seen = generation;
        entry.record
    });
    unsafe { (record as *mut u8).add(crate::BOX_DATA) }
}

/// The box holding a view's state record, so closures can keep it alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_record_box(data: *mut u8) -> *mut Header {
    if data.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { data.sub(crate::BOX_DATA) as *mut Header }
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_view_end() {
    with_builder(|b| b.end());
}

/// `.id(…)`: the next view is identified by `id` rather than its position,
/// so its state follows it when a list is reordered.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_next_id(id: u64) {
    with_builder(|b| b.pending_id = Some(id));
}

// ===== C ABI: modifiers (apply to the last finished view) =====

fn modify(f: impl FnOnce(&mut Mods)) {
    with_builder(|b| {
        if let Some(n) = b.last() {
            f(&mut n.mods);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_padding(top: f64, leading: f64, bottom: f64, trailing: f64) {
    modify(|m| {
        let p = &mut m.padding;
        p.top += top;
        p.leading += leading;
        p.bottom += bottom;
        p.trailing += trailing;
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_font(size: f64, has_size: u8, weight: i32, has_weight: u8) {
    modify(|m| {
        if has_size != 0 {
            m.font_size = Some(size);
        }
        if has_weight != 0 {
            m.weight = Some(WEIGHTS[weight as usize]);
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_color(color: i32, values: *const Header) {
    let c = unsafe { theme::color(color, values) };
    modify(|m| m.color = Some(c));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_background(color: i32, values: *const Header, radius: f64) {
    let c = unsafe { theme::color(color, values) };
    modify(|m| m.background = Some((c, radius)));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_border(color: i32, values: *const Header, width: f64, radius: f64) {
    let c = unsafe { theme::color(color, values) };
    modify(|m| m.border = Some((c, width, radius)));
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn tsl_ui_frame(
    has_width: u8,
    width: f64,
    has_height: u8,
    height: f64,
    has_min_width: u8,
    min_width: f64,
    has_max_width: u8,
    max_width: f64,
    has_min_height: u8,
    min_height: f64,
    has_max_height: u8,
    max_height: f64,
    has_align: u8,
    align: i32,
) {
    modify(|m| {
        if has_min_width != 0 {
            m.min_width = Some(min_width.max(0.0));
        }
        if has_min_height != 0 {
            m.min_height = Some(min_height.max(0.0));
        }
        if has_max_width != 0 {
            m.max_width = Some(max_width.max(0.0));
        }
        if has_max_height != 0 {
            m.max_height = Some(max_height.max(0.0));
        }
        if has_align != 0 {
            m.frame_align = Some(align.clamp(0, 8) as u8);
        }
        if has_width != 0 {
            m.width = Some(width);
        }
        if has_height != 0 {
            m.height = Some(height);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_opacity(value: f64) {
    modify(|m| m.opacity = Some(value.clamp(0.0, 1.0)));
}

/// Says where the last view is written (see `Mods::source`). Only the code
/// `tessel preview` compiles calls this.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_source(view: u32) {
    modify(|m| m.source = Some(view));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_hidden(hidden: u8) {
    modify(|m| m.hidden = hidden != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_disabled(disabled: u8) {
    modify(|m| m.disabled = disabled != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_tap(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_tap = Some(cb));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_drag(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_drag = Some(cb));
}

/// `.onPointer { event in … }`: `f` is called as `f(env, &PointerC)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_pointer(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_pointer = Some(cb));
}

/// `.onKey { key in … }`: `f` is called as `f(env, String)` (the string is the callee's).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_key(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_key = Some(cb));
}

/// `.offset(x:y:)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_offset(x: f64, y: f64) {
    modify(|m| m.offset = (if x.is_finite() { x } else { 0.0 }, if y.is_finite() { y } else { 0.0 }));
}

/// `.clipped()`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_clipped(clipped: u8) {
    modify(|m| m.clipped = clipped != 0);
}

/// `.onDrop { path in … }`: `f` is called as `f(env, String)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_drop(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_drop = Some(cb));
}

/// `.onResize { width, height in … }`: `f` is called as `f(env, width, height)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_resize(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_resize = Some(cb));
}

/// `.tooltip("…")`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_tooltip(text: *const Str) {
    let text = unsafe { text_of(text) };
    modify(|m| m.tooltip = (!text.is_empty()).then_some(text));
}

/// `.cursor(…)`: a `CursorStyle` case's tag.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_cursor(style: i32) {
    modify(|m| m.cursor = Some(style.clamp(0, 255) as u8));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_shortcut(key: *const Str, f: *const c_void, env: *mut Header) {
    let key = unsafe { text_of(key) }.to_lowercase();
    let cb = Callback::new(f, env);
    modify(|m| m.shortcut = Some((key, cb)));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_submit(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_submit = Some(cb));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_tab_item(title: *const Str) {
    let title = unsafe { text_of(title) };
    modify(|m| m.tab_title = Some(title));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_hover_background(color: i32, values: *const Header, radius: f64) {
    let c = unsafe { theme::color(color, values) };
    modify(|m| m.hover_background = Some((c, radius)));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_appear(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_appear = Some(cb));
}

/// Tags the last view with a copy of the value at `value` (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_tag(value: *const u8, ty: *const TslType) {
    let v = unsafe { Value::copy_from(value, ty) };
    modify(|m| m.tag = Some(v));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::str_new;

    extern "C" fn nothing(_env: *mut Header) {}

    static mut FLAG: u8 = 1;
    static mut NAME: *mut Str = std::ptr::null_mut();

    extern "C" fn flag(_env: *mut Header) -> *mut u8 {
        &raw mut FLAG
    }

    extern "C" fn name(_env: *mut Header) -> *mut u8 {
        unsafe {
            if NAME.is_null() {
                NAME = str_new("");
            }
            &raw mut NAME as *mut u8
        }
    }

    extern "C" fn counter_ui(_env: *mut Header) {
        let f = nothing as *const c_void;
        unsafe {
            tsl_ui_begin(0, 12.0, 1);
            tsl_ui_text(str_new("Count: 3"));
            tsl_ui_font(32.0, 1, 3, 1);
            tsl_ui_begin(1, 8.0, 1);
            tsl_ui_button(str_new("-"), f, std::ptr::null_mut());
            tsl_ui_button(str_new("+"), f, std::ptr::null_mut());
            tsl_ui_end();
            tsl_ui_text_field(str_new("Your name"), name as *const c_void, std::ptr::null_mut());
            tsl_ui_toggle(str_new("Enabled"), flag as *const c_void, std::ptr::null_mut());
            tsl_ui_end();
            tsl_ui_padding(24.0, 24.0, 24.0, 24.0);
        }
    }

    #[test]
    // Draws with the GPU. GitHub's Windows machines have none (the fallback
    // crashes), so this runs where CI has one; UI behavior is tested elsewhere
    // through the view tree, which needs no GPU.
    #[cfg_attr(not(target_os = "macos"), ignore = "needs a GPU")]
    fn renders_a_counter() {
        let root = Callback::new(counter_ui as *const c_void, std::ptr::null_mut());
        let mut tree = build(&root);
        let mut text = render::TextEngine::new();
        let scroll = HashMap::new();
        layout::Layouter { text: &mut text, scroll: &scroll, composing: None }.run(&mut tree, 320.0, 260.0);
        let vstack = &tree.children[0];
        assert!(matches!(vstack.kind, Kind::VStack(..)));
        // Centered horizontally in the window, and the text field stretches.
        let r = vstack.layout.rect;
        assert!((r.x + r.w / 2.0 - 160.0).abs() < 1.0, "{r:?}");
        let field = &vstack.children[2];
        assert!((field.layout.rect.w - (320.0 - 48.0)).abs() < 1.0, "{:?}", field.layout.rect);

        let mut list = draw::DrawList::new();
        let ui = render::Interaction::default();
        render::Painter { scene: &mut list, ui: &ui, scale: 2.0, offset: (0.0, 0.0), clip: None }.paint(&tree);
        let path = std::env::temp_dir().join("tessel-runtime-counter.png");
        let pixels = backend::Offscreen::default().rgba(&list, 640, 520, Some(theme::theme().colors[theme::BACKGROUND]));
        assert!(backend::save_rgba(path.to_str().unwrap(), 640, 520, pixels));
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    }
}
