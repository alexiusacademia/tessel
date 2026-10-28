//! The controls that edit numbers and colors (`Slider`, `Stepper`,
//! `ColorPicker`), `DisclosureGroup`, and which rows a `LazyVStack` builds.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;

use super::{text_of, with_builder, Builder, Callback, Kind, Rgba};
use crate::{Header, Str, TslType};

// ===== Slider and Stepper =====

/// `Slider(value:from:to:step:onChange:)`. `change_f` is null without `onChange`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_slider(
    f: *const c_void,
    env: *mut Header,
    from: f64,
    to: f64,
    step: f64,
    change_f: *const c_void,
    change_env: *mut Header,
) {
    let value = Callback::new(f, env);
    let on_change = (!change_f.is_null()).then(|| Callback::new(change_f, change_env));
    let (from, to) = finite_range(from, to);
    let step = if step.is_finite() { step.max(0.0) } else { 0.0 };
    with_builder(|b| b.leaf(Kind::Slider { value, from, to, step, on_change }));
}

/// `Stepper(value:step:from:to:decimals:onChange:)`: `int` says whether
/// `value` locates an `Int`; `decimals` < 0 means as many as `step` has.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_stepper(
    f: *const c_void,
    env: *mut Header,
    int: u8,
    step: f64,
    from: f64,
    to: f64,
    decimals: i64,
    change_f: *const c_void,
    change_env: *mut Header,
) {
    // (Its buttons, or its arrow, are built-in icons.)
    super::svg::enable_icons();
    let value = Callback::new(f, env);
    let on_change = (!change_f.is_null()).then(|| Callback::new(change_f, change_env));
    let int = int != 0;
    let step = if step.is_finite() && step > 0.0 { step } else { 1.0 };
    let step = if int { step.round().max(1.0) } else { step };
    let decimals = match decimals {
        d if d >= 0 => (d as usize).min(10),
        _ if int => 0,
        _ => decimals_of(step),
    };
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    with_builder(|b| b.leaf(Kind::Stepper { value, int, step, from, to, decimals, on_change }));
}

/// A slider's ends: in order, and finite (a slider can't reach infinity).
fn finite_range(from: f64, to: f64) -> (f64, f64) {
    let from = if from.is_finite() { from } else { 0.0 };
    let to = if to.is_finite() { to } else { from + 1.0 };
    if from <= to { (from, to) } else { (to, from) }
}

/// How many decimals a step like `0.25` needs (at most 6).
fn decimals_of(step: f64) -> usize {
    (0..6).find(|&d| ((step * 10f64.powi(d as i32)).round() - step * 10f64.powi(d as i32)).abs() < 1e-9).unwrap_or(6)
}

/// The number a slider or stepper's binding holds.
pub fn read_number(value: &Callback, int: bool) -> f64 {
    let p = value.locate();
    if int { unsafe { *(p as *const i64) as f64 } } else { unsafe { *(p as *const f64) } }
}

/// Stores a number in a slider or stepper's binding. Returns whether it changed.
pub fn write_number(value: &Callback, int: bool, v: f64) -> bool {
    let p = value.locate();
    unsafe {
        if int {
            let v = v.round() as i64;
            let changed = *(p as *const i64) != v;
            *(p as *mut i64) = v;
            changed
        } else {
            let changed = *(p as *const f64) != v;
            *(p as *mut f64) = v;
            changed
        }
    }
}

/// A slider's value at `t` (0 at its start, 1 at its end), on its steps.
pub fn slider_value(from: f64, to: f64, step: f64, t: f64) -> f64 {
    let v = from + t.clamp(0.0, 1.0) * (to - from);
    let v = if step > 0.0 { from + ((v - from) / step).round() * step } else { v };
    clean(v.clamp(from, to))
}

/// A stepper's value one step up (`dir` 1) or down (-1), within its range.
pub fn stepped(value: f64, step: f64, dir: f64, from: f64, to: f64) -> f64 {
    clean((value + dir * step).clamp(from, to))
}

