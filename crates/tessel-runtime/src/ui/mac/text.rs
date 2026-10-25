//! Laying text out with Core Text, for the macOS renderer: the system font
//! (San Francisco) and its monospaced one, shaped and broken into lines by
//! macOS, and drawn by it too (see `mac::play`).

use std::collections::HashMap;
use std::ffi::c_void;
use std::ops::Range;

use super::ffi::*;
use crate::ui::text::{Face, Family, Line, TextLayout, CODE_LINE};
use crate::ui::theme::{self, theme};
use crate::ui::Rgba;

/// A font: the system's, its monospaced one, or one of a family by name.
#[derive(Clone, Copy, PartialEq)]
struct Font {
    family: Family,
    size: f64,
    weight: f32,
    italic: bool,
}

impl Font {
    fn key(&self) -> (Family, u64, u32, bool) {
        (self.family, self.size.to_bits(), self.weight.to_bits(), self.italic)
    }
}

/// What a stretch of the text looks like, where it isn't as the rest.
struct Span {
    range: Range<usize>,
    font: Option<Font>,
    color: Option<Rgba>,
}

/// The ascent, descent and leading of a line from `start` to `end` (UTF-16
/// units): the largest of the fonts asked for in it, the spans' and (where
/// they don't cover it) the base font's.
fn asked_metrics(start: usize, end: usize, base: CTFontRef, spans: &[(usize, usize, CTFontRef)]) -> (f64, f64, f64) {
    let metrics = |f: CTFontRef| unsafe { (CTFontGetAscent(f), CTFontGetDescent(f), CTFontGetLeading(f)) };
    let mut over: Vec<(usize, usize, CTFontRef)> =
        spans.iter().filter(|(from, to, _)| *from < end.max(start + 1) && *to > start).copied().collect();
    over.sort_by_key(|s| s.0);
    // Whether the spans cover the whole line.
    let mut reach = start;
    for (from, to, _) in &over {
        if *from > reach {
            break;
        }
        reach = reach.max(*to);
    }
    let mut fonts: Vec<CTFontRef> = over.iter().map(|s| s.2).collect();
    if reach < end || over.is_empty() {
        fonts.push(base);
    }
    fonts.into_iter().map(metrics).fold((0.0, 0.0, 0.0), |a, m| (a.0.max(m.0), a.1.max(m.1), a.2.max(m.2)))
}

/// How tall lines are.
#[derive(Clone, Copy)]
enum LineHeight {
    /// As the font says.
    Natural,
    /// This many points.
    Absolute(f64),
    /// This many times the font's size.
    Relative(f64),
}

/// A line as Core Text made it, and where it's drawn: its left end and baseline.
pub struct ShapedLine {
    pub line: Option<Owned>,
    pub x: f64,
    pub baseline: f64,
}

/// A layout's lines, ready to draw, and the text they're of.
pub struct Shaped {
    pub lines: Vec<ShapedLine>,
    pub text: String,
}

pub struct TextEngine {
    fonts: HashMap<(Family, u64, u32, bool), Owned>,
    /// The attribute that holds each stretch's color (Core Text is told to
    /// take the color from the context, so it can be drawn in another one).
    color_key: Owned,
    /// For the layout being built: the font's kerning left out, and room
    /// added between letters (see `styled`).
    spacing: (bool, f64),
}

/// The weight trait Core Text uses, for a CSS weight (400 regular, 700 bold).
fn weight_trait(weight: f32) -> f64 {
    match weight {
        // (Thin, ultra light and light, as AppKit's weights are.)
        w if w <= 100.0 => -0.8,
        w if w <= 200.0 => -0.8 + 0.2 * (w as f64 - 100.0) / 100.0,
        w if w <= 300.0 => -0.6 + 0.2 * (w as f64 - 200.0) / 100.0,
        w if w < 400.0 => -0.4 + 0.4 * (w as f64 - 300.0) / 100.0,
        w if w <= 400.0 => 0.0,
        w if w <= 500.0 => 0.23 * (w as f64 - 400.0) / 100.0,
        w if w <= 600.0 => 0.23 + 0.07 * (w as f64 - 500.0) / 100.0,
        w if w <= 700.0 => 0.3 + 0.1 * (w as f64 - 600.0) / 100.0,
        // (Heavy and black.)
        w if w <= 800.0 => 0.4 + 0.16 * (w as f64 - 700.0) / 100.0,
        w => 0.56 + 0.06 * ((w as f64 - 800.0) / 100.0).min(1.0),
    }
}

