//! `Canvas`: a view the program draws itself.
//!
//! The view's block gets a `Graphics` value and calls its methods (`g.line`,
//! `g.fill`, `g.text`, …), which arrive here as the `tsl_canvas_*` calls
//! below. They draw into the canvas that is being painted (`CURRENT`); a
//! call made at any other time does nothing.
//!
//! Coordinates are in points, from the top left corner of the canvas. The
//! current transform (`g.translate`, `g.scale`, `g.rotate`) is applied here,
//! to each point as it is added, in 64-bit numbers: a map can be drawn in
//! coordinates of millions of units and still be exact on screen. Line
//! widths and text sizes are always in points, whatever the transform.
//!
//! Layers (`g.beginLayer`, and each clip) are layers of the drawing,
//! ended by `g.endLayer()`, `g.restore()` or the end of the drawing.
//!
//! `Canvas(redraw: n)` keeps its drawing between paints, until `n` changes
//! (or the canvas's size, or the appearance).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use kurbo::{Affine, Arc, BezPath, Cap, Circle, Ellipse, Join, PathEl, Point, Rect, RoundedRect, Shape, Stroke, Vec2};
use super::draw::{DrawList, FillRule, Gradient, Op};

use super::text::TextEngine;
use super::{theme, Callback, Rgba, WEIGHTS};
use crate::{Header, List, Str};

/// What a canvas drew, for `tree` and `dump` in headless runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub fills: usize,
    pub strokes: usize,
    pub texts: usize,
    pub images: usize,
}

impl Stats {
    /// "2 fills, 1 text", or "empty".
    pub fn describe(&self) -> String {
        let part = |n: usize, one: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            _ => Some(format!("{n} {one}s")),
        };
        let parts: Vec<String> = [
            part(self.fills, "fill"),
            part(self.strokes, "stroke"),
            part(self.texts, "text"),
            part(self.images, "image"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() { "empty".to_string() } else { parts.join(", ") }
    }
}

/// A finished drawing: in points, from the canvas's top left corner.
pub struct Drawing {
    pub list: Rc<DrawList>,
    pub stats: Stats,
}

/// The canvas being drawn.
struct Current {
    scene: DrawList,
    w: f64,
    h: f64,
    path: BezPath,
    transform: Affine,
    /// `g.zoom(…)`: how many times larger everything is drawn, line widths
    /// and text sizes included.
    zoom: f64,
    /// `g.save()`: the transform and zoom then, and how many layers were open.
    saved: Vec<(Affine, f64, usize)>,
    /// The layers of the scene open now, innermost last: clips (`false`),
    /// and layers from `g.beginLayer` (`true`).
    layers: Vec<bool>,
    stats: Stats,
    /// Points of the line being added, reused between calls.
    scratch: Vec<Point>,
    clipped: Vec<Point>,
    /// For `writePDF`: the pages finished by `g.newPage()`, before the one
    /// being drawn. `None` for a canvas or a picture, which have one page.
    pages: Option<Vec<DrawList>>,
}

/// A drawing kept for a `Canvas(redraw:)`.
struct Kept {
    redraw: i64,
    w: f64,
    h: f64,
    dark: bool,
    drawing: Rc<Drawing>,
}

thread_local! {
    // The renderer for `renderImage` (an app's window has its own). (Not
    // dropped when the thread ends: the GPU's own cleanup can't run that
    // late. The process is ending anyway.)
    static OFFSCREEN: RefCell<Option<std::mem::ManuallyDrop<super::backend::Offscreen>>> = const { RefCell::new(None) };
    static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
    static TEXT: RefCell<Option<TextEngine>> = const { RefCell::new(None) };
    static KEPT: RefCell<HashMap<u64, Kept>> = RefCell::new(HashMap::new());
}

/// Runs a canvas's block, and returns what it drew.
fn draw(block: &Callback, w: f64, h: f64) -> Drawing {
    draw_pages(block, w, h, false).0
}

/// Runs a block, and returns what it drew: its last page, and with `pages`
/// the pages `g.newPage()` finished before it.
fn draw_pages(block: &Callback, w: f64, h: f64, pages: bool) -> (Drawing, Vec<DrawList>) {
    let current = Current {
        scene: DrawList::new(),
        w,
        h,
        path: BezPath::new(),
        transform: Affine::IDENTITY,
        zoom: 1.0,
        saved: Vec::new(),
        layers: Vec::new(),
        stats: Stats::default(),
        scratch: Vec::new(),
        clipped: Vec::new(),
        pages: pages.then(Vec::new),
    };
    // (A canvas's block can't draw another canvas, but be safe if one is open.)
    let outer = CURRENT.with(|c| c.borrow_mut().replace(current));
    block.call_with_floats(w, h);
    let mut done = CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), outer)).expect("the canvas being drawn");
    for _ in done.layers.drain(..) {
        done.scene.pop();
    }
    (Drawing { list: Rc::new(done.scene), stats: done.stats }, done.pages.unwrap_or_default())
}

/// The drawing of the canvas with key `key`: its block's, or the one kept
/// from before when `redraw` (and the size and appearance) didn't change.
pub fn drawing(key: u64, block: &Callback, redraw: Option<i64>, w: f64, h: f64) -> Rc<Drawing> {
    let dark = theme::is_dark();
    if let Some(redraw) = redraw {
        let kept = KEPT.with(|k| {
            k.borrow().get(&key).filter(|k| k.redraw == redraw && k.w == w && k.h == h && k.dark == dark).map(|k| k.drawing.clone())
        });
        if let Some(d) = kept {
            return d;
        }
    }
    let d = Rc::new(draw(block, w, h));
    match redraw {
        Some(redraw) => {
            KEPT.with(|k| k.borrow_mut().insert(key, Kept { redraw, w, h, dark, drawing: d.clone() }));
        }
        None => {
            KEPT.with(|k| k.borrow_mut().remove(&key));
        }
    }
    d
}