/// Rounds away the error adding steps leaves (`0.1 + 0.2`).
fn clean(v: f64) -> f64 {
    let r = (v * 1e9).round() / 1e9;
    if r.is_finite() { r } else { v }
}

/// A number with `decimals` decimals.
pub fn format_number(v: f64, decimals: usize) -> String {
    format!("{v:.decimals$}")
}

// ===== ColorPicker =====

// `Color`'s cases with values (after its 18 named ones): `.rgb` and `.rgba`.
const RGB: i32 = 18;
const RGBA: i32 = 19;

unsafe extern "C" fn bytes_eq_rgb(a: *const u8, b: *const u8) -> bool {
    unsafe { std::slice::from_raw_parts(a, 24) == std::slice::from_raw_parts(b, 24) }
}

unsafe extern "C" fn bytes_eq_rgba(a: *const u8, b: *const u8) -> bool {
    unsafe { std::slice::from_raw_parts(a, 32) == std::slice::from_raw_parts(b, 32) }
}

/// The values of `.rgb(red, green, blue)`: three Ints.
static RGB_VALUES: TslType =
    TslType { size: 24, align: 8, retain: None, release: None, eq: Some(bytes_eq_rgb), hash: None, cmp: None, kind: 0 };
/// The values of `.rgba(red, green, blue, opacity)`: three Ints and a Float.
static RGBA_VALUES: TslType =
    TslType { size: 32, align: 8, retain: None, release: None, eq: Some(bytes_eq_rgba), hash: None, cmp: None, kind: 0 };

/// The color a `Color` binding holds.
pub fn read_color(color: &Callback) -> Rgba {
    // A `Color` is its case's tag (an i32), then the box of its values.
    let p = color.locate();
    unsafe {
        let tag = *(p as *const i32);
        let values = *(p.add(8) as *const *const Header);
        super::theme::color(tag, values)
    }
}

/// Stores `c` in a `Color` binding, as `.rgb(…)` (or `.rgba(…)` if it's
/// see-through). Returns whether it changed.
pub fn write_color(color: &Callback, c: Rgba) -> bool {
    if read_color(color) == c {
        return false;
    }
    let p = color.locate();
    let opaque = c.0[3] == 255;
    unsafe {
        let ty: *const TslType = if opaque { &RGB_VALUES } else { &RGBA_VALUES };
        let bx = crate::tsl_box_new(ty);
        let data = (bx as *mut u8).add(crate::BOX_DATA) as *mut i64;
        for i in 0..3 {
            *data.add(i) = c.0[i] as i64;
        }
        if !opaque {
            *(data.add(3) as *mut f64) = c.0[3] as f64 / 255.0;
        }
        let old_tag = *(p as *const i32);
        let old = *(p.add(8) as *const *mut Header);
        *(p as *mut i32) = if opaque { RGB } else { RGBA };
        *(p.add(8) as *mut *mut Header) = bx;
        if old_tag == RGB || old_tag == RGBA {
            crate::tsl_release(old);
        }
    }
    true
}

/// `#rrggbb` (or `#rrggbbaa` when it's see-through).
pub fn hex(c: Rgba) -> String {
    let [r, g, b, a] = c.0;
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// Reads `#rrggbb`, `rrggbb`, `#rgb` or `#rrggbbaa`.
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let h = text.trim().trim_start_matches('#');
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
            Some(Rgba([d(0)?, d(1)?, d(2)?, 255]))
        }
        6 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, 255])),
        8 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
        _ => None,
    }
}

