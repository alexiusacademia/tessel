//! A drawing, as a list of commands any renderer can play: fill this shape,
//! stroke that one, draw this text, clip and fade what follows. Views paint
//! into a `DrawList` (see `render::Painter`), and so does a `Canvas`; the
//! renderer (`ui::backend`) turns it into pixels.

use std::rc::Rc;

use kurbo::{Affine, BezPath, Shape, Stroke};

use super::text::TextLayout;
use super::Rgba;

/// How the inside of a path is found where it crosses itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

pub enum Op {
    Fill { path: BezPath, rule: FillRule, color: Rgba, transform: Affine },
    Stroke { path: BezPath, style: Stroke, color: Rgba, transform: Affine },
    /// What's drawn until the matching `Pop` shows only inside `clip`, and
    /// faded to `alpha`.
    Push { clip: BezPath, rule: FillRule, alpha: f32, transform: Affine },
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
        self.ops.push(Op::Push { clip: clip.to_path(0.1), rule, alpha, transform });
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
