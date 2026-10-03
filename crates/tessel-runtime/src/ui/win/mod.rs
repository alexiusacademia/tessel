//! The Windows renderer (the `native` feature): Direct2D draws, DirectWrite
//! lays text out, and WIC reads and writes pictures. These come with
//! Windows, so an app carries none of them: it's a fraction of the size of
//! one with the GPU renderer.
//!
//! Everything here has a counterpart in the other renderers, with the same
//! names (see `ui::backend`).

pub mod text;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Cap, Join, PathEl, Stroke};
use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows_numerics::{Matrix3x2, Vector2};
use winit::window::Window;

use super::draw::{DrawList, FillRule, Op};
use super::image::Picture;
use super::Rgba;

pub use super::svg_list::svg_scene;

/// A decoded picture, ready to draw: WIC's, in premultiplied BGRA.
pub struct ImageData(IWICBitmapSource);

/// A parsed SVG, ready to draw.
pub type SvgScene = DrawList;

/// The system's drawing, text and picture services, made once.
pub(crate) struct Factories {
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub wic: IWICImagingFactory,
}

thread_local! {
    // Never released: at exit, Windows may unload their libraries before
    // thread-local values are dropped, and releasing them then would crash.
    static FACTORIES: &'static Factories = unsafe {
        // (WIC is a COM object; COM may already be on, in some mode.)
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        Box::leak(Box::new(Factories {
            d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("Direct2D"),
            dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DirectWrite"),
            wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).expect("WIC"),
        }))
    };
}

pub(crate) fn factories() -> &'static Factories {
    FACTORIES.with(|f| *f)
}

fn matrix(a: Affine) -> Matrix3x2 {
    let [a, b, c, d, e, f] = a.as_coeffs();
    Matrix3x2 { M11: a as f32, M12: b as f32, M21: c as f32, M22: d as f32, M31: e as f32, M32: f as f32 }
}

fn color_f(c: Rgba) -> D2D1_COLOR_F {
    let [r, g, b, a] = c.0.map(|v| v as f32 / 255.0);
    D2D1_COLOR_F { r, g, b, a }
}

fn point(p: kurbo::Point) -> Vector2 {
    Vector2 { X: p.x as f32, Y: p.y as f32 }
}

/// A path as a Direct2D geometry.
fn geometry(path: &BezPath, rule: FillRule) -> Option<ID2D1PathGeometry> {
    unsafe {
        let g = factories().d2d.CreatePathGeometry().ok()?;
        let sink = g.Open().ok()?;
        sink.SetFillMode(if rule == FillRule::EvenOdd { D2D1_FILL_MODE_ALTERNATE } else { D2D1_FILL_MODE_WINDING });
        let mut open = false;
        let mut last = kurbo::Point::ZERO;
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(point(p), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                    last = p;
                }
                PathEl::LineTo(p) => {
                    sink.AddLines(&[point(p)]);
                    last = p;
                }
                PathEl::QuadTo(c, p) => {
                    // As a cubic: Direct2D's base sink only has those.
                    let c1 = last + (c - last) * (2.0 / 3.0);
                    let c2 = p + (c - p) * (2.0 / 3.0);
                    sink.AddBeziers(&[D2D1_BEZIER_SEGMENT { point1: point(c1), point2: point(c2), point3: point(p) }]);
                    last = p;
                }
                PathEl::CurveTo(c1, c2, p) => {
                    sink.AddBeziers(&[D2D1_BEZIER_SEGMENT { point1: point(c1), point2: point(c2), point3: point(p) }]);
                    last = p;
                }
                PathEl::ClosePath => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                }
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().ok()?;
        Some(g)
    }
}

fn stroke_style(s: &Stroke) -> Option<ID2D1StrokeStyle> {
    let cap = |c: Cap| match c {
        Cap::Butt => D2D1_CAP_STYLE_FLAT,
        Cap::Round => D2D1_CAP_STYLE_ROUND,
        Cap::Square => D2D1_CAP_STYLE_SQUARE,
    };
    let dashes: Vec<f32> = s.dash_pattern.iter().map(|d| (*d / s.width.max(0.001)) as f32).collect();
    let props = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: cap(s.start_cap),
        endCap: cap(s.end_cap),
        dashCap: cap(s.end_cap),
        lineJoin: match s.join {
            Join::Miter => D2D1_LINE_JOIN_MITER_OR_BEVEL,
            Join::Round => D2D1_LINE_JOIN_ROUND,
            Join::Bevel => D2D1_LINE_JOIN_BEVEL,
        },
        miterLimit: s.miter_limit as f32,
        dashStyle: if dashes.is_empty() { D2D1_DASH_STYLE_SOLID } else { D2D1_DASH_STYLE_CUSTOM },
        dashOffset: (s.dash_offset / s.width.max(0.001)) as f32,
    };
    unsafe { factories().d2d.CreateStrokeStyle(&props, (!dashes.is_empty()).then_some(dashes.as_slice())).ok() }
}

