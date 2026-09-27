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
        self.inset_by(Insets { top: d, leading: d, bottom: d, trailing: d })
    }

    pub fn inset_by(&self, i: Insets) -> Rect {
        Rect {
            x: self.x + i.leading,
            y: self.y + i.top,
            w: (self.w - i.horizontal()).max(0.0),
            h: (self.h - i.vertical()).max(0.0),
        }
    }
}

/// Space around a view's content, per edge (`.padding`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub leading: f64,
    pub bottom: f64,
    pub trailing: f64,
}

impl Insets {
    pub fn horizontal(&self) -> f64 {
        self.leading + self.trailing
    }

    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
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
    /// The content's own size (before `.frame` and padding).
    pub natural: Size,
    /// Where the content is drawn: `rect` less padding, placed by the frame's alignment.
    pub content: Rect,
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
    /// How lines of text line up: 0 leading, 1 center, 2 trailing.
    pub text_align: u8,
}

impl Default for Style {
    fn default() -> Style {
        Style { size: 14.0, weight: 400.0, color: COLORS[0], disabled: false, text_align: 0 }
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
            text_align: n.mods.text_align.unwrap_or(parent.text_align),
        }
    }

    fn text_layout(&mut self, s: &str, style: &Style, max_w: Option<f64>) -> parley::Layout<Paint> {
        self.text.layout(s, style.size, style.weight, style.color, max_w)
    }

    /// A `Text`'s layout: like `text_layout`, with its lines aligned.
    fn text_block(&mut self, s: &str, style: &Style, max_w: Option<f64>) -> parley::Layout<Paint> {
        let mut t = self.text_layout(s, style, max_w);
        let align = match style.text_align {
            1 => parley::Alignment::Center,
            2 => parley::Alignment::End,
            _ => return t,
        };
        // Parley aligns within the width lines were broken at; use the widest
        // line's, so the lines line up inside the text's own box.
        let w = t.width();
        t.break_all_lines(Some(w + 0.5));
        t.align(align, parley::AlignmentOptions::default());
        t
    }

    fn measure(&mut self, n: &mut Node, avail: Size, parent: &Style) -> Size {
        let style = Self::style_for(n, parent);
        n.layout.style = style;
        if n.mods.hidden {
            n.layout.size = Size::default();
            return n.layout.size;
        }
        let pad = n.mods.padding;
        let offered = Size {
            w: n.mods.width.unwrap_or(avail.w.min(n.mods.max_width.unwrap_or(f64::INFINITY))),
            h: n.mods.height.unwrap_or(avail.h.min(n.mods.max_height.unwrap_or(f64::INFINITY))),
        };
        let inner_avail = Size { w: (offered.w - pad.horizontal()).max(0.0), h: (offered.h - pad.vertical()).max(0.0) };
        let inner = self.measure_inner(n, inner_avail, &style);
        n.layout.natural = inner;
        // With a max size, take all the space offered (up to the max), but
        // never less than the content.
        let grow = |fixed: Option<f64>, max: Option<f64>, offered: f64, content: f64| match (fixed, max) {
            (Some(v), _) => v,
            (None, Some(_)) if offered.is_finite() => offered.max(content),
            (None, Some(m)) => content.min(m),
            (None, None) => content,
        };
        n.layout.size = Size {
            w: grow(n.mods.width, n.mods.max_width, offered.w, inner.w + pad.horizontal()).max(n.mods.min_width.unwrap_or(0.0)),
            h: grow(n.mods.height, n.mods.max_height, offered.h, inner.h + pad.vertical()).max(n.mods.min_height.unwrap_or(0.0)),
        };
        n.layout.size
    }

    fn measure_inner(&mut self, n: &mut Node, avail: Size, style: &Style) -> Size {
        let finite = |w: f64| w.is_finite().then_some(w);
        match &n.kind {
            Kind::Text(s) => {
                let s = s.clone();
                let t = self.text_block(&s, style, finite(avail.w));
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
            Kind::Svg { source, icon } => {
                // Icons follow the text size; other SVGs have their own size,
                // scaled to fit a `.frame` if there is one (keeping proportions).
                if icon.is_some() {
                    let s = (style.size * 1.15).round();
                    return Size { w: s, h: s };
                }
                let Some(svg) = super::svg::load(source, style.color) else { return Size { w: 16.0, h: 16.0 } };
                let (w, h) = (svg.width.max(1.0), svg.height.max(1.0));
                if n.mods.width.is_some() || n.mods.height.is_some() {
                    let s = (avail.w / w).min(avail.h / h);
                    return Size { w: w * s, h: h * s };
                }
                Size { w, h }
            }
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
            Kind::ZStack(_) | Kind::Root => {
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

    fn place(&mut self, n: &mut Node, mut rect: Rect) {
        // A stack may offer a flexible view more than its max size.
        let (max_w, max_h) = (n.mods.max_width.unwrap_or(f64::INFINITY), n.mods.max_height.unwrap_or(f64::INFINITY));
        if rect.w > max_w || rect.h > max_h {
            rect = aligned(rect, Size { w: rect.w.min(max_w), h: rect.h.min(max_h) }, 1);
        }
        // …or less than its min size (it then overflows, rather than squash its content).
        rect.w = rect.w.max(n.mods.min_width.unwrap_or(0.0));
        rect.h = rect.h.max(n.mods.min_height.unwrap_or(0.0));
        n.layout.rect = rect;
        if n.mods.hidden {
            return;
        }
        let mut content = rect.inset_by(n.mods.padding);
        // Content with a size of its own sits in a larger frame by the frame's
        // alignment. Text and SVGs are centered by default; stacks lay out
        // their children across the whole frame unless an alignment is given.
        let align = match &n.kind {
            Kind::Text(_) | Kind::Svg { .. } => Some(n.mods.frame_align.unwrap_or(1)),
            Kind::VStack(..) | Kind::HStack(..) | Kind::ZStack(_) => n.mods.frame_align,
            _ => None,
        };
        if let Some(align) = align {
            content = aligned(content, n.layout.natural, align);
        }
        n.layout.content = content;
        match &n.kind {
            Kind::VStack(spacing, align) => self.place_stack(n, *spacing, *align, Axis::Y, content),
            Kind::Group => self.place_stack(n, GROUP_SPACING, 1, Axis::Y, content),
            Kind::HStack(spacing, align) => self.place_stack(n, *spacing, *align, Axis::X, content),
            Kind::ZStack(_) | Kind::Root => {
                let align = if let Kind::ZStack(a) = n.kind { a } else { 1 };
                for c in &mut n.children {
                    let w = if flex(c, Axis::X, None) { content.w } else { c.layout.size.w.min(content.w) };
                    let h = if flex(c, Axis::Y, None) { content.h } else { c.layout.size.h.min(content.h) };
                    let r = aligned(content, Size { w, h }, align);
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
                    let t = self.text_block(&s, &style, Some((r.w - c.mods.padding.horizontal()).max(0.0)));
                    c.layout.natural = Size { w: t.width() as f64, h: t.height() as f64 };
                    c.layout.text = Some(t);
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
    let grows = match axis {
        Axis::X => n.mods.max_width.is_some(),
        Axis::Y => n.mods.max_height.is_some(),
    };
    if grows {
        return true;
    }
    match &n.kind {
        Kind::Spacer => parent == Some(axis),
        Kind::TextField { .. } => axis == Axis::X,
        Kind::Scroll | Kind::CodeEditor { .. } | Kind::TabView { .. } => true,
        // A divider spans across its stack.
        Kind::Divider => parent.is_some() && parent != Some(axis),
        Kind::VStack(..) | Kind::Group => n.children.iter().any(|c| flex(c, axis, Some(Axis::Y))),
        Kind::HStack(..) => n.children.iter().any(|c| flex(c, axis, Some(Axis::X))),
        Kind::ZStack(_) | Kind::Root => n.children.iter().any(|c| flex(c, axis, None)),
        _ => false,
    }
}

/// Reads the `String` a text field's locator points at.
pub fn read_string(locator: &super::Callback) -> String {
    let p = locator.locate() as *const *const crate::Str;
    unsafe { crate::str_text(*p) }.to_string()
}

/// A `size` box inside `outer`, placed by an `Alignment` tag.
fn aligned(outer: Rect, size: Size, align: u8) -> Rect {
    let (h, v) = super::align_2d(align);
    let (w, hgt) = (size.w.min(outer.w), size.h.min(outer.h));
    let frac = |p: u8| p as f64 / 2.0;
    Rect { x: outer.x + (outer.w - w) * frac(h), y: outer.y + (outer.h - hgt) * frac(v), w, h: hgt }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_boxes_in_a_frame() {
        let outer = Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 };
        let size = Size { w: 40.0, h: 10.0 };
        let at = |tag| {
            let r = aligned(outer, size, tag);
            (r.x, r.y)
        };
        assert_eq!(at(5), (10.0, 20.0)); // topLeading
        assert_eq!(at(1), (40.0, 40.0)); // center
        assert_eq!(at(0), (10.0, 40.0)); // leading
        assert_eq!(at(3), (40.0, 20.0)); // top
        assert_eq!(at(8), (70.0, 60.0)); // bottomTrailing
        // Content larger than the frame is clipped to it.
        let big = aligned(outer, Size { w: 500.0, h: 500.0 }, 8);
        assert_eq!((big.x, big.y, big.w, big.h), (10.0, 20.0, 100.0, 50.0));
    }
}