fn packed(c: Rgba) -> i32 {
    i32::from_be_bytes(c.0)
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { fonts: HashMap::new(), color_key: cfstring("TesselColor"), spacing: (false, 0.0) }
    }

    fn font(&mut self, f: Font) -> CTFontRef {
        if let Some(font) = self.fonts.get(&f.key()) {
            return font.get();
        }
        let font = unsafe {
            if let Some(font) = f.family.name().and_then(|name| named(&name, f)) {
                let r = font.get();
                self.fonts.insert(f.key(), font);
                return r;
            }
            let kind = if f.family == Family::Mono { kCTFontUIFontUserFixedPitch } else { kCTFontUIFontSystem };
            let mut font = Owned(CTFontCreateUIFontForLanguage(kind, f.size, std::ptr::null()));
            // The monospaced font (Menlo) in its weight, by its family: a copy
            // of its description with another weight keeps its regular face.
            if f.family == Family::Mono && (f.weight != 400.0 || f.italic) {
                let family = Owned::new(CTFontCopyFamilyName(font.get())).map(|n| string_of(n.get()));
                if let Some(styled) = family.and_then(|name| named(&name, f)) {
                    let r = styled.get();
                    self.fonts.insert(f.key(), styled);
                    return r;
                }
            }
            if f.weight != 400.0 {
                let w = cfnumber_f64(weight_trait(f.weight));
                let traits = cfdict(&[(kCTFontWeightTrait, w.get())]);
                let attrs = cfdict(&[(kCTFontTraitsAttribute, traits.get())]);
                let d = Owned::new(CTFontCopyFontDescriptor(font.get()));
                let d2 = d.and_then(|d| Owned::new(CTFontDescriptorCreateCopyWithAttributes(d.get(), attrs.get())));
                if let Some(weighted) = d2.and_then(|d2| Owned::new(CTFontCreateWithFontDescriptor(d2.get(), f.size, std::ptr::null()))) {
                    font = weighted;
                }
            }
            if f.italic {
                font = italic(font, f.size);
            }
            font
        };
        let r = font.get();
        self.fonts.insert(f.key(), font);
        r
    }

    /// Text in a family and style of its own (`g.text`, a `Text` with
    /// `.font(family:italic:)`).
    #[allow(clippy::too_many_arguments)]
    pub fn styled(&mut self, text: &str, size: f64, weight: f32, face: Face, color: Rgba, max_w: Option<f64>, align: u8) -> TextLayout {
        let font = Font { family: face.family, size, weight, italic: face.italic };
        self.spacing = (face.no_kerning, face.tracking());
        let layout = self.build(text, font, color, &[], LineHeight::Natural, max_w, align);
        self.spacing = (false, 0.0);
        layout
    }

    /// Lays `text` out in `font` and `color`, with `spans` in others.
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        text: &str,
        font: Font,
        color: Rgba,
        spans: &[Span],
        height: LineHeight,
        max_w: Option<f64>,
        align: u8,
    ) -> TextLayout {
        // Core Text counts in UTF-16 units: where each starts, in bytes.
        let mut bytes: Vec<usize> = Vec::with_capacity(text.len() + 1);
        for (i, c) in text.char_indices() {
            for _ in 0..c.len_utf16() {
                bytes.push(i);
            }
        }
        bytes.push(text.len());
        let n16 = bytes.len() - 1;
        let unit = |b: usize| bytes.partition_point(|&x| x < b);
        let base_font = self.font(font);
        let span_fonts: Vec<Option<CTFontRef>> = spans.iter().map(|s| s.font.map(|f| self.font(f))).collect();

        let mut lines: Vec<Line> = Vec::new();
        let mut shaped: Vec<ShapedLine> = Vec::new();
        unsafe {
            let s = cfstring(text);
            let attributed = Owned(CFAttributedStringCreateMutable(std::ptr::null(), 0) as CFTypeRef);
            let a = attributed.get() as CFMutableAttributedStringRef;
            CFAttributedStringReplaceString(a, CFRange::new(0, 0), s.get());
            CFAttributedStringBeginEditing(a);
            let all = CFRange::new(0, n16);
            if n16 > 0 {
                CFAttributedStringSetAttribute(a, all, kCTFontAttributeName, base_font);
                CFAttributedStringSetAttribute(a, all, kCTForegroundColorFromContextAttributeName, kCFBooleanTrue);
                let c = cfnumber_i32(packed(color));
                CFAttributedStringSetAttribute(a, all, self.color_key.get(), c.get());
                // (A kerning of 0 leaves the font's out; tracking adds to it.)
                if self.spacing.0 {
                    let none = cfnumber_f64(0.0);
                    CFAttributedStringSetAttribute(a, all, kCTKernAttributeName, none.get());
                }
                if self.spacing.1 != 0.0 {
                    let room = cfnumber_f64(self.spacing.1);
                    CFAttributedStringSetAttribute(a, all, kCTTrackingAttributeName, room.get());
                }
                for (span, f) in spans.iter().zip(&span_fonts) {
                    let (from, to) = (unit(span.range.start), unit(span.range.end));
                    if to <= from {
                        continue;
                    }
                    let r = CFRange::new(from, to - from);
                    if let Some(f) = f {
                        CFAttributedStringSetAttribute(a, r, kCTFontAttributeName, *f);
                    }
                    if let Some(c) = span.color {
                        let c = cfnumber_i32(packed(c));
                        CFAttributedStringSetAttribute(a, r, self.color_key.get(), c.get());
                    }
                }
            }
            CFAttributedStringEndEditing(a);

            // (No text has no typesetter.)
            let typesetter = if n16 > 0 { Owned::new(CTTypesetterCreateWithAttributedString(a as CFAttributedStringRef)) } else { None };
            let width = max_w.map_or(1.0e9, |w| w + 0.5);
            let mut start = 0;
            let mut made: Vec<(usize, usize, Option<Owned>)> = Vec::new();
            while let (true, Some(t)) = (start < n16, &typesetter) {
                let count = (CTTypesetterSuggestLineBreak(t.get(), start as CFIndex, width) as usize).max(1);
                let line = Owned::new(CTTypesetterCreateLine(t.get(), CFRange::new(start, count)));
                made.push((start, count, line));
                start += count;
            }
            // After a final line break (or for no text at all), an empty line.
            if n16 == 0 || text.ends_with('\n') {
                made.push((n16, 0, None));
            }

            let size = font.size;
            let (fa, fd, fl) = (CTFontGetAscent(base_font), CTFontGetDescent(base_font), CTFontGetLeading(base_font));
            // The spans with fonts of their own, in UTF-16 units.
            let font_spans: Vec<(usize, usize, CTFontRef)> = spans
                .iter()
                .zip(&span_fonts)
                .filter_map(|(sp, f)| Some((unit(sp.range.start), unit(sp.range.end), (*f)?)))
                .filter(|(from, to, _)| to > from)
                .collect();
            let mut measured = Vec::new();
            let mut widest: f64 = 0.0;
            let mut advance: f64 = 0.0;
            for (start, count, line) in &made {
                let (mut asc, mut desc, mut lead) = (fa, fd, fl);
                let (mut visible, mut full) = (0.0, 0.0);
                if let Some(l) = line {
                    let w = CTLineGetTypographicBounds(l.get(), &mut asc, &mut desc, &mut lead);
                    // The line is as tall as the fonts the text asks for, not
                    // the ones Core Text draws some characters with instead
                    // (Menlo's for Chinese, with a taller ascent and leading,
                    // put such a line lower than one without them).
                    (asc, desc, lead) = asked_metrics(*start, *start + *count, base_font, &font_spans);
                    visible = (w - CTLineGetTrailingWhitespaceWidth(l.get())).max(0.0);
                    full = w;
                    advance = advance.max(w);
                }
                widest = widest.max(visible);
                measured.push((*start, *count, asc, desc, lead, visible, full));
            }
            let mut top = 0.0;
            for ((start, count, line), (_, _, asc, desc, lead, visible, full)) in made.into_iter().zip(measured) {
                let (h, baseline) = match height {
                    LineHeight::Natural => (asc + desc + lead, lead / 2.0 + asc),
                    LineHeight::Absolute(h) => (h, (h - asc - desc) / 2.0 + asc),
                    LineHeight::Relative(k) => (k * size, (k * size - asc - desc) / 2.0 + asc),
                };
                let x = match align {
                    1 => (widest - visible) / 2.0,
                    2 => widest - visible,
                    _ => 0.0,
                };
                let range = bytes[start]..bytes[start + count];
                let mut stops: Vec<(usize, f64)> = Vec::new();
                if let Some(l) = &line {
                    let runs = CTLineGetGlyphRuns(l.get());
                    for r in 0..CFArrayGetCount(runs) {
                        let run = CFArrayGetValueAtIndex(runs, r);
                        let n = CTRunGetGlyphCount(run) as usize;
                        let mut positions = vec![CGPoint::default(); n];
                        let mut indices = vec![0 as CFIndex; n];
                        CTRunGetPositions(run, CFRange::new(0, 0), positions.as_mut_ptr());
                        CTRunGetStringIndices(run, CFRange::new(0, 0), indices.as_mut_ptr());
                        for (p, i) in positions.iter().zip(&indices) {
                            if let Some(&b) = bytes.get(*i as usize) {
                                stops.push((b, x + p.x));
                            }
                        }
                    }
                }
                stops.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                stops.dedup_by_key(|s| s.0);
                // Where the caret stands at the line's end: after its
                // spaces too (an indent on a line of its own), and before
                // its line break, which is where CoreText puts that.
                let mut end_x = x + full;
                if text[range.clone()].ends_with('\n') {
                    let nl = range.end - 1;
                    let at = stops.iter().find(|s| s.0 == nl).map(|s| s.1);
                    end_x = at.unwrap_or_else(|| stops.iter().filter(|s| s.0 < nl).map(|s| s.1).fold(x, f64::max).max(x + visible));
                    stops.retain(|s| s.0 != nl);
                    stops.push((nl, end_x));
                }
                if stops.is_empty() {
                    stops.push((range.start, x));
                }
                lines.push(Line { range, top, bottom: top + h, stops, end_x });
                shaped.push(ShapedLine { line, x, baseline: top + baseline });
                top += h;
            }
            let height = top;
            TextLayout { width: widest, advance: advance.max(widest), height, lines, len: text.len(), shaped: Shaped { lines: shaped, text: text.to_string() } }
        }
    }

    /// Text with stretches in styles of their own (a `RichTextEditor`'s),
    /// wrapped at `max_w`.
    pub fn runs(&mut self, text: &str, base: &crate::ui::text::Styled, runs: &[crate::ui::text::Styled], max_w: Option<f64>) -> TextLayout {
        let font = |s: &crate::ui::text::Styled| Font { family: s.family, size: s.size, weight: s.weight, italic: s.italic };
        let spans: Vec<Span> = runs.iter().map(|s| Span { range: s.range.clone(), font: Some(font(s)), color: Some(s.color) }).collect();
        self.build(text, font(base), base.color, &spans, LineHeight::Natural, max_w, 0)
    }

    /// Code: monospaced, not wrapped, with syntax colors.
    pub fn code_layout(&mut self, text: &str, size: f64) -> TextLayout {
        let spans = colors(crate::ui::highlight::highlight(text));
        let font = Font { family: Family::Mono, size, weight: 400.0, italic: false };
        let color = theme().colors[theme::PRIMARY];
        self.build(text, font, color, &spans, LineHeight::Absolute(CODE_LINE as f64), None, 0)
    }

    /// Monospaced text, left-aligned, in one color.
    pub fn mono_line(&mut self, text: &str, size: f64, weight: f32, color: Rgba) -> TextLayout {
        self.build(text, Font { family: Family::Mono, size, weight, italic: false }, color, &[], LineHeight::Natural, None, 0)
    }

    /// Monospaced gray text (line numbers), on the lines of the code, right-aligned.
    pub fn mono_layout(&mut self, text: &str, size: f64, color: Rgba) -> TextLayout {
        let font = Font { family: Family::Mono, size, weight: 400.0, italic: false };
        self.build(text, font, color, &[], LineHeight::Absolute(CODE_LINE as f64), None, 2)
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
        let base = Font { family: Family::System, size, weight, italic: false };
        let spans: Vec<Span> = spans
            .iter()
            .map(|(range, style)| {
                let mut font = base;
                if style.bold {
                    font.weight = 700.0;
                }
                if style.italic {
                    font.italic = true;
                }
                if style.code {
                    font.family = Family::Mono;
                    font.size = size * 0.92;
                }
                let color = if style.code {
                    Some(theme().code.type_name)
                } else if style.link.is_some() {
                    Some(theme().accent)
                } else {
                    None
                };
                Span { range: range.clone(), font: Some(font), color }
            })
            .collect();
        self.build(text, base, color, &spans, LineHeight::Relative(1.45), max_w, 0)
    }

    /// A block of code in a `Markdown` view: monospaced, wrapped at `width`,
    /// with syntax colors if it's Tessel.
    pub fn code_block(&mut self, text: &str, size: f64, tessel: bool, width: f64) -> TextLayout {
        let spans = if tessel { colors(crate::ui::highlight::highlight(text)) } else { Vec::new() };
        let font = Font { family: Family::Mono, size, weight: 400.0, italic: false };
        let color = theme().colors[theme::PRIMARY];
        self.build(text, font, color, &spans, LineHeight::Relative(1.45), Some(width), 0)
    }

    pub fn layout(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>) -> TextLayout {
        self.aligned(text, size, weight, color, max_w, 0)
    }

    /// Like `layout`, with the lines lined up (0 leading, 1 center, 2
    /// trailing) inside the width of the widest.
    pub fn aligned(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>, align: u8) -> TextLayout {
        self.build(text, Font { family: Family::System, size, weight, italic: false }, color, &[], LineHeight::Natural, max_w, align)
    }
}

