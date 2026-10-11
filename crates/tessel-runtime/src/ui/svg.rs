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

/// `source` with `color` where it says `currentColor`: the color, and how
/// much of it shows (a clear color draws nothing).
fn tinted(source: &str, color: Rgba) -> String {
    let [r, g, b, a] = color.0;
    source.replace("currentColor", &format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0))
}

fn parse(source: &str, color: Rgba) -> Option<Rc<Svg>> {
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
                let tree = usvg::Tree::from_str(&tinted(source, color), &usvg::Options::default()).ok()?;
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
    load((ICON.get()?)(name)?, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much the first stroke of an icon drawn in `color` shows.
    fn shows(color: Rgba) -> f32 {
        fn stroke(group: &usvg::Group) -> Option<f32> {
            group.children().iter().find_map(|node| match node {
                usvg::Node::Path(path) => path.stroke().map(|s| s.opacity().get()),
                usvg::Node::Group(inner) => stroke(inner),
                _ => None,
            })
        }
        let tree = usvg::Tree::from_str(&tinted(ICONS[0].1, color), &usvg::Options::default()).unwrap();
        stroke(tree.root()).expect("an icon is drawn with strokes")
    }

    #[test]
    fn an_icon_shows_as_much_as_its_color() {
        assert_eq!(shows(Rgba([255, 0, 0, 255])), 1.0);
        // A clear color draws nothing (it used to draw black: issue #19).
        assert_eq!(shows(Rgba([0, 0, 0, 0])), 0.0);
        assert!((shows(Rgba([255, 0, 0, 128])) - 0.5).abs() < 0.01);
    }

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
