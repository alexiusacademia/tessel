//! The macOS renderer (the `native` feature): Core Graphics draws, Core Text
//! lays text out, ImageIO reads and writes pictures, and frames reach the
//! window as the contents of its view's layer. These come with macOS, so an
//! app carries none of them: it's a fraction of the size of one with the GPU
//! renderer.
//!
//! Everything here has a counterpart in the other renderer, with the same
//! names (see `ui::backend`).

mod ffi;
pub mod text;

use std::ffi::c_void;
use std::sync::Arc;

use kurbo::{Affine, BezPath, Cap, Join, PathEl, Stroke};
use winit::window::Window;

use super::draw::{DrawList, FillRule, Op};
use super::image::Picture;
use super::Rgba;
use ffi::*;

pub use super::svg_list::svg_scene;

/// A decoded picture, ready to draw: a Core Graphics image.
pub struct ImageData(Owned);

/// A parsed SVG, ready to draw.
pub type SvgScene = DrawList;

fn transform(a: Affine) -> CGAffineTransform {
    let [a, b, c, d, tx, ty] = a.as_coeffs();
    CGAffineTransform { a, b, c, d, tx, ty }
}

fn add_path(ctx: CGContextRef, path: &BezPath) {
    unsafe {
        CGContextBeginPath(ctx);
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => CGContextMoveToPoint(ctx, p.x, p.y),
                PathEl::LineTo(p) => CGContextAddLineToPoint(ctx, p.x, p.y),
                PathEl::QuadTo(c, p) => CGContextAddQuadCurveToPoint(ctx, c.x, c.y, p.x, p.y),
                PathEl::CurveTo(c1, c2, p) => CGContextAddCurveToPoint(ctx, c1.x, c1.y, c2.x, c2.y, p.x, p.y),
                PathEl::ClosePath => CGContextClosePath(ctx),
            }
        }
    }
}

fn components(c: Rgba) -> (f64, f64, f64, f64) {
    let [r, g, b, a] = c.0.map(|v| v as f64 / 255.0);
    (r, g, b, a)
}

fn stroke_style(ctx: CGContextRef, s: &Stroke) {
    unsafe {
        CGContextSetLineWidth(ctx, s.width);
        CGContextSetLineCap(
            ctx,
            match s.start_cap {
                Cap::Butt => kCGLineCapButt,
                Cap::Round => kCGLineCapRound,
                Cap::Square => kCGLineCapSquare,
            },
        );
        CGContextSetLineJoin(
            ctx,
            match s.join {
                Join::Miter => kCGLineJoinMiter,
                Join::Round => kCGLineJoinRound,
                Join::Bevel => kCGLineJoinBevel,
            },
        );
        CGContextSetMiterLimit(ctx, s.miter_limit);
        let dashes: Vec<f64> = s.dash_pattern.iter().copied().collect();
        CGContextSetLineDash(ctx, s.dash_offset, dashes.as_ptr(), dashes.len());
    }
}

/// Plays a drawing into a context whose y grows downwards, with everything
/// placed by `base`.
pub fn play(list: &DrawList, ctx: CGContextRef, base: Affine) {
    // For each `Push` still open: whether it began a transparency layer.
    let mut open: Vec<bool> = Vec::new();
    play_into(list, ctx, base, &mut open);
    for layer in open.into_iter().rev() {
        unsafe {
            if layer {
                CGContextEndTransparencyLayer(ctx);
            }
            CGContextRestoreGState(ctx);
        }
    }
}

