//! Dragging a file out of the app (`.dragFile(path)`): onto the desktop, a
//! folder, or another app, which gets the file as if it were dragged from
//! Finder (macOS) or Explorer (Windows) or a Linux file manager (on X11;
//! an app that drags files out runs on X11, through XWayland on a Wayland
//! desktop). Elsewhere, nothing happens.

use winit::window::Window;

/// Starts dragging the file at `path` out of `window`, from where the mouse
/// is (its button held down). On Windows this returns once it's dropped.
pub fn drag_file(window: &Window, path: &str) {
    let _ = (window, path);
    #[cfg(target_os = "macos")]
    mac::drag_file(window, path);
    #[cfg(all(windows, feature = "native"))]
    win::drag_file(window, path);
    #[cfg(target_os = "linux")]
    x11::drag_file(path);
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
    use objc2::{class, msg_send, sel};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    /// `draggingSession:sourceOperationMaskForDraggingContext:`: the file is copied.
    extern "C-unwind" fn operations(_this: *mut AnyObject, _cmd: Sel, _session: *mut AnyObject, _context: isize) -> usize {
        // NSDragOperationCopy.
        1
    }

    /// The class of what a drag comes from (a dragging source), made once.
    fn source_class() -> Option<&'static AnyClass> {
        if let Some(c) = AnyClass::get(c"TesselFileDragSource") {
            return Some(c);
        }
        let mut builder = ClassBuilder::new(c"TesselFileDragSource", class!(NSObject))?;
        type Mask = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, isize) -> usize;
        unsafe { builder.add_method(sel!(draggingSession:sourceOperationMaskForDraggingContext:), operations as Mask) };
        if let Some(p) = objc2::runtime::AnyProtocol::get(c"NSDraggingSource") {
            builder.add_protocol(p);
        }
        Some(builder.register())
    }

    pub fn drag_file(window: &Window, path: &str) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        let view = h.ns_view.as_ptr() as *mut AnyObject;
        let Some(class) = source_class() else { return };
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let event: *mut AnyObject = msg_send![app, currentEvent];
            if event.is_null() {
                return;
            }
            let path = NSString::from_str(path);
            let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
            let item: *mut AnyObject = msg_send![class!(NSDraggingItem), alloc];
            let item: *mut AnyObject = msg_send![item, initWithPasteboardWriter: url];
            // What's dragged looks like the file, as Finder shows it.
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let icon: *mut AnyObject = msg_send![workspace, iconForFile: &*path];
            let at: NSPoint = msg_send![event, locationInWindow];
            let nil: *mut AnyObject = std::ptr::null_mut();
            let at: NSPoint = msg_send![view, convertPoint: at, fromView: nil];
            let size = 48.0;
            let frame = NSRect::new(NSPoint::new(at.x - size / 2.0, at.y - size / 2.0), NSSize::new(size, size));
            let _: () = msg_send![item, setDraggingFrame: frame, contents: icon];
            let items: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: item];
            let source: *mut AnyObject = msg_send![class, new];
            let _: *mut AnyObject = msg_send![view, beginDraggingSessionWithItems: items, event: event, source: source];
            let _: () = msg_send![item, release];
            // (The session keeps its source while it runs.)
            let _: () = msg_send![source, autorelease];
        }
    }
}

#[cfg(all(windows, feature = "native"))]
mod win {
    use windows::core::{implement, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK};
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY};
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName};
    use winit::window::Window;

    /// Says how the drag goes on: it's dropped when the mouse button is let
    /// go, and given up with Escape.
    #[implement(IDropSource)]
    struct Source;

    impl IDropSource_Impl for Source_Impl {
        fn QueryContinueDrag(&self, escape: windows::core::BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape.as_bool() {
                DRAGDROP_S_CANCEL
            } else if keys.0 & MK_LBUTTON.0 == 0 {
                DRAGDROP_S_DROP
            } else {
                S_OK
            }
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    pub fn drag_file(_window: &Window, path: &str) {
        let wide: Vec<u16> = path.replace('/', "\\").encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            // The file as Explorer has it, with all it says of it when it's dragged.
            let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide.as_ptr()), None::<&windows::Win32::System::Com::IBindCtx>) else { return };
            let Ok(data) = item.BindToHandler::<_, IDataObject>(None, &BHID_DataObject) else { return };
            let source: IDropSource = Source.into();
            let mut effect = DROPEFFECT(0);
            let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
        }
    }
}

/// The X11 drag-and-drop protocol (XDND, version 5) from the dragging
/// side, over a connection of its own: the file is offered as a
/// `text/uri-list`. The app's window has the mouse while its button is
/// down, so where the mouse is is asked, until the button is let go.
/// Returns once it's dropped (or the drop is refused).
#[cfg(target_os = "linux")]
mod x11 {
    use std::time::{Duration, Instant};

    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, KeyButMask, PropMode, SelectionNotifyEvent,
        WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    struct Atoms {
        aware: u32,
        selection: u32,
        enter: u32,
        position: u32,
        status: u32,
        leave: u32,
        drop: u32,
        finished: u32,
        copy: u32,
        uri_list: u32,
        targets: u32,
    }

    pub fn drag_file(path: &str) {
        let _ = drag(path);
    }

