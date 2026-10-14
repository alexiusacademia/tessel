//! Writing PDF documents (`writePDF`) from drawings, for renderers whose
//! system has no PDF writer to draw into (Windows: macOS's Core Graphics has
//! one, see `mac::write_pdf`).
//!
//! Each page's drawing becomes a content stream: shapes as paths, text as
//! glyphs of the fonts it was laid out in (in the file, with only the glyphs
//! it uses, and what text each glyph shows, so it can be selected and
//! searched), pictures as compressed images, clips as clipping paths, and
//! layers with an opacity or a blend mode as transparency groups. The
//! renderer tells what only it knows (`Source`): the glyphs of a text
//! layout, and the pixels of a picture.
//!
//! Pages are drawn with their y axis going down, from the top left corner,
//! as drawings are: each page's content starts by flipping PDF's.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::rc::Rc;

use kurbo::{Affine, BezPath, Cap, Join, PathEl, Stroke, StrokeOpts};
use pdf_writer::types::{BlendMode, CidFontType, FontFlags, FunctionShadingType, LineCapStyle, LineJoinStyle, SystemInfo, TextRenderingMode, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use super::draw::{Blend, DrawList, FillRule, Gradient, Op};
use super::image::Picture;
use super::text::TextLayout;
use super::Rgba;

/// A font file (or one font of a collection), which glyphs are set in.
pub struct FontFile {
    /// Tells fonts apart: the same font gives the same key.
    pub key: String,
    pub data: Rc<Vec<u8>>,
    /// Which font of a collection (`.ttc`); 0 otherwise.
    pub index: u32,
}

/// A glyph of a text layout, placed in the layout's coordinates.
pub struct PlacedGlyph {
    pub font: Rc<FontFile>,
    /// The glyph's number in its font.
    pub id: u16,
    pub size: f32,
    /// Its origin, on the baseline.
    pub x: f32,
    pub y: f32,
    /// How far it moves the next glyph along, at `size`.
    pub advance: f32,
    /// The text it shows (empty for a glyph that shows part of another's).
    pub text: String,
    pub color: Rgba,
    /// How far it leans (a font with no italic, slanted for one): across
    /// for each unit up.
    pub slant: f32,
}

/// What only the renderer knows.
pub trait Source {
    /// The glyphs a layout is drawn with.
    fn glyphs(&self, layout: &TextLayout) -> Vec<PlacedGlyph>;
    /// A picture's pixels: red, green, blue and opacity (not multiplied by
    /// it), row by row from the top; with its width and height.
    fn pixels(&self, picture: &Picture) -> Option<(u32, u32, Vec<u8>)>;
    /// A layout as shapes, each with its color, for a renderer whose fonts
    /// can't go into the file as they are (fonts that vary: one file for
    /// every weight). The text is then drawn, and can't be selected.
    fn outlines(&self, _layout: &TextLayout) -> Option<Vec<(BezPath, Rgba)>> {
        None
    }
}

/// Writes pages (each drawn in points, from its top left corner) as a PDF
/// of `width` × `height` points each. Whether it was written.
pub fn write(path: &str, title: &str, width: f64, height: f64, pages: &[DrawList], source: &dyn Source) -> bool {
    let bytes = document(title, width, height, pages, source);
    std::fs::write(path, bytes).is_ok()
}

/// The bytes of the PDF.
pub fn document(title: &str, width: f64, height: f64, pages: &[DrawList], source: &dyn Source) -> Vec<u8> {
    let mut doc = Doc::new(source);
    let catalog = doc.alloc();
    let tree = doc.alloc();
    let info = doc.alloc();
    let mut ids = Vec::new();
    for page in pages {
        let id = doc.alloc();
        let contents = doc.alloc();
        let mut writer = PageWriter { doc: &mut doc, frames: vec![Frame::new(None)] };
        // PDF's y axis goes up from the bottom; a drawing's goes down.
        writer.content().transform([1.0, 0.0, 0.0, -1.0, 0.0, height as f32]);
        writer.list(page, Affine::IDENTITY, width, height);
        while writer.frames.len() > 1 {
            writer.pop(width, height);
        }
        let content = writer.frames.pop().unwrap().content.finish();
        let resources = doc.resources;
        doc.pdf.stream(contents, &deflate(&content)).filter(Filter::FlateDecode);
        let mut p = doc.pdf.page(id);
        p.media_box(Rect::new(0.0, 0.0, width as f32, height as f32)).parent(tree).contents(contents);
        p.pair(Name(b"Resources"), resources);
        p.finish();
        ids.push(id);
    }
    doc.pdf.pages(tree).kids(ids.iter().copied()).count(ids.len() as i32);
    doc.pdf.catalog(catalog).pages(tree);
    let mut i = doc.pdf.document_info(info);
    if !title.is_empty() {
        i.title(TextStr(title));
    }
    i.creator(TextStr("Tessel"));
    i.finish();
    doc.finish()
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(bytes);
    e.finish().unwrap_or_default()
}

/// A font as it's used in the document.
struct UsedFont {
    name: String,
    file: Rc<FontFile>,
    remapper: subsetter::GlyphRemapper,
    /// By new glyph number: its width (thousandths of the size) and text.
    glyphs: BTreeMap<u16, (f32, String)>,
}

struct Doc<'s> {
    pdf: Pdf,
    next: i32,
    source: &'s dyn Source,
    /// One dictionary of everything the pages and groups use.
    resources: Ref,
    fonts: HashMap<String, UsedFont>,
    font_order: Vec<String>,
    /// Graphics states by what they set: name and object.
    states: HashMap<(u32, u32, u8), (String, Ref)>,
    /// Images (by the picture's address, which is kept alive with it).
    images: HashMap<usize, (String, Ref, Rc<Picture>)>,
    xobjects: Vec<(String, Ref)>,
    shadings: Vec<(String, Ref)>,
}

