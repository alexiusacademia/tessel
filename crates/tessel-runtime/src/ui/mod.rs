//! The UI runtime: a tree of views built by compiled code, laid out and
//! drawn by the runtime itself.
//!
//! Every time state changes, the program's build function runs again and
//! describes the whole UI through the `tsl_ui_*` calls below. The runtime
//! keeps what must survive rebuilds (view `state`, focus, scroll offsets)
//! keyed by each view's position in the tree.

mod access;
mod anim;
mod app;
mod canvas;
mod scene3d;
mod table;
mod controls;
mod draw;
mod edit;
mod highlight;
mod image;
mod layout;
mod markdown;
mod menu;
#[cfg(target_os = "macos")]
mod native;
mod render;
pub(crate) mod rich;
mod svg;
mod print;
mod text;
mod tray;
mod desktop;
pub(crate) mod media;
mod webview;

// The renderer: what draws, lays text out and reads pictures. With the
// `native` feature, the system's: `mac` on macOS and `win` on Windows;
// otherwise (and on other systems) the GPU's.
#[cfg(all(feature = "gpu", not(all(feature = "native", any(target_os = "macos", windows)))))]
pub(crate) mod gpu;
#[cfg(all(feature = "gpu", not(all(feature = "native", any(target_os = "macos", windows)))))]
pub(crate) use gpu as backend;
#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) mod mac;
#[cfg(all(feature = "native", target_os = "macos"))]
pub(crate) use mac as backend;
#[cfg(all(feature = "native", windows))]
pub(crate) mod win;
#[cfg(all(feature = "native", windows))]
pub(crate) use win as backend;
mod svg_list;
// Writing PDFs where the system has nothing to write them with (everywhere
// but macOS).
#[cfg(any(not(all(feature = "native", target_os = "macos")), test))]
mod pdf;
mod theme;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};

use crate::{tsl_release, tsl_retain, Header, Str, TslType};