/// What's made for one render target: brushes by color, and pictures.
#[derive(Default)]
struct Resources {
    brushes: HashMap<[u8; 4], ID2D1SolidColorBrush>,
    bitmaps: HashMap<usize, (Rc<Picture>, ID2D1Bitmap)>,
}

impl Resources {
    fn brush(&mut self, rt: &ID2D1RenderTarget, c: Rgba) -> Option<ID2D1SolidColorBrush> {
        if let Some(b) = self.brushes.get(&c.0) {
            return Some(b.clone());
        }
        let b = unsafe { rt.CreateSolidColorBrush(&color_f(c), None).ok()? };
        self.brushes.insert(c.0, b.clone());
        Some(b)
    }

    fn bitmap(&mut self, rt: &ID2D1RenderTarget, picture: &Rc<Picture>) -> Option<ID2D1Bitmap> {
        let key = Rc::as_ptr(picture) as usize;
        if let Some((_, b)) = self.bitmaps.get(&key) {
            return Some(b.clone());
        }
        let b = unsafe { rt.CreateBitmapFromWicBitmap(&picture.image.0, None).ok()? };
        // (The picture is kept with it, so the address isn't reused.)
        self.bitmaps.insert(key, (picture.clone(), b.clone()));
        Some(b)
    }
}

/// Plays a drawing into a render target, with everything placed by `base`.
fn play(list: &DrawList, rt: &ID2D1RenderTarget, base: Affine, res: &mut Resources, open: &mut usize) {
    for op in &list.ops {
        unsafe {
            match op {
                Op::Fill { path, rule, color, transform: t } => {
                    let mut p = path.clone();
                    p.apply_affine(base * *t);
                    if let (Some(g), Some(b)) = (geometry(&p, *rule), res.brush(rt, *color)) {
                        rt.SetTransform(&Matrix3x2::identity());
                        rt.FillGeometry(&g, &b, None);
                    }
                }
                Op::Stroke { path, style, color, transform: t } => {
                    if let (Some(g), Some(b)) = (geometry(path, FillRule::NonZero), res.brush(rt, *color)) {
                        rt.SetTransform(&matrix(base * *t));
                        rt.DrawGeometry(&g, &b, style.width as f32, stroke_style(style).as_ref());
                        rt.SetTransform(&Matrix3x2::identity());
                    }
                }
                Op::Push { clip, rule, alpha, transform: t } => {
                    let mut p = clip.clone();
                    p.apply_affine(base * *t);
                    let g = geometry(&p, *rule).map(|g| g.cast::<ID2D1Geometry>().ok()).flatten();
                    let params = D2D1_LAYER_PARAMETERS {
                        contentBounds: D2D_RECT_F { left: -1.0e9, top: -1.0e9, right: 1.0e9, bottom: 1.0e9 },
                        geometricMask: std::mem::ManuallyDrop::new(g),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: Matrix3x2::identity(),
                        opacity: *alpha,
                        opacityBrush: std::mem::ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS_NONE,
                    };
                    rt.SetTransform(&Matrix3x2::identity());
                    rt.PushLayer(&params, None);
                    // (The mask was only lent.)
                    let _ = std::mem::ManuallyDrop::into_inner(params.geometricMask);
                    *open += 1;
                }
                Op::Pop => {
                    if *open > 0 {
                        rt.PopLayer();
                        *open -= 1;
                    }
                }
                Op::Text { layout, transform: t, tint, halo } => {
                    rt.SetTransform(&matrix(base * *t));
                    text::draw(rt, layout, *tint, *halo, &mut |c| res.brush(rt, c));
                    rt.SetTransform(&Matrix3x2::identity());
                }
                Op::Image { picture, transform: t, alpha } => {
                    if let Some(b) = res.bitmap(rt, picture) {
                        rt.SetTransform(&matrix(base * *t));
                        let r = D2D_RECT_F { left: 0.0, top: 0.0, right: picture.width as f32, bottom: picture.height as f32 };
                        rt.DrawBitmap(&b, Some(&r), *alpha, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        rt.SetTransform(&Matrix3x2::identity());
                    }
                }
                Op::Svg { svg, transform: t } => play(&svg.scene, rt, base * *t, res, open),
                Op::List { list, transform: t } => play(list, rt, base * *t, res, open),
            }
        }
    }
}

/// Plays a whole drawing, closing what it left open.
fn play_all(list: &DrawList, rt: &ID2D1RenderTarget, res: &mut Resources) {
    let mut open = 0;
    play(list, rt, Affine::IDENTITY, res, &mut open);
    for _ in 0..open {
        unsafe { rt.PopLayer() };
    }
}

// ===== pictures =====

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A file's first frame, as WIC decodes it.
fn frame(path: &str) -> Option<IWICBitmapFrameDecode> {
    unsafe {
        let name = wide(path);
        let decoder = factories()
            .wic
            .CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .ok()?;
        decoder.GetFrame(0).ok()
    }
}

/// The EXIF orientation of a photo (1 when it's the right way up already).
fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    unsafe {
        let Ok(reader) = frame.GetMetadataQueryReader() else { return 1 };
        for query in ["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"] {
            let q = wide(query);
            let mut v = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();
            if reader.GetMetadataByName(PCWSTR(q.as_ptr()), &mut v).is_ok() {
                let inner = &v.Anonymous.Anonymous;
                if inner.vt == windows::Win32::System::Variant::VT_UI2 {
                    return inner.Anonymous.uiVal;
                }
            }
        }
        1
    }
}