/// The palette's colors, row by row: grays, then hues from light to dark.
pub fn palette() -> Vec<Vec<Rgba>> {
    let grays = [255, 230, 204, 178, 153, 127, 102, 76, 51, 0].map(|v| Rgba([v, v, v, 255])).to_vec();
    // Red, orange, yellow, lime, green, teal, cyan, blue, purple, pink.
    let hues: [f64; 10] = [0.0, 30.0, 52.0, 90.0, 130.0, 170.0, 190.0, 215.0, 270.0, 330.0];
    let mut rows = vec![grays];
    for (s, l) in [(0.75, 0.82), (0.8, 0.62), (0.75, 0.48), (0.7, 0.34), (0.65, 0.22)] {
        rows.push(hues.iter().map(|&h| hsl(h, s, l)).collect());
    }
    rows
}

fn hsl(h: f64, s: f64, l: f64) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let v = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba([v(r), v(g), v(b), 255])
}

thread_local! {
    /// The `Picker(style: .menu)` whose list of options is open.
    pub static OPEN_MENU: Cell<Option<u64>> = const { Cell::new(None) };
    /// The collapsed `RibbonGroup` whose panel of items is open.
    pub static OPEN_GROUP: Cell<Option<u64>> = const { Cell::new(None) };
    /// The `ColorPicker` whose palette is open.
    pub static OPEN_PICKER: Cell<Option<u64>> = const { Cell::new(None) };
    /// The open palette's hex field, and the color the picker had when it was built.
    pub static HEX_FIELD: Cell<Option<u64>> = const { Cell::new(None) };
    pub static PICKED: Cell<Option<Rgba>> = const { Cell::new(None) };
    /// Opened from the keyboard: its hex field starts out being edited.
    pub static FOCUS_HEX: Cell<bool> = const { Cell::new(false) };
}

/// The text of the palette's hex field (the UI thread's only).
static mut HEX_TEXT: *mut Str = std::ptr::null_mut();

/// The hex field's locator.
extern "C" fn hex_text(_env: *mut Header) -> *mut u8 {
    unsafe {
        if HEX_TEXT.is_null() {
            HEX_TEXT = crate::str_new("");
        }
        &raw mut HEX_TEXT as *mut u8
    }
}

/// What the hex field says.
pub fn hex_field_text() -> String {
    unsafe { super::text_of(*(hex_text(std::ptr::null_mut()) as *const *const Str)) }
}

/// Sets the hex field's text.
pub fn set_hex_field_text(text: &str) {
    unsafe {
        let slot = hex_text(std::ptr::null_mut()) as *mut *mut Str;
        let old = *slot;
        *slot = crate::str_new(text);
        crate::tsl_release(old as *mut Header);
    }
}

/// Frees the hex field's text (at exit, for leak checkers).
pub fn forget_hex_text() {
    unsafe {
        let old = std::ptr::replace(&raw mut HEX_TEXT, std::ptr::null_mut());
        crate::tsl_release(old as *mut Header);
    }
}

/// `ColorPicker(color:onChange:)`. With its palette open, the palette is
/// built inside it (it's laid out and drawn over the rest of the window).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_color_picker(f: *const c_void, env: *mut Header, change_f: *const c_void, change_env: *mut Header) {
    let color = Callback::new(f, env);
    let current = read_color(&color);
    let on_change = (!change_f.is_null()).then(|| Callback::new(change_f, change_env));
    with_builder(|b| {
        b.begin(Kind::ColorPicker { color, on_change }, 0);
        if OPEN_PICKER.get() == Some(b.parent().key) {
            PICKED.set(Some(current));
            build_palette(b);
        }
        b.end();
    });
}