pub use app::{on_reload, run_on_ui, run_until_idle, tsl_ui_hot, tsl_ui_run};
pub use theme::tsl_is_dark_mode;
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use native::check_menu_bar;
#[doc(hidden)]
pub use media::check_video;
#[doc(hidden)]
pub use webview::check_web_view;
#[doc(hidden)]
#[cfg(all(feature = "native", target_os = "macos"))]
pub use mac::check_layer;

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

    /// Runs a closure taking an `Int` and a pointer: `void fn(env, i64, ptr)`.
    pub fn call_with_int_ptr(&self, a: i64, b: *mut u8) {
        let f: extern "C" fn(*mut Header, i64, *mut u8) = unsafe { std::mem::transmute(self.f) };
        f(self.env, a, b);
    }

    /// Runs a closure taking a pointer and giving an `Int`: `i64 fn(env, ptr)`.
    pub fn call_ptr_to_int(&self, arg: *mut u8) -> i64 {
        let f: extern "C" fn(*mut Header, *mut u8) -> i64 = unsafe { std::mem::transmute(self.f) };
        f(self.env, arg)
    }

    /// Runs a closure taking two `Float`s: `void fn(env, f64, f64)`.
    pub fn call_with_floats(&self, a: f64, b: f64) {
        let f: extern "C" fn(*mut Header, f64, f64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, a, b);
    }

    /// Runs a closure taking a `Float` and a pointer: `void fn(env, f64, ptr)`.
    pub fn call_with_float_ptr(&self, a: f64, b: *mut u8) {
        let f: extern "C" fn(*mut Header, f64, *mut u8) = unsafe { std::mem::transmute(self.f) };
        f(self.env, a, b);
    }

    /// Runs a closure taking a pointer and two `Float`s: `void fn(env, ptr, f64, f64)`.
    pub fn call_with_ptr_floats(&self, a: *mut u8, x: f64, y: f64) {
        let f: extern "C" fn(*mut Header, *mut u8, f64, f64) = unsafe { std::mem::transmute(self.f) };
        f(self.env, a, x, y);
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
/// `Weight`'s cases, in order (the four first, then the others).
pub const WEIGHTS: [f32; 9] = [400.0, 500.0, 600.0, 700.0, 100.0, 200.0, 300.0, 800.0, 900.0];

pub enum Kind {
    Root,
    /// A custom view's body; lays out like a `VStack`.
    Group,
    Text(String),
    Button { label: String, action: Callback },
    /// `secure`: shows a dot for each character, and its text can't be
    /// copied.
    /// `lines` above 1: the text wraps, Enter starts a new line, and the
    /// field is that many lines high (and scrolls).
    /// `search`: a `SearchField`, with a magnifying glass before its text
    /// and a button that clears it after.
    TextField { placeholder: String, text: Callback, secure: bool, lines: u32, search: bool },
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
        /// For a debugger: the lines marked as breakpoints, the line a
        /// program is stopped on (0 for none), and what a click in the
        /// margin calls, with the line clicked.
        breakpoints: Vec<i64>,
        stopped_line: i64,
        on_breakpoint: Option<Callback>,
    },
    /// Tabs: each child is a page, titled by `.tabItem` and identified by `.tag`.
    TabView { selection: Callback, close: Option<Callback> },
    /// A hairline separator across its stack.
    Divider,
    /// `WebView(url:html:)`: a web page, shown by a view of the system's put
    /// over this one (see `webview`).
    WebView { url: String, html: String },
    /// `VideoPlayer(path, playing:position:controls:loop:volume:)`: a video,
    /// drawn by the runtime (see `media`). `playing` locates a `Bool` and
    /// `position` a `Float` (seconds), when they're bound.
    Video { path: String, playing: Option<Callback>, position: Option<Callback>, controls: bool, looping: bool, volume: f64 },
    /// The text of a `RichTextEditor`: its id (for its toolbar), its text
    /// (see `rich`), and what's told of each change (`fn(env, String)`,
    /// with the text's JSON).
    RichEditor { id: i64, doc: std::rc::Rc<rich::Doc>, on_change: Callback },
    /// An SVG drawing: a built-in icon (with its name), `Svg("…")`, or an SVG file.
    Svg { source: std::rc::Rc<str>, icon: Option<&'static str> },
    /// `style` is a `PickerStyle` tag: 0 segmented, 1 radio buttons, 2 a
    /// menu (a box with the chosen option, opening a list of them: see
    /// `controls::OPEN_MENU`).
    Picker { selection: Callback, style: u8 },
    /// Spacing, and alignment across the stack (0 leading, 1 center, 2 trailing).
    VStack(f64, u8),
    HStack(f64, u8),
    /// Its `Alignment` tag.
    ZStack(u8),
    /// Which ways it scrolls (`ScrollAxes`: 0 vertical, 1 horizontal, 2 both).
    Scroll(u8),
    /// `HSplit` (side by side) and `VSplit` (one above the other): two
    /// panes and a divider that's dragged between them. `size` is the
    /// first pane's width (height) when it's bound (see `layout::SPLITS`).
    Split { vertical: bool, size: Option<Callback> },
    /// `Grid(alignment:spacing:rowSpacing:)`: its `GridRow`s' cells in
    /// columns that line up (see `layout::grid`). Other views in it take a
    /// row of their own, as wide as the grid. `align` is an `Alignment` tag.
    Grid { spacing: f64, row_spacing: f64, align: u8 },
    /// A row of a `Grid`: each child is a cell. Elsewhere, it's laid out
    /// like an `HStack`.
    GridRow,
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
    /// A scene in three dimensions: `draw` is called with the view's size,
    /// and says what's in it (see `scene3d`).
    Scene3D { draw: Callback, redraw: Option<i64> },
    /// `Table` and `Tree` (see `table`): the header, drawn by itself, over
    /// its one child, a scroll view of its rows.
    Table(std::rc::Rc<table::Spec>),
    /// A row of a table: a cell (`VStack`) for each column.
    TableRow(table::Row),
    /// `Slider(value:from:to:step:onChange:)`: `value` locates a `Float`;
    /// a `step` of 0 means any value.
    Slider { value: Callback, from: f64, to: f64, step: f64, on_change: Option<Callback> },
    /// `Stepper(value:step:from:to:decimals:onChange:)`: `value` locates an
    /// `Int` (`int`) or a `Float`. `decimals` is how many the value shows.
    Stepper { value: Callback, int: bool, step: f64, from: f64, to: f64, decimals: usize, on_change: Option<Callback> },
    /// `ColorPicker(color:onChange:)`: `color` locates a `Color`. While its
    /// palette is open, its one child is the palette (see `picker`).
    ColorPicker { color: Callback, on_change: Option<Callback> },
    /// A color in a `ColorPicker`'s palette.
    Swatch(Rgba),
    /// `DisclosureGroup(title, isExpanded:)`: its children show while it's open.
    Disclosure { title: String, expanded: Callback },
    /// `LazyVStack(count:rowHeight:)`: rows `first ..` (its children, one
    /// stack each) of `count`, each `row_height` tall; only those near
    /// what shows are built (see `lazy`). `LazyHStack(count:columnWidth:)`
    /// (`across`): columns side by side, each `row_height` wide.
    Lazy { count: usize, row_height: f64, first: usize, across: bool },
    /// `MenuItem("Copy", shortcut: "cmd+c", checked:) { … }`: an item of a
    /// menu. Its shortcut is kept as `menu::Shortcut::parse` reads it.
    MenuItem { label: String, shortcut: String, checked: bool, action: Callback },
    /// `MenuDivider()`: a line between a menu's items.
    MenuDivider,
    /// `Menu("File") { … }`: a menu of the menu bar, or a submenu.
    Menu { title: String },
    /// The items of a `.contextMenu` or the menus of a `.menuBar` (kept in
    /// the view's `Mods`, not in the tree).
    MenuList,
    /// `.popover(isPresented:edge:) { … }`: content shown next to the view
    /// `anchor`, above everything else. Popovers are the last children of
    /// their window's root.
    /// With `edge` `SHEET`, it's a sheet (`.sheet`): in the middle of the
    /// window, over a dimmed rest that takes no clicks.
    Popover { anchor: u64, edge: u8, presented: Callback },
    /// `Window("Title", isOpen:width:height:) { … }`: another window. It's
    /// the root of its own tree (see `build`).
    Window { title: String, is_open: Callback, width: f64, height: f64 },
    /// `Ribbon(selection:) { tabs }`: a row of tabs (its children, which are
    /// `RibbonTab`s) over the selected tab's groups. `selection` locates the
    /// title of the tab that's selected.
    Ribbon { selection: Callback },
    /// `RibbonTab("Home") { groups }`.
    RibbonTab { title: String },
    /// `RibbonGroup("Clipboard") { items }`: its items in columns, and its
    /// title under them.
    /// `icon` is the button it becomes when there's no room for it (its
    /// first item's icon when it's "").
    RibbonGroup { title: String, icon: &'static str },
    /// `RibbonButton`, `RibbonToggle` and `RibbonMenu`: a built-in icon and a
    /// label, large (the icon over the label) or small (side by side). A
    /// menu's items are its children.
    RibbonItem { label: String, icon: &'static str, large: bool, role: RibbonRole },
}

/// What a ribbon's item does when it's clicked.
pub enum RibbonRole {
    Button(Callback),
    /// Flips the `Bool` it locates.
    Toggle(Callback),
    /// Opens a menu of its items.
    Menu,
}

impl Kind {
    /// Menus and their items: they show only in menus, never in a layout.
    pub fn is_menu(&self) -> bool {
        matches!(self, Kind::MenuItem { .. } | Kind::MenuDivider | Kind::Menu { .. } | Kind::MenuList)
    }
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
    /// `.font(italic:family:)`; inherited.
    pub italic: Option<bool>,
    pub family: Option<text::Family>,
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
    /// `.shortcut("s") { … }`: runs with Cmd+S (Ctrl+S on Windows). A view
    /// can have several; each adds one.
    pub shortcuts: Vec<Shortcut>,
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
    /// `.animation(seconds, curve:)`: how long changes to the view and the
    /// views inside it take (0 for at once), and how they move (see
    /// `anim::ease`); inherited.
    pub animation: Option<(f64, u8)>,
    /// `.onPointer { event in … }`: called with what the mouse does on the
    /// view (see `PointerC`).
    pub on_pointer: Option<Callback>,
    /// `.onKey { key in … }`: called with keys pressed while no text is edited.
    pub on_key: Option<Callback>,
    /// `.onKeyEvent { event in … }`: like `on_key`, with the modifier keys
    /// held (see `KeyC`).
    pub on_key_event: Option<Callback>,
    /// `.onKeyEvent(releases: true)`: keys let go, and modifiers on their
    /// own, go to it too.
    pub key_releases: bool,
    /// `.cursor(…)`: the pointer shown over the view (a `CursorStyle` tag).
    pub cursor: Option<u8>,
    /// `.offset(x:y:)`: the view (and what's in it) is shown this far from
    /// where layout put it, without moving anything else.
    pub offset: (f64, f64),
    /// `.clipped()`: nothing of the view's content shows outside its frame.
    pub clipped: bool,
    /// `.onDrop { path in … }`: called with each file dropped on the window.
    pub on_drop: Option<Callback>,
    /// `.draggable("payload")`: the view can be dragged onto a `.dropTarget`,
    /// which gets this text.
    pub draggable: Option<String>,
    /// `.dragFile(path)`: dragging the view drags that file out of the app.
    pub drag_file: Option<String>,
    /// `.dropTarget(isTargeted:) { payload, x, y in … }`.
    pub drop_target: Option<DropTarget>,
    /// `.tooltip("…")`: shown next to the pointer when it rests on the view.
    pub tooltip: Option<String>,
    /// `.accessibilityLabel("…")`: what a screen reader calls the view
    /// (see `access`).
    pub access_label: Option<String>,
    /// `.onResize { width, height in … }`: called with the view's size
    /// (inside its padding) when it's first laid out, and when it changes.
    pub on_resize: Option<Callback>,
    /// `.contextMenu { … }`: the menu a right click (or Ctrl-click) on the
    /// view opens: a `MenuList`.
    pub context_menu: Option<Box<Node>>,
    /// `.menuBar { … }`: the app's menus (a `MenuList` of `Menu`s).
    pub menu_bar: Option<Box<Node>>,
    /// `.trayIcon(icon, title:keepRunning:) { … }`: the app's icon in the
    /// menu bar (macOS) or the taskbar's notification area (Windows).
    pub tray: Option<Box<Tray>>,
    /// `.onOpenFile { path in … }`: called with each file the system asks
    /// the app to open.
    pub on_open_file: Option<Callback>,
    /// `.onCloseRequest { … }`: called instead of closing the window (or
    /// quitting) when the user asks to.
    pub on_close_request: Option<Callback>,
    /// `.rotation(degrees)`: turned (clockwise) around its center; it takes
    /// the room of the box around it turned.
    pub rotation: f64,
    /// `.scale(…)`: made larger or smaller across and down, around its
    /// center (none is 1 and 1); it takes the room of its new size.
    pub scale: Option<(f64, f64)>,
    /// `.gridCellColumns(n)`: how many columns of its `Grid` the cell takes.
    pub grid_columns: Option<usize>,
    /// `.gridColumnAlignment(…)`: how the cells of its column line up
    /// across (0 leading, 1 center, 2 trailing).
    pub grid_align: Option<u8>,
    /// `.selectable()`: its text (and the text in it) can be selected and
    /// copied; inherited.
    pub selectable: Option<bool>,
    /// `.lineLimit(n)`: its text (and the text in it) shows at most `n`
    /// lines, the last ending in "…"; inherited.
    pub line_limit: Option<usize>,
    /// Which view expression of the source this is, for `tessel preview`:
    /// its number among the ones the compiler listed.
    pub source: Option<u32>,
}

impl Mods {
    /// What an empty custom view still does, with nothing to show: it then
    /// isn't dropped from a ribbon or a grid.
    fn acts_alone(&self) -> bool {
        !self.shortcuts.is_empty()
            || self.on_appear.is_some()
            || self.on_key.is_some()
            || self.on_key_event.is_some()
            || self.on_drop.is_some()
            || self.menu_bar.is_some()
            || self.tray.is_some()
            || self.on_open_file.is_some()
            || self.on_close_request.is_some()
    }
}

/// A `.trayIcon`: its built-in icon's name, a title beside it (macOS), whether
/// the app goes on running in it once its window is closed, and its menu's
/// items (a `MenuList`).
pub struct Tray {
    pub icon: &'static str,
    pub title: String,
    pub keep_running: bool,
    pub items: Node,
}

/// A `.dropTarget`: `action` gets what's dropped (`fn(env, String, x, y)`),
/// and `targeted` (a `Bool` binding) says whether something is dragged over it.
pub struct DropTarget {
    pub action: Callback,
    pub targeted: Option<Callback>,
}

/// A `.shortcut(…)`: the key (lowercase), and whether Shift or Alt must be
/// held as well (`"shift+s"`). Cmd (Ctrl on Windows) always is.
pub struct Shortcut {
    pub key: String,
    pub shift: bool,
    pub alt: bool,
    pub action: Callback,
}

impl Shortcut {
    /// Reads a shortcut's key: `"s"`, `"shift+s"`, `"alt+shift+left"`.
    pub fn parse(text: &str, action: Callback) -> Shortcut {
        let mut key = text.to_lowercase();
        let (mut shift, mut alt) = (false, false);
        loop {
            if let Some(rest) = key.strip_prefix("shift+").filter(|r| !r.is_empty()) {
                shift = true;
                key = rest.to_string();
            } else if let Some(rest) = key.strip_prefix("alt+").filter(|r| !r.is_empty()) {
                alt = true;
                key = rest.to_string();
            } else {
                break;
            }
        }
        Shortcut { key, shift, alt, action }
    }
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
    /// Where the pointer is from the window's top left corner.
    pub window_x: f64,
    pub window_y: f64,
}

/// A `KeyEvent`, as the adapter the compiler generates reads it. The
/// strings are the callee's.
#[repr(C)]
pub struct KeyC {
    pub key: *mut Str,
    pub text: *mut Str,
    pub shift: u8,
    pub alt: u8,
    pub command: u8,
    pub control: u8,
    pub repeated: u8,
    /// The key was let go.
    pub up: u8,
}

pub struct Node {
    pub kind: Kind,
    pub children: Vec<Node>,
    pub mods: Mods,
    /// Identity across rebuilds: a hash of the path from the root.
    pub key: u64,
    /// Its `.id(…)` (a `String`'s hash), for `scrollTo`.
    pub id: Option<u64>,
    /// Filled in by layout.
    pub layout: layout::Box,
}

/// Whether a scroll view with these `ScrollAxes` scrolls up and down, and across.
pub fn scroll_axes(axes: u8) -> (bool, bool) {
    (axes != 1, axes != 0)
}

impl Node {
    /// For a TabView: the index of the page whose tag equals the selection.
    pub fn selected_tab(&self) -> usize {
        let Kind::TabView { selection, .. } = &self.kind else { return 0 };
        let current = selection.locate();
        self.children.iter().position(|c| c.mods.tag.as_ref().is_some_and(|t| t.equals(current))).unwrap_or(0)
    }

    /// For a Ribbon: the index of the tab whose title is the selection.
    pub fn selected_ribbon_tab(&self) -> usize {
        let Kind::Ribbon { selection } = &self.kind else { return 0 };
        let current = layout::read_string(selection);
        self.children.iter().position(|c| matches!(&c.kind, Kind::RibbonTab { title } if *title == current)).unwrap_or(0)
    }

    fn new(kind: Kind, key: u64) -> Node {
        Node { kind, children: Vec::new(), mods: Mods::default(), key, id: None, layout: layout::Box::default() }
    }

    /// Children that are showing: a tab view shows only its selected page.
    pub fn visible_children(&self) -> &[Node] {
        match self.kind {
            Kind::TabView { .. } => {
                let i = self.selected_tab();
                self.children.get(i..=i).unwrap_or(&[])
            }
            Kind::Ribbon { .. } => {
                let i = self.selected_ribbon_tab();
                self.children.get(i..=i).unwrap_or(&[])
            }
            // (A menu's items show only when it's opened.)
            Kind::RibbonItem { .. } => &[],
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

    pub fn find_mut(&mut self, key: u64) -> Option<&mut Node> {
        if self.key == key {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(key))
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
        Kind::Scroll(_) => 14,
        Kind::Spacer => 15,
        Kind::Image { .. } => 16,
        Kind::Svg { .. } => 17,
        Kind::Markdown { .. } => 18,
        Kind::Canvas { .. } => 19,
        Kind::Slider { .. } => 20,
        Kind::Stepper { .. } => 21,
        Kind::ColorPicker { .. } => 22,
        Kind::Swatch(_) => 23,
        Kind::Disclosure { .. } => 24,
        Kind::Lazy { .. } => 25,
        Kind::MenuItem { .. } => 26,
        Kind::MenuDivider => 27,
        Kind::Menu { .. } => 28,
        Kind::MenuList => 29,
        Kind::Popover { .. } => 30,
        Kind::Window { .. } => 31,
        Kind::Ribbon { .. } => 32,
        Kind::RibbonTab { .. } => 33,
        Kind::RibbonGroup { .. } => 34,
        Kind::RibbonItem { .. } => 35,
        Kind::Scene3D { .. } => 36,
        Kind::Table(_) => 37,
        Kind::TableRow(_) => 38,
        Kind::Split { .. } => 39,
        Kind::Grid { .. } => 40,
        Kind::GridRow => 41,
        Kind::RichEditor { .. } => 42,
        Kind::Video { .. } => 43,
        Kind::WebView { .. } => 44,
    }
}

fn child_key(parent: u64, index: usize, salt: u64) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (parent, index, salt).hash(&mut h);
    h.finish()
}

// ===== building =====

pub(super) struct Builder {
    /// Open containers, innermost last. The root is at the bottom.
    stack: Vec<Node>,
    /// Set by `.id(…)`: the identity of the next view, instead of its position.
    pending_id: Option<u64>,
    /// That id, once the next view's key is made from it.
    taken_id: Option<u64>,
    /// Open popovers, for the root of each window being built (innermost
    /// last): they become its last children.
    overlays: Vec<Vec<Node>>,
    /// Where each popover being built goes in its list (so a popover opened
    /// from another comes after it).
    overlay_at: Vec<usize>,
    /// Open windows (`Window`), each the root of its own tree.
    windows: Vec<Node>,
    /// How many `Window`s were declared so far (open or not): their identity.
    window_count: u64,
}

impl Builder {
    pub(super) fn parent(&mut self) -> &mut Node {
        self.stack.last_mut().expect("no open view")
    }

    fn next_key(&mut self, salt: u64) -> u64 {
        self.taken_id = self.pending_id.take();
        if let Some(id) = self.taken_id {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.parent().key, "id", id, salt).hash(&mut h);
            return h.finish();
        }
        let p = self.parent();
        child_key(p.key, p.children.len(), salt)
    }

    fn leaf(&mut self, kind: Kind) {
        let key = self.next_key(kind_salt(&kind));
        let node = Node { id: self.taken_id.take(), ..Node::new(kind, key) };
        self.parent().children.push(node);
    }

    fn begin(&mut self, kind: Kind, salt: u64) {
        let key = self.next_key(salt ^ kind_salt(&kind));
        let node = Node { id: self.taken_id.take(), ..Node::new(kind, key) };
        self.stack.push(node);
    }

    pub(super) fn end(&mut self) {
        let mut node = self.stack.pop().expect("unbalanced end");
        lift_ribbon_parts(&mut node);
        self.parent().children.push(node);
    }

    /// The most recently finished node, which modifiers apply to.
    fn last(&mut self) -> Option<&mut Node> {
        self.parent().children.last_mut()
    }

    /// The identity the table about to be begun (`Kind::Table`) will have.
    fn next_table_key(&mut self) -> u64 {
        let salt = 37;
        if let Some(id) = self.pending_id {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.parent().key, "id", id, salt).hash(&mut h);
            return h.finish();
        }
        let p = self.parent();
        child_key(p.key, p.children.len(), salt)
    }

    /// Opens a container with its own identity (rather than a position).
    fn begin_keyed(&mut self, kind: Kind, key: u64) {
        let id = self.pending_id.take();
        self.stack.push(Node { id, ..Node::new(kind, key) });
    }
}

fn hash_key(parts: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// What a build makes: the main window's tree, and the other open windows'.
pub struct Built {
    pub root: Node,
    pub windows: Vec<Node>,
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
/// A ribbon's tabs, a tab's groups and a group's items can come from views
/// of the program's own (`view HomeTab() { RibbonTab("Home") { … } }`),
/// which wrap what they make: here they're taken out of the wrapping, so
/// the ribbon finds them as if they were written in it.
fn lift_ribbon_parts(node: &mut Node) {
    fn wanted(parent: &Kind, c: &Node) -> bool {
        match (parent, &c.kind) {
            (Kind::Ribbon { .. }, Kind::RibbonTab { .. }) => true,
            (Kind::RibbonTab { .. }, Kind::RibbonGroup { .. }) => true,
            (Kind::RibbonGroup { .. }, Kind::RibbonItem { .. }) => true,
            (Kind::Grid { .. }, Kind::GridRow) => true,
            _ => false,
        }
    }
    /// Whether a custom view holds only such parts (in views of its own
    /// too). One that's empty (its body made nothing) holds none: it's
    /// dropped, so that it takes no room.
    fn only_parts(parent: &Kind, c: &Node) -> bool {
        matches!(c.kind, Kind::Group)
            && (!c.children.is_empty() || !c.mods.acts_alone())
            && c.children.iter().all(|g| wanted(parent, g) || only_parts(parent, g))
    }
    fn lift(parent: &Kind, children: Vec<Node>, out: &mut Vec<Node>) {
        for c in children {
            if only_parts(parent, &c) {
                lift(parent, c.children, out);
            } else {
                out.push(c);
            }
        }
    }
    if !matches!(node.kind, Kind::Ribbon { .. } | Kind::RibbonTab { .. } | Kind::RibbonGroup { .. } | Kind::Grid { .. }) {
        return;
    }
    if !node.children.iter().any(|c| only_parts(&node.kind, c)) {
        return;
    }
    let mut out = Vec::with_capacity(node.children.len());
    lift(&node.kind, std::mem::take(&mut node.children), &mut out);
    node.children = out;
}

pub fn build(root: &Callback) -> Node {
    build_all(root).root
}

/// Like `build`, with the other windows too.
pub fn build_all(root: &Callback) -> Built {
    let generation = GENERATION.with(|g| {
        *g.borrow_mut() += 1;
        *g.borrow()
    });
    BUILDER.with(|b| {
        *b.borrow_mut() = Some(Builder {
            stack: vec![Node::new(Kind::Root, 1)],
            pending_id: None,
            taken_id: None,
            overlays: vec![Vec::new()],
            overlay_at: Vec::new(),
            windows: Vec::new(),
            window_count: 0,
        })
    });
    root.call();
    let mut builder = BUILDER.with(|b| b.borrow_mut().take()).expect("builder");
    while builder.stack.len() > 1 {
        builder.end();
    }
    let popovers = builder.overlays.drain(..).next().unwrap_or_default();
    let gone: Vec<*mut Header> = STATES.with(|s| {
        let mut s = s.borrow_mut();
        let dead: Vec<u64> = s.iter().filter(|(_, v)| v.seen != generation).map(|(k, _)| *k).collect();
        dead.into_iter().filter_map(|k| s.remove(&k)).map(|v| v.record).collect()
    });
    for record in gone {
        unsafe { tsl_release(record) };
    }
    let mut root = builder.stack.pop().unwrap();
    root.children.extend(popovers);
    Built { root, windows: builder.windows }
}

/// Releases the pictures, drawings and fonts kept for reuse. At exit this
/// must happen before the process ends: Windows may have unloaded the
/// libraries they belong to by the time thread-local values are dropped.
pub fn release_caches() {
    media::forget_all();
    canvas::forget_all();
    scene3d::forget_all();
    image::forget_all();
}

/// Releases all view state (at exit, so leak checkers see a clean heap).
pub fn release_all_state() {
    release_caches();
    controls::forget_hex_text();
    table::forget_cell_text();
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

/// `TextField` (and `SearchField`): `flags` has 1 for a secure field, 2
/// for a search field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_text_field(placeholder: *const Str, flags: u8, lines: i64, f: *const c_void, env: *mut Header) {
    let placeholder = unsafe { text_of(placeholder) };
    let text = Callback::new(f, env);
    let (secure, search) = (flags & 1 != 0, flags & 2 != 0);
    if search {
        // (Its magnifying glass and clear button are built-in icons.)
        svg::enable_icons();
    }
    // (A secret is one line: its dots don't wrap; so is a search.)
    let lines = if secure || search { 1 } else { lines.clamp(1, 1000) as u32 };
    with_builder(|b| b.leaf(Kind::TextField { placeholder, text, secure, lines, search }));
}

/// `WebView(url:html:)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_web_view(url: *const Str, html: *const Str) {
    let (url, html) = unsafe { (text_of(url), text_of(html)) };
    with_builder(|b| b.leaf(Kind::WebView { url, html }));
}

/// `VideoPlayer(path, playing:position:controls:loop:volume:)`. The
/// locators are null when they aren't bound.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_video(
    path: *const Str,
    playing_f: *const c_void,
    playing_env: *mut Header,
    position_f: *const c_void,
    position_env: *mut Header,
    controls: u8,
    looping: u8,
    volume: f64,
) {
    // (Its play and pause buttons are built-in icons.)
    svg::enable_icons();
    let path = unsafe { text_of(path) };
    let playing = (!playing_f.is_null()).then(|| Callback::new(playing_f, playing_env));
    let position = (!position_f.is_null()).then(|| Callback::new(position_f, position_env));
    let volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    with_builder(|b| b.leaf(Kind::Video { path, playing, position, controls: controls != 0, looping: looping != 0, volume }));
}

/// `__RichEditor(id:json:onChange:)`: the text of a `RichTextEditor`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_rich_editor(id: i64, json: *const Str, f: *const c_void, env: *mut Header) {
    let doc = rich::Doc::parse(unsafe { crate::str_text(json) });
    let on_change = Callback::new(f, env);
    with_builder(|b| b.leaf(Kind::RichEditor { id, doc: std::rc::Rc::new(doc), on_change }));
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

/// `CodeEditor(text:errorLines:line:completions:completionStart:onComplete:breakpoints:stoppedLine:onBreakpoint:)`.
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
    breakpoints: *const crate::List,
    stopped_line: i64,
    breakpoint_f: *const c_void,
    breakpoint_env: *mut Header,
) {
    let numbers = |list: *const crate::List| -> Vec<i64> {
        let n = unsafe { crate::tsl_list_len(list) };
        (0..n).map(|i| unsafe { *(crate::tsl_list_get(list, i, std::ptr::null()) as *const i64) }).collect()
    };
    let breakpoints = numbers(breakpoints);
    let on_breakpoint = (!breakpoint_f.is_null()).then(|| Callback::new(breakpoint_f, breakpoint_env));
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
        b.leaf(Kind::CodeEditor {
            text,
            error_lines,
            line,
            completions,
            completion_start,
            on_complete,
            breakpoints,
            stopped_line,
            on_breakpoint,
        })
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

/// `Scene3D(redraw:) { s in … }`: `f` is called as `f(env, width, height)`
/// to say what's in the scene, as a canvas's is to draw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_scene3d(f: *const c_void, env: *mut Header, has_redraw: u8, redraw: i64) {
    scene3d::enable();
    let draw = Callback::new(f, env);
    let redraw = (has_redraw != 0).then_some(redraw);
    with_builder(|b| b.leaf(Kind::Scene3D { draw, redraw }));
}

// ===== C ABI: menus, popovers and windows =====

/// `MenuItem(label, shortcut:, checked:) { … }`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_menu_item(label: *const Str, shortcut: *const Str, checked: u8, f: *const c_void, env: *mut Header) {
    // (A drawn menu's check marks and submenu arrows are built-in icons.)
    svg::enable_icons();
    let (label, shortcut) = unsafe { (text_of(label), text_of(shortcut)) };
    let action = Callback::new(f, env);
    with_builder(|b| b.leaf(Kind::MenuItem { label, shortcut, checked: checked != 0, action }));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_menu_divider() {
    with_builder(|b| b.leaf(Kind::MenuDivider));
}

/// `Menu(title) { … }`: closed with `tsl_ui_end`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_menu(title: *const Str) {
    let title = unsafe { text_of(title) };
    with_builder(|b| b.begin(Kind::Menu { title }, 0));
}