fn play_into(list: &DrawList, ctx: CGContextRef, base: Affine, open: &mut Vec<bool>) {
    let key = text::color_key();
    for op in &list.ops {
        unsafe {
            match op {
                Op::Fill { path, rule, color, transform: t } => {
                    // (Placed here rather than with the context's transform,
                    // which strokes need and fills don't.)
                    let mut p = path.clone();
                    p.apply_affine(base * *t);
                    add_path(ctx, &p);
                    let (r, g, b, a) = components(*color);
                    CGContextSetRGBFillColor(ctx, r, g, b, a);
                    match rule {
                        FillRule::NonZero => CGContextFillPath(ctx),
                        FillRule::EvenOdd => CGContextEOFillPath(ctx),
                    }
                }
                Op::Stroke { path, style, color, transform: t } => {
                    CGContextSaveGState(ctx);
                    CGContextConcatCTM(ctx, transform(base * *t));
                    add_path(ctx, path);
                    stroke_style(ctx, style);
                    let (r, g, b, a) = components(*color);
                    CGContextSetRGBStrokeColor(ctx, r, g, b, a);
                    CGContextStrokePath(ctx);
                    CGContextRestoreGState(ctx);
                }
                Op::Push { clip, rule, alpha, transform: t } => {
                    CGContextSaveGState(ctx);
                    let mut p = clip.clone();
                    p.apply_affine(base * *t);
                    add_path(ctx, &p);
                    match rule {
                        FillRule::NonZero => CGContextClip(ctx),
                        FillRule::EvenOdd => CGContextEOClip(ctx),
                    }
                    let layer = *alpha < 1.0;
                    if layer {
                        CGContextSetAlpha(ctx, *alpha as f64);
                        CGContextBeginTransparencyLayer(ctx, std::ptr::null());
                    }
                    open.push(layer);
                }
                Op::Pop => {
                    if let Some(layer) = open.pop() {
                        if layer {
                            CGContextEndTransparencyLayer(ctx);
                        }
                        CGContextRestoreGState(ctx);
                    }
                }
                Op::Text { layout, transform: t, tint, halo } => {
                    CGContextSaveGState(ctx);
                    CGContextConcatCTM(ctx, transform(base * *t));
                    text::draw(ctx, layout, *tint, key, *halo);
                    CGContextRestoreGState(ctx);
                }
                Op::Image { picture, transform: t, alpha } => {
                    CGContextSaveGState(ctx);
                    CGContextConcatCTM(ctx, transform(base * *t));
                    // Pictures are drawn y-up: flip this one back.
                    CGContextTranslateCTM(ctx, 0.0, picture.height);
                    CGContextScaleCTM(ctx, 1.0, -1.0);
                    CGContextSetAlpha(ctx, *alpha as f64);
                    CGContextSetInterpolationQuality(ctx, kCGInterpolationHigh);
                    CGContextDrawImage(ctx, CGRect::new(0.0, 0.0, picture.width, picture.height), picture.image.0.get() as CGImageRef);
                    CGContextRestoreGState(ctx);
                }
                Op::Svg { svg, transform: t } => play_into(&svg.scene, ctx, base * *t, open),
                Op::List { list, transform: t } => play_into(list, ctx, base * *t, open),
            }
        }
    }
}

// ===== pictures =====

/// The image in a file (PNG, JPEG, HEIC, …), turned the way its camera says
/// is up.
fn decode(path: &str) -> Option<Owned> {
    unsafe {
        let url = file_url(path)?;
        let source = Owned::new(CGImageSourceCreateWithURL(url.get(), std::ptr::null()))?;
        let (w, h) = size_of(&source)?;
        let yes = kCFBooleanTrue as CFTypeRef;
        let max = cfnumber_i32(w.max(h) as i32);
        let options = cfdict(&[
            (kCGImageSourceCreateThumbnailWithTransform, yes),
            (kCGImageSourceCreateThumbnailFromImageAlways, yes),
            (kCGImageSourceThumbnailMaxPixelSize, max.get()),
        ]);
        Owned::new(CGImageSourceCreateThumbnailAtIndex(source.get(), 0, options.get()) as CFTypeRef)
    }
}

fn size_of(source: &Owned) -> Option<(u32, u32)> {
    unsafe {
        let props = Owned::new(CGImageSourceCopyPropertiesAtIndex(source.get(), 0, std::ptr::null()))?;
        let number = |key: CFStringRef| {
            let v = CFDictionaryGetValue(props.get(), key);
            let mut n: i32 = 0;
            (!v.is_null() && CFNumberGetValue(v, kCFNumberSInt32Type, &mut n as *mut i32 as *mut c_void)).then_some(n as u32)
        };
        Some((number(kCGImagePropertyPixelWidth)?, number(kCGImagePropertyPixelHeight)?))
    }
}

/// The picture in a file, the right way up.
pub fn decode_picture(path: &str) -> Option<Picture> {
    let image = decode(path)?;
    let (w, h) = unsafe { (CGImageGetWidth(image.get() as CGImageRef), CGImageGetHeight(image.get() as CGImageRef)) };
    Some(Picture { image: ImageData(image), width: w as f64, height: h as f64 })
}

/// A file's size in pixels.
pub fn image_size(path: &str) -> Option<(u32, u32)> {
    let url = file_url(path)?;
    let source = Owned::new(unsafe { CGImageSourceCreateWithURL(url.get(), std::ptr::null()) })?;
    size_of(&source)
}

/// A bitmap of `width` × `height` pixels (red, green, blue, opacity, the
/// colors multiplied by it), with y growing downwards.
struct Bitmap {
    ctx: CGContextRef,
    width: usize,
    height: usize,
}

