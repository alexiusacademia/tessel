//! Laying text out with DirectWrite, for the Windows renderer: Segoe UI and
//! a monospaced font, shaped and broken into lines by Windows, and drawn by
//! Direct2D (see `win::play`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use windows::core::{implement, Interface, PCWSTR};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows_numerics::Vector2;

use super::factories;
use crate::ui::image::Picture;
use crate::ui::pdf;
use crate::ui::text::{Face, Family, Line, TextLayout, CODE_LINE};
use crate::ui::theme::{self, theme};
use crate::ui::Rgba;

#[derive(Clone, Copy, PartialEq)]
struct Font {
    family: Family,
    size: f64,
    weight: f32,
    italic: bool,
}

struct Span {
    range: Range<usize>,
    font: Option<Font>,
    color: Option<Rgba>,
}

#[derive(Clone, Copy)]
enum LineHeight {
    Natural,
    Absolute(f64),
    Relative(f64),
}

/// A layout as DirectWrite made it, and the colors of its stretches (in
/// UTF-16 units), to draw it with.
pub struct Shaped {
    pub layout: Option<IDWriteTextLayout>,
    pub color: Rgba,
    pub colors: Vec<(DWRITE_TEXT_RANGE, Rgba)>,
    /// The text, which glyphs are said to show (see `glyphs`).
    pub text: String,
}

pub struct TextEngine {
    formats: HashMap<(Family, u64, u32, bool), IDWriteTextFormat>,
    /// For the layout being built: the font's kerning left out, and room
    /// added between letters (see `styled`).
    spacing: (bool, f64),
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The fonts installed.
fn system_fonts() -> Option<IDWriteFontCollection> {
    let mut collection = None;
    unsafe { factories().dwrite.GetSystemFontCollection(&mut collection, false).ok()? };
    collection
}

/// Whether a family of fonts is installed.
fn installed(family: &str) -> bool {
    let Some(c) = system_fonts() else { return false };
    let name = wide(family);
    let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
    unsafe { c.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() }
}

/// The monospaced font: Cascadia Mono where there is one (Windows 11), else Consolas.
fn mono_family() -> &'static str {
    static FAMILY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FAMILY.get_or_init(|| if installed("Cascadia Mono") { "Cascadia Mono" } else { "Consolas" })
}

/// A font's family: Segoe UI unless another's asked for (and installed).
fn family(f: Font) -> String {
    match f.family {
        Family::Mono => mono_family().to_string(),
        named => named.name().filter(|n| installed(n)).unwrap_or_else(|| "Segoe UI".to_string()),
    }
}

