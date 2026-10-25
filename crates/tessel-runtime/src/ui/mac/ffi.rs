//! The parts of macOS's C libraries the renderer uses: Core Foundation, Core
//! Graphics, Core Text and ImageIO. They come with the system, so apps link
//! them instead of carrying a renderer of their own.
//!
//! Every `*Ref` is a pointer to an object the system counts references to;
//! what's created or copied is the caller's to `CFRelease`.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFAttributedStringRef = *const c_void;
pub type CFMutableAttributedStringRef = *mut c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFURLRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFBooleanRef = *const c_void;
pub type CGContextRef = *mut c_void;
pub type CGColorSpaceRef = *mut c_void;
pub type CGImageRef = *mut c_void;
pub type CGDataProviderRef = *mut c_void;
pub type CTFontRef = *const c_void;
pub type CTFontDescriptorRef = *const c_void;
pub type CTTypesetterRef = *const c_void;
pub type CTLineRef = *const c_void;
pub type CTRunRef = *const c_void;
pub type CGImageSourceRef = *const c_void;
pub type CGImageDestinationRef = *mut c_void;
pub type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CFRange {
    pub location: CFIndex,
    pub length: CFIndex,
}

impl CFRange {
    pub fn new(location: usize, length: usize) -> CFRange {
        CFRange { location: location as CFIndex, length: length as CFIndex }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

impl CGRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> CGRect {
        CGRect { origin: CGPoint { x, y }, size: CGSize { width: w, height: h } }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGAffineTransform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

#[repr(C)]
pub struct CFDictionaryKeyCallBacks {
    _private: [u8; 0],
}

#[repr(C)]
pub struct CFDictionaryValueCallBacks {
    _private: [u8; 0],
}

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kCFNumberSInt32Type: CFIndex = 3;
pub const kCFNumberFloat64Type: CFIndex = 6;
/// Red, green, blue, opacity, in that order in memory, with the colors
/// multiplied by the opacity.
pub const kRGBA_PREMULTIPLIED: u32 = 1 | (4 << 12);
pub const kCGLineCapButt: i32 = 0;
pub const kCGLineCapRound: i32 = 1;
pub const kCGLineCapSquare: i32 = 2;
pub const kCGLineJoinMiter: i32 = 0;
pub const kCGLineJoinRound: i32 = 1;
pub const kCGLineJoinBevel: i32 = 2;
pub const kCGTextFill: i32 = 0;
pub const kCGTextStroke: i32 = 1;
pub const kCGInterpolationHigh: i32 = 3;
pub const kCGRenderingIntentDefault: i32 = 0;
/// `CTFontUIFontType`s.
pub const kCTFontUIFontSystem: u32 = 2;
pub const kCTFontUIFontUserFixedPitch: u32 = 1;
pub const kCTFontItalicTrait: u32 = 1 << 0;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
    pub static kCFBooleanTrue: CFBooleanRef;

    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: CFIndex,
        encoding: u32,
        external: bool,
    ) -> CFStringRef;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(len: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: CFIndex, encoding: u32) -> bool;
    pub fn CFAttributedStringCreateMutable(alloc: *const c_void, max: CFIndex) -> CFMutableAttributedStringRef;
    pub fn CFAttributedStringReplaceString(s: CFMutableAttributedStringRef, range: CFRange, with: CFStringRef);
    pub fn CFAttributedStringSetAttribute(s: CFMutableAttributedStringRef, range: CFRange, name: CFStringRef, value: CFTypeRef);
    pub fn CFAttributedStringBeginEditing(s: CFMutableAttributedStringRef);
    pub fn CFAttributedStringEndEditing(s: CFMutableAttributedStringRef);
    pub fn CFDictionaryCreate(
        alloc: *const c_void,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        n: CFIndex,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFDictionaryRef;
    pub fn CFDictionaryGetValue(d: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFNumberCreate(alloc: *const c_void, kind: CFIndex, value: *const c_void) -> CFNumberRef;
    pub fn CFNumberGetValue(n: CFNumberRef, kind: CFIndex, value: *mut c_void) -> bool;
    pub fn CFArrayGetCount(a: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, i: CFIndex) -> CFTypeRef;
    pub fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, path: *const u8, len: CFIndex, dir: bool) -> CFURLRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    // PDF documents (`writePDF`).
    pub static kCGPDFContextTitle: CFStringRef;
    pub static kCGPDFContextCreator: CFStringRef;
    pub fn CGPDFContextCreateWithURL(url: CFURLRef, media_box: *const CGRect, info: CFDictionaryRef) -> CGContextRef;
    pub fn CGContextBeginPage(c: CGContextRef, media_box: *const CGRect);
    pub fn CGContextEndPage(c: CGContextRef);
    pub fn CGPDFContextClose(c: CGContextRef);
    pub fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
    pub fn CGColorSpaceRelease(space: CGColorSpaceRef);
    pub fn CGColorCreate(space: CGColorSpaceRef, components: *const f64) -> *mut c_void;
    pub fn CGColorRelease(color: *mut c_void);
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        bytes_per_row: usize,
        space: CGColorSpaceRef,
        info: u32,
    ) -> CGContextRef;
    pub fn CGBitmapContextGetData(c: CGContextRef) -> *mut c_void;
    pub fn CGBitmapContextGetBytesPerRow(c: CGContextRef) -> usize;
    pub fn CGBitmapContextCreateImage(c: CGContextRef) -> CGImageRef;
    pub fn CGContextRelease(c: CGContextRef);
    pub fn CGContextSaveGState(c: CGContextRef);
    pub fn CGContextRestoreGState(c: CGContextRef);
    pub fn CGContextConcatCTM(c: CGContextRef, t: CGAffineTransform);
    pub fn CGContextTranslateCTM(c: CGContextRef, x: f64, y: f64);
    pub fn CGContextScaleCTM(c: CGContextRef, x: f64, y: f64);
    pub fn CGContextSetRGBFillColor(c: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextSetRGBStrokeColor(c: CGContextRef, r: f64, g: f64, b: f64, a: f64);
    pub fn CGContextBeginPath(c: CGContextRef);
    pub fn CGContextMoveToPoint(c: CGContextRef, x: f64, y: f64);
    pub fn CGContextAddLineToPoint(c: CGContextRef, x: f64, y: f64);
    pub fn CGContextAddQuadCurveToPoint(c: CGContextRef, cx: f64, cy: f64, x: f64, y: f64);
    pub fn CGContextAddCurveToPoint(c: CGContextRef, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64);
    pub fn CGContextClosePath(c: CGContextRef);
    pub fn CGContextFillPath(c: CGContextRef);
    pub fn CGContextEOFillPath(c: CGContextRef);
    pub fn CGContextStrokePath(c: CGContextRef);
    pub fn CGContextClip(c: CGContextRef);
    pub fn CGContextEOClip(c: CGContextRef);
    pub fn CGContextFillRect(c: CGContextRef, r: CGRect);
    pub fn CGContextClearRect(c: CGContextRef, r: CGRect);
    pub fn CGContextSetLineWidth(c: CGContextRef, w: f64);
    pub fn CGContextSetLineCap(c: CGContextRef, cap: i32);
    pub fn CGContextSetLineJoin(c: CGContextRef, join: i32);
    pub fn CGContextSetMiterLimit(c: CGContextRef, limit: f64);
    pub fn CGContextSetLineDash(c: CGContextRef, phase: f64, lengths: *const f64, count: usize);
    pub fn CGContextSetAlpha(c: CGContextRef, a: f64);
    pub fn CGContextBeginTransparencyLayer(c: CGContextRef, aux: CFDictionaryRef);
    pub fn CGContextEndTransparencyLayer(c: CGContextRef);
    pub fn CGContextDrawImage(c: CGContextRef, r: CGRect, image: CGImageRef);
    pub fn CGContextSetInterpolationQuality(c: CGContextRef, q: i32);
    pub fn CGContextSetBlendMode(c: CGContextRef, mode: i32);
    pub fn CGContextReplacePathWithStrokedPath(c: CGContextRef);
    pub fn CGGradientCreateWithColorComponents(
        space: CGColorSpaceRef,
        components: *const f64,
        locations: *const f64,
        count: usize,
    ) -> CFTypeRef;
    pub fn CGContextDrawLinearGradient(c: CGContextRef, g: CFTypeRef, start: CGPoint, end: CGPoint, options: u32);
    pub fn CGContextDrawRadialGradient(
        c: CGContextRef,
        g: CFTypeRef,
        start: CGPoint,
        start_radius: f64,
        end: CGPoint,
        end_radius: f64,
        options: u32,
    );
    pub fn CGContextSetTextMatrix(c: CGContextRef, t: CGAffineTransform);
    pub fn CGContextSetTextPosition(c: CGContextRef, x: f64, y: f64);
    pub fn CGContextSetTextDrawingMode(c: CGContextRef, mode: i32);
    pub fn CGContextSetShouldSmoothFonts(c: CGContextRef, smooth: bool);
    pub fn CGContextSetAllowsFontSmoothing(c: CGContextRef, allow: bool);
    pub fn CGImageGetWidth(image: CGImageRef) -> usize;
    pub fn CGImageGetHeight(image: CGImageRef) -> usize;
    pub fn CGImageRelease(image: CGImageRef);
    pub fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: CGColorSpaceRef,
        info: u32,
        provider: CGDataProviderRef,
        decode: *const f64,
        interpolate: bool,
        intent: i32,
    ) -> CGImageRef;
    pub fn CGDataProviderCreateWithCFData(data: CFDataRef) -> CGDataProviderRef;
    pub fn CGDataProviderCopyData(provider: CGDataProviderRef) -> CFDataRef;
    pub fn CGImageGetDataProvider(image: CGImageRef) -> CGDataProviderRef;
    pub fn CGImageGetBitsPerComponent(image: CGImageRef) -> usize;
    pub fn CGImageGetBitsPerPixel(image: CGImageRef) -> usize;
    pub fn CGImageGetBytesPerRow(image: CGImageRef) -> usize;
    pub fn CGImageGetBitmapInfo(image: CGImageRef) -> u32;
    pub fn CGImageGetColorSpace(image: CGImageRef) -> CGColorSpaceRef;
    pub fn CGColorSpaceGetModel(space: CGColorSpaceRef) -> i32;
    pub fn CGDataProviderRelease(provider: CGDataProviderRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFDataCreate(alloc: *const c_void, bytes: *const u8, len: CFIndex) -> CFDataRef;
    pub fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    pub fn CFDataGetLength(data: CFDataRef) -> CFIndex;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub static kCTFontAttributeName: CFStringRef;
    pub static kCTForegroundColorFromContextAttributeName: CFStringRef;
    pub static kCTFontTraitsAttribute: CFStringRef;
    pub static kCTFontWeightTrait: CFStringRef;
    pub static kCTFontFamilyNameAttribute: CFStringRef;

    pub fn CTFontCreateUIFontForLanguage(kind: u32, size: f64, language: CFStringRef) -> CTFontRef;
    pub fn CTFontCopyFontDescriptor(font: CTFontRef) -> CTFontDescriptorRef;
    pub fn CTFontDescriptorCreateCopyWithAttributes(d: CTFontDescriptorRef, attrs: CFDictionaryRef) -> CTFontDescriptorRef;
    pub fn CTFontCreateWithFontDescriptor(d: CTFontDescriptorRef, size: f64, matrix: *const CGAffineTransform) -> CTFontRef;
    pub fn CTFontCreateCopyWithSymbolicTraits(
        font: CTFontRef,
        size: f64,
        matrix: *const CGAffineTransform,
        value: u32,
        mask: u32,
    ) -> CTFontRef;
    pub fn CTFontDescriptorCreateWithAttributes(attrs: CFDictionaryRef) -> CTFontDescriptorRef;
    pub fn CTFontDescriptorCreateMatchingFontDescriptor(d: CTFontDescriptorRef, mandatory: CFTypeRef) -> CTFontDescriptorRef;
    pub fn CTFontManagerCopyAvailableFontFamilyNames() -> CFArrayRef;
    pub fn CTFontCreateCopyWithAttributes(font: CTFontRef, size: f64, matrix: *const CGAffineTransform, attrs: CTFontDescriptorRef) -> CTFontRef;
    pub fn CTFontGetMatrix(font: CTFontRef) -> CGAffineTransform;
    pub fn CTFontCopyFamilyName(font: CTFontRef) -> CFStringRef;
    pub fn CTFontGetSymbolicTraits(font: CTFontRef) -> u32;
    pub fn CTFontGetAscent(font: CTFontRef) -> f64;
    pub fn CTFontGetDescent(font: CTFontRef) -> f64;
    pub fn CTFontGetLeading(font: CTFontRef) -> f64;
    pub fn CTTypesetterCreateWithAttributedString(s: CFAttributedStringRef) -> CTTypesetterRef;
    pub fn CTTypesetterSuggestLineBreak(t: CTTypesetterRef, start: CFIndex, width: f64) -> CFIndex;
    pub fn CTTypesetterCreateLine(t: CTTypesetterRef, range: CFRange) -> CTLineRef;
    pub fn CTLineGetTypographicBounds(line: CTLineRef, ascent: *mut f64, descent: *mut f64, leading: *mut f64) -> f64;
    pub fn CTLineGetTrailingWhitespaceWidth(line: CTLineRef) -> f64;
    pub fn CTLineGetGlyphRuns(line: CTLineRef) -> CFArrayRef;
    pub fn CTLineGetStringRange(line: CTLineRef) -> CFRange;
    pub fn CTRunGetGlyphCount(run: CTRunRef) -> CFIndex;
    pub fn CTRunGetPositions(run: CTRunRef, range: CFRange, out: *mut CGPoint);
    pub fn CTRunGetAdvances(run: CTRunRef, range: CFRange, out: *mut CGSize);
    pub fn CTRunGetStringIndices(run: CTRunRef, range: CFRange, out: *mut CFIndex);
    pub fn CTRunGetAttributes(run: CTRunRef) -> CFDictionaryRef;
    pub fn CTRunDraw(run: CTRunRef, c: CGContextRef, range: CFRange);
    pub fn CTRunGetGlyphs(run: CTRunRef, range: CFRange, out: *mut u16);
    pub fn CTRunGetStringRange(run: CTRunRef) -> CFRange;
    pub fn CTFontGetCapHeight(font: CTFontRef) -> f64;
    pub fn CTFontGetXHeight(font: CTFontRef) -> f64;
    pub fn CTFontCreatePathForGlyph(font: CTFontRef, glyph: u16, matrix: *const CGAffineTransform) -> CGPathRef;
    pub fn CTFontDescriptorCreateMatchingFontDescriptors(d: CTFontDescriptorRef, mandatory: CFTypeRef) -> CFArrayRef;
    pub fn CTFontDescriptorCopyAttribute(d: CTFontDescriptorRef, name: CFStringRef) -> CFTypeRef;
    pub static kCTFontStyleNameAttribute: CFStringRef;
    pub static kCTFontSymbolicTrait: CFStringRef;
    pub static kCTKernAttributeName: CFStringRef;
    pub static kCTTrackingAttributeName: CFStringRef;
}

/// A part of a path, as `CGPathApply` gives it: its kind (move, line,
/// quadratic curve, cubic curve, close) and its points.
#[repr(C)]
pub struct CGPathElement {
    pub kind: i32,
    pub points: *const CGPoint,
}

pub type CGPathRef = *const c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGPathApply(path: CGPathRef, info: *mut c_void, f: unsafe extern "C" fn(*mut c_void, *const CGPathElement));
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    pub static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
    pub static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    pub static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    pub static kCGImagePropertyPixelWidth: CFStringRef;
    pub static kCGImagePropertyPixelHeight: CFStringRef;
    pub static kCGImagePropertyOrientation: CFStringRef;

    pub fn CGImageSourceCreateWithURL(url: CFURLRef, options: CFDictionaryRef) -> CGImageSourceRef;
    pub fn CGImageSourceCreateImageAtIndex(src: CGImageSourceRef, i: usize, options: CFDictionaryRef) -> CGImageRef;
    pub fn CGImageSourceCreateThumbnailAtIndex(src: CGImageSourceRef, i: usize, options: CFDictionaryRef) -> CGImageRef;
    pub fn CGImageSourceCopyPropertiesAtIndex(src: CGImageSourceRef, i: usize, options: CFDictionaryRef) -> CFDictionaryRef;
    pub fn CGImageDestinationCreateWithURL(url: CFURLRef, kind: CFStringRef, count: usize, options: CFDictionaryRef) -> CGImageDestinationRef;
    pub fn CGImageDestinationAddImage(dest: CGImageDestinationRef, image: CGImageRef, properties: CFDictionaryRef);
    pub fn CGImageDestinationFinalize(dest: CGImageDestinationRef) -> bool;
}

// ===== small helpers =====

/// A Core Foundation object owned here: released when dropped.
pub struct Owned(pub CFTypeRef);

impl Owned {
    /// Takes an object that was created or copied (null gives `None`).
    pub fn new(r: CFTypeRef) -> Option<Owned> {
        (!r.is_null()).then_some(Owned(r))
    }