/// A font of the family `name`, in `f`'s size and weight, or none when no
/// such family is installed.
unsafe fn named(name: &str, f: Font) -> Option<Owned> {
    unsafe {
        let family = cfstring(name);
        let w = cfnumber_f64(weight_trait(f.weight));
        let traits = cfdict(&[(kCTFontWeightTrait, w.get())]);
        let attrs = cfdict(&[(kCTFontFamilyNameAttribute, family.get()), (kCTFontTraitsAttribute, traits.get())]);
        let d = Owned::new(CTFontDescriptorCreateWithAttributes(attrs.get()))?;
        let matched = Owned::new(CTFontDescriptorCreateMatchingFontDescriptor(d.get(), std::ptr::null()))?;
        let font = Owned::new(CTFontCreateWithFontDescriptor(matched.get(), f.size, std::ptr::null()))?;
        // (Core Text may offer another family in its place.)
        let got = Owned::new(CTFontCopyFamilyName(font.get()));
        if !got.is_some_and(|g| string_of(g.get()).eq_ignore_ascii_case(name.trim())) {
            return None;
        }
        Some(if f.italic { italic(font, f.size) } else { font })
    }
}

/// `font` in italic: its family's italic, or the font slanted when the
/// family has none.
unsafe fn italic(font: Owned, size: f64) -> Owned {
    unsafe {
        if CTFontGetSymbolicTraits(font.get()) & kCTFontItalicTrait != 0 {
            return font;
        }
        let slanted = CTFontCreateCopyWithSymbolicTraits(font.get(), size, std::ptr::null(), kCTFontItalicTrait, kCTFontItalicTrait);
        if let Some(slanted) = Owned::new(slanted) {
            if CTFontGetSymbolicTraits(slanted.get()) & kCTFontItalicTrait != 0 {
                return slanted;
            }
        }
        // About 12 degrees, as fonts' obliques lean.
        let skew = CGAffineTransform { a: 1.0, b: 0.0, c: 0.21, d: 1.0, tx: 0.0, ty: 0.0 };
        // (Drawn slanted by `draw`, which turns glyphs by their font's matrix.)
        Owned::new(CTFontCreateCopyWithAttributes(font.get(), size, &skew, std::ptr::null())).unwrap_or(font)
    }
}

