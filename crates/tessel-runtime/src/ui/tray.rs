//! The app's icon in the system's tray (`.trayIcon`): the menu bar's right
//! side on macOS, the taskbar's notification area on Windows; and the
//! system's notifications (`showNotification`). Elsewhere, neither.
//!
//! The icon is one of the built-in ones, drawn by the runtime. Its menu's
//! items come to the app as `Shell::Tray` (a click on the icon itself on
//! Windows as `Shell::TrayClick`), and run like the menu bar's.

use super::menu::Entry;
use winit::window::Window;

/// What a `.trayIcon` says, as the app keeps it between builds.
#[derive(Clone)]
pub struct TrayInfo {
    pub icon: &'static str,
    pub title: String,
    pub keep_running: bool,
    pub items: Vec<Entry>,
}

impl TrayInfo {
    /// All it shows, to tell when it changed.
    pub fn describe(&self) -> String {
        let mut out = format!("{} {:?}\n", self.icon, self.title);
        super::menu::describe(&self.items, 0, &mut out);
        out
    }
}

/// Draws a built-in icon `px` pixels square in `color`: its pixels, four
/// bytes each (red, green, blue, opacity).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn icon_pixels(name: &str, px: u32, color: super::Rgba) -> Vec<u8> {
    super::svg::enable_icons();
    let Some(svg) = super::svg::icon(name, color) else { return vec![0; (px * px * 4) as usize] };
    let mut list = super::draw::DrawList::new();
    let s = px as f64 / svg.width.max(svg.height).max(1.0);
    list.svg(svg, kurbo::Affine::scale(s));
    super::backend::Offscreen::default().rgba(&list, px, px, None)
}

