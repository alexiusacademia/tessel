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

// ===== the activity's own Java (`TesselActivity`, in the CLI's sources) =====

/// A Java string of `text`. (From UTF-16: Java's own "UTF-8" can't hold emoji.)
unsafe fn java_string(env: *mut jni_sys::JNIEnv, text: &str) -> jni_sys::jstring {
    let units: Vec<u16> = text.encode_utf16().collect();
    unsafe { ((**env).NewString.unwrap())(env, units.as_ptr(), units.len() as i32) }
}

unsafe fn rust_string(env: *mut jni_sys::JNIEnv, text: jni_sys::jstring) -> Option<String> {
    if text.is_null() {
        return None;
    }
    unsafe {
        let len = ((**env).GetStringLength?)(env, text) as usize;
        let units = ((**env).GetStringChars?)(env, text, std::ptr::null_mut());
        if units.is_null() {
            return None;
        }
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(units, len));
        ((**env).ReleaseStringChars?)(env, text, units);
        Some(out)
    }
}

/// The activity's method `name` (`signature` is Java's).
unsafe fn method(env: *mut jni_sys::JNIEnv, activity: jni_sys::jobject, name: &std::ffi::CStr, signature: &std::ffi::CStr) -> Option<jni_sys::jmethodID> {
    unsafe {
        let class = ((**env).GetObjectClass?)(env, activity);
        let found = ((**env).GetMethodID?)(env, class, name.as_ptr(), signature.as_ptr());
        (!found.is_null()).then_some(found)
    }
}

/// What a text field is for, to the keyboard.
#[derive(Clone, Copy, PartialEq)]
pub enum Typing {
    Text,
    /// A secret: nothing is suggested or learned.
    Secret,
    /// Code: no corrections.
    Code,
}

/// Shows the on-screen keyboard for a field, or hides it. `field` numbers
/// the field (the keyboard's changes come back with it, see `Input::Edit`).
/// `text` is what's in the field, with its selection, for the keyboard to
/// read and correct; with none it only types.
pub fn keyboard(show: Option<Typing>, field: i32, text: Option<&Text>) {
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"keyboard", c"(ZIILjava/lang/String;II)V")?;
        let kind = match show {
            Some(Typing::Secret) => 1,
            Some(Typing::Code) => 2,
            _ => 0,
        };
        let (start, end) = text.map_or((0, 0), |t| t.selection());
        let args = [
            jni_sys::jvalue { z: show.is_some() as u8 },
            jni_sys::jvalue { i: kind },
            jni_sys::jvalue { i: field },
            jni_sys::jvalue { l: text.map_or(std::ptr::null_mut(), |t| java_string(env, &t.text)) },
            jni_sys::jvalue { i: start },
            jni_sys::jvalue { i: end },
        ];
        ((**env).CallVoidMethodA?)(env, activity, method, args.as_ptr());
        Some(())
    });
}

/// A field's text and selection (in bytes, as the app counts).
#[derive(Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub anchor: usize,
    pub focus: usize,
}

impl Text {
    /// The selection as Java counts: in UTF-16 units, start first.
    fn selection(&self) -> (i32, i32) {
        let (a, b) = (units(&self.text, self.anchor), units(&self.text, self.focus));
        (a.min(b), a.max(b))
    }
}

/// How many of Java's characters (UTF-16 units) come before byte `at`.
fn units(text: &str, at: usize) -> i32 {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text[..at].encode_utf16().count() as i32
}

/// The byte where Java's character `unit` starts (the end, past the last).
pub fn byte_at(text: &str, unit: usize) -> usize {
    let mut seen = 0;
    for (at, c) in text.char_indices() {
        if seen >= unit {
            return at;
        }
        seen += c.len_utf16();
    }
    text.len()
}

/// Tells the keyboard what's in the field now, after its change `serial`
/// was taken in. (If it's what the keyboard has, nothing happens; if the
/// app changed the text or the caret moved, the keyboard starts from this.)
pub fn typed(text: &Text, serial: i32) {
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"text", c"(Ljava/lang/String;III)V")?;
        let (start, end) = text.selection();
        let args = [
            jni_sys::jvalue { l: java_string(env, &text.text) },
            jni_sys::jvalue { i: start },
            jni_sys::jvalue { i: end },
            jni_sys::jvalue { i: serial },
        ];
        ((**env).CallVoidMethodA?)(env, activity, method, args.as_ptr());
        Some(())
    });
}

