//! `WebView(url:)` and `WebView(html:)`: web pages shown by the system's
//! own browser engine, in a view of the system's put over the window where
//! the `WebView` is laid out (WKWebView on macOS; WebView2 on Windows, which
//! Windows 11 and nearly every Windows 10 have). Being the system's, it's
//! drawn over the app's views: a popover doesn't show over it. Elsewhere,
//! and in a headless run, the view shows what it would load.

use std::collections::HashMap;

use winit::window::Window;

use super::layout::Rect;

/// What a web view is to show, and where (in points, in its window).
#[derive(Clone, PartialEq)]
pub struct Wanted {
    pub url: String,
    pub html: String,
    pub rect: Rect,
    pub visible: bool,
}

/// Puts the system's web views of `window` where `views` say (by the key of
/// their `WebView`), loading what they show when that changed, and takes
/// away those that aren't there any more.
pub fn sync(window: &Window, scale: f64, views: &HashMap<u64, Wanted>) {
    let _ = (window, scale, views);
    #[cfg(target_os = "macos")]
    mac::sync(window, views);
    #[cfg(all(windows, feature = "native"))]
    win::sync(window, scale, views);
}

/// Shows a page in a web view of its own, and says its title once it's
/// loaded (for `tests/web.rs`).
#[doc(hidden)]
pub fn check_web_view(html: &str) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return mac::check(html);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = html;
        Err("only on macOS".into())
    }
}

/// Whether `url` is a file's path rather than a web address.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn is_file(url: &str) -> bool {
    !url.contains("://") && !url.starts_with("about:") && !url.starts_with("data:") && !url.starts_with("mailto:")
}

// ===== macOS: WKWebView =====

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{class, msg_send};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use super::Wanted;

    struct Shown {
        view: *mut AnyObject,
        parent: *mut AnyObject,
        /// What it was last told to load (url, html).
        loaded: (String, String),
    }

    thread_local! {
        static SHOWN: RefCell<HashMap<u64, Shown>> = RefCell::new(HashMap::new());
    }

    fn webkit() -> bool {
        static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *LOADED.get_or_init(|| unsafe { !libc::dlopen(c"/System/Library/Frameworks/WebKit.framework/WebKit".as_ptr(), libc::RTLD_LAZY).is_null() })
    }

    fn load(view: *mut AnyObject, url: &str, html: &str) {
        unsafe {
            if !url.is_empty() {
                if super::is_file(url) {
                    // A file, with the folder it's in (for its pictures and styles).
                    let path = NSString::from_str(url);
                    let file: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
                    let folder: *mut AnyObject = msg_send![file, URLByDeletingLastPathComponent];
                    let _: *mut AnyObject = msg_send![view, loadFileURL: file, allowingReadAccessToURL: folder];
                } else {
                    let text = NSString::from_str(url);
                    let address: *mut AnyObject = msg_send![class!(NSURL), URLWithString: &*text];
                    if address.is_null() {
                        return;
                    }
                    let request: *mut AnyObject = msg_send![class!(NSURLRequest), requestWithURL: address];
                    let _: *mut AnyObject = msg_send![view, loadRequest: request];
                }
            } else {
                let text = NSString::from_str(html);
                let nil: *mut AnyObject = std::ptr::null_mut();
                let _: *mut AnyObject = msg_send![view, loadHTMLString: &*text, baseURL: nil];
            }
        }
    }

    pub fn sync(window: &Window, views: &HashMap<u64, Wanted>) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        sync_in(h.ns_view.as_ptr() as *mut AnyObject, views);
    }

    /// Loads a page into a web view in a view of its own (no window), and
    /// says the page's title once it's loaded. For `tests/web.rs`, on the
    /// main thread.
    pub fn check(html: &str) -> Result<String, String> {
        unsafe {
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 200.0));
            let parent: *mut AnyObject = msg_send![class!(NSView), alloc];
            let parent: *mut AnyObject = msg_send![parent, initWithFrame: frame];
            let mut views = HashMap::new();
            let rect = super::super::layout::Rect { x: 10.0, y: 10.0, w: 300.0, h: 180.0 };
            views.insert(1, Wanted { url: String::new(), html: html.to_string(), rect, visible: true });
            sync_in(parent, &views);
            let view = SHOWN.with(|s| s.borrow().get(&1).map(|s| s.view)).ok_or("no web view")?;
            let start = std::time::Instant::now();
            unsafe extern "C" {
                fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, once: u8) -> i32;
                static kCFRunLoopDefaultMode: *const std::ffi::c_void;
            }
            while start.elapsed().as_secs_f64() < 10.0 {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0);
                let title: *mut AnyObject = msg_send![view, title];
                if !title.is_null() {
                    let text: *const std::ffi::c_char = msg_send![title, UTF8String];
                    let title = std::ffi::CStr::from_ptr(text).to_string_lossy().to_string();
                    if !title.is_empty() {
                        let f: NSRect = msg_send![view, frame];
                        sync_in(parent, &HashMap::new());
                        return Ok(format!("title {title:?}, frame {} {} {} {}", f.origin.x, f.origin.y, f.size.width, f.size.height));
                    }
                }
            }
            Err("the page didn't load".into())
        }
    }

    fn sync_in(parent: *mut AnyObject, views: &HashMap<u64, Wanted>) {
        SHOWN.with(|shown| {
            let mut shown = shown.borrow_mut();
            // Those of this window that aren't there any more go.
            let gone: Vec<u64> = shown.iter().filter(|(k, s)| s.parent == parent && !views.contains_key(k)).map(|(k, _)| *k).collect();
            for key in gone {
                if let Some(s) = shown.remove(&key) {
                    unsafe {
                        let _: () = msg_send![s.view, removeFromSuperview];
                        let _: () = msg_send![s.view, release];
                    }
                }
            }
            if views.is_empty() || !webkit() {
                return;
            }
            let Some(web_class) = AnyClass::get(c"WKWebView") else { return };
            let Some(config_class) = AnyClass::get(c"WKWebViewConfiguration") else { return };
            unsafe {
                let bounds: NSRect = msg_send![parent, bounds];
                let flipped: bool = msg_send![parent, isFlipped];
                for (key, want) in views {
                    let r = want.rect;
                    let y = if flipped { r.y } else { bounds.size.height - r.y - r.h };
                    let frame = NSRect::new(NSPoint::new(r.x, y), NSSize::new(r.w.max(0.0), r.h.max(0.0)));
                    let s = shown.entry(*key).or_insert_with(|| {
                        let config: *mut AnyObject = msg_send![config_class, new];
                        let view: *mut AnyObject = msg_send![web_class, alloc];
                        let view: *mut AnyObject = msg_send![view, initWithFrame: frame, configuration: config];
                        let _: () = msg_send![config, release];
                        let _: () = msg_send![parent, addSubview: view];
                        Shown { view, parent, loaded: (String::new(), String::new()) }
                    });
                    let _: () = msg_send![s.view, setFrame: frame];
                    let _: () = msg_send![s.view, setHidden: !want.visible];
                    if s.loaded != (want.url.clone(), want.html.clone()) {
                        s.loaded = (want.url.clone(), want.html.clone());
                        load(s.view, &want.url, &want.html);
                    }
                }
            }
        });
    }
}

