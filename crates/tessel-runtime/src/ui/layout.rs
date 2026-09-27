//! Layout: sizes and positions views, following SwiftUI's defaults.
//!
//! - Stacks size to their content and center their children across their axis.
//! - `Spacer` takes the leftover space along its stack's axis; text fields
//!   stretch horizontally; scroll views take all the space they're offered.
//!   A stack containing something flexible becomes flexible too.
//! - The root centers its content in the window.

use std::collections::HashMap;

use super::render::{Paint, TextEngine};
use super::{Kind, Node, Rgba, COLORS};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn inset(&self, d: f64) -> Rect {
        Rect { x: self.x + d, y: self.y + d, w: (self.w - 2.0 * d).max(0.0), h: (self.h - 2.0 * d).max(0.0) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

/// Layout results for one node.
#[derive(Default)]
pub struct Box {
    /// Outer frame, in window coordinates (points).
    pub rect: Rect,
    /// Measured size.
    pub size: Size,
    /// Laid-out text (labels, text, field contents).
    pub text: Option<parley::Layout<Paint>>,
    /// A text field's placeholder, and whether the field is empty.
    pub placeholder: Option<parley::Layout<Paint>>,
    pub empty: bool,
    /// For code editors: the width of the line-number gutter.
    pub gutter: f64,
    /// For tab views: each tab's title, rectangle and close button.
    pub tabs: Vec<Tab>,
    /// For scroll views: the height of the content.
    pub content_h: f64,
    /// For pickers: the rectangle of each option.
    pub segments: Vec<Rect>,
    /// Inherited style, resolved for this node.
    pub style: Style,
}

pub struct Tab {
    pub title: parley::Layout<Paint>,
    pub rect: Rect,
    pub close: Option<Rect>,
}

pub const TAB_BAR: f64 = 34.0;

/// Styles inherited down the tree (like SwiftUI's environment).
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub size: f64,
    pub weight: f32,
    pub color: Rgba,
    pub disabled: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style { size: 14.0, weight: 400.0, color: COLORS[0], disabled: false }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

const BUTTON_PAD: (f64, f64) = (14.0, 6.0);
const FIELD_PAD: (f64, f64) = (8.0, 6.0);
const SEGMENT_PAD: f64 = 12.0;
const SWITCH: Size = Size { w: 38.0, h: 22.0 };
const GROUP_SPACING: f64 = 8.0;

pub struct Layouter<'a> {
    pub text: &'a mut TextEngine,
    pub scroll: &'a HashMap<u64, f64>,
}

impl Layouter<'_> {
    pub fn run(&mut self, root: &mut Node, w: f64, h: f64) {
        self.measure(root, Size { w, h }, &Style::default());
        self.place(root, Rect { x: 0.0, y: 0.0, w, h });
    }

    fn style_for(n: &Node, parent: &Style) -> Style {
        Style {
            size: n.mods.font_size.unwrap_or(parent.size),
            weight: n.mods.weight.unwrap_or(parent.weight),
            color: n.mods.color.unwrap_or(parent.color),
            disabled: parent.disabled || n.mods.disabled,
        }
    }

    fn text_layout(&mut self, s: &str, style: &Style, max_w: Option<f64>) -> parley::Layout<Paint> {
        self.text.layout(s, style.size, style.weight, style.color, max_w)
    }

    fn measure(&mut self, n: &mut Node, avail: Size, parent: &Style) -> Size {
        let style = Self::style_for(n, parent);
        n.layout.style = style;
        if n.mods.hidden {
            n.layout.size = Size::default();
            return n.layout.size;
        }
        let pad = n.mods.padding.unwrap_or(0.0);
        let inner_avail = Size {
            w: (n.mods.width.unwrap_or(avail.w) - 2.0 * pad).max(0.0),
            h: (n.mods.height.unwrap_or(avail.h) - 2.0 * pad).max(0.0),
        };
        let inner = self.measure_inner(n, inner_avail, &style);
        n.layout.size = Size {
            w: n.mods.width.unwrap_or(inner.w + 2.0 * pad),
            h: n.mods.height.unwrap_or(inner.h + 2.0 * pad),
        };
        n.layout.size
    }

    fn measure_inner(&mut self, n: &mut Node, avail: Size, style: &Style) -> Size {
        let finite = |w: f64| w.is_finite().then_some(w);
        match &n.kind {
            Kind::Text(s) => {
                let s = s.clone();
                let t = self.text_layout(&s, style, finite(avail.w));
                let size = Size { w: t.width() as f64, h: t.height() as f64 };
                n.layout.text = Some(t);
                size
            }
            Kind::Button { label, .. } => {
                let label = label.clone();
                let t = self.text_layout(&label, style, None);
                let size = Size { w: t.width() as f64 + 2.0 * BUTTON_PAD.0, h: t.height() as f64 + 2.0 * BUTTON_PAD.1 };
                n.layout.text = Some(t);
                size
            }
            Kind::TextField { placeholder, text } => {
                let value = read_string(text);
                let placeholder = placeholder.clone();
                let ph = self.text_layout(&placeholder, style, None);
                let t = self.text_layout(&value, style, None);
                let line_h = (ph.height() as f64).max(t.height() as f64);
                let w = (t.width().max(ph.width()) as f64 + 2.0 * FIELD_PAD.0).max(120.0);
                n.layout.empty = value.is_empty();
                n.layout.text = Some(t);
                n.layout.placeholder = Some(ph);
                Size { w: w.min(avail.w.max(60.0)), h: line_h + 2.0 * FIELD_PAD.1 }
            }
            Kind::Toggle { label, .. } => {
                if label.is_empty() {
                    return SWITCH;
                }
                let label = label.clone();
                let t = self.text_layout(&label, style, None);
                let size = Size { w: t.width() as f64 + 8.0 + SWITCH.w, h: (t.height() as f64).max(SWITCH.h) };
                n.layout.text = Some(t);
                size
            }
            Kind::CodeEditor { text, .. } => {
                let value = read_string(text);
                let t = self.text.code_layout(&value, 13.0);
                let count = t.lines().count().max(1);
                let digits = count.to_string().len().max(2);
                let numbers: Vec<String> = (1..=count).map(|i| format!("{i:>digits$}")).collect();
                let numbers = self.text.mono_layout(&numbers.join("\n"), 13.0, Rgba([160, 160, 166, 255]));
                n.layout.gutter = numbers.width() as f64 + 14.0;
                n.layout.content_h = t.height() as f64 + 12.0;
                n.layout.text = Some(t);
                n.layout.placeholder = Some(numbers);
                let w = if avail.w.is_finite() { avail.w } else { 480.0 };
                let h = if avail.h.is_finite() { avail.h } else { 320.0 };
                Size { w: w.max(160.0), h: h.max(80.0) }
            }
            Kind::TabView { close, .. } => {
                let closable = close.is_some();
                let titles: Vec<String> =
                    n.children.iter().map(|c| c.mods.tab_title.clone().unwrap_or_else(|| "Untitled".into())).collect();
                let mut tabs = Vec::new();
                for title in titles {
                    let t = self.text_layout(&title, style, None);
                    tabs.push(Tab { title: t, rect: Rect::default(), close: closable.then(Rect::default) });
                }
                n.layout.tabs = tabs;
                let selected = n.selected_tab();
                let page = Size { w: avail.w, h: (avail.h - TAB_BAR).max(0.0) };
                if let Some(c) = n.children.get_mut(selected) {
                    self.measure(c, page, style);
                }
                Size {
                    w: if avail.w.is_finite() { avail.w } else { 480.0 },
                    h: if avail.h.is_finite() { avail.h } else { 320.0 },
                }
            }
            Kind::Divider => Size { w: 1.0, h: 1.0 },
            Kind::Spacer => Size::default(),
            Kind::Image(_) => Size { w: 64.0, h: 64.0 },
            Kind::Picker { .. } => {
                let mut w = 0.0;
                let mut h: f64 = 0.0;
                for c in &mut n.children {
                    let s = self.measure(c, avail, style);
                    w += s.w + 2.0 * SEGMENT_PAD;
                    h = h.max(s.h + 10.0);
                }
                Size { w: w + 4.0, h: h + 4.0 }
            }
            Kind::VStack(spacing, _) => self.measure_stack(n, *spacing, Axis::Y, avail, style),
            Kind::Group => self.measure_stack(n, GROUP_SPACING, Axis::Y, avail, style),
            Kind::HStack(spacing, _) => self.measure_stack(n, *spacing, Axis::X, avail, style),
            Kind::ZStack | Kind::Root => {
                let mut size = Size::default();
                for c in &mut n.children {
                    let s = self.measure(c, avail, style);
                    size.w = size.w.max(s.w);
                    size.h = size.h.max(s.h);
                }
                size
            }
            Kind::Scroll => {
                let mut content_h: f64 = 0.0;
                let mut w: f64 = 0.0;
                for c in &mut n.children {
                    let s = self.measure(c, Size { w: avail.w, h: f64::INFINITY }, style);
                    content_h = content_h.max(s.h);
                    w = w.max(s.w);
                }
                n.layout.content_h = content_h;
                Size { w, h: content_h.min(avail.h) }
            }
        }
    }

    fn measure_stack(&mut self, n: &mut Node, spacing: f64, axis: Axis, avail: Size, style: &Style) -> Size {
        let along = |s: Size| if axis == Axis::Y { s.h } else { s.w };
        let across = |s: Size| if axis == Axis::Y { s.w } else { s.h };
        let avail_along = along(avail);
        let visible = n.children.iter().filter(|c| !c.mods.hidden).count();
        let gaps = spacing * visible.saturating_sub(1) as f64;
        let mut used = 0.0;
        let mut max_across: f64 = 0.0;
        // Fixed-size children first; flexible ones share what's left.
        let mut flexible = Vec::new();
        for (i, c) in n.children.iter_mut().enumerate() {
            if c.mods.hidden {
                continue;
            }
            if flex(c, axis, Some(axis)) {
                flexible.push(i);
                continue;
            }
            let remaining = (avail_along - used).max(0.0);
            let a = if axis == Axis::Y { Size { w: avail.w, h: remaining } } else { Size { w: remaining, h: avail.h } };
            let s = self.measure(c, a, style);
            used += along(s);
            max_across = max_across.max(across(s));
        }
        let leftover = (avail_along - used - gaps).max(0.0);
        let share = if avail_along.is_finite() && !flexible.is_empty() { leftover / flexible.len() as f64 } else { 0.0 };
        let mut flex_min = 0.0;
        for i in flexible {
            let c = &mut n.children[i];
            let a = if axis == Axis::Y { Size { w: avail.w, h: share } } else { Size { w: share, h: avail.h } };
            let s = self.measure(c, a, style);
            flex_min += along(s);
            max_across = max_across.max(across(s));
        }
        let has_flex = n.children.iter().any(|c| flex(c, axis, Some(axis)));
        let total_along = if has_flex && avail_along.is_finite() { avail_along } else { used + gaps + flex_min };
        let cross_axis = if axis == Axis::Y { Axis::X } else { Axis::Y };
        let fills_across = n.children.iter().any(|c| flex(c, cross_axis, Some(axis)));
        let total_across = if fills_across && across(avail).is_finite() { across(avail) } else { max_across };
        if axis == Axis::Y {
            Size { w: total_across, h: total_along }
        } else {
            Size { w: total_along, h: total_across }
        }
    }

    fn place(&mut self, n: &mut Node, rect: Rect) {
        n.layout.rect = rect;
        if n.mods.hidden {
            return;
        }
        let content = rect.inset(n.mods.padding.unwrap_or(0.0));
        match &n.kind {
            Kind::VStack(spacing, align) => self.place_stack(n, *spacing, *align, Axis::Y, content),
            Kind::Group => self.place_stack(n, GROUP_SPACING, 1, Axis::Y, content),
            Kind::HStack(spacing, align) => self.place_stack(n, *spacing, *align, Axis::X, content),
            Kind::ZStack | Kind::Root => {
                for c in &mut n.children {
                    let w = if flex(c, Axis::X, None) { content.w } else { c.layout.size.w.min(content.w) };
                    let h = if flex(c, Axis::Y, None) { content.h } else { c.layout.size.h.min(content.h) };
                    let r = Rect { x: content.x + (content.w - w) / 2.0, y: content.y + (content.h - h) / 2.0, w, h };
                    self.place(c, r);
                }
            }
            Kind::Scroll => {
                let offset = self.scroll.get(&n.key).copied().unwrap_or(0.0);
                let max = (n.layout.content_h - content.h).max(0.0);
                let offset = offset.clamp(0.0, max);
                // Scrolling content starts at the top left (lists read that way).
                for c in &mut n.children {
                    let w = if flex(c, Axis::X, Some(Axis::Y)) { content.w } else { c.layout.size.w.min(content.w) };
                    let r = Rect { x: content.x, y: content.y - offset, w, h: c.layout.size.h };
                    self.place(c, r);
                }
            }
            Kind::Picker { .. } => {
                let mut x = content.x + 2.0;
                let mut segments = Vec::new();
                for c in &mut n.children {
                    let w = c.layout.size.w + 2.0 * SEGMENT_PAD;
                    let seg = Rect { x, y: content.y + 2.0, w, h: content.h - 4.0 };
                    let label = Rect {
                        x: x + SEGMENT_PAD,
                        y: seg.y + (seg.h - c.layout.size.h) / 2.0,
                        w: c.layout.size.w,
                        h: c.layout.size.h,
                    };
                    self.place(c, label);
                    segments.push(seg);
                    x += w;
                }
                n.layout.segments = segments;
            }
            Kind::TabView { .. } => {
                let mut x = content.x;
                for tab in &mut n.layout.tabs {
                    let w = (tab.title.width() as f64 + 28.0 + if tab.close.is_some() { 18.0 } else { 0.0 }).max(90.0);
                    tab.rect = Rect { x, y: content.y, w, h: TAB_BAR };
                    if let Some(close) = &mut tab.close {
                        *close = Rect { x: x + w - 24.0, y: content.y + (TAB_BAR - 16.0) / 2.0, w: 16.0, h: 16.0 };
                    }
                    x += w;
                }
                let selected = n.selected_tab();
                let page = Rect { x: content.x, y: content.y + TAB_BAR, w: content.w, h: (content.h - TAB_BAR).max(0.0) };
                if let Some(c) = n.children.get_mut(selected) {
                    self.place(c, page);
                }
            }
            Kind::TextField { .. } => {
                // Re-wrap nothing; the field just takes the width it was given.
            }
            _ => {}
        }
    }

    fn place_stack(&mut self, n: &mut Node, spacing: f64, align: u8, axis: Axis, content: Rect) {
        let (avail_along, avail_across) = if axis == Axis::Y { (content.h, content.w) } else { (content.w, content.h) };
        let visible: Vec<usize> = (0..n.children.len()).filter(|&i| !n.children[i].mods.hidden).collect();
        let gaps = spacing * visible.len().saturating_sub(1) as f64;
        let fixed: f64 = visible
            .iter()
            .map(|&i| &n.children[i])
            .filter(|c| !flex(c, axis, Some(axis)))
            .map(|c| if axis == Axis::Y { c.layout.size.h } else { c.layout.size.w })
            .sum();
        let flexible = visible.iter().filter(|&&i| flex(&n.children[i], axis, Some(axis))).count();
        let share = if flexible > 0 { ((avail_along - fixed - gaps) / flexible as f64).max(0.0) } else { 0.0 };
        // Without flexible children, center the run of children along the axis.
        let total = fixed + gaps + share * flexible as f64;
        let mut pos = if flexible == 0 { (avail_along - total).max(0.0) / 2.0 } else { 0.0 };
        let cross_axis = if axis == Axis::Y { Axis::X } else { Axis::Y };
        for i in visible {
            let c = &mut n.children[i];
            let (size_along, size_across) = if axis == Axis::Y { (c.layout.size.h, c.layout.size.w) } else { (c.layout.size.w, c.layout.size.h) };
            let along = if flex(c, axis, Some(axis)) { share } else { size_along };
            let across = if flex(c, cross_axis, Some(axis)) { avail_across } else { size_across.min(avail_across) };
            let offset_across = match align {
                0 => 0.0,
                2 => avail_across - across,
                _ => (avail_across - across) / 2.0,
            };
            let r = if axis == Axis::Y {
                Rect { x: content.x + offset_across, y: content.y + pos, w: across, h: along }
            } else {
                Rect { x: content.x + pos, y: content.y + offset_across, w: along, h: across }
            };
            // Text re-wraps to the width it finally gets.
            if let (Kind::Text(s), Axis::X) = (&c.kind, cross_axis) {
                if (r.w - c.layout.size.w).abs() > 0.5 {
                    let style = c.layout.style;
                    let s = s.clone();
                    c.layout.text = Some(self.text_layout(&s, &style, Some(r.w)));
                }
            }
            self.place(c, r);
            pos += along + spacing;
        }
    }
}