/// `renderImage(path:width:height:scale:) { g in … }`: runs the block as a
/// canvas of `width` × `height` points would, and saves what it draws as a
/// PNG (or a JPEG, for a path ending in .jpg or .jpeg) of `scale` pixels to
/// the point. Where nothing is drawn the picture is clear (white, in a
/// JPEG). Returns whether it was saved. `f` is called as `f(env, width, height)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_render_image(
    path: *const Str,
    width: f64,
    height: f64,
    scale: f64,
    f: *const std::ffi::c_void,
    env: *mut Header,
) -> u8 {
    let path = unsafe { crate::str_text(path) }.to_string();
    let block = Callback::new(f, env);
    let (w, h) = ((width * scale).round(), (height * scale).round());
    if !(w >= 1.0 && h >= 1.0 && w <= 16384.0 && h <= 16384.0 && scale > 0.0) {
        return 0;
    }
    let drawing = draw(&block, width, height);
    let mut list = DrawList::new();
    list.append(drawing.list.clone(), Affine::scale(scale));
    let (w, h) = (w as u32, h as u32);
    let jpeg = path.to_lowercase().ends_with(".jpg") || path.to_lowercase().ends_with(".jpeg");
    let base = jpeg.then_some(Rgba([255, 255, 255, 255]));
    let pixels = OFFSCREEN.with(|o| {
        let mut o = o.borrow_mut();
        o.get_or_insert_with(|| std::mem::ManuallyDrop::new(super::backend::Offscreen::default())).rgba(&list, w, h, base)
    });
    super::backend::save_rgba(&path, w, h, pixels) as u8
}

/// `writePDF(path:title:width:height:) { g in … }`: runs the block, with
/// `g.newPage()` starting each page after the first, and saves the pages
/// as a PDF of `width` × `height` points each. Returns whether it was
/// saved. `f` is called as `f(env, width, height)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_write_pdf(
    path: *const Str,
    title: *const Str,
    width: f64,
    height: f64,
    f: *const std::ffi::c_void,
    env: *mut Header,
) -> u8 {
    let (path, title) = unsafe { (crate::str_text(path).to_string(), crate::str_text(title).to_string()) };
    let block = Callback::new(f, env);
    // (14 400 points is the largest page PDF readers take: 5 meters.)
    if !(width >= 1.0 && height >= 1.0 && width <= 14400.0 && height <= 14400.0) {
        return 0;
    }
    // On paper, in the light appearance: `.primary` text is black even
    // while the app is dark.
    let dark = theme::is_dark();
    theme::set_dark(false);
    let (last, mut pages) = draw_pages(&block, width, height, true);
    theme::set_dark(dark);
    pages.push(Rc::try_unwrap(last.list).unwrap_or_else(|l| {
        let mut list = DrawList::new();
        list.append(l, Affine::IDENTITY);
        list
    }));
    super::backend::write_pdf(&path, &title, width, height, &pages) as u8
}

/// Drops the drawings kept for canvases that aren't in the tree any more.
pub fn keep_only(keys: &HashSet<u64>) {
    KEPT.with(|k| k.borrow_mut().retain(|key, _| keys.contains(key)));
}

/// Drops every kept drawing (at exit).
pub fn forget_all() {
    KEPT.with(|k| k.borrow_mut().clear());
    TEXT.with(|t| *t.borrow_mut() = None);
    OFFSCREEN.with(|o| {
        if let Some(mut offscreen) = o.borrow_mut().take() {
            unsafe { std::mem::ManuallyDrop::drop(&mut offscreen) };
        }
    });
}

fn with(f: impl FnOnce(&mut Current)) {
    CURRENT.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            f(c);
        }
    });
}

fn with_text<R>(f: impl FnOnce(&mut TextEngine) -> R) -> R {
    TEXT.with(|t| f(t.borrow_mut().get_or_insert_with(TextEngine::new)))
}

/// The items of a `[Float]`.
unsafe fn floats<'a>(l: *const List) -> &'a [f64] {
    unsafe {
        if l.is_null() || (*l).len <= 0 {
            return &[];
        }
        std::slice::from_raw_parts((*l).data as *const f64, (*l).len as usize)
    }
}

/// The items of a `[Color]` (each is its case's place and the box of its
/// values, as an enum is passed on its own).
unsafe fn colors(l: *const List) -> Vec<Rgba> {
    unsafe {
        if l.is_null() || (*l).len <= 0 || (*(*l).elem).size < 16 {
            return Vec::new();
        }
        let size = (*(*l).elem).size as usize;
        (0..(*l).len as usize)
            .map(|i| {
                let at = (*l).data.add(i * size);
                let tag = *(at as *const i32);
                let payload = *(at.add(8) as *const *const Header);
                theme::color(tag, payload)
            })
            .collect()
    }
}

/// The items of an `[Int]`.
unsafe fn ints<'a>(l: *const List) -> &'a [i64] {
    unsafe {
        if l.is_null() || (*l).len <= 0 {
            return &[];
        }
        std::slice::from_raw_parts((*l).data as *const i64, (*l).len as usize)
    }
}

/// Whether a canvas is being drawn now.
fn drawing_now() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