/// The font families installed: each one's English name, or its first.
pub fn families() -> Vec<String> {
    let Some(c) = system_fonts() else { return Vec::new() };
    let mut out = Vec::new();
    unsafe {
        for i in 0..c.GetFontFamilyCount() {
            let Ok(names) = c.GetFontFamily(i).and_then(|f| f.GetFamilyNames()) else { continue };
            let locale = wide("en-us");
            let (mut at, mut exists) = (0u32, windows::core::BOOL(0));
            if names.FindLocaleName(PCWSTR(locale.as_ptr()), &mut at, &mut exists).is_err() || !exists.as_bool() {
                at = 0;
            }
            let Ok(len) = names.GetStringLength(at) else { continue };
            let mut buf = vec![0u16; len as usize + 1];
            if names.GetString(at, &mut buf).is_ok() {
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
        }
    }
    out
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { formats: HashMap::new(), spacing: (false, 0.0) }
    }

    fn format(&mut self, f: Font) -> Option<IDWriteTextFormat> {
        let key = (f.family, f.size.to_bits(), f.weight.to_bits(), f.italic);
        if let Some(x) = self.formats.get(&key) {
            return Some(x.clone());
        }
        let name = wide(&family(f));
        let locale = wide("en-us");
        let format = unsafe {
            factories()
                .dwrite
                .CreateTextFormat(
                    PCWSTR(name.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(f.weight as i32),
                    if f.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                    DWRITE_FONT_STRETCH_NORMAL,
                    f.size as f32,
                    PCWSTR(locale.as_ptr()),
                )
                .ok()?
        };
        self.formats.insert(key, format.clone());
        Some(format)
    }

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
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // Where each UTF-16 unit starts, in bytes.
        let mut bytes: Vec<usize> = Vec::with_capacity(utf16.len() + 1);
        for (i, c) in text.char_indices() {
            for _ in 0..c.len_utf16() {
                bytes.push(i);
            }
        }
        bytes.push(text.len());
        let unit = |b: usize| bytes.partition_point(|&x| x < b) as u32;
        let empty = |color| TextLayout {
            width: 0.0,
            advance: 0.0,
            height: font.size * 1.2,
            lines: vec![Line { range: 0..text.len(), top: 0.0, bottom: font.size * 1.2, stops: vec![(0, 0.0)], end_x: 0.0 }],
            len: text.len(),
            shaped: Shaped { layout: None, color, colors: Vec::new(), text: text.to_string() },
        };
        let Some(format) = self.format(font) else { return empty(color) };
        let made = unsafe {
            let wrap = max_w.is_some();
            let layout = match factories().dwrite.CreateTextLayout(&utf16, &format, max_w.map_or(1.0e7, |w| w as f32 + 0.5), 1.0e7) {
                Ok(l) => l,
                Err(_) => return empty(color),
            };
            let _ = layout.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP });
            let whole = DWRITE_TEXT_RANGE { startPosition: 0, length: utf16.len() as u32 };
            // The font's kerning left out, and room added between letters.
            if self.spacing.0 {
                if let Ok(t) = factories().dwrite.CreateTypography() {
                    let off = DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_KERNING, parameter: 0 };
                    if t.AddFontFeature(off).is_ok() {
                        let _ = layout.SetTypography(&t, whole);
                    }
                }
            }
            if self.spacing.1 != 0.0 {
                if let Ok(spaced) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = spaced.SetCharacterSpacing(0.0, self.spacing.1 as f32, 0.0, whole);
                }
            }
            let mut colors = Vec::new();
            for span in spans {
                let r = DWRITE_TEXT_RANGE { startPosition: unit(span.range.start), length: unit(span.range.end) - unit(span.range.start) };
                if r.length == 0 {
                    continue;
                }
                if let Some(f) = span.font {
                    if f.family != font.family {
                        let n = wide(&family(f));
                        let _ = layout.SetFontFamilyName(PCWSTR(n.as_ptr()), r);
                    }
                    let _ = layout.SetFontWeight(DWRITE_FONT_WEIGHT(f.weight as i32), r);
                    let _ = layout.SetFontSize(f.size as f32, r);
                    if f.italic {
                        let _ = layout.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, r);
                    }
                }
                if let Some(c) = span.color {
                    colors.push((r, c));
                }
            }
            // Line heights: as the font says, or set ones with the text
            // in the middle of each line.
            let mut first = [DWRITE_LINE_METRICS::default(); 1];
            let mut count = 0u32;
            let _ = layout.GetLineMetrics(Some(&mut first), &mut count);
            let natural = first[0];
            let set = match height {
                LineHeight::Natural => None,
                LineHeight::Absolute(h) => Some(h as f32),
                LineHeight::Relative(k) => Some((k * font.size) as f32),
            };
            if let Some(h) = set {
                let baseline = (h - natural.height) / 2.0 + natural.baseline;
                let _ = layout.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, h, baseline);
            }
            // Lines line up inside the width of the widest.
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut m);
            if align != 0 {
                let _ = layout.SetMaxWidth(m.width + 0.5);
                let _ = layout.SetTextAlignment(if align == 1 { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_TRAILING });
                let _ = layout.GetMetrics(&mut m);
            }
            (layout, colors, m)
        };
        let (layout, colors, m) = made;
        let mut lines = Vec::new();
        unsafe {
            let mut count = 0u32;
            let _ = layout.GetLineMetrics(None, &mut count);
            let mut metrics = vec![DWRITE_LINE_METRICS::default(); count as usize];
            let _ = layout.GetLineMetrics(Some(&mut metrics), &mut count);
            let mut start16 = 0u32;
            let mut top = 0.0;
            for lm in metrics.iter().take(count as usize) {
                let end16 = (start16 + lm.length).min(utf16.len() as u32);
                let range = bytes[start16 as usize]..bytes[end16 as usize];
                let mut stops = Vec::new();
                // Where the caret stands before each cluster.
                let mut ccount = 0u32;
                let mut clusters = Vec::new();
                let _ = layout.GetClusterMetrics(None, &mut ccount);
                clusters.resize(ccount as usize, DWRITE_CLUSTER_METRICS::default());
                let _ = layout.GetClusterMetrics(Some(&mut clusters), &mut ccount);
                let mut p = 0u32;
                let mut end_x: f64 = 0.0;
                for c in clusters.iter().take(ccount as usize) {
                    if p >= start16 && p < end16 {
                        let (mut x, mut y) = (0.0f32, 0.0f32);
                        let mut hit = DWRITE_HIT_TEST_METRICS::default();
                        let _ = layout.HitTestTextPosition(p, false, &mut x, &mut y, &mut hit);
                        stops.push((bytes[p as usize], x as f64));
                        // (Flags: wrap after, whitespace, line break, …)
                        let newline = (c._bitfield >> 2) & 1 == 1;
                        if !newline {
                            end_x = end_x.max(x as f64 + c.width as f64);
                        }
                    }
                    p += c.length as u32;
                }
                stops.sort_by(|a, b| a.0.cmp(&b.0));
                stops.dedup_by_key(|s| s.0);
                if stops.is_empty() {
                    let (mut x, mut y) = (0.0f32, 0.0f32);
                    let mut hit = DWRITE_HIT_TEST_METRICS::default();
                    let _ = layout.HitTestTextPosition(start16, false, &mut x, &mut y, &mut hit);
                    stops.push((range.start, x as f64));
                    end_x = x as f64;
                }
                let h = lm.height as f64;
                lines.push(Line { range, top, bottom: top + h, stops, end_x });
                top += h;
                start16 = end16;
            }
        }
        if lines.is_empty() {
            return empty(color);
        }
        TextLayout {
            width: m.width as f64,
            advance: (m.widthIncludingTrailingWhitespace as f64).max(m.width as f64),
            height: m.height as f64,
            lines,
            len: text.len(),
            shaped: Shaped { layout: Some(layout), color, colors, text: text.to_string() },
        }
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
        self.build(text, font, theme().colors[theme::PRIMARY], &spans, LineHeight::Absolute(CODE_LINE as f64), None, 0)
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
        self.build(text, font, theme().colors[theme::PRIMARY], &spans, LineHeight::Relative(1.45), Some(width), 0)
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