/// Opens the items of a `.contextMenu` (`which` = 0) or the menus of a
/// `.menuBar` (1) of the last view; `tsl_ui_end_attached` gives them to it.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin_attached(which: i32) {
    with_builder(|b| {
        let anchor = b.last().map_or(0, |n| n.key);
        b.begin_keyed(Kind::MenuList, hash_key((anchor, "menu", which)));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_end_attached(which: i32) {
    with_builder(|b| {
        let list = b.stack.pop().expect("unbalanced end");
        if let Some(n) = b.last() {
            if which == 0 {
                n.mods.context_menu = Some(Box::new(list));
            } else {
                n.mods.menu_bar = Some(Box::new(list));
            }
        }
    });
}

/// Opens the items of a `.trayIcon`; `tsl_ui_end_tray` gives them to the last view.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin_tray() {
    with_builder(|b| {
        let anchor = b.last().map_or(0, |n| n.key);
        b.begin_keyed(Kind::MenuList, hash_key((anchor, "tray")));
    });
}

/// `.trayIcon(icon, title:keepRunning:)`: `icon` is an `IconName` case.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_end_tray(icon: i32, title: *const Str, keep_running: u8) {
    svg::enable_icons();
    let title = unsafe { text_of(title) };
    let icon = svg::ICONS.get(icon as usize).map_or("", |(name, _)| name);
    with_builder(|b| {
        let items = b.stack.pop().expect("unbalanced end");
        if let Some(n) = b.last() {
            n.mods.tray = Some(Box::new(Tray { icon, title, keep_running: keep_running != 0, items }));
        }
    });
}

