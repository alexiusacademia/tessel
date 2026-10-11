//! `Scene3D`: a view that shows things in three dimensions.
//!
//! The view's block gets a `Scene` value and calls its methods (`s.box`,
//! `s.sphere`, `s.camera`, …), which arrive here as the `tsl_scene_*` calls
//! below, as a `Canvas`'s do. They say what's in the scene that is being
//! made (`CURRENT`); a call made at any other time does nothing.
//!
//! The scene is drawn by the graphics card (`gpu`): solid shapes with a
//! depth buffer, lit by the scene's lights, with smoothed edges. It's drawn
//! into a picture, which the window then shows like any other, so a scene
//! can be anywhere a view can: in a scroll view, under a popover, in a
//! picture a test takes.
//!
//! Coordinates are the scene's own: x to the right, y up, z towards the
//! viewer. The current transform (`s.translate`, `s.rotateY`, `s.scale`) is
//! applied to each shape as it's added. Angles are in radians.
//!
//! `Scene3D(redraw: n)` keeps its picture between paints, until `n` changes
//! (or the view's size, or the appearance). Without it the block runs at
//! each paint, and the picture is only made again when what the block said
//! has changed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use super::image::Picture;
use super::text::TextLayout;
use super::{theme, Callback, Rgba};
use crate::{Header, List, Str};

/// A transform: `m[row][column]`, applied to points as columns.
type Mat = [[f64; 4]; 4];
type V3 = [f64; 3];