/// A system dialog with a message (`cancel` none) or a question. Waits for
/// the answer: whether it was confirmed.
pub fn ask(title: &str, message: &str, ok: &str, cancel: Option<&str>) -> bool {
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"ask", c"(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z")?;
        let args = [
            jni_sys::jvalue { l: java_string(env, title) },
            jni_sys::jvalue { l: java_string(env, message) },
            jni_sys::jvalue { l: java_string(env, ok) },
            jni_sys::jvalue { l: cancel.map_or(std::ptr::null_mut(), |c| java_string(env, c)) },
        ];
        Some(((**env).CallBooleanMethodA?)(env, activity, method, args.as_ptr()) != 0)
    })
    .unwrap_or(cancel.is_none())
}

/// Puts text on the system's clipboard.
pub fn copy(text: &str) -> bool {
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"copy", c"(Ljava/lang/String;)V")?;
        let args = [jni_sys::jvalue { l: java_string(env, text) }];
        ((**env).CallVoidMethodA?)(env, activity, method, args.as_ptr());
        Some(((**env).ExceptionCheck?)(env) == 0)
    })
    .unwrap_or(false)
}

/// The text on the system's clipboard.
pub fn pasted() -> Option<String> {
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"pasted", c"()Ljava/lang/String;")?;
        let text = ((**env).CallObjectMethodA?)(env, activity, method, std::ptr::null());
        rust_string(env, text)
    })
}

// ===== choosing files =====

/// What the chooser is for.
#[derive(Clone, Copy, PartialEq)]
pub enum Choose {
    Open,
    OpenSeveral,
    /// A place to save a file, under a suggested name.
    Save,
}

/// The chooser's answer, from Java's thread.
static CHOSEN: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);
/// What became of the screen while the app waited for the chooser, which
/// winit wasn't told: 1 if it was taken away, 2 if a new one came too.
static SCREEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// The app's thread (the one `android_main` runs on).
static MAIN: OnceLock<std::thread::ThreadId> = OnceLock::new();

/// Opens the system's file chooser and waits for the answer: the paths
/// chosen (none if it was cancelled). `types` are extensions without the dot.
///
/// The chooser is a screen of its own. While it's up, Android pauses the
/// app and takes its screen away, and waits for the app to take note of
/// each step. So the wait here takes note of them, in the event loop's
/// place, and tells the app afterwards what became of its screen (`screen`).
///
/// The paths are of copies, in the app's cache: Android's documents aren't
/// files with paths. What the app writes to a copy goes to the document
/// too (`wrote`).
pub fn choose(kind: Choose, name: &str, types: &[String]) -> Vec<String> {
    use std::sync::atomic::Ordering;
    use winit::platform::android::activity::{InputStatus, MainEvent, PollEvent};
    // (Only the app's thread may look at the system's events.)
    if MAIN.get() != Some(&std::thread::current().id()) {
        return Vec::new();
    }
    *CHOSEN.lock().unwrap() = None;
    let started = with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"choose", c"(ILjava/lang/String;[Ljava/lang/String;)V")?;
        let string = ((**env).FindClass?)(env, c"java/lang/String".as_ptr());
        let list = ((**env).NewObjectArray?)(env, types.len() as i32, string, std::ptr::null_mut());
        if list.is_null() {
            return None;
        }
        for (i, t) in types.iter().enumerate() {
            let t = java_string(env, t);
            ((**env).SetObjectArrayElement?)(env, list, i as i32, t);
            ((**env).DeleteLocalRef?)(env, t);
        }
        let kind = match kind {
            Choose::Open => 0,
            Choose::OpenSeveral => 1,
            Choose::Save => 2,
        };
        let args = [jni_sys::jvalue { i: kind }, jni_sys::jvalue { l: java_string(env, name) }, jni_sys::jvalue { l: list }];
        ((**env).CallVoidMethodA?)(env, activity, method, args.as_ptr());
        Some(((**env).ExceptionCheck?)(env) == 0)
    });
    if started != Some(true) {
        return Vec::new();
    }
    let app = app();
    let (mut lost, mut regained) = (false, false);
    let chosen = loop {
        if let Some(chosen) = CHOSEN.lock().unwrap().take() {
            break chosen;
        }
        let (mut input, mut gone) = (false, false);
        app.poll_events(Some(std::time::Duration::from_millis(100)), |event| match event {
            PollEvent::Main(MainEvent::TerminateWindow { .. }) => (lost, regained) = (true, false),
            PollEvent::Main(MainEvent::InitWindow { .. }) => regained = true,
            PollEvent::Main(MainEvent::InputAvailable) => input = true,
            PollEvent::Main(MainEvent::Destroy) => gone = true,
            _ => {}
        });
        // (Touches and keys that came just before the chooser: nothing in
        // the app is there to take them.)
        if input {
            if let Ok(mut events) = app.input_events_iter() {
                while events.next(|_| InputStatus::Handled) {}
            }
        }
        if gone {
            break Vec::new();
        }
    };
    // (An activity that's gone has no screen either.)
    if lost || chosen.is_empty() && destroyed() {
        SCREEN.store(if regained { 2 } else { 1 }, Ordering::Relaxed);
    }
    chosen
}

