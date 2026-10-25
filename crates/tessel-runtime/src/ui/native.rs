//! macOS: the menu bar, and the messages of the system winit leaves out —
//! files to open (Finder's Open With, a document double-clicked, one dropped
//! on the Dock icon, and the one the app was launched to open) and Quit
//! asked for from outside the app's own menu (the Dock, logging out).
//!
//! winit's application delegate (the object the system sends these to) is
//! given the methods for them: `application:openURLs:`,
//! `applicationShouldTerminate:`, and `tesselMenuChosen:`, the action of the
//! menu items. Menu items have no target, so their action goes along the
//! responder chain, which ends at that delegate. What arrives is handed to
//! the app as `Shell` events.

use std::cell::RefCell;
use std::ffi::{c_char, CStr};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::NSString;

use super::app::{shell_post, Shell, ASKS_TO_CLOSE};
use super::menu::{Entry, Shortcut, Std};

thread_local! {
    /// The path (indices, menu by menu) of each item of the menu bar, by tag.
    static PATHS: RefCell<Vec<Vec<usize>>> = const { RefCell::new(Vec::new()) };
    /// The same, for the items of the tray icon's menu.
    static TRAY_PATHS: RefCell<Vec<Vec<usize>>> = const { RefCell::new(Vec::new()) };
}

extern "C-unwind" fn tray_chosen(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    if sender.is_null() {
        return;
    }
    let tag: isize = unsafe { msg_send![sender, tag] };
    if let Some(path) = TRAY_PATHS.with(|p| p.borrow().get(tag as usize).cloned()) {
        shell_post(Shell::Tray(path));
    }
}

/// The menu of the tray icon: `items`, whose choices come as `Shell::Tray`.
pub fn tray_menu(items: &[Entry]) -> Retained<AnyObject> {
    let mut paths = Vec::new();
    let menu = unsafe { new_menu("") };
    unsafe { add_items(&menu, items, &mut Vec::new(), &mut paths, sel!(tesselTrayChosen:)) };
    TRAY_PATHS.with(|p| *p.borrow_mut() = paths);
    menu
}

extern "C-unwind" fn open_urls(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject, urls: *mut AnyObject) {
    if urls.is_null() {
        return;
    }
    let count: usize = unsafe { msg_send![urls, count] };
    for i in 0..count {
        let url: *mut AnyObject = unsafe { msg_send![urls, objectAtIndex: i] };
        if url.is_null() {
            continue;
        }
        let is_file: bool = unsafe { msg_send![url, isFileURL] };
        let path: *mut AnyObject = if is_file { unsafe { msg_send![url, path] } } else { unsafe { msg_send![url, absoluteString] } };
        if path.is_null() {
            continue;
        }
        let text: *const c_char = unsafe { msg_send![path, UTF8String] };
        if !text.is_null() {
            let path = unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned();
            shell_post(Shell::OpenFile(path));
        }
    }
}

/// `NSTerminateNow` lets the app end; `NSTerminateCancel` keeps it running.
const TERMINATE_NOW: usize = 1;
const TERMINATE_CANCEL: usize = 0;

extern "C-unwind" fn should_terminate(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject) -> usize {
    // With an `.onCloseRequest`, the app decides (and ends itself, if it
    // does: `quit()`); without one, it ends as it always would.
    if ASKS_TO_CLOSE.load(std::sync::atomic::Ordering::Relaxed) {
        shell_post(Shell::Quit);
        TERMINATE_CANCEL
    } else {
        TERMINATE_NOW
    }
}

extern "C-unwind" fn menu_chosen(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject) {
    if sender.is_null() {
        return;
    }
    let tag: isize = unsafe { msg_send![sender, tag] };
    if let Some(path) = PATHS.with(|p| p.borrow().get(tag as usize).cloned()) {
        shell_post(Shell::Menu(path));
    }
}

/// Gives winit's application delegate the methods above. Call it after the
/// event loop is made (which makes the delegate's class) and before it runs.
pub fn install() {
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else { return };
    let class = class as *const AnyClass as *mut AnyClass;
    unsafe {
        type Open = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);
        type Terminate = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize;
        type Chosen = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);
        let open = std::mem::transmute::<Open, Imp>(open_urls);
        let terminate = std::mem::transmute::<Terminate, Imp>(should_terminate);
        let chosen = std::mem::transmute::<Chosen, Imp>(menu_chosen);
        objc2::ffi::class_addMethod(class, sel!(application:openURLs:), open, c"v@:@@".as_ptr());
        objc2::ffi::class_addMethod(class, sel!(applicationShouldTerminate:), terminate, c"Q@:@".as_ptr());
        objc2::ffi::class_addMethod(class, sel!(tesselMenuChosen:), chosen, c"v@:@".as_ptr());
        let tray = std::mem::transmute::<Chosen, Imp>(tray_chosen);
        objc2::ffi::class_addMethod(class, sel!(tesselTrayChosen:), tray, c"v@:@".as_ptr());
        // The app's arguments aren't files for AppKit to open: the app gives
        // those to `.onOpenFile` itself (as on the other systems).
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let value = NSString::from_str("NO");
        let key = NSString::from_str("NSTreatUnknownArgumentsAsOpen");
        let dict: *mut AnyObject = msg_send![class!(NSDictionary), dictionaryWithObject: &*value, forKey: &*key];
        if !defaults.is_null() && !dict.is_null() {
            let _: () = msg_send![defaults, registerDefaults: dict];
        }
    }
}

