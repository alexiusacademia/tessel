//! Drawing: turns a laid-out tree into a vello scene, and renders scenes
//! with the GPU (to a window, or off-screen to a PNG for tests).

use std::collections::HashMap;
use std::num::NonZeroUsize;

use parley::{FontContext, GenericFamily, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty};
use vello::kurbo::{Affine, Circle, RoundedRect, Stroke};
use vello::peniko::{Color, Fill, Mix};
use vello::util::RenderContext;
use vello::{wgpu, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

use super::layout::Rect;
use super::theme::{self, theme};
use super::{Kind, Node, Rgba};

/// Text color carried through parley layouts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paint(pub [u8; 4]);

// A code editor's spacing, in points.
/// The height of a line of code (its text is 13 points): room to read.
const CODE_LINE: f32 = 20.0;
/// Left and right of the line numbers.
pub const GUTTER_PAD: (f64, f64) = (12.0, 10.0);
/// Between the line numbers and the code, and after the longest line.
pub const CODE_PAD_X: f64 = 12.0;
/// Above the first line and below the last.
pub const CODE_PAD_Y: f64 = 10.0;

pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<Paint>,
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { fonts: FontContext::new(), layouts: LayoutContext::new() }
    }

    /// Code: monospaced, not wrapped, with syntax colors.
    pub fn code_layout(&mut self, text: &str, size: f64) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(CODE_LINE)));
        b.push_default(StyleProperty::Brush(Paint(theme().colors[theme::PRIMARY].0)));
        for (range, color) in super::highlight::highlight(text) {
            b.push(StyleProperty::Brush(Paint(color.0)), range);
        }
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }

    /// Monospaced text, left-aligned, in one color.
    pub fn mono_line(&mut self, text: &str, size: f64, weight: f32, color: Rgba) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }

    /// Monospaced gray text (line numbers), on the lines of the code.
    pub fn mono_layout(&mut self, text: &str, size: f64, color: Rgba) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(CODE_LINE)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(parley::Alignment::End, parley::AlignmentOptions::default());
        layout
    }

    /// Text with stretches in other styles (bold, italic, code, links), for
    /// `Markdown`: wrapped at `max_w`, with room between its lines.
    pub fn rich(
        &mut self,
        text: &str,
        size: f64,
        weight: f32,
        color: Rgba,
        spans: &[(std::ops::Range<usize>, super::markdown::Inline)],
        max_w: Option<f64>,
    ) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::SystemUi));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.45)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        for (range, style) in spans {
            if style.bold {
                b.push(StyleProperty::FontWeight(parley::FontWeight::new(700.0)), range.clone());
            }
            if style.italic {
                b.push(StyleProperty::FontStyle(parley::FontStyle::Italic), range.clone());
            }
            if style.code {
                b.push(StyleProperty::from(GenericFamily::Monospace), range.clone());
                b.push(StyleProperty::FontSize(size as f32 * 0.92), range.clone());
                b.push(StyleProperty::Brush(Paint(theme().code.type_name.0)), range.clone());
            }
            if style.link.is_some() {
                b.push(StyleProperty::Brush(Paint(theme().accent.0)), range.clone());
            }
        }
        let mut layout = b.build(text);
        layout.break_all_lines(max_w.map(|w| w as f32 + 0.5));
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }

    /// A block of code in a `Markdown` view: monospaced, wrapped at `width`,
    /// with syntax colors if it's Tessel.
    pub fn code_block(&mut self, text: &str, size: f64, tessel: bool, width: f64) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.45)));
        b.push_default(StyleProperty::Brush(Paint(theme().colors[theme::PRIMARY].0)));
        if tessel {
            for (range, color) in super::highlight::highlight(text) {
                b.push(StyleProperty::Brush(Paint(color.0)), range);
            }
        }
        let mut layout = b.build(text);
        layout.break_all_lines(Some(width as f32 + 0.5));
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }

    pub fn layout(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::SystemUi));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        let mut layout = b.build(text);
        layout.break_all_lines(max_w.map(|w| w as f32 + 0.5));
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }
}

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
    /// The code editor's completion list, laid out, when one shows.
    pub popup: Option<Popup>,
    /// Text an input method is composing in the view being edited.
    pub composing: Option<Composing>,
    pub anim: super::anim::Animations,
    /// The `.tooltip` that shows, laid out.
    pub tooltip: Option<Tooltip>,
    /// The control the keyboard is on (a slider, stepper, color picker or
    /// disclosure group), when no text is edited.
    pub focused: Option<u64>,
    /// A view being dragged with `.draggable`.
    pub dragging: Option<Dragging>,
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
    pub layout: parley::Layout<Paint>,
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