impl<'s> Doc<'s> {
    fn new(source: &'s dyn Source) -> Doc<'s> {
        let mut pdf = Pdf::new();
        pdf.set_version(1, 7);
        Doc {
            pdf,
            next: 2,
            source,
            resources: Ref::new(1),
            fonts: HashMap::new(),
            font_order: Vec::new(),
            states: HashMap::new(),
            images: HashMap::new(),
            xobjects: Vec::new(),
            shadings: Vec::new(),
        }
    }

    fn alloc(&mut self) -> Ref {
        self.next += 1;
        Ref::new(self.next - 1)
    }

    /// A graphics state with these opacities (fill, stroke) and blend mode.
    fn state(&mut self, fill: f32, stroke: f32, blend: Blend) -> String {
        let key = (fill.to_bits(), stroke.to_bits(), blend);
        if let Some((name, _)) = self.states.get(&key) {
            return name.clone();
        }
        let id = self.alloc();
        let name = format!("G{}", self.states.len());
        let mut s = self.pdf.ext_graphics(id);
        s.non_stroking_alpha(fill).stroking_alpha(stroke);
        if let Some(mode) = blend_mode(blend) {
            s.blend_mode(mode);
        }
        s.finish();
        self.states.insert(key, (name.clone(), id));
        name
    }

    /// A picture's image object, made the first time.
    fn image(&mut self, picture: &Rc<Picture>) -> Option<String> {
        let key = Rc::as_ptr(picture) as usize;
        if let Some((name, ..)) = self.images.get(&key) {
            return Some(name.clone());
        }
        let (w, h, rgba) = self.source.pixels(picture)?;
        let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let alpha: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
        let id = self.alloc();
        let mask = if alpha.iter().any(|a| *a < 255) {
            let mask = self.alloc();
            let data = deflate(&alpha);
            let mut m = self.pdf.image_xobject(mask, &data);
            m.width(w as i32).height(h as i32).bits_per_component(8);
        m.filter(Filter::FlateDecode);
            m.color_space().device_gray();
            m.finish();
            Some(mask)
        } else {
            None
        };
        let data = deflate(&rgb);
        let mut image = self.pdf.image_xobject(id, &data);
        image.width(w as i32).height(h as i32).bits_per_component(8);
        image.filter(Filter::FlateDecode);
        image.color_space().device_rgb();
        if let Some(mask) = mask {
            image.s_mask(mask);
        }
        image.finish();
        let name = format!("I{}", self.images.len());
        self.images.insert(key, (name.clone(), id, picture.clone()));
        self.xobjects.push((name.clone(), id));
        Some(name)
    }

    /// A gradient as a shading, in the gradient's own coordinates.
    fn shading(&mut self, gradient: &Gradient) -> Option<String> {
        let stops = match gradient {
            Gradient::Linear { stops, .. } | Gradient::Radial { stops, .. } => stops,
        };
        let mut stops: Vec<(f32, Rgba)> = stops.iter().map(|(at, c)| (at.clamp(0.0, 1.0), *c)).collect();
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (first, last) = (*stops.first()?, *stops.last()?);
        if first.0 > 0.0 {
            stops.insert(0, (0.0, first.1));
        }
        if last.0 < 1.0 {
            stops.push((1.0, last.1));
        }
        let rgb = |c: Rgba| c.0[..3].iter().map(|v| *v as f32 / 255.0).collect::<Vec<f32>>();
        let function = self.alloc();
        if stops.len() == 2 {
            self.pdf.exponential_function(function).domain([0.0, 1.0]).c0(rgb(stops[0].1)).c1(rgb(stops[1].1)).n(1.0);
        } else {
            let mut parts = Vec::new();
            for pair in stops.windows(2) {
                let part = self.alloc();
                self.pdf.exponential_function(part).domain([0.0, 1.0]).c0(rgb(pair[0].1)).c1(rgb(pair[1].1)).n(1.0);
                parts.push(part);
            }
            let bounds: Vec<f32> = stops[1..stops.len() - 1].iter().map(|s| s.0).collect();
            let encode: Vec<f32> = parts.iter().flat_map(|_| [0.0, 1.0]).collect();
            self.pdf.stitching_function(function).domain([0.0, 1.0]).functions(parts).bounds(bounds).encode(encode);
        }
        let id = self.alloc();
        let mut s = self.pdf.function_shading(id);
        match gradient {
            Gradient::Linear { start, end, .. } => {
                s.shading_type(FunctionShadingType::Axial);
                s.coords([start.x as f32, start.y as f32, end.x as f32, end.y as f32]);
            }
            Gradient::Radial { center, radius, .. } => {
                s.shading_type(FunctionShadingType::Radial);
                let (x, y) = (center.x as f32, center.y as f32);
                s.coords([x, y, 0.0, x, y, *radius as f32]);
            }
        }
        s.color_space().device_rgb();
        s.function(function).extend([true, true]);
        s.finish();
        let name = format!("S{}", self.shadings.len());
        self.shadings.push((name.clone(), id));
        Some(name)
    }

    /// The name of a font in the content, and the number its glyph `id`
    /// has there (its number in the font made of the glyphs used).
    fn glyph(&mut self, g: &PlacedGlyph) -> (String, u16) {
        let key = g.font.key.clone();
        if !self.fonts.contains_key(&key) {
            let name = format!("F{}", self.fonts.len());
            let mut remapper = subsetter::GlyphRemapper::new();
            remapper.remap(0);
            self.fonts.insert(key.clone(), UsedFont { name, file: g.font.clone(), remapper, glyphs: BTreeMap::new() });
            self.font_order.push(key.clone());
        }
        let font = self.fonts.get_mut(&key).unwrap();
        let cid = font.remapper.remap(g.id);
        let width = if g.size > 0.0 { g.advance / g.size * 1000.0 } else { 0.0 };
        let entry = font.glyphs.entry(cid).or_insert((width, String::new()));
        if entry.1.is_empty() && !g.text.is_empty() {
            entry.1 = g.text.clone();
        }
        (font.name.clone(), cid)
    }

    /// Writes the fonts and the resources, and returns the file's bytes.
    fn finish(mut self) -> Vec<u8> {
        let mut fonts = Vec::new();
        for key in std::mem::take(&mut self.font_order) {
            let font = self.fonts.remove(&key).unwrap();
            if let Some(id) = self.write_font(&font) {
                fonts.push((font.name.clone(), id));
            }
        }
        let mut r = self.pdf.indirect(self.resources).start::<pdf_writer::writers::Resources>();
        let mut f = r.fonts();
        for (name, id) in &fonts {
            f.pair(Name(name.as_bytes()), *id);
        }
        f.finish();
        let mut x = r.x_objects();
        for (name, id) in &self.xobjects {
            x.pair(Name(name.as_bytes()), *id);
        }
        x.finish();
        let mut g = r.ext_g_states();
        for (name, id) in self.states.values() {
            g.pair(Name(name.as_bytes()), *id);
        }
        g.finish();
        let mut s = r.shadings();
        for (name, id) in &self.shadings {
            s.pair(Name(name.as_bytes()), *id);
        }
        s.finish();
        r.finish();
        self.pdf.finish()
    }

    /// A font with the glyphs used of it, as a CID font (Identity-H: each
    /// glyph shown by its number in the subset).
    fn write_font(&mut self, font: &UsedFont) -> Option<Ref> {
        let subset = subsetter::subset(&font.file.data, font.file.index, &font.remapper).ok()?;
        let metrics = Metrics::read(&font.file.data, font.file.index).unwrap_or_default();
        let cff = has_table(&subset, b"CFF ");
        let (type0, cid, descriptor, file, cmap) = (self.alloc(), self.alloc(), self.alloc(), self.alloc(), self.alloc());
        // A subset's name starts with six capital letters and a +.
        let tag: String = font.name.bytes().chain(std::iter::repeat(b'A')).take(6).map(|b| (b'A' + b % 26) as char).collect();
        let base = format!("{tag}+{}", metrics.name.as_deref().unwrap_or("Font"));
        let base = Name(base.as_bytes());
        self.pdf.type0_font(type0).base_font(base).encoding_predefined(Name(b"Identity-H")).descendant_font(cid).to_unicode(cmap);
        let info = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };
        let mut c = self.pdf.cid_font(cid);
        c.subtype(if cff { CidFontType::Type0 } else { CidFontType::Type2 });
        c.base_font(base).system_info(info).font_descriptor(descriptor).default_width(0.0);
        if !cff {
            c.cid_to_gid_map_predefined(Name(b"Identity"));
        }
        let mut w = c.widths();
        for (cid, (width, _)) in &font.glyphs {
            w.consecutive(*cid, [*width]);
        }
        w.finish();
        c.finish();
        let scale = 1000.0 / metrics.units_per_em.max(1) as f32;
        let [x0, y0, x1, y1] = metrics.bbox.map(|v| v as f32 * scale);
        let mut d = self.pdf.font_descriptor(descriptor);
        d.name(base).flags(FontFlags::SYMBOLIC).bbox(Rect::new(x0, y0, x1, y1)).italic_angle(0.0);
        d.ascent(metrics.ascender as f32 * scale).descent(metrics.descender as f32 * scale);
        d.cap_height(metrics.cap_height as f32 * scale).stem_v(80.0);
        if cff {
            d.font_file3(file);
        } else {
            d.font_file2(file);
        }
        d.finish();
        let packed = deflate(&subset);
        let mut s = self.pdf.stream(file, &packed);
        s.filter(Filter::FlateDecode);
        if cff {
            s.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
        s.finish();
        let mut map = UnicodeCmap::new(Name(b"Custom"), info);
        for (cid, (_, text)) in &font.glyphs {
            if !text.is_empty() {
                map.pair_with_multiple(*cid, text.chars());
            }
        }
        let map = map.finish();
        self.pdf.cmap(cmap, &deflate(map.as_slice())).filter(Filter::FlateDecode);
        Some(type0)
    }
}

/// PDF's blend mode for a layer's `BlendMode` (`None`: normal). Tessel's
/// cases are in PDF's order.
fn blend_mode(blend: Blend) -> Option<BlendMode> {
    Some(match blend {
        1 => BlendMode::Multiply,
        2 => BlendMode::Screen,
        3 => BlendMode::Overlay,
        4 => BlendMode::Darken,
        5 => BlendMode::Lighten,
        6 => BlendMode::ColorDodge,
        7 => BlendMode::ColorBurn,
        8 => BlendMode::HardLight,
        9 => BlendMode::SoftLight,
        10 => BlendMode::Difference,
        11 => BlendMode::Exclusion,
        12 => BlendMode::Hue,
        13 => BlendMode::Saturation,
        14 => BlendMode::Color,
        15 => BlendMode::Luminosity,
        _ => return None,
    })
}

/// A layer drawn by itself (into its own content), then put onto what's
/// under it inside `clip`, at `alpha`, blended by `blend`.
struct Group {
    clip: BezPath,
    rule: FillRule,
    alpha: f32,
    blend: Blend,
}

/// Content being written: the page's, or a layer's (`group`).
struct Frame {
    content: Content,
    group: Option<Group>,
    /// The clips open in this content (each a saved state, restored by its
    /// `Pop`).
    clips: usize,
}

impl Frame {
    fn new(group: Option<Group>) -> Frame {
        Frame { content: Content::new(), group, clips: 0 }
    }
}

struct PageWriter<'d, 's> {
    doc: &'d mut Doc<'s>,
    frames: Vec<Frame>,
}

fn matrix(a: Affine) -> [f32; 6] {
    a.as_coeffs().map(|v| v as f32)
}

fn path_into(c: &mut Content, path: &BezPath) {
    let mut last = kurbo::Point::ZERO;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                c.move_to(p.x as f32, p.y as f32);
                last = p;
            }
            PathEl::LineTo(p) => {
                c.line_to(p.x as f32, p.y as f32);
                last = p;
            }
            PathEl::QuadTo(q, p) => {
                let c1 = last + (q - last) * (2.0 / 3.0);
                let c2 = p + (q - p) * (2.0 / 3.0);
                c.cubic_to(c1.x as f32, c1.y as f32, c2.x as f32, c2.y as f32, p.x as f32, p.y as f32);
                last = p;
            }
            PathEl::CurveTo(a, b, p) => {
                c.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32);
                last = p;
            }
            PathEl::ClosePath => {
                c.close_path();
            }
        }
    }
}