const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn mul(a: &Mat, b: &Mat) -> Mat {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

fn point(m: &Mat, p: V3) -> V3 {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` at length 1 (or `otherwise`, for a `v` of no length).
fn unit(v: V3, otherwise: V3) -> V3 {
    let len = dot(v, v).sqrt();
    if len > 1e-12 && len.is_finite() { [v[0] / len, v[1] / len, v[2] / len] } else { otherwise }
}

/// What turns directions that stand on a surface (its normals) as `m` turns
/// the surface: the inverse of its turning and stretching part, transposed.
fn normals(m: &Mat) -> [[f64; 3]; 3] {
    let a = |r: usize, c: usize| m[r][c];
    let co = |r: usize, c: usize| {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)
    };
    // (The cofactors are that, times the determinant, whose size doesn't
    // matter to a direction: only its sign does.)
    let det = a(0, 0) * co(0, 0) + a(0, 1) * co(0, 1) + a(0, 2) * co(0, 2);
    let sign = if det < 0.0 { -1.0 } else { 1.0 };
    std::array::from_fn(|r| std::array::from_fn(|c| sign * co(r, c)))
}

/// The view from `eye` towards `target`, with `up` up.
fn look_at(eye: V3, target: V3, up: V3) -> Mat {
    let f = unit(sub(target, eye), [0.0, 0.0, -1.0]);
    let s = unit(cross(f, up), [1.0, 0.0, 0.0]);
    let u = cross(s, f);
    [
        [s[0], s[1], s[2], -dot(s, eye)],
        [u[0], u[1], u[2], -dot(u, eye)],
        [-f[0], -f[1], -f[2], dot(f, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// What's seen within `fov` (the angle up and down, in radians) from `near`
/// to `far`: nearer things larger, with depths from 0 to 1.
fn perspective(fov: f64, aspect: f64, near: f64, far: f64) -> Mat {
    let f = 1.0 / (fov / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), near * far / (near - far)],
        [0.0, 0.0, -1.0, 0.0],
    ]
}

/// A corner of a triangle, as the graphics card takes it: where it is, the
/// direction its surface faces, and its color (red, green, blue as light,
/// not as a screen's numbers).
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    at: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

/// An end of a line.
#[repr(C)]
#[derive(Clone, Copy)]
struct LineEnd {
    at: [f32; 3],
    color: [f32; 4],
}

/// A screen's color number (0 to 255) as an amount of light (0 to 1).
fn light(channel: u8) -> f32 {
    let c = channel as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn lit(c: Rgba) -> [f32; 4] {
    [light(c.0[0]), light(c.0[1]), light(c.0[2]), c.0[3] as f32 / 255.0]
}

struct Camera {
    eye: V3,
    target: V3,
    /// The angle seen from top to bottom, in degrees.
    fov: f64,
}

/// A text placed at a point of the scene, drawn over the picture.
struct Label {
    text: String,
    at: V3,
    size: f64,
    color: Rgba,
}

/// The scene being made.
struct Current {
    triangles: Vec<Vertex>,
    lines: Vec<LineEnd>,
    transform: Mat,
    saved: Vec<Mat>,
    camera: Camera,
    /// Each light: the direction it comes from, and its color.
    lights: Vec<(V3, [f32; 3])>,
    ambient: Option<f32>,
    labels: Vec<Label>,
    /// What's behind everything (clear: what's under the view shows).
    background: Rgba,
    shapes: usize,
}

/// A label, where it shows in the view (in points from its top left).
pub struct Placed {
    pub layout: Rc<TextLayout>,
    pub x: f64,
    pub y: f64,
    pub color: Rgba,
}

/// A finished scene: its picture (none when it couldn't be drawn), and the
/// labels to draw over it.
pub struct Shot {
    pub picture: Option<Rc<Picture>>,
    pub labels: Vec<Placed>,
    /// For `tree` and `dump` in headless runs.
    pub shapes: usize,
    pub triangles: usize,
    pub lines: usize,
}

impl Shot {
    /// "3 shapes, 36 triangles", or "empty".
    pub fn describe(&self) -> String {
        let part = |n: usize, one: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            _ => Some(format!("{n} {one}s")),
        };
        let parts: Vec<String> =
            [part(self.shapes, "shape"), part(self.triangles, "triangle"), part(self.lines, "line"), part(self.labels.len(), "label")]
                .into_iter()
                .flatten()
                .collect();
        if parts.is_empty() { "empty".to_string() } else { parts.join(", ") }
    }
}

struct Kept {
    redraw: Option<i64>,
    w: f64,
    h: f64,
    scale: f64,
    dark: bool,
    /// Of everything the picture was made from.
    made_from: u64,
    /// Whether its picture was asked for (a headless run's `tree` only
    /// asks what's in a scene).
    rendered: bool,
    shot: Rc<Shot>,
}

thread_local! {
    static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
    static KEPT: RefCell<HashMap<u64, Kept>> = RefCell::new(HashMap::new());
}

fn with(f: impl FnOnce(&mut Current)) {
    CURRENT.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            f(c);
        }
    });
}

/// What draws a frame: its pixels, four bytes each (red, green, blue,
/// opacity), row by row from the top. Only programs with a `Scene3D` have
/// one (`enable`), so only they carry the graphics card's code.
type Render = fn(&Frame) -> Option<Vec<u8>>;

static RENDER: OnceLock<Render> = OnceLock::new();

/// A scene as the graphics card draws it.
pub struct Frame<'a> {
    pub width: u32,
    pub height: u32,
    /// `Vertex`es, three to a triangle, and `LineEnd`s, two to a line.
    pub triangles: &'a [u8],
    pub lines: &'a [u8],
    /// The view and its lights (see `gpu`'s shader for what's where).
    pub uniforms: [f32; 56],
    /// What's behind everything, as light, with how much of it shows.
    pub background: [f32; 4],
}

/// Called when a program builds a `Scene3D`.
pub fn enable() {
    #[cfg(feature = "gpu")]
    let _ = RENDER.set(gpu::render);
}

fn bytes<T: Copy>(items: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(items.as_ptr() as *const u8, std::mem::size_of_val(items)) }
}

/// The scene of the view `key` (`w` × `h` points, at `scale` pixels to the
/// point), as its block describes it. Without `render`, only what's in
/// it is found out: nothing is drawn.
pub fn shot(key: u64, block: &Callback, redraw: Option<i64>, w: f64, h: f64, scale: f64, render: bool) -> Rc<Shot> {
    let dark = theme::is_dark();
    // (Asked only what's in it, a scene is as it was last drawn.)
    let scale = if render { scale } else { KEPT.with(|k| k.borrow().get(&key).map_or(scale, |k| k.scale)) };
    let same_view = |k: &Kept| k.w == w && k.h == h && k.scale == scale && k.dark == dark;
    if redraw.is_some() {
        let kept = KEPT.with(|k| k.borrow().get(&key).filter(|k| k.redraw == redraw && same_view(k) && (k.rendered || !render)).map(|k| k.shot.clone()));
        if let Some(shot) = kept {
            return shot;
        }
    }
    let current = Current {
        triangles: Vec::new(),
        lines: Vec::new(),
        transform: IDENTITY,
        saved: Vec::new(),
        camera: Camera { eye: [3.0, 2.5, 4.0], target: [0.0, 0.0, 0.0], fov: 45.0 },
        lights: Vec::new(),
        ambient: None,
        labels: Vec::new(),
        background: Rgba([0, 0, 0, 0]),
        shapes: 0,
    };
    let outer = CURRENT.with(|c| c.borrow_mut().replace(current));
    block.call_with_floats(w, h);
    let scene = CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), outer)).expect("the scene being made");

    // The view: from the camera, as far as the scene goes.
    let (eye, target) = (scene.camera.eye, scene.camera.target);
    let reach = scene
        .triangles
        .iter()
        .map(|v| v.at)
        .chain(scene.lines.iter().map(|l| l.at))
        .map(|p| {
            let d = sub([p[0] as f64, p[1] as f64, p[2] as f64], eye);
            dot(d, d).sqrt()
        })
        .fold(dot(sub(target, eye), sub(target, eye)).sqrt(), f64::max)
        .max(1e-3);
    let (near, far) = (reach / 2000.0, reach * 2.5);
    let aspect = if h > 0.0 { w / h } else { 1.0 };
    let view = look_at(eye, target, [0.0, 1.0, 0.0]);
    let seen = mul(&perspective(scene.camera.fov.clamp(1.0, 170.0).to_radians(), aspect, near, far), &view);

    // With no light of its own, a scene is lit from over the viewer's
    // left shoulder.
    let mut lights = scene.lights.clone();
    if lights.is_empty() {
        let back = unit(sub(eye, target), [0.0, 0.0, 1.0]);
        let right = unit(cross([0.0, 1.0, 0.0], back), [1.0, 0.0, 0.0]);
        let from = unit([back[0] * 0.6 - right[0] * 0.5, back[1] * 0.6 + 0.8, back[2] * 0.6 - right[2] * 0.5], [0.0, 1.0, 0.0]);
        lights.push((from, [0.85, 0.85, 0.85]));
    }
    lights.truncate(4);
    let ambient = scene.ambient.unwrap_or(0.3);

    let mut uniforms = [0.0f32; 56];
    for c in 0..4 {
        for r in 0..4 {
            // (Columns first, as the shader reads a matrix.)
            uniforms[c * 4 + r] = seen[r][c] as f32;
        }
    }
    uniforms[16..19].copy_from_slice(&[eye[0] as f32, eye[1] as f32, eye[2] as f32]);
    uniforms[20..24].copy_from_slice(&[ambient, ambient, ambient, lights.len() as f32]);
    for (i, (from, color)) in lights.iter().enumerate() {
        uniforms[24 + i * 4..27 + i * 4].copy_from_slice(&[from[0] as f32, from[1] as f32, from[2] as f32]);
        uniforms[40 + i * 4..43 + i * 4].copy_from_slice(color);
    }

    let pixels = |points: f64| ((points * scale).round() as u32).clamp(1, 4096);
    let shown = lit(scene.background);
    let frame = Frame {
        width: pixels(w),
        height: pixels(h),
        triangles: bytes(&scene.triangles),
        lines: bytes(&scene.lines),
        uniforms,
        background: [shown[0] * shown[3], shown[1] * shown[3], shown[2] * shown[3], shown[3]],
    };
    // The picture, unless what it's made from is what the last one was.
    let made_from = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (frame.width, frame.height, frame.triangles, frame.lines, bytes(&frame.uniforms), bytes(&frame.background)).hash(&mut h);
        h.finish()
    };
    let before =
        KEPT.with(|k| k.borrow().get(&key).filter(|k| k.rendered && k.made_from == made_from && same_view(k)).map(|k| k.shot.picture.clone()));
    let rendered = render || before.is_some();
    let picture = match before {
        Some(picture) => picture,
        None if !render || w < 1.0 || h < 1.0 => None,
        None => RENDER.get().and_then(|render| render(&frame)).and_then(|mut rgba| {
            straighten(&mut rgba);
            super::backend::picture_from_rgba(frame.width, frame.height, rgba).map(Rc::new)
        }),
    };

    // Labels: where their points show (in front of the camera).
    let labels = scene
        .labels
        .iter()
        .filter_map(|l| {
            let p = [l.at[0], l.at[1], l.at[2], 1.0];
            let clip: [f64; 4] = std::array::from_fn(|r| (0..4).map(|c| seen[r][c] * p[c]).sum());
            if clip[3] <= 1e-9 {
                return None;
            }
            let (x, y) = ((clip[0] / clip[3] + 1.0) / 2.0 * w, (1.0 - clip[1] / clip[3]) / 2.0 * h);
            let layout = Rc::new(super::canvas::with_text(|t| {
                t.styled(&l.text, l.size, 400.0, super::text::Face::default(), l.color, None, 0)
            }));
            Some(Placed { x: x - layout.width() / 2.0, y: y - layout.height() / 2.0, layout, color: l.color })
        })
        .collect();

    let shot = Rc::new(Shot { picture, labels, shapes: scene.shapes, triangles: scene.triangles.len() / 3, lines: scene.lines.len() / 2 });
    KEPT.with(|k| k.borrow_mut().insert(key, Kept { redraw, w, h, scale, dark, made_from, rendered, shot: shot.clone() }));
    shot
}

/// The graphics card smooths an edge by mixing what's on each side of it:
/// against a clear background, a pixel at an edge comes back darker by how
/// much of it is clear. A picture's colors are told apart from how much of
/// them shows, so those pixels are made as bright as their shape again.
fn straighten(rgba: &mut [u8]) {
    static BACK: OnceLock<[u8; 4096]> = OnceLock::new();
    // An amount of light as a screen's number.
    let screen = BACK.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 4095.0;
            let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round().clamp(0.0, 255.0) as u8
        })
    });
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 || a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            let whole = (light(*c) * 255.0 / a as f32).min(1.0);
            *c = screen[(whole * 4095.0).round() as usize];
        }
    }
}

/// Drops every picture kept (see `ui::release_caches`).
pub fn forget_all() {
    KEPT.with(|k| k.borrow_mut().clear());
    #[cfg(feature = "gpu")]
    gpu::forget();
}

/// Drops the pictures kept for scenes that aren't in the tree any more.
pub fn keep_only(keys: &HashSet<u64>) {
    KEPT.with(|k| k.borrow_mut().retain(|key, _| keys.contains(key)));
}

// ===== shapes =====

impl Current {
    /// A triangle with a direction for each corner's surface, in the
    /// shape's own coordinates.
    fn corner(&mut self, at: V3, normal: V3, color: [f32; 4], turn: &[[f64; 3]; 3]) {
        let p = point(&self.transform, at);
        let n = unit(std::array::from_fn(|r| turn[r][0] * normal[0] + turn[r][1] * normal[1] + turn[r][2] * normal[2]), [0.0, 1.0, 0.0]);
        self.triangles.push(Vertex { at: p.map(|v| v as f32), normal: n.map(|v| v as f32), color });
    }

    /// Triangles: `corners` (a place and its surface's direction), three
    /// at a time.
    fn solid(&mut self, corners: &[(V3, V3)], color: Rgba) {
        let (color, turn) = (lit(color), normals(&self.transform));
        for (at, normal) in corners {
            self.corner(*at, *normal, color, &turn);
        }
        self.shapes += 1;
    }

    fn line(&mut self, a: V3, b: V3, color: Rgba) {
        let color = lit(color);
        for p in [a, b] {
            self.lines.push(LineEnd { at: point(&self.transform, p).map(|v| v as f32), color });
        }
    }
}

/// A flat four-cornered face, as two triangles facing `normal`.
fn face(out: &mut Vec<(V3, V3)>, corners: [V3; 4], normal: V3) {
    for i in [0, 1, 2, 0, 2, 3] {
        out.push((corners[i], normal));
    }
}

fn box_corners(w: f64, h: f64, d: f64) -> Vec<(V3, V3)> {
    let (x, y, z) = (w / 2.0, h / 2.0, d / 2.0);
    let mut out = Vec::with_capacity(36);
    face(&mut out, [[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]], [0.0, 0.0, 1.0]);
    face(&mut out, [[x, -y, -z], [-x, -y, -z], [-x, y, -z], [x, y, -z]], [0.0, 0.0, -1.0]);
    face(&mut out, [[x, -y, z], [x, -y, -z], [x, y, -z], [x, y, z]], [1.0, 0.0, 0.0]);
    face(&mut out, [[-x, -y, -z], [-x, -y, z], [-x, y, z], [-x, y, -z]], [-1.0, 0.0, 0.0]);
    face(&mut out, [[-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]], [0.0, 1.0, 0.0]);
    face(&mut out, [[-x, -y, -z], [x, -y, -z], [x, -y, z], [-x, -y, z]], [0.0, -1.0, 0.0]);
    out
}

/// A ball: bands from pole to pole, each a ring of faces.
fn sphere_corners(radius: f64) -> Vec<(V3, V3)> {
    let (around, down) = (40, 20);
    let at = |i: usize, j: usize| -> (V3, V3) {
        let (turn, tilt) = (i as f64 / around as f64 * std::f64::consts::TAU, j as f64 / down as f64 * std::f64::consts::PI);
        let n = [tilt.sin() * turn.sin(), tilt.cos(), tilt.sin() * turn.cos()];
        (n.map(|v| v * radius), n)
    };
    let mut out = Vec::with_capacity(around * down * 6);
    for j in 0..down {
        for i in 0..around {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            out.extend([a, b, c, a, c, d]);
        }
    }
    out
}

/// A cylinder (`top` = `radius`) or a cone (`top` = 0) standing on its
/// middle, with its ends closed.
fn round_corners(radius: f64, top: f64, height: f64) -> Vec<(V3, V3)> {
    let around = 40;
    let (lo, hi) = (-height / 2.0, height / 2.0);
    // (A cone's side leans in: so does the direction it faces.)
    let lean = if height.abs() > 1e-12 { (radius - top) / height } else { 0.0 };
    let side = |i: usize, r: f64, y: f64| -> (V3, V3) {
        let turn = i as f64 / around as f64 * std::f64::consts::TAU;
        ([turn.sin() * r, y, turn.cos() * r], unit([turn.sin(), lean, turn.cos()], [0.0, 1.0, 0.0]))
    };
    let mut out = Vec::new();
    for i in 0..around {
        let (a, b, c, d) = (side(i, radius, lo), side(i + 1, radius, lo), side(i + 1, top, hi), side(i, top, hi));
        out.extend([a, b, c, a, c, d]);
        for (r, y, n) in [(radius, lo, -1.0), (top, hi, 1.0)] {
            if r > 0.0 {
                out.extend([([0.0, y, 0.0], [0.0, n, 0.0]), (side(i, r, y).0, [0.0, n, 0.0]), (side(i + 1, r, y).0, [0.0, n, 0.0])]);
            }
        }
    }
    out
}

/// The direction a triangle faces.
fn facing(a: V3, b: V3, c: V3) -> V3 {
    unit(cross(sub(b, a), sub(c, a)), [0.0, 1.0, 0.0])
}

/// A color between the ones of `colors`, `t` (0 to 1) of the way along.
fn between(colors: &[Rgba], t: f64) -> Rgba {
    let at = t.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
    let (i, part) = ((at.floor() as usize).min(colors.len() - 1), at.fract());
    let (a, b) = (colors[i], colors[(i + 1).min(colors.len() - 1)]);
    Rgba(std::array::from_fn(|k| (a.0[k] as f64 + (b.0[k] as f64 - a.0[k] as f64) * part).round() as u8))
}

// ===== what a block calls =====

/// A shape's own place, added to the transform for the shape only.
fn placed(c: &mut Current, x: f64, y: f64, z: f64, shape: impl FnOnce(&mut Current)) {
    let before = c.transform;
    c.transform = mul(&c.transform, &[[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]]);
    shape(c);
    c.transform = before;
}

/// `s.box(width:height:depth:color:x:y:z:)`: a box around (x, y, z).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_box(w: f64, h: f64, d: f64, tag: i32, payload: *const Header, x: f64, y: f64, z: f64) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| placed(c, x, y, z, |c| c.solid(&box_corners(w, h, d), color)));
}

/// `s.sphere(radius:color:x:y:z:)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_sphere(radius: f64, tag: i32, payload: *const Header, x: f64, y: f64, z: f64) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| placed(c, x, y, z, |c| c.solid(&sphere_corners(radius), color)));
}

