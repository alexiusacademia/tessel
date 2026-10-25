//! Drawing: turns a laid-out tree into a drawing (`DrawList`), which the
//! renderer (`ui::backend`) shows in the window or saves as a PNG.

use std::collections::HashMap;
use std::rc::Rc;

use kurbo::{Affine, Circle, RoundedRect, Stroke};

use super::draw::{DrawList, FillRule};
use super::layout::Rect;
use super::text::TextLayout;
use super::theme::{self, theme};
use super::{Kind, Node, Rgba};

pub use super::text::TextEngine;

// A code editor's spacing, in points.
/// Left and right of the line numbers.
pub const GUTTER_PAD: (f64, f64) = (12.0, 10.0);
/// Between the line numbers and the code, and after the longest line.
pub const CODE_PAD_X: f64 = 12.0;
/// Above the first line and below the last.
pub const CODE_PAD_Y: f64 = 10.0;

/// What the user is doing right now, which changes how views are drawn.
#[derive(Default)]
pub struct Interaction {
    /// The control (button, field, tappable view, …) under the pointer.
    pub hovered: Option<u64>,
    /// The innermost view with `.hoverBackground` under the pointer.
    pub hover_background: Option<u64>,
    pub pressed: Option<u64>,
    /// The text field being edited.
    pub editing: Option<super::edit::Editing>,
    pub scroll: HashMap<u64, f64>,
    /// Horizontal scroll of code editors, by key.
    pub scroll_x: HashMap<u64, f64>,
    /// The caret line each code editor last reported to its `line:` binding.
    pub editor_line: HashMap<u64, i64>,
    /// Where the mouse is (for hover effects inside views, like tabs).
    pub pointer: Option<(f64, f64)>,
    /// Undo and redo history of text fields and code editors, by key.
    pub history: HashMap<u64, super::edit::History>,
    /// The text field or code editor whose text changed last (by typing, or
    /// by the program, for a code editor): Cmd+Z undoes there while
    /// nothing is being edited.
    pub last_edited: Option<u64>,
    /// The view with `.onKey` pressed last, which has the keyboard: keys go
    /// to it (Cmd ones too), and a code editor whose `line:` changes then
    /// scrolls to it without taking it.
    pub keys_to: Option<u64>,
    /// A code editor to scroll to a character, once it's laid out.
    pub scroll_to: Option<(u64, usize)>,
    /// The code editor's completion list, laid out, when one shows.
    pub popup: Option<Popup>,
    /// Text an input method is composing in the view being edited.
    pub composing: Option<Composing>,
    pub anim: super::anim::Animations,
    /// The `.tooltip` that shows, laid out.
    pub tooltip: Option<Tooltip>,
    /// The notice (`showNotice`) that shows, laid out.
    pub notice: Option<Tooltip>,
    /// The control the keyboard is on (a slider, stepper, color picker or
    /// disclosure group), when no text is edited.
    pub focused: Option<u64>,
    /// Whether the focused control shows its ring: once the keyboard is used
    /// on it (not when it was just clicked).
    pub focus_ring: bool,
    /// A view being dragged with `.draggable`.
    pub dragging: Option<Dragging>,
    /// The menu that's open (a context menu, or one of the drawn menu
    /// bar's), and its open submenus, laid out.
    pub menus: Vec<super::menu::Panel>,
    /// The menu bar, when it's drawn in the window.
    pub menu_bar: Option<super::menu::Bar>,
    /// For each table: the row selecting started from (Shift extends from
    /// it), and the row the keyboard is on.
    pub table_at: HashMap<u64, (usize, usize)>,
    /// Shift, and Cmd (Ctrl), held for the click being handled.
    pub click_mods: (bool, bool),
    /// Shift held for the key being handled.
    pub key_shift: bool,
    /// The last click on a table's row, when (a second soon after opens it).
    pub last_row_click: Option<(std::time::Instant, u64, usize)>,
}

/// A view being dragged (`.draggable`): it follows the pointer.
pub struct Dragging {
    /// The view, and its payload.
    pub key: u64,
    pub payload: String,
    /// Where the pointer is, and where on the view it took hold of it.
    pub at: (f64, f64),
    pub grab: (f64, f64),
    /// The view with `.dropTarget` under the pointer.
    pub target: Option<u64>,
}

/// A view's `.tooltip`, shown next to the pointer.
pub struct Tooltip {
    pub rect: Rect,
    pub text: String,
    pub layout: Rc<TextLayout>,
}

/// Text being composed with an input method (for Chinese, Japanese, accents
/// typed with dead keys, …): shown at the caret, underlined, and not part of
/// the view's text until it's committed.
pub struct Composing {
    pub text: String,
    /// The input method's caret, as a byte offset into `text`.
    pub caret: Option<usize>,
}

impl Interaction {
    /// The text being composed in view `key`, and the part of the view's
    /// text it takes the place of (the selection).
    pub fn composing_in(&self, key: u64) -> Option<(std::ops::Range<usize>, &Composing)> {
        let e = self.editing.as_ref().filter(|e| e.key == key)?;
        Some((e.range(), self.composing.as_ref()?))
    }

    /// The selection of view `key` as it's drawn, if it's being edited:
    /// while text is composed, a caret inside that text.
    pub fn shown_editing(&self, key: u64) -> Option<super::edit::Editing> {
        let mut e = self.editing.clone().filter(|e| e.key == key)?;
        if let Some((range, c)) = self.composing_in(key) {
            let caret = range.start + c.caret.unwrap_or(c.text.len()).min(c.text.len());
            (e.anchor, e.focus) = (caret, caret);
        }
        Some(e)
    }
}

/// A scroll bar of a scroll view or a code editor: it shows when there's
/// more than fits, and can be dragged.
pub struct Bar {
    /// Where the thumb can go.
    pub track: Rect,
    pub thumb: Rect,
    /// How far the view can be scrolled.
    pub max: f64,
}

impl Bar {
    const THICKNESS: f64 = 6.0;
    const MARGIN: f64 = 3.0;

    /// The bar along one side of the `view` (what shows), for content
    /// `total` long, scrolled by `offset`. `None` if it all fits.
    fn along(view: Rect, vertical: bool, total: f64, offset: f64, shorter: f64) -> Option<Bar> {
        let visible = if vertical { view.h } else { view.w };
        let max = total - visible;
        if max <= 0.5 || visible < 40.0 {
            return None;
        }
        let length = visible - 2.0 * Bar::MARGIN - shorter;
        let thumb = (length * visible / total).max(28.0).min(length);
        let at = (length - thumb) * (offset / max).clamp(0.0, 1.0);
        let (track, thumb) = if vertical {
            let x = view.x + view.w - Bar::MARGIN - Bar::THICKNESS;
            let y = view.y + Bar::MARGIN;
            (Rect { x, y, w: Bar::THICKNESS, h: length }, Rect { x, y: y + at, w: Bar::THICKNESS, h: thumb })
        } else {
            let x = view.x + Bar::MARGIN;
            let y = view.y + view.h - Bar::MARGIN - Bar::THICKNESS;
            (Rect { x, y, w: length, h: Bar::THICKNESS }, Rect { x: x + at, y, w: thumb, h: Bar::THICKNESS })
        };
        Some(Bar { track, thumb, max })
    }

    /// How far the view is scrolled when the thumb starts `at` along the
    /// track (from the track's start).
    pub fn offset_for(&self, at: f64) -> f64 {
        let vertical = self.track.h > self.track.w;
        let room = if vertical { self.track.h - self.thumb.h } else { self.track.w - self.thumb.w };
        if room <= 0.0 { 0.0 } else { (at / room).clamp(0.0, 1.0) * self.max }
    }
}