/// The `edge` of a popover that's a sheet.
pub const SHEET: u8 = 4;

/// `.popover(isPresented:edge:) { … }` on the last view (and `.sheet`, with
/// the edge `SHEET`). Returns whether
/// it's presented: only then is its content built, and closed with
/// `tsl_ui_end_popover`. (The locator is taken either way.)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_popover(f: *const c_void, env: *mut Header, edge: i32) -> u8 {
    let presented = Callback::new(f, env);
    if unsafe { *presented.locate() } & 1 == 0 {
        return 0;
    }
    let mut shown = 0;
    with_builder(|b| {
        let Some(anchor) = b.last().map(|n| n.key) else { return };
        let at = b.overlays.last().map_or(0, Vec::len);
        b.overlay_at.push(at);
        b.begin_keyed(Kind::Popover { anchor, edge: edge.clamp(0, SHEET as i32) as u8, presented }, hash_key((anchor, "popover", edge == SHEET as i32)));
        shown = 1;
    });
    shown
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_end_popover() {
    with_builder(|b| {
        let node = b.stack.pop().expect("unbalanced end");
        let at = b.overlay_at.pop().unwrap_or(0);
        if let Some(list) = b.overlays.last_mut() {
            list.insert(at.min(list.len()), node);
        }
    });
}

