//! Laying text out with parley, for the GPU renderer: the system's fonts,
//! shaped and broken into lines, and drawn by vello (see `gpu::play`).

use std::ops::Range;

use parley::{FontContext, GenericFamily, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty};

use crate::ui::text::{Face, Family, Line, TextLayout, CODE_LINE};
use crate::ui::theme::{self, theme};
use crate::ui::Rgba;

/// Text color carried through parley layouts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paint(pub [u8; 4]);

/// A layout as parley made it, with the text it's of (which a PDF says
/// its glyphs show).
pub struct Shaped {
    layout: parley::Layout<Paint>,
    text: String,
}

impl std::ops::Deref for Shaped {
    type Target = parley::Layout<Paint>;

    fn deref(&self) -> &parley::Layout<Paint> {
        &self.layout
    }
}

/// Lays text out with parley. Each layout takes the fonts the system has.
pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<Paint>,
}

/// How a layout's lines line up: 0 leading, 1 center, 2 trailing.
fn alignment(align: u8) -> parley::Alignment {
    match align {
        1 => parley::Alignment::Center,
        2 => parley::Alignment::End,
        _ => parley::Alignment::Start,
    }
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { fonts: FontContext::new(), layouts: LayoutContext::new() }
    }

    /// Code: monospaced, not wrapped, with syntax colors.
    pub fn code_layout(&mut self, text: &str, size: f64) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(CODE_LINE)));
        b.push_default(StyleProperty::Brush(Paint(theme().colors[theme::PRIMARY].0)));
        for (range, color) in crate::ui::highlight::highlight(text) {
            b.push(StyleProperty::Brush(Paint(color.0)), range);
        }
        finish(b.build(text), text, None, 0)
    }

    /// Monospaced text, left-aligned, in one color.
    pub fn mono_line(&mut self, text: &str, size: f64, weight: f32, color: Rgba) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        finish(b.build(text), text, None, 0)
    }

    /// Monospaced gray text (line numbers), on the lines of the code, right-aligned.
    pub fn mono_layout(&mut self, text: &str, size: f64, color: Rgba) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::Absolute(CODE_LINE)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        finish(b.build(text), text, None, 2)
    }

    /// Text with stretches in other styles (bold, italic, code, links), for
    /// `Markdown`: wrapped at `max_w`, with room between its lines.
    pub fn rich(
        &mut self,
        text: &str,
        size: f64,
        weight: f32,
        color: Rgba,
        spans: &[(Range<usize>, crate::ui::markdown::Inline)],
        max_w: Option<f64>,
    ) -> TextLayout {
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
        finish(b.build(text), text, max_w, 0)
    }

    /// A block of code in a `Markdown` view: monospaced, wrapped at `width`,
    /// with syntax colors if it's Tessel.
    pub fn code_block(&mut self, text: &str, size: f64, tessel: bool, width: f64) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.45)));
        b.push_default(StyleProperty::Brush(Paint(theme().colors[theme::PRIMARY].0)));
        if tessel {
            for (range, color) in crate::ui::highlight::highlight(text) {
                b.push(StyleProperty::Brush(Paint(color.0)), range);
            }
        }
        finish(b.build(text), text, Some(width), 0)
    }

    pub fn layout(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>) -> TextLayout {
        self.aligned(text, size, weight, color, max_w, 0)
    }

    /// Like `layout`, with the lines lined up (0 leading, 1 center, 2
    /// trailing) inside the width of the widest.
    pub fn aligned(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>, align: u8) -> TextLayout {
        self.styled(text, size, weight, Face::default(), color, max_w, align)
    }

    /// Text with stretches in styles of their own (a `RichTextEditor`'s),
    /// wrapped at `max_w`.
    pub fn runs(&mut self, text: &str, base: &crate::ui::text::Styled, runs: &[crate::ui::text::Styled], max_w: Option<f64>) -> TextLayout {
        fn family(f: Family) -> StyleProperty<'static, Paint> {
            match f {
                Family::System => StyleProperty::from(GenericFamily::SystemUi),
                Family::Mono => StyleProperty::from(GenericFamily::Monospace),
                named => {
                    let name = named.name().unwrap_or_default().replace('"', "");
                    StyleProperty::FontFamily(parley::FontFamily::Source(format!("\"{name}\", system-ui").into()))
                }
            }
        }
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(family(base.family));
        b.push_default(StyleProperty::FontSize(base.size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(base.weight)));
        b.push_default(StyleProperty::Brush(Paint(base.color.0)));
        for r in runs {
            if r.family != base.family {
                b.push(family(r.family), r.range.clone());
            }
            b.push(StyleProperty::FontSize(r.size as f32), r.range.clone());
            b.push(StyleProperty::FontWeight(parley::FontWeight::new(r.weight)), r.range.clone());
            if r.italic {
                b.push(StyleProperty::FontStyle(parley::FontStyle::Italic), r.range.clone());
            }
            b.push(StyleProperty::Brush(Paint(r.color.0)), r.range.clone());
        }
        finish(b.build(text), text, max_w, 0)
    }

    /// Text in a family and style of its own (`g.text`, a `Text` with
    /// `.font(family:italic:)`).
    #[allow(clippy::too_many_arguments)]
    pub fn styled(&mut self, text: &str, size: f64, weight: f32, face: Face, color: Rgba, max_w: Option<f64>, align: u8) -> TextLayout {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        match face.family {
            Family::System => b.push_default(StyleProperty::from(GenericFamily::SystemUi)),
            Family::Mono => b.push_default(StyleProperty::from(GenericFamily::Monospace)),
            named => {
                // (The system's font when the family isn't installed.)
                let name = named.name().unwrap_or_default().replace('"', "");
                b.push_default(StyleProperty::FontFamily(parley::FontFamily::Source(format!("\"{name}\", system-ui").into())));
            }
        }
        if face.italic {
            b.push_default(StyleProperty::FontStyle(parley::FontStyle::Italic));
        }
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        if face.no_kerning {
            b.push_default(StyleProperty::FontFeatures(parley::FontFeatures::Source("\"kern\" 0".into())));
        }
        if face.tracking != 0 {
            b.push_default(StyleProperty::LetterSpacing(face.tracking() as f32));
        }
        finish(b.build(text), text, max_w, align)
    }
}