// ===== macOS =====

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;

    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
    use objc2::{class, msg_send, sel};
    use objc2_foundation::NSString;

    use super::TrayInfo;

    thread_local! {
        /// The status item showing (retained).
        static ITEM: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
        /// The notification center, once it's been asked to let the app
        /// notify (and given the delegate that shows them in front).
        static CENTER: Cell<*mut AnyObject> = const { Cell::new(std::ptr::null_mut()) };
        static ASKED: Cell<bool> = const { Cell::new(false) };
        static COUNT: RefCell<u64> = const { RefCell::new(0) };
    }

    pub fn set(info: Option<&TrayInfo>) {
        unsafe {
            let bar: *mut AnyObject = msg_send![class!(NSStatusBar), systemStatusBar];
            let mut item = ITEM.get();
            let Some(info) = info else {
                if !item.is_null() {
                    let _: () = msg_send![bar, removeStatusItem: item];
                    let _: () = msg_send![item, release];
                    ITEM.set(std::ptr::null_mut());
                }
                return;
            };
            if item.is_null() {
                // (NSVariableStatusItemLength: as wide as what it shows.)
                item = msg_send![bar, statusItemWithLength: -1.0f64];
                let _: *mut AnyObject = msg_send![item, retain];
                ITEM.set(item);
            }
            let button: *mut AnyObject = msg_send![item, button];
            if !button.is_null() {
                // The icon, drawn black (the system shows it in the menu bar's color).
                let px = 36;
                let pixels = super::icon_pixels(info.icon, px, super::super::Rgba([0, 0, 0, 255]));
                let file = std::env::temp_dir().join(format!("tessel-tray-{}-{}.png", std::process::id(), info.icon));
                let path = file.to_string_lossy().to_string();
                if super::super::backend::save_rgba(&path, px, px, pixels) {
                    let path = NSString::from_str(&path);
                    let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
                    let image: *mut AnyObject = msg_send![image, initWithContentsOfFile: &*path];
                    if !image.is_null() {
                        let _: () = msg_send![image, setSize: objc2_foundation::NSSize::new(18.0, 18.0)];
                        let _: () = msg_send![image, setTemplate: true];
                        let _: () = msg_send![button, setImage: image];
                        let _: () = msg_send![image, release];
                    }
                }
                let title = NSString::from_str(&info.title);
                let _: () = msg_send![button, setTitle: &*title];
                // (NSImageLeft: the icon before the title.)
                let _: () = msg_send![button, setImagePosition: if info.title.is_empty() { 1usize } else { 2usize }];
            }
            let menu = super::super::native::tray_menu(&info.items);
            let _: () = msg_send![item, setMenu: &*menu];
        }
    }

    /// A block taking a `BOOL` and an `NSError` (and doing nothing), for
    /// `requestAuthorizationWithOptions:completionHandler:`.
    #[repr(C)]
    struct Block {
        isa: *const c_void,
        flags: i32,
        reserved: i32,
        invoke: unsafe extern "C" fn(*mut Block, bool, *mut AnyObject),
        descriptor: *const Descriptor,
    }

    #[repr(C)]
    struct Descriptor {
        reserved: usize,
        size: usize,
    }

    unsafe impl objc2::encode::RefEncode for Block {
        // (As AppKit's methods say a block is.)
        const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Block;
    }

    unsafe extern "C" {
        static _NSConcreteGlobalBlock: c_void;
    }

    unsafe extern "C" fn answered(_block: *mut Block, _granted: bool, _error: *mut AnyObject) {}

    /// `userNotificationCenter:willPresentNotification:withCompletionHandler:`:
    /// a notification shows while the app is in front too (as a banner,
    /// with its sound, and in the list).
    extern "C-unwind" fn will_present(_this: *mut AnyObject, _cmd: Sel, _center: *mut AnyObject, _note: *mut AnyObject, handler: *mut c_void) {
        if handler.is_null() {
            return;
        }
        // A block's function is after its class, flags and reserved field.
        let invoke: unsafe extern "C" fn(*mut c_void, usize) = unsafe { *(handler as *const u8).add(16).cast() };
        const SOUND: usize = 1 << 1;
        const LIST: usize = 1 << 3;
        const BANNER: usize = 1 << 4;
        unsafe { invoke(handler, BANNER | LIST | SOUND) };
    }

    /// The notification center, when the app can have one: a bundled app
    /// (one run on its own, without a bundle, has no identity to notify as).
    unsafe fn center() -> Option<*mut AnyObject> {
        let have = CENTER.get();
        if !have.is_null() {
            return Some(have);
        }
        unsafe {
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            let id: *mut AnyObject = msg_send![bundle, bundleIdentifier];
            if id.is_null() {
                return None;
            }
            let framework = c"/System/Library/Frameworks/UserNotifications.framework/UserNotifications";
            if libc::dlopen(framework.as_ptr(), libc::RTLD_LAZY).is_null() {
                return None;
            }
            let class = AnyClass::get(c"UNUserNotificationCenter")?;
            let center: *mut AnyObject = msg_send![class, currentNotificationCenter];
            if center.is_null() {
                return None;
            }
            let _: *mut AnyObject = msg_send![center, retain];
            // Its delegate, so notifications show while the app is in front.
            if let Some(mut builder) = ClassBuilder::new(c"TesselNotificationDelegate", class!(NSObject)) {
                type Present = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject, *mut c_void);
                builder.add_method(sel!(userNotificationCenter:willPresentNotification:withCompletionHandler:), will_present as Present);
                let delegate_class = builder.register();
                let delegate: *mut AnyObject = msg_send![delegate_class, new];
                let _: () = msg_send![center, setDelegate: delegate];
            }
            CENTER.set(center);
            Some(center)
        }
    }

    pub fn notify(title: &str, message: &str) {
        unsafe {
            let Some(center) = center() else {
                // Without a bundle (`tessel run`): through the system's scripting.
                let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
                let script = format!("display notification \"{}\" with title \"{}\"", quote(message), quote(title));
                let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
                return;
            };
            if !ASKED.replace(true) {
                // The first time, the system asks the user whether the app may notify.
                let descriptor = Box::leak(Box::new(Descriptor { reserved: 0, size: std::mem::size_of::<Block>() }));
                let block = Box::leak(Box::new(Block {
                    isa: &raw const _NSConcreteGlobalBlock,
                    flags: 1 << 28,
                    reserved: 0,
                    invoke: answered,
                    descriptor,
                }));
                // (Alert and sound.)
                let options: usize = (1 << 2) | (1 << 1);
                let _: () = msg_send![center, requestAuthorizationWithOptions: options, completionHandler: block as *mut Block];
            }
            let Some(content_class) = AnyClass::get(c"UNMutableNotificationContent") else { return };
            let Some(request_class) = AnyClass::get(c"UNNotificationRequest") else { return };
            let content: *mut AnyObject = msg_send![content_class, new];
            let (t, m) = (NSString::from_str(title), NSString::from_str(message));
            let _: () = msg_send![content, setTitle: &*t];
            let _: () = msg_send![content, setBody: &*m];
            if let Some(sound) = AnyClass::get(c"UNNotificationSound") {
                let s: *mut AnyObject = msg_send![sound, defaultSound];
                let _: () = msg_send![content, setSound: s];
            }
            let n = COUNT.with(|c| {
                *c.borrow_mut() += 1;
                *c.borrow()
            });
            let id = NSString::from_str(&format!("tessel-{}-{n}", std::process::id()));
            let nil: *mut AnyObject = std::ptr::null_mut();
            let request: *mut AnyObject = msg_send![request_class, requestWithIdentifier: &*id, content: content, trigger: nil];
            let _: () = msg_send![center, addNotificationRequest: request, withCompletionHandler: std::ptr::null_mut::<Block>()];
            let _: () = msg_send![content, release];
        }
    }

    pub fn shutdown() {
        set(None);
    }
}