fn rgb(c: Rgba) -> (f32, f32, f32) {
    let [r, g, b, _] = c.0.map(|v| v as f32 / 255.0);
    (r, g, b)
}

fn alpha(c: Rgba) -> f32 {
    c.0[3] as f32 / 255.0
}

impl PageWriter<'_, '_> {
    fn content(&mut self) -> &mut Content {
        &mut self.frames.last_mut().unwrap().content
    }

    /// Sets the opacities (and blend mode) to draw with.
    fn set_state(&mut self, fill: f32, stroke: f32, blend: Blend) {
        let name = self.doc.state(fill, stroke, blend);
        self.content().set_parameters(Name(name.as_bytes()));
    }

    fn list(&mut self, list: &DrawList, base: Affine, width: f64, height: f64) {
        for op in &list.ops {
            self.op(op, base, width, height);
        }
    }

    fn op(&mut self, op: &Op, base: Affine, width: f64, height: f64) {
        match op {
            Op::Fill { path, rule, color, transform: t } => {
                if alpha(*color) <= 0.0 {
                    return;
                }
                let mut p = path.clone();
                p.apply_affine(base * *t);
                let (r, g, b) = rgb(*color);
                self.content().save_state();
                self.set_state(alpha(*color), 1.0, 0);
                let c = self.content();
                c.set_fill_rgb(r, g, b);
                path_into(c, &p);
                if *rule == FillRule::EvenOdd {
                    c.fill_even_odd();
                } else {
                    c.fill_nonzero();
                }
                c.restore_state();
            }
            Op::Stroke { path, style, color, transform: t } => {
                if alpha(*color) <= 0.0 {
                    return;
                }
                let (r, g, b) = rgb(*color);
                self.content().save_state();
                self.set_state(1.0, alpha(*color), 0);
                let c = self.content();
                c.transform(matrix(base * *t));
                stroke_style(c, style);
                c.set_stroke_rgb(r, g, b);
                path_into(c, path);
                c.stroke();
                c.restore_state();
            }
            Op::FillGradient { path, rule, gradient, place, transform: t } => {
                let mut p = path.clone();
                p.apply_affine(base * *t);
                self.gradient(&p, *rule, gradient, base * *t * *place);
            }
            Op::StrokeGradient { path, style, gradient, place, transform: t } => {
                let mut outline = kurbo::stroke(path.iter(), style, &StrokeOpts::default(), 0.05);
                outline.apply_affine(base * *t);
                self.gradient(&outline, FillRule::NonZero, gradient, base * *t * *place);
            }
            Op::Push { clip, rule, alpha, blend, transform: t } => {
                let mut p = clip.clone();
                p.apply_affine(base * *t);
                if *alpha >= 1.0 && blend_mode(*blend).is_none() {
                    let c = self.content();
                    c.save_state();
                    clip_into(c, &p, *rule);
                    self.frames.last_mut().unwrap().clips += 1;
                } else {
                    self.frames.push(Frame::new(Some(Group { clip: p, rule: *rule, alpha: *alpha, blend: *blend })));
                }
            }
            Op::Pop => self.pop(width, height),
            Op::Text { layout, transform: t, tint, halo } => self.text(layout, base * *t, *tint, *halo),
            Op::Image { picture, transform: t, alpha } => {
                let Some(name) = self.doc.image(picture) else { return };
                let (w, h) = (picture.width, picture.height);
                self.content().save_state();
                self.set_state(*alpha, *alpha, 0);
                let c = self.content();
                // An image fills the unit square, its first row at the top.
                c.transform(matrix(base * *t * Affine::new([w, 0.0, 0.0, -h, 0.0, h])));
                c.x_object(Name(name.as_bytes()));
                c.restore_state();
            }
            Op::Svg { svg, transform: t } => self.list(&svg.scene, base * *t, width, height),
            Op::List { list, transform: t } => self.list(list, base * *t, width, height),
        }
    }