/// The font families installed.
pub fn families() -> Vec<String> {
    let mut fonts = FontContext::new();
    fonts.collection.family_names().map(str::to_string).collect()
}

/// Breaks a parley layout into lines (at `max_w`), aligns them, and finds
/// where the caret can stand.
fn finish(mut shaped: parley::Layout<Paint>, text: &str, max_w: Option<f64>, align: u8) -> TextLayout {
    shaped.break_all_lines(max_w.map(|w| w as f32 + 0.5));
    if align != 0 {
        // Parley aligns within the width lines were broken at; use the
        // widest line's, so the lines line up inside the text's own box.
        let w = shaped.width();
        shaped.break_all_lines(Some(w + 0.5));
    }
    shaped.align(alignment(align), parley::AlignmentOptions::default());
    let mut lines = Vec::new();
    for line in shaped.lines() {
        let m = line.metrics();
        let range = line.text_range();
        let mut stops = Vec::new();
        let mut end_x = m.offset as f64;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let mut x = run.offset() as f64;
            for cluster in run.run().visual_clusters() {
                let r = cluster.text_range();
                if r.start < range.start || r.start >= range.end {
                    continue;
                }
                stops.push((r.start, x));
                x += cluster.advance() as f64;
                // (The line break itself takes no room.)
                if !text[r.clone()].ends_with('\n') {
                    end_x = end_x.max(x);
                }
            }
        }
        stops.sort_by_key(|s| s.0);
        stops.dedup_by_key(|s| s.0);
        if stops.is_empty() {
            stops.push((range.start, m.offset as f64));
        }
        lines.push(Line { range, top: m.block_min_coord as f64, bottom: m.block_max_coord as f64, stops, end_x });
    }
    if lines.is_empty() {
        lines.push(Line { range: 0..0, top: 0.0, bottom: shaped.height() as f64, stops: vec![(0, 0.0)], end_x: 0.0 });
    }
    let advance = shaped.lines().map(|l| l.metrics().advance as f64).fold(shaped.width() as f64, f64::max);
    let (width, height) = (shaped.width() as f64, shaped.height() as f64);
    TextLayout { width, advance, height, lines, len: text.len(), shaped: Shaped { layout: shaped, text: text.to_string() } }
}


// ===== PDFs =====

/// What `ui::pdf` asks of the renderer: the glyphs a layout is drawn with,
/// the font files they're in, and pictures' pixels.
#[derive(Default)]
pub struct PdfSource {
    /// Font files met so far, by where their bytes are and their index.
    fonts: std::cell::RefCell<std::collections::HashMap<(u64, u32), std::rc::Rc<crate::ui::pdf::FontFile>>>,
}

