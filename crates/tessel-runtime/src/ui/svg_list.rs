//! SVG for the system renderers (macOS, Windows): usvg's tree of shapes, as
//! a drawing (`DrawList`) they play like any other. Shapes
//! are filled and stroked in their colors; a gradient is drawn in its first
//! color, and pictures and patterns inside an SVG are left out.

use kurbo::{Affine, BezPath, Cap, Join, Point, Rect, Stroke};

use crate::ui::draw::{DrawList, FillRule};
use crate::ui::Rgba;

fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx as f64, t.ky as f64, t.kx as f64, t.sy as f64, t.tx as f64, t.ty as f64])
}

fn path_of(data: &usvg::tiny_skia_path::Path) -> BezPath {
    use usvg::tiny_skia_path::PathSegment;
    let p = |q: usvg::tiny_skia_path::Point| Point::new(q.x as f64, q.y as f64);
    let mut out = BezPath::new();
    for seg in data.segments() {
        match seg {
            PathSegment::MoveTo(a) => out.move_to(p(a)),
            PathSegment::LineTo(a) => out.line_to(p(a)),
            PathSegment::QuadTo(c, a) => out.quad_to(p(c), p(a)),
            PathSegment::CubicTo(c1, c2, a) => out.curve_to(p(c1), p(c2), p(a)),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

/// A paint's color, at `opacity`.
fn color(paint: &usvg::Paint, opacity: f32) -> Option<Rgba> {
    let (c, extra) = match paint {
        usvg::Paint::Color(c) => (*c, 1.0),
        usvg::Paint::LinearGradient(g) => (g.stops().first()?.color(), g.stops().first()?.opacity().get()),
        usvg::Paint::RadialGradient(g) => (g.stops().first()?.color(), g.stops().first()?.opacity().get()),
        usvg::Paint::Pattern(_) => return None,
    };
    let a = (opacity * extra * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Rgba([c.red, c.green, c.blue, a]))
}

fn group(g: &usvg::Group, list: &mut DrawList) {
    let faded = g.opacity().get() < 1.0;
    if faded {
        // (Everything shows: the layer only fades what's in it.)
        let all = Rect::new(-1.0e6, -1.0e6, 1.0e6, 1.0e6);
        list.push(FillRule::NonZero, g.opacity().get(), Affine::IDENTITY, &all);
    }
    for node in g.children() {
        match node {
            usvg::Node::Group(g) => group(g, list),
            usvg::Node::Path(p) => path(p, list),
            usvg::Node::Text(t) => group(t.flattened(), list),
            usvg::Node::Image(_) => {}
        }
    }
    if faded {
        list.pop();
    }
}

fn path(p: &usvg::Path, list: &mut DrawList) {
    if !p.is_visible() {
        return;
    }
    let shape = path_of(p.data());
    let t = affine(p.abs_transform());
    let fill = |list: &mut DrawList| {
        if let Some(f) = p.fill() {
            if let Some(c) = color(f.paint(), f.opacity().get()) {
                let rule = match f.rule() {
                    usvg::FillRule::NonZero => FillRule::NonZero,
                    usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                };
                list.fill(rule, t, c, &shape);
            }
        }
    };
    let stroke = |list: &mut DrawList| {
        if let Some(s) = p.stroke() {
            if let Some(c) = color(s.paint(), s.opacity().get()) {
                let cap = match s.linecap() {
                    usvg::LineCap::Butt => Cap::Butt,
                    usvg::LineCap::Round => Cap::Round,
                    usvg::LineCap::Square => Cap::Square,
                };
                let join = match s.linejoin() {
                    usvg::LineJoin::Round => Join::Round,
                    usvg::LineJoin::Bevel => Join::Bevel,
                    _ => Join::Miter,
                };
                let mut style = Stroke::new(s.width().get() as f64).with_caps(cap).with_join(join).with_miter_limit(s.miterlimit().get() as f64);
                if let Some(d) = s.dasharray() {
                    style = style.with_dashes(s.dashoffset() as f64, d.iter().map(|v| *v as f64));
                }
                list.stroke(&style, t, c, &shape);
            }
        }
    };
    match p.paint_order() {
        usvg::PaintOrder::FillAndStroke => {
            fill(list);
            stroke(list);
        }
        usvg::PaintOrder::StrokeAndFill => {
            stroke(list);
            fill(list);
        }
    }
}

/// A parsed SVG as a drawing, in its own units from its top left corner.
pub fn svg_scene(tree: &usvg::Tree) -> DrawList {
    let mut list = DrawList::new();
    group(tree.root(), &mut list);
    list
}