impl Bitmap {
    fn new(width: usize, height: usize) -> Option<Bitmap> {
        unsafe {
            let space = CGColorSpaceCreateDeviceRGB();
            let ctx = CGBitmapContextCreate(std::ptr::null_mut(), width, height, 8, width * 4, space, kRGBA_PREMULTIPLIED);
            CGColorSpaceRelease(space);
            if ctx.is_null() {
                return None;
            }
            CGContextTranslateCTM(ctx, 0.0, height as f64);
            CGContextScaleCTM(ctx, 1.0, -1.0);
            CGContextSetShouldSmoothFonts(ctx, false);
            Some(Bitmap { ctx, width, height })
        }
    }

    /// Fills it all with `base`, or clears it.
    fn clear(&self, base: Option<Rgba>) {
        unsafe {
            let all = CGRect::new(0.0, 0.0, self.width as f64, self.height as f64);
            CGContextClearRect(self.ctx, all);
            if let Some(c) = base {
                let (r, g, b, a) = components(c);
                CGContextSetRGBFillColor(self.ctx, r, g, b, a);
                CGContextFillRect(self.ctx, all);
            }
        }
    }

    /// The pixels, row by row from the top, with colors not multiplied by
    /// their opacity.
    fn rgba(&self) -> Vec<u8> {
        unsafe {
            let data = CGBitmapContextGetData(self.ctx) as *const u8;
            let row = CGBitmapContextGetBytesPerRow(self.ctx);
            let mut out = Vec::with_capacity(self.width * self.height * 4);
            for y in 0..self.height {
                out.extend_from_slice(std::slice::from_raw_parts(data.add(y * row), self.width * 4));
            }
            for p in out.chunks_exact_mut(4) {
                let a = p[3] as u32;
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            out
        }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe { CGContextRelease(self.ctx) };
    }
}

/// A file's pixels (red, green, blue, opacity), and its size: as they're
/// stored, when they're 8-bit RGB (a picture that's drawn instead rounds
/// colors that aren't solid).
pub fn read_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    if let Some(stored) = stored_rgba(path) {
        return Some(stored);
    }
    let picture = decode_picture(path)?;
    let (w, h) = (picture.width as usize, picture.height as usize);
    let bitmap = Bitmap::new(w, h)?;
    let mut list = DrawList::new();
    list.image(std::rc::Rc::new(picture), Affine::IDENTITY, 1.0);
    play(&list, bitmap.ctx, Affine::IDENTITY);
    Some((w as u32, h as u32, bitmap.rgba()))
}

/// The pixels of an 8-bit RGB image file as they're stored.
fn stored_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let url = file_url(path)?;
        let source = Owned::new(CGImageSourceCreateWithURL(url.get(), std::ptr::null()))?;
        let image = Owned::new(CGImageSourceCreateImageAtIndex(source.get(), 0, std::ptr::null()) as CFTypeRef)?;
        let img = image.get() as CGImageRef;
        // RGB, 8 bits a color.
        if CGColorSpaceGetModel(CGImageGetColorSpace(img)) != 1 || CGImageGetBitsPerComponent(img) != 8 {
            return None;
        }
        let (w, h) = (CGImageGetWidth(img), CGImageGetHeight(img));
        let bpp = CGImageGetBitsPerPixel(img) / 8;
        let row = CGImageGetBytesPerRow(img);
        let info = CGImageGetBitmapInfo(img);
        let (alpha, order) = (info & 0x1f, info & 0x7000);
        if !(bpp == 3 || bpp == 4) || !(order == 0 || order == 0x4000 || (order == 0x2000 && bpp == 4)) {
            return None;
        }
        let data = Owned::new(CGDataProviderCopyData(CGImageGetDataProvider(img)))?;
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data.get()), CFDataGetLength(data.get()) as usize);
        if bytes.len() < row * h {
            return None;
        }
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let p = &bytes[y * row + x * bpp..y * row + x * bpp + bpp];
                // In memory order: reversed for 32-bit little-endian pixels.
                let px: [u8; 4] = if bpp == 3 { [p[0], p[1], p[2], 255] } else if order == 0x2000 { [p[3], p[2], p[1], p[0]] } else { [p[0], p[1], p[2], p[3]] };
                // Where opacity is (first or last, or none), and whether colors are multiplied by it.
                let (r, g, b, a, premultiplied) = match alpha {
                    0 | 5 => (px[0], px[1], px[2], 255, false),
                    6 => (px[1], px[2], px[3], 255, false),
                    1 => (px[0], px[1], px[2], px[3], true),
                    2 => (px[1], px[2], px[3], px[0], true),
                    3 => (px[0], px[1], px[2], px[3], false),
                    4 => (px[1], px[2], px[3], px[0], false),
                    _ => return None,
                };
                let un = |c: u8| if premultiplied && a > 0 && a < 255 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 } else { c };
                out.extend_from_slice(&[un(r), un(g), un(b), a]);
            }
        }
        Some((w as u32, h as u32, out))
    }
}

