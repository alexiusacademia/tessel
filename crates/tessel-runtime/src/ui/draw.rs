//! A drawing, as a list of commands any renderer can play: fill this shape,
//! stroke that one, draw this text, clip and fade what follows. Views paint
//! into a `DrawList` (see `render::Painter`), and so does a `Canvas`; the
//! renderer (`ui::backend`) turns it into pixels.

use std::rc::Rc;

use kurbo::{Affine, BezPath, Point, Shape, Stroke};

use super::text::TextLayout;
use super::Rgba;

/// How the inside of a path is found where it crosses itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// How a layer's drawing goes onto what's under it (`BlendMode`'s cases,
/// in order): 0 is normal, 1 multiply, 2 screen, 3 overlay, 4 darken, 5
/// lighten, 6 color dodge, 7 color burn, 8 hard light, 9 soft light, 10
/// difference, 11 exclusion, 12 hue, 13 saturation, 14 color, 15 luminosity.
pub type Blend = u8;

/// Colors that change across a shape: along a line, or out from a point.
/// Each stop is where (from 0 to 1) and its color; before the first and
/// after the last, those colors go on.
#[derive(Clone, Debug)]
pub enum Gradient {
    Linear { start: Point, end: Point, stops: Vec<(f32, Rgba)> },
    Radial { center: Point, radius: f64, stops: Vec<(f32, Rgba)> },
}

#[derive(Clone)]
pub enum Op {
    Fill { path: BezPath, rule: FillRule, color: Rgba, transform: Affine },
    Stroke { path: BezPath, style: Stroke, color: Rgba, transform: Affine },
    /// Like `Fill` and `Stroke`, with a gradient, whose points are placed by
    /// `transform * place`.
    FillGradient { path: BezPath, rule: FillRule, gradient: Gradient, place: Affine, transform: Affine },
    StrokeGradient { path: BezPath, style: Stroke, gradient: Gradient, place: Affine, transform: Affine },
    /// What's drawn until the matching `Pop` shows only inside `clip`, and
    /// goes onto what's under it faded to `alpha` and blended by `blend`.
    Push { clip: BezPath, rule: FillRule, alpha: f32, blend: Blend, transform: Affine },
    Pop,
    /// Text, at the top left of its layout; `tint` draws it all in one
    /// color, and `halo` outlines it first (color, width).
    Text { layout: Rc<TextLayout>, transform: Affine, tint: Option<Rgba>, halo: Option<(Rgba, f64)> },
    Image { picture: Rc<super::image::Picture>, transform: Affine, alpha: f32 },
    Svg { svg: Rc<super::svg::Svg>, transform: Affine },
    /// Another drawing, placed with `transform`.
    List { list: Rc<DrawList>, transform: Affine },
}

#[derive(Default)]
pub struct DrawList {
    pub ops: Vec<Op>,
}

impl DrawList {
    pub fn new() -> DrawList {
        DrawList::default()
    }

    pub fn fill(&mut self, rule: FillRule, transform: Affine, color: Rgba, shape: &impl Shape) {
        self.ops.push(Op::Fill { path: shape.to_path(0.1), rule, color, transform });
    }

    pub fn stroke(&mut self, style: &Stroke, transform: Affine, color: Rgba, shape: &impl Shape) {
        self.ops.push(Op::Stroke { path: shape.to_path(0.1), style: style.clone(), color, transform });
    }

    /// Clips (and fades) what's drawn until the matching `pop`.
    pub fn push(&mut self, rule: FillRule, alpha: f32, transform: Affine, clip: &impl Shape) {
        self.ops.push(Op::Push { clip: clip.to_path(0.1), rule, alpha, blend: 0, transform });
    }

    pub fn pop(&mut self) {
        self.ops.push(Op::Pop);
    }

    pub fn text(&mut self, layout: Rc<TextLayout>, transform: Affine, tint: Option<Rgba>) {
        self.ops.push(Op::Text { layout, transform, tint, halo: None });
    }

    pub fn image(&mut self, picture: Rc<super::image::Picture>, transform: Affine, alpha: f32) {
        self.ops.push(Op::Image { picture, transform, alpha });
    }

    pub fn svg(&mut self, svg: Rc<super::svg::Svg>, transform: Affine) {
        self.ops.push(Op::Svg { svg, transform });
    }

    pub fn append(&mut self, list: Rc<DrawList>, transform: Affine) {
        self.ops.push(Op::List { list, transform });
    }
}