fn colors(ranges: Vec<(Range<usize>, Rgba)>) -> Vec<Span> {
    ranges.into_iter().map(|(range, color)| Span { range, font: None, color: Some(color) }).collect()
}

/// Draws a layout at the target's (0, 0): each stretch in its color, or all
/// in `tint`; a halo is the text drawn around itself in the halo's color.
pub fn draw(
    rt: &ID2D1RenderTarget,
    layout: &TextLayout,
    tint: Option<Rgba>,
    halo: Option<(Rgba, f64)>,
    brush: &mut dyn FnMut(Rgba) -> Option<ID2D1SolidColorBrush>,
) {
    let Some(l) = &layout.shaped.layout else { return };
    let all = DWRITE_TEXT_RANGE { startPosition: 0, length: u32::MAX };
    unsafe {
        let paint = |color: Option<Rgba>, origin: Vector2, brush: &mut dyn FnMut(Rgba) -> Option<ID2D1SolidColorBrush>| {
            let base = color.unwrap_or(layout.shaped.color);
            let Some(default) = brush(base) else { return };
            let _ = l.SetDrawingEffect(None, all);
            if color.is_none() {
                for (r, c) in &layout.shaped.colors {
                    if let Some(effect) = brush(*c).and_then(|b| b.cast::<windows::core::IUnknown>().ok()) {
                        let _ = l.SetDrawingEffect(&effect, *r);
                    }
                }
            }
            rt.DrawTextLayout(origin, l, &default, D2D1_DRAW_TEXT_OPTIONS_NONE);
        };
        if let Some((c, width)) = halo {
            // Around the text, in eight directions.
            let d = (width / 2.0) as f32;
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)] {
                paint(Some(c), Vector2 { X: dx * d, Y: dy * d }, brush);
            }
        }
        paint(tint, Vector2 { X: 0.0, Y: 0.0 }, brush);
    }
}

// ===== PDFs =====

