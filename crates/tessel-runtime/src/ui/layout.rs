//! Layout: sizes and positions views, following SwiftUI's defaults.
//!
//! - Stacks size to their content and center their children across their axis.
//! - `Spacer` takes the leftover space along its stack's axis; text fields
//!   stretch horizontally; scroll views take all the space they're offered.
//!   A stack containing something flexible becomes flexible too.
//! - The root centers its content in the window.

use std::collections::HashMap;

use super::render::{TextEngine, CODE_PAD_Y, GUTTER_PAD};
use super::theme::{self, theme};
use super::{Kind, Node, Rgba};

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
    pub text: Option<std::rc::Rc<super::text::TextLayout>>,
    /// A text field's placeholder, and whether the field is empty.
    pub placeholder: Option<std::rc::Rc<super::text::TextLayout>>,
    pub empty: bool,
    /// For code editors: the width of the line-number gutter.
    pub gutter: f64,
    /// For tab views: each tab's title, rectangle and close button.
    pub tabs: Vec<Tab>,
    /// A ribbon group (and its items) shown small, for lack of room.
    pub compact: bool,
    /// A ribbon group shown as one button, for even less room: its items
    /// are in `panel`, under the button, which shows while it's open
    /// (`controls::OPEN_GROUP`).
    pub collapsed: bool,
    pub panel: Rect,
    /// For scroll views: the height of the content (and its width, for one
    /// that scrolls across).
    pub content_h: f64,
    pub content_w: f64,
    /// For pickers: the rectangle of each option.
    pub segments: Vec<Rect>,
    /// For Markdown: what to draw, and where its links are.
    pub markdown: Option<std::rc::Rc<super::markdown::Laid>>,
    /// For text cut short by its `.lineLimit`: it shows only the start of
    /// its text, then "…".
    pub cut: bool,
    /// For a view that's turned or scaled: its size as it is (`size` is
    /// the room it takes, turned).
    pub turned: Option<Size>,
    /// For grids: how its columns and rows came out (see `grid`).
    pub grid: Option<std::rc::Rc<GridSizes>>,
    /// Inherited style, resolved for this node.
    pub style: Style,
}

pub struct Tab {
    pub title: std::rc::Rc<super::text::TextLayout>,
    pub rect: Rect,
    pub close: Option<Rect>,
}

pub const TAB_BAR: f64 = 34.0;
/// A ribbon's row of tabs, and the height of its groups (their items, and
/// their titles under them).
pub const RIBBON_TABS: f64 = 30.0;
pub const RIBBON_BODY: f64 = 96.0;
/// A small item of a ribbon (three to a column), and the space around and
/// between them.
pub const RIBBON_ROW: f64 = 22.0;
pub const RIBBON_PAD: f64 = 4.0;
const RIBBON_GAP: f64 = 2.0;
/// The height of a ribbon group's title, under its items.
pub const RIBBON_TITLE: f64 = 18.0;
/// The size of a small item's icon, and of a large one's.
pub const RIBBON_ICON: f64 = 16.0;
pub const RIBBON_LARGE_ICON: f64 = 32.0;

/// How a ribbon group's items go in columns (by their place among its
/// children, once they're measured): a large item has a column of its own;
/// small ones (and other views) stack, three small ones to a column, as
/// many as fit.
pub fn ribbon_columns(n: &Node) -> Vec<Vec<usize>> {
    let compact = n.layout.compact;
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let (mut small, mut used): (Vec<usize>, f64) = (Vec::new(), 0.0);
    for (i, c) in n.children.iter().enumerate() {
        if c.mods.hidden || c.kind.is_menu() {
            continue;
        }
        if matches!(c.kind, Kind::RibbonItem { large: true, .. }) && !compact {
            if !small.is_empty() {
                columns.push(std::mem::take(&mut small));
            }
            columns.push(vec![i]);
            used = 0.0;
            continue;
        }
        let h = c.layout.size.h.max(RIBBON_ROW);
        if !small.is_empty() && used + RIBBON_ROW_GAP + h > RIBBON_LARGE + 0.5 {
            columns.push(std::mem::take(&mut small));
            used = 0.0;
        }
        used += if small.is_empty() { h } else { RIBBON_ROW_GAP + h };
        small.push(i);
    }
    if !small.is_empty() {
        columns.push(small);
    }
    columns
}

/// Between the small items of a column.
const RIBBON_ROW_GAP: f64 = 1.0;

/// The height a large ribbon item has: all of its group but the title.
pub const RIBBON_LARGE: f64 = RIBBON_BODY - RIBBON_TITLE - 2.0 * RIBBON_PAD;

/// Styles inherited down the tree (like SwiftUI's environment).
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub size: f64,
    pub weight: f32,
    /// `.font(family:italic:)`.
    pub face: super::text::Face,
    pub color: Rgba,
    pub disabled: bool,
    /// How lines of text line up: 0 leading, 1 center, 2 trailing.
    pub text_align: u8,
    /// `.animation(seconds)`: how long changes take; 0 for at once.
    pub animation: f64,
    /// How they move (see `anim::ease`).
    pub curve: u8,
    /// `.selectable()`: text can be selected and copied.
    pub selectable: bool,
    /// `.lineLimit(n)`: the most lines text shows (0 for no limit).
    pub line_limit: usize,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            size: 14.0,
            weight: 400.0,
            face: Default::default(),
            color: theme().colors[theme::PRIMARY],
            disabled: false,
            text_align: 0,
            animation: 0.0,
            curve: 0,
            selectable: false,
            line_limit: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

const BUTTON_PAD: (f64, f64) = (14.0, 6.0);
const FIELD_PAD: (f64, f64) = (8.0, 6.0);
/// Room before a search field's text (for its magnifying glass), and
/// after it (for its clear button).
pub const SEARCH_LEFT: f64 = 26.0;
pub const SEARCH_RIGHT: f64 = 24.0;
/// Room kept beside the text of a field of several lines, for its scroll bar.
const LINES_BAR: f64 = 8.0;
const SEGMENT_PAD: f64 = 12.0;
/// A radio button (`Picker(style: .radio)`), the room between it and its
/// option, and between options.
pub const RADIO: f64 = 16.0;
/// A menu picker's room before its option, its arrow, the room for the
/// check mark before each option in its list, and the least height of a row.
pub const MENU_PAD: f64 = 10.0;
pub const MENU_ARROW: f64 = 22.0;
pub const MENU_CHECK: f64 = 22.0;
const MENU_ROW: f64 = 20.0;
const RADIO_GAP: f64 = 6.0;
const RADIO_ROWS: f64 = 6.0;
const SWITCH: Size = Size { w: 38.0, h: 22.0 };
const GROUP_SPACING: f64 = 8.0;
/// A `Slider`'s height, and the radius of its knob.
pub const SLIDER_H: f64 = 22.0;
pub const KNOB: f64 = 9.0;
/// The width of a `Stepper`'s − and + buttons.
pub const STEPPER_BUTTON: f64 = 26.0;
/// A `ColorPicker`'s swatch.
const PICKER: Size = Size { w: 44.0, h: 24.0 };
/// How far a `DisclosureGroup`'s content is indented, the size of its
/// arrow, and the space between its title and content.
pub const DISCLOSURE_INDENT: f64 = 18.0;
pub const ARROW: f64 = 12.0;
const DISCLOSURE_GAP: f64 = 4.0;

thread_local! {
    /// The window's size, for what's laid out over the rest (a palette).
    static WINDOW: std::cell::Cell<Size> = const { std::cell::Cell::new(Size { w: 0.0, h: 0.0 }) };
}
/// Around a popover's content.
pub const POPOVER_PAD: f64 = 12.0;
/// The most room a popover's content is offered (it can be larger with a `.frame`).
const POPOVER_MAX: Size = Size { w: 280.0, h: 480.0 };
/// Between a sheet and the window's edges, at least.
const SHEET_MARGIN: f64 = 24.0;

/// The window's size, as it was last laid out.
pub fn window_size() -> Size {
    WINDOW.with(|w| w.get())
}
/// Between a popover and its view.
const POPOVER_GAP: f64 = 6.0;

pub struct Layouter<'a> {
    pub text: &'a mut TextEngine,
    pub scroll: &'a HashMap<u64, f64>,
    pub scroll_x: &'a HashMap<u64, f64>,
    /// The widths of the columns of the tables being laid out (innermost last).
    pub columns: Vec<Vec<f64>>,
    /// Text an input method is composing: the view it's for, the part of the
    /// view's text it will replace, and the text so far.
    pub composing: Option<(u64, std::ops::Range<usize>, &'a str)>,
}