/// `s.cylinder(radius:height:color:x:y:z:)`: standing up, (x, y, z) at its middle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_cylinder(radius: f64, height: f64, tag: i32, payload: *const Header, x: f64, y: f64, z: f64) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| placed(c, x, y, z, |c| c.solid(&round_corners(radius, radius, height), color)));
}

/// `s.cone(radius:height:color:x:y:z:)`: its point up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_cone(radius: f64, height: f64, tag: i32, payload: *const Header, x: f64, y: f64, z: f64) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| placed(c, x, y, z, |c| c.solid(&round_corners(radius, 0.0, height), color)));
}

/// `s.plane(width:depth:color:x:y:z:)`: a flat sheet, lying down.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_plane(w: f64, d: f64, tag: i32, payload: *const Header, x: f64, y: f64, z: f64) {
    let color = unsafe { theme::color(tag, payload) };
    let (hx, hz) = (w / 2.0, d / 2.0);
    let mut corners = Vec::new();
    face(&mut corners, [[-hx, 0.0, hz], [hx, 0.0, hz], [hx, 0.0, -hz], [-hx, 0.0, -hz]], [0.0, 1.0, 0.0]);
    with(|c| placed(c, x, y, z, |c| c.solid(&corners, color)));
}

/// `s.mesh(vertices:triangles:color:smooth:)`: triangles of the program's
/// own. `vertices` is x, y, z for each point; `triangles` is three points'
/// numbers for each triangle (without it, every three points are one).
/// `smooth` shades across the edges between triangles that share points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_mesh(vertices: *const List, triangles: *const List, tag: i32, payload: *const Header, smooth: u8) {
    let color = unsafe { theme::color(tag, payload) };
    let (flat, index) = unsafe { (super::canvas::floats(vertices), super::canvas::ints(triangles)) };
    let points: Vec<V3> = flat.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    let numbers: Vec<usize> = match index.is_empty() {
        true => (0..points.len() / 3 * 3).collect(),
        false => index.iter().take(index.len() / 3 * 3).map(|i| *i as usize).collect(),
    };
    if numbers.iter().any(|i| *i >= points.len()) {
        return;
    }
    // Each point faces the way its triangles do, taken together.
    let mut shared = vec![[0.0; 3]; if smooth != 0 { points.len() } else { 0 }];
    if smooth != 0 {
        for t in numbers.chunks_exact(3) {
            let n = cross(sub(points[t[1]], points[t[0]]), sub(points[t[2]], points[t[0]]));
            for i in t {
                shared[*i] = [shared[*i][0] + n[0], shared[*i][1] + n[1], shared[*i][2] + n[2]];
            }
        }
    }
    let mut corners = Vec::with_capacity(numbers.len());
    for t in numbers.chunks_exact(3) {
        let own = facing(points[t[0]], points[t[1]], points[t[2]]);
        for i in t {
            corners.push((points[*i], if smooth != 0 { unit(shared[*i], own) } else { own }));
        }
    }
    with(|c| c.solid(&corners, color));
}