    /// Closes what the last `Push` opened.
    fn pop(&mut self, width: f64, height: f64) {
        // A clip in this frame first.
        let frame = self.frames.last_mut().unwrap();
        if frame.clips > 0 {
            frame.clips -= 1;
            frame.content.restore_state();
            return;
        }
        if self.frames.len() < 2 {
            return;
        }
        let frame = self.frames.pop().unwrap();
        let Some(Group { clip, rule, alpha, blend }) = frame.group else { return };
        let content = frame.content.finish();
        let id = self.doc.alloc();
        let data = deflate(&content);
        let mut form = self.doc.pdf.form_xobject(id, &data);
        form.filter(Filter::FlateDecode);
        form.bbox(Rect::new(-1.0e5, -1.0e5, 1.0e5, 1.0e5));
        form.pair(Name(b"Resources"), self.doc.resources);
        // (Not isolated: a blend mode blends with what's under the group.)
        form.group().transparency().isolated(false).knockout(false);
        form.finish();
        let name = format!("X{}", self.doc.xobjects.len());
        self.doc.xobjects.push((name.clone(), id));
        let _ = (width, height);
        self.content().save_state();
        clip_into(self.content(), &clip, rule);
        self.set_state(alpha, alpha, blend);
        let c = self.content();
        c.x_object(Name(name.as_bytes()));
        c.restore_state();
    }