// ===== Windows: WebView2 =====

#[cfg(all(windows, feature = "native"))]
#[allow(non_snake_case)]
mod win {
    //! WebView2, without its loader library: the runtime installed with the
    //! system (or by the user) is found in its folder, and its environment
    //! made with the function its DLL has for that. Its interfaces are used
    //! by their places in their tables of functions, as WebView2.h has them.

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::c_void;

    use windows::core::{implement, interface, IUnknown, IUnknown_Vtbl, Interface, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{HMODULE, HWND, RECT};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use super::Wanted;

    /// `ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler`.
    #[interface("4e8a3389-c9d8-4bd2-b6b5-124fee6cc14d")]
    unsafe trait EnvironmentMade: IUnknown {
        fn Invoke(&self, result: HRESULT, environment: *mut c_void) -> HRESULT;
    }

    /// `ICoreWebView2CreateCoreWebView2ControllerCompletedHandler`.
    #[interface("6c4819f3-c9b7-4260-8127-c9f5bde7f68c")]
    unsafe trait ControllerMade: IUnknown {
        fn Invoke(&self, result: HRESULT, controller: *mut c_void) -> HRESULT;
    }

    // Places in the interfaces' tables (after IUnknown's three).
    const ENV_CREATE_CONTROLLER: usize = 3;
    const CONTROLLER_SET_VISIBLE: usize = 4;
    const CONTROLLER_SET_BOUNDS: usize = 6;
    const CONTROLLER_CLOSE: usize = 24;
    const CONTROLLER_WEBVIEW: usize = 25;
    const WEBVIEW_NAVIGATE: usize = 5;
    const WEBVIEW_NAVIGATE_TO_STRING: usize = 6;

    /// The function at `index` in a COM object's table.
    unsafe fn method<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let table = *(object as *const *const usize);
            std::mem::transmute_copy(&*table.add(index))
        }
    }

    unsafe fn release(object: *mut c_void) {
        if !object.is_null() {
            unsafe { drop(IUnknown::from_raw(object)) };
        }
    }

    enum Environment {
        None,
        Making,
        Made(*mut c_void),
        Failed,
    }

    /// A web view: being made, or made (its controller and browser).
    struct Shown {
        hwnd: isize,
        controller: *mut c_void,
        webview: *mut c_void,
        want: Wanted,
        loaded: (String, String),
        scale: f64,
    }

    thread_local! {
        static ENVIRONMENT: RefCell<Environment> = const { RefCell::new(Environment::None) };
        static SHOWN: RefCell<HashMap<u64, Shown>> = RefCell::new(HashMap::new());
    }

    /// The newest WebView2 runtime's DLL installed, where the system (or the user) has them.
    fn runtime_dll() -> Option<Vec<u16>> {
        let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
        let roots = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"];
        let mut best: Option<(Vec<u32>, std::path::PathBuf)> = None;
        for root in roots {
            let Some(base) = std::env::var_os(root) else { continue };
            let apps = std::path::Path::new(&base).join("Microsoft").join("EdgeWebView").join("Application");
            let Ok(entries) = std::fs::read_dir(&apps) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Some(version) = name.split('.').map(|p| p.parse::<u32>().ok()).collect::<Option<Vec<u32>>>() else { continue };
                let dll = e.path().join("EBWebView").join(arch).join("EmbeddedBrowserWebView.dll");
                if dll.exists() && best.as_ref().is_none_or(|(v, _)| version > *v) {
                    best = Some((version, dll));
                }
            }
        }
        let (_, dll) = best?;
        Some(dll.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect())
    }

    /// Where the browser keeps what it keeps: the app's own folder in the user's.
    fn data_folder() -> Vec<u16> {
        let app = std::env::current_exe().ok().and_then(|e| e.file_stem().map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "Tessel".into());
        let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let folder = base.join(app).join("WebView2");
        let _ = std::fs::create_dir_all(&folder);
        folder.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect()
    }

    #[implement(EnvironmentMade)]
    struct EnvironmentDone;

    impl EnvironmentMade_Impl for EnvironmentDone_Impl {
        unsafe fn Invoke(&self, result: HRESULT, environment: *mut c_void) -> HRESULT {
            if result.is_err() || environment.is_null() {
                ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Failed);
                eprintln!("warning: WebView2 can't start ({result:?})");
                return HRESULT(0);
            }
            unsafe {
                // (Kept: the handler's is only lent.)
                let kept = IUnknown::from_raw_borrowed(&environment).map(|u| u.clone().into_raw());
                ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Made(kept.unwrap_or(environment)));
            }
            // The views waiting for it.
            let waiting: Vec<(u64, isize)> = SHOWN.with(|s| s.borrow().iter().map(|(k, v)| (*k, v.hwnd)).collect());
            for (key, hwnd) in waiting {
                make_controller(key, HWND(hwnd as *mut c_void));
            }
            HRESULT(0)
        }
    }

    #[implement(ControllerMade)]
    struct ControllerDone {
        key: u64,
    }

    impl ControllerMade_Impl for ControllerDone_Impl {
        unsafe fn Invoke(&self, result: HRESULT, controller: *mut c_void) -> HRESULT {
            if result.is_err() || controller.is_null() {
                return HRESULT(0);
            }
            unsafe {
                let Some(controller) = IUnknown::from_raw_borrowed(&controller).map(|u| u.clone().into_raw()) else { return HRESULT(0) };
                let mut webview: *mut c_void = std::ptr::null_mut();
                let get: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT = method(controller, CONTROLLER_WEBVIEW);
                let _ = get(controller, &mut webview);
                let gone = SHOWN.with(|s| {
                    let mut s = s.borrow_mut();
                    match s.get_mut(&self.key) {
                        Some(shown) => {
                            shown.controller = controller;
                            shown.webview = webview;
                            false
                        }
                        None => true,
                    }
                });
                if gone {
                    close(controller, webview);
                    return HRESULT(0);
                }
                SHOWN.with(|s| {
                    if let Some(shown) = s.borrow_mut().get_mut(&self.key) {
                        apply(shown, true);
                    }
                });
            }
            HRESULT(0)
        }
    }

    fn start_environment() {
        let Some(dll) = runtime_dll() else {
            ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Failed);
            eprintln!("warning: WebView2 isn't installed, so `WebView` shows nothing");
            return;
        };
        ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Making);
        unsafe {
            let Ok(module): Result<HMODULE, _> = LoadLibraryW(PCWSTR(dll.as_ptr())) else {
                ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Failed);
                return;
            };
            let Some(create) = GetProcAddress(module, windows::core::s!("CreateWebViewEnvironmentWithOptionsInternal")) else {
                ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Failed);
                return;
            };
            type Create = unsafe extern "system" fn(bool, i32, PCWSTR, *mut c_void, *mut c_void) -> HRESULT;
            let create: Create = std::mem::transmute(create);
            let folder = data_folder();
            let handler: EnvironmentMade = EnvironmentDone.into();
            // (The installed runtime; no options.)
            let made = create(true, 0, PCWSTR(folder.as_ptr()), std::ptr::null_mut(), handler.as_raw());
            if made.is_err() {
                ENVIRONMENT.with(|e| *e.borrow_mut() = Environment::Failed);
                eprintln!("warning: WebView2 can't start ({made:?})");
            }
        }
    }

    fn make_controller(key: u64, hwnd: HWND) {
        let env = ENVIRONMENT.with(|e| match *e.borrow() {
            Environment::Made(p) => Some(p),
            _ => None,
        });
        let Some(env) = env else { return };
        unsafe {
            let handler: ControllerMade = ControllerDone { key }.into();
            let create: unsafe extern "system" fn(*mut c_void, HWND, *mut c_void) -> HRESULT = method(env, ENV_CREATE_CONTROLLER);
            let _ = create(env, hwnd, handler.as_raw());
        }
    }

    unsafe fn close(controller: *mut c_void, webview: *mut c_void) {
        unsafe {
            if !controller.is_null() {
                let close: unsafe extern "system" fn(*mut c_void) -> HRESULT = method(controller, CONTROLLER_CLOSE);
                let _ = close(controller);
            }
            release(webview);
            release(controller);
        }
    }

    /// Gives a made web view its place and visibility, and what it shows.
    fn apply(s: &mut Shown, force: bool) {
        if s.controller.is_null() {
            return;
        }
        unsafe {
            let r = s.want.rect;
            let px = |v: f64| (v * s.scale).round() as i32;
            let bounds = RECT { left: px(r.x), top: px(r.y), right: px(r.x + r.w), bottom: px(r.y + r.h) };
            let set_bounds: unsafe extern "system" fn(*mut c_void, RECT) -> HRESULT = method(s.controller, CONTROLLER_SET_BOUNDS);
            let _ = set_bounds(s.controller, bounds);
            let set_visible: unsafe extern "system" fn(*mut c_void, windows::core::BOOL) -> HRESULT = method(s.controller, CONTROLLER_SET_VISIBLE);
            let _ = set_visible(s.controller, s.want.visible.into());
            let wanted = (s.want.url.clone(), s.want.html.clone());
            if (force || s.loaded != wanted) && !s.webview.is_null() {
                s.loaded = wanted;
                let wide = |t: &str| -> Vec<u16> { t.encode_utf16().chain(std::iter::once(0)).collect() };
                if !s.want.url.is_empty() {
                    let address = if super::is_file(&s.want.url) { format!("file:///{}", s.want.url.replace('\\', "/")) } else { s.want.url.clone() };
                    let text = wide(&address);
                    let navigate: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT = method(s.webview, WEBVIEW_NAVIGATE);
                    let _ = navigate(s.webview, PCWSTR(text.as_ptr()));
                } else {
                    let text = wide(&s.want.html);
                    let show: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT = method(s.webview, WEBVIEW_NAVIGATE_TO_STRING);
                    let _ = show(s.webview, PCWSTR(text.as_ptr()));
                }
            }
        }
    }

    pub fn sync(window: &Window, scale: f64, views: &HashMap<u64, Wanted>) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = h.hwnd.get();
        let gone: Vec<(*mut c_void, *mut c_void)> = SHOWN.with(|s| {
            let mut s = s.borrow_mut();
            let keys: Vec<u64> = s.iter().filter(|(k, v)| v.hwnd == hwnd && !views.contains_key(k)).map(|(k, _)| *k).collect();
            keys.into_iter().filter_map(|k| s.remove(&k)).map(|v| (v.controller, v.webview)).collect()
        });
        for (controller, webview) in gone {
            unsafe { close(controller, webview) };
        }
        if views.is_empty() {
            return;
        }
        let state = ENVIRONMENT.with(|e| match *e.borrow() {
            Environment::None => 0,
            Environment::Making => 1,
            Environment::Made(_) => 2,
            Environment::Failed => 3,
        });
        if state == 3 {
            return;
        }
        let mut new = Vec::new();
        SHOWN.with(|s| {
            let mut s = s.borrow_mut();
            for (key, want) in views {
                match s.get_mut(key) {
                    Some(shown) => {
                        shown.scale = scale;
                        if shown.want != *want {
                            shown.want = want.clone();
                            apply(shown, false);
                        }
                    }
                    None => {
                        s.insert(*key, Shown { hwnd, controller: std::ptr::null_mut(), webview: std::ptr::null_mut(), want: want.clone(), loaded: (String::new(), String::new()), scale });
                        new.push(*key);
                    }
                }
            }
        });
        if state == 0 {
            start_environment();
        } else if state == 2 {
            for key in new {
                make_controller(key, HWND(hwnd as *mut c_void));
            }
        }
    }
}