impl crate::ui::pdf::Source for PdfSource {
    fn glyphs(&self, layout: &TextLayout) -> Vec<crate::ui::pdf::PlacedGlyph> {
        let mut out = Vec::new();
        for line in layout.shaped.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let r = run.run();
                let font = r.font();
                let key = (font.data.id(), font.index);
                let file = self
                    .fonts
                    .borrow_mut()
                    .entry(key)
                    .or_insert_with(|| {
                        std::rc::Rc::new(crate::ui::pdf::FontFile {
                            key: format!("{}:{}", key.0, key.1),
                            data: std::rc::Rc::new(font.data.data().to_vec()),
                            index: font.index,
                        })
                    })
                    .clone();
                // (A font with no italic of its own is slanted for one.)
                let slant = r.synthesis().skew().map_or(0.0, |degrees| degrees.to_radians().tan());
                // The text each glyph shows: the first glyph of a cluster
                // gets its characters.
                let mut shows = Vec::new();
                for cluster in r.visual_clusters() {
                    let text = layout.shaped.text.get(cluster.text_range()).unwrap_or_default();
                    for (i, _) in cluster.glyphs().enumerate() {
                        shows.push(if i == 0 { text } else { "" });
                    }
                }
                let color = Rgba(run.style().brush.0);
                for (i, g) in run.positioned_glyphs().enumerate() {
                    out.push(crate::ui::pdf::PlacedGlyph {
                        font: file.clone(),
                        id: g.id as u16,
                        size: r.font_size(),
                        x: g.x,
                        y: g.y,
                        advance: g.advance,
                        text: shows.get(i).copied().unwrap_or_default().trim_end_matches(['\n', '\r']).to_string(),
                        color,
                        slant,
                    });
                }
            }
        }
        out
    }

    /// The fonts of a phone are mostly ones that vary (one file for every
    /// weight and slant), which a PDF can't hold as they are: a layout that
    /// uses one, or a font slanted or thickened to stand in for a style it
    /// lacks, is drawn as its glyphs' shapes.
    fn outlines(&self, layout: &TextLayout) -> Option<Vec<(kurbo::BezPath, Rgba)>> {
        use skrifa::MetadataProvider;
        let plain = layout.shaped.lines().flat_map(|l| l.items()).all(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => {
                let r = run.run();
                r.normalized_coords().iter().all(|c| *c == 0)
                    && r.synthesis().skew().is_none()
                    && !r.synthesis().embolden()
                    && skrifa::FontRef::from_index(r.font().data.data(), r.font().index).is_ok_and(|f| f.axes().is_empty())
            }
            _ => true,
        });
        if plain {
            return None;
        }
        Some(shapes(layout))
    }

    fn pixels(&self, picture: &crate::ui::image::Picture) -> Option<(u32, u32, Vec<u8>)> {
        let image = &picture.image;
        Some((image.width, image.height, image.data.data().to_vec()))
    }
}

/// A layout's glyphs as shapes, a run (in its color) at a time, in the
/// layout's coordinates.
fn shapes(layout: &TextLayout) -> Vec<(kurbo::BezPath, Rgba)> {
        use skrifa::MetadataProvider;
        let mut shapes = Vec::new();
        for line in layout.shaped.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let r = run.run();
                let Ok(font) = skrifa::FontRef::from_index(r.font().data.data(), r.font().index) else { continue };
                let glyphs = font.outline_glyphs();
                let coords: Vec<skrifa::instance::NormalizedCoord> =
                    r.normalized_coords().iter().map(|c| skrifa::instance::NormalizedCoord::from_bits(*c)).collect();
                let slant = r.synthesis().skew().map_or(0.0, |degrees| (degrees as f64).to_radians().tan());
                let mut pen = Pen { path: kurbo::BezPath::new(), x: 0.0, y: 0.0, slant };
                for g in run.positioned_glyphs() {
                    let Some(glyph) = glyphs.get(skrifa::GlyphId::new(g.id as u32)) else { continue };
                    (pen.x, pen.y) = (g.x as f64, g.y as f64);
                    let how = skrifa::outline::DrawSettings::unhinted(skrifa::instance::Size::new(r.font_size()), skrifa::instance::LocationRef::new(&coords));
                    let _ = glyph.draw(how, &mut pen);
                }
                if !pen.path.elements().is_empty() {
                    shapes.push((pen.path, Rgba(run.style().brush.0)));
                }
            }
        }
        shapes
}

// ===== glyphs, outlines, metrics and styles =====

/// The outline of a layout's glyphs, in its coordinates (y down from its
/// top): the font's own curves, at the font's weight and slant.
pub fn outline(layout: &TextLayout) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    for (shape, _) in shapes(layout) {
        path.extend(shape);
    }
    path
}