/// `s.surface(heights:columns:width:depth:color:colors:)`: a sheet over a
/// grid, `columns` across, whose height at each point of the grid is in
/// `heights`, row by row from the back. With `colors`, each point is
/// colored by its height, from the first color (the lowest) to the last.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_surface(
    heights: *const List,
    columns: i64,
    w: f64,
    d: f64,
    tag: i32,
    payload: *const Header,
    colors: *const List,
) {
    let color = unsafe { theme::color(tag, payload) };
    let (heights, colors) = unsafe { (super::canvas::floats(heights), super::canvas::colors(colors)) };
    let cols = columns.clamp(0, 4096) as usize;
    if cols < 2 || heights.len() / cols < 2 {
        return;
    }
    let rows = heights.len() / cols;
    let at = |i: usize, j: usize| -> V3 { [(i as f64 / (cols - 1) as f64 - 0.5) * w, heights[j * cols + i], (j as f64 / (rows - 1) as f64 - 0.5) * d] };
    let (lowest, highest) = heights[..rows * cols].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    // Each point faces the way the sheet slopes there.
    let normal = |i: usize, j: usize| -> V3 {
        let (left, right) = (at(i.saturating_sub(1), j), at((i + 1).min(cols - 1), j));
        let (back, front) = (at(i, j.saturating_sub(1)), at(i, (j + 1).min(rows - 1)));
        unit(cross(sub(front, back), sub(right, left)), [0.0, 1.0, 0.0])
    };
    with(|c| {
        let turn = normals(&c.transform);
        let corner = |c: &mut Current, i: usize, j: usize| {
            let p = at(i, j);
            let tint = match colors.is_empty() {
                true => color,
                false => between(&colors, if highest > lowest { (p[1] - lowest) / (highest - lowest) } else { 0.0 }),
            };
            c.corner(p, normal(i, j), lit(tint), &turn);
        };
        for j in 0..rows - 1 {
            for i in 0..cols - 1 {
                for (di, dj) in [(0, 0), (0, 1), (1, 1), (0, 0), (1, 1), (1, 0)] {
                    corner(c, i + di, j + dj);
                }
            }
        }
        c.shapes += 1;
    });
}

