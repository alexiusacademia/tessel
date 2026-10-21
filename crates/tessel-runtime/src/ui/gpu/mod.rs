//! The GPU renderer (the `gpu` feature): vello draws, with wgpu, and parley
//! lays text out. It plays the drawings views make (`DrawList`) into vello
//! scenes, shows them in the window, renders them off-screen for snapshots
//! and `renderImage`, and reads and writes picture files with `image`.
//!
//! Everything here has a counterpart in the other renderer, with the same
//! names (see `ui::backend`).

pub mod text;

use std::num::NonZeroUsize;
use std::sync::Arc;

use kurbo::{Affine, Stroke};
use vello::peniko::{Blob, BlendMode, Brush, Color, ColorStop, Fill, ImageAlphaType, ImageBrush, ImageFormat, Mix};
use vello::util::{RenderContext, RenderSurface};
use vello::{wgpu, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use winit::window::Window;

use super::draw::{DrawList, FillRule, Gradient, Op};
use super::image::Picture;
use super::text::TextLayout;
use super::Rgba;

/// A decoded picture, ready to draw.
pub type ImageData = vello::peniko::ImageData;

/// A parsed SVG, ready to draw: a drawing like any other (so it's the same
/// with every renderer, and goes into a PDF as shapes).
pub type SvgScene = DrawList;

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.0[0], c.0[1], c.0[2], c.0[3])
}

fn fill_rule(rule: FillRule) -> Fill {
    match rule {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

/// `Blend`s, in order.
const MIXES: [Mix; 16] = [
    Mix::Normal,
    Mix::Multiply,
    Mix::Screen,
    Mix::Overlay,
    Mix::Darken,
    Mix::Lighten,
    Mix::ColorDodge,
    Mix::ColorBurn,
    Mix::HardLight,
    Mix::SoftLight,
    Mix::Difference,
    Mix::Exclusion,
    Mix::Hue,
    Mix::Saturation,
    Mix::Color,
    Mix::Luminosity,
];

fn brush(g: &Gradient) -> Brush {
    let stops = |s: &[(f32, Rgba)]| -> Vec<ColorStop> { s.iter().map(|(at, c)| (*at, color(*c)).into()).collect() };
    let gradient = match g {
        Gradient::Linear { start, end, stops: s } => vello::peniko::Gradient::new_linear(*start, *end).with_stops(stops(s).as_slice()),
        Gradient::Radial { center, radius, stops: s } => {
            vello::peniko::Gradient::new_radial(*center, *radius as f32).with_stops(stops(s).as_slice())
        }
    };
    Brush::Gradient(gradient)
}

/// Plays a drawing into a vello scene, with everything placed by `base`.
pub fn play(list: &DrawList, scene: &mut Scene, base: Affine) {
    for op in &list.ops {
        match op {
            Op::Fill { path, rule, color: c, transform } => scene.fill(fill_rule(*rule), base * *transform, color(*c), None, path),
            Op::Stroke { path, style, color: c, transform } => scene.stroke(style, base * *transform, color(*c), None, path),
            Op::FillGradient { path, rule, gradient, place, transform } => {
                scene.fill(fill_rule(*rule), base * *transform, &brush(gradient), Some(*place), path)
            }
            Op::StrokeGradient { path, style, gradient, place, transform } => {
                scene.stroke(style, base * *transform, &brush(gradient), Some(*place), path)
            }
            Op::Push { clip, rule, alpha, blend, transform } => {
                let mix = MIXES[(*blend as usize).min(MIXES.len() - 1)];
                scene.push_layer(fill_rule(*rule), BlendMode::from(mix), *alpha, base * *transform, clip);
            }
            Op::Pop => scene.pop_layer(),
            Op::Text { layout, transform, tint, halo } => {
                let t = base * *transform;
                if let Some((c, width)) = halo {
                    glyphs(scene, layout, t, Some(*c), Some(*width));
                }
                glyphs(scene, layout, t, *tint, None);
            }
            Op::Image { picture, transform, alpha } => {
                let brush = ImageBrush::new(picture.image.clone()).with_alpha(*alpha);
                scene.draw_image(&brush, base * *transform);
            }
            Op::Svg { svg, transform } => play(&svg.scene, scene, base * *transform),
            Op::List { list, transform } => play(list, scene, base * *transform),
        }
    }
}

/// Draws a layout's glyphs: filled, or outlined `stroke` wide.
fn glyphs(scene: &mut Scene, layout: &TextLayout, t: Affine, tint: Option<Rgba>, stroke: Option<f64>) {
    for line in layout.shaped.lines() {
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let brush = tint.map(|c| c.0).unwrap_or(run.style().brush.0);
            let r = run.run();
            let glyphs = run.positioned_glyphs().map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y });
            let draw = scene
                .draw_glyphs(r.font())
                .brush(Color::from_rgba8(brush[0], brush[1], brush[2], brush[3]))
                .hint(false)
                .transform(t)
                .font_size(r.font_size())
                .normalized_coords(r.normalized_coords());
            match stroke {
                Some(width) => draw.draw(&Stroke::new(width), glyphs),
                None => draw.draw(Fill::NonZero, glyphs),
            }
        }
    }
}