/// The scroll bars of a view: (down the side, along the bottom). A scroll
/// view has the first; a code editor both.
pub fn scroll_bars(n: &Node, ui: &Interaction) -> (Option<Bar>, Option<Bar>) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    let down = ui.scroll.get(&n.key).copied().unwrap_or(0.0);
    match &n.kind {
        Kind::Scroll => (Bar::along(content, true, n.layout.content_h, down, 0.0), None),
        Kind::CodeEditor { .. } => {
            let code = Rect { x: content.x + n.layout.gutter, w: (content.w - n.layout.gutter).max(0.0), ..content };
            let across = ui.scroll_x.get(&n.key).copied().unwrap_or(0.0);
            let width = n.layout.text.as_ref().map_or(0.0, |t| t.width() as f64) + 2.0 * CODE_PAD_X;
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
    pub badge: parley::Layout<Paint>,
    pub badge_color: Rgba,
    pub label: parley::Layout<Paint>,
    pub detail: parley::Layout<Paint>,
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

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.0[0], c.0[1], c.0[2], c.0[3])
}

fn rounded(r: Rect, radius: f64) -> RoundedRect {
    RoundedRect::new(r.x, r.y, r.x + r.w, r.y + r.h, radius)
}

pub struct Painter<'a> {
    pub scene: &'a mut Scene,
    pub ui: &'a Interaction,
    /// Points to pixels.
    pub scale: f64,
    /// How far the view being drawn is from where layout put it (while it's
    /// animated to its place), in points.
    pub offset: (f64, f64),
    /// The part of the window that shows, inside scroll views: what's
    /// outside needn't be drawn.
    pub clip: Option<Rect>,
}