    fn drag(path: &str) -> Option<()> {
        let full = std::fs::canonicalize(path).ok()?;
        let uri = format!("file://{}\r\n", percent(&full.to_string_lossy()));
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;
        let atom = |name: &str| conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        let a = Atoms {
            aware: atom("XdndAware")?,
            selection: atom("XdndSelection")?,
            enter: atom("XdndEnter")?,
            position: atom("XdndPosition")?,
            status: atom("XdndStatus")?,
            leave: atom("XdndLeave")?,
            drop: atom("XdndDrop")?,
            finished: atom("XdndFinished")?,
            copy: atom("XdndActionCopy")?,
            uri_list: atom("text/uri-list")?,
            targets: atom("TARGETS")?,
        };
        // A window of its own, unseen: it owns what's dragged, and the
        // target's answers come to it.
        let me = conn.generate_id().ok()?;
        conn.create_window(0, me, root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).ok()?;
        conn.set_selection_owner(me, a.selection, x11rb::CURRENT_TIME).ok()?;
        conn.flush().ok()?;

        let send = |to: u32, kind: u32, data: [u32; 5]| {
            let event = ClientMessageEvent::new(32, to, kind, data);
            let _ = conn.send_event(false, to, EventMask::NO_EVENT, event);
            let _ = conn.flush();
        };
        let mut target: Option<u32> = None;
        let mut accepted = false;
        let mut last = (i16::MIN, i16::MIN);
        let started = Instant::now();
        loop {
            answer(&conn, &a, me, &uri, &mut accepted, &mut None);
            let pointer = conn.query_pointer(root).ok()?.reply().ok()?;
            let down = pointer.mask.contains(KeyButMask::BUTTON1);
            let under = aware_window(&conn, root, a.aware);
            if under != target {
                if let Some(t) = target {
                    send(t, a.leave, [me, 0, 0, 0, 0]);
                }
                if let Some(t) = under {
                    // Version 5, three types at most: ours is the one.
                    send(t, a.enter, [me, 5 << 24, a.uri_list, 0, 0]);
                }
                target = under;
                accepted = false;
                last = (i16::MIN, i16::MIN);
            }
            if let Some(t) = target {
                if (pointer.root_x, pointer.root_y) != last {
                    last = (pointer.root_x, pointer.root_y);
                    let at = ((pointer.root_x as u32) << 16) | (pointer.root_y as u16 as u32);
                    send(t, a.position, [me, 0, at, x11rb::CURRENT_TIME, a.copy]);
                }
            }
            if !down {
                break;
            }
            // (A drag that's somehow never let go ends after a while.)
            if started.elapsed() > Duration::from_secs(600) {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        let Some(t) = target else { return Some(()) };
        // A last look at the answer to the last position.
        let wait = Instant::now();
        while !accepted && wait.elapsed() < Duration::from_millis(200) {
            answer(&conn, &a, me, &uri, &mut accepted, &mut None);
            std::thread::sleep(Duration::from_millis(10));
        }
        if !accepted {
            send(t, a.leave, [me, 0, 0, 0, 0]);
            return Some(());
        }
        send(t, a.drop, [me, 0, x11rb::CURRENT_TIME, 0, 0]);
        // The target asks for the file's address, then says it's done.
        let mut finished = None;
        let wait = Instant::now();
        while finished.is_none() && wait.elapsed() < Duration::from_secs(5) {
            answer(&conn, &a, me, &uri, &mut accepted, &mut finished);
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = conn.destroy_window(me);
        let _ = conn.flush();
        Some(())
    }

    /// Handles what came to the window: the target's status (whether it
    /// takes the drop), its asking for what's dragged, and its finishing.
    fn answer(conn: &impl Connection, a: &Atoms, me: u32, uri: &str, accepted: &mut bool, finished: &mut Option<bool>) {
        while let Ok(Some(event)) = conn.poll_for_event() {
            match event {
                Event::ClientMessage(m) if m.type_ == a.status => {
                    *accepted = m.data.as_data32()[1] & 1 == 1;
                }
                Event::ClientMessage(m) if m.type_ == a.finished => *finished = Some(true),
                Event::SelectionRequest(r) if r.selection == a.selection => {
                    let property = if r.property == 0 { r.target } else { r.property };
                    let given = if r.target == a.uri_list {
                        conn.change_property8(PropMode::REPLACE, r.requestor, property, a.uri_list, uri.as_bytes()).is_ok()
                    } else if r.target == a.targets {
                        conn.change_property32(PropMode::REPLACE, r.requestor, property, AtomEnum::ATOM, &[a.targets, a.uri_list]).is_ok()
                    } else {
                        false
                    };
                    let reply = SelectionNotifyEvent {
                        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: r.time,
                        requestor: r.requestor,
                        selection: r.selection,
                        target: r.target,
                        property: if given { property } else { 0 },
                    };
                    let _ = conn.send_event(false, r.requestor, EventMask::NO_EVENT, reply);
                    let _ = conn.flush();
                }
                _ => {}
            }
        }
        let _ = me;
    }

    /// The window under the mouse that takes drops (has `XdndAware`): the
    /// top-level one, found going down from the root.
    fn aware_window(conn: &impl Connection, root: u32, aware: u32) -> Option<u32> {
        let mut at = root;
        for _ in 0..16 {
            let child = conn.query_pointer(at).ok()?.reply().ok()?.child;
            if child == 0 {
                return None;
            }
            let has = conn.get_property(false, child, aware, AtomEnum::ATOM, 0, 1).ok()?.reply().ok()?;
            if has.value_len > 0 {
                return Some(child);
            }
            at = child;
        }
        None
    }

    /// A path as a URI's: what isn't plain written as `%XX`.
    fn percent(path: &str) -> String {
        let mut out = String::new();
        for b in path.bytes() {
            if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }
}