/// What `ui::pdf` asks of DirectWrite and WIC: the glyphs a layout is drawn
/// with, the font files they're in, and pictures' pixels.
#[derive(Default)]
pub struct PdfSource {
    /// Font files read so far, by path and index in the file.
    fonts: Rc<RefCell<HashMap<String, Rc<pdf::FontFile>>>>,
}

impl pdf::Source for PdfSource {
    fn glyphs(&self, layout: &TextLayout) -> Vec<pdf::PlacedGlyph> {
        let Some(l) = &layout.shaped.layout else { return Vec::new() };
        let out = Rc::new(RefCell::new(Vec::new()));
        let collector = GlyphCollector {
            out: out.clone(),
            fonts: self.fonts.clone(),
            color: layout.shaped.color,
            colors: layout.shaped.colors.clone(),
        };
        let renderer: IDWriteTextRenderer = collector.into();
        unsafe {
            let _ = l.Draw(None, &renderer, 0.0, 0.0);
        }
        out.take()
    }

    fn pixels(&self, picture: &Picture) -> Option<(u32, u32, Vec<u8>)> {
        let source = &picture.image.0;
        let (mut w, mut h) = (0, 0);
        unsafe {
            source.GetSize(&mut w, &mut h).ok()?;
            let mut pixels = vec![0u8; (w * h * 4) as usize];
            source.CopyPixels(std::ptr::null(), w * 4, &mut pixels).ok()?;
            super::to_rgba(&mut pixels);
            Some((w, h, pixels))
        }
    }
}

/// Collects the glyph runs a layout draws (DirectWrite calls it as it
/// "draws" the layout), placed where they'd be drawn.
#[implement(IDWriteTextRenderer)]
struct GlyphCollector {
    out: Rc<RefCell<Vec<pdf::PlacedGlyph>>>,
    fonts: Rc<RefCell<HashMap<String, Rc<pdf::FontFile>>>>,
    color: Rgba,
    colors: Vec<(DWRITE_TEXT_RANGE, Rgba)>,
}

impl GlyphCollector {
    /// The file a font face is in (read once).
    fn font_file(&self, face: &IDWriteFontFace) -> Option<Rc<pdf::FontFile>> {
        unsafe {
            let mut count = 0u32;
            face.GetFiles(&mut count, None).ok()?;
            if count == 0 {
                return None;
            }
            let mut files: Vec<Option<IDWriteFontFile>> = vec![None; count as usize];
            face.GetFiles(&mut count, Some(files.as_mut_ptr())).ok()?;
            let file = files.first()?.as_ref()?;
            let mut key = std::ptr::null_mut();
            let mut size = 0u32;
            file.GetReferenceKey(&mut key, &mut size).ok()?;
            let local: IDWriteLocalFontFileLoader = file.GetLoader().ok()?.cast().ok()?;
            let len = local.GetFilePathLengthFromKey(key, size).ok()?;
            let mut path = vec![0u16; len as usize + 1];
            local.GetFilePathFromKey(key, size, &mut path).ok()?;
            let path = String::from_utf16_lossy(&path[..len as usize]);
            let index = face.GetIndex();
            let name = format!("{path}#{index}");
            if let Some(f) = self.fonts.borrow().get(&name) {
                return Some(f.clone());
            }
            // (The fonts of one collection share its bytes.)
            let same_file = self.fonts.borrow().values().find(|f| f.key.starts_with(&format!("{path}#"))).map(|f| f.data.clone());
            let data = match same_file {
                Some(d) => d,
                None => Rc::new(std::fs::read(&path).ok()?),
            };
            let f = Rc::new(pdf::FontFile { key: name.clone(), data, index });
            self.fonts.borrow_mut().insert(name, f.clone());
            Some(f)
        }
    }

    /// The color of the text at `at` (in UTF-16 units).
    fn color_at(&self, at: u32) -> Rgba {
        let inside = |r: &DWRITE_TEXT_RANGE| at >= r.startPosition && at - r.startPosition < r.length;
        self.colors.iter().rev().find(|(r, _)| inside(r)).map_or(self.color, |(_, c)| *c)
    }
}

/// `n` items at `p`, or none for a null pointer.
unsafe fn items<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if p.is_null() || n == 0 { &[] } else { unsafe { std::slice::from_raw_parts(p, n) } }
}