// How far outside the canvas a line may still matter (its width reaches in).
const MARGIN: f64 = 256.0;
// Lines that reach further out than this are cut at `CUT` from the canvas:
// the GPU works in 32-bit numbers, which lose their fractions far away.
const FAR: f64 = 100_000.0;
const CUT: f64 = 1024.0;
// Points closer than this to the one before add nothing that can be seen.
const SMALLEST: f64 = 0.25;

impl Current {
    fn has_current_point(&self) -> bool {
        !matches!(self.path.elements().last(), None | Some(PathEl::ClosePath))
    }

    fn add_shape(&mut self, shape: &impl Shape) {
        let mut p = shape.to_path(0.1);
        p.apply_affine(self.transform);
        self.path.extend(p);
    }

    /// Adds a line through `points` (x, y, x, y, …), as a part of its own.
    fn add_lines(&mut self, points: &[f64], closed: bool) {
        let n = points.len() / 2;
        if n == 0 {
            return;
        }
        let [a, b, c, d, e, f] = self.transform.as_coeffs();
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for i in 0..n {
            let (x, y) = (points[2 * i], points[2 * i + 1]);
            let p = Point::new(a * x + c * y + e, b * x + d * y + f);
            if !(p.x.is_finite() && p.y.is_finite()) {
                continue;
            }
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
            scratch.push(p);
        }
        // Nothing of it shows.
        let outside = x1 < -MARGIN || y1 < -MARGIN || x0 > self.w + MARGIN || y0 > self.h + MARGIN;
        if scratch.is_empty() || outside {
            self.scratch = scratch;
            return;
        }
        if x0 < -FAR || y0 < -FAR || x1 > self.w + FAR || y1 > self.h + FAR {
            let area = Rect::new(-CUT, -CUT, self.w + CUT, self.h + CUT);
            if closed {
                let mut out = std::mem::take(&mut self.clipped);
                clip_ring(&scratch, area, &mut out);
                self.emit(&out, true);
                self.clipped = out;
            } else {
                self.emit_cut(&scratch, area);
            }
        } else {
            self.emit(&scratch, closed);
        }
        self.scratch = scratch;
    }

    /// Adds points (already in canvas coordinates) as one part of the path,
    /// leaving out those too close together to be told apart.
    fn emit(&mut self, points: &[Point], closed: bool) {
        let Some((&first, rest)) = points.split_first() else { return };
        self.path.move_to(first);
        let mut last = first;
        let count = rest.len();
        for (i, &p) in rest.iter().enumerate() {
            let near = (p.x - last.x).abs() < SMALLEST && (p.y - last.y).abs() < SMALLEST;
            // (The last point always counts: the line ends where it says.)
            if near && i + 1 < count {
                continue;
            }
            self.path.line_to(p);
            last = p;
        }
        if closed {
            self.path.close_path();
        }
    }