/// The palette: rows of colors, and a field for a color's hex code.
fn build_palette(b: &mut Builder) {
    let pad = super::layout::Insets { top: 10.0, leading: 10.0, bottom: 10.0, trailing: 10.0 };
    b.begin(Kind::VStack(6.0, 0), 0);
    {
        let m = &mut b.parent().mods;
        m.padding = pad;
        m.background = Some((super::theme::theme().popup, 8.0));
        m.border = Some((super::theme::theme().field_border, 1.0, 8.0));
    }
    for row in palette() {
        b.begin(Kind::HStack(4.0, 1), 0);
        for c in row {
            b.leaf(Kind::Swatch(c));
            if let Some(n) = b.last() {
                n.mods.width = Some(18.0);
                n.mods.height = Some(18.0);
            }
        }
        b.end();
    }
    b.begin(Kind::HStack(6.0, 1), 0);
    b.leaf(Kind::Text("Hex".into()));
    if let Some(n) = b.last() {
        n.mods.font_size = Some(12.0);
        n.mods.color = Some(super::theme::theme().colors[super::theme::SECONDARY]);
    }
    b.leaf(Kind::TextField { placeholder: "Hex".into(), text: Callback::new(hex_text as *const c_void, std::ptr::null_mut()), secure: false, lines: 1, search: false, on_change: None });
    if let Some(n) = b.last() {
        n.mods.width = Some(120.0);
        n.mods.autofocus = FOCUS_HEX.replace(false);
        HEX_FIELD.set(Some(n.key));
    }
    b.end();
    b.end();
}

// ===== DisclosureGroup =====

/// `DisclosureGroup(title, isExpanded:)`: opens it, and says whether it's
/// open (then its content is built; `tsl_ui_end` closes it either way).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_begin_disclosure(title: *const Str, f: *const c_void, env: *mut Header) -> u8 {
    // (Its buttons, or its arrow, are built-in icons.)
    super::svg::enable_icons();
    let title = unsafe { text_of(title) };
    let expanded = Callback::new(f, env);
    let open = unsafe { *expanded.locate() } & 1;
    with_builder(|b| b.begin(Kind::Disclosure { title, expanded }, 0));
    open
}

// ===== LazyVStack =====

thread_local! {
    /// The rows each `LazyVStack` is to build (from, up to), by key: set
    /// after layout to those near what shows (see `App::lazy_rows`).
    pub static LAZY: RefCell<HashMap<u64, (usize, usize)>> = RefCell::new(HashMap::new());
}

/// Rows built before a list has been laid out (and what shows of it is
/// known): none, as it's built again right after its first layout.
const FIRST_ROWS: usize = 0;

/// `LazyVStack(count:rowHeight:) { i in … }`: builds the rows near what
/// shows, each with `f(env, i)` (the closure is the runtime's).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_lazy(count: i64, row_height: f64, f: *const c_void, env: *mut Header) {
    lazy(count, row_height, f, env, false);
}

/// `LazyHStack(count:columnWidth:) { i in … }`: like a `LazyVStack`, with
/// columns side by side.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_lazy_across(count: i64, width: f64, f: *const c_void, env: *mut Header) {
    lazy(count, width, f, env, true);
}

fn lazy(count: i64, row_height: f64, f: *const c_void, env: *mut Header, across: bool) {
    let rows = Callback::new(f, env);
    let count = count.max(0) as usize;
    let row_height = if row_height.is_finite() { row_height.max(1.0) } else { 20.0 };
    let mut key = None;
    with_builder(|b| {
        b.begin(Kind::Lazy { count, row_height, first: 0, across }, 0);
        key = Some(b.parent().key);
    });
    // (Not in a build: nothing to do.)
    let Some(key) = key else { return };
    let (from, to) = LAZY.with(|l| l.borrow().get(&key).copied()).unwrap_or((0, FIRST_ROWS));
    let to = to.min(count);
    let from = from.min(to);
    with_builder(|b| {
        if let Kind::Lazy { first, .. } = &mut b.parent().kind {
            *first = from;
        }
    });
    for i in from..to {
        // Each row is known by its number, so it keeps its state as the list scrolls.
        with_builder(|b| {
            b.pending_id = Some(i as u64);
            // (A column's views are centered across it.)
            b.begin(Kind::VStack(0.0, across as u8), 0);
        });
        rows.call_with_int(i as i64);
        with_builder(|b| b.end());
    }
    with_builder(|b| b.end());
}