/// `Window(title, isOpen:width:height:) { … }`. Returns whether it's open:
/// only then is its content built, and closed with `tsl_ui_end_window`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_window(title: *const Str, f: *const c_void, env: *mut Header, width: f64, height: f64) -> u8 {
    let title = unsafe { text_of(title) };
    let is_open = Callback::new(f, env);
    let mut shown = 0;
    with_builder(|b| {
        b.window_count += 1;
        if unsafe { *is_open.locate() } & 1 == 0 {
            return;
        }
        let key = hash_key(("window", b.window_count));
        b.begin_keyed(Kind::Window { title, is_open, width: width.max(1.0), height: height.max(1.0) }, key);
        b.overlays.push(Vec::new());
        shown = 1;
    });
    shown
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_end_window() {
    with_builder(|b| {
        let mut node = b.stack.pop().expect("unbalanced end");
        node.children.extend(b.overlays.pop().unwrap_or_default());
        b.windows.push(node);
    });
}

/// `.onOpenFile { path in … }`: `f` is called as `f(env, String)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_open_file(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_open_file = Some(cb));
}

/// `.onCloseRequest { … }`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_close_request(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_close_request = Some(cb));
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
/// `align` is an `Alignment` tag (for a Scroll, its `ScrollAxes` tag).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin(kind: i32, spacing: f64, align: i32) {
    if kind == 3 {
        with_builder(|b| b.begin(Kind::Scroll(align.clamp(0, 2) as u8), 0));
        return;
    }
    let align = align.clamp(0, 8) as u8;
    let (h, v) = align_2d(align);
    let kind = match kind {
        // Stacks store the position across their axis: 0 start, 1 center, 2 end.
        0 => Kind::VStack(spacing, h),
        // In an HStack, `.leading`/`.trailing` mean top/bottom.
        1 => Kind::HStack(spacing, if align == 0 || align == 2 { align } else { v }),
        _ => Kind::ZStack(align),
    };
    with_builder(|b| b.begin(kind, 0));
}