/// The font families installed.
pub fn families() -> Vec<String> {
    unsafe {
        let Some(names) = Owned::new(CTFontManagerCopyAvailableFontFamilyNames()) else { return Vec::new() };
        (0..CFArrayGetCount(names.get())).map(|i| string_of(CFArrayGetValueAtIndex(names.get(), i))).collect()
    }
}

fn colors(ranges: Vec<(Range<usize>, Rgba)>) -> Vec<Span> {
    ranges.into_iter().map(|(range, color)| Span { range, font: None, color: Some(color) }).collect()
}

/// Draws a layout's lines in a context whose y grows downwards, with its
/// top left at (0, 0): each stretch in its color, or all in `tint`.
pub fn draw(ctx: CGContextRef, layout: &TextLayout, tint: Option<Rgba>, color_key: CFStringRef, halo: Option<(Rgba, f64)>) {
    unsafe {
        // Glyphs are drawn y-up: flip them back.
        let flipped = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: -1.0, tx: 0.0, ty: 0.0 };
        CGContextSetTextMatrix(ctx, flipped);
        for pass in [true, false] {
            let outline = pass;
            if outline && halo.is_none() {
                continue;
            }
            if let (true, Some((c, width))) = (outline, halo) {
                CGContextSetTextDrawingMode(ctx, kCGTextStroke);
                CGContextSetLineWidth(ctx, width);
                CGContextSetLineJoin(ctx, kCGLineJoinRound);
                let [r, g, b, a] = c.0.map(|v| v as f64 / 255.0);
                CGContextSetRGBStrokeColor(ctx, r, g, b, a);
            } else {
                CGContextSetTextDrawingMode(ctx, kCGTextFill);
            }
            for l in &layout.shaped.lines {
                let Some(line) = &l.line else { continue };
                CGContextSetTextPosition(ctx, l.x, l.baseline);
                let runs = CTLineGetGlyphRuns(line.get());
                for i in 0..CFArrayGetCount(runs) {
                    let run = CFArrayGetValueAtIndex(runs, i);
                    // A font slanted for italic (see `italic`): its glyphs
                    // are turned by its matrix, which drawing leaves out.
                    let font = CFDictionaryGetValue(CTRunGetAttributes(run), kCTFontAttributeName);
                    let m = if font.is_null() { None } else { Some(CTFontGetMatrix(font)) };
                    let slant = m.filter(|m| m.c != 0.0 || m.b != 0.0);
                    if let Some(m) = slant {
                        CGContextSetTextMatrix(ctx, CGAffineTransform { a: m.a, b: -m.b, c: m.c, d: -m.d, tx: 0.0, ty: 0.0 });
                        CGContextSetTextPosition(ctx, l.x, l.baseline);
                    }
                    if !outline {
                        let c = tint.unwrap_or_else(|| {
                            let value = CFDictionaryGetValue(CTRunGetAttributes(run), color_key);
                            let mut v: i32 = -1;
                            if !value.is_null() {
                                CFNumberGetValue(value, kCFNumberSInt32Type, &mut v as *mut i32 as *mut c_void);
                            }
                            Rgba(v.to_be_bytes())
                        });
                        let [r, g, b, a] = c.0.map(|v| v as f64 / 255.0);
                        CGContextSetRGBFillColor(ctx, r, g, b, a);
                    }
                    CTRunDraw(run, ctx, CFRange::new(0, 0));
                    if slant.is_some() {
                        CGContextSetTextMatrix(ctx, flipped);
                        CGContextSetTextPosition(ctx, l.x, l.baseline);
                    }
                }
            }
        }
        CGContextSetTextDrawingMode(ctx, kCGTextFill);
    }
}