/// `s.line(x1, y1, z1, x2, y2, z2, color:)`: a thin line.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_line(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64, tag: i32, payload: *const Header) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| c.line([x1, y1, z1], [x2, y2, z2], color));
}

/// `s.lines(points, color:closed:)`: a line through points (x, y, z each).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_lines(points: *const List, tag: i32, payload: *const Header, closed: u8) {
    let color = unsafe { theme::color(tag, payload) };
    let points: Vec<V3> = unsafe { super::canvas::floats(points) }.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    with(|c| {
        for pair in points.windows(2) {
            c.line(pair[0], pair[1], color);
        }
        if let (true, Some(first), Some(last)) = (closed != 0 && points.len() > 2, points.first(), points.last()) {
            c.line(*last, *first, color);
        }
    });
}

/// `s.label(text, x:y:z:size:color:)`: text at a point of the scene, facing
/// the viewer, always the same size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_label(text: *const Str, x: f64, y: f64, z: f64, size: f64, tag: i32, payload: *const Header) {
    let text = unsafe { crate::str_text(text) }.to_string();
    let color = unsafe { theme::color(tag, payload) };
    if text.is_empty() || size <= 0.0 {
        return;
    }
    with(|c| {
        let at = point(&c.transform, [x, y, z]);
        c.labels.push(Label { text, at, size, color });
    });
}