/// A picture in premultiplied BGRA, turned the way its camera says is up.
fn decode(path: &str, turn: bool) -> Option<IWICBitmapSource> {
    unsafe {
        let wic = &factories().wic;
        let frame = frame(path)?;
        let o = if turn { orientation(&frame) } else { 1 };
        let converter = wic.CreateFormatConverter().ok()?;
        converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let source: IWICBitmapSource = converter.cast().ok()?;
        let transform = match o {
            2 => WICBitmapTransformFlipHorizontal,
            3 => WICBitmapTransformRotate180,
            4 => WICBitmapTransformFlipVertical,
            5 => WICBitmapTransformOptions(WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0),
            6 => WICBitmapTransformRotate90,
            7 => WICBitmapTransformOptions(WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0),
            8 => WICBitmapTransformRotate270,
            _ => return Some(source),
        };
        let rotator = wic.CreateBitmapFlipRotator().ok()?;
        rotator.Initialize(&source, transform).ok()?;
        rotator.cast().ok()
    }
}

fn size(source: &IWICBitmapSource) -> (u32, u32) {
    let (mut w, mut h) = (0, 0);
    unsafe {
        let _ = source.GetSize(&mut w, &mut h);
    }
    (w, h)
}

/// The picture in a file, the right way up.
pub fn decode_picture(path: &str) -> Option<Picture> {
    let source = decode(path, true)?;
    // (Decoded now, once: a source is decoded each time it's read.)
    let bitmap = unsafe { factories().wic.CreateBitmapFromSource(&source, WICBitmapCacheOnLoad).ok()? };
    let (w, h) = size(&source);
    Some(Picture { image: ImageData(bitmap.cast().ok()?), width: w as f64, height: h as f64 })
}

/// A file's size in pixels.
pub fn image_size(path: &str) -> Option<(u32, u32)> {
    let frame = frame(path)?;
    let (mut w, mut h) = (0, 0);
    unsafe { frame.GetSize(&mut w, &mut h).ok()? };
    Some((w, h))
}

/// Premultiplied BGRA rows to plain RGBA.
fn to_rgba(bgra: &mut [u8]) {
    for p in bgra.chunks_exact_mut(4) {
        let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
        let un = |c: u32| if a > 0 && a < 255 { ((c * 255 + a / 2) / a).min(255) } else { c };
        p.copy_from_slice(&[un(r) as u8, un(g) as u8, un(b) as u8, a as u8]);
    }
}

/// A file's pixels (red, green, blue, opacity), and its size.
pub fn read_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let frame = frame(path)?;
        // Not premultiplied, so colors that aren't solid come back as they are.
        let converter = factories().wic.CreateFormatConverter().ok()?;
        converter.Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let source: IWICBitmapSource = converter.cast().ok()?;
        let (w, h) = size(&source);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        source.CopyPixels(std::ptr::null(), w * 4, &mut pixels).ok()?;
        Some((w, h, pixels))
    }
}

