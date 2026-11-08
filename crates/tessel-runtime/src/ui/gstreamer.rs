//! Sound and video on Linux, through GStreamer: a `playbin` plays the file
//! and its sound; a video's pictures come out of an `appsink` as RGBA.
//!
//! GStreamer is loaded as the first player opens, not linked: programs
//! need it only to play, and every Linux desktop has it (its "good" and
//! "base" plugins play the common formats; more come with "bad", "ugly"
//! and "libav"). Without it, nothing plays, as with a file that can't be.

use std::cell::Cell;
use std::ffi::{CString, c_char, c_double, c_int, c_uint, c_void};
use std::sync::OnceLock;

type Ptr = *mut c_void;

/// GStreamer's functions this uses.
struct Gst {
    _libs: Vec<libloading::Library>,
    init_check: unsafe extern "C" fn(*mut c_int, *mut c_void, *mut Ptr) -> c_int,
    factory_make: unsafe extern "C" fn(*const c_char, *const c_char) -> Ptr,
    filename_to_uri: unsafe extern "C" fn(*const c_char, *mut Ptr) -> *mut c_char,
    caps_from_string: unsafe extern "C" fn(*const c_char) -> Ptr,
    caps_get_structure: unsafe extern "C" fn(Ptr, c_uint) -> Ptr,
    structure_get_int: unsafe extern "C" fn(Ptr, *const c_char, *mut c_int) -> c_int,
    set_state: unsafe extern "C" fn(Ptr, c_int) -> c_int,
    get_bus: unsafe extern "C" fn(Ptr) -> Ptr,
    bus_pop_filtered: unsafe extern "C" fn(Ptr, c_uint) -> Ptr,
    query_position: unsafe extern "C" fn(Ptr, c_int, *mut i64) -> c_int,
    query_duration: unsafe extern "C" fn(Ptr, c_int, *mut i64) -> c_int,
    seek_simple: unsafe extern "C" fn(Ptr, c_int, c_int, i64) -> c_int,
    object_unref: unsafe extern "C" fn(Ptr),
    mini_object_unref: unsafe extern "C" fn(Ptr),
    try_pull_sample: unsafe extern "C" fn(Ptr, u64) -> Ptr,
    sample_get_buffer: unsafe extern "C" fn(Ptr) -> Ptr,
    sample_get_caps: unsafe extern "C" fn(Ptr) -> Ptr,
    buffer_map: unsafe extern "C" fn(Ptr, *mut MapInfo, c_int) -> c_int,
    buffer_unmap: unsafe extern "C" fn(Ptr, *mut MapInfo),
    /// (Its values: a NULL-ended list of names and values.)
    object_set: unsafe extern "C" fn(Ptr, *const c_char, ...),
    free: unsafe extern "C" fn(Ptr),
}

// (Only ever used on the app's thread; the functions are plain pointers.)
unsafe impl Send for Gst {}
unsafe impl Sync for Gst {}

/// `GstMapInfo`.
#[repr(C)]
struct MapInfo {
    memory: Ptr,
    flags: c_int,
    data: *mut u8,
    size: usize,
    maxsize: usize,
    user_data: [Ptr; 4],
    reserved: [Ptr; 4],
}

const STATE_NULL: c_int = 1;
const STATE_PAUSED: c_int = 3;
const STATE_PLAYING: c_int = 4;
const FORMAT_TIME: c_int = 3;
const SEEK_FLUSH_KEY_UNIT: c_int = 1 | 4;
const MESSAGE_EOS: c_uint = 1;
const MESSAGE_ERROR: c_uint = 2;
const MAP_READ: c_int = 1;

fn gst() -> Option<&'static Gst> {
    static GST: OnceLock<Option<Gst>> = OnceLock::new();
    GST.get_or_init(|| unsafe { load() }).as_ref()
}