// NSEventModifierFlags.
const SHIFT: usize = 1 << 17;
const OPTION: usize = 1 << 19;
const COMMAND: usize = 1 << 20;

/// What a menu item's key equivalent is, as AppKit has it.
fn key_equivalent(s: &Shortcut) -> (String, usize) {
    let function = |code: u32| char::from_u32(code).map(String::from).unwrap_or_default();
    let key = match s.key.as_str() {
        "enter" => "\r".to_string(),
        "escape" => "\u{1b}".to_string(),
        "tab" => "\t".to_string(),
        "backspace" => "\u{8}".to_string(),
        "delete" => function(0xF728),
        "up" => function(0xF700),
        "down" => function(0xF701),
        "left" => function(0xF702),
        "right" => function(0xF703),
        "home" => function(0xF729),
        "end" => function(0xF72B),
        "space" => " ".to_string(),
        k => k.to_string(),
    };
    let mut mask = 0;
    for (on, flag) in [(s.cmd, COMMAND), (s.shift, SHIFT), (s.alt, OPTION)] {
        if on {
            mask |= flag;
        }
    }
    (key, mask)
}

unsafe fn new_menu(title: &str) -> Retained<AnyObject> {
    let title = NSString::from_str(title);
    unsafe {
        let alloc: Allocated<AnyObject> = msg_send![class!(NSMenu), alloc];
        let menu: Retained<AnyObject> = msg_send![alloc, initWithTitle: &*title];
        // Items are enabled as the app says, not by AppKit's own rules.
        let _: () = msg_send![&*menu, setAutoenablesItems: false];
        menu
    }
}

/// Adds `items` to `menu`: the app's own go to `chosen` (with their tags,
/// places in `paths`).
unsafe fn add_items(menu: &AnyObject, items: &[Entry], path: &mut Vec<usize>, paths: &mut Vec<Vec<usize>>, chosen: Sel) {
    for (i, e) in items.iter().enumerate() {
        path.push(i);
        unsafe {
            match e {
                Entry::Divider => {
                    let item: *mut AnyObject = msg_send![class!(NSMenuItem), separatorItem];
                    let _: () = msg_send![menu, addItem: item];
                }
                Entry::Item { label, shortcut, enabled, checked, command } => {
                    // The system's own commands go to the system.
                    let action = match command {
                        super::menu::Command::Std(Std::Hide) => sel!(hide:),
                        super::menu::Command::Std(Std::HideOthers) => sel!(hideOtherApplications:),
                        super::menu::Command::Std(Std::ShowAll) => sel!(unhideAllApplications:),
                        super::menu::Command::Std(Std::Minimize) => sel!(performMiniaturize:),
                        super::menu::Command::Std(Std::Zoom) => sel!(performZoom:),
                        _ => chosen,
                    };
                    let (key, mask) = shortcut.as_ref().map_or((String::new(), 0), key_equivalent);
                    let (title, key) = (NSString::from_str(label), NSString::from_str(&key));
                    let alloc: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
                    let item: Retained<AnyObject> = msg_send![alloc, initWithTitle: &*title, action: action, keyEquivalent: &*key];
                    let _: () = msg_send![&*item, setKeyEquivalentModifierMask: mask];
                    let _: () = msg_send![&*item, setEnabled: *enabled];
                    let _: () = msg_send![&*item, setState: if *checked { 1isize } else { 0isize }];
                    let _: () = msg_send![&*item, setTag: paths.len() as isize];
                    paths.push(path.clone());
                    let _: () = msg_send![menu, addItem: &*item];
                }
                Entry::Sub { title, enabled, items } => {
                    let sub = new_menu(title);
                    add_items(&sub, items, path, paths, chosen);
                    let name = NSString::from_str(title);
                    let empty = NSString::from_str("");
                    let alloc: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
                    let item: Retained<AnyObject> =
                        msg_send![alloc, initWithTitle: &*name, action: Option::<Sel>::None, keyEquivalent: &*empty];
                    let _: () = msg_send![&*item, setSubmenu: &*sub];
                    let _: () = msg_send![&*item, setEnabled: *enabled];
                    let _: () = msg_send![menu, addItem: &*item];
                }
            }
        }
        path.pop();
    }
}

/// Makes `menus` (each an `Entry::Sub`; the first is the app menu) the
/// menu bar.
pub fn set_menu_bar(menus: &[Entry]) {
    let mut paths = Vec::new();
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let bar = new_menu("");
        let mut path = Vec::new();
        add_items(&bar, menus, &mut path, &mut paths, sel!(tesselMenuChosen:));
        let _: () = msg_send![app, setMainMenu: &*bar];
        // The Window menu lists the app's windows.
        let index = menus.iter().position(|m| matches!(m, Entry::Sub { title, .. } if title == "Window"));
        if let Some(i) = index {
            let item: *mut AnyObject = msg_send![&*bar, itemAtIndex: i as isize];
            let window_menu: *mut AnyObject = msg_send![item, submenu];
            if !window_menu.is_null() {
                let _: () = msg_send![app, setWindowsMenu: window_menu];
            }
        }
    }
    PATHS.with(|p| *p.borrow_mut() = paths);
}

