//! Drawing: turns a laid-out tree into a vello scene, and renders scenes
//! with the GPU (to a window, or off-screen to a PNG for tests).

use std::collections::HashMap;
use std::num::NonZeroUsize;

use parley::{FontContext, GenericFamily, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello::kurbo::{Affine, Circle, RoundedRect, Stroke};
use vello::peniko::{Color, Fill, Mix};
use vello::util::RenderContext;
use vello::{wgpu, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

use super::layout::Rect;
use super::{Kind, Node, Rgba};

/// Text color carried through parley layouts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paint(pub [u8; 4]);

pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<Paint>,
}

impl TextEngine {
    pub fn new() -> TextEngine {
        TextEngine { fonts: FontContext::new(), layouts: LayoutContext::new() }
    }

    pub fn layout(&mut self, text: &str, size: f64, weight: f32, color: Rgba, max_w: Option<f64>) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::SystemUi));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        let mut layout = b.build(text);
        layout.break_all_lines(max_w.map(|w| w as f32 + 0.5));
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }
}

/// What the user is doing right now, which changes how views are drawn.
#[derive(Default)]
pub struct Interaction {
    pub hovered: Option<u64>,
    pub pressed: Option<u64>,
    pub focused: Option<u64>,
    pub scroll: HashMap<u64, f64>,
}

const ACCENT: Rgba = Rgba([0, 122, 255, 255]);
const CONTROL: Rgba = Rgba([232, 232, 237, 255]);
const CONTROL_HOVER: Rgba = Rgba([221, 221, 227, 255]);
const CONTROL_PRESSED: Rgba = Rgba([205, 205, 212, 255]);
const FIELD_BORDER: Rgba = Rgba([199, 199, 204, 255]);
const PLACEHOLDER: Rgba = Rgba([160, 160, 166, 255]);
const SWITCH_ON: Rgba = Rgba([52, 199, 89, 255]);
const WHITE: Rgba = Rgba([255, 255, 255, 255]);
pub const BACKGROUND: Rgba = Rgba([255, 255, 255, 255]);

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.0[0], c.0[1], c.0[2], c.0[3])
}

fn rounded(r: Rect, radius: f64) -> RoundedRect {
    RoundedRect::new(r.x, r.y, r.x + r.w, r.y + r.h, radius)
}

pub struct Painter<'a> {
    pub scene: &'a mut Scene,
    pub ui: &'a Interaction,
    /// Points to pixels.
    pub scale: f64,
}