/// `s.camera(x:y:z:lookAtX:lookAtY:lookAtZ:fieldOfView:)`: where the scene
/// is seen from, and what's at the middle of the view.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_camera(x: f64, y: f64, z: f64, tx: f64, ty: f64, tz: f64, fov: f64) {
    with(|c| c.camera = Camera { eye: [x, y, z], target: [tx, ty, tz], fov });
}

/// `s.orbit(distance:yaw:pitch:x:y:z:fieldOfView:)`: the camera on a ball
/// around (x, y, z): `yaw` around it, `pitch` above it.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_orbit(distance: f64, yaw: f64, pitch: f64, x: f64, y: f64, z: f64, fov: f64) {
    // (Never straight above or below: which way is up there?)
    let pitch = pitch.clamp(-1.55, 1.55);
    let eye = [x + distance * pitch.cos() * yaw.sin(), y + distance * pitch.sin(), z + distance * pitch.cos() * yaw.cos()];
    with(|c| c.camera = Camera { eye, target: [x, y, z], fov });
}

/// `s.light(x:y:z:color:strength:)`: light from the direction of (x, y, z).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_light(x: f64, y: f64, z: f64, tag: i32, payload: *const Header, strength: f64) {
    let color = lit(unsafe { theme::color(tag, payload) });
    let strength = strength.clamp(0.0, 10.0) as f32;
    with(|c| c.lights.push((unit([x, y, z], [0.0, 1.0, 0.0]), [color[0] * strength, color[1] * strength, color[2] * strength])));
}

/// `s.ambient(amount)`: how much light there is everywhere, from 0 to 1.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_ambient(amount: f64) {
    with(|c| c.ambient = Some(amount.clamp(0.0, 1.0) as f32));
}