/// The scroll bars of a view: (down the side, along the bottom), each
/// where it scrolls that way and there's more than shows.
pub fn scroll_bars(n: &Node, ui: &Interaction) -> (Option<Bar>, Option<Bar>) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    let down = ui.scroll.get(&n.key).copied().unwrap_or(0.0);
    match &n.kind {
        Kind::Scroll(axes) => {
            let (dy, dx) = super::layout::scroll_offsets(n, content, &ui.scroll, &ui.scroll_x);
            let (downwards, across) = super::scroll_axes(*axes);
            let bottom = if across { Bar::along(content, false, n.layout.content_w, dx, 0.0) } else { None };
            let side = if downwards { Bar::along(content, true, n.layout.content_h, dy, 0.0) } else { None };
            // (The two don't meet in the corner.)
            let corner = Bar::THICKNESS + Bar::MARGIN;
            let bottom = match (&side, across) {
                (Some(_), true) => Bar::along(content, false, n.layout.content_w, dx, corner),
                _ => bottom,
            };
            let side = match (&bottom, downwards) {
                (Some(_), true) => Bar::along(content, true, n.layout.content_h, dy, corner),
                _ => side,
            };
            (side, bottom)
        }
        Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. } => (Bar::along(content, true, n.layout.content_h, down, 0.0), None),
        Kind::CodeEditor { .. } => {
            let code = Rect { x: content.x + n.layout.gutter, w: (content.w - n.layout.gutter).max(0.0), ..content };
            let across = ui.scroll_x.get(&n.key).copied().unwrap_or(0.0);
            let width = n.layout.text.as_ref().map_or(0.0, |t| t.width()) + 2.0 * CODE_PAD_X;
            let bottom = Bar::along(code, false, width, across, 0.0);
            // (The two don't meet in the corner.)
            let shorter = if bottom.is_some() { Bar::THICKNESS + Bar::MARGIN } else { 0.0 };
            (Bar::along(content, true, n.layout.content_h, down, shorter), bottom)
        }
        _ => (None, None),
    }
}

/// A code editor's completion list, drawn over everything else.
pub struct Popup {
    pub rect: Rect,
    pub row_h: f64,
    /// The visible rows (the list scrolls to keep the selection in view).
    pub rows: Vec<PopupRow>,
    /// Index of the selected row within `rows`, if it's visible.
    pub selected: Option<usize>,
}

pub struct PopupRow {
    /// Index into the matching suggestions.
    pub index: usize,
    pub badge: Rc<TextLayout>,
    pub badge_color: Rgba,
    pub label: Rc<TextLayout>,
    pub detail: Rc<TextLayout>,
}

impl Popup {
    /// The suggestion (index into the matches) at a point, if it's on the list.
    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let i = ((y - self.rect.y - 4.0) / self.row_h).floor();
        (i >= 0.0).then(|| self.rows.get(i as usize).map(|r| r.index)).flatten()
    }
}

fn rounded(r: Rect, radius: f64) -> RoundedRect {
    RoundedRect::new(r.x, r.y, r.x + r.w, r.y + r.h, radius)
}

pub struct Painter<'a> {
    pub scene: &'a mut DrawList,
    pub ui: &'a Interaction,
    /// Points to pixels.
    pub scale: f64,
    /// How far the view being drawn is from where layout put it (while it's
    /// animated to its place), in points.
    pub offset: (f64, f64),
    /// How the views around the one being drawn are turned and scaled
    /// (`.rotation`, `.scale`), in points.
    pub turn: Affine,
    /// The part of the window that shows, inside scroll views: what's
    /// outside needn't be drawn.
    pub clip: Option<Rect>,
    /// The views under `.animation` that were drawn: each one's key, where
    /// its drawing is in `scene`, and how long it's animated for (for
    /// `Animations::drew`: a view that's gone fades out as it was drawn).
    pub marks: Vec<(u64, usize, usize, f64)>,
}

