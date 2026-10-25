//! Sound and video: `playSound` and the `VideoPlayer` view.
//!
//! The system decodes them (AVFoundation on macOS, Media Foundation on
//! Windows) and plays their sound; the runtime takes each new picture of a
//! video and draws it itself, like any other picture, with controls of its
//! own. A headless run (and a system without either) has a stand-in player
//! that plays nothing and keeps time on the app's clock, so tests can say
//! where a player is.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A player of one file.
pub struct Player {
    inner: Inner,
    pub looping: bool,
    /// The last picture of a video (drawn until there's a newer one).
    pub picture: Option<Rc<super::image::Picture>>,
    /// For a video: the position its `position:` binding was last given,
    /// and the second its time was last shown at.
    pub told: f64,
    pub shown: i64,
    /// Whether it was playing when last looked at (so its end is seen).
    pub was_playing: bool,
}

enum Inner {
    Stand(Stand),
    #[cfg(target_os = "macos")]
    Mac(mac::Player),
    #[cfg(all(windows, feature = "native"))]
    Win(win::Player),
}

/// The stand-in: how far it is, and since when (on the app's clock) it's
/// been playing from there.
struct Stand {
    at: f64,
    since: Option<f64>,
    duration: f64,
}

/// How long the stand-in says every file is.
const STAND_DURATION: f64 = 60.0;

impl Stand {
    fn position(&self) -> f64 {
        let now = super::app::clock();
        (self.at + self.since.map_or(0.0, |s| now - s)).min(self.duration)
    }
}

impl Player {
    /// A player of the file at `path` (with its pictures, for a `video`);
    /// `None` when it can't be played.
    pub fn open(path: &str, video: bool) -> Option<Player> {
        if !std::path::Path::new(path).exists() {
            return None;
        }
        let stand = || Inner::Stand(Stand { at: 0.0, since: None, duration: STAND_DURATION });
        let headless = std::env::var_os("TESSEL_SCRIPT").is_some();
        let inner = if headless {
            stand()
        } else {
            #[cfg(target_os = "macos")]
            let made = mac::Player::open(path, video).map(Inner::Mac);
            #[cfg(all(windows, feature = "native"))]
            let made = win::Player::open(path, video).map(Inner::Win);
            #[cfg(not(any(target_os = "macos", all(windows, feature = "native"))))]
            let made = {
                let _ = video;
                Some(stand())
            };
            made?
        };
        Some(Player { inner, looping: false, picture: None, told: 0.0, shown: -1, was_playing: false })
    }