/// `s.background(color)`: what's behind everything. Without it, a scene
/// shows what's under its view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_scene_background(tag: i32, payload: *const Header) {
    let color = unsafe { theme::color(tag, payload) };
    with(|c| c.background = color);
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_save() {
    with(|c| c.saved.push(c.transform));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_restore() {
    with(|c| {
        if let Some(t) = c.saved.pop() {
            c.transform = t;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_translate(x: f64, y: f64, z: f64) {
    with(|c| c.transform = mul(&c.transform, &[[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z], [0.0, 0.0, 0.0, 1.0]]));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_scale(x: f64, y: f64, z: f64) {
    with(|c| c.transform = mul(&c.transform, &[[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0], [0.0, 0.0, 0.0, 1.0]]));
}

/// A turn by `angle` around the axis `axis` (0 x, 1 y, 2 z).
fn turn(axis: usize, angle: f64) -> Mat {
    let (s, c) = angle.sin_cos();
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut m = IDENTITY;
    (m[a][a], m[a][b], m[b][a], m[b][b]) = (c, -s, s, c);
    m
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_rotate_x(angle: f64) {
    with(|c| c.transform = mul(&c.transform, &turn(0, angle)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_rotate_y(angle: f64) {
    with(|c| c.transform = mul(&c.transform, &turn(1, angle)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_scene_rotate_z(angle: f64) {
    with(|c| c.transform = mul(&c.transform, &turn(2, angle)));
}

// ===== the graphics card =====

#[cfg(feature = "gpu")]
mod gpu {
    use super::Frame;

    /// What's seen, and its lights: `Frame::uniforms`.
    const SHADER: &str = r#"
struct Scene {
    seen: mat4x4<f32>,
    eye: vec4<f32>,
    // The light everywhere, and (in w) how many lights there are.
    ambient: vec4<f32>,
    light_from: array<vec4<f32>, 4>,
    light_color: array<vec4<f32>, 4>,
}
@group(0) @binding(0) var<uniform> scene: Scene;

struct Corner {
    @builtin(position) place: vec4<f32>,
    @location(0) at: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn solid_corner(@location(0) at: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Corner {
    var out: Corner;
    out.place = scene.seen * vec4<f32>(at, 1.0);
    out.at = at;
    out.normal = normal;
    out.color = color;
    return out;
}

@fragment
fn solid(corner: Corner) -> @location(0) vec4<f32> {
    let towards = normalize(scene.eye.xyz - corner.at);
    var normal = normalize(corner.normal);
    // (A sheet is lit on the side it's seen from.)
    if dot(normal, towards) < 0.0 {
        normal = -normal;
    }
    var light = scene.ambient.xyz;
    var shine = vec3<f32>(0.0);
    let count = i32(scene.ambient.w);
    for (var i = 0; i < count; i++) {
        let source = normalize(scene.light_from[i].xyz);
        let facing = max(dot(normal, source), 0.0);
        light += facing * scene.light_color[i].xyz;
        if facing > 0.0 {
            let middle = normalize(source + towards);
            shine += pow(max(dot(normal, middle), 0.0), 40.0) * 0.22 * scene.light_color[i].xyz;
        }
    }
    return vec4<f32>(corner.color.rgb * light + shine, 1.0);
}

struct End {
    @builtin(position) place: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn line_end(@location(0) at: vec3<f32>, @location(1) color: vec4<f32>) -> End {
    var out: End;
    out.place = scene.seen * vec4<f32>(at, 1.0);
    out.color = color;
    return out;
}

@fragment
fn line(end: End) -> @location(0) vec4<f32> {
    return vec4<f32>(end.color.rgb, 1.0);
}
"#;

    const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
    const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
    /// How many points of a pixel are looked at, to smooth edges.
    const SAMPLES: u32 = 4;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        solids: wgpu::RenderPipeline,
        lines: wgpu::RenderPipeline,
        layout: wgpu::BindGroupLayout,
    }

    thread_local! {
        /// The graphics card, once it's been asked for (none inside: there
        /// isn't one). It's let go by `forget`, never when the thread ends:
        /// by then what it needs to go may be gone.
        static GPU: std::cell::Cell<Option<&'static Option<Gpu>>> = const { std::cell::Cell::new(None) };
    }

    pub fn forget() {
        if let Some(gpu) = GPU.take() {
            drop(unsafe { Box::from_raw(gpu as *const Option<Gpu> as *mut Option<Gpu>) });
        }
    }

    fn start() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("scene"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = |vertex: &str, fragment: &str, attributes: &[wgpu::VertexAttribute], stride: u64, topology: wgpu::PrimitiveTopology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers: &[wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Vertex, attributes }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                // (Both sides of a triangle show: a sheet has two.)
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        let line = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let solids = pipeline("solid_corner", "solid", &solid, 40, wgpu::PrimitiveTopology::TriangleList);
        let lines = pipeline("line_end", "line", &line, 28, wgpu::PrimitiveTopology::LineList);
        Some(Gpu { device, queue, solids, lines, layout })
    }

    /// Draws a frame, and reads its picture back.
    pub fn render(frame: &Frame) -> Option<Vec<u8>> {
        let gpu = GPU.get().unwrap_or_else(|| {
            let started = start();
            if started.is_none() {
                eprintln!("warning: there's no graphics card to draw a Scene3D with");
            }
            let kept: &'static Option<Gpu> = Box::leak(Box::new(started));
            GPU.set(Some(kept));
            kept
        });
        Some(draw(gpu.as_ref()?, frame))
    }

    fn draw(gpu: &Gpu, frame: &Frame) -> Vec<u8> {
        use wgpu::util::DeviceExt;
        let (device, queue) = (&gpu.device, &gpu.queue);
        let (width, height) = (frame.width, frame.height);
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = |format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let drawn = texture(COLOR, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = texture(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let picture = texture(COLOR, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (drawn_view, depth_view, picture_view) = (view(&drawn), view(&depth), view(&picture));

        let buffer = |label, contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
        let uniforms = buffer("scene", super::bytes(&frame.uniforms), wgpu::BufferUsages::UNIFORM);
        let bound = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &gpu.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let solids = (!frame.triangles.is_empty()).then(|| buffer("solids", frame.triangles, wgpu::BufferUsages::VERTEX));
        let lines = (!frame.lines.is_empty()).then(|| buffer("lines", frame.lines, wgpu::BufferUsages::VERTEX));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let [r, g, b, a] = frame.background.map(|c| c as f64);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &drawn_view,
                    depth_slice: None,
                    resolve_target: Some(&picture_view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &bound, &[]);
            if let Some(solids) = &solids {
                pass.set_pipeline(&gpu.solids);
                pass.set_vertex_buffer(0, solids.slice(..));
                pass.draw(0..(frame.triangles.len() / 40) as u32, 0..1);
            }
            if let Some(lines) = &lines {
                pass.set_pipeline(&gpu.lines);
                pass.set_vertex_buffer(0, lines.slice(..));
                pass.draw(0..(frame.lines.len() / 28) as u32, 0..1);
            }
        }
        // The picture's rows, as the graphics card lays them out (each a
        // multiple of 256 bytes long), then as a picture has them.
        let row = (width * 4).div_ceil(256) * 256;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            picture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
            size,
        );
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            pixels.extend_from_slice(&data[start..start + (width * 4) as usize]);
        }
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn transforms_place_points() {
        // A quarter turn around y takes what's on x to the back (-z).
        assert!(close(point(&turn(1, std::f64::consts::FRAC_PI_2), [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
        assert!(close(point(&turn(0, std::f64::consts::FRAC_PI_2), [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(close(point(&turn(2, std::f64::consts::FRAC_PI_2), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        // A surface stretched keeps facing where it faces: a floor stays up.
        let stretch: Mat = [[3.0, 0.0, 0.0, 0.0], [0.0, 0.5, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let n = normals(&stretch);
        assert!(close(unit([n[0][1], n[1][1], n[2][1]], [0.0; 3]), [0.0, 1.0, 0.0]));
    }

    #[test]
    fn the_camera_sees_what_it_looks_at_in_the_middle() {
        let seen = mul(&perspective(45f64.to_radians(), 1.5, 0.1, 100.0), &look_at([3.0, 2.0, 4.0], [0.0, 0.5, 0.0], [0.0, 1.0, 0.0]));
        let p = [0.0, 0.5, 0.0, 1.0];
        let clip: [f64; 4] = std::array::from_fn(|r| (0..4).map(|c| seen[r][c] * p[c]).sum());
        assert!((clip[0] / clip[3]).abs() < 1e-9 && (clip[1] / clip[3]).abs() < 1e-9);
        // Its depth is between the near and the far end.
        assert!(clip[2] / clip[3] > 0.0 && clip[2] / clip[3] < 1.0);
    }

    #[test]
    fn shapes_are_closed_and_face_out() {
        assert_eq!(box_corners(1.0, 2.0, 3.0).len(), 36);
        for (at, normal) in sphere_corners(2.0) {
            assert!((dot(at, at).sqrt() - 2.0).abs() < 1e-9);
            assert!(close(unit(at, [0.0; 3]), normal));
        }
        // A box's faces face away from its middle.
        for corners in box_corners(1.0, 1.0, 1.0).chunks(3) {
            let middle = [0, 1, 2].map(|i| (corners[0].0[i] + corners[1].0[i] + corners[2].0[i]) / 3.0);
            assert!(dot(middle, corners[0].1) > 0.0);
        }
        assert_eq!(std::mem::size_of::<Vertex>(), 40);
        assert_eq!(std::mem::size_of::<LineEnd>(), 28);
    }

    #[test]
    fn colors_go_from_one_to_the_next() {
        let (black, white) = (Rgba([0, 0, 0, 255]), Rgba([255, 255, 255, 255]));
        assert_eq!(between(&[black, white], 0.0), black);
        assert_eq!(between(&[black, white], 1.0), white);
        assert_eq!(between(&[black, white], 0.5).0[0], 128);
        assert_eq!(between(&[black], 0.7), black);
        assert!((light(255) - 1.0).abs() < 1e-6 && light(0) == 0.0 && light(128) < 0.25);
    }
}
