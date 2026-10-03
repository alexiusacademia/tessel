//! Pictures: `Image("photo.jpg")` shows PNG and JPEG files.
//!
//! The tree is rebuilt on every change, so decoded pictures are kept by path
//! (and read again when the file changes).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use image::ImageDecoder;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

pub struct Picture {
    pub image: ImageData,
    /// In points: one for each pixel.
    pub width: f64,
    pub height: f64,
}

/// A picture that was read: the file's modification time then, and when
/// it was last asked for (a count of the askings).
struct Entry {
    modified: Option<SystemTime>,
    picture: Option<Rc<Picture>>,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    /// Counts the askings.
    clock: u64,
    /// The bytes of the pictures kept.
    bytes: usize,
}

/// Pictures are kept up to this many bytes; past it, the ones not asked for
/// the longest go (a map shows thousands of tiles in a session), down to
/// three quarters of it.
const LIMIT: usize = 256 * 1024 * 1024;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn size(picture: &Option<Rc<Picture>>) -> usize {
    picture.as_ref().map_or(0, |p| p.image.width as usize * p.image.height as usize * 4)
}

/// The picture in the file at `path`; `None` if it's missing or not a PNG or JPEG.
pub fn load(path: &str) -> Option<Rc<Picture>> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    kept(path, modified, || decode(path))
}

/// A picture made from pixels in memory (`g.image(pixels:…)`), kept under
/// `key` (which says what the pixels are) with the pictures read from files,
/// and made by `make` only when it isn't kept already.
pub fn from_memory(key: &str, make: impl FnOnce() -> Option<Picture>) -> Option<Rc<Picture>> {
    kept(key, None, make)
}

/// Drops the picture kept under `key`, if there is one.
pub fn forget(key: &str) {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(old) = c.entries.remove(key) {
            c.bytes -= size(&old.picture);
        }
    });
}

/// The picture kept under `key` if it's from `modified`; otherwise the one
/// `make` makes, which is kept from now on.
fn kept(key: &str, modified: Option<SystemTime>, make: impl FnOnce() -> Option<Picture>) -> Option<Rc<Picture>> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.clock += 1;
        let now = c.clock;
        if let Some(e) = c.entries.get_mut(key).filter(|e| e.modified == modified) {
            e.used = now;
            return e.picture.clone();
        }
        let picture = make().map(Rc::new);
        let added = size(&picture);
        if let Some(old) = c.entries.insert(key.to_string(), Entry { modified, picture: picture.clone(), used: now }) {
            c.bytes -= size(&old.picture);
        }
        c.bytes += added;
        if c.bytes > LIMIT {
            let mut by_age: Vec<(u64, String)> = c.entries.iter().map(|(k, e)| (e.used, k.clone())).collect();
            by_age.sort();
            for (_, old_key) in by_age {
                if c.bytes <= LIMIT / 4 * 3 || old_key == key {
                    break;
                }
                if let Some(old) = c.entries.remove(&old_key) {
                    c.bytes -= size(&old.picture);
                }
            }
        }
        picture
    })
}

fn decode(path: &str) -> Option<Picture> {
    let mut decoder = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    // Photos from cameras and phones say which way up they are.
    let orientation = decoder.orientation().ok()?;
    let mut picture = image::DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    let rgba = picture.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some(Picture {
        image: ImageData {
            data: Blob::new(Arc::new(rgba.into_raw())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        },
        width: width as f64,
        height: height as f64,
    })
}
