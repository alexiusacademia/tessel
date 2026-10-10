//! Running as an Android app.
//!
//! An Android app is a shared library that the system's `NativeActivity`
//! loads; winit's activity glue starts a thread in it and calls
//! `android_main` there. That's where a Tessel program starts: the
//! compiler gives an Android program no `main`, only its `tsl_main`.

use std::sync::OnceLock;

use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

unsafe extern "C" {
    /// The program (see the compiler's `Gen::program`).
    fn tsl_main();
    fn __android_log_write(priority: i32, tag: *const libc::c_char, text: *const libc::c_char) -> i32;
}

/// The activity the app runs in, for the event loop.
pub fn app() -> AndroidApp {
    APP.get().expect("the app has started").clone()
}

/// What the program prints goes to the system's log (`adb logcat -s tessel`):
/// an app has no terminal.
fn print_to_log() {
    let mut ends = [0i32; 2];
    unsafe {
        if libc::pipe(ends.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(ends[1], 1);
        libc::dup2(ends[1], 2);
        libc::close(ends[1]);
    }
    let read = ends[0];
    std::thread::spawn(move || {
        use std::io::BufRead;
        use std::os::fd::FromRawFd;
        let file = unsafe { std::fs::File::from_raw_fd(read) };
        for line in std::io::BufReader::new(file).split(b'\n').map_while(Result::ok) {
            let Ok(text) = std::ffi::CString::new(line) else { continue };
            // (4: ANDROID_LOG_INFO.)
            unsafe { __android_log_write(4, c"tessel".as_ptr(), text.as_ptr()) };
        }
    });
}

/// Calls into the activity, which is a Java object: `f` gets the Java
/// environment of this thread and the activity. Objects it makes are let go
/// of when it's done. None if Java can't be reached, or `f` gives none.
fn with_activity<T>(f: impl FnOnce(*mut jni_sys::JNIEnv, jni_sys::jobject) -> Option<T>) -> Option<T> {
    let app = APP.get()?;
    let vm = app.vm_as_ptr() as *mut jni_sys::JavaVM;
    unsafe {
        let mut env: *mut jni_sys::JNIEnv = std::ptr::null_mut();
        let attach = (**vm).AttachCurrentThread?;
        if attach(vm, &mut env as *mut _ as *mut *mut std::ffi::c_void, std::ptr::null_mut()) != 0 || env.is_null() {
            return None;
        }
        if ((**env).PushLocalFrame?)(env, 16) != 0 {
            return None;
        }
        let result = f(env, app.activity_as_ptr() as jni_sys::jobject);
        // (A call that failed mustn't fail the calls after it.)
        if ((**env).ExceptionCheck?)(env) != 0 {
            ((**env).ExceptionClear?)(env);
        }
        ((**env).PopLocalFrame?)(env, std::ptr::null_mut());
        result
    }
}

/// `object.name()` for a method that takes nothing and gives an object
/// (`signature` is Java's, like `()Landroid/view/Window;`).
unsafe fn object_of(env: *mut jni_sys::JNIEnv, object: jni_sys::jobject, name: &std::ffi::CStr, signature: &std::ffi::CStr) -> Option<jni_sys::jobject> {
    unsafe {
        let class = ((**env).GetObjectClass?)(env, object);
        let method = ((**env).GetMethodID?)(env, class, name.as_ptr(), signature.as_ptr());
        if method.is_null() {
            return None;
        }
        let found = ((**env).CallObjectMethodA?)(env, object, method, std::ptr::null());
        (!found.is_null()).then_some(found)
    }
}

/// Whether the phone is in its dark theme. None if it can't be told.
pub fn dark() -> Option<bool> {
    with_activity(|env, activity| unsafe {
        let resources = object_of(env, activity, c"getResources", c"()Landroid/content/res/Resources;")?;
        let config = object_of(env, resources, c"getConfiguration", c"()Landroid/content/res/Configuration;")?;
        let class = ((**env).GetObjectClass?)(env, config);
        let field = ((**env).GetFieldID?)(env, class, c"uiMode".as_ptr(), c"I".as_ptr());
        if field.is_null() {
            return None;
        }
        // (`UI_MODE_NIGHT_MASK`, and `UI_MODE_NIGHT_YES`.)
        Some(((**env).GetIntField?)(env, config, field) & 0x30 == 0x20)
    })
}

/// Makes the icons of the system's bars dark (for a light app) or light.
/// (The package's theme says so too, but only as the app starts.)
pub fn bar_icons(dark_icons: bool) {
    with_activity(|env, activity| unsafe {
        let window = object_of(env, activity, c"getWindow", c"()Landroid/view/Window;")?;
        let controller = object_of(env, window, c"getInsetsController", c"()Landroid/view/WindowInsetsController;")?;
        let class = ((**env).GetObjectClass?)(env, controller);
        let method = ((**env).GetMethodID?)(env, class, c"setSystemBarsAppearance".as_ptr(), c"(II)V".as_ptr());
        if method.is_null() {
            return None;
        }
        // (`APPEARANCE_LIGHT_STATUS_BARS`, 8, and `…_NAVIGATION_BARS`, 16.)
        let light = 8 | 16;
        let args = [jni_sys::jvalue { i: if dark_icons { light } else { 0 } }, jni_sys::jvalue { i: light }];
        ((**env).CallVoidMethodA?)(env, controller, method, args.as_ptr());
        Some(())
    });
}

/// How far the system's bars (the status bar, the navigation bar), the
/// screen's cutout and the on-screen keyboard reach into the window, in
/// pixels: left, top, right, bottom. None while the window isn't on screen
/// yet.
pub fn insets() -> Option<[f64; 4]> {
    with_activity(|env, activity| unsafe {
        let window = object_of(env, activity, c"getWindow", c"()Landroid/view/Window;")?;
        let view = object_of(env, window, c"getDecorView", c"()Landroid/view/View;")?;
        let all = object_of(env, view, c"getRootWindowInsets", c"()Landroid/view/WindowInsets;")?;
        let class = ((**env).GetObjectClass?)(env, all);
        let method = ((**env).GetMethodID?)(env, class, c"getInsets".as_ptr(), c"(I)Landroid/graphics/Insets;".as_ptr());
        if method.is_null() {
            return None;
        }
        // (`WindowInsets.Type`: the system's bars, 7, the keyboard, 8, and
        // the cutout, 128.)
        let kinds = [jni_sys::jvalue { i: 7 | 8 | 128 }];
        let insets = ((**env).CallObjectMethodA?)(env, all, method, kinds.as_ptr());
        if insets.is_null() {
            return None;
        }
        let class = ((**env).GetObjectClass?)(env, insets);
        let mut sides = [0.0; 4];
        for (side, name) in sides.iter_mut().zip([c"left", c"top", c"right", c"bottom"]) {
            let field = ((**env).GetFieldID?)(env, class, name.as_ptr(), c"I".as_ptr());
            if field.is_null() {
                return None;
            }
            *side = ((**env).GetIntField?)(env, insets, field) as f64;
        }
        Some(sides)
    })
}

/// The Back button with nothing left to close: the app steps back, behind
/// what was open before it (it keeps running, as apps do on Android).
pub fn leave() {
    with_activity(|env, activity| unsafe {
        let class = ((**env).GetObjectClass?)(env, activity);
        let method = ((**env).GetMethodID?)(env, class, c"moveTaskToBack".as_ptr(), c"(Z)Z".as_ptr());
        if method.is_null() {
            return None;
        }
        let args = [jni_sys::jvalue { z: 1 }];
        ((**env).CallBooleanMethodA?)(env, activity, method, args.as_ptr());
        Some(())
    });
}

/// Whether the system has done away with the activity (the app was swiped
/// away, or the system wanted its memory). The app can't go on then: the
/// system waits for it to stop, and starts it anew the next time.
pub fn destroyed() -> bool {
    with_activity(|env, activity| unsafe {
        let class = ((**env).GetObjectClass?)(env, activity);
        let method = ((**env).GetMethodID?)(env, class, c"isDestroyed".as_ptr(), c"()Z".as_ptr());
        if method.is_null() {
            return None;
        }
        Some(((**env).CallBooleanMethodA?)(env, activity, method, std::ptr::null()) != 0)
    })
    .unwrap_or(false)
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    print_to_log();
    // Relative paths are in the app's own folder (the only one it may write).
    if let Some(folder) = app.internal_data_path() {
        let _ = std::env::set_current_dir(folder);
    }
    let _ = APP.set(app);
    unsafe { tsl_main() };
    // (A program without an `app`: its timers and background work.)
    super::run_until_idle();
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