    pub fn play(&mut self) {
        match &mut self.inner {
            Inner::Stand(s) => {
                if s.since.is_none() {
                    if s.at >= s.duration {
                        s.at = 0.0;
                    }
                    s.since = Some(super::app::clock());
                }
            }
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.play(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.play(),
        }
    }

    pub fn pause(&mut self) {
        match &mut self.inner {
            Inner::Stand(s) => {
                s.at = s.position();
                s.since = None;
            }
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.pause(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.pause(),
        }
    }

    pub fn playing(&self) -> bool {
        match &self.inner {
            Inner::Stand(s) => s.since.is_some() && s.position() < s.duration,
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.playing(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.playing(),
        }
    }

    /// Where it is, in seconds.
    pub fn position(&self) -> f64 {
        let p = match &self.inner {
            Inner::Stand(s) => s.position(),
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.position(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.position(),
        };
        if p.is_finite() { p.max(0.0) } else { 0.0 }
    }

    /// How long it is, in seconds (0 until that's known).
    pub fn duration(&self) -> f64 {
        let d = match &self.inner {
            Inner::Stand(s) => s.duration,
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.duration(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.duration(),
        };
        if d.is_finite() { d.max(0.0) } else { 0.0 }
    }

    pub fn seek(&mut self, to: f64) {
        let to = to.clamp(0.0, self.duration().max(0.0));
        match &mut self.inner {
            Inner::Stand(s) => {
                s.at = to;
                if s.since.is_some() {
                    s.since = Some(super::app::clock());
                }
            }
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.seek(to),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.seek(to),
        }
    }

    pub fn set_volume(&mut self, volume: f64) {
        let v = volume.clamp(0.0, 1.0);
        match &mut self.inner {
            Inner::Stand(_) => {}
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.set_volume(v),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.set_volume(v),
        }
        let _ = v;
    }

    /// Plays it again from the start when it's come to the end, if it loops.
    /// Whether it's playing.
    pub fn tick(&mut self) -> bool {
        if self.looping && !self.playing() && self.duration() > 0.0 && self.position() >= self.duration() - 0.05 {
            self.seek(0.0);
            self.play();
        }
        self.playing()
    }

    /// The video's newest picture, when there's one it hasn't given yet.
    pub fn take_picture(&mut self) -> bool {
        let frame = match &mut self.inner {
            Inner::Stand(_) => None,
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.frame(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.frame(),
        };
        let Some((w, h, rgba)) = frame else { return false };
        match super::backend::picture_from_rgba(w, h, rgba) {
            Some(p) => {
                self.picture = Some(Rc::new(p));
                true
            }
            None => false,
        }
    }

    /// The video's size in pixels, once it's known.
    pub fn size(&self) -> Option<(f64, f64)> {
        let s = match &self.inner {
            Inner::Stand(_) => None,
            #[cfg(target_os = "macos")]
            Inner::Mac(p) => p.size(),
            #[cfg(all(windows, feature = "native"))]
            Inner::Win(p) => p.size(),
        };
        s.filter(|(w, h)| *w > 0.0 && *h > 0.0)
    }
}

// ===== sounds (`playSound`) =====

thread_local! {
    /// The sounds playing (or paused), by id.
    static SOUNDS: RefCell<HashMap<i64, Player>> = RefCell::new(HashMap::new());
    static LAST_SOUND: RefCell<i64> = const { RefCell::new(0) };
    /// The video players, by their view's key.
    pub static VIDEOS: RefCell<HashMap<u64, (String, Player)>> = RefCell::new(HashMap::new());
}

fn with_sound<T>(id: i64, f: impl FnOnce(&mut Player) -> T) -> Option<T> {
    SOUNDS.with(|s| s.borrow_mut().get_mut(&id).map(f))
}

/// `playSound(path, volume:loop:)`: starts playing the file; its id (0 when
/// it can't be played).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_play_sound(path: *const crate::Str, volume: f64, looping: u8) -> i64 {
    let path = unsafe { crate::str_text(path) }.to_string();
    let Some(mut player) = Player::open(&path, false) else {
        eprintln!("warning: can't play `{path}`");
        return 0;
    };
    player.looping = looping != 0;
    player.set_volume(volume);
    player.play();
    let id = LAST_SOUND.with(|l| {
        *l.borrow_mut() += 1;
        *l.borrow()
    });
    SOUNDS.with(|s| s.borrow_mut().insert(id, player));
    super::app::wake_for_media();
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_pause(id: i64) {
    with_sound(id, Player::pause);
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_resume(id: i64) {
    with_sound(id, Player::play);
    super::app::wake_for_media();
}

/// `sound.stop()`: it stops, and is let go.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_stop(id: i64) {
    if let Some(mut p) = SOUNDS.with(|s| s.borrow_mut().remove(&id)) {
        p.pause();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_seek(id: i64, to: f64) {
    with_sound(id, |p| p.seek(to));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_set_volume(id: i64, volume: f64) {
    with_sound(id, |p| p.set_volume(volume));
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_is_playing(id: i64) -> u8 {
    with_sound(id, |p| p.playing()).unwrap_or(false) as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_position(id: i64) -> f64 {
    with_sound(id, |p| p.position()).unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_sound_duration(id: i64) -> f64 {
    with_sound(id, |p| p.duration()).unwrap_or(0.0)
}

/// Loops the sounds that loop, lets go of those that ended (and don't), and
/// says whether any is playing (the app looks again soon, then).
pub fn tick_sounds() -> bool {
    SOUNDS.with(|s| {
        let mut s = s.borrow_mut();
        let mut any = false;
        for p in s.values_mut() {
            any |= p.tick();
        }
        any
    })
}

/// Plays the video at `path` with the system's player (muted), and says what
/// it found: its size, how long it is, and a picture's size. For
/// `tests/media.rs` (on the main thread, where the system's run loop is).
#[doc(hidden)]
pub fn check_video(path: &str) -> Result<String, String> {
    let mut p = Player::open(path, true).ok_or("can't open it")?;
    p.set_volume(0.0);
    p.play();
    let start = std::time::Instant::now();
    let mut picture = None;
    while start.elapsed().as_secs_f64() < 5.0 {
        #[cfg(target_os = "macos")]
        unsafe {
            // (Let the system's player tell what it's done.)
            unsafe extern "C" {
                fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, once: u8) -> i32;
                static kCFRunLoopDefaultMode: *const std::ffi::c_void;
            }
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0);
        }
        #[cfg(not(target_os = "macos"))]
        std::thread::sleep(std::time::Duration::from_millis(50));
        if p.take_picture() {
            picture = p.picture.clone();
            if p.position() > 0.3 {
                break;
            }
        }
    }
    let picture = picture.ok_or("no picture came")?;
    let at = p.position();
    p.seek(1.5);
    p.pause();
    Ok(format!(
        "size {:?}, {:.1} s long, picture {}x{}, played to {} s, then at {:.1} s",
        p.size(),
        p.duration(),
        picture.width,
        picture.height,
        if at > 0.0 { "more than 0" } else { "0" },
        p.position()
    ))
}

/// Drops every player (at exit).
pub fn forget_all() {
    SOUNDS.with(|s| s.borrow_mut().clear());
    VIDEOS.with(|v| v.borrow_mut().clear());
}

/// Drops the video players whose views are gone.
pub fn keep_only(keys: &std::collections::HashSet<u64>) {
    VIDEOS.with(|v| v.borrow_mut().retain(|k, _| keys.contains(k)));
}

// ===== macOS: AVFoundation =====

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use objc2::encode::{Encode, Encoding};
    #[allow(unused_imports)]
    use objc2::encode::RefEncode;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{class, msg_send};
    use objc2_foundation::{NSSize, NSString};

    /// A time, as Core Media has it.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    unsafe impl Encode for CMTime {
        const ENCODING: Encoding = Encoding::Struct("?", &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING]);
    }

    unsafe impl objc2::encode::RefEncode for CMTime {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    /// A Core Video pixel buffer (as its methods say it is).
    #[repr(C)]
    struct CVBuffer {
        _opaque: [u8; 0],
    }

    unsafe impl objc2::encode::RefEncode for CVBuffer {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CVBuffer", &[]));
    }

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        static kCVPixelBufferPixelFormatTypeKey: *const AnyObject;
        fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, flags: u64) -> i32;
        fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *const u8;
        fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
        fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
        fn CVBufferRelease(buffer: *mut c_void);
    }

    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {
        fn CACurrentMediaTime() -> f64;
    }

    /// Core Media's functions, from the framework once it's loaded.
    struct Media {
        seconds: unsafe extern "C" fn(CMTime) -> f64,
        make: unsafe extern "C" fn(f64, i32) -> CMTime,
    }

    fn media() -> Option<&'static Media> {
        static MEDIA: OnceLock<Option<Media>> = OnceLock::new();
        MEDIA
            .get_or_init(|| unsafe {
                let av = c"/System/Library/Frameworks/AVFoundation.framework/AVFoundation";
                let cm = c"/System/Library/Frameworks/CoreMedia.framework/CoreMedia";
                if libc::dlopen(av.as_ptr(), libc::RTLD_LAZY).is_null() {
                    return None;
                }
                let lib = libc::dlopen(cm.as_ptr(), libc::RTLD_LAZY);
                if lib.is_null() {
                    return None;
                }
                let seconds = libc::dlsym(lib, c"CMTimeGetSeconds".as_ptr());
                let make = libc::dlsym(lib, c"CMTimeMakeWithSeconds".as_ptr());
                if seconds.is_null() || make.is_null() {
                    return None;
                }
                Some(Media { seconds: std::mem::transmute::<*mut c_void, _>(seconds), make: std::mem::transmute::<*mut c_void, _>(make) })
            })
            .as_ref()
    }

    pub struct Player {
        player: *mut AnyObject,
        item: *mut AnyObject,
        /// What gives the video's pictures (none for sound).
        output: *mut AnyObject,
    }

    impl Player {
        pub fn open(path: &str, video: bool) -> Option<Player> {
            media()?;
            unsafe {
                let path = NSString::from_str(path);
                let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
                let item_class = AnyClass::get(c"AVPlayerItem")?;
                let item: *mut AnyObject = msg_send![item_class, playerItemWithURL: url];
                if item.is_null() {
                    return None;
                }
                let _: *mut AnyObject = msg_send![item, retain];
                let mut output: *mut AnyObject = std::ptr::null_mut();
                if video {
                    if let Some(out_class) = AnyClass::get(c"AVPlayerItemVideoOutput") {
                        // Blue, green, red and opacity, a byte each ('BGRA').
                        let format: *mut AnyObject = msg_send![class!(NSNumber), numberWithUnsignedInt: 0x42475241u32];
                        let key = kCVPixelBufferPixelFormatTypeKey;
                        let settings: *mut AnyObject = msg_send![class!(NSDictionary), dictionaryWithObject: format, forKey: key];
                        let o: *mut AnyObject = msg_send![out_class, alloc];
                        output = msg_send![o, initWithPixelBufferAttributes: settings];
                        if !output.is_null() {
                            let _: () = msg_send![item, addOutput: output];
                        }
                    }
                }
                let player_class = AnyClass::get(c"AVPlayer")?;
                let player: *mut AnyObject = msg_send![player_class, playerWithPlayerItem: item];
                if player.is_null() {
                    return None;
                }
                let _: *mut AnyObject = msg_send![player, retain];
                Some(Player { player, item, output })
            }
        }

        pub fn play(&mut self) {
            unsafe {
                // At the end, it starts again from the beginning.
                if self.duration() > 0.0 && self.position() >= self.duration() - 0.01 {
                    self.seek(0.0);
                }
                let _: () = msg_send![self.player, play];
            }
        }

        pub fn pause(&mut self) {
            unsafe {
                let _: () = msg_send![self.player, pause];
            }
        }

        pub fn playing(&self) -> bool {
            let rate: f32 = unsafe { msg_send![self.player, rate] };
            rate != 0.0
        }

        pub fn position(&self) -> f64 {
            let Some(m) = media() else { return 0.0 };
            unsafe {
                let t: CMTime = msg_send![self.player, currentTime];
                (m.seconds)(t)
            }
        }

        pub fn duration(&self) -> f64 {
            let Some(m) = media() else { return 0.0 };
            unsafe {
                let t: CMTime = msg_send![self.item, duration];
                (m.seconds)(t)
            }
        }

        pub fn seek(&mut self, to: f64) {
            let Some(m) = media() else { return };
            unsafe {
                let t = (m.make)(to, 600);
                // (Exactly there, not at the nearest key frame.)
                let zero = CMTime { value: 0, timescale: 1, flags: 1, epoch: 0 };
                let _: () = msg_send![self.player, seekToTime: t, toleranceBefore: zero, toleranceAfter: zero];
            }
        }

        pub fn set_volume(&mut self, volume: f64) {
            unsafe {
                let _: () = msg_send![self.player, setVolume: volume as f32];
            }
        }

        pub fn frame(&mut self) -> Option<(u32, u32, Vec<u8>)> {
            if self.output.is_null() {
                return None;
            }
            unsafe {
                let t: CMTime = msg_send![self.output, itemTimeForHostTime: CACurrentMediaTime()];
                let fresh: bool = msg_send![self.output, hasNewPixelBufferForItemTime: t];
                if !fresh {
                    return None;
                }
                let buffer: *mut CVBuffer = msg_send![self.output, copyPixelBufferForItemTime: t, itemTimeForDisplay: std::ptr::null_mut::<CMTime>()];
                let buffer = buffer as *mut c_void;
                if buffer.is_null() {
                    return None;
                }
                CVPixelBufferLockBaseAddress(buffer, 1);
                let (w, h, row) = (CVPixelBufferGetWidth(buffer), CVPixelBufferGetHeight(buffer), CVPixelBufferGetBytesPerRow(buffer));
                let base = CVPixelBufferGetBaseAddress(buffer);
                let mut rgba = Vec::with_capacity(w * h * 4);
                if !base.is_null() {
                    for y in 0..h {
                        let line = std::slice::from_raw_parts(base.add(y * row), w * 4);
                        for p in line.chunks_exact(4) {
                            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
                        }
                    }
                }
                CVPixelBufferUnlockBaseAddress(buffer, 1);
                CVBufferRelease(buffer);
                (rgba.len() == w * h * 4 && w > 0).then_some((w as u32, h as u32, rgba))
            }
        }

        pub fn size(&self) -> Option<(f64, f64)> {
            let s: NSSize = unsafe { msg_send![self.item, presentationSize] };
            Some((s.width, s.height))
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![self.player, pause];
                let _: () = msg_send![self.player, release];
                let _: () = msg_send![self.item, release];
                if !self.output.is_null() {
                    let _: () = msg_send![self.output, release];
                }
            }
        }
    }
}

// ===== Windows: Media Foundation =====

#[cfg(all(windows, feature = "native"))]
mod win {
    use std::sync::Once;

    use windows::core::{implement, BSTR};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory, WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
    };
    use windows::Win32::Media::MediaFoundation::{
        CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory, IMFMediaEngineNotify,
        IMFMediaEngineNotify_Impl, MFCreateAttributes, MFStartup, MF_MEDIA_ENGINE_AUDIOONLY, MF_MEDIA_ENGINE_CALLBACK,
        MF_MEDIA_ENGINE_CREATEFLAGS, MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MFSTARTUP_FULL, MF_VERSION,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    /// What the media engine tells of itself (nothing the runtime needs).
    #[implement(IMFMediaEngineNotify)]
    struct Notify;

    impl IMFMediaEngineNotify_Impl for Notify_Impl {
        fn EventNotify(&self, _event: u32, _a: usize, _b: u32) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub struct Player {
        engine: IMFMediaEngine,
        video: bool,
        wic: Option<IWICImagingFactory>,
    }

    impl Player {
        pub fn open(path: &str, video: bool) -> Option<Player> {
            static STARTED: Once = Once::new();
            STARTED.call_once(|| unsafe {
                let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
            });
            unsafe {
                let factory: IMFMediaEngineClassFactory = CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER).ok()?;
                let mut attributes: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut attributes, 2).ok()?;
                let attributes = attributes?;
                let notify: IMFMediaEngineNotify = Notify.into();
                attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify).ok()?;
                attributes.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32).ok()?;
                let flags = if video { MF_MEDIA_ENGINE_CREATEFLAGS(0) } else { MF_MEDIA_ENGINE_AUDIOONLY };
                let engine = factory.CreateInstance(flags.0 as u32, &attributes).ok()?;
                let url = format!("file:///{}", path.replace('\\', "/"));
                engine.SetSource(&BSTR::from(url)).ok()?;
                let wic = if video { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() } else { None };
                Some(Player { engine, video, wic })
            }
        }

        pub fn play(&mut self) {
            unsafe {
                if self.engine.IsEnded().as_bool() {
                    let _ = self.engine.SetCurrentTime(0.0);
                }
                let _ = self.engine.Play();
            }
        }

        pub fn pause(&mut self) {
            unsafe {
                let _ = self.engine.Pause();
            }
        }

        pub fn playing(&self) -> bool {
            unsafe { !self.engine.IsPaused().as_bool() && !self.engine.IsEnded().as_bool() }
        }

        pub fn position(&self) -> f64 {
            unsafe { self.engine.GetCurrentTime() }
        }

        pub fn duration(&self) -> f64 {
            unsafe { self.engine.GetDuration() }
        }

        pub fn seek(&mut self, to: f64) {
            unsafe {
                let _ = self.engine.SetCurrentTime(to);
            }
        }

        pub fn set_volume(&mut self, volume: f64) {
            unsafe {
                let _ = self.engine.SetVolume(volume);
            }
        }

        pub fn frame(&mut self) -> Option<(u32, u32, Vec<u8>)> {
            if !self.video {
                return None;
            }
            unsafe {
                let mut pts = 0i64;
                // (S_OK: there's a new picture; S_FALSE: there isn't.)
                let tick = (windows::core::Interface::vtable(&self.engine).OnVideoStreamTick)(windows::core::Interface::as_raw(&self.engine), &mut pts);
                if tick != windows::Win32::Foundation::S_OK {
                    return None;
                }
                let (mut w, mut h) = (0u32, 0u32);
                self.engine.GetNativeVideoSize(Some(&mut w), Some(&mut h)).ok()?;
                if w == 0 || h == 0 {
                    return None;
                }
                let bitmap = self.wic.as_ref()?.CreateBitmap(w, h, &GUID_WICPixelFormat32bppBGRA, WICBitmapCacheOnLoad).ok()?;
                let all = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
                self.engine.TransferVideoFrame(&bitmap, None, &all, None).ok()?;
                let lock = bitmap.Lock(&WICRect { X: 0, Y: 0, Width: w as i32, Height: h as i32 }, WICBitmapLockRead.0 as u32).ok()?;
                let stride = lock.GetStride().ok()? as usize;
                let (mut size, mut data) = (0u32, std::ptr::null_mut());
                lock.GetDataPointer(&mut size, &mut data).ok()?;
                let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                for y in 0..h as usize {
                    let line = std::slice::from_raw_parts(data.add(y * stride), w as usize * 4);
                    for p in line.chunks_exact(4) {
                        rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
                    }
                }
                Some((w, h, rgba))
            }
        }

        pub fn size(&self) -> Option<(f64, f64)> {
            let (mut w, mut h) = (0u32, 0u32);
            unsafe { self.engine.GetNativeVideoSize(Some(&mut w), Some(&mut h)).ok()? };
            Some((w as f64, h as f64))
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            unsafe {
                let _ = self.engine.Shutdown();
            }
        }
    }
}