thread_local! {
    static COLOR_KEY: Owned = cfstring("TesselColor");
}

/// The attribute the engines keep colors in (the same text in each).
pub fn color_key() -> CFStringRef {
    COLOR_KEY.with(|k| k.get())
}

// ===== glyphs, outlines, metrics and styles =====

/// Each run of a layout's lines: the line, its run, and the run's font.
unsafe fn runs(layout: &TextLayout, mut f: impl FnMut(&ShapedLine, CTRunRef, CTFontRef)) {
    unsafe {
        for l in &layout.shaped.lines {
            let Some(line) = &l.line else { continue };
            let runs = CTLineGetGlyphRuns(line.get());
            for i in 0..CFArrayGetCount(runs) {
                let run = CFArrayGetValueAtIndex(runs, i);
                let font = CFDictionaryGetValue(CTRunGetAttributes(run), kCTFontAttributeName);
                f(l, run, font);
            }
        }
    }
}

/// Where each UTF-16 unit of `text` starts, in bytes (and its end).
fn unit_bytes(text: &str) -> Vec<usize> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        for _ in 0..c.len_utf16() {
            bytes.push(i);
        }
    }
    bytes.push(text.len());
    bytes
}

/// The glyphs of a layout, as they're placed (see `PlacedGlyph`).
pub fn glyphs(layout: &TextLayout) -> Vec<crate::ui::text::PlacedGlyph> {
    let bytes = unit_bytes(&layout.shaped.text);
    let byte = |i: CFIndex| bytes.get(i.max(0) as usize).copied().unwrap_or(layout.shaped.text.len());
    let mut out = Vec::new();
    unsafe {
        for l in &layout.shaped.lines {
            let Some(line) = &l.line else { continue };
            let range = CTLineGetStringRange(line.get());
            let end = byte(range.location + range.length);
            let mut at = Vec::new();
            let mut placed = Vec::new();
            let all = CTLineGetGlyphRuns(line.get());
            for r in 0..CFArrayGetCount(all) {
                let run = CFArrayGetValueAtIndex(all, r);
                let n = CTRunGetGlyphCount(run) as usize;
                let mut positions = vec![CGPoint::default(); n];
                let mut advances = vec![CGSize::default(); n];
                let mut indices = vec![0 as CFIndex; n];
                CTRunGetPositions(run, CFRange::new(0, 0), positions.as_mut_ptr());
                CTRunGetAdvances(run, CFRange::new(0, 0), advances.as_mut_ptr());
                CTRunGetStringIndices(run, CFRange::new(0, 0), indices.as_mut_ptr());
                for i in 0..n {
                    // (Core Text gives a line break a glyph of its own: it's none.)
                    if matches!(layout.shaped.text.as_bytes().get(byte(indices[i])), Some(b'\n' | b'\r')) {
                        continue;
                    }
                    at.push(byte(indices[i]));
                    placed.push((l.x + positions[i].x, l.baseline - positions[i].y, advances[i].width));
                }
            }
            // (The line break at the end of a line is no glyph's.)
            let text = &layout.shaped.text;
            let end = if text[..end].ends_with('\n') { end - 1 } else { end };
            for ((x, y, advance), text) in placed.into_iter().zip(crate::ui::text::cluster_ranges(&at, end)) {
                out.push(crate::ui::text::PlacedGlyph { x, y, advance, text });
            }
        }
    }
    out
}