/// Opens an `HSplit` (`vertical` 0) or `VSplit` (1). `f` is null without a
/// `size:` binding; otherwise it locates the first pane's size (a `Float`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_split(vertical: u8, f: *const c_void, env: *mut Header) {
    let size = (!f.is_null()).then(|| Callback::new(f, env));
    with_builder(|b| b.begin(Kind::Split { vertical: vertical != 0, size }, 0));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_picker(style: i32, f: *const c_void, env: *mut Header) {
    let selection = Callback::new(f, env);
    // (A menu's arrow and check mark are built-in icons.)
    if style == 2 {
        svg::enable_icons();
    }
    with_builder(|b| b.begin(Kind::Picker { selection, style: style.clamp(0, 2) as u8 }, 0));
}

/// `Grid(alignment:spacing:rowSpacing:)`: a `rowSpacing` that's not a
/// number is `spacing`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin_grid(align: i32, spacing: f64, row_spacing: f64) {
    let spacing = if spacing.is_finite() { spacing.max(0.0) } else { 8.0 };
    let row_spacing = if row_spacing.is_finite() { row_spacing.max(0.0) } else { spacing };
    with_builder(|b| b.begin(Kind::Grid { spacing, row_spacing, align: align.clamp(0, 8) as u8 }, 0));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_begin_grid_row() {
    with_builder(|b| b.begin(Kind::GridRow, 0));
}

