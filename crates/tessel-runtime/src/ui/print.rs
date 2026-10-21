//! Printing (`printDocument`, see the prelude): the pages are drawn into a
//! PDF file as `writePDF` does, and the file is handed to the system to
//! print. On macOS that's the system's print panel, for the document; on
//! Windows, the app that opens PDF files is asked to print it (or to open
//! it, if it can't be asked that).

use crate::Str;

/// `__printFile(path, title)`: prints the PDF at `path`; whether it was
/// printed (or handed over to be). The file is a temporary one: it's deleted
/// when it isn't needed any more.
///
/// In a scripted run nothing is printed: the request is, and `answer
/// cancel` makes it end as a cancelled dialog does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_print_file(path: *const Str, title: *const Str) -> u8 {
    let (path, title) = unsafe { (crate::str_text(path).to_string(), crate::str_text(title).to_string()) };
    if std::env::var_os("TESSEL_SCRIPT").is_some() {
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        println!("Print {title:?}{}", if size > 0 { "" } else { " (nothing to print)" });
        let _ = std::fs::remove_file(&path);
        return (size > 0 && super::app::scripted_answer().is_none_or(|a| a.is_some())) as u8;
    }
    let printed = system::print(&path, &title);
    if system::DELETE_AFTER {
        let _ = std::fs::remove_file(&path);
    }
    printed as u8
}

#[cfg(target_os = "macos")]
mod system {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    /// (The print job has the document itself by then.)
    pub const DELETE_AFTER: bool = true;

    /// The document's pages, sized to fit the paper.
    const SCALE_TO_FIT: isize = 1;

    /// PDFKit's `PDFDocument`, loaded when first asked for: apps that never
    /// print don't load PDFKit.
    fn pdf_document() -> Option<&'static AnyClass> {
        if let Some(c) = AnyClass::get(c"PDFDocument") {
            return Some(c);
        }
        for path in [c"/System/Library/Frameworks/PDFKit.framework/PDFKit", c"/System/Library/Frameworks/Quartz.framework/Quartz"] {
            unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
            if let Some(c) = AnyClass::get(c"PDFDocument") {
                return Some(c);
            }
        }
        None
    }

    /// The print operation for the PDF at `path` (not run yet).
    pub fn operation(path: &str, title: &str) -> Option<Retained<AnyObject>> {
        let class = pdf_document()?;
        unsafe {
            let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), fileURLWithPath: &*NSString::from_str(path)];
            let url = url?;
            let alloc: Allocated<AnyObject> = msg_send![class, alloc];
            let document: Option<Retained<AnyObject>> = msg_send![alloc, initWithURL: &*url];
            let document = document?;
            let info: Option<Retained<AnyObject>> = msg_send![class!(NSPrintInfo), sharedPrintInfo];
            let info = info?;
            let operation: Option<Retained<AnyObject>> =
                msg_send![&*document, printOperationForPrintInfo: &*info, scalingMode: SCALE_TO_FIT, autoRotate: true];
            let operation = operation?;
            if !title.is_empty() {
                let _: () = msg_send![&*operation, setJobTitle: &*NSString::from_str(title)];
            }
            Some(operation)
        }
    }

    pub fn print(path: &str, title: &str) -> bool {
        let Some(operation) = operation(path, title) else { return false };
        // The print panel; the program waits for it.
        unsafe { msg_send![&*operation, runOperation] }
    }
}

#[cfg(windows)]
mod system {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    /// (The PDF app reads the file after this returns.)
    pub const DELETE_AFTER: bool = false;

    /// No console window for the helper.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn print(path: &str, _title: &str) -> bool {
        // The app that opens PDF files prints it (its "Print" action)...
        let script = "Start-Process -FilePath $args[0] -Verb Print";
        let asked = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script, path])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success());
        if asked {
            return true;
        }
        // ...or opens it, to be printed from there.
        Command::new("cmd").args(["/C", "start", "", path]).creation_flags(CREATE_NO_WINDOW).status().is_ok_and(|s| s.success())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod system {
    pub const DELETE_AFTER: bool = false;

    pub fn print(path: &str, _title: &str) -> bool {
        std::process::Command::new("lp").arg(path).status().is_ok_and(|s| s.success())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// A print operation can be made for a PDF Tessel wrote (it isn't run:
    /// that would open the print panel).
    #[test]
    fn makes_a_print_operation() {
        let path = std::env::temp_dir().join(format!("tessel-print-test-{}.pdf", std::process::id()));
        let page = crate::ui::canvas::Page { list: crate::ui::draw::DrawList::new(), width: 595.0, height: 842.0 };
        assert!(crate::ui::backend::write_pdf(path.to_str().unwrap(), "Test", &[page]));
        let operation = super::system::operation(path.to_str().unwrap(), "Test");
        let _ = std::fs::remove_file(&path);
        assert!(operation.is_some());
    }
}
