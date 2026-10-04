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
use crate::ui::text::{Line, TextLayout, CODE_LINE};
use crate::ui::theme::{self, theme};
use crate::ui::Rgba;

#[derive(Clone, Copy, PartialEq)]
struct Font {
    mono: bool,
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
}

pub struct TextEngine {
    formats: HashMap<(bool, u64, u32, bool), IDWriteTextFormat>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The monospaced font: Cascadia Mono where there is one (Windows 11), else Consolas.
fn mono_family() -> &'static str {
    static FAMILY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FAMILY.get_or_init(|| unsafe {
        let mut collection = None;
        if factories().dwrite.GetSystemFontCollection(&mut collection, false).is_ok() {
            if let Some(c) = collection {
                let name = wide("Cascadia Mono");
                let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
                if c.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() {
                    return "Cascadia Mono";
                }
            }
        }
        "Consolas"
    })
}

fn family(f: Font) -> &'static str {
    if f.mono { mono_family() } else { "Segoe UI" }
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { formats: HashMap::new() }
    }

    fn format(&mut self, f: Font) -> Option<IDWriteTextFormat> {
        let key = (f.mono, f.size.to_bits(), f.weight.to_bits(), f.italic);
        if let Some(x) = self.formats.get(&key) {
            return Some(x.clone());
        }
        let name = wide(family(f));
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
            height: font.size * 1.2,
            lines: vec![Line { range: 0..text.len(), top: 0.0, bottom: font.size * 1.2, stops: vec![(0, 0.0)], end_x: 0.0 }],
            len: text.len(),
            shaped: Shaped { layout: None, color, colors: Vec::new() },
        };
        let Some(format) = self.format(font) else { return empty(color) };
        let made = unsafe {
            let wrap = max_w.is_some();
            let layout = match factories().dwrite.CreateTextLayout(&utf16, &format, max_w.map_or(1.0e7, |w| w as f32 + 0.5), 1.0e7) {
                Ok(l) => l,
                Err(_) => return empty(color),
            };
            let _ = layout.SetWordWrapping(if wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP });
            let mut colors = Vec::new();
            for span in spans {
                let r = DWRITE_TEXT_RANGE { startPosition: unit(span.range.start), length: unit(span.range.end) - unit(span.range.start) };
                if r.length == 0 {
                    continue;
                }
                if let Some(f) = span.font {
                    if f.mono != font.mono {
                        let n = wide(family(f));
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
            height: m.height as f64,
            lines,
            len: text.len(),
            shaped: Shaped { layout: Some(layout), color, colors },
        }
    }

    /// Code: monospaced, not wrapped, with syntax colors.
    pub fn code_layout(&mut self, text: &str, size: f64) -> TextLayout {
        let spans = colors(crate::ui::highlight::highlight(text));
        let font = Font { mono: true, size, weight: 400.0, italic: false };
        self.build(text, font, theme().colors[theme::PRIMARY], &spans, LineHeight::Absolute(CODE_LINE as f64), None, 0)
    }

    /// Monospaced text, left-aligned, in one color.
    pub fn mono_line(&mut self, text: &str, size: f64, weight: f32, color: Rgba) -> TextLayout {
        self.build(text, Font { mono: true, size, weight, italic: false }, color, &[], LineHeight::Natural, None, 0)
    }

    /// Monospaced gray text (line numbers), on the lines of the code, right-aligned.
    pub fn mono_layout(&mut self, text: &str, size: f64, color: Rgba) -> TextLayout {
        let font = Font { mono: true, size, weight: 400.0, italic: false };
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
        let base = Font { mono: false, size, weight, italic: false };
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
                    font.mono = true;
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
        let font = Font { mono: true, size, weight: 400.0, italic: false };
        self.build(text, font, theme().colors[theme::PRIMARY], &spans, LineHeight::Relative(1.45), Some(width), 0)
    }

    pub fn layout(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>) -> TextLayout {
        self.aligned(text, size, weight, color, max_w, 0)
    }

    /// Like `layout`, with the lines lined up (0 leading, 1 center, 2
    /// trailing) inside the width of the widest.
    pub fn aligned(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>, align: u8) -> TextLayout {
        self.build(text, Font { mono: false, size, weight, italic: false }, color, &[], LineHeight::Natural, max_w, align)
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