impl Painter<'_> {
    fn t(&self) -> Affine {
        Affine::scale(self.scale) * Affine::translate(self.offset)
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
    fn underline_composing(&mut self, n: &Node, layout: &parley::Layout<Paint>, x: f64, y: f64) {
        let Some((range, c)) = self.ui.composing_in(n.key) else { return };
        let mut span = super::edit::Editing::new(n.key, "");
        (span.anchor, span.focus) = (range.start, range.start + c.text.len());
        for r in super::edit::geometry(layout, &span).1 {
            let line = Rect { x: x + r.x0, y: y + r.y1 - 1.0, w: r.x1 - r.x0, h: 1.0 };
            self.fill(line, 0.0, n.layout.style.color);
        }
    }

    /// Draws `svg` centered in `r`, scaled to fit.
    fn draw_svg(&mut self, svg: &super::svg::Svg, r: Rect) {
        let s = (r.w / svg.width.max(1.0)).min(r.h / svg.height.max(1.0));
        let x = r.x + (r.w - svg.width * s) / 2.0;
        let y = r.y + (r.h - svg.height * s) / 2.0;
        let t = self.t() * Affine::translate((x, y)) * Affine::scale(s);
        self.scene.append(&svg.scene, Some(t));
    }

    /// Draws the built-in icon `name` in `r`.
    fn svg(&mut self, name: &str, r: Rect, c: Rgba) {
        if let Some(svg) = super::svg::icon(name, c) {
            self.draw_svg(&svg, r);
        }
    }

    fn fill(&mut self, r: Rect, radius: f64, c: Rgba) {
        let t = self.t();
        self.scene.fill(Fill::NonZero, t, color(c), None, &rounded(r, radius));
    }

    fn circle(&mut self, x: f64, y: f64, radius: f64, c: Rgba) {
        let t = self.t();
        self.scene.fill(Fill::NonZero, t, color(c), None, &Circle::new((x, y), radius));
    }

    fn stroke_circle(&mut self, x: f64, y: f64, radius: f64, width: f64, c: Rgba) {
        let t = self.t();
        self.scene.stroke(&Stroke::new(width), t, color(c), None, &Circle::new((x, y), radius - width / 2.0));
    }

    /// Draws the open `ColorPicker`'s palette over the rest of the window.
    pub fn paint_overlay(&mut self, tree: &Node) {
        let Some(key) = super::controls::OPEN_PICKER.get() else { return };
        let Some(palette) = tree.find(key).and_then(|n| n.children.first()) else { return };
        let r = palette.layout.rect;
        self.fill(Rect { x: r.x, y: r.y + 4.0, w: r.w, h: r.h }.inset(-3.0), 11.0, Rgba([0, 0, 0, 30]));
        self.paint(palette);
    }

    /// Draws the view being dragged (`.draggable`) under the pointer, see-through.
    pub fn paint_dragged(&mut self, tree: &Node) {
        let Some(d) = self.ui.dragging.as_ref() else { return };
        let Some(n) = tree.find(d.key) else { return };
        let r = n.layout.rect;
        let (dx, dy) = (d.at.0 - d.grab.0 - r.x, d.at.1 - d.grab.1 - r.y);
        let mut copy = Scene::new();
        let mut painter = Painter { scene: &mut copy, ui: self.ui, scale: self.scale, offset: (0.0, 0.0), clip: None };
        painter.paint(n);
        // On a card, lifted off the window.
        let t = self.t();
        let moved = Rect { x: r.x + dx, y: r.y + dy, ..r };
        self.scene.push_layer(Fill::NonZero, Mix::Normal, 0.8, t, &rounded(moved.inset(-8.0), 0.0));
        self.fill(Rect { x: moved.x, y: moved.y + 3.0, ..moved }.inset(-2.0), 7.0, Rgba([0, 0, 0, 30]));
        self.fill(moved, 6.0, theme().popup);
        self.stroke(moved, 6.0, 1.0, theme().field_border);
        self.scene.append(&copy, Some(Affine::translate((dx * self.scale, dy * self.scale))));
        self.scene.pop_layer();
    }

    fn stroke(&mut self, r: Rect, radius: f64, width: f64, c: Rgba) {
        let t = self.t();
        let inner = r.inset(width / 2.0);
        self.scene.stroke(&Stroke::new(width), t, color(c), None, &rounded(inner, radius));
    }

    fn text(&mut self, layout: &parley::Layout<Paint>, x: f64, y: f64, tint: Option<Rgba>) {
        let t = self.t() * Affine::translate((x, y));
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let brush = tint.map(|c| c.0).unwrap_or(run.style().brush.0);
                let r = run.run();
                self.scene
                    .draw_glyphs(r.font())
                    .brush(Color::from_rgba8(brush[0], brush[1], brush[2], brush[3]))
                    .hint(false)
                    .transform(t)
                    .font_size(r.font_size())
                    .normalized_coords(r.normalized_coords())
                    .draw(
                        Fill::NonZero,
                        run.positioned_glyphs().map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y }),
                    );
            }
        }
    }

    /// Draws the completion list, if one shows (after the tree, so it's on top).
    pub fn paint_popup(&mut self) {
        let Some(p) = self.ui.popup.as_ref() else { return };
        let r = p.rect;
        let shadow = Rect { x: r.x, y: r.y + 3.0, w: r.w, h: r.h };
        self.fill(shadow.inset(-2.0), 8.0, Rgba([0, 0, 0, 22]));
        self.fill(r, 6.0, theme().popup);
        let t = self.t();
        self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &rounded(r, 6.0));
        for (i, row) in p.rows.iter().enumerate() {
            let y = r.y + 4.0 + i as f64 * p.row_h;
            if p.selected == Some(i) {
                self.fill(Rect { x: r.x + 4.0, y, w: r.w - 8.0, h: p.row_h }, 4.0, theme().selection);
            }
            let badge = Rect { x: r.x + 8.0, y: y + (p.row_h - 16.0) / 2.0, w: 16.0, h: 16.0 };
            self.fill(badge, 4.0, row.badge_color);
            let (bw, bh) = (row.badge.width() as f64, row.badge.height() as f64);
            self.text(&row.badge, badge.x + (16.0 - bw) / 2.0, badge.y + (16.0 - bh) / 2.0, None);
            let ty = y + (p.row_h - row.label.height() as f64) / 2.0;
            self.text(&row.label, r.x + 32.0, ty, None);
            let dx = (r.x + r.w - 10.0 - row.detail.width() as f64).max(r.x + 44.0 + row.label.width() as f64);
            let dy = y + (p.row_h - row.detail.height() as f64) / 2.0;
            self.text(&row.detail, dx, dy, None);
        }
        self.scene.pop_layer();
        self.stroke(r, 6.0, 1.0, theme().field_border);
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
        if n.mods.hidden {
            return;
        }
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
            self.scene.push_layer(Fill::NonZero, Mix::Normal, alpha as f32, t, &rounded(r, 0.0));
        }
        if let Some((c, radius)) = shown.map_or(n.mods.background, |s| s.background) {
            self.fill(r, radius, c);
        }
        // `.clipped()`: what's in the view stays inside its frame (with the
        // corners of its background or border).
        if n.mods.clipped {
            let radius = n.mods.background.map(|b| b.1).or(n.mods.border.map(|b| b.2)).unwrap_or(0.0);
            let t = self.t();
            self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &rounded(r, radius));
        }
        if let (Some((c, radius)), true) = (n.mods.hover_background, self.ui.hover_background == Some(n.key)) {
            self.fill(r, radius, c);
        }
        self.offset = (r.x - placed.x, r.y - placed.y);
        let content = placed.inset_by(n.mods.padding);
        let hovered = self.ui.hovered == Some(n.key) && !style.disabled;
        let pressed = self.ui.pressed == Some(n.key) && !style.disabled;
        match &n.kind {
            Kind::Text(_) => {
                if let Some(t) = &n.layout.text {
                    let c = n.layout.content;
                    self.text(t, c.x, c.y, None);
                }
            }
            Kind::Button { .. } => {
                if n.mods.background.is_none() {
                    let bg = if pressed { theme().control_pressed } else if hovered { theme().control_hover } else { theme().control };
                    self.fill(content, 7.0, bg);
                }
                if let Some(t) = &n.layout.text {
                    let x = content.x + (content.w - t.width() as f64) / 2.0;
                    let y = content.y + (content.h - t.height() as f64) / 2.0;
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
                if let (Some(t), Some(ph)) = (&n.layout.text, &n.layout.placeholder) {
                    let (x0, y0) = field_origin(n);
                    let line_h = (ph.height() as f64).max(t.height() as f64);
                    let t_scene = self.t();
                    self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(content.inset(2.0), 4.0));
                    let scroll = editing.map_or(0.0, |e| e.scroll);
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
                    self.scene.pop_layer();
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
                self.scene.fill(Fill::NonZero, t, color(Rgba([255, 255, 255, 255])), None, &Circle::new((knob_x, sw.y + 11.0), 9.0));
                if let Some(label) = &n.layout.text {
                    self.text(label, content.x, content.y + (content.h - label.height() as f64) / 2.0, None);
                }
            }
            Kind::Picker { selection } => {
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
            Kind::CodeEditor { error_lines, .. } => {
                let editing = self.ui.shown_editing(n.key);
                let editing = editing.as_ref();
                let t_scene = self.t();
                self.fill(content, 6.0, theme().field);
                self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(content, 6.0));
                let gutter = Rect { x: content.x, y: content.y, w: n.layout.gutter, h: content.h };
                self.fill(gutter, 0.0, theme().gutter);
                let (x0, y0) = code_origin(n);
                let scroll_y = self.ui.scroll.get(&n.key).copied().unwrap_or(0.0);
                let scroll_x = self.ui.scroll_x.get(&n.key).copied().unwrap_or(0.0);
                if let Some(t) = &n.layout.text {
                    let lines: Vec<(f64, f64)> = t
                        .lines()
                        .map(|l| (l.metrics().block_min_coord as f64, l.metrics().block_max_coord as f64))
                        .collect();
                    for &line in error_lines {
                        let Some(&(top, bottom)) = lines.get((line - 1).max(0) as usize) else { continue };
                        let y = y0 - scroll_y + top;
                        let band = Rect { x: content.x + n.layout.gutter, y, w: content.w - n.layout.gutter, h: bottom - top };
                        self.fill(band, 0.0, theme().error_line);
                        self.fill(Rect { x: content.x, y, w: 3.0, h: bottom - top }, 0.0, theme().error_mark);
                    }
                    if let Some(numbers) = &n.layout.placeholder {
                        self.text(numbers, content.x + GUTTER_PAD.0, y0 - scroll_y, None);
                    }
                    let code_area = Rect { x: content.x + n.layout.gutter, y: content.y, w: content.w - n.layout.gutter, h: content.h };
                    self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(code_area, 0.0));
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
                    self.scene.pop_layer();
                }
                self.scene.pop_layer();
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
                    let ty = r.y + (r.h - tab.title.height() as f64) / 2.0;
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
            Kind::Divider => {
                self.fill(placed, 0.0, theme().colors[theme::SEPARATOR]);
            }
            Kind::Svg { source, .. } => {
                if let Some(svg) = super::svg::load(source, style.color) {
                    self.draw_svg(&svg, n.layout.content);
                }
            }
            Kind::Image { picture: Some(p), .. } => {
                let c = n.layout.content;
                let s = (c.w / p.width.max(1.0)).min(c.h / p.height.max(1.0));
                let t = self.t() * Affine::translate((c.x, c.y)) * Affine::scale(s);
                self.scene.draw_image(&p.image, t);
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
                self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &rounded(content, 0.0));
                self.scene.append(&drawing.scene, Some(t * Affine::translate((content.x, content.y))));
                self.scene.pop_layer();
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
            Kind::Scroll => {
                let t = self.t();
                self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &rounded(content, 0.0));
                let outer = self.clip.replace(content);
                for c in &n.children {
                    self.paint(c);
                }
                self.clip = outer;
                self.scene.pop_layer();
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
                                self.scene.draw_image(&picture.image, t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        self.offset = (0.0, 0.0);
        // (A color picker's palette is drawn last, over everything: `paint_overlay`.)
        if !matches!(n.kind, Kind::Picker { .. } | Kind::Scroll | Kind::TabView { .. } | Kind::ColorPicker { .. }) {
            for c in &n.children {
                self.paint(c);
            }
        }
        // The control the keyboard is on.
        if self.ui.focused == Some(n.key) {
            let (ring, radius) = match &n.kind {
                Kind::Disclosure { .. } => (n.layout.segments.first().copied().unwrap_or(content), 5.0),
                Kind::Slider { .. } => (content.inset(2.0), 11.0),
                _ => (content, 7.0),
            };
            self.stroke(ring.inset(-3.0), radius + 3.0, 2.0, theme().accent);
        }
        if n.mods.clipped {
            self.scene.pop_layer();
        }
        if let Some((c, width, radius)) = n.mods.border {
            self.stroke(r, radius, width, c);
        }
        if layered {
            self.scene.pop_layer();
        }
        self.offset = outer;
    }
}

/// Where a code editor's text starts (before scrolling).
pub fn code_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    (content.x + n.layout.gutter + CODE_PAD_X, content.y + CODE_PAD_Y)
}

