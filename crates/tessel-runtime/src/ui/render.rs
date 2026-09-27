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

    /// Code: monospaced, not wrapped, with syntax colors.
    pub fn code_layout(&mut self, text: &str, size: f64) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::Brush(Paint(super::highlight::PLAIN.0)));
        for (range, color) in super::highlight::highlight(text) {
            b.push(StyleProperty::Brush(Paint(color.0)), range);
        }
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(parley::Alignment::Start, parley::AlignmentOptions::default());
        layout
    }

    /// Monospaced gray text (line numbers).
    pub fn mono_layout(&mut self, text: &str, size: f64, color: Rgba) -> parley::Layout<Paint> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::from(GenericFamily::Monospace));
        b.push_default(StyleProperty::FontSize(size as f32));
        b.push_default(StyleProperty::Brush(Paint(color.0)));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(parley::Alignment::End, parley::AlignmentOptions::default());
        layout
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
    /// The control (button, field, tappable view, …) under the pointer.
    pub hovered: Option<u64>,
    /// The innermost view with `.hoverBackground` under the pointer.
    pub hover_background: Option<u64>,
    pub pressed: Option<u64>,
    /// The text field being edited.
    pub editing: Option<super::edit::Editing>,
    pub scroll: HashMap<u64, f64>,
    /// Horizontal scroll of code editors, by key.
    pub scroll_x: HashMap<u64, f64>,
    /// The caret line each code editor last reported to its `line:` binding.
    pub editor_line: HashMap<u64, i64>,
    /// Where the mouse is (for hover effects inside views, like tabs).
    pub pointer: Option<(f64, f64)>,
    /// Undo and redo history of text fields and code editors, by key.
    pub history: HashMap<u64, super::edit::History>,
}