unsafe fn load() -> Option<Gst> {
    let open = |name: &str| unsafe { libloading::Library::new(name) }.ok();
    let (gst, app, gobject, glib) =
        (open("libgstreamer-1.0.so.0")?, open("libgstapp-1.0.so.0")?, open("libgobject-2.0.so.0")?, open("libglib-2.0.so.0")?);
    macro_rules! f {
        ($lib:expr, $name:literal) => {
            *unsafe { $lib.get(concat!($name, "\0").as_bytes()) }.ok()?
        };
    }
    let g = Gst {
        init_check: f!(gst, "gst_init_check"),
        factory_make: f!(gst, "gst_element_factory_make"),
        filename_to_uri: f!(gst, "gst_filename_to_uri"),
        caps_from_string: f!(gst, "gst_caps_from_string"),
        caps_get_structure: f!(gst, "gst_caps_get_structure"),
        structure_get_int: f!(gst, "gst_structure_get_int"),
        set_state: f!(gst, "gst_element_set_state"),
        get_bus: f!(gst, "gst_element_get_bus"),
        bus_pop_filtered: f!(gst, "gst_bus_pop_filtered"),
        query_position: f!(gst, "gst_element_query_position"),
        query_duration: f!(gst, "gst_element_query_duration"),
        seek_simple: f!(gst, "gst_element_seek_simple"),
        object_unref: f!(gst, "gst_object_unref"),
        mini_object_unref: f!(gst, "gst_mini_object_unref"),
        sample_get_buffer: f!(gst, "gst_sample_get_buffer"),
        sample_get_caps: f!(gst, "gst_sample_get_caps"),
        buffer_map: f!(gst, "gst_buffer_map"),
        buffer_unmap: f!(gst, "gst_buffer_unmap"),
        try_pull_sample: f!(app, "gst_app_sink_try_pull_sample"),
        object_set: f!(gobject, "g_object_set"),
        free: f!(glib, "g_free"),
        _libs: Vec::new(),
    };
    let mut error: Ptr = std::ptr::null_mut();
    if unsafe { (g.init_check)(std::ptr::null_mut(), std::ptr::null_mut(), &mut error) } == 0 {
        return None;
    }
    Some(Gst { _libs: vec![gst, app, gobject, glib], ..g })
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

pub struct Player {
    playbin: Ptr,
    bus: Ptr,
    /// A video's pictures come out here (null for a sound).
    sink: Ptr,
    playing: Cell<bool>,
    size: Option<(u32, u32)>,
}

impl Player {
    pub fn open(path: &str, video: bool) -> Option<Player> {
        let g = gst()?;
        unsafe {
            let playbin = (g.factory_make)(c("playbin").as_ptr(), std::ptr::null());
            if playbin.is_null() {
                return None;
            }
            let full = std::fs::canonicalize(path).ok()?;
            let mut error: Ptr = std::ptr::null_mut();
            let uri = (g.filename_to_uri)(c(&full.to_string_lossy()).as_ptr(), &mut error);
            if uri.is_null() {
                (g.object_unref)(playbin);
                return None;
            }
            (g.object_set)(playbin, c("uri").as_ptr(), uri, std::ptr::null::<c_char>());
            (g.free)(uri as Ptr);
            let mut sink = std::ptr::null_mut();
            if video {
                sink = (g.factory_make)(c("appsink").as_ptr(), std::ptr::null());
                if !sink.is_null() {
                    let caps = (g.caps_from_string)(c("video/x-raw,format=RGBA").as_ptr());
                    // Only the newest picture is kept; the sound sets the pace.
                    (g.object_set)(
                        sink,
                        c("caps").as_ptr(),
                        caps,
                        c("max-buffers").as_ptr(),
                        1 as c_uint,
                        c("drop").as_ptr(),
                        1 as c_int,
                        c("sync").as_ptr(),
                        1 as c_int,
                        std::ptr::null::<c_char>(),
                    );
                    (g.mini_object_unref)(caps);
                    // (The playbin keeps it.)
                    (g.object_set)(playbin, c("video-sink").as_ptr(), sink, std::ptr::null::<c_char>());
                }
            } else {
                // A sound: no window for a picture, even if the file has one.
                let fake = (g.factory_make)(c("fakesink").as_ptr(), std::ptr::null());
                if !fake.is_null() {
                    (g.object_set)(playbin, c("video-sink").as_ptr(), fake, std::ptr::null::<c_char>());
                }
            }
            // Ready to play, at its first picture.
            (g.set_state)(playbin, STATE_PAUSED);
            let bus = (g.get_bus)(playbin);
            Some(Player { playbin, bus, sink, playing: Cell::new(false), size: None })
        }
    }

    pub fn play(&mut self) {
        let Some(g) = gst() else { return };
        if !self.playing.get() && self.duration() > 0.0 && self.position() >= self.duration() - 0.01 {
            self.seek(0.0);
        }
        unsafe { (g.set_state)(self.playbin, STATE_PLAYING) };
        self.playing.set(true);
    }

    pub fn pause(&mut self) {
        let Some(g) = gst() else { return };
        unsafe { (g.set_state)(self.playbin, STATE_PAUSED) };
        self.playing.set(false);
    }

    /// Playing, until it comes to its end (or fails).
    pub fn playing(&self) -> bool {
        let Some(g) = gst() else { return false };
        if self.playing.get() && !self.bus.is_null() {
            let ended = unsafe { (g.bus_pop_filtered)(self.bus, MESSAGE_EOS | MESSAGE_ERROR) };
            if !ended.is_null() {
                unsafe {
                    (g.mini_object_unref)(ended);
                    (g.set_state)(self.playbin, STATE_PAUSED);
                }
                self.playing.set(false);
            }
        }
        self.playing.get()
    }

    fn query(&self, query: unsafe extern "C" fn(Ptr, c_int, *mut i64) -> c_int) -> f64 {
        let mut ns: i64 = 0;
        if unsafe { query(self.playbin, FORMAT_TIME, &mut ns) } == 0 || ns < 0 {
            return 0.0;
        }
        ns as f64 / 1e9
    }

    pub fn position(&self) -> f64 {
        gst().map_or(0.0, |g| self.query(g.query_position))
    }

    pub fn duration(&self) -> f64 {
        gst().map_or(0.0, |g| self.query(g.query_duration))
    }

    pub fn seek(&mut self, to: f64) {
        let Some(g) = gst() else { return };
        unsafe { (g.seek_simple)(self.playbin, FORMAT_TIME, SEEK_FLUSH_KEY_UNIT, (to * 1e9) as i64) };
    }

    pub fn set_volume(&mut self, volume: f64) {
        let Some(g) = gst() else { return };
        unsafe { (g.object_set)(self.playbin, c("volume").as_ptr(), volume as c_double, std::ptr::null::<c_char>()) };
    }

    /// The newest picture, if there's one not yet taken: its size and RGBA.
    pub fn frame(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let g = gst()?;
        if self.sink.is_null() {
            return None;
        }
        unsafe {
            let sample = (g.try_pull_sample)(self.sink, 0);
            if sample.is_null() {
                return None;
            }
            let out = (|| {
                let caps = (g.sample_get_caps)(sample);
                let s = (g.caps_get_structure)(caps, 0);
                let (mut w, mut h) = (0, 0);
                (g.structure_get_int)(s, c("width").as_ptr(), &mut w);
                (g.structure_get_int)(s, c("height").as_ptr(), &mut h);
                let (w, h) = (u32::try_from(w).ok()?, u32::try_from(h).ok()?);
                let buffer = (g.sample_get_buffer)(sample);
                let mut map: MapInfo = std::mem::zeroed();
                if buffer.is_null() || (g.buffer_map)(buffer, &mut map, MAP_READ) == 0 {
                    return None;
                }
                let need = (w * h * 4) as usize;
                let pixels = (map.size >= need).then(|| std::slice::from_raw_parts(map.data, need).to_vec());
                (g.buffer_unmap)(buffer, &mut map);
                Some((w, h, pixels?))
            })();
            (g.mini_object_unref)(sample);
            if let Some((w, h, _)) = &out {
                self.size = Some((*w, *h));
            }
            out
        }
    }

    pub fn size(&self) -> Option<(f64, f64)> {
        self.size.map(|(w, h)| (w as f64, h as f64))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let Some(g) = gst() else { return };
        unsafe {
            (g.set_state)(self.playbin, STATE_NULL);
            if !self.bus.is_null() {
                (g.object_unref)(self.bus);
            }
            (g.object_unref)(self.playbin);
        }
    }
}
