//! Animations: views under `.animation()` move, resize and fade to their new
//! place when the UI changes, instead of jumping there; ones that appear
//! fade in. Toggles always slide.
//!
//! Layout always gives where views end up (clicks go there). This keeps, by
//! view, what's drawn on the way.

use std::collections::HashMap;

use super::layout::Rect;
use super::{Kind, Node, Rgba};

/// How long a toggle takes to slide, in seconds.
const TOGGLE: f64 = 0.15;

/// What's drawn of a view at one moment.
#[derive(Clone, Copy, PartialEq)]
pub struct Shown {
    pub rect: Rect,
    pub opacity: f64,
    /// `.background`: its color and radius.
    pub background: Option<(Rgba, f64)>,
}

struct Track {
    from: Shown,
    to: Shown,
    now: Shown,
    start: f64,
    duration: f64,
    /// How far the scroll views around it are scrolled: scrolling moves a
    /// view at once, so the rectangles here are as if nothing was scrolled.
    scrolled: f64,
    seen: u64,
}

struct Slide {
    from: f64,
    to: f64,
    now: f64,
    start: f64,
}

#[derive(Default)]
pub struct Animations {
    /// Off in headless runs, unless the script says `animations on`.
    pub enabled: bool,
    /// Whether anything is still on its way (so another frame is needed).
    pub running: bool,
    tracks: HashMap<u64, Track>,
    /// Toggles: 0 for off to 1 for on.
    toggles: HashMap<u64, Slide>,
    /// The window's size when last drawn: resizing it isn't animated.
    size: (f64, f64),
    frame: u64,
}

fn ease(t: f64) -> f64 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn mix_color(a: Rgba, b: Rgba, t: f64) -> Rgba {
    Rgba(std::array::from_fn(|i| mix(a.0[i] as f64, b.0[i] as f64, t).round() as u8))
}

fn mix_shown(a: &Shown, b: &Shown, t: f64) -> Shown {
    let clear = |c: Rgba| Rgba([c.0[0], c.0[1], c.0[2], 0]);
    Shown {
        rect: Rect {
            x: mix(a.rect.x, b.rect.x, t),
            y: mix(a.rect.y, b.rect.y, t),
            w: mix(a.rect.w, b.rect.w, t),
            h: mix(a.rect.h, b.rect.h, t),
        },
        opacity: mix(a.opacity, b.opacity, t),
        // A background that comes or goes fades in or out.
        background: match (a.background, b.background) {
            (Some((ca, _)), Some((cb, r))) => Some((mix_color(ca, cb, t), r)),
            (None, Some((cb, r))) => Some((mix_color(clear(cb), cb, t), r)),
            (Some((ca, r)), None) if t < 1.0 => Some((mix_color(ca, clear(ca), t), r)),
            _ => None,
        },
    }
}

impl Animations {
    /// After layout: starts animations for what changed since the last
    /// frame, and moves the ones under way to time `now`.
    pub fn step(&mut self, root: &Node, now: f64, size: (f64, f64), scroll: &HashMap<u64, f64>) {
        self.running = false;
        if !self.enabled {
            self.tracks.clear();
            self.toggles.clear();
            return;
        }
        // Nothing animates in the first frame, or while the window is resized.
        let jump = self.frame == 0 || size != self.size;
        self.size = size;
        self.frame += 1;
        self.visit(root, now, jump, 0.0, scroll);
        let frame = self.frame;
        self.tracks.retain(|_, t| t.seen == frame);
        self.toggles.retain(|k, _| self.tracks.contains_key(k));
    }

    fn visit(&mut self, n: &Node, now: f64, jump: bool, scrolled: f64, scroll: &HashMap<u64, f64>) {
        if n.mods.hidden {
            return;
        }
        let rect = n.layout.rect;
        let to = Shown {
            rect: Rect { y: rect.y + scrolled, ..rect },
            opacity: n.mods.opacity.unwrap_or(1.0),
            background: n.mods.background,
        };
        let duration = if jump { 0.0 } else { n.layout.style.animation };
        let frame = self.frame;
        let track = self.tracks.entry(n.key).or_insert_with(|| Track {
            // A view that appears fades in.
            from: Shown { opacity: 0.0, ..to },
            to,
            now: to,
            start: now,
            duration,
            scrolled,
            seen: 0,
        });
        if track.to != to {
            *track = Track { from: track.now, to, now: track.now, start: now, duration, scrolled, seen: frame };
        }
        track.seen = frame;
        track.scrolled = scrolled;
        let t = if track.duration > 0.0 { ((now - track.start) / track.duration).clamp(0.0, 1.0) } else { 1.0 };
        track.now = mix_shown(&track.from, &track.to, ease(t));
        self.running |= t < 1.0;

        if let Kind::Toggle { is_on, .. } = &n.kind {
            let to = (unsafe { *is_on.locate() } & 1) as f64;
            let slide = self.toggles.entry(n.key).or_insert(Slide { from: to, to, now: to, start: now });
            if slide.to != to {
                *slide = Slide { from: slide.now, to, now: slide.now, start: now };
            }
            let t = if jump { 1.0 } else { ((now - slide.start) / TOGGLE).clamp(0.0, 1.0) };
            slide.now = mix(slide.from, slide.to, ease(t));
            self.running |= t < 1.0;
        }

        let below = match n.kind {
            // As layout scrolls it: no further than its content goes.
            Kind::Scroll => {
                let visible = n.layout.rect.inset_by(n.mods.padding).h;
                let offset = scroll.get(&n.key).copied().unwrap_or(0.0);
                scrolled + offset.clamp(0.0, (n.layout.content_h - visible).max(0.0))
            }
            _ => scrolled,
        };
        for c in n.visible_children() {
            self.visit(c, now, jump, below, scroll);
        }
    }

    /// What to draw of a view now, where that isn't simply its layout.
    pub fn shown(&self, key: u64) -> Option<Shown> {
        let t = self.tracks.get(&key)?;
        let rect = Rect { y: t.now.rect.y - t.scrolled, ..t.now.rect };
        Some(Shown { rect, ..t.now })
    }

    /// How far on a toggle is drawn: 0 for off to 1 for on.
    pub fn toggle(&self, key: u64, on: bool) -> f64 {
        self.toggles.get(&key).map_or(on as u8 as f64, |s| s.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_from_one_state_to_the_other() {
        assert_eq!((ease(0.0), ease(0.5), ease(1.0)), (0.0, 0.5, 1.0));
        assert!(ease(0.25) < 0.25 && ease(0.75) > 0.75);
        let rect = |x: f64| Rect { x, y: 0.0, w: 10.0, h: 10.0 };
        let a = Shown { rect: rect(0.0), opacity: 1.0, background: None };
        let b = Shown { rect: rect(100.0), opacity: 0.0, background: Some((Rgba([200, 100, 0, 255]), 4.0)) };
        let half = mix_shown(&a, &b, 0.5);
        assert_eq!((half.rect.x, half.opacity), (50.0, 0.5));
        // A new background fades in; one that goes fades out, then is gone.
        assert_eq!(half.background, Some((Rgba([200, 100, 0, 128]), 4.0)));
        assert_eq!(mix_shown(&b, &a, 0.5).background, Some((Rgba([200, 100, 0, 128]), 4.0)));
        assert_eq!(mix_shown(&b, &a, 1.0).background, None);
    }
}