/// `.gridCellColumns(n)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_grid_cell_columns(n: i64) {
    modify(|m| m.grid_columns = Some(n.clamp(1, 1000) as usize));
}

/// `.gridColumnAlignment(…)`: only the horizontal part of the `Alignment` counts.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_grid_column_alignment(align: i32) {
    let (h, _) = align_2d(align.clamp(0, 8) as u8);
    modify(|m| m.grid_align = Some(h));
}

/// `.rotation(degrees)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_rotation(degrees: f64) {
    modify(|m| m.rotation = if degrees.is_finite() { degrees } else { 0.0 });
}

/// `.scale(factor, x:, y:)`: across `factor * x`, down `factor * y`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_scale(factor: f64, x: f64, y: f64) {
    let ok = |v: f64| if v.is_finite() { v.max(0.0) } else { 1.0 };
    let (sx, sy) = (ok(factor) * ok(x), ok(factor) * ok(y));
    modify(|m| m.scale = ((sx, sy) != (1.0, 1.0)).then_some((sx, sy)));
}

/// `.selectable(on)`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_selectable(on: u8) {
    modify(|m| m.selectable = Some(on != 0));
}

/// `.lineLimit(n)`: 0 or less is no limit.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_line_limit(n: i64) {
    modify(|m| m.line_limit = Some(n.clamp(0, 1_000_000) as usize));
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
pub extern "C" fn tsl_ui_animation(seconds: f64, curve: i32) {
    modify(|m| m.animation = Some((seconds.max(0.0), curve.clamp(0, 5) as u8)));
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
pub unsafe extern "C" fn tsl_ui_font(size: f64, has_size: u8, weight: i32, has_weight: u8, italic: u8, has_italic: u8, family: *const Str) {
    // (No family given comes as null.)
    let family = (!family.is_null()).then(|| text::Family::named(unsafe { crate::str_text(family) }));
    modify(|m| {
        if has_italic != 0 {
            m.italic = Some(italic != 0);
        }
        if family.is_some() {
            m.family = family;
        }
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

/// `.onKeyEvent(releases:) { event in … }`: `f` is called as `f(env,
/// &KeyC)`. With `releases`, also when a key is let go, and for the
/// modifier keys on their own.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_key_event(f: *const c_void, env: *mut Header, releases: u8) {
    let cb = Callback::new(f, env);
    modify(|m| {
        m.on_key_event = Some(cb);
        m.key_releases = releases != 0;
    });
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

/// `.draggable(payload)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_draggable(payload: *const Str) {
    let payload = unsafe { text_of(payload) };
    modify(|m| m.draggable = Some(payload));
}

/// `.dragFile(path)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_drag_file(path: *const Str) {
    let path = unsafe { text_of(path) };
    modify(|m| m.drag_file = (!path.is_empty()).then_some(path));
}

/// `.dropTarget(isTargeted:) { payload, x, y in … }`: `f` is called as
/// `f(env, String, x, y)`; `targeted_f` (a locator) is null without `isTargeted`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_drop_target(f: *const c_void, env: *mut Header, targeted_f: *const c_void, targeted_env: *mut Header) {
    let action = Callback::new(f, env);
    let targeted = (!targeted_f.is_null()).then(|| Callback::new(targeted_f, targeted_env));
    modify(|m| m.drop_target = Some(DropTarget { action, targeted }));
}

/// `.onResize { width, height in … }`: `f` is called as `f(env, width, height)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_on_resize(f: *const c_void, env: *mut Header) {
    let cb = Callback::new(f, env);
    modify(|m| m.on_resize = Some(cb));
}

