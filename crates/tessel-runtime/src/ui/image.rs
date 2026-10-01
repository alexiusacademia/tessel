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

/// Pictures by path, with the file's modification time when it was read.
type Cache = HashMap<String, (Option<SystemTime>, Option<Rc<Picture>>)>;

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
}

/// The picture in the file at `path`; `None` if it's missing or not a PNG or JPEG.
pub fn load(path: &str) -> Option<Rc<Picture>> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        match c.get(path) {
            Some((at, picture)) if *at == modified => picture.clone(),
            _ => {
                let picture = decode(path).map(Rc::new);
                c.insert(path.to_string(), (modified, picture.clone()));
                picture
            }
        }
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