/// Saves pixels (red, green, blue, opacity) as a PNG, or a JPEG for a path
/// ending in .jpg or .jpeg. Whether it was saved.
pub fn save_rgba(path: &str, width: u32, height: u32, rgba: Vec<u8>) -> bool {
    let lower = path.to_lowercase();
    let jpeg = lower.ends_with(".jpg") || lower.ends_with(".jpeg");
    let save = || -> windows::core::Result<()> {
        unsafe {
            let wic = &factories().wic;
            let stream = wic.CreateStream()?;
            let name = wide(path);
            stream.InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_WRITE.0)?;
            let format: GUID = if jpeg { GUID_ContainerFormatJpeg } else { GUID_ContainerFormatPng };
            let encoder = wic.CreateEncoder(&format, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.ok_or_else(windows::core::Error::empty)?;
            frame.Initialize(None)?;
            frame.SetSize(width, height)?;
            // JPEG has no opacity: its pixels go as BGR.
            let (mut pixel_format, data, stride) = if jpeg {
                let bgr: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
                (GUID_WICPixelFormat24bppBGR, bgr, width * 3)
            } else {
                (GUID_WICPixelFormat32bppRGBA, rgba.clone(), width * 4)
            };
            // The encoder may take another format than the one asked for
            // (BGRA rather than RGBA, say): the pixels go in the one it took.
            let asked = pixel_format;
            frame.SetPixelFormat(&mut pixel_format)?;
            let data = if pixel_format == asked {
                data
            } else if pixel_format == GUID_WICPixelFormat32bppBGRA && !jpeg {
                rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect()
            } else if pixel_format == GUID_WICPixelFormat24bppRGB && jpeg {
                rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
            } else {
                return Err(windows::core::Error::empty());
            };
            frame.WritePixels(height, stride, &data)?;
            frame.Commit()?;
            encoder.Commit()?;
            Ok(())
        }
    };
    rgba.len() >= (width * height * 4) as usize && save().is_ok()
}

// ===== the screen =====

/// Where a window's drawings are shown: a Direct2D render target on it.
pub struct Screen {
    window: Arc<Window>,
    hwnd: HWND,
    target: Option<ID2D1HwndRenderTarget>,
    res: Resources,
}

impl Screen {
    pub fn new(window: Arc<Window>) -> Result<Screen, String> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return Err("not a Win32 window".into()) };
        let hwnd = HWND(h.hwnd.get() as *mut std::ffi::c_void);
        Ok(Screen { window, hwnd, target: None, res: Resources::default() })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(t) = &self.target {
            unsafe {
                let _ = t.Resize(&D2D_SIZE_U { width: width.max(1), height: height.max(1) });
            }
        }
    }

    /// Shows a drawing (in pixels) over `background`. False if the target
    /// was lost: draw again on the next frame.
    pub fn present(&mut self, list: &DrawList, background: Rgba) -> bool {
        let size = self.window.inner_size();
        if self.target.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                // (Drawings are already in pixels.)
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U { width: size.width.max(1), height: size.height.max(1) },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            self.target = unsafe { factories().d2d.CreateHwndRenderTarget(&props, &hwnd_props).ok() };
            self.res = Resources::default();
        }
        let Some(target) = &self.target else { return true };
        let rt: ID2D1RenderTarget = match target.cast() {
            Ok(rt) => rt,
            Err(_) => return true,
        };
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&color_f(background)));
            play_all(list, &rt, &mut self.res);
            if rt.EndDraw(None, None).is_err() {
                // Lost (the graphics card was reset, say): make it again.
                self.target = None;
                return false;
            }
        }
        true
    }
}

/// Renders drawings off the screen (for snapshots and `renderImage`).
#[derive(Default)]
pub struct Offscreen;

impl Offscreen {
    /// A drawing's pixels, four bytes each (red, green, blue, opacity), row
    /// by row, over `base` (`None`: clear).
    pub fn rgba(&mut self, list: &DrawList, width: u32, height: u32, base: Option<Rgba>) -> Vec<u8> {
        let render = || -> windows::core::Result<Vec<u8>> {
            unsafe {
                let f = factories();
                let bitmap = f.wic.CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
                let props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    ..Default::default()
                };
                let rt = f.d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
                let mut res = Resources::default();
                rt.BeginDraw();
                rt.Clear(Some(&color_f(base.unwrap_or(Rgba([0, 0, 0, 0])))));
                play_all(list, &rt, &mut res);
                rt.EndDraw(None, None)?;
                drop(res);
                drop(rt);
                let mut pixels = vec![0u8; (width * height * 4) as usize];
                bitmap.CopyPixels(std::ptr::null(), width * 4, &mut pixels)?;
                to_rgba(&mut pixels);
                Ok(pixels)
            }
        };
        render().unwrap_or_default()
    }
}