impl IDWritePixelSnapping_Impl for GlyphCollector_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const std::ffi::c_void) -> windows::core::Result<windows::core::BOOL> {
        Ok(true.into())
    }

    fn GetCurrentTransform(&self, _: *const std::ffi::c_void, transform: *mut DWRITE_MATRIX) -> windows::core::Result<()> {
        unsafe { *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
        Ok(())
    }

    fn GetPixelsPerDip(&self, _: *const std::ffi::c_void) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for GlyphCollector_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const std::ffi::c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        let Some(run) = (unsafe { run.as_ref() }) else { return Ok(()) };
        let Some(face) = run.fontFace.as_ref() else { return Ok(()) };
        let Some(file) = self.font_file(face) else { return Ok(()) };
        let n = run.glyphCount as usize;
        let (ids, advances, offsets) = unsafe { (items(run.glyphIndices, n), items(run.glyphAdvances, n), items(run.glyphOffsets, n)) };
        // The characters each glyph shows (the first glyph of a cluster
        // gets its characters), and where they start in the layout's text.
        let mut chars: Vec<Vec<u16>> = vec![Vec::new(); n];
        let mut first: Vec<Option<u32>> = vec![None; n];
        if let Some(d) = unsafe { description.as_ref() } {
            let len = d.stringLength as usize;
            let (text, map) = unsafe { (items(d.string.0, len), items(d.clusterMap, len)) };
            for (i, (c, g)) in text.iter().zip(map).enumerate() {
                let g = *g as usize;
                if g < n {
                    chars[g].push(*c);
                    first[g].get_or_insert(d.textPosition + i as u32);
                }
            }
        }
        let rtl = run.bidiLevel & 1 == 1;
        // DirectWrite slants a font that has no italic as about 20 degrees.
        let oblique = unsafe { face.GetSimulations() }.0 & DWRITE_FONT_SIMULATIONS_OBLIQUE.0 != 0;
        let slant = if oblique { 0.35 } else { 0.0 };
        let mut pen = x;
        let mut out = self.out.borrow_mut();
        for i in 0..n {
            let advance = advances.get(i).copied().unwrap_or(0.0);
            let (dx, dy) = offsets.get(i).map_or((0.0, 0.0), |o| (o.advanceOffset, o.ascenderOffset));
            if rtl {
                pen -= advance;
            }
            let at = if rtl { pen - dx } else { pen + dx };
            out.push(pdf::PlacedGlyph {
                font: file.clone(),
                id: ids[i],
                size: run.fontEmSize,
                x: at,
                y: y - dy,
                advance,
                text: String::from_utf16_lossy(&chars[i]),
                color: first[i].map_or(self.color, |p| self.color_at(p)),
                slant,
            });
            if !rtl {
                pen += advance;
            }
        }
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: windows::core::Ref<IDWriteInlineObject>,
        _: windows::core::BOOL,
        _: windows::core::BOOL,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

// ===== glyphs, outlines, metrics and styles =====

/// A run of glyphs as DirectWrite draws it.
struct Run {
    face: IDWriteFontFace,
    size: f32,
    /// Where it starts, on its baseline.
    x: f32,
    y: f32,
    ids: Vec<u16>,
    advances: Vec<f32>,
    offsets: Vec<DWRITE_GLYPH_OFFSET>,
    rtl: bool,
    sideways: bool,
    /// Where each glyph's text starts (UTF-16 units; none after a cluster's first).
    at: Vec<Option<u32>>,
}

/// Collects the glyph runs a layout draws.
#[implement(IDWriteTextRenderer)]
struct RunCollector {
    runs: Rc<RefCell<Vec<Run>>>,
}

impl IDWritePixelSnapping_Impl for RunCollector_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const std::ffi::c_void) -> windows::core::Result<windows::core::BOOL> {
        Ok(true.into())
    }

    fn GetCurrentTransform(&self, _: *const std::ffi::c_void, transform: *mut DWRITE_MATRIX) -> windows::core::Result<()> {
        unsafe { *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
        Ok(())
    }

    fn GetPixelsPerDip(&self, _: *const std::ffi::c_void) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for RunCollector_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const std::ffi::c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        description: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        let Some(run) = (unsafe { run.as_ref() }) else { return Ok(()) };
        let Some(face) = run.fontFace.as_ref() else { return Ok(()) };
        let n = run.glyphCount as usize;
        let (ids, advances, offsets) = unsafe { (items(run.glyphIndices, n), items(run.glyphAdvances, n), items(run.glyphOffsets, n)) };
        let mut at = vec![None; n];
        if let Some(d) = unsafe { description.as_ref() } {
            let map = unsafe { items(d.clusterMap, d.stringLength as usize) };
            for (i, g) in map.iter().enumerate() {
                if let Some(slot) = at.get_mut(*g as usize) {
                    slot.get_or_insert(d.textPosition + i as u32);
                }
            }
        }
        self.runs.borrow_mut().push(Run {
            face: face.clone(),
            size: run.fontEmSize,
            x,
            y,
            ids: ids.to_vec(),
            advances: if advances.is_empty() { vec![0.0; n] } else { advances.to_vec() },
            offsets: if offsets.is_empty() { vec![DWRITE_GLYPH_OFFSET::default(); n] } else { offsets.to_vec() },
            rtl: run.bidiLevel & 1 == 1,
            sideways: run.isSideways.as_bool(),
            at,
        });
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _: *const std::ffi::c_void,
        _: f32,
        _: f32,
        _: windows::core::Ref<IDWriteInlineObject>,
        _: windows::core::BOOL,
        _: windows::core::BOOL,
        _: windows::core::Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// The glyph runs of a layout, as it draws them from its top left corner.
fn runs_of(layout: &TextLayout) -> Vec<Run> {
    let Some(l) = &layout.shaped.layout else { return Vec::new() };
    let runs = Rc::new(RefCell::new(Vec::new()));
    let renderer: IDWriteTextRenderer = RunCollector { runs: runs.clone() }.into();
    unsafe {
        let _ = l.Draw(None, &renderer, 0.0, 0.0);
    }
    runs.take()
}

/// The glyphs of a layout, as they're placed (see `PlacedGlyph`).
pub fn glyphs(layout: &TextLayout) -> Vec<crate::ui::text::PlacedGlyph> {
    let text = &layout.shaped.text;
    let mut bytes: Vec<usize> = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        for _ in 0..c.len_utf16() {
            bytes.push(i);
        }
    }
    bytes.push(text.len());
    let (mut at, mut placed) = (Vec::new(), Vec::new());
    for run in runs_of(layout) {
        let mut pen = run.x;
        let mut last = None;
        for i in 0..run.ids.len() {
            let advance = run.advances[i];
            let o = run.offsets[i];
            if run.rtl {
                pen -= advance;
            }
            let x = if run.rtl { pen - o.advanceOffset } else { pen + o.advanceOffset };
            // (A glyph after a cluster's first shows nothing of its own.)
            let start = run.at[i].or(last).map_or(text.len(), |u| bytes.get(u as usize).copied().unwrap_or(text.len()));
            last = run.at[i].or(last);
            at.push(start);
            placed.push((x as f64, (run.y - o.ascenderOffset) as f64, advance as f64));
            if !run.rtl {
                pen += advance;
            }
        }
    }
    let ranges = crate::ui::text::cluster_ranges(&at, text.len());
    placed
        .into_iter()
        .zip(ranges)
        .map(|((x, y, advance), mut range)| {
            // (A line break is no glyph's text.)
            while range.end > range.start && matches!(text.as_bytes()[range.end - 1], b'\n' | b'\r') {
                range.end -= 1;
            }
            crate::ui::text::PlacedGlyph { x, y, advance, text: range }
        })
        .collect()
}

