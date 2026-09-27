//! The UI runtime: a tree of views built by compiled code, laid out and
//! drawn by the runtime itself.
//!
//! Every time state changes, the program's build function runs again and
//! describes the whole UI through the `tsl_ui_*` calls below. The runtime
//! keeps what must survive rebuilds (view `state`, focus, scroll offsets)
//! keyed by each view's position in the tree.

mod app;
mod edit;
mod layout;
mod render;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};

use crate::{tsl_release, tsl_retain, Header, Str, TslType};

pub use app::tsl_ui_run;

// ===== callbacks =====

/// A compiled closure `{ fn, env }`. Owns one reference to `env`.
pub struct Callback {
    f: *const c_void,
    env: *mut Header,
}

impl Callback {
    /// Takes ownership of one reference to `env`.
    fn new(f: *const c_void, env: *mut Header) -> Callback {
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

    /// Runs a closure taking one `Int`: `void fn(env, i64)`.
    pub fn call_with_int(&self, arg: i64) {
        let f: extern "C" fn(*mut Header, i64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, arg);
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
    bytes: Vec<u8>,
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

    fn equals(&self, p: *const u8) -> bool {
        match unsafe { (*self.ty).eq } {
            Some(eq) => unsafe { eq(self.bytes.as_ptr(), p) },
            None => false,
        }
    }

    /// Replaces the value stored at `p` with a copy of this one.
    unsafe fn store_to(&self, p: *mut u8) {
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

/// Colors in the order of the `Color` enum.
pub const COLORS: [Rgba; 13] = [
    Rgba([29, 29, 31, 255]),    // primary
    Rgba([110, 110, 115, 255]), // secondary
    Rgba([142, 142, 147, 255]), // gray
    Rgba([255, 59, 48, 255]),   // red
    Rgba([255, 149, 0, 255]),   // orange
    Rgba([255, 204, 0, 255]),   // yellow
    Rgba([52, 199, 89, 255]),   // green
    Rgba([0, 122, 255, 255]),   // blue
    Rgba([175, 82, 222, 255]),  // purple
    Rgba([255, 45, 85, 255]),   // pink
    Rgba([255, 255, 255, 255]), // white
    Rgba([0, 0, 0, 255]),       // black
    Rgba([0, 0, 0, 0]),         // clear
];

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
    Picker { selection: Callback },
    VStack(f64),
    HStack(f64),
    ZStack,
    Scroll,
    Spacer,
    Image(String),
}

#[derive(Default)]
pub struct Mods {
    pub padding: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub font_size: Option<f64>,
    pub weight: Option<f32>,
    pub color: Option<Rgba>,
    pub background: Option<(Rgba, f64)>,
    pub border: Option<(Rgba, f64, f64)>,
    pub opacity: Option<f64>,
    pub hidden: bool,
    pub disabled: bool,
    pub on_tap: Option<Callback>,
    pub tag: Option<Value>,
    /// `.shortcut("s") { … }`: runs with Cmd+S (Ctrl+S on Windows).
    pub shortcut: Option<(String, Callback)>,
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
    fn new(kind: Kind, key: u64) -> Node {
        Node { kind, children: Vec::new(), mods: Mods::default(), key, layout: layout::Box::default() }
    }

    /// Finds a node by key.
    pub fn find(&self, key: u64) -> Option<&Node> {
        if self.key == key {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(key))
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
        let key = self.next_key(0);
        self.parent().children.push(Node::new(kind, key));
    }

    fn begin(&mut self, kind: Kind, salt: u64) {
        let key = self.next_key(salt);
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

/// Releases all view state (at exit, so leak checkers see a clean heap).
pub fn release_all_state() {
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

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_spacer() {
    with_builder(|b| b.leaf(Kind::Spacer));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_image(path: *const Str) {
    let path = unsafe { text_of(path) };
    with_builder(|b| b.leaf(Kind::Image(path)));
}

/// Opens a container: 0 = VStack, 1 = HStack, 2 = ZStack, 3 = Scroll.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin(kind: i32, spacing: f64) {
    let kind = match kind {
        0 => Kind::VStack(spacing),
        1 => Kind::HStack(spacing),
        2 => Kind::ZStack,
        _ => Kind::Scroll,
    };
    with_builder(|b| b.begin(kind, 0));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_picker(f: *const c_void, env: *mut Header) {
    let selection = Callback::new(f, env);
    with_builder(|b| b.begin(Kind::Picker { selection }, 0));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_end() {
    with_builder(|b| b.end());
}

/// Opens a custom view instance. Returns its persistent state record (the
/// data of a box of type `ty`), and sets `*fresh` if it was just created and
/// must be initialized. `ty` may be null for views without state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_view_begin(view_id: u64, ty: *const TslType, fresh: *mut u8) -> *mut u8 {
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
    let record = STATES.with(|s| {
        let mut s = s.borrow_mut();
        let entry = s.entry(key).or_insert_with(|| {
            unsafe { *fresh = 1 };
            ViewState { record: unsafe { crate::tsl_box_new(ty) }, seen: generation }
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
pub extern "C" fn tsl_ui_padding(amount: f64) {
    modify(|m| m.padding = Some(m.padding.unwrap_or(0.0) + amount));
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
pub extern "C" fn tsl_ui_color(color: i32) {
    modify(|m| m.color = Some(COLORS[color as usize]));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_background(color: i32, radius: f64) {
    modify(|m| m.background = Some((COLORS[color as usize], radius)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_border(color: i32, width: f64, radius: f64) {
    modify(|m| m.border = Some((COLORS[color as usize], width, radius)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_frame(has_width: u8, width: f64, has_height: u8, height: f64) {
    modify(|m| {
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
pub unsafe extern "C" fn tsl_ui_shortcut(key: *const Str, f: *const c_void, env: *mut Header) {
    let key = unsafe { text_of(key) }.to_lowercase();
    let cb = Callback::new(f, env);
    modify(|m| m.shortcut = Some((key, cb)));
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
            tsl_ui_begin(0, 12.0);
            tsl_ui_text(str_new("Count: 3"));
            tsl_ui_font(32.0, 1, 3, 1);
            tsl_ui_begin(1, 8.0);
            tsl_ui_button(str_new("-"), f, std::ptr::null_mut());
            tsl_ui_button(str_new("+"), f, std::ptr::null_mut());
            tsl_ui_end();
            tsl_ui_text_field(str_new("Your name"), name as *const c_void, std::ptr::null_mut());
            tsl_ui_toggle(str_new("Enabled"), flag as *const c_void, std::ptr::null_mut());
            tsl_ui_end();
            tsl_ui_padding(24.0);
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
        layout::Layouter { text: &mut text, scroll: &scroll }.run(&mut tree, 320.0, 260.0);
        let vstack = &tree.children[0];
        assert!(matches!(vstack.kind, Kind::VStack(_)));
        // Centered horizontally in the window, and the text field stretches.
        let r = vstack.layout.rect;
        assert!((r.x + r.w / 2.0 - 160.0).abs() < 1.0, "{r:?}");
        let field = &vstack.children[2];
        assert!((field.layout.rect.w - (320.0 - 48.0)).abs() < 1.0, "{:?}", field.layout.rect);

        let mut scene = vello::Scene::new();
        let ui = render::Interaction::default();
        render::Painter { scene: &mut scene, ui: &ui, scale: 2.0 }.paint(&tree);
        let path = std::env::temp_dir().join("tessel-runtime-counter.png");
        let mut cx = vello::util::RenderContext::new();
        render::render_png(&mut cx, &mut None, &scene, 640, 520, path.to_str().unwrap());
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    }
}