    pub fn get(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // (Releasing nothing would stop the program.)
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

pub fn cfstring(s: &str) -> Owned {
    Owned(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, kCFStringEncodingUTF8, false) })
}

/// A `CFString`'s text.
pub fn string_of(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe {
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0u8; size as usize];
        if !CFStringGetCString(s, buf.as_mut_ptr(), size, kCFStringEncodingUTF8) {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
}

pub fn cfnumber_f64(v: f64) -> Owned {
    Owned(unsafe { CFNumberCreate(std::ptr::null(), kCFNumberFloat64Type, &v as *const f64 as *const c_void) })
}

pub fn cfnumber_i32(v: i32) -> Owned {
    Owned(unsafe { CFNumberCreate(std::ptr::null(), kCFNumberSInt32Type, &v as *const i32 as *const c_void) })
}

/// A dictionary of `pairs`.
pub fn cfdict(pairs: &[(CFTypeRef, CFTypeRef)]) -> Owned {
    let keys: Vec<CFTypeRef> = pairs.iter().map(|p| p.0).collect();
    let values: Vec<CFTypeRef> = pairs.iter().map(|p| p.1).collect();
    Owned(unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as CFIndex,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    })
}

pub fn file_url(path: &str) -> Option<Owned> {
    Owned::new(unsafe { CFURLCreateFromFileSystemRepresentation(std::ptr::null(), path.as_ptr(), path.len() as CFIndex, false) })
}