/// The outline of a layout's glyphs, in its coordinates (y down from its
/// top): the font's own curves.
pub fn outline(layout: &TextLayout) -> kurbo::BezPath {
    unsafe extern "C" fn element(info: *mut c_void, e: *const CGPathElement) {
        let path = unsafe { &mut *(info as *mut kurbo::BezPath) };
        let e = unsafe { &*e };
        let p = |i: usize| {
            let q = unsafe { *e.points.add(i) };
            kurbo::Point::new(q.x, q.y)
        };
        match e.kind {
            0 => path.move_to(p(0)),
            1 => path.line_to(p(0)),
            2 => path.quad_to(p(0), p(1)),
            3 => path.curve_to(p(0), p(1), p(2)),
            _ => path.close_path(),
        }
    }
    let mut path = kurbo::BezPath::new();
    unsafe {
        runs(layout, |l, run, font| {
            if font.is_null() {
                return;
            }
            let n = CTRunGetGlyphCount(run) as usize;
            let mut glyphs = vec![0u16; n];
            let mut positions = vec![CGPoint::default(); n];
            CTRunGetGlyphs(run, CFRange::new(0, 0), glyphs.as_mut_ptr());
            CTRunGetPositions(run, CFRange::new(0, 0), positions.as_mut_ptr());
            for (g, p) in glyphs.iter().zip(&positions) {
                // Glyphs are y up from the baseline: flipped, and put in place.
                let m = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: -1.0, tx: l.x + p.x, ty: l.baseline - p.y };
                let Some(glyph) = Owned::new(CTFontCreatePathForGlyph(font, *g, &m)) else { continue };
                CGPathApply(glyph.get(), &mut path as *mut kurbo::BezPath as *mut c_void, element);
            }
        });
    }
    path
}

