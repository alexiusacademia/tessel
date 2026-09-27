//! SVG drawing: built-in icons, `Svg("…")` and `Image("file.svg")`.
//!
//! SVGs are parsed with usvg and turned into vello scenes, cached by source
//! and color (`currentColor` in an SVG takes the view's `.color`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use vello_svg::usvg;

use super::Rgba;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub struct Svg {
    pub scene: vello::Scene,
    pub width: f64,
    pub height: f64,
}

/// Parsed SVGs by (source hash, color).
type Cache = HashMap<(u64, [u8; 4]), Option<Rc<Svg>>>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// The parsed SVG, drawn in `color` where it says `currentColor`; `None` if it's not valid SVG.
pub fn load(source: &str, color: Rgba) -> Option<Rc<Svg>> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    let key = (h.finish(), color.0);
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let [r, g, b, _] = color.0;
                let text = source.replace("currentColor", &format!("#{r:02x}{g:02x}{b:02x}"));
                let tree = usvg::Tree::from_str(&text, &usvg::Options::default()).ok()?;
                let size = tree.size();
                Some(Rc::new(Svg {
                    scene: vello_svg::render_tree(&tree),
                    width: size.width() as f64,
                    height: size.height() as f64,
                }))
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses() {
        assert!(ICONS.len() >= 50);
        for (name, svg) in ICONS {
            let icon = load(svg, Rgba([0, 0, 0, 255])).unwrap_or_else(|| panic!("icon {name} doesn't parse"));
            assert_eq!((icon.width, icon.height), (24.0, 24.0), "{name}");
        }
    }
}