impl Painter<'_> {
    fn t(&self) -> Affine {
        Affine::scale(self.scale) * self.turn * Affine::translate(self.offset)
    }

    /// Draws the scroll bars of a scroll view or code editor, over its content.
    fn paint_scroll_bars(&mut self, n: &Node) {
        let (down, across) = scroll_bars(n, self.ui);
        for bar in [down, across].into_iter().flatten() {
            self.fill(bar.thumb, Bar::THICKNESS / 2.0, theme().scroll_thumb);
        }
    }

    /// Underlines the text an input method is composing in view `n`, whose
    /// text is drawn at (x, y).
    fn underline_composing(&mut self, n: &Node, layout: &TextLayout, x: f64, y: f64) {
        let Some((range, c)) = self.ui.composing_in(n.key) else { return };
        let mut span = super::edit::Editing::new(n.key, "");
        (span.anchor, span.focus) = (range.start, range.start + c.text.len());
        for r in super::edit::geometry(layout, &span).1 {
            let line = Rect { x: x + r.x0, y: y + r.y1 - 1.0, w: r.x1 - r.x0, h: 1.0 };
            self.fill(line, 0.0, n.layout.style.color);
        }
    }

    /// Draws `svg` centered in `r`, scaled to fit.
    fn draw_svg(&mut self, svg: Rc<super::svg::Svg>, r: Rect) {
        let s = (r.w / svg.width.max(1.0)).min(r.h / svg.height.max(1.0));
        let x = r.x + (r.w - svg.width * s) / 2.0;
        let y = r.y + (r.h - svg.height * s) / 2.0;
        let t = self.t() * Affine::translate((x, y)) * Affine::scale(s);
        self.scene.svg(svg, t);
    }

    /// Draws the built-in icon `name` in `r`.
    fn svg(&mut self, name: &str, r: Rect, c: Rgba) {
        if let Some(svg) = super::svg::icon(name, c) {
            self.draw_svg(svg, r);
        }
    }

    fn fill(&mut self, r: Rect, radius: f64, c: Rgba) {
        let t = self.t();
        self.scene.fill(FillRule::NonZero, t, c, &rounded(r, radius));
    }

    fn circle(&mut self, x: f64, y: f64, radius: f64, c: Rgba) {
        let t = self.t();
        self.scene.fill(FillRule::NonZero, t, c, &Circle::new((x, y), radius));
    }

    fn stroke_circle(&mut self, x: f64, y: f64, radius: f64, width: f64, c: Rgba) {
        let t = self.t();
        self.scene.stroke(&Stroke::new(width), t, c, &Circle::new((x, y), radius - width / 2.0));
    }

    /// Draws the open `ColorPicker`'s palette over the rest of the window.
    pub fn paint_overlay(&mut self, tree: &Node) {
        self.paint_menu_list(tree);
        self.paint_ribbon_panel(tree);
        let Some(key) = super::controls::OPEN_PICKER.get() else { return };
        let Some(palette) = tree.find(key).and_then(|n| n.children.first()) else { return };
        let r = palette.layout.rect;
        self.fill(Rect { x: r.x, y: r.y + 2.0, w: r.w, h: r.h }.inset(-1.5), 9.5, Rgba([0, 0, 0, 24]));
        self.paint(palette);
    }

    /// Draws the open panel of a collapsed ribbon group, over everything
    /// else: a card with the group's items, and its title under them.
    fn paint_ribbon_panel(&mut self, tree: &Node) {
        use super::layout::{RIBBON_BODY, RIBBON_TITLE};
        let Some(n) = super::controls::OPEN_GROUP.get().and_then(|k| tree.find(k)) else { return };
        if !n.layout.collapsed {
            return;
        }
        let r = n.layout.panel;
        self.fill(Rect { x: r.x, y: r.y + 3.0, ..r }.inset(-1.5), 7.5, Rgba([0, 0, 0, 26]));
        self.fill(r, 6.0, theme().popup);
        self.stroke(r, 6.0, 1.0, theme().field_border);
        for c in &n.children {
            self.paint(c);
        }
        if let Some(t) = &n.layout.text {
            let y = r.y + RIBBON_BODY - RIBBON_TITLE + (RIBBON_TITLE - t.height()) / 2.0 - 3.0;
            self.text(t, r.x + (r.w - t.width()) / 2.0, y, None);
        }
    }

    /// Draws the open list of a menu picker, over everything else: its
    /// options, a check mark by the chosen one, and the one under the pointer.
    fn paint_menu_list(&mut self, tree: &Node) {
        let Some(n) = super::controls::OPEN_MENU.get().and_then(|k| tree.find(k)) else { return };
        let Kind::Picker { selection, .. } = &n.kind else { return };
        let (Some(first), Some(last)) = (n.layout.segments.first(), n.layout.segments.last()) else { return };
        let r = Rect { x: first.x - 4.0, y: first.y - 4.0, w: first.w + 8.0, h: last.y + last.h - first.y + 8.0 };
        self.fill(Rect { x: r.x, y: r.y + 3.0, ..r }.inset(-1.5), 9.5, Rgba([0, 0, 0, 26]));
        self.fill(r, 8.0, theme().popup);
        self.stroke(r, 8.0, 1.0, theme().field_border);
        let current = selection.locate();
        for (c, row) in n.children.iter().zip(&n.layout.segments) {
            if self.ui.pointer.is_some_and(|(x, y)| row.contains(x, y)) {
                self.fill(*row, 5.0, theme().selection);
            }
            if c.mods.tag.as_ref().is_some_and(|t| t.equals(current)) {
                let s = 11.0;
                self.svg("check", Rect { x: row.x + (super::layout::MENU_CHECK - s) / 2.0, y: row.y + (row.h - s) / 2.0, w: s, h: s }, theme().accent);
            }
            self.paint(c);
        }
    }

    /// Draws the view being dragged (`.draggable`) under the pointer, see-through.
    pub fn paint_dragged(&mut self, tree: &Node) {
        let Some(d) = self.ui.dragging.as_ref() else { return };
        let Some(n) = tree.find(d.key) else { return };
        let r = n.layout.rect;
        let (dx, dy) = (d.at.0 - d.grab.0 - r.x, d.at.1 - d.grab.1 - r.y);
        let mut copy = DrawList::new();
        let mut painter = Painter { scene: &mut copy, ui: self.ui, scale: self.scale, offset: (0.0, 0.0), turn: Affine::IDENTITY, clip: None, marks: Vec::new() };
        painter.paint(n);
        // On a card, lifted off the window.
        let t = self.t();
        let moved = Rect { x: r.x + dx, y: r.y + dy, ..r };
        self.scene.push(FillRule::NonZero, 0.8, t, &rounded(moved.inset(-8.0), 0.0));
        self.fill(Rect { x: moved.x, y: moved.y + 3.0, ..moved }.inset(-2.0), 7.0, Rgba([0, 0, 0, 30]));
        self.fill(moved, 6.0, theme().popup);
        self.stroke(moved, 6.0, 1.0, theme().field_border);
        self.scene.append(Rc::new(copy), Affine::translate((dx * self.scale, dy * self.scale)));
        self.scene.pop();
    }

    fn stroke(&mut self, r: Rect, radius: f64, width: f64, c: Rgba) {
        let t = self.t();
        let inner = r.inset(width / 2.0);
        self.scene.stroke(&Stroke::new(width), t, c, &rounded(inner, radius));
    }

    fn text(&mut self, layout: &Rc<TextLayout>, x: f64, y: f64, tint: Option<Rgba>) {
        let t = self.t() * Affine::translate((x, y));
        self.scene.text(layout.clone(), t, tint);
    }

    /// Draws the completion list, if one shows (after the tree, so it's on top).
    pub fn paint_popup(&mut self) {
        let Some(p) = self.ui.popup.as_ref() else { return };
        let r = p.rect;
        let shadow = Rect { x: r.x, y: r.y + 3.0, w: r.w, h: r.h };
        self.fill(shadow.inset(-2.0), 8.0, Rgba([0, 0, 0, 22]));
        self.fill(r, 6.0, theme().popup);
        let t = self.t();
        self.scene.push(FillRule::NonZero, 1.0, t, &rounded(r, 6.0));
        for (i, row) in p.rows.iter().enumerate() {
            let y = r.y + 4.0 + i as f64 * p.row_h;
            if p.selected == Some(i) {
                self.fill(Rect { x: r.x + 4.0, y, w: r.w - 8.0, h: p.row_h }, 4.0, theme().selection);
            }
            let badge = Rect { x: r.x + 8.0, y: y + (p.row_h - 16.0) / 2.0, w: 16.0, h: 16.0 };
            self.fill(badge, 4.0, row.badge_color);
            let (bw, bh) = (row.badge.width(), row.badge.height());
            self.text(&row.badge, badge.x + (16.0 - bw) / 2.0, badge.y + (16.0 - bh) / 2.0, None);
            let ty = y + (p.row_h - row.label.height()) / 2.0;
            self.text(&row.label, r.x + 32.0, ty, None);
            let dx = (r.x + r.w - 10.0 - row.detail.width()).max(r.x + 44.0 + row.label.width());
            let dy = y + (p.row_h - row.detail.height()) / 2.0;
            self.text(&row.detail, dx, dy, None);
        }
        self.scene.pop();
        self.stroke(r, 6.0, 1.0, theme().field_border);
    }

    /// Draws the menu bar, when it's drawn in the window.
    pub fn paint_menu_bar(&mut self) {
        let Some(bar) = self.ui.menu_bar.as_ref() else { return };
        let r = Rect { x: 0.0, y: 0.0, w: bar.width, h: super::menu::BAR_H };
        self.fill(r, 0.0, theme().colors[theme::SIDEBAR]);
        self.fill(Rect { x: 0.0, y: r.h - 1.0, w: r.w, h: 1.0 }, 0.0, theme().colors[theme::SEPARATOR]);
        for (i, (title, t)) in bar.titles.iter().enumerate() {
            let hovered = self.ui.pointer.is_some_and(|(x, y)| t.contains(x, y));
            if bar.open == Some(i) {
                self.fill(t.inset(2.0), 4.0, theme().selection);
            } else if hovered {
                self.fill(t.inset(2.0), 4.0, theme().colors[theme::LIGHT_GRAY]);
            }
            let y = t.y + (t.h - title.height() as f64) / 2.0;
            self.text(title, t.x + 10.0, y, None);
        }
    }

    /// Draws the open menus, if any (over everything but the tooltip).
    pub fn paint_menus(&mut self) {
        for panel in &self.ui.menus {
            let r = panel.rect;
            self.fill(Rect { x: r.x, y: r.y + 2.0, w: r.w, h: r.h }.inset(-1.0), 8.0, Rgba([0, 0, 0, 20]));
            self.fill(r, 6.0, theme().popup);
            for (i, row) in panel.rows.iter().enumerate() {
                let rr = row.rect;
                let Some(label) = &row.label else {
                    let line = Rect { x: rr.x + 6.0, y: rr.y + (rr.h / 2.0).floor(), w: rr.w - 12.0, h: 1.0 };
                    self.fill(line, 0.0, theme().colors[theme::SEPARATOR]);
                    continue;
                };
                let selected = panel.selected == Some(i) && row.enabled;
                if selected {
                    self.fill(rr, 4.0, theme().accent);
                }
                let tint = selected.then_some(Rgba([255, 255, 255, 255]));
                let ty = rr.y + (rr.h - label.height() as f64) / 2.0;
                if row.checked {
                    let c = tint.unwrap_or(theme().colors[theme::PRIMARY]);
                    self.svg("check", Rect { x: rr.x + 4.0, y: rr.y + (rr.h - 12.0) / 2.0, w: 12.0, h: 12.0 }, c);
                }
                self.text(label, rr.x + 18.0, ty, tint);
                if let Some(keys) = &row.shortcut {
                    let x = rr.x + rr.w - 10.0 - keys.width() as f64;
                    self.text(keys, x, rr.y + (rr.h - keys.height() as f64) / 2.0, tint);
                }
                if row.submenu {
                    let c = tint.unwrap_or(theme().colors[theme::SECONDARY]);
                    self.svg("chevronRight", Rect { x: rr.x + rr.w - 18.0, y: rr.y + (rr.h - 10.0) / 2.0, w: 10.0, h: 10.0 }, c);
                }
            }
            self.stroke(r, 6.0, 1.0, theme().field_border);
        }
    }

    /// Draws the notice (`showNotice`), if one shows: a dark pill near the
    /// bottom of the window.
    pub fn paint_notice(&mut self) {
        let Some(n) = self.ui.notice.as_ref() else { return };
        let r = n.rect;
        self.fill(Rect { x: r.x, y: r.y + 2.0, w: r.w, h: r.h }.inset(-1.0), r.h / 2.0 + 1.0, Rgba([0, 0, 0, 30]));
        self.fill(r, r.h / 2.0, Rgba([44, 44, 46, 240]));
        self.text(&n.layout, r.x + 16.0, r.y + 8.0, None);
    }

    /// Draws the tooltip, if one shows (last, so it's on top of everything).
    pub fn paint_tooltip(&mut self) {
        let Some(tip) = self.ui.tooltip.as_ref() else { return };
        let r = tip.rect;
        self.fill(Rect { x: r.x, y: r.y + 2.0, w: r.w, h: r.h }.inset(-1.0), 6.0, Rgba([0, 0, 0, 26]));
        self.fill(r, 5.0, theme().popup);
        self.stroke(r, 5.0, 1.0, theme().field_border);
        self.text(&tip.layout, r.x + 8.0, r.y + 5.0, None);
    }

    pub fn paint(&mut self, n: &Node) {
        // A view that's scrolled out of sight isn't drawn (a long list is
        // mostly that). Only one that's where layout put it: not while
        // it or what it's in is moving, or shown somewhere else.
        if let Some(clip) = self.clip {
            let (r, near) = (n.layout.rect, 64.0);
            let out = r.y > clip.y + clip.h + near || r.y + r.h < clip.y - near || r.x > clip.x + clip.w + near || r.x + r.w < clip.x - near;
            if out && self.offset == (0.0, 0.0) && n.mods.offset == (0.0, 0.0) && self.ui.anim.shown(n.key).is_none() {
                return;
            }
        }
        let from = self.scene.ops.len();
        self.paint_view(n);
        let animated = n.layout.style.animation;
        if animated > 0.0 && self.ui.anim.enabled && !n.mods.hidden {
            self.marks.push((n.key, from, self.scene.ops.len(), animated));
        }
    }

    fn paint_view(&mut self, n: &Node) {
        if n.mods.hidden {
            return;
        }
        // Turned or scaled: it, and what's in it.
        let around = self.turn;
        if let Some(turn) = super::layout::turn_of(n) {
            self.turn = around * turn;
        }
        self.paint_turned(n);
        self.turn = around;
    }

    fn paint_turned(&mut self, n: &Node) {
        // While the view is animated, its frame (background, border) is drawn
        // where it is now, and its content moved along with it.
        let placed = n.layout.rect;
        let shown = self.ui.anim.shown(n.key);
        let r = shown.map_or(placed, |s| s.rect);
        let outer = std::mem::replace(&mut self.offset, (0.0, 0.0));
        let style = n.layout.style;
        let dim = if style.disabled && !n.children.is_empty() { 1.0 } else if style.disabled { 0.4 } else { 1.0 };
        let alpha = shown.map_or(n.mods.opacity.unwrap_or(1.0), |s| s.opacity) * dim;
        let layered = alpha < 1.0;
        if layered {
            let t = self.t();
            self.scene.push(FillRule::NonZero, alpha as f32, t, &rounded(r, 0.0));
        }
        if let Some((c, radius)) = shown.map_or(n.mods.background, |s| s.background) {
            self.fill(r, radius, c);
        }
        // `.clipped()`: what's in the view stays inside its frame (with the
        // corners of its background or border).
        if n.mods.clipped {
            let radius = n.mods.background.map(|b| b.1).or(n.mods.border.map(|b| b.2)).unwrap_or(0.0);
            let t = self.t();
            self.scene.push(FillRule::NonZero, 1.0, t, &rounded(r, radius));
        }
        if let (Some((c, radius)), true) = (n.mods.hover_background, self.ui.hover_background == Some(n.key)) {
            self.fill(r, radius, c);
        }
        self.offset = (r.x - placed.x, r.y - placed.y);
        let content = placed.inset_by(n.mods.padding);
        // A popover: a card, with a shadow, that its content is drawn on.
        if let Kind::Popover { edge, .. } = n.kind {
            if edge == super::SHEET {
                // A sheet dims the rest of the window.
                let window = super::layout::window_size();
                self.fill(Rect { x: 0.0, y: 0.0, w: window.w, h: window.h }, 0.0, Rgba([0, 0, 0, 70]));
            }
            self.fill(Rect { x: r.x, y: r.y + 2.0, w: r.w, h: r.h }.inset(-1.0), 10.0, Rgba([0, 0, 0, 20]));
            self.fill(r, 8.0, theme().popup);
            self.stroke(r, 8.0, 1.0, theme().field_border);
        }
        let hovered = self.ui.hovered == Some(n.key) && !style.disabled;
        let pressed = self.ui.pressed == Some(n.key) && !style.disabled;
        match &n.kind {
            Kind::Text(_) => {
                if let Some(t) = &n.layout.text {
                    let c = n.layout.content;
                    // Text that's selectable shows what's selected of it.
                    if let Some(e) = self.ui.shown_editing(n.key) {
                        for r in t.selection(e.range()) {
                            self.fill(Rect { x: c.x + r.x0, y: c.y + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 }, 2.0, theme().selection);
                        }
                    }
                    self.text(t, c.x, c.y, None);
                }
            }
            Kind::Button { .. } => {
                if n.mods.background.is_none() {
                    let bg = if pressed { theme().control_pressed } else if hovered { theme().control_hover } else { theme().control };
                    self.fill(content, 7.0, bg);
                }
                if let Some(t) = &n.layout.text {
                    let x = content.x + (content.w - t.width()) / 2.0;
                    let y = content.y + (content.h - t.height()) / 2.0;
                    let tint = if n.mods.color.is_none() { Some(theme().accent) } else { None };
                    self.text(t, x, y, tint);
                }
            }
            Kind::TextField { .. } => {
                let editing = self.ui.shown_editing(n.key);
                let editing = editing.as_ref();
                self.fill(content, 6.0, theme().field);
                if editing.is_some() {
                    self.stroke(content, 6.0, 2.0, theme().accent);
                } else {
                    self.stroke(content, 6.0, 1.0, theme().field_border);
                }
                let search = matches!(n.kind, Kind::TextField { search: true, .. });
                if search {
                    // The magnifying glass, and the button that clears the text.
                    let size = 14.0;
                    let glass = Rect { x: content.x + 8.0, y: content.y + (content.h - size) / 2.0, w: size, h: size };
                    self.svg("search", glass, theme().colors[theme::SECONDARY]);
                    if let Some(r) = search_clear(n) {
                        self.fill(r, r.w / 2.0, theme().colors[theme::SECONDARY]);
                        let x = 8.0;
                        self.svg("close", Rect { x: r.x + (r.w - x) / 2.0, y: r.y + (r.h - x) / 2.0, w: x, h: x }, theme().field);
                    }
                }
                if let (Some(t), Some(ph)) = (&n.layout.text, &n.layout.placeholder) {
                    let (x0, y0) = field_origin(n);
                    let line_h = ph.height().max(t.height());
                    let t_scene = self.t();
                    // (A search field's text stays between its glass and its button.)
                    let area = if search {
                        let left = super::layout::SEARCH_LEFT - 2.0;
                        Rect { x: content.x + left, y: content.y, w: (content.w - left - super::layout::SEARCH_RIGHT).max(0.0), h: content.h }.inset(2.0)
                    } else {
                        content.inset(2.0)
                    };
                    self.scene.push(FillRule::NonZero, 1.0, t_scene, &rounded(area, 4.0));
                    let scroll = editing.map_or(0.0, |e| e.scroll);
                    // (Several lines scroll down, one line across.)
                    let down = self.ui.scroll.get(&n.key).copied().unwrap_or(0.0);
                    let (scroll, y0) = if matches!(n.kind, Kind::TextField { lines: 2.., .. }) { (0.0, y0 - down) } else { (scroll, y0) };
                    if let Some(e) = editing {
                        let (caret, rects) = super::edit::geometry(t, e);
                        for r in rects {
                            let sel = Rect { x: x0 - scroll + r.x0, y: y0 + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 };
                            self.fill(sel, 2.0, theme().selection);
                        }
                        let caret = match caret {
                            Some(c) => Rect { x: x0 - scroll + c.x0, y: y0 + c.y0, w: 1.5, h: c.y1 - c.y0 },
                            None => Rect { x: x0, y: y0, w: 1.5, h: line_h },
                        };
                        if e.range().is_empty() {
                            self.fill(caret, 0.0, theme().accent);
                        }
                    }
                    if n.layout.empty {
                        self.text(ph, x0, y0, Some(theme().placeholder));
                    } else {
                        self.text(t, x0 - scroll, y0, None);
                        self.underline_composing(n, t, x0 - scroll, y0);
                    }
                    self.scene.pop();
                    self.paint_scroll_bars(n);
                }
            }
            // (The system's web view is over it. Without one, a box says what it shows.)
            Kind::WebView { .. } => {
                self.fill(content, 0.0, theme().colors[theme::BACKGROUND]);
                self.stroke(content, 0.0, 1.0, theme().field_border);
            }
            Kind::Video { controls, .. } => {
                self.fill(content, 0.0, Rgba([0, 0, 0, 255]));
                let (picture, playing, at, of) = super::media::VIDEOS.with(|v| {
                    let v = v.borrow();
                    let p = v.get(&n.key).map(|(_, p)| p);
                    (p.and_then(|p| p.picture.clone()), p.is_some_and(super::media::Player::playing), p.map_or(0.0, |p| p.position()), p.map_or(0.0, |p| p.duration()))
                });
                if let Some(pic) = picture {
                    // Fitted inside, in the middle.
                    let s = (content.w / pic.width.max(1.0)).min(content.h / pic.height.max(1.0));
                    let (w, h) = (pic.width * s, pic.height * s);
                    let t = self.t() * Affine::translate((content.x + (content.w - w) / 2.0, content.y + (content.h - h) / 2.0)) * Affine::scale(s);
                    self.scene.image(pic, t, 1.0);
                }
                let over = self.ui.pointer.is_some_and(|(x, y)| content.contains(x, y));
                if *controls && (over || !playing) {
                    let (bar, button, track) = video_parts(n);
                    self.fill(bar, 0.0, Rgba([0, 0, 0, 140]));
                    let white = Rgba([255, 255, 255, 255]);
                    if playing {
                        // Pause: two bars.
                        let (w, h) = (4.0, 12.0);
                        let (x, y) = (button.x + button.w / 2.0, button.y + (button.h - h) / 2.0);
                        self.fill(Rect { x: x - 5.0, y, w, h }, 1.0, white);
                        self.fill(Rect { x: x + 1.0, y, w, h }, 1.0, white);
                    } else {
                        self.svg("play", button.inset(4.0), white);
                    }
                    self.fill(track, 2.0, Rgba([255, 255, 255, 70]));
                    let part = if of > 0.0 { (at / of).clamp(0.0, 1.0) } else { 0.0 };
                    self.fill(Rect { w: track.w * part, ..track }, 2.0, white);
                    let knob = 10.0;
                    self.fill(Rect { x: track.x + track.w * part - knob / 2.0, y: track.y + track.h / 2.0 - knob / 2.0, w: knob, h: knob }, knob / 2.0, white);
                    if let Some(t) = &n.layout.text {
                        self.text(t, bar.x + bar.w - t.width() - 10.0, bar.y + (bar.h - t.height()) / 2.0, None);
                    }
                }
            }
            Kind::RichEditor { doc, .. } => {
                let editing = self.ui.shown_editing(n.key);
                self.fill(content, 6.0, theme().field);
                if let Some(t) = &n.layout.text {
                    let (x0, y0) = field_origin(n);
                    let y0 = y0 - self.ui.scroll.get(&n.key).copied().unwrap_or(0.0);
                    let t_scene = self.t();
                    self.scene.push(FillRule::NonZero, 1.0, t_scene, &rounded(content.inset(2.0), 4.0));
                    if let Some(e) = &editing {
                        let (caret, rects) = super::edit::geometry(t, e);
                        for r in rects {
                            self.fill(Rect { x: x0 + r.x0, y: y0 + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 }, 2.0, theme().selection);
                        }
                        if e.range().is_empty() {
                            let caret = match caret {
                                Some(c) => Rect { x: x0 + c.x0, y: y0 + c.y0, w: 1.5, h: c.y1 - c.y0 },
                                None => Rect { x: x0, y: y0, w: 1.5, h: style.size * 1.2 },
                            };
                            self.fill(caret, 0.0, theme().accent);
                        }
                    }
                    self.text(t, x0, y0, None);
                    // Underlines and lines through, under the runs that have them.
                    for (range, m) in &doc.runs {
                        if !(m.underline || m.strike) {
                            continue;
                        }
                        let size = m.size.unwrap_or(style.size);
                        let color = m.color.as_ref().and_then(super::rich::color_of).unwrap_or(style.color);
                        let thick = (size / 14.0).max(1.0);
                        for r in t.selection(range.clone()) {
                            let w = r.x1 - r.x0;
                            if m.underline {
                                self.fill(Rect { x: x0 + r.x0, y: y0 + r.y1 - size * 0.16, w, h: thick }, 0.0, color);
                            }
                            if m.strike {
                                self.fill(Rect { x: x0 + r.x0, y: y0 + r.y1 - size * 0.55, w, h: thick }, 0.0, color);
                            }
                        }
                    }
                    self.underline_composing(n, t, x0, y0);
                    self.scene.pop();
                    self.paint_scroll_bars(n);
                }
                if editing.is_some() {
                    self.stroke(content, 6.0, 2.0, theme().accent);
                } else {
                    self.stroke(content, 6.0, 1.0, theme().field_border);
                }
            }
            Kind::Toggle { is_on, .. } => {
                // How far on it's drawn: it slides from one side to the other.
                let on = self.ui.anim.toggle(n.key, unsafe { *is_on.locate() } & 1 != 0);
                let sw = Rect { x: content.x + content.w - 38.0, y: content.y + (content.h - 22.0) / 2.0, w: 38.0, h: 22.0 };
                let (off_color, on_color) = (theme().control_pressed, theme().switch_on);
                let track = Rgba(std::array::from_fn(|i| {
                    (off_color.0[i] as f64 + (on_color.0[i] as f64 - off_color.0[i] as f64) * on).round() as u8
                }));
                self.fill(sw, 11.0, track);
                let knob_x = sw.x + 11.0 + (sw.w - 22.0) * on;
                let t = self.t();
                self.scene.fill(FillRule::NonZero, t, Rgba([255, 255, 255, 255]), &Circle::new((knob_x, sw.y + 11.0), 9.0));
                if let Some(label) = &n.layout.text {
                    self.text(label, content.x, content.y + (content.h - label.height()) / 2.0, None);
                }
            }
            Kind::Picker { selection, style: 2 } => {
                // The box: the chosen option, and an arrow.
                let bg = if pressed { theme().control_pressed } else if hovered { theme().control_hover } else { theme().field };
                self.fill(content, 6.0, bg);
                self.stroke(content, 6.0, 1.0, theme().field_border);
                let arrow = 10.0;
                let a = Rect { x: content.x + content.w - super::layout::MENU_ARROW + (super::layout::MENU_ARROW - arrow) / 2.0 - 2.0, y: content.y + (content.h - arrow) / 2.0, w: arrow, h: arrow };
                self.svg("chevronDown", a, theme().colors[theme::SECONDARY]);
                let current = selection.locate();
                let chosen = n.children.iter().find(|c| c.mods.tag.as_ref().is_some_and(|t| t.equals(current)));
                if let Some(c) = chosen {
                    // (While the list is open, the option is there: drawn in the box too.)
                    let at = (content.x + super::layout::MENU_PAD, content.y + (content.h - c.layout.size.h) / 2.0);
                    let shift = (at.0 - c.layout.rect.x, at.1 - c.layout.rect.y);
                    let around = self.turn;
                    self.turn = around * Affine::translate(shift);
                    self.paint(c);
                    self.turn = around;
                }
            }
            Kind::Picker { selection, style: 1 } => {
                // Radio buttons: a ring, filled with a dot when it's chosen.
                let current = selection.locate();
                let size = super::layout::RADIO;
                for (c, seg) in n.children.iter().zip(&n.layout.segments) {
                    if c.mods.hidden {
                        continue;
                    }
                    let selected = c.mods.tag.as_ref().is_some_and(|tag| tag.equals(current));
                    let ring = Rect { x: seg.x, y: seg.y + (seg.h - size) / 2.0, w: size, h: size };
                    let over = self.ui.pointer.is_some_and(|(x, y)| seg.contains(x, y)) && !style.disabled;
                    if selected {
                        self.fill(ring, size / 2.0, theme().accent);
                        let dot = 6.0;
                        self.fill(Rect { x: ring.x + (size - dot) / 2.0, y: ring.y + (size - dot) / 2.0, w: dot, h: dot }, dot / 2.0, Rgba([255, 255, 255, 255]));
                    } else {
                        self.fill(ring, size / 2.0, if over { theme().control_hover } else { theme().field });
                        self.stroke(ring, size / 2.0, 1.0, theme().field_border);
                    }
                    self.paint(c);
                }
            }
            Kind::Picker { selection, .. } => {
                self.fill(content, 8.0, theme().control);
                let current = selection.locate();
                for (c, seg) in n.children.iter().zip(&n.layout.segments) {
                    let selected = c.mods.tag.as_ref().is_some_and(|tag| tag.equals(current));
                    if selected {
                        self.fill(*seg, 6.0, theme().segment);
                        self.stroke(*seg, 6.0, 0.5, theme().field_border);
                    }
                    self.paint(c);
                }
            }
            Kind::CodeEditor { error_lines, breakpoints, stopped_line, .. } => {
                let editing = self.ui.shown_editing(n.key);
                let editing = editing.as_ref();
                let t_scene = self.t();
                self.fill(content, 6.0, theme().field);
                self.scene.push(FillRule::NonZero, 1.0, t_scene, &rounded(content, 6.0));
                let gutter = Rect { x: content.x, y: content.y, w: n.layout.gutter, h: content.h };
                self.fill(gutter, 0.0, theme().gutter);
                let (x0, y0) = code_origin(n);
                let scroll_y = self.ui.scroll.get(&n.key).copied().unwrap_or(0.0);
                let scroll_x = self.ui.scroll_x.get(&n.key).copied().unwrap_or(0.0);
                if let Some(t) = &n.layout.text {
                    let lines: Vec<(f64, f64)> = t.lines.iter().map(|l| (l.top, l.bottom)).collect();
                    for &line in error_lines {
                        let Some(&(top, bottom)) = lines.get((line - 1).max(0) as usize) else { continue };
                        let y = y0 - scroll_y + top;
                        let band = Rect { x: content.x + n.layout.gutter, y, w: content.w - n.layout.gutter, h: bottom - top };
                        self.fill(band, 0.0, theme().error_line);
                        self.fill(Rect { x: content.x, y, w: 3.0, h: bottom - top }, 0.0, theme().error_mark);
                    }
                    // Where a program is stopped: the whole line, in yellow.
                    if let Some(&(top, bottom)) = lines.get((*stopped_line - 1).max(0) as usize).filter(|_| *stopped_line > 0) {
                        let band = Rect { x: content.x, y: y0 - scroll_y + top, w: content.w, h: bottom - top };
                        self.fill(band, 0.0, theme().stopped_line);
                    }
                    // Breakpoints: a red dot at the margin's left.
                    for &line in breakpoints {
                        let Some(&(top, bottom)) = lines.get((line - 1).max(0) as usize) else { continue };
                        let size = ((bottom - top) * 0.55).min(9.0);
                        let dot = Rect { x: content.x + 3.0, y: y0 - scroll_y + top + (bottom - top - size) / 2.0, w: size, h: size };
                        self.fill(dot, size / 2.0, theme().error_mark);
                    }
                    if let Some(numbers) = &n.layout.placeholder {
                        self.text(numbers, content.x + GUTTER_PAD.0, y0 - scroll_y, None);
                    }
                    let code_area = Rect { x: content.x + n.layout.gutter, y: content.y, w: content.w - n.layout.gutter, h: content.h };
                    self.scene.push(FillRule::NonZero, 1.0, t_scene, &rounded(code_area, 0.0));
                    if let Some(e) = editing {
                        let (caret, rects) = super::edit::geometry(t, e);
                        for r in rects {
                            let sel = Rect { x: x0 - scroll_x + r.x0, y: y0 - scroll_y + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 };
                            self.fill(sel, 1.0, theme().selection);
                        }
                        if e.range().is_empty() {
                            let line_h = lines.first().map_or(16.0, |(a, b)| b - a);
                            let caret = match caret {
                                Some(c) => Rect { x: x0 - scroll_x + c.x0, y: y0 - scroll_y + c.y0, w: 1.5, h: c.y1 - c.y0 },
                                None => Rect { x: x0, y: y0, w: 1.5, h: line_h },
                            };
                            self.fill(caret, 0.0, theme().accent);
                        }
                    }
                    self.text(t, x0 - scroll_x, y0 - scroll_y, None);
                    self.underline_composing(n, t, x0 - scroll_x, y0 - scroll_y);
                    self.scene.pop();
                }
                self.scene.pop();
                self.paint_scroll_bars(n);
                if editing.is_some() {
                    self.stroke(content, 6.0, 2.0, theme().accent);
                } else {
                    self.stroke(content, 6.0, 1.0, theme().field_border);
                }
            }
            Kind::TabView { .. } => {
                let bar = Rect { x: content.x, y: content.y, w: content.w, h: super::layout::TAB_BAR };
                self.fill(bar, 0.0, theme().colors[theme::SIDEBAR]);
                self.fill(Rect { x: bar.x, y: bar.y + bar.h - 1.0, w: bar.w, h: 1.0 }, 0.0, theme().colors[theme::SEPARATOR]);
                let selected = n.selected_tab();
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    let r = tab.rect;
                    let hovered = self.ui.pointer.is_some_and(|(x, y)| r.contains(x, y));
                    if i == selected {
                        self.fill(r, 0.0, theme().colors[theme::BACKGROUND]);
                        self.fill(Rect { x: r.x, y: r.y, w: r.w, h: 2.0 }, 0.0, theme().accent);
                    } else if hovered {
                        self.fill(r, 0.0, theme().colors[theme::LIGHT_GRAY]);
                    }
                    self.fill(Rect { x: r.x + r.w - 1.0, y: r.y, w: 1.0, h: r.h }, 0.0, theme().colors[theme::SEPARATOR]);
                    let tint = if i == selected { None } else { Some(theme().colors[theme::SECONDARY]) };
                    let ty = r.y + (r.h - tab.title.height()) / 2.0;
                    self.text(&tab.title, r.x + 14.0, ty, tint);
                    if let (Some(c), true) = (tab.close, i == selected || hovered) {
                        let over = self.ui.pointer.is_some_and(|(x, y)| c.contains(x, y));
                        if over {
                            self.fill(c, 4.0, theme().colors[theme::LIGHT_GRAY]);
                        }
                        self.svg("close", Rect { x: c.x + c.w / 2.0 - 6.0, y: c.y + c.h / 2.0 - 6.0, w: 12.0, h: 12.0 }, theme().colors[theme::SECONDARY]);
                    }
                }
                if let Some(c) = n.children.get(selected) {
                    self.paint(c);
                }
            }
            Kind::Ribbon { .. } => {
                use super::layout::{RIBBON_BODY, RIBBON_TABS};
                let strip = Rect { x: content.x, y: content.y, w: content.w, h: RIBBON_TABS };
                let body = Rect { x: content.x, y: content.y + RIBBON_TABS, w: content.w, h: RIBBON_BODY };
                self.fill(strip, 0.0, theme().colors[theme::SIDEBAR]);
                self.fill(body, 0.0, theme().colors[theme::BACKGROUND]);
                self.fill(Rect { x: body.x, y: body.y + body.h - 1.0, w: body.w, h: 1.0 }, 0.0, theme().colors[theme::SEPARATOR]);
                self.fill(Rect { x: body.x, y: body.y, w: body.w, h: 1.0 }, 0.0, theme().colors[theme::SEPARATOR]);
                let selected = n.selected_ribbon_tab();
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    let r = tab.rect;
                    let over = self.ui.pointer.is_some_and(|(x, y)| r.contains(x, y));
                    if i == selected {
                        // Joined to the panel under it.
                        self.fill(Rect { h: r.h + 1.0, ..r }, 0.0, theme().colors[theme::BACKGROUND]);
                        let tw = tab.title.width();
                        self.fill(Rect { x: r.x + (r.w - tw) / 2.0, y: r.y + r.h - 3.0, w: tw, h: 2.0 }, 1.0, theme().accent);
                    } else if over {
                        self.fill(r.inset(1.0), 4.0, theme().control_hover);
                    }
                    let tint = if i == selected { Some(theme().accent) } else { None };
                    let ty = r.y + (r.h - tab.title.height()) / 2.0 - 1.0;
                    self.text(&tab.title, r.x + (r.w - tab.title.width()) / 2.0, ty, tint);
                }
                if let Some(c) = n.children.get(selected) {
                    self.paint(c);
                }
            }
            Kind::RibbonGroup { icon, .. } => {
                use super::layout::{RIBBON_BODY, RIBBON_PAD, RIBBON_TITLE};
                let r = placed;
                self.fill(Rect { x: r.x + r.w, y: r.y + 6.0, w: 1.0, h: RIBBON_BODY - 12.0 }, 0.0, theme().colors[theme::SEPARATOR]);
                if n.layout.collapsed {
                    // One button: the group's icon over its title and an arrow.
                    let open = super::controls::OPEN_GROUP.get() == Some(n.key);
                    let b = r.inset(RIBBON_PAD);
                    if pressed || open {
                        self.fill(b, 4.0, theme().control_pressed);
                    } else if hovered {
                        self.fill(b, 4.0, theme().control_hover);
                    }
                    let color = theme().colors[theme::PRIMARY];
                    let s = super::layout::RIBBON_LARGE_ICON;
                    self.svg(ribbon_group_icon(n, icon), Rect { x: b.x + (b.w - s) / 2.0, y: b.y + 8.0, w: s, h: s }, color);
                    if let Some(t) = &n.layout.placeholder {
                        let x = b.x + (b.w - t.width() - 12.0) / 2.0;
                        let y = b.y + 8.0 + s + 6.0;
                        self.text(t, x, y, None);
                        self.svg("chevronDown", Rect { x: x + t.width() + 2.0, y: y + (t.height() - 10.0) / 2.0, w: 10.0, h: 10.0 }, color);
                    }
                } else if let Some(t) = &n.layout.text {
                    let y = r.y + RIBBON_BODY - RIBBON_TITLE + (RIBBON_TITLE - t.height()) / 2.0 - 3.0;
                    self.text(t, r.x + (r.w - t.width()) / 2.0, y, None);
                }
            }
            Kind::RibbonItem { icon, large, role, .. } => {
                use super::layout::{RIBBON_ICON, RIBBON_PAD};
                let r = placed;
                let on = match role {
                    super::RibbonRole::Toggle(is_on) => (unsafe { *is_on.locate() } & 1) != 0,
                    _ => false,
                };
                let open = self.ui.menus.first().is_some() && pressed;
                if on {
                    self.fill(r, 4.0, theme().selection);
                }
                if pressed || open {
                    self.fill(r, 4.0, theme().control_pressed);
                } else if hovered {
                    self.fill(r, 4.0, theme().control_hover);
                }
                let color = theme().colors[theme::PRIMARY];
                let menu = matches!(role, super::RibbonRole::Menu);
                let large = *large && !n.layout.compact;
                if large {
                    let s = super::layout::RIBBON_LARGE_ICON;
                    self.svg(icon, Rect { x: r.x + (r.w - s) / 2.0, y: r.y + 6.0, w: s, h: s }, color);
                    if let Some(t) = &n.layout.text {
                        let arrow = if menu { 12.0 } else { 0.0 };
                        let x = r.x + (r.w - t.width() - arrow) / 2.0;
                        let y = r.y + 6.0 + s + 4.0;
                        self.text(t, x, y, None);
                        if menu {
                            self.svg("chevronDown", Rect { x: x + t.width() + 2.0, y: y + (t.height() - 10.0) / 2.0, w: 10.0, h: 10.0 }, color);
                        }
                    }
                } else {
                    let y = r.y + (r.h - RIBBON_ICON) / 2.0;
                    self.svg(icon, Rect { x: r.x + RIBBON_PAD, y, w: RIBBON_ICON, h: RIBBON_ICON }, color);
                    if let Some(t) = &n.layout.text {
                        let x = r.x + RIBBON_PAD + RIBBON_ICON + 5.0;
                        self.text(t, x, r.y + (r.h - t.height()) / 2.0, None);
                        if menu {
                            self.svg("chevronDown", Rect { x: x + t.width() + 3.0, y: r.y + (r.h - 10.0) / 2.0, w: 10.0, h: 10.0 }, color);
                        }
                    }
                }
            }
            Kind::Divider => {
                // (Inside its padding: padding is room around the line.)
                self.fill(content, 0.0, theme().colors[theme::SEPARATOR]);
            }
            Kind::Split { .. } => {
                if let Some(line) = n.layout.segments.first() {
                    self.fill(*line, 0.0, theme().colors[theme::SEPARATOR]);
                }
            }
            Kind::Svg { source, .. } => {
                if let Some(svg) = super::svg::load(source, style.color) {
                    self.draw_svg(svg, n.layout.content);
                }
            }
            Kind::Image { picture: Some(p), .. } => {
                let c = n.layout.content;
                let s = (c.w / p.width.max(1.0)).min(c.h / p.height.max(1.0));
                let t = self.t() * Affine::translate((c.x, c.y)) * Affine::scale(s);
                self.scene.image(p.clone(), t, 1.0);
            }
            Kind::Image { .. } => {
                self.fill(content, 8.0, theme().control);
                self.stroke(content, 8.0, 1.0, theme().field_border);
            }
            Kind::Canvas { draw, redraw } => {
                // The program's drawing, in the canvas's own coordinates, and
                // nothing of it outside the canvas.
                let drawing = super::canvas::drawing(n.key, draw, *redraw, content.w, content.h);
                let t = self.t();
                self.scene.push(FillRule::NonZero, 1.0, t, &rounded(content, 0.0));
                self.scene.append(drawing.list.clone(), t * Affine::translate((content.x, content.y)));
                self.scene.pop();
            }
            Kind::Scene3D { draw, redraw } => {
                // The scene's picture, as large as the view, and its labels over it.
                let shot = super::scene3d::shot(n.key, draw, *redraw, content.w, content.h, self.scale, true);
                let t = self.t() * Affine::translate((content.x, content.y));
                match &shot.picture {
                    Some(p) => self.scene.image(p.clone(), t * Affine::scale_non_uniform(content.w / p.width.max(1.0), content.h / p.height.max(1.0)), 1.0),
                    None => {
                        self.fill(content, 0.0, theme().control);
                        self.stroke(content, 0.0, 1.0, theme().field_border);
                    }
                }
                self.scene.push(FillRule::NonZero, 1.0, self.t(), &rounded(content, 0.0));
                for label in &shot.labels {
                    self.scene.text(label.layout.clone(), t * Affine::translate((label.x, label.y)), Some(label.color));
                }
                self.scene.pop();
            }
            Kind::Slider { value, from, to, .. } => {
                let v = super::controls::read_number(value, false);
                let f = if to > from { ((v - from) / (to - from)).clamp(0.0, 1.0) } else { 0.0 };
                let knob = super::layout::KNOB;
                let (x0, x1) = (content.x + knob, content.x + content.w - knob);
                let cy = content.y + content.h / 2.0;
                let kx = x0 + f * (x1 - x0).max(0.0);
                self.fill(Rect { x: x0, y: cy - 2.0, w: (x1 - x0).max(0.0), h: 4.0 }, 2.0, theme().control_pressed);
                self.fill(Rect { x: x0, y: cy - 2.0, w: kx - x0, h: 4.0 }, 2.0, theme().accent);
                self.circle(kx, cy + 1.0, knob + 0.5, Rgba([0, 0, 0, 40]));
                self.circle(kx, cy, knob, if pressed { theme().control } else { Rgba([255, 255, 255, 255]) });
                self.stroke_circle(kx, cy, knob, 1.0, theme().field_border);
            }
            Kind::Stepper { value, int, from, to, .. } => {
                let v = super::controls::read_number(value, *int);
                let b = super::layout::STEPPER_BUTTON;
                self.fill(content, 7.0, theme().control);
                let minus = Rect { x: content.x, y: content.y, w: b, h: content.h };
                let plus = Rect { x: content.x + content.w - b, y: content.y, w: b, h: content.h };
                for (half, enabled) in [(minus, v > *from), (plus, v < *to)] {
                    let over = self.ui.pointer.is_some_and(|(x, y)| half.contains(x, y));
                    if hovered && over && enabled {
                        let bg = if pressed { theme().control_pressed } else { theme().control_hover };
                        self.fill(half, 7.0, bg);
                    }
                }
                let sep = theme().colors[theme::SEPARATOR];
                self.fill(Rect { x: minus.x + b, y: content.y + 5.0, w: 1.0, h: content.h - 10.0 }, 0.0, sep);
                self.fill(Rect { x: plus.x - 1.0, y: content.y + 5.0, w: 1.0, h: content.h - 10.0 }, 0.0, sep);
                let icon = |r: Rect| Rect { x: r.x + r.w / 2.0 - 6.0, y: r.y + r.h / 2.0 - 6.0, w: 12.0, h: 12.0 };
                let tint = |on: bool| if on { theme().accent } else { theme().colors[theme::GRAY] };
                self.svg("minus", icon(minus), tint(v > *from));
                self.svg("plus", icon(plus), tint(v < *to));
                if let Some(t) = &n.layout.text {
                    let x = content.x + (content.w - t.width() as f64) / 2.0;
                    let y = content.y + (content.h - t.height() as f64) / 2.0;
                    self.text(t, x, y, None);
                }
            }
            Kind::ColorPicker { color: c, .. } => {
                let c = super::controls::read_color(c);
                let bg = if hovered { theme().control_hover } else { theme().control };
                self.fill(content, 6.0, bg);
                self.stroke(content, 6.0, 1.0, theme().field_border);
                let inner = content.inset(4.0);
                self.fill(inner, 3.0, c);
                self.stroke(inner, 3.0, 1.0, Rgba([0, 0, 0, 40]));
            }
            Kind::Swatch(c) => {
                self.fill(content, 4.0, *c);
                self.stroke(content, 4.0, 1.0, Rgba([0, 0, 0, 36]));
                if super::controls::PICKED.get() == Some(*c) {
                    self.stroke(content.inset(-2.5), 6.0, 2.0, theme().accent);
                } else if hovered {
                    self.stroke(content.inset(-1.5), 5.0, 1.5, theme().colors[theme::PRIMARY]);
                }
            }
            Kind::Table(spec) => {
                let t = theme();
                self.fill(content, 6.0, t.field);
                self.stroke(content, 6.0, 1.0, t.field_border);
                if spec.tree.is_none() {
                    // The header: each column's title (lined up as its cells
                    // are), the arrow of the column sorted by, the edges.
                    let bar = Rect { x: content.x, y: content.y, w: content.w, h: super::table::HEADER };
                    self.fill(bar, 6.0, t.control);
                    self.fill(Rect { y: bar.y + 6.0, h: bar.h - 6.0, ..bar }, 0.0, t.control);
                    self.fill(Rect { y: bar.y + bar.h - 1.0, h: 1.0, ..bar }, 0.0, t.field_border);
                    let sorted = spec.sort.as_ref().map(|(by, up)| unsafe { (*(by.locate() as *const i64), *up.locate() & 1 != 0) });
                    let pad = super::table::CELL_PAD;
                    let t_scene = self.t();
                    self.scene.push(FillRule::NonZero, 1.0, t_scene, &rounded(bar, 0.0));
                    for (i, (tab, column)) in n.layout.tabs.iter().zip(&spec.columns).enumerate() {
                        let r = tab.rect;
                        let arrow = matches!(sorted, Some((c, _)) if c == i as i64);
                        let room = if arrow { 14.0 } else { 0.0 };
                        let w = tab.title.width();
                        let x = match column.align {
                            1 => r.x + (r.w - w - room) / 2.0,
                            2 => r.x + r.w - pad - w - room,
                            _ => r.x + pad,
                        };
                        self.text(&tab.title, x, r.y + (r.h - tab.title.height()) / 2.0, Some(t.colors[theme::SECONDARY]));
                        if let (true, Some((_, up))) = (arrow, sorted) {
                            let a = Rect { x: x + w + 4.0, y: r.y + (r.h - 10.0) / 2.0, w: 10.0, h: 10.0 };
                            self.svg(if up { "chevronUp" } else { "chevronDown" }, a, t.colors[theme::SECONDARY]);
                        }
                        if i > 0 {
                            self.fill(Rect { x: r.x - 0.5, y: r.y + 6.0, w: 1.0, h: r.h - 12.0 }, 0.0, t.field_border);
                        }
                    }
                    self.scene.pop();
                }
            }
            Kind::TableRow(row) => {
                let t = theme();
                if row.selected {
                    // (Brighter while the keyboard is on the table.)
                    let focused = self.ui.focused == Some(row.table);
                    self.fill(content, 0.0, if focused { t.selection } else { t.control_pressed });
                } else if row.index % 2 == 1 {
                    let c = t.control;
                    self.fill(content, 0.0, Rgba([c.0[0], c.0[1], c.0[2], c.0[3] / 2]));
                }
                if row.expandable {
                    let x = content.x + super::table::CELL_PAD + row.depth as f64 * super::table::INDENT;
                    let a = Rect { x: x + 2.0, y: content.y + (content.h - 11.0) / 2.0, w: 11.0, h: 11.0 };
                    self.svg(if row.expanded { "chevronDown" } else { "chevronRight" }, a, t.colors[theme::SECONDARY]);
                }
            }
            Kind::Disclosure { expanded, .. } => {
                let open = unsafe { *expanded.locate() } & 1 != 0;
                if let Some(header) = n.layout.segments.first().copied() {
                    let a = super::layout::ARROW;
                    let arrow = Rect { x: header.x, y: header.y + (header.h - a) / 2.0, w: a, h: a };
                    self.svg(if open { "chevronDown" } else { "chevronRight" }, arrow, theme().colors[theme::SECONDARY]);
                    if let Some(t) = &n.layout.text {
                        self.text(t, header.x + a + 6.0, header.y + (header.h - t.height() as f64) / 2.0, None);
                    }
                }
            }
            Kind::Scroll(_) => {
                let t = self.t();
                self.scene.push(FillRule::NonZero, 1.0, t, &rounded(content, 0.0));
                let outer = self.clip.replace(content);
                for c in &n.children {
                    self.paint(c);
                }
                self.clip = outer;
                self.scene.pop();
                self.paint_scroll_bars(n);
            }
            Kind::Markdown { .. } => {
                if let Some(laid) = &n.layout.markdown {
                    let (x0, y0) = (content.x, content.y);
                    // Only the rows that show (a page can be very long).
                    let (top, bottom) = self.clip.map_or((f64::MIN, f64::MAX), |c| (c.y - y0, c.y + c.h - y0));
                    for item in &laid.items {
                        let (from, to) = item.rows();
                        if to < top || from > bottom {
                            continue;
                        }
                        match item {
                            super::markdown::Item::Text { x, y, layout } => self.text(layout, x0 + x, y0 + y, None),
                            super::markdown::Item::Fill { rect, radius, color } => {
                                self.fill(Rect { x: x0 + rect.x, y: y0 + rect.y, ..*rect }, *radius, *color)
                            }
                            super::markdown::Item::Picture { rect, picture } => {
                                let s = rect.w / picture.width.max(1.0);
                                let t = self.t() * Affine::translate((x0 + rect.x, y0 + rect.y)) * Affine::scale(s);
                                self.scene.image(picture.clone(), t, 1.0);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        self.offset = (0.0, 0.0);
        // (A color picker's palette is drawn last, over everything: `paint_overlay`.)
        // (A collapsed ribbon group's items are drawn in its panel, when it's open.)
        let collapsed = matches!(n.kind, Kind::RibbonGroup { .. }) && n.layout.collapsed;
        if !collapsed && !matches!(n.kind, Kind::Picker { .. } | Kind::Scroll(_) | Kind::TabView { .. } | Kind::ColorPicker { .. } | Kind::Ribbon { .. } | Kind::RibbonItem { .. }) {
            for c in &n.children {
                self.paint(c);
            }
        }
        // The control the keyboard is on.
        if self.ui.focused == Some(n.key) && self.ui.focus_ring {
            let (ring, radius) = match &n.kind {
                Kind::Disclosure { .. } => (n.layout.segments.first().copied().unwrap_or(content), 5.0),
                Kind::Slider { .. } => (content.inset(2.0), 11.0),
                Kind::Table(_) => (content, 6.0),
                // (Tabs: the one that's selected.)
                Kind::TabView { .. } => (n.layout.tabs.get(n.selected_tab()).map_or(content, |t| t.rect.inset(4.0)), 3.0),
                _ => (content, 7.0),
            };
            self.stroke(ring.inset(-3.0), radius + 3.0, 2.0, theme().accent);
        }
        if n.mods.clipped {
            self.scene.pop();
        }
        if let Some((c, width, radius)) = n.mods.border {
            self.stroke(r, radius, width, c);
        }
        if layered {
            self.scene.pop();
        }
        self.offset = outer;
    }
}

/// Where a code editor's text starts (before scrolling).
pub fn code_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    (content.x + n.layout.gutter + CODE_PAD_X, content.y + CODE_PAD_Y)
}

/// Where a text field's text starts (before it's scrolled). A field of
/// several lines has its text at the top; one of one line, in the middle.
pub fn field_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    if matches!(n.kind, Kind::TextField { lines: 2.., .. } | Kind::RichEditor { .. }) {
        return (content.x + 8.0, content.y + 6.0);
    }
    let line_h = match (&n.layout.text, &n.layout.placeholder) {
        (Some(t), Some(p)) => p.height().max(t.height()),
        _ => 0.0,
    };
    let left = if matches!(n.kind, Kind::TextField { search: true, .. }) { super::layout::SEARCH_LEFT } else { 8.0 };
    (content.x + left, content.y + (content.h - line_h) / 2.0)
}

/// A video's controls: their bar along its bottom, the play (or pause)
/// button, and the track that shows (and sets) where it is.
pub fn video_parts(n: &Node) -> (Rect, Rect, Rect) {
    let c = n.layout.rect.inset_by(n.mods.padding);
    let h = 34.0f64.min(c.h);
    let bar = Rect { x: c.x, y: c.y + c.h - h, w: c.w, h };
    let button = Rect { x: bar.x + 6.0, y: bar.y + (h - 24.0) / 2.0, w: 24.0, h: 24.0 };
    let time = n.layout.text.as_ref().map_or(60.0, |t| t.width());
    let left = button.x + button.w + 10.0;
    let track = Rect { x: left, y: bar.y + h / 2.0 - 2.0, w: (bar.x + bar.w - time - 20.0 - left).max(0.0), h: 4.0 };
    (bar, button, track)
}

/// A search field's clear button, when it has text to clear.
pub fn search_clear(n: &Node) -> Option<Rect> {
    if !matches!(n.kind, Kind::TextField { search: true, .. }) || n.layout.empty {
        return None;
    }
    let c = n.layout.rect.inset_by(n.mods.padding);
    let size = 14.0;
    Some(Rect { x: c.x + c.w - super::layout::SEARCH_RIGHT + (super::layout::SEARCH_RIGHT - size) / 2.0 - 2.0, y: c.y + (c.h - size) / 2.0, w: size, h: size })
}

/// The icon of a collapsed ribbon group's button: its own, or its first
/// item's.
pub fn ribbon_group_icon(n: &Node, icon: &'static str) -> &'static str {
    if !icon.is_empty() {
        return icon;
    }
    n.children
        .iter()
        .find_map(|c| match c.kind {
            Kind::RibbonItem { icon, .. } if !icon.is_empty() => Some(icon),
            _ => None,
        })
        .unwrap_or("chevronDown")
}