impl Painter<'_> {
    fn t(&self) -> Affine {
        Affine::scale(self.scale)
    }

    fn fill(&mut self, r: Rect, radius: f64, c: Rgba) {
        let t = self.t();
        self.scene.fill(Fill::NonZero, t, color(c), None, &rounded(r, radius));
    }

    fn stroke(&mut self, r: Rect, radius: f64, width: f64, c: Rgba) {
        let t = self.t();
        let inner = r.inset(width / 2.0);
        self.scene.stroke(&Stroke::new(width), t, color(c), None, &rounded(inner, radius));
    }

    fn text(&mut self, layout: &parley::Layout<Paint>, x: f64, y: f64, tint: Option<Rgba>) {
        let t = self.t() * Affine::translate((x, y));
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let brush = tint.map(|c| c.0).unwrap_or(run.style().brush.0);
                let r = run.run();
                self.scene
                    .draw_glyphs(r.font())
                    .brush(Color::from_rgba8(brush[0], brush[1], brush[2], brush[3]))
                    .hint(false)
                    .transform(t)
                    .font_size(r.font_size())
                    .normalized_coords(r.normalized_coords())
                    .draw(
                        Fill::NonZero,
                        run.positioned_glyphs().map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y }),
                    );
            }
        }
    }

    pub fn paint(&mut self, n: &Node) {
        if n.mods.hidden {
            return;
        }
        let r = n.layout.rect;
        let style = n.layout.style;
        let dim = if style.disabled && !n.children.is_empty() { 1.0 } else if style.disabled { 0.4 } else { 1.0 };
        let alpha = n.mods.opacity.unwrap_or(1.0) * dim;
        let layered = alpha < 1.0;
        if layered {
            let t = self.t();
            self.scene.push_layer(Fill::NonZero, Mix::Normal, alpha as f32, t, &rounded(r, 0.0));
        }
        if let Some((c, radius)) = n.mods.background {
            self.fill(r, radius, c);
        }
        let content = r.inset(n.mods.padding.unwrap_or(0.0));
        let hovered = self.ui.hovered == Some(n.key) && !style.disabled;
        let pressed = self.ui.pressed == Some(n.key) && !style.disabled;
        match &n.kind {
            Kind::Text(_) => {
                if let Some(t) = &n.layout.text {
                    self.text(t, content.x, content.y, None);
                }
            }
            Kind::Button { .. } => {
                if n.mods.background.is_none() {
                    let bg = if pressed { CONTROL_PRESSED } else if hovered { CONTROL_HOVER } else { CONTROL };
                    self.fill(content, 7.0, bg);
                }
                if let Some(t) = &n.layout.text {
                    let x = content.x + (content.w - t.width() as f64) / 2.0;
                    let y = content.y + (content.h - t.height() as f64) / 2.0;
                    let tint = if n.mods.color.is_none() { Some(ACCENT) } else { None };
                    self.text(t, x, y, tint);
                }
            }
            Kind::TextField { text, .. } => {
                let focused = self.ui.focused == Some(n.key);
                self.fill(content, 6.0, WHITE);
                if focused {
                    self.stroke(content, 6.0, 2.0, ACCENT);
                } else {
                    self.stroke(content, 6.0, 1.0, FIELD_BORDER);
                }
                let empty = super::layout::read_string(text).is_empty();
                if let Some(t) = &n.layout.text {
                    let x = content.x + 8.0;
                    let y = content.y + (content.h - t.height() as f64) / 2.0;
                    let t_scene = self.t();
                    self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(content.inset(2.0), 4.0));
                    self.text(t, x, y, if empty { Some(PLACEHOLDER) } else { None });
                    if focused {
                        let cx = if empty { x } else { x + t.width() as f64 + 1.0 };
                        let caret = Rect { x: cx, y: y + 1.0, w: 1.5, h: t.height() as f64 - 2.0 };
                        self.fill(caret, 0.0, ACCENT);
                    }
                    self.scene.pop_layer();
                }
            }
            Kind::Toggle { is_on, .. } => {
                let on = unsafe { *is_on.locate() } & 1 != 0;
                let sw = Rect { x: content.x + content.w - 38.0, y: content.y + (content.h - 22.0) / 2.0, w: 38.0, h: 22.0 };
                self.fill(sw, 11.0, if on { SWITCH_ON } else { CONTROL_PRESSED });
                let knob_x = if on { sw.x + sw.w - 11.0 } else { sw.x + 11.0 };
                let t = self.t();
                self.scene.fill(Fill::NonZero, t, color(WHITE), None, &Circle::new((knob_x, sw.y + 11.0), 9.0));
                if let Some(label) = &n.layout.text {
                    self.text(label, content.x, content.y + (content.h - label.height() as f64) / 2.0, None);
                }
            }
            Kind::Picker { selection } => {
                self.fill(content, 8.0, CONTROL);
                let current = selection.locate();
                for (c, seg) in n.children.iter().zip(&n.layout.segments) {
                    let selected = c.mods.tag.as_ref().is_some_and(|tag| tag.equals(current));
                    if selected {
                        self.fill(*seg, 6.0, WHITE);
                        self.stroke(*seg, 6.0, 0.5, FIELD_BORDER);
                    }
                    self.paint(c);
                }
            }
            Kind::Image(_) => {
                self.fill(content, 8.0, CONTROL);
                self.stroke(content, 8.0, 1.0, FIELD_BORDER);
            }
            Kind::Scroll => {
                let t = self.t();
                self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t, &rounded(content, 0.0));
                for c in &n.children {
                    self.paint(c);
                }
                self.scene.pop_layer();
            }
            _ => {}
        }
        if !matches!(n.kind, Kind::Picker { .. } | Kind::Scroll) {
            for c in &n.children {
                self.paint(c);
            }
        }
        if let Some((c, width, radius)) = n.mods.border {
            self.stroke(r, radius, width, c);
        }
        if layered {
            self.scene.pop_layer();
        }
    }
}

// ===== GPU =====

pub fn renderer(device: &wgpu::Device) -> Renderer {
    Renderer::new(
        device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )
    .expect("can't create the renderer")
}

pub fn params(width: u32, height: u32) -> RenderParams {
    RenderParams { base_color: color(BACKGROUND), width, height, antialiasing_method: AaConfig::Area }
}

/// Renders a scene off-screen and saves it as a PNG.
pub fn render_png(cx: &mut RenderContext, renderer: &mut Option<Renderer>, scene: &Scene, width: u32, height: u32, path: &str) {
    let dev = pollster::block_on(cx.device(None)).expect("no GPU device");
    let handle = &cx.devices[dev];
    let (device, queue) = (&handle.device, &handle.queue);
    let renderer = renderer.get_or_insert_with(|| self::renderer(device));
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
    renderer.render_to_texture(device, queue, scene, &view, &params(width, height)).expect("render failed");
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
    let file = std::fs::File::create(path).expect("can't create snapshot file");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(&pixels)).expect("can't write snapshot");
}