/// Where a text field's text starts (before horizontal scrolling).
pub fn field_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    let line_h = match (&n.layout.text, &n.layout.placeholder) {
        (Some(t), Some(p)) => (p.height() as f64).max(t.height() as f64),
        _ => 0.0,
    };
    (content.x + 8.0, content.y + (content.h - line_h) / 2.0)
}

// ===== GPU =====

pub fn renderer(device: &wgpu::Device) -> Renderer {
    Renderer::new(
        device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )
    .expect("can't create the renderer")
}

pub fn params(width: u32, height: u32) -> RenderParams {
    RenderParams { base_color: color(theme().colors[theme::BACKGROUND]), width, height, antialiasing_method: AaConfig::Area }
}

/// Renders a scene off-screen and saves it as a PNG.
pub fn render_png(cx: &mut RenderContext, renderer: &mut Option<Renderer>, scene: &Scene, width: u32, height: u32, path: &str) {
    let pixels = render_pixels(cx, renderer, scene, width, height, color(theme().colors[theme::BACKGROUND]));
    let file = std::fs::File::create(path).expect("can't create snapshot file");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&pixels)).expect("can't write snapshot");
}

/// Renders a scene off-screen, over `base`: its pixels, four bytes each
/// (red, green, blue, opacity), row by row.
pub fn render_pixels(cx: &mut RenderContext, renderer: &mut Option<Renderer>, scene: &Scene, width: u32, height: u32, base: Color) -> Vec<u8> {
    let dev = pollster::block_on(cx.device(None)).expect("no GPU device");
    let handle = &cx.devices[dev];
    let (device, queue) = (&handle.device, &handle.queue);
    let renderer = renderer.get_or_insert_with(|| self::renderer(device));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let params = RenderParams { base_color: base, width, height, antialiasing_method: AaConfig::Area };
    renderer.render_to_texture(device, queue, scene, &view, &params).expect("render failed");
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    let data = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    pixels
}