impl Layouter<'_> {
    /// Lays out a window's tree below `top` (a menu bar drawn in the
    /// window), then its popovers next to their views.
    pub fn run_below(&mut self, root: &mut Node, w: f64, h: f64, top: f64) {
        WINDOW.with(|s| s.set(Size { w, h }));
        let area = Rect { x: 0.0, y: top, w, h: (h - top).max(0.0) };
        self.measure(root, Size { w: area.w, h: area.h }, &Style::default());
        self.place(root, area);
        let window = Rect { x: 0.0, y: 0.0, w, h };
        for i in 0..root.children.len() {
            let Kind::Popover { anchor, edge, .. } = root.children[i].kind else { continue };
            // (In a popover opened from another, the anchor is in that one.)
            let Some(at) = root.find(anchor).map(|n| n.layout.rect) else { continue };
            let style = root.layout.style;
            let c = &mut root.children[i];
            if edge == super::SHEET {
                // A sheet: as large as its content, in the middle of the window.
                let room = Size { w: (w - 2.0 * SHEET_MARGIN).max(0.0), h: (h - 2.0 * SHEET_MARGIN).max(0.0) };
                let size = self.measure(c, room, &style);
                let size = Size { w: size.w.min(room.w), h: size.h.min(room.h) };
                self.place(c, Rect { x: ((w - size.w) / 2.0).round(), y: ((h - size.h) / 2.0).round(), w: size.w, h: size.h });
                continue;
            }
            let room = Size { w: (w - 16.0).clamp(0.0, POPOVER_MAX.w), h: (h - 16.0).clamp(0.0, POPOVER_MAX.h) };
            let size = self.measure(c, room, &style);
            let r = popover_rect(at, Size { w: size.w.min(w - 8.0), h: size.h.min(h - 8.0) }, edge, window);
            self.place(c, r);
        }
    }

    fn style_for(n: &Node, parent: &Style) -> Style {
        Style {
            size: n.mods.font_size.unwrap_or(parent.size),
            weight: n.mods.weight.unwrap_or(parent.weight),
            face: super::text::Face {
                family: n.mods.family.unwrap_or(parent.face.family),
                italic: n.mods.italic.unwrap_or(parent.face.italic),
                ..parent.face
            },
            color: n.mods.color.unwrap_or(parent.color),
            disabled: parent.disabled || n.mods.disabled,
            text_align: n.mods.text_align.unwrap_or(parent.text_align),
            animation: n.mods.animation.map_or(parent.animation, |a| a.0),
            curve: n.mods.animation.map_or(parent.curve, |a| a.1),
            selectable: n.mods.selectable.unwrap_or(parent.selectable),
            line_limit: n.mods.line_limit.unwrap_or(parent.line_limit),
        }
    }

    /// The text an editable view shows: its own, with text being composed
    /// in place of the selection.
    fn shown_text(&self, key: u64, mut text: String) -> String {
        if let Some((k, range, composed)) = &self.composing {
            if *k == key && text.get(range.clone()).is_some() {
                text.replace_range(range.clone(), composed);
            }
        }
        text
    }

    fn text_layout(&mut self, s: &str, style: &Style, max_w: Option<f64>) -> std::rc::Rc<super::text::TextLayout> {
        std::rc::Rc::new(self.text.styled(s, style.size, style.weight, style.face, style.color, max_w, 0))
    }

    /// A `RichTextEditor`'s text, wrapped inside a field `w` wide, in the
    /// editor's style with its runs in theirs (and text being composed in
    /// place of the selection, in the style typing takes).
    fn rich_text(&mut self, key: u64, doc: &super::rich::Doc, style: &Style, w: f64) -> std::rc::Rc<super::text::TextLayout> {
        use super::text::{Family, Styled};
        let mut shown;
        let mut doc = doc;
        if let Some((k, range, composed)) = &self.composing {
            if *k == key && doc.text.get(range.clone()).is_some() {
                let marks = super::rich::typing_marks(key, doc, range.clone());
                shown = doc.clone();
                shown.replace(range.clone(), composed, &marks);
                doc = &shown;
            }
        }
        let base = Styled { range: 0..0, family: style.face.family, size: style.size, weight: style.weight, italic: style.face.italic, color: style.color };
        let runs: Vec<Styled> = doc
            .runs
            .iter()
            .map(|(range, m)| Styled {
                range: range.clone(),
                family: m.font.as_deref().map_or(base.family, Family::named),
                size: m.size.unwrap_or(base.size),
                weight: if m.bold { base.weight.max(700.0) } else { base.weight },
                italic: m.italic || base.italic,
                color: m.color.as_ref().and_then(super::rich::color_of).unwrap_or(base.color),
            })
            .collect();
        let wrap = (w - 2.0 * FIELD_PAD.0 - LINES_BAR).max(1.0);
        std::rc::Rc::new(self.text.runs(&doc.text, &base, &runs, Some(wrap)))
    }

    /// A secure field's text: a dot for each character, laid out so that
    /// its offsets are the text's own (editing works on the text).
    fn masked(&mut self, s: &str, style: &Style) -> std::rc::Rc<super::text::TextLayout> {
        const DOT: char = '•';
        let dots: String = s.chars().map(|_| DOT).collect();
        let mut t = self.text.styled(&dots, style.size, style.weight, style.face, style.color, None, 0);
        // Where each dot starts, and where its character does.
        let starts: Vec<usize> = s.char_indices().map(|(i, _)| i).chain(std::iter::once(s.len())).collect();
        let real = |b: usize| starts[(b / DOT.len_utf8()).min(starts.len() - 1)];
        for l in &mut t.lines {
            l.range = real(l.range.start)..real(l.range.end);
            for stop in &mut l.stops {
                stop.0 = real(stop.0);
            }
        }
        t.len = s.len();
        std::rc::Rc::new(t)
    }

    /// A `Text`'s layout: like `text_layout`, with its lines aligned, and
    /// no more of them than its `.lineLimit` (the last cut short with "…").
    /// Whether it's cut short.
    fn text_block(&mut self, s: &str, style: &Style, max_w: Option<f64>) -> (std::rc::Rc<super::text::TextLayout>, bool) {
        let t = self.text.styled(s, style.size, style.weight, style.face, style.color, max_w, style.text_align);
        let limit = style.line_limit;
        if limit == 0 || t.lines.len() <= limit {
            return (std::rc::Rc::new(t), false);
        }
        // The most of the last line that fits with "…" after it.
        let line = t.lines[limit - 1].range.clone();
        let ends: Vec<usize> = s[line.clone()].char_indices().map(|(i, _)| line.start + i).chain(std::iter::once(line.end)).collect();
        let mut lay = |end: usize| {
            let shown = format!("{}…", s[..end].trim_end());
            self.text.styled(&shown, style.size, style.weight, style.face, style.color, max_w, style.text_align)
        };
        let (mut lo, mut hi) = (0, ends.len() - 1);
        let mut best = lay(ends[0]);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let t = lay(ends[mid]);
            if t.lines.len() <= limit {
                lo = mid;
                best = t;
            } else {
                hi = mid - 1;
            }
        }
        if lo == 0 && best.lines.len() > limit {
            // (Not even "…" fits on the line: what fits of it, cut off.)
            best.lines.truncate(limit);
        }
        (std::rc::Rc::new(best), true)
    }

    fn measure(&mut self, n: &mut Node, avail: Size, parent: &Style) -> Size {
        let style = Self::style_for(n, parent);
        n.layout.style = style;
        // Menus show only when they're opened; popovers are laid out on
        // their own (see `run_below`).
        if n.mods.hidden || n.kind.is_menu() {
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
        // Turned or scaled, it takes the room of the box around it as it shows.
        n.layout.turned = None;
        if transformed(n) {
            let s = n.layout.size;
            n.layout.turned = Some(s);
            let (sx, sy) = n.mods.scale.unwrap_or((1.0, 1.0));
            let (w, h) = (s.w * sx, s.h * sy);
            let a = n.mods.rotation.to_radians();
            let (c, si) = (a.cos().abs(), a.sin().abs());
            n.layout.size = Size { w: w * c + h * si, h: w * si + h * c };
        }
        n.layout.size
    }

    fn measure_inner(&mut self, n: &mut Node, avail: Size, style: &Style) -> Size {
        let finite = |w: f64| w.is_finite().then_some(w);
        match &n.kind {
            Kind::Text(s) => {
                let s = s.clone();
                let (t, cut) = self.text_block(&s, style, finite(avail.w));
                let size = Size { w: t.width(), h: t.height() };
                n.layout.text = Some(t);
                n.layout.cut = cut;
                size
            }
            Kind::Button { label, .. } => {
                let label = label.clone();
                let t = self.text_layout(&label, style, None);
                let size = Size { w: t.width() + 2.0 * BUTTON_PAD.0, h: t.height() + 2.0 * BUTTON_PAD.1 };
                n.layout.text = Some(t);
                size
            }
            Kind::TextField { placeholder, text, lines, .. } if *lines > 1 => {
                // As wide as it's let be, and `lines` high; the text wraps
                // inside (again when it's placed: `place`).
                let value = self.shown_text(n.key, read_string(text));
                let (placeholder, lines) = (placeholder.clone(), *lines);
                let w = if avail.w.is_finite() { avail.w.max(60.0) } else { 240.0 };
                let wrap = (w - 2.0 * FIELD_PAD.0 - LINES_BAR).max(1.0);
                let ph = self.text_layout(&placeholder, style, Some(wrap));
                let t = self.text_layout(&value, style, Some(wrap));
                let line_h = self.text_layout("X", style, None).height();
                n.layout.empty = value.is_empty();
                n.layout.content_h = t.height() + 2.0 * FIELD_PAD.1;
                n.layout.text = Some(t);
                n.layout.placeholder = Some(ph);
                Size { w, h: lines as f64 * line_h + 2.0 * FIELD_PAD.1 }
            }
            Kind::TextField { placeholder, text, secure, search, .. } => {
                let value = self.shown_text(n.key, read_string(text));
                let (placeholder, secure, search) = (placeholder.clone(), *secure, *search);
                let ph = self.text_layout(&placeholder, style, None);
                let t = if secure { self.masked(&value, style) } else { self.text_layout(&value, style, None) };
                let line_h = (ph.height()).max(t.height());
                let extra = if search { SEARCH_LEFT + SEARCH_RIGHT - 2.0 * FIELD_PAD.0 } else { 0.0 };
                let w = (t.width().max(ph.width()) + 2.0 * FIELD_PAD.0 + extra).max(120.0);
                n.layout.empty = value.is_empty();
                n.layout.text = Some(t);
                n.layout.placeholder = Some(ph);
                Size { w: w.min(avail.w.max(60.0)), h: line_h + 2.0 * FIELD_PAD.1 }
            }
            Kind::Video { .. } => {
                // The video's own size (16:9 until it's known), made smaller to
                // fit the space there is; a `.frame` scales it to fit.
                let (player_size, time) = super::media::VIDEOS.with(|v| {
                    let v = v.borrow();
                    let p = v.get(&n.key).map(|(_, p)| p);
                    (p.and_then(super::media::Player::size), p.map(|p| (p.position(), p.duration())))
                });
                let (w, h) = player_size.unwrap_or((320.0, 180.0));
                let s = (avail.w / w).min(avail.h / h);
                let framed = n.mods.width.is_some() || n.mods.height.is_some();
                let s = if framed && s.is_finite() { s } else { s.min(1.0) };
                // Its time, for its controls.
                let (at, of) = time.unwrap_or((0.0, 0.0));
                let label = Style { size: 11.0, color: super::Rgba([255, 255, 255, 255]), ..*style };
                n.layout.text = Some(self.text_layout(&format!("{} / {}", clock_text(at), clock_text(of)), &label, None));
                Size { w: w * s, h: h * s }
            }
            Kind::RichEditor { doc, .. } => {
                // As large as it's let be; its text wraps inside (again when
                // it's placed) and scrolls.
                let doc = doc.clone();
                let w = if avail.w.is_finite() { avail.w.max(80.0) } else { 320.0 };
                let h = if avail.h.is_finite() { avail.h.max(60.0) } else { 200.0 };
                let t = self.rich_text(n.key, &doc, style, w);
                n.layout.content_h = t.height() + 2.0 * FIELD_PAD.1;
                n.layout.text = Some(t);
                Size { w, h }
            }
            Kind::Toggle { label, .. } => {
                if label.is_empty() {
                    return SWITCH;
                }
                let label = label.clone();
                let t = self.text_layout(&label, style, None);
                let size = Size { w: t.width() + 8.0 + SWITCH.w, h: (t.height()).max(SWITCH.h) };
                n.layout.text = Some(t);
                size
            }
            Kind::CodeEditor { text, .. } => {
                let value = self.shown_text(n.key, read_string(text));
                let t = self.text.code_layout(&value, 13.0);
                let count = t.lines.len().max(1);
                let digits = count.to_string().len().max(2);
                let numbers: Vec<String> = (1..=count).map(|i| format!("{i:>digits$}")).collect();
                let numbers = self.text.mono_layout(&numbers.join("\n"), 13.0, theme().line_number);
                n.layout.gutter = numbers.width() + GUTTER_PAD.0 + GUTTER_PAD.1;
                n.layout.content_h = t.height() + 2.0 * CODE_PAD_Y;
                n.layout.text = Some(std::rc::Rc::new(t));
                n.layout.placeholder = Some(std::rc::Rc::new(numbers));
                let w = if avail.w.is_finite() { avail.w } else { 480.0 };
                let h = if avail.h.is_finite() { avail.h } else { 320.0 };
                Size { w: w.max(160.0), h: h.max(80.0) }
            }
            Kind::Ribbon { .. } => {
                let mut tabs = Vec::new();
                for c in &n.children {
                    let Kind::RibbonTab { title } = &c.kind else { continue };
                    let t = self.text_layout(title, style, None);
                    tabs.push(Tab { title: t, rect: Rect::default(), close: None });
                }
                n.layout.tabs = tabs;
                let selected = n.selected_ribbon_tab();
                // (The other tabs' groups aren't shown, so none is collapsed and open.)
                for (i, tab) in n.children.iter_mut().enumerate() {
                    if i != selected {
                        tab.children.iter_mut().for_each(|g| g.layout.collapsed = false);
                    }
                }
                let mut w = 0.0;
                if let Some(c) = n.children.get_mut(selected) {
                    w = self.measure(c, Size { w: avail.w, h: RIBBON_BODY }, style).w;
                }
                Size { w: if avail.w.is_finite() { avail.w } else { w }, h: RIBBON_TABS + RIBBON_BODY }
            }
            Kind::RibbonTab { .. } => {
                // Groups show their items small, from the last one on,
                // until they fit; then, from the last one on, they become
                // one button each, which opens them.
                let body = Size { w: f64::INFINITY, h: RIBBON_BODY };
                let mut widths = Vec::new();
                for c in &mut n.children {
                    c.layout.compact = false;
                    c.layout.collapsed = false;
                    widths.push(self.measure(c, body, style).w + 1.0);
                }
                for collapse in [false, true] {
                    let mut i = n.children.len();
                    while widths.iter().sum::<f64>() > avail.w && i > 0 {
                        i -= 1;
                        let c = &mut n.children[i];
                        if !matches!(c.kind, Kind::RibbonGroup { .. }) {
                            continue;
                        }
                        if collapse {
                            c.layout.collapsed = true;
                        } else {
                            c.layout.compact = true;
                        }
                        let w = self.measure(c, body, style).w + 1.0;
                        // (A group that's smaller than its button stays as it is.)
                        if collapse && w >= widths[i] {
                            c.layout.collapsed = false;
                            self.measure(c, body, style);
                            continue;
                        }
                        widths[i] = w;
                    }
                }
                Size { w: widths.iter().sum(), h: RIBBON_BODY }
            }
            Kind::RibbonGroup { title, .. } => {
                let title = title.clone();
                let label = Style { size: 11.0, color: theme().colors[theme::SECONDARY], ..*style };
                let t = self.text_layout(&title, &label, None);
                // (A collapsed group's panel shows its items as they are with room.)
                let collapsed = n.layout.collapsed;
                let compact = n.layout.compact && !collapsed;
                for c in &mut n.children {
                    c.layout.compact = compact;
                    let room = if matches!(c.kind, Kind::RibbonItem { large: true, .. }) && !compact { RIBBON_LARGE } else { RIBBON_ROW };
                    self.measure(c, Size { w: f64::INFINITY, h: room }, style);
                }
                let columns = ribbon_columns(n);
                let mut w = 0.0;
                for column in &columns {
                    w += column.iter().map(|i| n.children[*i].layout.size.w).fold(0.0, f64::max);
                }
                w += RIBBON_GAP * columns.len().saturating_sub(1) as f64 + 2.0 * RIBBON_PAD;
                let w = w.max(t.width() + 4.0 * RIBBON_PAD);
                n.layout.text = Some(t);
                if collapsed {
                    // The button: the group's icon over its title and an arrow.
                    n.layout.panel = Rect { x: 0.0, y: 0.0, w, h: RIBBON_BODY };
                    let button = self.text_layout(&title, &Style { size: 12.0, ..*style }, None);
                    let w = (button.width() + 12.0 + 2.0 * RIBBON_PAD + 4.0).max(52.0) + 2.0 * RIBBON_PAD;
                    n.layout.placeholder = Some(button);
                    return Size { w, h: RIBBON_BODY };
                }
                Size { w, h: RIBBON_BODY }
            }
            Kind::RibbonItem { label, large, role, .. } => {
                let large = *large && !n.layout.compact;
                let menu = matches!(role, super::RibbonRole::Menu);
                let label = label.clone();
                let t = self.text_layout(&label, &Style { size: 12.0, ..*style }, None);
                let size = if large {
                    let arrow = if menu { 12.0 } else { 0.0 };
                    Size { w: (t.width() + arrow + 2.0 * RIBBON_PAD + 4.0).max(44.0), h: RIBBON_LARGE }
                } else {
                    let arrow = if menu { 14.0 } else { 0.0 };
                    Size { w: RIBBON_PAD + RIBBON_ICON + 5.0 + t.width() + arrow + 2.0 * RIBBON_PAD, h: RIBBON_ROW }
                };
                n.layout.text = Some(t);
                size
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
            Kind::Image { picture: Some(p), .. } => {
                // A picture has its own size (a point for each pixel), made
                // smaller to fit the space there is. A `.frame` scales it to
                // fit, larger too (keeping proportions).
                let (w, h) = (p.width.max(1.0), p.height.max(1.0));
                let s = (avail.w / w).min(avail.h / h);
                let framed = n.mods.width.is_some() || n.mods.height.is_some();
                let s = if framed && s.is_finite() { s } else { s.min(1.0) };
                Size { w: w * s, h: h * s }
            }
            Kind::Markdown { source, folder, .. } => {
                // As wide as the space there is, and as tall as that makes it.
                let w = if avail.w.is_finite() { avail.w } else { 480.0 };
                let laid = super::markdown::layout(self.text, source, folder, w, style);
                let size = Size { w, h: laid.height };
                n.layout.markdown = Some(laid);
                size
            }
            Kind::Spacer => Size::default(),
            Kind::Image { .. } => Size { w: 64.0, h: 64.0 },
            // A canvas takes the space it's offered (and a web view).
            Kind::Canvas { .. } | Kind::Scene3D { .. } | Kind::WebView { .. } => Size {
                w: if avail.w.is_finite() { avail.w } else { 300.0 },
                h: if avail.h.is_finite() { avail.h } else { 150.0 },
            },
            Kind::Picker { style: 2, .. } => {
                // A menu: a box as wide as the widest option, with an arrow.
                let (mut w, mut h): (f64, f64) = (0.0, 0.0);
                for c in &mut n.children {
                    let s = self.measure(c, Size { w: f64::INFINITY, h: f64::INFINITY }, style);
                    w = w.max(s.w);
                    h = h.max(s.h);
                }
                Size { w: w + 2.0 * MENU_PAD + MENU_ARROW, h: h + 12.0 }
            }
            Kind::Picker { style: 1, .. } => {
                // Radio buttons: an option a line, with its button before it.
                let room = Size { w: (avail.w - RADIO - RADIO_GAP).max(0.0), h: avail.h };
                let (mut w, mut h): (f64, f64) = (0.0, 0.0);
                let mut first = true;
                for c in &mut n.children {
                    let s = self.measure(c, room, style);
                    if c.mods.hidden {
                        continue;
                    }
                    w = w.max(RADIO + RADIO_GAP + s.w);
                    h += s.h.max(RADIO) + if first { 0.0 } else { RADIO_ROWS };
                    first = false;
                }
                Size { w, h }
            }
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
            Kind::Slider { .. } => Size { w: 160f64.min(avail.w).max(60.0), h: SLIDER_H },
            Kind::Stepper { value, int, decimals, .. } => {
                let shown = super::controls::format_number(super::controls::read_number(value, *int), *decimals);
                let t = self.text_layout(&shown, style, None);
                let size = Size {
                    w: 2.0 * STEPPER_BUTTON + (t.width() as f64 + 16.0).max(36.0),
                    h: (t.height() as f64 + 8.0).max(24.0),
                };
                n.layout.text = Some(t);
                size
            }
            Kind::ColorPicker { .. } => {
                // The palette (when it's open) has its own size.
                for c in &mut n.children {
                    self.measure(c, Size { w: f64::INFINITY, h: f64::INFINITY }, style);
                }
                PICKER
            }
            Kind::Swatch(_) => Size { w: 18.0, h: 18.0 },
            Kind::Disclosure { title, .. } => {
                let title = title.clone();
                let t = self.text_layout(&title, style, None);
                let header = Size { w: ARROW + 6.0 + t.width() as f64 + 4.0, h: (t.height() as f64).max(ARROW + 4.0) + 6.0 };
                n.layout.text = Some(t);
                n.layout.segments = vec![Rect { x: 0.0, y: 0.0, w: header.w, h: header.h }];
                let inner = Size { w: (avail.w - DISCLOSURE_INDENT).max(0.0), h: (avail.h - header.h - DISCLOSURE_GAP).max(0.0) };
                let content = self.measure_stack(n, DISCLOSURE_GAP, Axis::Y, inner, style);
                let open = n.children.iter().any(|c| !c.mods.hidden);
                Size {
                    w: header.w.max(if open { DISCLOSURE_INDENT + content.w } else { 0.0 }),
                    h: header.h + if open { DISCLOSURE_GAP + content.h } else { 0.0 },
                }
            }
            Kind::Lazy { count, row_height, across: true, .. } => {
                // Columns: as tall as the tallest, or the room there is.
                let (count, width) = (*count, *row_height);
                let mut h: f64 = 0.0;
                for c in &mut n.children {
                    h = h.max(self.measure(c, Size { w: width, h: avail.h }, style).h);
                }
                Size { w: count as f64 * width, h: if avail.h.is_finite() { avail.h } else { h } }
            }
            Kind::Lazy { count, row_height, .. } => {
                let (count, row_height) = (*count, *row_height);
                let mut w: f64 = 0.0;
                for c in &mut n.children {
                    w = w.max(self.measure(c, Size { w: avail.w, h: row_height }, style).w);
                }
                Size { w: if avail.w.is_finite() { avail.w } else { w }, h: count as f64 * row_height }
            }
            Kind::Grid { .. } => self.measure_grid(n, avail, style),
            Kind::GridRow => self.measure_stack(n, GROUP_SPACING, Axis::X, avail, style),
            Kind::VStack(spacing, _) => self.measure_stack(n, *spacing, Axis::Y, avail, style),
            Kind::Group => self.measure_stack(n, GROUP_SPACING, Axis::Y, avail, style),
            Kind::HStack(spacing, _) => self.measure_stack(n, *spacing, Axis::X, avail, style),
            Kind::Popover { .. } => {
                let inner = Size { w: (avail.w - 2.0 * POPOVER_PAD).max(0.0), h: (avail.h - 2.0 * POPOVER_PAD).max(0.0) };
                let mut size = Size::default();
                for c in &mut n.children {
                    let s = self.measure(c, inner, style);
                    size.w = size.w.max(s.w);
                    size.h = size.h.max(s.h);
                }
                Size { w: size.w + 2.0 * POPOVER_PAD, h: size.h + 2.0 * POPOVER_PAD }
            }
            Kind::MenuItem { .. } | Kind::MenuDivider | Kind::Menu { .. } | Kind::MenuList => Size::default(),
            Kind::ZStack(_) | Kind::Root | Kind::Window { .. } => {
                let mut size = Size::default();
                for c in &mut n.children {
                    if matches!(c.kind, Kind::Popover { .. }) {
                        continue;
                    }
                    let s = self.measure(c, avail, style);
                    size.w = size.w.max(s.w);
                    size.h = size.h.max(s.h);
                }
                size
            }
            Kind::Table(spec) => {
                // As large as it's let be; its header's titles, and its rows
                // in columns as wide as they're given.
                let spec = spec.clone();
                let w = if avail.w.is_finite() { avail.w } else { 480.0 };
                let header = if spec.tree.is_some() { 0.0 } else { super::table::HEADER };
                let tall = header + spec.count as f64 * spec.row_height;
                let h = if avail.h.is_finite() { avail.h } else { tall.min(320.0) };
                let titles = Style { size: (style.size - 1.0).max(9.0), weight: 600.0, ..*style };
                n.layout.tabs = spec
                    .columns
                    .iter()
                    .map(|c| Tab { title: self.text_layout(&c.title, &titles, None), rect: Rect::default(), close: None })
                    .collect();
                self.columns.push(super::table::widths(&spec, n.key, w));
                for c in &mut n.children {
                    self.measure(c, Size { w, h: (h - header).max(0.0) }, style);
                }
                self.columns.pop();
                Size { w, h }
            }
            Kind::TableRow(row) => {
                let indent = if row.path.is_some() { (row.depth + 1) as f64 * super::table::INDENT } else { 0.0 };
                let widths = self.columns.last().cloned().unwrap_or_default();
                for (i, c) in n.children.iter_mut().enumerate() {
                    let w = widths.get(i).copied().unwrap_or(avail.w) - 2.0 * super::table::CELL_PAD - if i == 0 { indent } else { 0.0 };
                    self.measure(c, Size { w: w.max(0.0), h: avail.h }, style);
                }
                Size { w: if avail.w.is_finite() { avail.w } else { widths.iter().sum() }, h: avail.h }
            }
            Kind::Split { vertical, .. } => {
                // Each pane gets its share of the room (all of it, without
                // a size to share); the split takes all it's offered.
                let vertical = *vertical;
                let along = if vertical { avail.h } else { avail.w };
                let first = along.is_finite().then(|| split_first(n, along));
                let mut total = Size::default();
                for (i, c) in n.children.iter_mut().enumerate() {
                    let share = match (first, i) {
                        (Some(f), 0) => f,
                        (Some(f), 1) => (along - f - SPLIT_DIVIDER).max(0.0),
                        (Some(_), _) => 0.0,
                        (None, _) => f64::INFINITY,
                    };
                    let room = if vertical { Size { w: avail.w, h: share } } else { Size { w: share, h: avail.h } };
                    let s = self.measure(c, room, style);
                    if vertical {
                        total = Size { w: total.w.max(s.w), h: total.h + s.h };
                    } else {
                        total = Size { w: total.w + s.w, h: total.h.max(s.h) };
                    }
                }
                Size {
                    w: if avail.w.is_finite() { avail.w } else { total.w },
                    h: if avail.h.is_finite() { avail.h } else { total.h },
                }
            }
            Kind::Scroll(axes) => {
                // Several children are stacked downwards, like a VStack.
                // They get all the room they want the ways it scrolls.
                let (down, across) = super::scroll_axes(*axes);
                let room = Size { w: if across { f64::INFINITY } else { avail.w }, h: if down { f64::INFINITY } else { avail.h } };
                let mut content_h: f64 = 0.0;
                let mut w: f64 = 0.0;
                let mut first = true;
                for c in &mut n.children {
                    let s = self.measure(c, room, style);
                    if c.mods.hidden {
                        continue;
                    }
                    content_h += s.h + if first { 0.0 } else { GROUP_SPACING };
                    first = false;
                    // (A view that takes all the width it's offered gets
                    // the scroll view's.)
                    w = w.max(if s.w.is_finite() { s.w } else { avail.w });
                }
                n.layout.content_h = content_h;
                n.layout.content_w = w;
                Size { w: w.min(avail.w), h: content_h.min(avail.h) }
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
        // Turned or scaled: laid out at its own size, in the middle of its room.
        if let Some(s) = n.layout.turned {
            rect = Rect { x: rect.x + (rect.w - s.w) / 2.0, y: rect.y + (rect.h - s.h) / 2.0, w: s.w, h: s.h };
        }
        // A stack may offer a flexible view more than its max size.
        let (max_w, max_h) = (n.mods.max_width.unwrap_or(f64::INFINITY), n.mods.max_height.unwrap_or(f64::INFINITY));
        if rect.w > max_w || rect.h > max_h {
            rect = aligned(rect, Size { w: rect.w.min(max_w), h: rect.h.min(max_h) }, 1);
        }
        // …or less than its min size (it then overflows, rather than squash its content).
        rect.w = rect.w.max(n.mods.min_width.unwrap_or(0.0));
        rect.h = rect.h.max(n.mods.min_height.unwrap_or(0.0));
        // `.offset`: shown (with all that's in it) away from its place.
        rect.x += n.mods.offset.0;
        rect.y += n.mods.offset.1;
        n.layout.rect = rect;
        if n.mods.hidden {
            return;
        }
        let mut content = rect.inset_by(n.mods.padding);
        // Content with a size of its own sits in a larger frame by the frame's
        // alignment. Text and SVGs are centered by default; stacks lay out
        // their children across the whole frame unless an alignment is given.
        let align = match &n.kind {
            Kind::Text(_) | Kind::Svg { .. } | Kind::Image { picture: Some(_), .. } => Some(n.mods.frame_align.unwrap_or(1)),
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
            Kind::Popover { .. } => {
                let inner = content.inset(POPOVER_PAD);
                for c in &mut n.children {
                    let w = if flex(c, Axis::X, None) { inner.w } else { c.layout.size.w.min(inner.w) };
                    let h = if flex(c, Axis::Y, None) { inner.h } else { c.layout.size.h.min(inner.h) };
                    self.place(c, aligned(inner, Size { w, h }, 5));
                }
            }
            Kind::ZStack(_) | Kind::Root | Kind::Window { .. } => {
                let align = if let Kind::ZStack(a) = n.kind { a } else { 1 };
                for c in &mut n.children {
                    if matches!(c.kind, Kind::Popover { .. }) {
                        continue;
                    }
                    let w = if flex(c, Axis::X, None) { content.w } else { c.layout.size.w.min(content.w) };
                    let h = if flex(c, Axis::Y, None) { content.h } else { c.layout.size.h.min(content.h) };
                    let r = aligned(content, Size { w, h }, align);
                    self.place(c, r);
                }
            }
            Kind::Table(spec) => {
                let spec = spec.clone();
                let header = if spec.tree.is_some() { 0.0 } else { super::table::HEADER };
                let widths = super::table::widths(&spec, n.key, content.w);
                let mut x = content.x;
                for (t, w) in n.layout.tabs.iter_mut().zip(&widths) {
                    t.rect = Rect { x, y: content.y, w: *w, h: header };
                    x += w;
                }
                self.columns.push(widths);
                // (Inside the table's border.)
                let body = Rect { x: content.x + 1.0, y: content.y + header, w: (content.w - 2.0).max(0.0), h: (content.h - header - 1.0).max(0.0) };
                for c in &mut n.children {
                    self.place(c, body);
                }
                self.columns.pop();
            }
            Kind::TableRow(row) => {
                let indent = if row.path.is_some() { (row.depth + 1) as f64 * super::table::INDENT } else { 0.0 };
                let widths = self.columns.last().cloned().unwrap_or_default();
                let pad = super::table::CELL_PAD;
                let mut x = content.x;
                for (i, c) in n.children.iter_mut().enumerate() {
                    let w = widths.get(i).copied().unwrap_or(content.w);
                    let inset = if i == 0 { indent } else { 0.0 };
                    self.place(c, Rect { x: x + pad + inset, y: content.y, w: (w - 2.0 * pad - inset).max(0.0), h: content.h });
                    x += w;
                }
            }
            Kind::Split { vertical, .. } => {
                let vertical = *vertical;
                let along = if vertical { content.h } else { content.w };
                let first = split_first(n, along);
                let second = (along - first - SPLIT_DIVIDER).max(0.0);
                let (a, divider, b) = if vertical {
                    (
                        Rect { h: first, ..content },
                        Rect { y: content.y + first, h: SPLIT_DIVIDER, ..content },
                        Rect { y: content.y + first + SPLIT_DIVIDER, h: second, ..content },
                    )
                } else {
                    (
                        Rect { w: first, ..content },
                        Rect { x: content.x + first, w: SPLIT_DIVIDER, ..content },
                        Rect { x: content.x + first + SPLIT_DIVIDER, w: second, ..content },
                    )
                };
                n.layout.segments = vec![divider];
                let style = n.layout.style.clone();
                for (i, c) in n.children.iter_mut().enumerate() {
                    let pane = match i {
                        0 => a,
                        1 => b,
                        _ => Rect { w: 0.0, h: 0.0, ..b },
                    };
                    // (Measured again for the room it has now.)
                    self.measure(c, Size { w: pane.w, h: pane.h }, &style);
                    self.place(c, pane);
                }
            }
            Kind::Scroll(axes) => {
                let (down, across) = super::scroll_axes(*axes);
                let (dy, dx) = scroll_offsets(n, content, self.scroll, self.scroll_x);
                // Scrolling content starts at the top left (lists read that way).
                let mut y = content.y - dy;
                let x = content.x - dx;
                // As wide as the widest child, when it scrolls across.
                let width = if across { n.layout.content_w.max(content.w) } else { content.w };
                for c in &mut n.children {
                    if c.mods.hidden {
                        self.place(c, Rect { x, y, w: 0.0, h: 0.0 });
                        continue;
                    }
                    let w = if flex(c, Axis::X, Some(Axis::Y)) { width } else { c.layout.size.w.min(width) };
                    // (Not scrolling down, a flexible child is as tall as the view.)
                    let h = if !down && flex(c, Axis::Y, Some(Axis::Y)) { content.h } else { c.layout.size.h };
                    let r = Rect { x, y, w, h };
                    self.place(c, r);
                    y += c.layout.size.h + GROUP_SPACING;
                }
            }
            Kind::Picker { style: 2, selection } => {
                // Closed: the chosen option in the box. Open: all of them in a
                // list under it (over it, without room), inside the window.
                let current = selection.locate();
                let chosen = n.children.iter().position(|c| c.mods.tag.as_ref().is_some_and(|t| t.equals(current)));
                let open = super::controls::OPEN_MENU.get() == Some(n.key);
                let window = WINDOW.with(|w| w.get());
                let row_h = n.children.iter().map(|c| c.layout.size.h).fold(MENU_ROW, f64::max) + 4.0;
                let widest = n.children.iter().map(|c| c.layout.size.w).fold(0.0, f64::max);
                let list_w = (widest + 2.0 * MENU_PAD + MENU_CHECK).max(content.w);
                let list_h = row_h * n.children.len() as f64 + 8.0;
                let below = content.y + content.h + 4.0;
                let list_y = if below + list_h <= window.h - 4.0 || content.y - 4.0 - list_h < 4.0 { below } else { content.y - 4.0 - list_h };
                let list_x = content.x.min(window.w - list_w - 4.0).max(4.0);
                let mut segments = Vec::new();
                for (i, c) in n.children.iter_mut().enumerate() {
                    let s = c.layout.size;
                    let r = if open {
                        let row = Rect { x: list_x + 4.0, y: list_y + 4.0 + i as f64 * row_h, w: list_w - 8.0, h: row_h };
                        segments.push(row);
                        Rect { x: row.x + MENU_CHECK, y: row.y + (row_h - s.h) / 2.0, w: s.w, h: s.h }
                    } else if Some(i) == chosen {
                        Rect { x: content.x + MENU_PAD, y: content.y + (content.h - s.h) / 2.0, w: s.w, h: s.h }
                    } else {
                        Rect { x: content.x, y: content.y, w: 0.0, h: 0.0 }
                    };
                    self.place(c, r);
                }
                n.layout.segments = segments;
            }
            Kind::Picker { style: 1, .. } => {
                let mut y = content.y;
                let mut segments = Vec::new();
                for c in &mut n.children {
                    if c.mods.hidden {
                        self.place(c, Rect { x: content.x, y, w: 0.0, h: 0.0 });
                        segments.push(Rect::default());
                        continue;
                    }
                    let s = c.layout.size;
                    let h = s.h.max(RADIO);
                    let w = s.w.min((content.w - RADIO - RADIO_GAP).max(0.0));
                    self.place(c, Rect { x: content.x + RADIO + RADIO_GAP, y: y + (h - s.h) / 2.0, w, h: s.h });
                    segments.push(Rect { x: content.x, y, w: RADIO + RADIO_GAP + w, h });
                    y += h + RADIO_ROWS;
                }
                n.layout.segments = segments;
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
            Kind::Ribbon { .. } => {
                let mut x = content.x + 6.0;
                for tab in &mut n.layout.tabs {
                    let w = tab.title.width() + 24.0;
                    tab.rect = Rect { x, y: content.y + 4.0, w, h: RIBBON_TABS - 4.0 };
                    x += w + 2.0;
                }
                let selected = n.selected_ribbon_tab();
                let body = Rect { x: content.x, y: content.y + RIBBON_TABS, w: content.w, h: RIBBON_BODY };
                if let Some(c) = n.children.get_mut(selected) {
                    self.place(c, body);
                }
            }
            Kind::RibbonTab { .. } => {
                let mut x = content.x;
                for c in &mut n.children {
                    let w = c.layout.size.w;
                    self.place(c, Rect { x, y: content.y, w, h: RIBBON_BODY });
                    x += w + 1.0;
                }
            }
            Kind::RibbonGroup { .. } => {
                let (mut x, mut top) = (content.x + RIBBON_PAD, content.y + RIBBON_PAD);
                if n.layout.collapsed {
                    // Its items go in the panel under its button, inside the window.
                    let window = WINDOW.with(|w| w.get());
                    let size = n.layout.panel;
                    let px = content.x.min(window.w - size.w - 4.0).max(4.0);
                    n.layout.panel = Rect { x: px, y: content.y + content.h + 2.0, w: size.w, h: size.h };
                    x = px + RIBBON_PAD;
                    top = n.layout.panel.y + RIBBON_PAD;
                }
                for column in ribbon_columns(n) {
                    let w = column.iter().map(|i| n.children[*i].layout.size.w).fold(0.0, f64::max);
                    let mut y = top;
                    for i in &column {
                        let c = &mut n.children[*i];
                        let s = c.layout.size;
                        // Small items are a row tall; other views take what they need.
                        let h = s.h.max(RIBBON_ROW);
                        self.place(c, Rect { x, y: y + (h - s.h) / 2.0, w: s.w.min(w), h: s.h });
                        y += h + RIBBON_ROW_GAP;
                    }
                    x += w + RIBBON_GAP;
                }
            }
            Kind::TabView { .. } => {
                let mut x = content.x;
                for tab in &mut n.layout.tabs {
                    let w = (tab.title.width() + 28.0 + if tab.close.is_some() { 18.0 } else { 0.0 }).max(90.0);
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
            Kind::TextField { placeholder, text, lines, .. } if *lines > 1 => {
                // The text wraps at the width the field was given.
                let wrap = (content.w - 2.0 * FIELD_PAD.0 - LINES_BAR).max(1.0);
                {
                    let style = n.layout.style.clone();
                    let style = &style;
                    let value = self.shown_text(n.key, read_string(text));
                    let placeholder = placeholder.clone();
                    let t = self.text_layout(&value, style, Some(wrap));
                    n.layout.content_h = t.height() + 2.0 * FIELD_PAD.1;
                    n.layout.text = Some(t);
                    n.layout.placeholder = Some(self.text_layout(&placeholder, style, Some(wrap)));
                }
            }
            Kind::TextField { .. } => {
                // Re-wrap nothing; the field just takes the width it was given.
            }
            Kind::RichEditor { doc, .. } => {
                let (doc, style) = (doc.clone(), n.layout.style);
                let t = self.rich_text(n.key, &doc, &style, content.w);
                n.layout.content_h = t.height() + 2.0 * FIELD_PAD.1;
                n.layout.text = Some(t);
            }
            Kind::ColorPicker { .. } => {
                // The palette goes below the swatch (above it if there's no
                // room), inside the window.
                let window = WINDOW.with(|w| w.get());
                if let Some(c) = n.children.first_mut() {
                    let size = c.layout.size;
                    let x = rect.x.min(window.w - size.w - 4.0).max(4.0);
                    let below = rect.y + rect.h + 4.0;
                    let above = rect.y - 4.0 - size.h;
                    let y = if below + size.h <= window.h - 4.0 {
                        below
                    } else if above >= 4.0 {
                        above
                    } else {
                        // Room on neither side: as much of it as fits.
                        (window.h - 4.0 - size.h).max(4.0)
                    };
                    self.place(c, Rect { x, y, w: size.w, h: size.h });
                }
            }
            Kind::Disclosure { .. } => {
                let header_h = n.layout.segments.first().map_or(0.0, |s| s.h);
                n.layout.segments = vec![Rect { x: content.x, y: content.y, w: content.w, h: header_h }];
                let top = header_h + DISCLOSURE_GAP;
                let inner = Rect {
                    x: content.x + DISCLOSURE_INDENT,
                    y: content.y + top,
                    w: (content.w - DISCLOSURE_INDENT).max(0.0),
                    h: (content.h - top).max(0.0),
                };
                self.place_stack(n, DISCLOSURE_GAP, 0, Axis::Y, inner);
            }
            Kind::Lazy { row_height, first, across, .. } => {
                let (row_height, first, across) = (*row_height, *first, *across);
                for (i, c) in n.children.iter_mut().enumerate() {
                    let along = (first + i) as f64 * row_height;
                    let r = if across {
                        Rect { x: content.x + along, y: content.y, w: row_height, h: content.h }
                    } else {
                        Rect { x: content.x, y: content.y + along, w: content.w, h: row_height }
                    };
                    self.place(c, r);
                }
            }
            Kind::Grid { .. } => self.place_grid(n, content),
            Kind::GridRow => self.place_stack(n, GROUP_SPACING, 1, Axis::X, content),
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
                    let (t, cut) = self.text_block(&s, &style, Some((r.w - c.mods.padding.horizontal()).max(0.0)));
                    c.layout.natural = Size { w: t.width(), h: t.height() };
                    c.layout.text = Some(t);
                    c.layout.cut = cut;
                }
            }
            self.place(c, r);
            pos += along + spacing;
        }
    }
}

/// A time in seconds as a clock shows it: `1:05`, `1:02:03`.
fn clock_text(seconds: f64) -> String {
    let s = seconds.max(0.0).floor() as i64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Whether a view is turned or scaled (`.rotation`, `.scale`).
fn transformed(n: &Node) -> bool {
    n.mods.rotation.rem_euclid(360.0) != 0.0 || n.mods.scale.is_some()
}

/// How a view that's turned or scaled is drawn: around its middle.
pub fn turn_of(n: &Node) -> Option<kurbo::Affine> {
    if !transformed(n) {
        return None;
    }
    let r = n.layout.rect;
    let c = kurbo::Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (sx, sy) = n.mods.scale.unwrap_or((1.0, 1.0));
    Some(kurbo::Affine::translate(c) * kurbo::Affine::rotate(n.mods.rotation.to_radians()) * kurbo::Affine::scale_non_uniform(sx, sy) * kurbo::Affine::translate(-c))
}

/// Does `n` stretch along `axis` inside a parent laid out along `parent`?
fn flex(n: &Node, axis: Axis, parent: Option<Axis>) -> bool {
    // (A view that's turned or scaled has the size it shows at.)
    if n.mods.hidden || transformed(n) {
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
        Kind::Lazy { across: true, .. } => axis == Axis::Y,
        Kind::TextField { .. } | Kind::Markdown { .. } | Kind::Slider { .. } | Kind::Lazy { .. } | Kind::Ribbon { .. } => axis == Axis::X,
        Kind::Grid { .. } => n.children.iter().any(|c| match c.kind {
            Kind::GridRow => c.children.iter().any(|cell| flex(cell, axis, Some(Axis::X))),
            _ => flex(c, axis, Some(Axis::Y)),
        }),
        Kind::GridRow => n.children.iter().any(|c| flex(c, axis, Some(Axis::X))),
        Kind::Disclosure { .. } => axis == Axis::X,
        Kind::Scroll(_) | Kind::Split { .. } | Kind::CodeEditor { .. } | Kind::RichEditor { .. } | Kind::WebView { .. } | Kind::TabView { .. } | Kind::Canvas { .. } | Kind::Scene3D { .. } | Kind::Table(_) => true,
        Kind::TableRow(_) => axis == Axis::X,
        // A divider spans across its stack.
        Kind::Divider => parent.is_some() && parent != Some(axis),
        Kind::VStack(..) | Kind::Group => n.children.iter().any(|c| flex(c, axis, Some(Axis::Y))),
        Kind::HStack(..) => n.children.iter().any(|c| flex(c, axis, Some(Axis::X))),
        Kind::ZStack(_) | Kind::Root | Kind::Window { .. } => n.children.iter().any(|c| flex(c, axis, None)),
        _ => false,
    }
}

/// Where a popover of `size` goes next to a view at `at`, on its `edge`
/// (an `Edge` tag: 0 top, 1 bottom, 2 leading, 3 trailing), or on the
/// other side when there's no room, and inside the window.
fn popover_rect(at: Rect, size: Size, edge: u8, window: Rect) -> Rect {
    let margin = 4.0;
    let fits_x = |x: f64| x >= margin && x + size.w <= window.w - margin;
    let fits_y = |y: f64| y >= margin && y + size.h <= window.h - margin;
    let clamp = |v: f64, len: f64, room: f64| v.min(room - len - margin).max(margin);
    let (x, y) = match edge {
        0 | 1 => {
            let below = at.y + at.h + POPOVER_GAP;
            let above = at.y - POPOVER_GAP - size.h;
            let y = match edge {
                0 if !fits_y(above) && fits_y(below) => below,
                0 => above,
                _ if !fits_y(below) && fits_y(above) => above,
                _ => below,
            };
            (clamp(at.x + (at.w - size.w) / 2.0, size.w, window.w), clamp(y, size.h, window.h))
        }
        _ => {
            let after = at.x + at.w + POPOVER_GAP;
            let before = at.x - POPOVER_GAP - size.w;
            let x = match edge {
                2 if !fits_x(before) && fits_x(after) => after,
                2 => before,
                _ if !fits_x(after) && fits_x(before) => before,
                _ => after,
            };
            (clamp(x, size.w, window.w), clamp(at.y + (at.h - size.h) / 2.0, size.h, window.h))
        }
    };
    Rect { x, y, w: size.w, h: size.h }
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

/// How far a scroll view is scrolled (down, across): as far as its content
/// goes, and only the ways it scrolls.
pub fn scroll_offsets(n: &Node, content: Rect, scroll: &HashMap<u64, f64>, scroll_x: &HashMap<u64, f64>) -> (f64, f64) {
    let Kind::Scroll(axes) = n.kind else { return (0.0, 0.0) };
    let (down, across) = super::scroll_axes(axes);
    let (max_y, max_x) = ((n.layout.content_h - content.h).max(0.0), (n.layout.content_w - content.w).max(0.0));
    let dy = if down { scroll.get(&n.key).copied().unwrap_or(0.0).clamp(0.0, max_y) } else { 0.0 };
    let dx = if across { scroll_x.get(&n.key).copied().unwrap_or(0.0).clamp(0.0, max_x) } else { 0.0 };
    (dy, dx)
}

/// The width (height) of the line between a split view's panes.
pub const SPLIT_DIVIDER: f64 = 1.0;
/// The least a pane of a split view is, without a `minWidth` (`minHeight`).
const SPLIT_MIN: f64 = 24.0;

thread_local! {
    /// Where each split view's divider was dragged (its first pane's size),
    /// for one without a `size:` binding.
    pub static SPLITS: std::cell::RefCell<HashMap<u64, f64>> = std::cell::RefCell::new(HashMap::new());
}

/// The size of a split view's first pane, in `along` points: from its
/// binding, where it was dragged, or half; no pane less than its minimum.
pub fn split_first(n: &Node, along: f64) -> f64 {
    let Kind::Split { vertical, size } = &n.kind else { return 0.0 };
    let wanted = match size {
        Some(cb) => super::controls::read_number(cb, false),
        None => SPLITS.with(|s| s.borrow().get(&n.key).copied()).unwrap_or(((along - SPLIT_DIVIDER) / 2.0).round()),
    };
    let min = |c: Option<&Node>| {
        let m = c.and_then(|c| if *vertical { c.mods.min_height } else { c.mods.min_width });
        m.unwrap_or(SPLIT_MIN)
    };
    let (lo, hi) = (min(n.children.first()), along - SPLIT_DIVIDER - min(n.children.get(1)));
    let wanted = if wanted.is_finite() { wanted } else { lo };
    if hi < lo { (along - SPLIT_DIVIDER).max(0.0) / 2.0 } else { wanted.clamp(lo, hi) }
}

// ===== grids =====

/// How a grid's columns came out when it was measured: each column's least
/// width (from cells that aren't flexible), and whether it's flexible (it
/// shares the room left, like a `Spacer`).
#[derive(Debug, Default)]
pub struct GridSizes {
    pub widths: Vec<f64>,
    pub flexible: Vec<bool>,
}

/// A grid's rows: for each child, its cells (the cell's place in the row,
/// the first column it takes and how many), or `None` for a view that's a
/// row of its own, as wide as the grid. And how many columns there are.
fn grid_plan(n: &Node) -> (Vec<Option<Vec<(usize, usize, usize)>>>, usize) {
    let mut columns = 0;
    let plan = n
        .children
        .iter()
        .map(|c| {
            let Kind::GridRow = c.kind else { return None };
            let mut at = 0;
            let cells = c
                .children
                .iter()
                .enumerate()
                .filter(|(_, cell)| !cell.kind.is_menu())
                .map(|(i, cell)| {
                    let span = cell.mods.grid_columns.unwrap_or(1).max(1);
                    let place = (i, at, span);
                    at += span;
                    place
                })
                .collect();
            columns = columns.max(at);
            Some(cells)
        })
        .collect();
    (plan, columns)
}

impl Layouter<'_> {
    /// The width of `span` columns from `col`, with the spaces between them.
    fn span_width(widths: &[f64], col: usize, span: usize, spacing: f64) -> f64 {
        let end = (col + span).min(widths.len());
        widths[col.min(end)..end].iter().sum::<f64>() + spacing * span.saturating_sub(1) as f64
    }

    /// The columns' widths when the grid is `room` wide: their least widths,
    /// and the room left shared by the flexible ones.
    fn grid_widths(sizes: &GridSizes, spacing: f64, room: f64) -> Vec<f64> {
        let mut widths = sizes.widths.clone();
        let flexible = sizes.flexible.iter().filter(|f| **f).count();
        if flexible > 0 && room.is_finite() {
            let gaps = spacing * widths.len().saturating_sub(1) as f64;
            let fixed: f64 = widths.iter().zip(&sizes.flexible).filter(|(_, f)| !**f).map(|(w, _)| w).sum();
            let share = ((room - gaps - fixed) / flexible as f64).max(0.0);
            for (w, f) in widths.iter_mut().zip(&sizes.flexible) {
                if *f {
                    *w = w.max(share);
                }
            }
        }
        widths
    }

    fn measure_grid(&mut self, n: &mut Node, avail: Size, style: &Style) -> Size {
        let Kind::Grid { spacing, row_spacing, .. } = n.kind else { return Size::default() };
        let (plan, columns) = grid_plan(n);
        let mut sizes = GridSizes { widths: vec![0.0; columns], flexible: vec![false; columns] };
        let free = Size { w: avail.w, h: f64::INFINITY };
        let mut wide: f64 = 0.0;
        // The cells' own widths: the widest in each column, then what cells
        // over several columns need more.
        for (row, cells) in n.children.iter_mut().zip(&plan) {
            let row_style = Self::style_for(row, style);
            row.layout.style = row_style;
            let Some(cells) = cells else {
                let s = self.measure(row, free, style);
                if !flex(row, Axis::X, Some(Axis::Y)) {
                    wide = wide.max(s.w);
                }
                continue;
            };
            for &(i, col, span) in cells {
                let c = &mut row.children[i];
                if flex(c, Axis::X, Some(Axis::X)) {
                    if span == 1 {
                        sizes.flexible[col] = true;
                    }
                    continue;
                }
                let s = self.measure(c, free, &row_style);
                if span == 1 {
                    sizes.widths[col] = sizes.widths[col].max(s.w);
                }
            }
        }
        for (row, cells) in n.children.iter().zip(&plan) {
            for &(i, col, span) in cells.iter().flatten() {
                let c = &row.children[i];
                if span > 1 && !flex(c, Axis::X, Some(Axis::X)) {
                    let have = Self::span_width(&sizes.widths, col, span, spacing);
                    let last = (col + span).min(columns) - 1;
                    if c.layout.size.w > have {
                        sizes.widths[last] += c.layout.size.w - have;
                    }
                }
            }
        }
        let widths = Self::grid_widths(&sizes, spacing, avail.w);
        let total = Self::span_width(&widths, 0, columns, spacing).max(wide);
        let grows = sizes.flexible.iter().any(|f| *f) && avail.w.is_finite();
        let w = if grows { avail.w.max(total) } else { total };
        n.layout.grid = Some(std::rc::Rc::new(sizes));
        // The rows, with the cells as wide as their columns.
        let h = self.grid_rows(n, &plan, &widths, w, row_spacing, spacing, style).iter().sum::<f64>();
        let shown = n.children.iter().filter(|c| !c.mods.hidden).count();
        Size { w, h: h + row_spacing * shown.saturating_sub(1) as f64 }
    }

    /// Measures each row's cells at the widths of their columns (and a
    /// row of its own at the grid's width `w`): the rows' heights.
    #[allow(clippy::too_many_arguments)]
    fn grid_rows(
        &mut self,
        n: &mut Node,
        plan: &[Option<Vec<(usize, usize, usize)>>],
        widths: &[f64],
        w: f64,
        _row_spacing: f64,
        spacing: f64,
        style: &Style,
    ) -> Vec<f64> {
        let mut heights = Vec::new();
        for (row, cells) in n.children.iter_mut().zip(plan) {
            if row.mods.hidden {
                heights.push(0.0);
                continue;
            }
            let Some(cells) = cells else {
                heights.push(self.measure(row, Size { w, h: f64::INFINITY }, style).h);
                continue;
            };
            let row_style = row.layout.style;
            let mut h: f64 = 0.0;
            for &(i, col, span) in cells {
                let cell_w = Self::span_width(widths, col, span, spacing);
                let c = &mut row.children[i];
                h = h.max(self.measure(c, Size { w: cell_w, h: f64::INFINITY }, &row_style).h);
            }
            row.layout.size = Size { w, h };
            heights.push(h);
        }
        heights
    }

    fn place_grid(&mut self, n: &mut Node, content: Rect) {
        let Kind::Grid { spacing, row_spacing, align } = n.kind else { return };
        let (plan, columns) = grid_plan(n);
        let Some(sizes) = n.layout.grid.clone() else { return };
        let widths = Self::grid_widths(&sizes, spacing, content.w);
        let style = n.layout.style;
        // (Measured again, for the widths the columns have now.)
        let heights = self.grid_rows(n, &plan, &widths, content.w, row_spacing, spacing, &style);
        let (h_align, v_align) = super::align_2d(align);
        // How each column's cells line up across: as one of them says, or the grid.
        let mut column_align = vec![h_align; columns];
        for (row, cells) in n.children.iter().zip(&plan) {
            for &(i, col, span) in cells.iter().flatten() {
                if let (Some(a), 1) = (row.children[i].mods.grid_align, span) {
                    column_align[col] = a;
                }
            }
        }
        let mut lefts = Vec::with_capacity(columns);
        let mut x = content.x;
        for w in &widths {
            lefts.push(x);
            x += w + spacing;
        }
        let total = Self::span_width(&widths, 0, columns, spacing);
        let mut y = content.y;
        for ((row, cells), h) in n.children.iter_mut().zip(&plan).zip(&heights) {
            if row.mods.hidden {
                self.place(row, Rect { x: content.x, y, w: 0.0, h: 0.0 });
                continue;
            }
            let Some(cells) = cells else {
                self.place(row, Rect { x: content.x, y, w: content.w, h: *h });
                y += h + row_spacing;
                continue;
            };
            let r = Rect { x: content.x, y, w: total.max(content.w), h: *h };
            row.layout.rect = r;
            row.layout.content = r;
            for &(i, col, span) in cells {
                let cell = Rect { x: lefts.get(col).copied().unwrap_or(x), y, w: Self::span_width(&widths, col, span, spacing), h: *h };
                let c = &mut row.children[i];
                let w = if flex(c, Axis::X, Some(Axis::X)) { cell.w } else { c.layout.size.w.min(cell.w) };
                let ch = if flex(c, Axis::Y, Some(Axis::X)) { cell.h } else { c.layout.size.h.min(cell.h) };
                // (A cell over several columns lines up as it says, or as the grid does.)
                let across = match span {
                    1 => column_align.get(col).copied().unwrap_or(h_align),
                    _ => c.mods.grid_align.unwrap_or(h_align),
                };
                let at = |a: u8, room: f64, len: f64| (room - len) * a as f64 / 2.0;
                self.place(c, Rect { x: cell.x + at(across, cell.w, w), y: cell.y + at(v_align, cell.h, ch), w, h: ch });
            }
            y += h + row_spacing;
        }
    }
}