    /// An open line, cut where it leaves `area`: each stretch inside is a part.
    fn emit_cut(&mut self, points: &[Point], area: Rect) {
        let mut run: Vec<Point> = Vec::new();
        for pair in points.windows(2) {
            match clip_segment(pair[0], pair[1], area) {
                Some((p, q)) => {
                    if run.last() != Some(&p) {
                        if run.len() > 1 {
                            self.emit(&run, false);
                        }
                        run.clear();
                        run.push(p);
                    }
                    run.push(q);
                }
                None => {
                    if run.len() > 1 {
                        self.emit(&run, false);
                    }
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            self.emit(&run, false);
        } else if points.len() == 1 && area.contains(points[0]) {
            self.emit(points, false);
        }
    }
}

/// The part of the segment from `p` to `q` inside `r` (Liang–Barsky).
fn clip_segment(p: Point, q: Point, r: Rect) -> Option<(Point, Point)> {
    let (dx, dy) = (q.x - p.x, q.y - p.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (den, num) in [(-dx, p.x - r.x0), (dx, r.x1 - p.x), (-dy, p.y - r.y0), (dy, r.y1 - p.y)] {
        if den == 0.0 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = num / den;
        if den < 0.0 {
            if t > t1 {
                return None;
            }
            t0 = t0.max(t);
        } else {
            if t < t0 {
                return None;
            }
            t1 = t1.min(t);
        }
    }
    let at = |t: f64| if t == 0.0 { p } else if t == 1.0 { q } else { Point::new(p.x + dx * t, p.y + dy * t) };
    Some((at(t0), at(t1)))
}

/// The part of the ring `points` inside `r` (Sutherland–Hodgman): what's
/// outside is replaced by edges along `r`, which are out of sight.
fn clip_ring(points: &[Point], r: Rect, out: &mut Vec<Point>) {
    let mut a: Vec<Point> = points.to_vec();
    let mut b: Vec<Point> = Vec::with_capacity(a.len() + 8);
    // Each side of `r`: is a point inside it, and where an edge crosses it.
    for side in 0..4 {
        let inside = |p: Point| match side {
            0 => p.x >= r.x0,
            1 => p.x <= r.x1,
            2 => p.y >= r.y0,
            _ => p.y <= r.y1,
        };
        let cross = |p: Point, q: Point| match side {
            0 | 1 => {
                let x = if side == 0 { r.x0 } else { r.x1 };
                Point::new(x, p.y + (q.y - p.y) * (x - p.x) / (q.x - p.x))
            }
            _ => {
                let y = if side == 2 { r.y0 } else { r.y1 };
                Point::new(p.x + (q.x - p.x) * (y - p.y) / (q.y - p.y), y)
            }
        };
        b.clear();
        let Some(&last) = a.last() else { break };
        let mut prev = last;
        for &p in &a {
            match (inside(prev), inside(p)) {
                (true, true) => b.push(p),
                (true, false) => b.push(cross(prev, p)),
                (false, true) => {
                    b.push(cross(prev, p));
                    b.push(p);
                }
                (false, false) => {}
            }
            prev = p;
        }
        std::mem::swap(&mut a, &mut b);
    }
    out.clear();
    out.extend_from_slice(&a);
}

// ===== C ABI: paths =====

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_begin_path() {
    with(|c| c.path.truncate(0));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_move_to(x: f64, y: f64) {
    with(|c| {
        let p = c.transform * Point::new(x, y);
        c.path.move_to(p);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_line_to(x: f64, y: f64) {
    with(|c| {
        let p = c.transform * Point::new(x, y);
        if c.has_current_point() { c.path.line_to(p) } else { c.path.move_to(p) }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_quad_to(cx: f64, cy: f64, x: f64, y: f64) {
    with(|c| {
        let (ctrl, p) = (c.transform * Point::new(cx, cy), c.transform * Point::new(x, y));
        if !c.has_current_point() {
            c.path.move_to(ctrl);
        }
        c.path.quad_to(ctrl, p);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_curve_to(c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
    with(|c| {
        let t = c.transform;
        let (c1, c2, p) = (t * Point::new(c1x, c1y), t * Point::new(c2x, c2y), t * Point::new(x, y));
        if !c.has_current_point() {
            c.path.move_to(c1);
        }
        c.path.curve_to(c1, c2, p);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_close_path() {
    with(|c| {
        if c.has_current_point() {
            c.path.close_path();
        }
    });
}

/// `g.lines(points, closed:)`: a line through the points (x, y, x, y, …).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_lines(points: *const List, closed: u8) {
    let points = unsafe { floats(points) };
    with(|c| c.add_lines(points, closed != 0));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_rect(x: f64, y: f64, w: f64, h: f64, radius: f64) {
    with(|c| {
        let r = Rect::new(x, y, x + w, y + h);
        if radius > 0.0 { c.add_shape(&RoundedRect::from_rect(r, radius)) } else { c.add_shape(&r) }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_circle(x: f64, y: f64, radius: f64) {
    with(|c| c.add_shape(&Circle::new((x, y), radius.max(0.0))));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_ellipse(x: f64, y: f64, rx: f64, ry: f64) {
    with(|c| c.add_shape(&Ellipse::new((x, y), (rx.max(0.0), ry.max(0.0)), 0.0)));
}

/// `g.arc(x:y:radius:from:to:)`: part of a circle, between two angles
/// (in radians, clockwise from the x axis as the canvas is seen).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_arc(x: f64, y: f64, radius: f64, from: f64, to: f64) {
    with(|c| {
        let r = radius.max(0.0);
        let arc = Arc::new((x, y), (r, r), from, to - from, 0.0);
        let start = c.transform * Point::new(x + r * from.cos(), y + r * from.sin());
        if c.has_current_point() { c.path.line_to(start) } else { c.path.move_to(start) }
        let t = c.transform;
        for el in arc.append_iter(0.1) {
            c.path.push(t * el);
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_fill(tag: i32, payload: *const Header, even_odd: u8) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        if c.path.elements().is_empty() {
            return;
        }
        let rule = if even_odd != 0 { FillRule::EvenOdd } else { FillRule::NonZero };
        c.scene.fill(rule, Affine::IDENTITY, col, &c.path);
        c.stats.fills += 1;
    });
}

fn stroke_style(width: f64, dash: &[f64]) -> Stroke {
    let s = Stroke::new(width.max(0.0));
    let dash: Vec<f64> = dash.iter().copied().filter(|d| d.is_finite() && *d > 0.0).collect();
    if dash.is_empty() { s } else { s.with_caps(Cap::Butt).with_dashes(0.0, dash) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_stroke(tag: i32, payload: *const Header, width: f64, dash: *const List) {
    let col = unsafe { theme::color(tag, payload) };
    let dash = unsafe { floats(dash) };
    with(|c| {
        if c.path.elements().is_empty() || width <= 0.0 {
            return;
        }
        let dash: Vec<f64> = dash.iter().map(|d| d * c.zoom).collect();
        c.scene.stroke(&stroke_style(width * c.zoom, &dash), Affine::IDENTITY, col, &c.path);
        c.stats.strokes += 1;
    });
}

// ===== C ABI: gradients =====

/// The brush for a gradient through `colors` (at `stops`, from 0 to 1, or
/// spread evenly when there isn't one stop for each color), and the
/// transform it's drawn with: `make` makes it with its first point at the
/// origin, which is `at` in the current coordinates. `None` for no colors.
fn gradient(c: &Current, colors: &[Rgba], stops: &[f64], at: (f64, f64), make: impl FnOnce(Vec<(f32, Rgba)>) -> Gradient) -> Option<(Gradient, Affine)> {
    let place = c.transform * Affine::translate(at);
    let (&first, _) = colors.split_first()?;
    if colors.len() == 1 {
        return Some((make(vec![(0.0, first), (1.0, first)]), place));
    }
    let last = (colors.len() - 1) as f64;
    let given = stops.len() == colors.len() && stops.iter().all(|s| s.is_finite());
    let mut offset = 0.0f64;
    let points: Vec<(f32, Rgba)> = colors
        .iter()
        .enumerate()
        .map(|(i, &col)| {
            // (Stops can't go back: one before the stop it follows is moved up to it.)
            offset = if given { stops[i].clamp(offset, 1.0) } else { i as f64 / last };
            (offset as f32, col)
        })
        .collect();
    Some((make(points), place))
}

/// `g.fillLinear(…)`: fills the current path with colors that change along
/// the line from (x1, y1) to (x2, y2): the first color before it, the last
/// after it.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_fill_linear(x1: f64, y1: f64, x2: f64, y2: f64, cols: *const List, stops: *const List, even_odd: u8) {
    let (cols, stops) = unsafe { (colors(cols), floats(stops)) };
    with(|c| {
        if c.path.elements().is_empty() {
            return;
        }
        let make = |stops| Gradient::Linear { start: Point::ZERO, end: Point::new(x2 - x1, y2 - y1), stops };
        let Some((gradient, place)) = gradient(c, &cols, stops, (x1, y1), make) else { return };
        let rule = if even_odd != 0 { FillRule::EvenOdd } else { FillRule::NonZero };
        c.scene.ops.push(Op::FillGradient { path: c.path.clone(), rule, gradient, place, transform: Affine::IDENTITY });
        c.stats.fills += 1;
    });
}

/// `g.fillRadial(…)`: fills the current path with colors that change from
/// the point (x, y) out to `radius` from it (the last color beyond).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_fill_radial(x: f64, y: f64, radius: f64, cols: *const List, stops: *const List, even_odd: u8) {
    let (cols, stops) = unsafe { (colors(cols), floats(stops)) };
    with(|c| {
        if c.path.elements().is_empty() {
            return;
        }
        let make = |stops| Gradient::Radial { center: Point::ZERO, radius: radius.max(0.0), stops };
        let Some((gradient, place)) = gradient(c, &cols, stops, (x, y), make) else { return };
        let rule = if even_odd != 0 { FillRule::EvenOdd } else { FillRule::NonZero };
        c.scene.ops.push(Op::FillGradient { path: c.path.clone(), rule, gradient, place, transform: Affine::IDENTITY });
        c.stats.fills += 1;
    });
}

/// `g.strokeLinear(…)`: draws the current path's lines in colors that
/// change along the line from (x1, y1) to (x2, y2).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_stroke_linear(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    cols: *const List,
    stops: *const List,
    width: f64,
    dash: *const List,
) {
    let (cols, stops, dash) = unsafe { (colors(cols), floats(stops), floats(dash)) };
    with(|c| {
        if c.path.elements().is_empty() || width <= 0.0 {
            return;
        }
        let make = |stops| Gradient::Linear { start: Point::ZERO, end: Point::new(x2 - x1, y2 - y1), stops };
        let Some((gradient, place)) = gradient(c, &cols, stops, (x1, y1), make) else { return };
        let dash: Vec<f64> = dash.iter().map(|d| d * c.zoom).collect();
        let style = stroke_style(width * c.zoom, &dash);
        c.scene.ops.push(Op::StrokeGradient { path: c.path.clone(), style, gradient, place, transform: Affine::IDENTITY });
        c.stats.strokes += 1;
    });
}

/// `g.strokeRadial(…)`: draws the current path's lines in colors that
/// change from the point (x, y) out to `radius` from it.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_stroke_radial(
    x: f64,
    y: f64,
    radius: f64,
    cols: *const List,
    stops: *const List,
    width: f64,
    dash: *const List,
) {
    let (cols, stops, dash) = unsafe { (colors(cols), floats(stops), floats(dash)) };
    with(|c| {
        if c.path.elements().is_empty() || width <= 0.0 {
            return;
        }
        let make = |stops| Gradient::Radial { center: Point::ZERO, radius: radius.max(0.0), stops };
        let Some((gradient, place)) = gradient(c, &cols, stops, (x, y), make) else { return };
        let dash: Vec<f64> = dash.iter().map(|d| d * c.zoom).collect();
        let style = stroke_style(width * c.zoom, &dash);
        c.scene.ops.push(Op::StrokeGradient { path: c.path.clone(), style, gradient, place, transform: Affine::IDENTITY });
        c.stats.strokes += 1;
    });
}

// ===== C ABI: shapes drawn at once (the path stays as it is) =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_line(x1: f64, y1: f64, x2: f64, y2: f64, tag: i32, payload: *const Header, width: f64) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        if width <= 0.0 {
            return;
        }
        let line = kurbo::Line::new(c.transform * Point::new(x1, y1), c.transform * Point::new(x2, y2));
        c.scene.stroke(&Stroke::new(width * c.zoom), Affine::IDENTITY, col, &line);
        c.stats.strokes += 1;
    });
}

fn shape_path(c: &Current, shape: &impl Shape) -> BezPath {
    let mut p = shape.to_path(0.1);
    p.apply_affine(c.transform);
    p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_fill_rect(x: f64, y: f64, w: f64, h: f64, tag: i32, payload: *const Header, radius: f64) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        let p = shape_path(c, &RoundedRect::new(x, y, x + w, y + h, radius.max(0.0)));
        c.scene.fill(FillRule::NonZero, Affine::IDENTITY, col, &p);
        c.stats.fills += 1;
    });
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_stroke_rect(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    tag: i32,
    payload: *const Header,
    line_width: f64,
    radius: f64,
) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        if line_width <= 0.0 {
            return;
        }
        let p = shape_path(c, &RoundedRect::new(x, y, x + w, y + h, radius.max(0.0)));
        // Square corners stay square.
        let style = Stroke::new(line_width * c.zoom).with_join(Join::Miter);
        c.scene.stroke(&style, Affine::IDENTITY, col, &p);
        c.stats.strokes += 1;
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_fill_circle(x: f64, y: f64, radius: f64, tag: i32, payload: *const Header) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        let p = shape_path(c, &Circle::new((x, y), radius.max(0.0)));
        c.scene.fill(FillRule::NonZero, Affine::IDENTITY, col, &p);
        c.stats.fills += 1;
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_stroke_circle(x: f64, y: f64, radius: f64, tag: i32, payload: *const Header, line_width: f64) {
    let col = unsafe { theme::color(tag, payload) };
    with(|c| {
        if line_width <= 0.0 {
            return;
        }
        let p = shape_path(c, &Circle::new((x, y), radius.max(0.0)));
        c.scene.stroke(&Stroke::new(line_width * c.zoom), Affine::IDENTITY, col, &p);
        c.stats.strokes += 1;
    });
}

// ===== C ABI: text and pictures =====

/// `g.text(…)`: text with the point (x, y) at the place in its box that
/// `align` names (its top left corner for `.topLeading`, its middle for
/// `.center`, …), turned by `angle` around that point. A halo is an outline
/// in another color, drawn under the text, that keeps it readable on a busy
/// drawing.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_text(
    text: *const Str,
    x: f64,
    y: f64,
    size: f64,
    tag: i32,
    payload: *const Header,
    weight: i32,
    align: i32,
    angle: f64,
    halo_tag: i32,
    halo_payload: *const Header,
    halo_width: f64,
    italic: u8,
    family: *const Str,
) {
    let text = unsafe { crate::str_text(text) };
    let face = unsafe { face(italic, family) };
    let ink = unsafe { theme::color(tag, payload) };
    let halo = unsafe { theme::color(halo_tag, halo_payload) };
    if text.is_empty() || size <= 0.0 {
        return;
    }
    let weight = WEIGHTS[(weight.max(0) as usize).min(WEIGHTS.len() - 1)];
    with(|c| {
        let (size, halo_width) = (size * c.zoom, halo_width * c.zoom);
        let layout = Rc::new(with_text(|t| t.styled(text, size, weight, face, ink, None, 0)));
        let (h, v) = super::align_2d(align.clamp(0, 8) as u8);
        let (w, ht) = (layout.width(), layout.height());
        let anchor = c.transform * Point::new(x, y);
        let t = Affine::translate(anchor.to_vec2())
            * Affine::rotate(angle)
            * Affine::translate(Vec2::new(-w * h as f64 / 2.0, -ht * v as f64 / 2.0));
        // The halo first, under the text.
        let halo = (halo_width > 0.0).then_some((halo, halo_width * 2.0));
        c.scene.ops.push(super::draw::Op::Text { layout, transform: t, tint: Some(ink), halo });
        c.stats.texts += 1;
    });
}

/// `g.textWidth(…)`: how wide `g.text` would draw the text, in points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_text_width(text: *const Str, size: f64, weight: i32, italic: u8, family: *const Str) -> f64 {
    let text = unsafe { crate::str_text(text) };
    if text.is_empty() || size <= 0.0 {
        return 0.0;
    }
    let weight = WEIGHTS[(weight.max(0) as usize).min(WEIGHTS.len() - 1)];
    let face = unsafe { face(italic, family) };
    with_text(|t| t.styled(text, size, weight, face, Rgba([0, 0, 0, 255]), None, 0).width())
}

/// `italic:` and `family:` of `g.text` and `g.textWidth`.
unsafe fn face(italic: u8, family: *const Str) -> super::text::Face {
    let family = if family.is_null() { "" } else { unsafe { crate::str_text(family) } };
    super::text::Face { family: super::text::Family::named(family), italic: italic != 0 }
}

/// `g.image(path, …)`: a PNG or JPEG file, in the rectangle given. Returns
/// whether there was a picture to draw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_canvas_image(path: *const Str, x: f64, y: f64, w: f64, h: f64, opacity: f64) -> u8 {
    let path = unsafe { crate::str_text(path) };
    super::image::enable();
    let Some(picture) = super::image::load(path) else { return 0 };
    let mut drawn = 0;
    with(|c| {
        draw_picture(c, &picture, x, y, w, h, opacity);
        drawn = 1;
    });
    drawn
}

/// Draws `picture` stretched over the rectangle given, in the current
/// coordinates: it turns and scales with them, as paths do.
fn draw_picture(c: &mut Current, picture: &Rc<super::image::Picture>, x: f64, y: f64, w: f64, h: f64, opacity: f64) {
    let t = c.transform * Affine::translate((x, y)) * Affine::scale_non_uniform(w / picture.width.max(1.0), h / picture.height.max(1.0));
    c.scene.image(picture.clone(), t, opacity.clamp(0.0, 1.0) as f32);
    c.stats.images += 1;
}

/// The widest and tallest picture `g.image(pixels:…)` draws, in pixels.
const MOST_PIXELS: i64 = 8192;

thread_local! {
    /// For `g.image(pixels:…)`: the key of the picture last made from each
    /// list (by where its items are). When a list's pixels change, the
    /// picture made from them before is dropped at once.
    static MADE_FROM: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
}

/// A hash of the pixels, to know whether they're the ones a picture was
/// made from: in four lanes, to read them as fast as memory goes.
fn hash_pixels(pixels: &[i64]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mix = |h: u64, x: u64| (h ^ x).wrapping_mul(K).rotate_left(29);
    let mut lanes = [1u64, 2, 3, 4];
    let (chunks, rest) = pixels.as_chunks::<4>();
    for c in chunks {
        for i in 0..4 {
            lanes[i] = mix(lanes[i], c[i] as u64);
        }
    }
    for (i, &x) in rest.iter().enumerate() {
        lanes[i] = mix(lanes[i], x as u64);
    }
    let mut h = pixels.len() as u64;
    for l in lanes {
        h = mix(h, l);
    }
    // (Spreads every bit over all of it.)
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^ (h >> 33)
}

/// `g.image(pixels:pixelWidth:pixelHeight:…)`: a picture made from pixels
/// in memory, row by row from the top left, each as `writeImage` takes
/// them (red × 16777216 + green × 65536 + blue × 256 + opacity), drawn as
/// `g.image(path, …)` draws a file. Returns false (and draws nothing) when
/// the size isn't from 1 to `MOST_PIXELS` or there are too few pixels.
///
/// The picture is kept with the pictures read from files: drawing the same
/// pixels again (in the next frame) only reads them through, to see that
/// they're the same.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_image_pixels(
    pixels: *const List,
    pixel_width: i64,
    pixel_height: i64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    opacity: f64,
) -> u8 {
    let all = unsafe { ints(pixels) };
    let size_ok = (1..=MOST_PIXELS).contains(&pixel_width) && (1..=MOST_PIXELS).contains(&pixel_height);
    if !size_ok || (all.len() as i64) < pixel_width * pixel_height || !drawing_now() {
        return 0;
    }
    let pixels = &all[..(pixel_width * pixel_height) as usize];
    let key = format!("\0pixels {:016x} {pixel_width}x{pixel_height}", hash_pixels(pixels));
    MADE_FROM.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 1024 {
            m.clear();
        }
        if let Some(old) = m.insert(all.as_ptr() as usize, key.clone())
            && old != key
        {
            super::image::forget(&old);
        }
    });
    let make = || {
        let mut bytes = vec![0u8; pixels.len() * 4];
        for (b, &p) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
            *b = (p as u32).to_be_bytes();
        }
        super::backend::picture_from_rgba(pixel_width as u32, pixel_height as u32, bytes)
    };
    let Some(picture) = super::image::from_memory(&key, make) else { return 0 };
    with(|c| draw_picture(c, &picture, x, y, w, h, opacity));
    1
}

/// Draws an SVG scaled to fit the rectangle given and centered in it, in
/// the current coordinates.
fn draw_svg(c: &mut Current, svg: &Rc<super::svg::Svg>, x: f64, y: f64, w: f64, h: f64, opacity: f64) {
    let s = (w / svg.width.max(1e-9)).min(h / svg.height.max(1e-9));
    if !(s.is_finite() && s > 0.0) {
        return;
    }
    let (sx, sy) = (x + (w - svg.width * s) / 2.0, y + (h - svg.height * s) / 2.0);
    let t = c.transform * Affine::translate((sx, sy)) * Affine::scale(s);
    let opacity = if opacity.is_nan() { 1.0 } else { opacity.clamp(0.0, 1.0) };
    if opacity < 1.0 {
        let area = Rect::new(-CUT, -CUT, c.w + CUT, c.h + CUT);
        c.scene.push(FillRule::NonZero, opacity as f32, Affine::IDENTITY, &area);
        c.scene.svg(svg.clone(), t);
        c.scene.pop();
    } else {
        c.scene.svg(svg.clone(), t);
    }
    c.stats.images += 1;
}

/// `g.svg(path, …)`: an SVG file, scaled to fit the rectangle given and
/// centered in it; `currentColor` in it is `color`. Returns whether there
/// was an SVG to draw.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_svg(
    path: *const Str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    tag: i32,
    payload: *const Header,
    opacity: f64,
) -> u8 {
    let path = unsafe { crate::str_text(path) };
    let ink = unsafe { theme::color(tag, payload) };
    if !drawing_now() {
        return 0;
    }
    super::svg::enable();
    let Some(svg) = super::svg::load_file(path, ink) else { return 0 };
    with(|c| draw_svg(c, &svg, x, y, w, h, opacity));
    1
}

/// `g.svg(text: …)`: the same, for SVG source in the program.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_canvas_svg_text(
    text: *const Str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    tag: i32,
    payload: *const Header,
    opacity: f64,
) -> u8 {
    let text = unsafe { crate::str_text(text) };
    let ink = unsafe { theme::color(tag, payload) };
    if !drawing_now() {
        return 0;
    }
    super::svg::enable();
    let Some(svg) = super::svg::load(text, ink) else { return 0 };
    with(|c| draw_svg(c, &svg, x, y, w, h, opacity));
    1
}

// ===== C ABI: the transform, and clipping =====

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_translate(x: f64, y: f64) {
    with(|c| c.transform *= Affine::translate((x, y)));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_scale(x: f64, y: f64) {
    with(|c| c.transform *= Affine::scale_non_uniform(x, y));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_rotate(angle: f64) {
    with(|c| c.transform *= Affine::rotate(angle));
}

/// `g.zoom(factor)`: what follows is drawn `factor` times larger, as a
/// whole: positions, and line widths and text sizes too.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_zoom(factor: f64) {
    with(|c| {
        if factor.is_finite() && factor > 0.0 {
            c.transform *= Affine::scale(factor);
            c.zoom *= factor;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_reset_transform() {
    with(|c| {
        c.transform = Affine::IDENTITY;
        c.zoom = 1.0;
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_save() {
    with(|c| c.saved.push((c.transform, c.zoom, c.layers.len())));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_restore() {
    with(|c| {
        let Some((transform, zoom, layers)) = c.saved.pop() else { return };
        c.transform = transform;
        c.zoom = zoom;
        while c.layers.len() > layers {
            c.scene.pop();
            c.layers.pop();
        }
    });
}

/// `g.clip(x:y:width:height:)`: until the matching `g.restore()` (or the
/// end of the drawing), only what's inside the rectangle shows.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_clip(x: f64, y: f64, w: f64, h: f64) {
    with(|c| {
        let p = shape_path(c, &Rect::new(x, y, x + w, y + h));
        c.scene.push(FillRule::NonZero, 1.0, Affine::IDENTITY, &p);
        c.layers.push(false);
    });
}

/// `g.clipPath(evenOdd:)`: until the matching `g.restore()` (or the end of
/// the drawing), only what's inside the current path shows.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_clip_path(even_odd: u8) {
    with(|c| {
        let rule = if even_odd != 0 { FillRule::EvenOdd } else { FillRule::NonZero };
        c.scene.push(rule, 1.0, Affine::IDENTITY, &c.path);
        c.layers.push(false);
    });
}

// ===== C ABI: layers =====

/// `g.beginLayer(blend:opacity:)`: what's drawn until the matching
/// `g.endLayer()` is drawn on its own first, and then onto what was there
/// before, blended as `blend` says and at `opacity`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_begin_layer(blend: i32, opacity: f64) {
    let blend = blend.clamp(0, 15) as u8;
    with(|c| {
        let area = Rect::new(-CUT, -CUT, c.w + CUT, c.h + CUT);
        let opacity = if opacity.is_nan() { 1.0 } else { opacity.clamp(0.0, 1.0) };
        let clip = area.to_path(0.1);
        c.scene.ops.push(Op::Push { clip, rule: FillRule::NonZero, alpha: opacity as f32, blend, transform: Affine::IDENTITY });
        c.layers.push(true);
    });
}

/// `g.endLayer()`: ends the layer the last `g.beginLayer` began (and the
/// clips begun since). Without one, does nothing.
/// `g.newPage()`: in `writePDF`, the page so far is done, and drawing goes
/// on a new one, with nothing transformed or clipped. Elsewhere it does
/// nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_new_page() {
    with(|c| {
        let Some(pages) = c.pages.as_mut() else { return };
        for _ in c.layers.drain(..) {
            c.scene.pop();
        }
        pages.push(std::mem::replace(&mut c.scene, DrawList::new()));
        c.path = BezPath::new();
        c.transform = Affine::IDENTITY;
        c.zoom = 1.0;
        c.saved.clear();
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_canvas_end_layer() {
    with(|c| {
        let Some(i) = c.layers.iter().rposition(|&layer| layer) else { return };
        while c.layers.len() > i {
            c.scene.pop();
            c.layers.pop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn cuts_segments_at_the_edges() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(clip_segment(p(2.0, 2.0), p(8.0, 8.0), r), Some((p(2.0, 2.0), p(8.0, 8.0))));
        assert_eq!(clip_segment(p(-5.0, 5.0), p(15.0, 5.0), r), Some((p(0.0, 5.0), p(10.0, 5.0))));
        assert_eq!(clip_segment(p(-5.0, -5.0), p(-1.0, 20.0), r), None);
        assert_eq!(clip_segment(p(5.0, -5.0), p(5.0, 5.0), r), Some((p(5.0, 0.0), p(5.0, 5.0))));
    }

    #[test]
    fn cuts_rings_to_the_area() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let mut out = Vec::new();
        // A square around the whole area becomes the area.
        clip_ring(&[p(-100.0, -100.0), p(100.0, -100.0), p(100.0, 100.0), p(-100.0, 100.0)], r, &mut out);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|q| (q.x == 0.0 || q.x == 10.0) && (q.y == 0.0 || q.y == 10.0)), "{out:?}");
        // One outside it leaves nothing.
        clip_ring(&[p(20.0, 20.0), p(30.0, 20.0), p(30.0, 30.0)], r, &mut out);
        assert!(out.is_empty());
        // A triangle poking in keeps its tip.
        clip_ring(&[p(5.0, 5.0), p(25.0, 0.0), p(25.0, 10.0)], r, &mut out);
        assert!(out.contains(&p(5.0, 5.0)) && out.iter().all(|q| q.x <= 10.0), "{out:?}");
    }

    #[test]
    fn tells_pixels_apart() {
        let a: Vec<i64> = (0..1001).map(|i| i * 7919 % 4096).collect();
        assert_eq!(hash_pixels(&a), hash_pixels(&a.clone()));
        // Any one pixel changed, in any lane or in the rest after them.
        for at in [0, 1, 2, 3, 500, 999, 1000] {
            let mut b = a.clone();
            b[at] ^= 1 << 24;
            assert_ne!(hash_pixels(&a), hash_pixels(&b), "{at}");
        }
        // Two pixels swapped, and one fewer.
        let mut b = a.clone();
        b.swap(10, 11);
        assert_ne!(hash_pixels(&a), hash_pixels(&b));
        assert_ne!(hash_pixels(&a), hash_pixels(&a[..1000]));
    }

    #[test]
    fn describes_what_was_drawn() {
        assert_eq!(Stats::default().describe(), "empty");
        assert_eq!(Stats { fills: 2, strokes: 0, texts: 1, images: 0 }.describe(), "2 fills, 1 text");
    }
}