/// Takes the outline DirectWrite gives a run into a path.
#[implement(windows::Win32::Graphics::Direct2D::Common::ID2D1SimplifiedGeometrySink)]
struct OutlineSink {
    path: Rc<RefCell<kurbo::BezPath>>,
    x: f64,
    y: f64,
}

impl windows::Win32::Graphics::Direct2D::Common::ID2D1SimplifiedGeometrySink_Impl for OutlineSink_Impl {
    fn SetFillMode(&self, _: windows::Win32::Graphics::Direct2D::Common::D2D1_FILL_MODE) {}

    fn SetSegmentFlags(&self, _: windows::Win32::Graphics::Direct2D::Common::D2D1_PATH_SEGMENT) {}

    fn BeginFigure(&self, start: &Vector2, _: windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_BEGIN) {
        self.path.borrow_mut().move_to((self.x + start.X as f64, self.y + start.Y as f64));
    }

    fn AddLines(&self, points: *const Vector2, count: u32) {
        let mut path = self.path.borrow_mut();
        for p in unsafe { items(points, count as usize) } {
            path.line_to((self.x + p.X as f64, self.y + p.Y as f64));
        }
    }

    fn AddBeziers(&self, beziers: *const windows::Win32::Graphics::Direct2D::Common::D2D1_BEZIER_SEGMENT, count: u32) {
        let mut path = self.path.borrow_mut();
        let at = |p: &Vector2| (self.x + p.X as f64, self.y + p.Y as f64);
        for b in unsafe { items(beziers, count as usize) } {
            path.curve_to(at(&b.point1), at(&b.point2), at(&b.point3));
        }
    }

