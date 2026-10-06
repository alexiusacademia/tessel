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

/// A layout as parley made it.
pub type Shaped = parley::Layout<Paint>;

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
    TextLayout { width: shaped.width() as f64, advance, height: shaped.height() as f64, lines, len: text.len(), shaped }
}

