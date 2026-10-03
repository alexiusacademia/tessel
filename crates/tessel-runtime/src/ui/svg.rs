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

/// SVG files read for `g.svg(path, …)`: by path, with the file's
/// modification time then.
type Files = HashMap<String, (Option<std::time::SystemTime>, Option<Rc<str>>)>;

/// Parsed SVGs are kept up to this many (a canvas could draw SVG text that
/// changes every frame); past it, they're all dropped and parsed again.
const LIMIT: usize = 512;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    static FILES: RefCell<Files> = RefCell::new(HashMap::new());
}

/// The parsed SVG, drawn in `color` where it says `currentColor`; `None` if it's not valid SVG.
pub fn load(source: &str, color: Rgba) -> Option<Rc<Svg>> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    let key = (h.finish(), color.0);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= LIMIT && !c.contains_key(&key) {
            c.clear();
        }
        c.entry(key)
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

/// The SVG file at `path`, parsed (and read again when the file changes);
/// `None` if it can't be read or isn't valid SVG.
pub fn load_file(path: &str, color: Rgba) -> Option<Rc<Svg>> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let source = FILES.with(|f| {
        let mut f = f.borrow_mut();
        if let Some((m, source)) = f.get(path)
            && *m == modified
        {
            return source.clone();
        }
        if f.len() >= LIMIT {
            f.clear();
        }
        let source: Option<Rc<str>> = std::fs::read_to_string(path).ok().map(Into::into);
        f.insert(path.to_string(), (modified, source.clone()));
        source
    })?;
    load(&source, color)
}

/// A built-in icon by name, for the runtime's own controls.
pub fn icon(name: &str, color: Rgba) -> Option<Rc<Svg>> {
    ICONS.iter().find(|(n, _)| *n == name).and_then(|(_, source)| load(source, color))
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