    fn gradient(&mut self, area: &BezPath, rule: FillRule, gradient: &Gradient, place: Affine) {
        let Some(name) = self.doc.shading(gradient) else { return };
        let c = self.content();
        c.save_state();
        clip_into(c, area, rule);
        c.transform(matrix(place));
        c.shading(Name(name.as_bytes()));
        c.restore_state();
    }

    fn text(&mut self, layout: &TextLayout, t: Affine, tint: Option<Rgba>, halo: Option<(Rgba, f64)>) {
        if let Some(shapes) = self.doc.source.outlines(layout) {
            self.content().save_state();
            self.content().transform(matrix(t));
            if let Some((color, width)) = halo {
                let (r, g, b) = rgb(color);
                let c = self.content();
                c.set_stroke_rgb(r, g, b);
                c.set_line_width(width as f32);
                c.set_line_join(LineJoinStyle::RoundJoin);
                for (shape, _) in &shapes {
                    path_into(c, shape);
                    c.stroke();
                }
            }
            for (shape, color) in &shapes {
                let color = tint.unwrap_or(*color);
                if alpha(color) <= 0.0 {
                    continue;
                }
                let (r, g, b) = rgb(color);
                self.content().save_state();
                self.set_state(alpha(color), 1.0, 0);
                let c = self.content();
                c.set_fill_rgb(r, g, b);
                path_into(c, shape);
                c.fill_nonzero();
                c.restore_state();
            }
            self.content().restore_state();
            return;
        }
        let glyphs = self.doc.source.glyphs(layout);
        if glyphs.is_empty() {
            return;
        }
        let shown: Vec<(String, u16, &PlacedGlyph)> = glyphs.iter().map(|g| {
            let (font, cid) = self.doc.glyph(g);
            (font, cid, g)
        }).collect();
        self.content().save_state();
        self.content().transform(matrix(t));
        let pass = |w: &mut Self, color: Option<Rgba>, stroke: Option<f64>| {
            let c = w.content();
            c.begin_text();
            // (The mode stays set after the text: it's set each time.)
            match stroke {
                Some(width) => {
                    c.set_text_rendering_mode(TextRenderingMode::Stroke);
                    c.set_line_width(width as f32);
                    c.set_line_join(LineJoinStyle::RoundJoin);
                }
                None => {
                    c.set_text_rendering_mode(TextRenderingMode::Fill);
                }
            }
            let mut current: Option<(String, f32, Rgba)> = None;
            for (font, cid, g) in &shown {
                let color = color.unwrap_or(g.color);
                if current.as_ref().is_none_or(|(f, s, c)| f != font || *s != g.size || *c != color) {
                    let (r, gr, b) = rgb(color);
                    if stroke.is_some() {
                        c.set_stroke_rgb(r, gr, b);
                    } else {
                        c.set_fill_rgb(r, gr, b);
                    }
                    c.set_font(Name(font.as_bytes()), 1.0);
                    current = Some((font.clone(), g.size, color));
                }
                // (Up is down here: the text is flipped back.)
                c.set_text_matrix([g.size, 0.0, g.slant * g.size, -g.size, g.x, g.y]);
                c.show(Str(&cid.to_be_bytes()));
            }
            c.end_text();
        };
        if let Some((color, width)) = halo {
            pass(self, Some(color), Some(width));
        }
        pass(self, tint, None);
        self.content().restore_state();
    }
}