/// What became of the screen while the app waited in `choose`, once: 1 if
/// it was taken away, 2 if a new one came too, 0 if nothing.
pub fn screen() -> u8 {
    SCREEN.swap(0, std::sync::atomic::Ordering::Relaxed)
}

/// The program wrote the file at `path`. If it's the copy of a document
/// (see `choose`), the document gets what's in it.
pub fn wrote(path: &str) {
    if !path.contains("/cache/documents/") {
        return;
    }
    with_activity(|env, activity| unsafe {
        let method = method(env, activity, c"store", c"(Ljava/lang/String;)Z")?;
        let args = [jni_sys::jvalue { l: java_string(env, path) }];
        ((**env).CallBooleanMethodA?)(env, activity, method, args.as_ptr());
        Some(())
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_tessel_TesselActivity_onChosen(env: *mut jni_sys::JNIEnv, _class: jni_sys::jclass, paths: jni_sys::jobjectArray) {
    let mut chosen = Vec::new();
    unsafe {
        let count = if paths.is_null() { 0 } else { ((**env).GetArrayLength.unwrap())(env, paths) };
        for i in 0..count {
            let path = ((**env).GetObjectArrayElement.unwrap())(env, paths, i);
            chosen.extend(rust_string(env, path));
            ((**env).DeleteLocalRef.unwrap())(env, path);
        }
    }
    *CHOSEN.lock().unwrap() = Some(chosen);
    super::app::wake();
}

/// What the keyboard and the system tell the app, from Java's thread; the
/// app takes them on its own (see `take_input`).
pub enum Input {
    Typed(String),
    /// The word being composed: shown at the caret, in place of the one
    /// before, and typed only when the keyboard says so.
    Composing(String),
    /// Characters before and after the caret are taken back.
    Delete(usize, usize),
    /// The keyboard changed the text it was given (`keyboard`): what was
    /// from `start` to `end` is now `text`, and the selection is `select`,
    /// all in Java's characters (see `byte_at`). `field` is the field it
    /// was given for, and `serial` counts its changes.
    Edit { field: i32, serial: i32, start: usize, end: usize, text: String, select: (usize, usize) },
    /// The system's bars or the keyboard moved.
    Insets,
}

static INPUT: std::sync::Mutex<Vec<Input>> = std::sync::Mutex::new(Vec::new());

fn tell(input: Input) {
    INPUT.lock().unwrap().push(input);
    super::app::wake();
}

pub fn take_input() -> Vec<Input> {
    std::mem::take(&mut *INPUT.lock().unwrap())
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_tessel_TesselActivity_onText(env: *mut jni_sys::JNIEnv, _class: jni_sys::jclass, text: jni_sys::jstring, kind: i32) {
    let text = unsafe { rust_string(env, text) }.unwrap_or_default();
    tell(if kind == 1 { Input::Composing(text) } else { Input::Typed(text) });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_tessel_TesselActivity_onDelete(_env: *mut jni_sys::JNIEnv, _class: jni_sys::jclass, before: i32, after: i32) {
    tell(Input::Delete(before.max(0) as usize, after.max(0) as usize));
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_tessel_TesselActivity_onEdit(
    env: *mut jni_sys::JNIEnv,
    _class: jni_sys::jclass,
    field: i32,
    serial: i32,
    start: i32,
    end: i32,
    text: jni_sys::jstring,
    select_start: i32,
    select_end: i32,
) {
    let text = unsafe { rust_string(env, text) }.unwrap_or_default();
    let at = |n: i32| n.max(0) as usize;
    tell(Input::Edit { field, serial, start: at(start), end: at(end), text, select: (at(select_start), at(select_end)) });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_tessel_TesselActivity_onInsets(_env: *mut jni_sys::JNIEnv, _class: jni_sys::jclass) {
    tell(Input::Insets);
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    print_to_log();
    // Relative paths are in the app's own folder (the only one it may write).
    // It's the app's home too (`homeFolder()`, and the folders in it).
    if let Some(folder) = app.internal_data_path() {
        let _ = std::env::set_current_dir(&folder);
        if std::env::var_os("HOME").is_none_or(|h| h.is_empty() || h == "/") {
            // (Nothing else runs yet that could be reading the environment.)
            unsafe { std::env::set_var("HOME", &folder) };
        }
    }
    let _ = APP.set(app);
    let _ = MAIN.set(std::thread::current().id());
    unsafe { tsl_main() };
    // (A program without an `app`: its timers and background work.)
    super::run_until_idle();
    let _ = std::io::Write::flush(&mut std::io::stdout());
}