/// Does `n` stretch along `axis` inside a parent laid out along `parent`?
fn flex(n: &Node, axis: Axis, parent: Option<Axis>) -> bool {
    if n.mods.hidden {
        return false;
    }
    let fixed = match axis {
        Axis::X => n.mods.width.is_some(),
        Axis::Y => n.mods.height.is_some(),
    };
    if fixed {
        return false;
    }
    match &n.kind {
        Kind::Spacer => parent == Some(axis),
        Kind::TextField { .. } => axis == Axis::X,
        Kind::Scroll | Kind::CodeEditor { .. } | Kind::TabView { .. } => true,
        // A divider spans across its stack.
        Kind::Divider => parent.is_some() && parent != Some(axis),
        Kind::VStack(..) | Kind::Group => n.children.iter().any(|c| flex(c, axis, Some(Axis::Y))),
        Kind::HStack(..) => n.children.iter().any(|c| flex(c, axis, Some(Axis::X))),
        Kind::ZStack | Kind::Root => n.children.iter().any(|c| flex(c, axis, None)),
        _ => false,
    }
}

/// Reads the `String` a text field's locator points at.
pub fn read_string(locator: &super::Callback) -> String {
    let p = locator.locate() as *const *const crate::Str;
    unsafe { crate::str_text(*p) }.to_string()
}