/// The glyphs of a layout, as they're placed (see `PlacedGlyph`).
pub fn glyphs(layout: &TextLayout) -> Vec<crate::ui::text::PlacedGlyph> {
    let text = &layout.shaped.text;
    let mut out = Vec::new();
    for line in layout.shaped.lines() {
        // Each glyph's place in the text: its cluster's start. A ligature's
        // glyph is in its first cluster; the clusters after it have none, so
        // it shows their text too (see `cluster_ranges`).
        let (mut at, mut placed) = (Vec::new(), Vec::new());
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let mut starts = Vec::new();
            for cluster in run.run().visual_clusters() {
                for _ in cluster.glyphs() {
                    starts.push(cluster.text_range().start);
                }
            }
            for (i, g) in run.positioned_glyphs().enumerate() {
                at.push(starts.get(i).copied().unwrap_or(0));
                placed.push((g.x as f64, g.y as f64, g.advance as f64));
            }
        }
        // (The line break at the end of a line is no glyph's.)
        let mut end = line.text_range().end.min(text.len());
        while end > 0 && matches!(text.as_bytes()[end - 1], b'\n' | b'\r') {
            end -= 1;
        }
        for ((x, y, advance), text) in placed.into_iter().zip(crate::ui::text::cluster_ranges(&at, end)) {
            out.push(crate::ui::text::PlacedGlyph { x, y, advance, text });
        }
    }
    out
}

/// The metrics of the font a layout's text starts in, at its size.
pub fn metrics(layout: &TextLayout) -> Option<crate::ui::text::FontMetrics> {
    use skrifa::MetadataProvider;
    let run = layout.shaped.lines().flat_map(|l| l.items()).find_map(|item| match item {
        PositionedLayoutItem::GlyphRun(run) => Some(run),
        _ => None,
    })?;
    let r = run.run();
    let font = skrifa::FontRef::from_index(r.font().data.data(), r.font().index).ok()?;
    let coords: Vec<skrifa::instance::NormalizedCoord> =
        r.normalized_coords().iter().map(|c| skrifa::instance::NormalizedCoord::from_bits(*c)).collect();
    let location = skrifa::instance::LocationRef::new(&coords);
    let size = skrifa::instance::Size::new(r.font_size());
    let m = font.metrics(size, location);
    // A font that doesn't say how tall its letters are: as tall as its H
    // and x are drawn.
    let height_of = |c: char| -> Option<f64> {
        let id = font.charmap().map(c)?;
        let glyph = font.outline_glyphs().get(id)?;
        let mut pen = Pen { path: kurbo::BezPath::new(), x: 0.0, y: 0.0, slant: 0.0 };
        glyph.draw(skrifa::outline::DrawSettings::unhinted(size, location), &mut pen).ok()?;
        Some(-kurbo::Shape::bounding_box(&pen.path).y0)
    };
    Some(crate::ui::text::FontMetrics {
        ascent: m.ascent as f64,
        descent: (m.descent as f64).abs(),
        line_gap: m.leading as f64,
        cap_height: m.cap_height.map(|h| h as f64).or_else(|| height_of('H')).unwrap_or(0.0),
        x_height: m.x_height.map(|h| h as f64).or_else(|| height_of('x')).unwrap_or(0.0),
    })
}

/// The faces of the family `name`, lightest first.
pub fn styles(name: &str) -> Vec<crate::ui::text::FontStyle> {
    use skrifa::MetadataProvider;
    let mut fonts = FontContext::new();
    let Some(family) = fonts.collection.family_by_name(name.trim()) else { return Vec::new() };
    let mut out: Vec<crate::ui::text::FontStyle> = Vec::new();
    for info in family.fonts() {
        let italic = !matches!(info.style(), parley::fontique::FontStyle::Normal);
        let weight = info.weight().value();
        // Its name in the family, from the font itself.
        let named = info.load(None).and_then(|blob| {
            let font = skrifa::FontRef::from_index(blob.data(), info.index()).ok()?;
            let strings = font.localized_strings(skrifa::string::StringId::TYPOGRAPHIC_SUBFAMILY_NAME);
            let pick = |s: skrifa::string::LocalizedStrings| s.english_or_first().map(|s| s.to_string());
            pick(strings).or_else(|| pick(font.localized_strings(skrifa::string::StringId::SUBFAMILY_NAME)))
        });
        let name = named.unwrap_or_else(|| if italic { "Italic".into() } else { "Regular".into() });
        if !out.iter().any(|o| o.name == name) {
            out.push(crate::ui::text::FontStyle { name, weight, italic });
        }
    }
    out.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)));
    out
}

/// Takes a glyph's outline (which has y going up, from the glyph's origin)
/// into a path of the layout's.
struct Pen {
    path: kurbo::BezPath,
    x: f64,
    y: f64,
    slant: f64,
}

impl Pen {
    fn at(&self, x: f32, y: f32) -> kurbo::Point {
        kurbo::Point::new(self.x + x as f64 + self.slant * y as f64, self.y - y as f64)
    }
}

impl skrifa::outline::OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.at(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.at(x, y));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.quad_to(self.at(cx, cy), self.at(x, y));
    }

    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        self.path.curve_to(self.at(ax, ay), self.at(bx, by), self.at(x, y));
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}