/// `.accessibilityLabel("…")`: what a screen reader calls the view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_accessibility_label(text: *const Str) {
    let text = unsafe { text_of(text) };
    modify(|m| m.access_label = (!text.is_empty()).then_some(text));
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
    let key = unsafe { text_of(key) };
    let shortcut = Shortcut::parse(&key, Callback::new(f, env));
    modify(|m| m.shortcuts.push(shortcut));
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
            tsl_ui_font(32.0, 1, 3, 1, 0, 0, std::ptr::null());
            tsl_ui_begin(1, 8.0, 1);
            tsl_ui_button(str_new("-"), f, std::ptr::null_mut());
            tsl_ui_button(str_new("+"), f, std::ptr::null_mut());
            tsl_ui_end();
            tsl_ui_text_field(str_new("Your name"), 0, 1, name as *const c_void, std::ptr::null_mut());
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
        layout::Layouter { text: &mut text, scroll: &scroll, scroll_x: &scroll, columns: Vec::new(), composing: None }.run_below(&mut tree, 320.0, 260.0, 0.0);
        let vstack = &tree.children[0];
        assert!(matches!(vstack.kind, Kind::VStack(..)));
        // Centered horizontally in the window, and the text field stretches.
        let r = vstack.layout.rect;
        assert!((r.x + r.w / 2.0 - 160.0).abs() < 1.0, "{r:?}");
        let field = &vstack.children[2];
        assert!((field.layout.rect.w - (320.0 - 48.0)).abs() < 1.0, "{:?}", field.layout.rect);

        let mut list = draw::DrawList::new();
        let ui = render::Interaction::default();
        render::Painter { scene: &mut list, ui: &ui, scale: 2.0, offset: (0.0, 0.0), turn: kurbo::Affine::IDENTITY, clip: None, marks: Vec::new() }.paint(&tree);
        let path = std::env::temp_dir().join("tessel-runtime-counter.png");
        let pixels = backend::Offscreen::default().rgba(&list, 640, 520, Some(theme::theme().colors[theme::BACKGROUND]));
        assert!(backend::save_rgba(path.to_str().unwrap(), 640, 520, pixels));
        assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    }
}

// ===== ribbons =====

/// `Ribbon(selection:) { … }`: `f` locates the selected tab's title.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_ribbon(f: *const c_void, env: *mut Header) {
    let selection = Callback::new(f, env);
    with_builder(|b| b.begin(Kind::Ribbon { selection }, 0));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_ribbon_tab(title: *const Str) {
    let title = unsafe { text_of(title) };
    with_builder(|b| b.begin(Kind::RibbonTab { title }, 0));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_ribbon_group(title: *const Str, icon: i32) {
    let title = unsafe { text_of(title) };
    let icon = if icon < 0 { "" } else { ribbon_icon(icon) };
    with_builder(|b| b.begin(Kind::RibbonGroup { title, icon }, 0));
}

/// A ribbon item's icon (a built-in one, by its `IconName` case).
fn ribbon_icon(index: i32) -> &'static str {
    svg::enable_icons();
    svg::ICONS.get(index as usize).map_or("", |(name, _)| name)
}

/// `RibbonButton(label, icon:, size:) { … }`; `size` is 0 small, 1 large.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_ribbon_button(label: *const Str, icon: i32, size: i32, f: *const c_void, env: *mut Header) {
    let label = unsafe { text_of(label) };
    let role = RibbonRole::Button(Callback::new(f, env));
    with_builder(|b| b.leaf(Kind::RibbonItem { label, icon: ribbon_icon(icon), large: size == 1, role }));
}

/// `RibbonToggle(label, icon:, size:, isOn:)`: `f` locates the `Bool`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_ribbon_toggle(label: *const Str, icon: i32, size: i32, f: *const c_void, env: *mut Header) {
    let label = unsafe { text_of(label) };
    let role = RibbonRole::Toggle(Callback::new(f, env));
    with_builder(|b| b.leaf(Kind::RibbonItem { label, icon: ribbon_icon(icon), large: size == 1, role }));
}

/// `RibbonMenu(label, icon:, size:) { items }`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_ribbon_menu(label: *const Str, icon: i32, size: i32) {
    let label = unsafe { text_of(label) };
    let icon = ribbon_icon(icon);
    // (Its arrow, and its menu's check marks and submenu arrows, are icons too.)
    with_builder(|b| b.begin(Kind::RibbonItem { label, icon, large: size == 1, role: RibbonRole::Menu }, 0));
}