/// Builds the standard menu bar with AppKit, without running the app or
/// showing anything, so AppKit checks the messages it's sent (and objc2, in
/// debug builds, their types). For `tests/native_menu.rs`, on the main thread.
#[doc(hidden)]
pub fn check_menu_bar() {
    assert!(objc2::MainThreadMarker::new().is_some(), "AppKit needs the main thread");
    let bar = super::menu::bar(None, "Demo", true, &[]);
    set_menu_bar(&bar);
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let main: *mut AnyObject = msg_send![app, mainMenu];
        let count: isize = msg_send![main, numberOfItems];
        assert_eq!(count, 3, "Demo, Edit, Window");
        let edit: *mut AnyObject = msg_send![main, itemAtIndex: 1isize];
        let menu: *mut AnyObject = msg_send![edit, submenu];
        let copy: *mut AnyObject = msg_send![menu, itemAtIndex: 4isize];
        let title: *mut AnyObject = msg_send![copy, title];
        let text: *const c_char = msg_send![title, UTF8String];
        assert_eq!(CStr::from_ptr(text).to_str().unwrap(), "Copy");
        let mask: usize = msg_send![copy, keyEquivalentModifierMask];
        assert_eq!(mask, COMMAND);
        let enabled: bool = msg_send![copy, isEnabled];
        assert!(enabled);
    }
    // Every item but the dividers and submenus has a path.
    assert_eq!(PATHS.with(|p| p.borrow().len()), 5 + 6 + 3);

    // winit's application delegate (made with the event loop, which isn't
    // run) gets the messages, and hands them to the app.
    let _event_loop = winit::event_loop::EventLoop::new().expect("an event loop");
    install();
    let taken = || std::mem::take(&mut *super::app::SHELL.lock().unwrap());
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![app, delegate];
        for sel in [sel!(application:openURLs:), sel!(applicationShouldTerminate:), sel!(tesselMenuChosen:)] {
            let responds: bool = msg_send![delegate, respondsToSelector: sel];
            assert!(responds, "{sel:?}");
        }
        // A file to open.
        let path = NSString::from_str("/tmp/roads.gpkg");
        let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
        let urls: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: url];
        let _: () = msg_send![delegate, application: app, openURLs: urls];
        assert!(matches!(taken().as_slice(), [super::app::Shell::OpenFile(p)] if p == "/tmp/roads.gpkg"));
        // Quitting: the app decides when it asks (and ends itself).
        ASKS_TO_CLOSE.store(false, std::sync::atomic::Ordering::Relaxed);
        let answer: usize = msg_send![delegate, applicationShouldTerminate: app];
        assert_eq!(answer, TERMINATE_NOW);
        ASKS_TO_CLOSE.store(true, std::sync::atomic::Ordering::Relaxed);
        let answer: usize = msg_send![delegate, applicationShouldTerminate: app];
        assert_eq!(answer, TERMINATE_CANCEL);
        assert!(matches!(taken().as_slice(), [super::app::Shell::Quit]));
        ASKS_TO_CLOSE.store(false, std::sync::atomic::Ordering::Relaxed);
        // A menu item chosen (Copy, in the Edit menu): its path.
        let main: *mut AnyObject = msg_send![app, mainMenu];
        let edit: *mut AnyObject = msg_send![main, itemAtIndex: 1isize];
        let menu: *mut AnyObject = msg_send![edit, submenu];
        let copy: *mut AnyObject = msg_send![menu, itemAtIndex: 4isize];
        let _: () = msg_send![delegate, tesselMenuChosen: copy];
        assert!(matches!(taken().as_slice(), [super::app::Shell::Menu(p)] if p == &[1, 4]));
    }
}

/// The About panel, with the app's name, version and copyright from its
/// `tessel.toml` (see `appInfo`): an app run without a bundle has no
/// Info.plist for the panel to read them from.
pub fn show_about() {
    let info = crate::stdlib::app_info();
    let field = |i: usize| info.get(i).cloned().unwrap_or_default();
    let (name, version, build, copyright) = (field(0), field(2), field(3), field(6));
    unsafe {
        let options: Retained<AnyObject> = msg_send![class!(NSMutableDictionary), new];
        let set = |key: &str, value: &str| {
            if !value.is_empty() {
                let (k, v) = (NSString::from_str(key), NSString::from_str(value));
                let _: () = msg_send![&*options, setObject: &*v, forKey: &*k];
            }
        };
        set("ApplicationName", &name);
        set("ApplicationVersion", &version);
        // (The build number shows in parentheses when it isn't the version.)
        set("Version", if build != version { &build } else { "" });
        set("Copyright", &copyright);
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions: &*options];
    }
}