    fn EndFigure(&self, _: windows::Win32::Graphics::Direct2D::Common::D2D1_FIGURE_END) {
        self.path.borrow_mut().close_path();
    }

    fn Close(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// The outline of a layout's glyphs, in its coordinates (y down from its
/// top): the font's own curves, at the font's weight and slant.
pub fn outline(layout: &TextLayout) -> kurbo::BezPath {
    let path = Rc::new(RefCell::new(kurbo::BezPath::new()));
    for run in runs_of(layout) {
        let sink: windows::Win32::Graphics::Direct2D::Common::ID2D1SimplifiedGeometrySink =
            OutlineSink { path: path.clone(), x: run.x as f64, y: run.y as f64 }.into();
        unsafe {
            let _ = run.face.GetGlyphRunOutline(
                run.size,
                run.ids.as_ptr(),
                Some(run.advances.as_ptr()),
                Some(run.offsets.as_ptr()),
                run.ids.len() as u32,
                run.sideways,
                run.rtl,
                &sink,
            );
        }
    }
    path.take()
}

/// The metrics of the font a layout's text starts in, at its size.
pub fn metrics(layout: &TextLayout) -> Option<crate::ui::text::FontMetrics> {
    let run = runs_of(layout).into_iter().next()?;
    let mut m = DWRITE_FONT_METRICS::default();
    unsafe { run.face.GetMetrics(&mut m) };
    let scale = run.size as f64 / m.designUnitsPerEm.max(1) as f64;
    Some(crate::ui::text::FontMetrics {
        ascent: m.ascent as f64 * scale,
        descent: m.descent as f64 * scale,
        line_gap: m.lineGap as f64 * scale,
        cap_height: m.capHeight as f64 * scale,
        x_height: m.xHeight as f64 * scale,
    })
}

/// A localized name, in English if there's one.
fn english(names: &IDWriteLocalizedStrings) -> Option<String> {
    unsafe {
        let locale = wide("en-us");
        let (mut at, mut exists) = (0u32, windows::core::BOOL(0));
        if names.FindLocaleName(PCWSTR(locale.as_ptr()), &mut at, &mut exists).is_err() || !exists.as_bool() {
            at = 0;
        }
        let len = names.GetStringLength(at).ok()?;
        let mut buf = vec![0u16; len as usize + 1];
        names.GetString(at, &mut buf).ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The faces of the family `name`, lightest first.
pub fn styles(name: &str) -> Vec<crate::ui::text::FontStyle> {
    let mut out: Vec<crate::ui::text::FontStyle> = Vec::new();
    let Some(c) = system_fonts() else { return out };
    unsafe {
        let wanted = wide(name.trim());
        let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
        if c.FindFamilyName(PCWSTR(wanted.as_ptr()), &mut index, &mut exists).is_err() || !exists.as_bool() {
            return out;
        }
        let Ok(family) = c.GetFontFamily(index) else { return out };
        for i in 0..family.GetFontCount() {
            let Ok(font) = family.GetFont(i) else { continue };
            let Some(style) = font.GetFaceNames().ok().and_then(|n| english(&n)) else { continue };
            let italic = font.GetStyle() != DWRITE_FONT_STYLE_NORMAL;
            let weight = font.GetWeight().0 as f32;
            if !out.iter().any(|o| o.name == style) {
                out.push(crate::ui::text::FontStyle { name: style, weight, italic });
            }
        }
    }
    out.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)));
    out
}