// ===== pictures and SVG =====

/// The picture in a PNG or JPEG file, the right way up.
pub fn decode_picture(path: &str) -> Option<Picture> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    // Photos from cameras and phones say which way up they are.
    let orientation = decoder.orientation().ok()?;
    let mut picture = image::DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    let rgba = picture.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some(Picture {
        image: ImageData {
            data: Blob::new(Arc::new(rgba.into_raw())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        },
        width: width as f64,
        height: height as f64,
    })
}

/// A picture made from pixels (red, green, blue, opacity, row by row).
pub fn picture_from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Picture> {
    if width == 0 || height == 0 || rgba.len() < width as usize * height as usize * 4 {
        return None;
    }
    Some(Picture {
        image: ImageData { data: Blob::new(Arc::new(rgba)), format: ImageFormat::Rgba8, alpha_type: ImageAlphaType::Alpha, width, height },
        width: width as f64,
        height: height as f64,
    })
}

pub use super::svg_list::svg_scene;

// ===== picture files =====

/// Saves pixels (red, green, blue, opacity) as a PNG, or a JPEG for a path
/// ending in .jpg or .jpeg (without the opacity). Whether it was saved.
pub fn save_rgba(path: &str, width: u32, height: u32, rgba: Vec<u8>) -> bool {
    let Some(picture) = image::RgbaImage::from_raw(width, height, rgba) else { return false };
    let lower = path.to_lowercase();
    let saved = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        image::DynamicImage::ImageRgba8(picture).to_rgb8().save_with_format(path, image::ImageFormat::Jpeg)
    } else {
        picture.save_with_format(path, image::ImageFormat::Png)
    };
    saved.is_ok()
}

/// A PNG or JPEG file's pixels (red, green, blue, opacity), and its size.
pub fn read_rgba(path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let picture = image::open(path).ok()?.to_rgba8();
    Some((picture.width(), picture.height(), picture.into_raw()))
}

/// A PNG or JPEG file's size in pixels.
pub fn image_size(path: &str) -> Option<(u32, u32)> {
    image::image_dimensions(path).ok()
}

// ===== the screen =====

/// ARM's graphics cards (Mali), by the number their maker has.
const ARM: u32 = 0x13B5;