fn clip_into(c: &mut Content, path: &BezPath, rule: FillRule) {
    path_into(c, path);
    if rule == FillRule::EvenOdd {
        c.clip_even_odd();
    } else {
        c.clip_nonzero();
    }
    c.end_path();
}

fn stroke_style(c: &mut Content, s: &Stroke) {
    c.set_line_width(s.width as f32);
    c.set_line_cap(match s.start_cap {
        Cap::Butt => LineCapStyle::ButtCap,
        Cap::Round => LineCapStyle::RoundCap,
        Cap::Square => LineCapStyle::ProjectingSquareCap,
    });
    c.set_line_join(match s.join {
        Join::Miter => LineJoinStyle::MiterJoin,
        Join::Round => LineJoinStyle::RoundJoin,
        Join::Bevel => LineJoinStyle::BevelJoin,
    });
    c.set_miter_limit(s.miter_limit as f32);
    if !s.dash_pattern.is_empty() {
        c.set_dash_pattern(s.dash_pattern.iter().map(|d| *d as f32), s.dash_offset as f32);
    }
}

/// What the font descriptor tells of a font, read from its tables.
#[derive(Default)]
struct Metrics {
    units_per_em: u16,
    bbox: [i16; 4],
    ascender: i16,
    descender: i16,
    cap_height: i16,
    name: Option<String>,
}