const ACCENT: Rgba = Rgba([0, 122, 255, 255]);
const CONTROL: Rgba = Rgba([232, 232, 237, 255]);
const CONTROL_HOVER: Rgba = Rgba([221, 221, 227, 255]);
const CONTROL_PRESSED: Rgba = Rgba([205, 205, 212, 255]);
const FIELD_BORDER: Rgba = Rgba([199, 199, 204, 255]);
const PLACEHOLDER: Rgba = Rgba([160, 160, 166, 255]);
const SWITCH_ON: Rgba = Rgba([52, 199, 89, 255]);
const WHITE: Rgba = Rgba([255, 255, 255, 255]);
const SELECTION: Rgba = Rgba([179, 215, 255, 255]);
const GUTTER: Rgba = Rgba([245, 245, 247, 255]);
const ERROR_LINE: Rgba = Rgba([255, 235, 235, 255]);
const ERROR_MARK: Rgba = Rgba([255, 59, 48, 255]);
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

    /// Draws `svg` centered in `r`, scaled to fit.
    fn draw_svg(&mut self, svg: &super::svg::Svg, r: Rect) {
        let s = (r.w / svg.width.max(1.0)).min(r.h / svg.height.max(1.0));
        let x = r.x + (r.w - svg.width * s) / 2.0;
        let y = r.y + (r.h - svg.height * s) / 2.0;
        let t = self.t() * Affine::translate((x, y)) * Affine::scale(s);
        self.scene.append(&svg.scene, Some(t));
    }

    /// Draws the built-in icon `name` in `r`.
    fn svg(&mut self, name: &str, r: Rect, c: Rgba) {
        if let Some(svg) = super::svg::icon(name, c) {
            self.draw_svg(&svg, r);
        }
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
        if let (Some((c, radius)), true) = (n.mods.hover_background, self.ui.hover_background == Some(n.key)) {
            self.fill(r, radius, c);
        }
        let content = r.inset_by(n.mods.padding);
        let hovered = self.ui.hovered == Some(n.key) && !style.disabled;
        let pressed = self.ui.pressed == Some(n.key) && !style.disabled;
        match &n.kind {
            Kind::Text(_) => {
                if let Some(t) = &n.layout.text {
                    let c = n.layout.content;
                    self.text(t, c.x, c.y, None);
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
            Kind::TextField { .. } => {
                let editing = self.ui.editing.as_ref().filter(|e| e.key == n.key);
                self.fill(content, 6.0, WHITE);
                if editing.is_some() {
                    self.stroke(content, 6.0, 2.0, ACCENT);
                } else {
                    self.stroke(content, 6.0, 1.0, FIELD_BORDER);
                }
                if let (Some(t), Some(ph)) = (&n.layout.text, &n.layout.placeholder) {
                    let (x0, y0) = field_origin(n);
                    let line_h = (ph.height() as f64).max(t.height() as f64);
                    let t_scene = self.t();
                    self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(content.inset(2.0), 4.0));
                    let scroll = editing.map_or(0.0, |e| e.scroll);
                    if let Some(e) = editing {
                        let (caret, rects) = super::edit::geometry(t, e);
                        for r in rects {
                            let sel = Rect { x: x0 - scroll + r.x0, y: y0 + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 };
                            self.fill(sel, 2.0, SELECTION);
                        }
                        let caret = match caret {
                            Some(c) => Rect { x: x0 - scroll + c.x0, y: y0 + c.y0, w: 1.5, h: c.y1 - c.y0 },
                            None => Rect { x: x0, y: y0, w: 1.5, h: line_h },
                        };
                        if e.range().is_empty() {
                            self.fill(caret, 0.0, ACCENT);
                        }
                    }
                    if n.layout.empty {
                        self.text(ph, x0, y0, Some(PLACEHOLDER));
                    } else {
                        self.text(t, x0 - scroll, y0, None);
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
            Kind::CodeEditor { error_lines, .. } => {
                let editing = self.ui.editing.as_ref().filter(|e| e.key == n.key);
                let t_scene = self.t();
                self.fill(content, 6.0, WHITE);
                self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(content, 6.0));
                let gutter = Rect { x: content.x, y: content.y, w: n.layout.gutter, h: content.h };
                self.fill(gutter, 0.0, GUTTER);
                let (x0, y0) = code_origin(n);
                let scroll_y = self.ui.scroll.get(&n.key).copied().unwrap_or(0.0);
                let scroll_x = self.ui.scroll_x.get(&n.key).copied().unwrap_or(0.0);
                if let Some(t) = &n.layout.text {
                    let lines: Vec<(f64, f64)> = t
                        .lines()
                        .map(|l| (l.metrics().block_min_coord as f64, l.metrics().block_max_coord as f64))
                        .collect();
                    for &line in error_lines {
                        let Some(&(top, bottom)) = lines.get((line - 1).max(0) as usize) else { continue };
                        let y = y0 - scroll_y + top;
                        let band = Rect { x: content.x + n.layout.gutter, y, w: content.w - n.layout.gutter, h: bottom - top };
                        self.fill(band, 0.0, ERROR_LINE);
                        self.fill(Rect { x: content.x, y, w: 3.0, h: bottom - top }, 0.0, ERROR_MARK);
                    }
                    if let Some(numbers) = &n.layout.placeholder {
                        self.text(numbers, content.x + 4.0, y0 - scroll_y, None);
                    }
                    let code_area = Rect { x: content.x + n.layout.gutter, y: content.y, w: content.w - n.layout.gutter, h: content.h };
                    self.scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, t_scene, &rounded(code_area, 0.0));
                    if let Some(e) = editing {
                        let (caret, rects) = super::edit::geometry(t, e);
                        for r in rects {
                            let sel = Rect { x: x0 - scroll_x + r.x0, y: y0 - scroll_y + r.y0, w: r.x1 - r.x0, h: r.y1 - r.y0 };
                            self.fill(sel, 1.0, SELECTION);
                        }
                        if e.range().is_empty() {
                            let line_h = lines.first().map_or(16.0, |(a, b)| b - a);
                            let caret = match caret {
                                Some(c) => Rect { x: x0 - scroll_x + c.x0, y: y0 - scroll_y + c.y0, w: 1.5, h: c.y1 - c.y0 },
                                None => Rect { x: x0, y: y0, w: 1.5, h: line_h },
                            };
                            self.fill(caret, 0.0, ACCENT);
                        }
                    }
                    self.text(t, x0 - scroll_x, y0 - scroll_y, None);
                    self.scene.pop_layer();
                }
                self.scene.pop_layer();
                if editing.is_some() {
                    self.stroke(content, 6.0, 2.0, ACCENT);
                } else {
                    self.stroke(content, 6.0, 1.0, FIELD_BORDER);
                }
            }
            Kind::TabView { .. } => {
                let bar = Rect { x: content.x, y: content.y, w: content.w, h: super::layout::TAB_BAR };
                self.fill(bar, 0.0, super::COLORS[14]);
                self.fill(Rect { x: bar.x, y: bar.y + bar.h - 1.0, w: bar.w, h: 1.0 }, 0.0, super::COLORS[16]);
                let selected = n.selected_tab();
                for (i, tab) in n.layout.tabs.iter().enumerate() {
                    let r = tab.rect;
                    let hovered = self.ui.pointer.is_some_and(|(x, y)| r.contains(x, y));
                    if i == selected {
                        self.fill(r, 0.0, WHITE);
                        self.fill(Rect { x: r.x, y: r.y, w: r.w, h: 2.0 }, 0.0, ACCENT);
                    } else if hovered {
                        self.fill(r, 0.0, super::COLORS[13]);
                    }
                    self.fill(Rect { x: r.x + r.w - 1.0, y: r.y, w: 1.0, h: r.h }, 0.0, super::COLORS[16]);
                    let tint = if i == selected { None } else { Some(super::COLORS[1]) };
                    let ty = r.y + (r.h - tab.title.height() as f64) / 2.0;
                    self.text(&tab.title, r.x + 14.0, ty, tint);
                    if let (Some(c), true) = (tab.close, i == selected || hovered) {
                        let over = self.ui.pointer.is_some_and(|(x, y)| c.contains(x, y));
                        if over {
                            self.fill(c, 4.0, super::COLORS[13]);
                        }
                        self.svg("close", Rect { x: c.x + c.w / 2.0 - 6.0, y: c.y + c.h / 2.0 - 6.0, w: 12.0, h: 12.0 }, super::COLORS[1]);
                    }
                }
                if let Some(c) = n.children.get(selected) {
                    self.paint(c);
                }
            }
            Kind::Divider => {
                self.fill(r, 0.0, super::COLORS[16]);
            }
            Kind::Svg { source, .. } => {
                if let Some(svg) = super::svg::load(source, style.color) {
                    self.draw_svg(&svg, n.layout.content);
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
        if !matches!(n.kind, Kind::Picker { .. } | Kind::Scroll | Kind::TabView { .. }) {
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

/// Where a code editor's text starts (before scrolling).
pub fn code_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    (content.x + n.layout.gutter + 8.0, content.y + 6.0)
}

/// Where a text field's text starts (before horizontal scrolling).
pub fn field_origin(n: &Node) -> (f64, f64) {
    let content = n.layout.rect.inset_by(n.mods.padding);
    let line_h = match (&n.layout.text, &n.layout.placeholder) {
        (Some(t), Some(p)) => (p.height() as f64).max(t.height() as f64),
        _ => 0.0,
    };
    (content.x + 8.0, content.y + (content.h - line_h) / 2.0)
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
