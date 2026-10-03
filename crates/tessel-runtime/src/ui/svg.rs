//! SVG drawing: built-in icons, `Svg("…")` and `Image("file.svg")`.
//!
//! SVGs are parsed with usvg and turned into the renderer's, cached by source
//! and color (`currentColor` in an SVG takes the view's `.color`).
//!
//! Only apps that show an SVG or an icon have this code: the views that do
//! turn it on (`enable`, `enable_icons`), and the rest of the runtime reaches
//! it through function pointers, so the linker leaves it out of other apps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::OnceLock;


use super::Rgba;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

pub struct Svg {
    /// The renderer's.
    pub scene: super::backend::SvgScene,
    pub width: f64,
    pub height: f64,
}

/// Parsed SVGs by (source hash, color).
type Cache = HashMap<(u64, [u8; 4]), Option<Rc<Svg>>>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
}

static PARSE: OnceLock<fn(&str, Rgba) -> Option<Rc<Svg>>> = OnceLock::new();
static ICON: OnceLock<fn(&str) -> Option<&'static str>> = OnceLock::new();

/// Turns SVG drawing on (see the module's comment).
pub fn enable() {
    let _ = PARSE.set(parse);
}

/// Turns SVG drawing and the built-in icons on.
pub fn enable_icons() {
    enable();
    let _ = ICON.set(|name| ICONS.iter().find(|(n, _)| *n == name).map(|(_, source)| *source));
}

/// The parsed SVG, drawn in `color` where it says `currentColor`; `None` if
/// it's not valid SVG (or SVG drawing isn't on).
pub fn load(source: &str, color: Rgba) -> Option<Rc<Svg>> {
    (PARSE.get()?)(source, color)
}

fn parse(source: &str, color: Rgba) -> Option<Rc<Svg>> {
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
                    scene: super::backend::svg_scene(&tree),
                    width: size.width() as f64,
                    height: size.height() as f64,
                }))
            })
            .clone()
    })
}

/// A built-in icon by name, for the runtime's own controls.
pub fn icon(name: &str, color: Rgba) -> Option<Rc<Svg>> {
    load((ICON.get()?)(name)?, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses() {
        enable_icons();
        assert!(ICONS.len() >= 50);
        for (name, svg) in ICONS {
            let icon = load(svg, Rgba([0, 0, 0, 255])).unwrap_or_else(|| panic!("icon {name} doesn't parse"));
            assert_eq!((icon.width, icon.height), (24.0, 24.0), "{name}");
        }
    }
}