/// The tables of the font at `index` of a font file: (tag, offset, length).
fn tables(data: &[u8], index: u32) -> Option<Vec<([u8; 4], usize, usize)>> {
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let u16_at = |at: usize| data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let start = if data.get(0..4)? == b"ttcf" { u32_at(12 + 4 * index as usize)? } else { 0 };
    let count = u16_at(start + 4)?;
    let mut out = Vec::new();
    for i in 0..count {
        let at = start + 12 + 16 * i;
        let tag: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        out.push((tag, u32_at(at + 8)?, u32_at(at + 12)?));
    }
    Some(out)
}

fn has_table(data: &[u8], tag: &[u8; 4]) -> bool {
    tables(data, 0).is_some_and(|t| t.iter().any(|(t, ..)| t == tag))
}

impl Metrics {
    fn read(data: &[u8], index: u32) -> Option<Metrics> {
        let tables = tables(data, index)?;
        let table = |tag: &[u8; 4]| tables.iter().find(|(t, ..)| t == tag).and_then(|(_, at, len)| data.get(*at..*at + *len));
        let i16_at = |t: &[u8], at: usize| t.get(at..at + 2).map(|b| i16::from_be_bytes([b[0], b[1]]));
        let head = table(b"head")?;
        let hhea = table(b"hhea")?;
        let mut m = Metrics {
            units_per_em: i16_at(head, 18)? as u16,
            bbox: [i16_at(head, 36)?, i16_at(head, 38)?, i16_at(head, 40)?, i16_at(head, 42)?],
            ascender: i16_at(hhea, 4)?,
            descender: i16_at(hhea, 6)?,
            cap_height: 0,
            name: None,
        };
        m.cap_height = table(b"OS/2").and_then(|t| (t.len() >= 90).then(|| i16_at(t, 88)).flatten()).unwrap_or(m.ascender * 7 / 10);
        m.name = table(b"name").and_then(postscript_name);
        Some(m)
    }
}