/// The metrics of the font a layout's text starts in.
pub fn metrics(layout: &TextLayout) -> Option<crate::ui::text::FontMetrics> {
    let mut found = None;
    unsafe {
        runs(layout, |_, _, font| {
            if found.is_none() && !font.is_null() {
                found = Some(crate::ui::text::FontMetrics {
                    ascent: CTFontGetAscent(font),
                    descent: CTFontGetDescent(font),
                    line_gap: CTFontGetLeading(font),
                    cap_height: CTFontGetCapHeight(font),
                    x_height: CTFontGetXHeight(font),
                });
            }
        });
    }
    found
}

/// The weight (100 to 900) a Core Text weight trait stands for.
fn css_weight(t: f64) -> f32 {
    let marks = [(-0.8, 100.0), (-0.6, 200.0), (-0.4, 300.0), (0.0, 400.0), (0.23, 500.0), (0.3, 600.0), (0.4, 700.0), (0.56, 800.0), (0.62, 900.0)];
    marks.iter().min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs())).map_or(400.0, |m| m.1)
}

/// The faces of the family `name`, lightest first.
pub fn styles(name: &str) -> Vec<crate::ui::text::FontStyle> {
    let mut out = Vec::new();
    unsafe {
        let family = cfstring(name.trim());
        let attrs = cfdict(&[(kCTFontFamilyNameAttribute, family.get())]);
        let Some(d) = Owned::new(CTFontDescriptorCreateWithAttributes(attrs.get())) else { return out };
        let Some(all) = Owned::new(CTFontDescriptorCreateMatchingFontDescriptors(d.get(), std::ptr::null())) else { return out };
        for i in 0..CFArrayGetCount(all.get()) {
            let face = CFArrayGetValueAtIndex(all.get(), i);
            // (Only the family's own faces, not what Core Text offers in its place.)
            let of = Owned::new(CTFontDescriptorCopyAttribute(face, kCTFontFamilyNameAttribute)).map(|f| string_of(f.get()));
            if !of.is_some_and(|f| f.eq_ignore_ascii_case(name.trim())) {
                continue;
            }
            let style = Owned::new(CTFontDescriptorCopyAttribute(face, kCTFontStyleNameAttribute)).map(|s| string_of(s.get())).unwrap_or_default();
            let (mut weight, mut italic) = (400.0, false);
            if let Some(traits) = Owned::new(CTFontDescriptorCopyAttribute(face, kCTFontTraitsAttribute)) {
                let w = CFDictionaryGetValue(traits.get(), kCTFontWeightTrait);
                let mut t = 0.0f64;
                if !w.is_null() && CFNumberGetValue(w, kCFNumberFloat64Type, &mut t as *mut f64 as *mut c_void) {
                    weight = css_weight(t);
                }
                let s = CFDictionaryGetValue(traits.get(), kCTFontSymbolicTrait);
                let mut bits = 0i32;
                if !s.is_null() && CFNumberGetValue(s, kCFNumberSInt32Type, &mut bits as *mut i32 as *mut c_void) {
                    italic = bits as u32 & kCTFontItalicTrait != 0;
                }
            }
            if !style.is_empty() && !out.iter().any(|o: &crate::ui::text::FontStyle| o.name == style) {
                out.push(crate::ui::text::FontStyle { name: style, weight, italic });
            }
        }
    }
    out.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::Face;

    /// The monospaced font in bold is its bold face (issue #43).
    #[test]
    fn monospaced_bold_is_bold() {
        unsafe extern "C" {
            fn CTFontCopyPostScriptName(font: CTFontRef) -> CFStringRef;
        }
        let mut e = TextEngine::new();
        let font = e.font(Font { family: Family::Mono, size: 13.0, weight: 700.0, italic: false });
        let name = Owned::new(unsafe { CTFontCopyPostScriptName(font) } as CFTypeRef).map(|n| string_of(n.get() as CFStringRef));
        assert!(name.as_deref().is_some_and(|n| n.contains("Bold")), "{name:?}");
    }

    /// Chinese characters drawn with another font than Menlo don't move
    /// the monospaced line's baseline (issue #39).
    #[test]
    fn fallback_fonts_keep_the_baseline() {
        let mut e = TextEngine::new();
        let mono = Face { family: Family::Mono, ..Face::default() };
        let black = crate::ui::Rgba([0, 0, 0, 255]);
        let latin = e.styled("Wall", 16.0, 400.0, mono, black, None, 0);
        let cjk = e.styled("日本の基礎", 16.0, 400.0, mono, black, None, 0);
        assert_eq!(latin.shaped.lines[0].baseline, cjk.shaped.lines[0].baseline);
        assert_eq!(latin.height, cjk.height);
    }
}