/// Saves pixels (red, green, blue, opacity) as a PNG, or a JPEG for a path
/// ending in .jpg or .jpeg. Whether it was saved.
pub fn save_rgba(path: &str, width: u32, height: u32, rgba: Vec<u8>) -> bool {
    if rgba.len() < width as usize * height as usize * 4 {
        return false;
    }
    let lower = path.to_lowercase();
    let kind = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { "public.jpeg" } else { "public.png" };
    unsafe {
        let Some(data) = Owned::new(CFDataCreate(std::ptr::null(), rgba.as_ptr(), rgba.len() as CFIndex)) else { return false };
        let provider = CGDataProviderCreateWithCFData(data.get());
        let space = CGColorSpaceCreateDeviceRGB();
        // (Colors not multiplied by their opacity, which comes last.)
        let image = CGImageCreate(
            width as usize,
            height as usize,
            8,
            32,
            width as usize * 4,
            space,
            3 | (4 << 12),
            provider,
            std::ptr::null(),
            false,
            kCGRenderingIntentDefault,
        );
        CGColorSpaceRelease(space);
        CGDataProviderRelease(provider);
        let Some(image) = Owned::new(image as CFTypeRef) else { return false };
        let Some(url) = file_url(path) else { return false };
        let kind = cfstring(kind);
        let Some(dest) = Owned::new(CGImageDestinationCreateWithURL(url.get(), kind.get(), 1, std::ptr::null()) as CFTypeRef) else {
            return false;
        };
        CGImageDestinationAddImage(dest.get() as CGImageDestinationRef, image.get() as CGImageRef, std::ptr::null());
        CGImageDestinationFinalize(dest.get() as CGImageDestinationRef)
    }
}

// ===== the screen =====

/// Where a window's drawings are shown: drawn into a bitmap, which becomes
/// the contents of the window's view's layer.
pub struct Screen {
    window: Arc<Window>,
    view: *mut objc2::runtime::AnyObject,
    bitmap: Option<Bitmap>,
}

impl Screen {
    pub fn new(window: Arc<Window>) -> Result<Screen, String> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return Err("not an AppKit window".into()) };
        let view = h.ns_view.as_ptr() as *mut objc2::runtime::AnyObject;
        unsafe {
            let _: () = objc2::msg_send![view, setWantsLayer: true];
        }
        Ok(Screen { window, view, bitmap: None })
    }

    pub fn resize(&mut self, _width: u32, _height: u32) {
        // (The bitmap is made again at the next frame's size.)
    }

    /// Shows a drawing (in pixels) over `background`.
    pub fn present(&mut self, list: &DrawList, background: Rgba) -> bool {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1) as usize, size.height.max(1) as usize);
        if self.bitmap.as_ref().is_none_or(|b| b.width != w || b.height != h) {
            self.bitmap = Bitmap::new(w, h);
        }
        let Some(bitmap) = &self.bitmap else { return true };
        bitmap.clear(Some(background));
        play(list, bitmap.ctx, Affine::IDENTITY);
        unsafe {
            let image = CGBitmapContextCreateImage(bitmap.ctx);
            if image.is_null() {
                return true;
            }
            let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![self.view, layer];
            let transaction = objc2::class!(CATransaction);
            let _: () = objc2::msg_send![transaction, begin];
            let _: () = objc2::msg_send![transaction, setDisableActions: true];
            let _: () = objc2::msg_send![layer, setContents: image as *mut objc2::runtime::AnyObject];
            let _: () = objc2::msg_send![transaction, commit];
            CGImageRelease(image);
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
        let Some(bitmap) = Bitmap::new(width as usize, height as usize) else { return Vec::new() };
        bitmap.clear(base);
        play(list, bitmap.ctx, Affine::IDENTITY);
        bitmap.rgba()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHOTO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/pictures/photo.jpg");
    const TILES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/pictures/tiles.png");

    #[test]
    fn reads_pictures() {
        assert_eq!(image_size(PHOTO), Some((120, 80)));
        let p = decode_picture(PHOTO).expect("the photo");
        assert_eq!((p.width, p.height), (120.0, 80.0));
        let t = decode_picture(TILES).expect("the tiles");
        assert_eq!((t.width, t.height), (48.0, 48.0));
        assert!(decode_picture("no such file.png").is_none());
    }
}