/// The font's PostScript name (name 6), in Windows' or the Mac's encoding.
fn postscript_name(t: &[u8]) -> Option<String> {
    let u16_at = |at: usize| t.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let (count, strings) = (u16_at(2)?, u16_at(4)?);
    for i in 0..count {
        let r = 6 + 12 * i;
        let (platform, name, len, at) = (u16_at(r)?, u16_at(r + 6)?, u16_at(r + 8)?, u16_at(r + 10)?);
        if name != 6 {
            continue;
        }
        let bytes = t.get(strings + at..strings + at + len)?;
        let text = if platform == 3 || platform == 0 {
            String::from_utf16(&bytes.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>()).ok()?
        } else {
            bytes.iter().map(|b| *b as char).collect()
        };
        // (A PDF name can't have spaces or delimiters.)
        let clean: String = text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if !clean.is_empty() {
            return Some(clean);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use kurbo::{Point, Rect as KRect, Shape};

    use super::*;

    /// Glyphs from a font file, one after another (no shaping), for the
    /// texts the test lays out.
    struct TestSource {
        font: Rc<FontFile>,
        texts: RefCell<HashMap<usize, (String, f32)>>,
        pixels: Vec<u8>,
    }

    impl Source for TestSource {
        fn glyphs(&self, layout: &TextLayout) -> Vec<PlacedGlyph> {
            let texts = self.texts.borrow();
            let Some((text, size)) = texts.get(&(layout as *const TextLayout as usize)) else { return Vec::new() };
            let face = ttf_parser::Face::parse(&self.font.data, self.font.index).unwrap();
            let scale = size / face.units_per_em() as f32;
            let mut x = 0.0;
            text.chars()
                .map(|ch| {
                    let id = face.glyph_index(ch).map_or(0, |g| g.0);
                    let advance = face.glyph_hor_advance(ttf_parser::GlyphId(id)).unwrap_or(0) as f32 * scale;
                    let g = PlacedGlyph {
                        font: self.font.clone(),
                        id,
                        size: *size,
                        x,
                        y: size * 0.8,
                        advance,
                        text: ch.to_string(),
                        color: Rgba([20, 20, 20, 255]),
                        slant: 0.0,
                    };
                    x += advance;
                    g
                })
                .collect()
        }

        fn pixels(&self, _: &Picture) -> Option<(u32, u32, Vec<u8>)> {
            Some((2, 2, self.pixels.clone()))
        }
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
        KRect::new(x, y, x + w, y + h).to_path(0.1)
    }

    #[test]
    fn writes_a_document() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Helvetica.ttc") else { return };
        let font = Rc::new(FontFile { key: "helvetica".into(), data: Rc::new(data), index: 0 });
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 255, 255, 255, 0];
        let source = TestSource { font, texts: RefCell::new(HashMap::new()), pixels: pixels.clone() };
        let mut engine = super::super::text::TextEngine::new();
        let mut text = |t: &str, size: f32| {
            let layout = Rc::new(engine.layout(t, size as f64, 400.0, Rgba([0, 0, 0, 255]), None));
            source.texts.borrow_mut().insert(Rc::as_ptr(&layout) as usize, (t.to_string(), size));
            layout
        };
        let id = Affine::IDENTITY;
        let mut one = DrawList::new();
        one.ops.push(Op::Text { layout: text("Hello, PDF — ünïcode", 24.0), transform: Affine::translate((40.0, 40.0)), tint: None, halo: None });
        one.ops.push(Op::Fill { path: rect(40.0, 90.0, 200.0, 40.0), rule: FillRule::NonZero, color: Rgba([255, 0, 0, 128]), transform: id });
        let style = Stroke::new(3.0).with_dashes(0.0, [6.0, 4.0]);
        one.ops.push(Op::Stroke { path: rect(40.0, 150.0, 200.0, 40.0), style, color: Rgba([0, 0, 255, 255]), transform: id });
        let gradient = Gradient::Linear {
            start: Point::new(0.0, 0.0),
            end: Point::new(200.0, 0.0),
            stops: vec![(0.0, Rgba([255, 0, 0, 255])), (0.5, Rgba([0, 255, 0, 255])), (1.0, Rgba([0, 0, 255, 255]))],
        };
        one.ops.push(Op::FillGradient {
            path: rect(40.0, 210.0, 200.0, 30.0),
            rule: FillRule::NonZero,
            gradient,
            place: Affine::translate((40.0, 0.0)),
            transform: id,
        });
        // A clip, and a layer that multiplies.
        one.ops.push(Op::Push { clip: rect(40.0, 260.0, 100.0, 60.0), rule: FillRule::NonZero, alpha: 1.0, blend: 0, transform: id });
        one.ops.push(Op::Fill { path: rect(0.0, 250.0, 300.0, 100.0), rule: FillRule::NonZero, color: Rgba([255, 255, 0, 255]), transform: id });
        one.ops.push(Op::Push { clip: rect(0.0, 0.0, 400.0, 400.0), rule: FillRule::NonZero, alpha: 1.0, blend: 1, transform: id });
        one.ops.push(Op::Fill { path: rect(80.0, 280.0, 100.0, 60.0), rule: FillRule::NonZero, color: Rgba([0, 255, 255, 255]), transform: id });
        one.ops.push(Op::Pop);
        one.ops.push(Op::Pop);
        let picture = Rc::new(super::super::backend::picture_from_rgba(2, 2, pixels).unwrap());
        one.ops.push(Op::Image { picture, transform: Affine::translate((40.0, 340.0)) * Affine::scale(20.0), alpha: 1.0 });
        let mut two = DrawList::new();
        two.ops.push(Op::Text { layout: text("Page two", 18.0), transform: Affine::translate((40.0, 40.0)), tint: Some(Rgba([200, 0, 0, 255])), halo: Some((Rgba([255, 255, 0, 255]), 2.0)) });
        let bytes = document("Writer test", 300.0, 400.0, &[one, two], &source);
        assert!(bytes.starts_with(b"%PDF-1.7"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2"));
        assert!(text.contains("/FontFile2") || text.contains("/FontFile3"));
        if let Some(path) = std::env::var_os("TESSEL_PDF_TEST_OUT") {
            std::fs::write(path, &bytes).unwrap();
        }
    }
}