// ===== Windows =====

#[cfg(all(windows, feature = "native"))]
mod win {
    use std::cell::{Cell, RefCell};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        DefSubclassProc, SetWindowSubclass, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_STATE, NIF_TIP, NIIF_INFO,
        NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIS_HIDDEN, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, LoadIconW, PostMessageW,
        SetForegroundWindow, TrackPopupMenu, HICON, HMENU, ICONINFO, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
        MF_STRING, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
    };

    use super::super::app::{shell_post, Shell};
    use super::super::menu::Entry;
    use super::TrayInfo;

    /// The message the icons send the main window.
    const MESSAGE: u32 = WM_APP + 0x7E5;
    /// The icon of `.trayIcon`, and the hidden one notifications come from
    /// when there's none.
    const TRAY: u32 = 1;
    const NOTICE: u32 = 2;
    // The icon's events (in the low word of `lParam`, with version 4).
    const LEFT_UP: u32 = 0x0202;
    const RIGHT_UP: u32 = 0x0205;
    const CONTEXT: u32 = 0x007B;
    const BALLOON_CLICKED: u32 = 0x0405;

    thread_local! {
        static HWND_IN: Cell<isize> = const { Cell::new(0) };
        static SHOWN: Cell<[bool; 3]> = const { Cell::new([false; 3]) };
        static ICON: Cell<isize> = const { Cell::new(0) };
        /// The menu's items, by their command's number (from 1): their paths.
        static ITEMS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    }

    fn wide(s: &str, out: &mut [u16]) {
        let w: Vec<u16> = s.encode_utf16().take(out.len() - 1).collect();
        out[..w.len()].copy_from_slice(&w);
        out[w.len()] = 0;
    }

    fn data(hwnd: HWND, id: u32) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: id, ..Default::default() }
    }

    /// Whether the taskbar is dark (Windows' default), to draw the icon light on it.
    fn dark_taskbar() -> bool {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
        let name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
        let mut value = 0u32;
        let mut size = 4u32;
        let read = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as *mut _), Some(&mut size))
        };
        read.is_err() || value == 0
    }

    /// An icon from pixels (red, green, blue, opacity).
    fn icon_of(rgba: &[u8], px: u32) -> Option<HICON> {
        // Windows wants blue, green, red, opacity, with the colors premultiplied.
        let bgra: Vec<u8> = rgba
            .chunks(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                [(p[2] as u32 * a / 255) as u8, (p[1] as u32 * a / 255) as u8, (p[0] as u32 * a / 255) as u8, p[3]]
            })
            .collect();
        unsafe {
            let color = CreateBitmap(px as i32, px as i32, 1, 32, Some(bgra.as_ptr() as *const _));
            let mask_bits = vec![0u8; (px * px / 8) as usize];
            let mask = CreateBitmap(px as i32, px as i32, 1, 1, Some(mask_bits.as_ptr() as *const _));
            let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
            let icon = CreateIconIndirect(&info).ok();
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(mask.into());
            icon
        }
    }

    unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
        if msg == MESSAGE {
            let event = (lparam.0 as u32) & 0xFFFF;
            let icon = ((lparam.0 as u32) >> 16) & 0xFFFF;
            match event {
                RIGHT_UP | CONTEXT if icon == TRAY => show_menu(hwnd),
                LEFT_UP if icon == TRAY => shell_post(Shell::TrayClick),
                BALLOON_CLICKED => shell_post(Shell::TrayClick),
                _ => {}
            }
            return LRESULT(0);
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    /// Hooks the icons' messages into the main window (once).
    fn hook(hwnd: HWND) {
        if HWND_IN.get() == hwnd.0 as isize {
            return;
        }
        HWND_IN.set(hwnd.0 as isize);
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(subclass), 0x7E55, 0);
        }
    }

    /// Adds `items` to `menu`, numbering the commands from `ids` on (with their paths).
    unsafe fn add(menu: HMENU, items: &[Entry], path: &mut Vec<usize>, paths: &mut Vec<Vec<usize>>) {
        for (i, e) in items.iter().enumerate() {
            path.push(i);
            unsafe {
                match e {
                    Entry::Divider => {
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    }
                    Entry::Item { label, enabled, checked, .. } => {
                        paths.push(path.clone());
                        let text: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
                        let mut flags = MF_STRING;
                        if !enabled {
                            flags |= MF_GRAYED;
                        }
                        if *checked {
                            flags |= MF_CHECKED;
                        }
                        let _ = AppendMenuW(menu, flags, paths.len(), PCWSTR(text.as_ptr()));
                    }
                    Entry::Sub { title, enabled, items } => {
                        let Ok(sub) = CreatePopupMenu() else { continue };
                        add(sub, items, path, paths);
                        let text: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                        let flags = if *enabled { MF_POPUP } else { MF_POPUP | MF_GRAYED };
                        let _ = AppendMenuW(menu, flags, sub.0 as usize, PCWSTR(text.as_ptr()));
                    }
                }
            }
            path.pop();
        }
    }

    fn show_menu(hwnd: HWND) {
        let items = ITEMS.with(|i| i.borrow().clone());
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let mut paths = Vec::new();
            add(menu, &items, &mut Vec::new(), &mut paths);
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            // (So the menu closes when the user clicks elsewhere.)
            let _ = SetForegroundWindow(hwnd);
            let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, at.x, at.y, None, hwnd, None);
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            if let Some(path) = (chosen.0 as usize).checked_sub(1).and_then(|i| paths.get(i)) {
                shell_post(Shell::Tray(path.clone()));
            }
        }
    }

    pub fn set(hwnd: Option<HWND>, info: Option<&TrayInfo>) {
        let Some(hwnd) = hwnd else { return };
        hook(hwnd);
        let mut shown = SHOWN.get();
        unsafe {
            let Some(info) = info else {
                if shown[TRAY as usize] {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd, TRAY));
                    shown[TRAY as usize] = false;
                }
                SHOWN.set(shown);
                return;
            };
            let px = 32;
            let color = if dark_taskbar() { super::super::Rgba([255, 255, 255, 255]) } else { super::super::Rgba([0, 0, 0, 255]) };
            let icon = icon_of(&super::icon_pixels(info.icon, px, color), px);
            let mut d = data(hwnd, TRAY);
            d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
            d.uCallbackMessage = MESSAGE;
            d.hIcon = icon.unwrap_or_default();
            wide(&info.title, &mut d.szTip);
            if shown[TRAY as usize] {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
            } else {
                let _ = Shell_NotifyIconW(NIM_ADD, &d);
                d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
                shown[TRAY as usize] = true;
            }
            let old = ICON.replace(d.hIcon.0 as isize);
            if old != 0 {
                let _ = DestroyIcon(HICON(old as *mut _));
            }
            ITEMS.with(|i| *i.borrow_mut() = info.items.clone());
        }
        SHOWN.set(shown);
    }

    pub fn notify(hwnd: Option<HWND>, app: &str, title: &str, message: &str) {
        let Some(hwnd) = hwnd else { return };
        hook(hwnd);
        let mut shown = SHOWN.get();
        unsafe {
            // From the tray icon, or else a hidden one with the app's icon.
            let id = if shown[TRAY as usize] { TRAY } else { NOTICE };
            if !shown[id as usize] {
                let mut d = data(hwnd, NOTICE);
                d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_STATE | NIF_TIP;
                d.uCallbackMessage = MESSAGE;
                let module = GetModuleHandleW(None).ok();
                // (The app's icon is its first icon resource, if it has one.)
                d.hIcon = module
                    .and_then(|m| LoadIconW(Some(m.into()), PCWSTR(1 as *const u16)).ok())
                    .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                    .unwrap_or_default();
                d.dwState = NIS_HIDDEN;
                d.dwStateMask = NIS_HIDDEN;
                wide(app, &mut d.szTip);
                let _ = Shell_NotifyIconW(NIM_ADD, &d);
                d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
                shown[NOTICE as usize] = true;
            }
            let mut d = data(hwnd, id);
            d.uFlags = NIF_INFO;
            wide(title, &mut d.szInfoTitle);
            wide(message, &mut d.szInfo);
            d.dwInfoFlags = NIIF_INFO;
            let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
        }
        SHOWN.set(shown);
    }

    /// Takes the icons away (at exit: otherwise they stay until the mouse goes over them).
    pub fn shutdown() {
        let hwnd = HWND(HWND_IN.get() as *mut _);
        let shown = SHOWN.get();
        for id in [TRAY, NOTICE] {
            if shown[id as usize] {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd, id));
                }
            }
        }
        SHOWN.set([false; 3]);
    }
}

#[cfg(all(windows, feature = "native"))]
fn hwnd_of(window: Option<&Window>) -> Option<windows::Win32::Foundation::HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window?.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// Shows the tray icon (with its menu), or takes it away.
pub fn set(window: Option<&Window>, info: Option<&TrayInfo>) {
    let _ = (window, info);
    #[cfg(target_os = "macos")]
    mac::set(info);
    #[cfg(all(windows, feature = "native"))]
    win::set(hwnd_of(window), info);
}

/// Shows a notification from the system.
pub fn notify(window: Option<&Window>, app: &str, title: &str, message: &str) {
    let _ = (window, app, title, message);
    #[cfg(target_os = "macos")]
    mac::notify(title, message);
    #[cfg(all(windows, feature = "native"))]
    win::notify(hwnd_of(window), app, title, message);
}

/// Takes the tray icon away, as the app ends.
pub fn shutdown() {
    #[cfg(target_os = "macos")]
    mac::shutdown();
    #[cfg(all(windows, feature = "native"))]
    win::shutdown();
}