fn renderer(dev: &vello::util::DeviceHandle) -> Renderer {
    // The renderer does most of its work on the graphics card, in steps.
    // On a Mali (most Android phones: a Pixel 10a's G715 was tried), one of
    // them, `tile_alloc`, makes the driver give the card up, and nothing
    // more can be drawn. There the steps before the last one are done by
    // the processor instead: slower (some 20 to 70 ms a frame), but drawn.
    let slow = dev.adapter().get_info().vendor == ARM;
    Renderer::new(
        &dev.device,
        RendererOptions {
            use_cpu: slow,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )
    .expect("can't create the renderer")
}

/// Where a window's drawings are shown.
pub struct Screen {
    cx: RenderContext,
    renderer: Option<Renderer>,
    surface: RenderSurface<'static>,
    /// Where the drawing's top left corner goes, in pixels (on a phone,
    /// below the status bar).
    pub origin: (f64, f64),
}

impl Screen {
    pub fn new(window: Arc<Window>) -> Result<Screen, String> {
        let size = window.inner_size();
        let mut cx = RenderContext::new();
        let surface = pollster::block_on(cx.create_surface(window, size.width.max(1), size.height.max(1), wgpu::PresentMode::AutoVsync))
            .map_err(|e| e.to_string())?;
        Ok(Screen { cx, renderer: None, surface, origin: (0.0, 0.0) })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.cx.resize_surface(&mut self.surface, width.max(1), height.max(1));
    }

    /// Shows a drawing (in pixels) over `background`. False if the window
    /// wasn't ready: draw again on the next frame.
    pub fn present(&mut self, list: &DrawList, background: Rgba) -> bool {
        let mut scene = Scene::new();
        play(list, &mut scene, Affine::translate(self.origin));
        let surface = &self.surface;
        let dev = &self.cx.devices[surface.dev_id];
        let renderer = self.renderer.get_or_insert_with(|| renderer(dev));
        let (w, h) = (surface.config.width, surface.config.height);
        let params = RenderParams { base_color: color(background), width: w, height: h, antialiasing_method: AaConfig::Area };
        if renderer.render_to_texture(&dev.device, &dev.queue, &scene, &surface.target_view, &params).is_err() {
            return true;
        }
        let frame = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                // Outdated or lost: reconfigure and try on the next frame.
                self.cx.configure_surface(&self.surface);
                return false;
            }
        };
        let mut encoder = dev.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        surface.blitter.copy(&dev.device, &mut encoder, &surface.target_view, &view);
        dev.queue.submit([encoder.finish()]);
        frame.present();
        true
    }
}

/// Renders drawings off the screen (for snapshots and `renderImage`).
pub struct Offscreen {
    cx: RenderContext,
    renderer: Option<Renderer>,
}

impl Default for Offscreen {
    fn default() -> Offscreen {
        Offscreen { cx: RenderContext::new(), renderer: None }
    }
}

impl Offscreen {
    /// A drawing's pixels, four bytes each (red, green, blue, opacity), row
    /// by row, over `base` (`None`: clear). Colors that aren't solid are as
    /// they are, not multiplied by their opacity.
    pub fn rgba(&mut self, list: &DrawList, width: u32, height: u32, base: Option<Rgba>) -> Vec<u8> {
        let mut scene = Scene::new();
        play(list, &mut scene, Affine::IDENTITY);
        let base = base.map_or(Color::TRANSPARENT, color);
        let mut pixels = render_pixels(&mut self.cx, &mut self.renderer, &scene, width, height, base);
        // The GPU gives colors already multiplied by their opacity; files don't.
        for p in pixels.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        pixels
    }
}

/// Renders a scene off-screen, over `base`: its pixels, four bytes each
/// (red, green, blue, opacity), row by row.
fn render_pixels(cx: &mut RenderContext, renderer: &mut Option<Renderer>, scene: &Scene, width: u32, height: u32, base: Color) -> Vec<u8> {
    let dev = pollster::block_on(cx.device(None)).expect("no GPU device");
    let handle = &cx.devices[dev];
    let (device, queue) = (&handle.device, &handle.queue);
    let renderer = renderer.get_or_insert_with(|| self::renderer(handle));
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

/// Writes pages as a PDF (see `ui::pdf`).
pub fn write_pdf(path: &str, title: &str, pages: &[super::canvas::Page]) -> bool {
    super::pdf::write(path, title, pages, &text::PdfSource::default())
}
