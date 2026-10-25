//! Dragging a file out of the app (`.dragFile(path)`): onto the desktop, a
//! folder, or another app, which gets the file as if it were dragged from
//! Finder (macOS) or Explorer (Windows). Elsewhere, nothing happens.

use winit::window::Window;

/// Starts dragging the file at `path` out of `window`, from where the mouse
/// is (its button held down). On Windows this returns once it's dropped.
pub fn drag_file(window: &Window, path: &str) {
    let _ = (window, path);
    #[cfg(target_os = "macos")]
    mac::drag_file(window, path);
    #[cfg(all(windows, feature = "native"))]
    win::drag_file(window, path);
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
