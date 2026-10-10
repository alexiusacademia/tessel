//! `tessel build --android`: an app as an Android package (`.apk`).
//!
//! The program is compiled for Android and linked with the Android build
//! of the runtime into a shared library, which the system's own
//! `NativeActivity` loads: the package has no Java code. Compiling needs
//! nothing but `tessel`; linking and packaging use Android's own tools (the
//! NDK's linker, and `aapt2`, `zipalign` and `apksigner` of the SDK's build
//! tools), which is why the Android SDK and NDK have to be installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use tessel_compiler::codegen;

use super::{AppInfo, manifest};

/// The target apps are compiled for: 64-bit ARM, which every phone that
/// can install apps from Google Play has.
const TRIPLE: &str = "aarch64-linux-android";
/// The oldest Android an app runs on (11), and the one it's made for.
const MIN_SDK: u32 = 30;

/// Where Android's tools are.
struct Tools {
    /// The SDK's newest build tools (`aapt2`, `zipalign`, `apksigner`).
    build_tools: PathBuf,
    /// The newest platform's `android.jar`, and its API level.
    platform: (PathBuf, u32),
    /// The NDK's compiler, which links.
    clang: PathBuf,
    java: PathBuf,
}

/// The folder in `dir` with the highest version number in its name.
fn newest(dir: &Path) -> Option<PathBuf> {
    let version = |p: &PathBuf| -> Vec<u32> {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect()
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    found.sort_by_key(version);
    found.pop()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn home() -> Option<PathBuf> {
    env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

/// The Android SDK's folder.
fn sdk() -> Result<PathBuf, String> {
    env_path("ANDROID_HOME")
        .or_else(|| env_path("ANDROID_SDK_ROOT"))
        .or_else(|| home().map(|h| h.join("Library/Android/sdk")))
        .filter(|p| p.is_dir())
        .or_else(|| env_path("LOCALAPPDATA").map(|p| p.join("Android/Sdk")).filter(|p| p.is_dir()))
        .or_else(|| home().map(|h| h.join("Android/Sdk")).filter(|p| p.is_dir()))
        .ok_or_else(|| {
            "can't find the Android SDK\n = help: install it (with Android Studio, or `sdkmanager`), and set ANDROID_HOME to its folder if it isn't in the usual place".to_string()
        })
}

fn missing(sdk: &Path, what: &str, package: &str) -> String {
    format!("the Android SDK at `{}` has no {what}\n = help: install it: `sdkmanager \"{package}\"`", sdk.display())
}

fn build_tools(sdk: &Path) -> Result<PathBuf, String> {
    newest(&sdk.join("build-tools")).ok_or_else(|| missing(sdk, "build tools", "build-tools;36.0.0"))
}

/// The newest platform's `android.jar`, and its API level.
fn platform(sdk: &Path) -> Result<(PathBuf, u32), String> {
    let platform = newest(&sdk.join("platforms")).ok_or_else(|| missing(sdk, "platform", "platforms;android-36"))?;
    let level = platform.file_name().and_then(|n| n.to_string_lossy().split('-').nth(1)?.split('.').next()?.parse().ok());
    Ok((platform.join("android.jar"), level.unwrap_or(MIN_SDK)))
}

/// The NDK's compiler.
fn ndk_clang(sdk: &Path) -> Result<PathBuf, String> {
    let ndk = env_path("ANDROID_NDK_HOME")
        .filter(|p| p.is_dir())
        .or_else(|| newest(&sdk.join("ndk")))
        .ok_or_else(|| missing(sdk, "NDK", "ndk;28.2.13676358"))?;
    newest(&ndk.join("toolchains/llvm/prebuilt"))
        .map(|host| host.join("bin").join(format!("clang{}", std::env::consts::EXE_SUFFIX)))
        .filter(|c| c.is_file())
        .ok_or_else(|| format!("the Android NDK at `{}` has no compiler", ndk.display()))
}

impl Tools {
    fn find() -> Result<Tools, String> {
        let sdk = sdk()?;
        Ok(Tools { build_tools: build_tools(&sdk)?, platform: platform(&sdk)?, clang: ndk_clang(&sdk)?, java: java()? })
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }
}

/// `tessel android doctor`: what building for Android needs, whether each
/// is there, and how to get what isn't.
pub fn doctor() -> std::process::ExitCode {
    let mut problems = 0;
    let mut line = |what: &str, found: Result<String, String>| match found {
        Ok(where_) => println!("  ok       {what}: {where_}"),
        Err(why) => {
            problems += 1;
            let (first, help) = why.split_once("\n = help: ").unwrap_or((&why, ""));
            println!("  missing  {what}: {first}");
            if !help.is_empty() {
                println!("           {help}");
            }
        }
    };
    let shown = |p: PathBuf| p.display().to_string();
    println!("Building Android apps (`tessel build --android`) needs:");
    let sdk = sdk();
    line("Android SDK", sdk.clone().map(shown));
    if let Ok(sdk) = &sdk {
        line("build tools", build_tools(sdk).map(shown));
        line("platform", platform(sdk).map(|(jar, level)| format!("Android API {level} ({})", jar.display())));
        line("NDK", ndk_clang(sdk).map(shown));
    }
    line("Java", java().map(shown));
    line("Tessel's runtime for Android", runtime_lib().map(shown));
    // Not needed to build: only to try the app.
    println!("To run an app:");
    match sdk.as_ref().ok().map(|s| s.join("platform-tools").join(format!("adb{}", std::env::consts::EXE_SUFFIX))) {
        Some(adb) if adb.is_file() => {
            let listed = Command::new(&adb).arg("devices").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            let devices: Vec<String> = listed
                .unwrap_or_default()
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once('\t'))
                .filter(|(_, state)| state.trim() == "device")
                .map(|(name, _)| name.to_string())
                .collect();
            match devices.is_empty() {
                true => println!("  none     no phone or emulator is connected (start an emulator, or plug in a phone with USB debugging on)"),
                false => println!("  ok       connected: {}; install with `adb install -r <app>.apk`", devices.join(", ")),
            }
        }
        _ => println!("  missing  adb: install it: `sdkmanager \"platform-tools\"`"),
    }
    if problems == 0 {
        println!("Everything needed to build is here.");
        std::process::ExitCode::SUCCESS
    } else {
        println!("{problems} missing; see above.");
        std::process::ExitCode::FAILURE
    }
}

/// Java, which signs packages (`apksigner` is written in it).
fn java() -> Result<PathBuf, String> {
    let exe = std::env::consts::EXE_SUFFIX;
    let works = |java: &PathBuf| Command::new(java).arg("-version").output().is_ok_and(|o| o.status.success());
    let mut places: Vec<PathBuf> = env_path("JAVA_HOME").map(|h| h.join("bin").join(format!("java{exe}"))).into_iter().collect();
    places.push(PathBuf::from(format!("java{exe}")));
    // (Android Studio's own, and Homebrew's, which aren't on the PATH.)
    places.push(PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home/bin/java"));
    for brew in ["/opt/homebrew/opt", "/usr/local/opt"] {
        let mut found: Vec<PathBuf> = std::fs::read_dir(brew)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("openjdk")))
            .collect();
        found.sort();
        places.extend(found.into_iter().rev().map(|p| p.join("bin/java")));
    }
    places.into_iter().find(works).ok_or_else(|| {
        "can't find Java, which signs Android packages\n = help: install a JDK (Android Studio comes with one), or set JAVA_HOME".to_string()
    })
}

/// The Android build of the runtime library: `android/libtessel_runtime.a`
/// next to `tessel` (or, in the repository, where cargo builds it).
fn runtime_lib() -> Result<PathBuf, String> {
    if let Some(lib) = env_path("TESSEL_ANDROID_RUNTIME") {
        return Ok(lib);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let places = [
        exe.with_file_name("android").join("libtessel_runtime.a"),
        exe.with_file_name("..").join(format!("android/{TRIPLE}/release/libtessel_runtime.a")),
    ];
    places.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        format!("this `tessel` has no runtime for Android (it would be at `{}`)", places[0].display())
    })
}

/// Runs a tool, and gives what it printed if it failed.
fn run(what: &str, command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(|e| format!("can't run {what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let details = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Err(format!("{what} failed ({}):\n{}", output.status, details.trim()))
}

/// What `app Name(appearance: …)` says: dark (true), light, or none (the
/// app follows the phone's theme).
fn appearance(program: &tessel_compiler::ast::Program) -> Option<bool> {
    use tessel_compiler::ast::{ExprKind, Item};
    let app = program.items.iter().find_map(|i| match i {
        Item::App(a) => Some(a),
        _ => None,
    })?;
    let arg = app.args.iter().find(|a| a.label.as_ref().is_some_and(|l| l.name == "appearance"))?;
    let case = match &arg.value.kind {
        ExprKind::DotName(n) => n.as_str(),
        ExprKind::Member { name, .. } => name.name.as_str(),
        _ => return None,
    };
    match case {
        "dark" => Some(true),
        "light" => Some(false),
        _ => None,
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A version like `1.4.2` as the number Android compares versions by.
fn version_code(version: &str, build: Option<&str>) -> u32 {
    if let Some(n) = build.and_then(|b| b.trim().parse::<u32>().ok()) {
        return n.max(1);
    }
    let parts: Vec<u32> = version.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect();
    let part = |i: usize| parts.get(i).copied().unwrap_or(0).min(999);
    (part(0) * 1_000_000 + part(1) * 1_000 + part(2)).max(1)
}

/// The key debug packages are signed with: Android's usual one, made the
/// first time. (A package for Google Play is signed with the app's own.)
fn debug_key(java: &Path) -> Result<PathBuf, String> {
    let folder = home().ok_or("can't find the home folder")?.join(".android");
    let key = folder.join("debug.keystore");
    if key.is_file() {
        return Ok(key);
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("can't create `{}`: {e}", folder.display()))?;
    let keytool = java.with_file_name(format!("keytool{}", std::env::consts::EXE_SUFFIX));
    run(
        "keytool",
        Command::new(if keytool.is_file() { keytool } else { PathBuf::from("keytool") })
            .args(["-genkeypair", "-keystore"])
            .arg(&key)
            .args(["-alias", "androiddebugkey", "-storepass", "android", "-keypass", "android"])
            .args(["-keyalg", "RSA", "-keysize", "2048", "-validity", "10000"])
            .args(["-dname", "CN=Android Debug,O=Android,C=US"]),
    )?;
    Ok(key)
}

/// Builds the program at `path` as an Android package at `out`.
pub fn build(path: &Path, out: &Path) -> Result<(), std::process::ExitCode> {
    use std::process::ExitCode;
    let fail = |msg: String| {
        eprintln!("tessel: {msg}");
        ExitCode::FAILURE
    };
    let (mut sources, _) = super::load(path)?;
    let analysis = tessel_compiler::analyze(&mut sources, true);
    if !analysis.diags.is_empty() {
        super::report(&analysis.diags.list, &sources);
        return Err(ExitCode::FAILURE);
    }
    let Some(app) = AppInfo::of(&analysis.program) else {
        return Err(fail("an Android package needs an `app`; this program has only `fn main()`".into()));
    };
    let tools = Tools::find().map_err(fail)?;
    let runtime = runtime_lib().map_err(fail)?;

    let work = std::env::temp_dir().join(format!("tessel-android-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let made = package(path, out, &app, &analysis, &sources, &tools, &runtime, &work);
    let _ = std::fs::remove_dir_all(&work);
    match made {
        Ok(()) => Ok(()),
        Err(Failed::Reported) => Err(ExitCode::FAILURE),
        Err(Failed::With(msg)) => Err(fail(msg)),
    }
}

enum Failed {
    /// The compiler's errors, already shown.
    Reported,
    With(String),
}

impl From<String> for Failed {
    fn from(msg: String) -> Failed {
        Failed::With(msg)
    }
}

#[allow(clippy::too_many_arguments)]
fn package(
    path: &Path,
    out: &Path,
    app: &AppInfo,
    analysis: &tessel_compiler::Analysis,
    sources: &tessel_compiler::source::SourceMap,
    tools: &Tools,
    runtime: &Path,
    work: &Path,
) -> Result<(), Failed> {
    let write = |file: &Path, bytes: &[u8]| -> Result<(), String> {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("can't create `{}`: {e}", parent.display()))?;
        }
        std::fs::write(file, bytes).map_err(|e| format!("can't write `{}`: {e}", file.display()))
    };
    let found = manifest::find(path).ok().flatten();
    let info = found.as_ref().map(|f| f.manifest.app.clone()).unwrap_or_default();
    let name = info.name.clone().unwrap_or_else(|| app.name.clone());
    let id = info.id.clone().unwrap_or_else(|| tessel_compiler::default_app_id(&name));
    let version = info.version.clone().unwrap_or_else(|| "1.0".into());

    // The program, as the library the activity loads.
    let obj = work.join("program.o");
    write(&obj, b"")?;
    if let Err(diags) = codegen::compile(&analysis.program, &analysis.checked, sources, &obj, true, Some(TRIPLE)) {
        super::report(&diags, sources);
        return Err(Failed::Reported);
    }
    let lib = work.join("lib/arm64-v8a/libmain.so");
    write(&lib, b"")?;
    run(
        "the Android linker",
        Command::new(&tools.clang)
            .arg(format!("--target={TRIPLE}{MIN_SDK}"))
            .args(["-shared", "-o"])
            .arg(&lib)
            .arg(&obj)
            // (The activity's entry point is the runtime's: kept, though nothing here uses it.)
            .args(["-Wl,-u,ANativeActivity_onCreate", "-Wl,--gc-sections", "-Wl,-z,max-page-size=16384"])
            .arg(runtime)
            .args(["-llog", "-landroid", "-ldl", "-lm", "-Wl,--strip-debug"]),
    )?;

    // What the system is told about the app, and its icon.
    let manifest_file = work.join("AndroidManifest.xml");
    let target = tools.platform.1.max(MIN_SDK);
    write(
        &manifest_file,
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{id}" android:versionCode="{code}" android:versionName="{version}">
    <uses-sdk android:minSdkVersion="{MIN_SDK}" android:targetSdkVersion="{target}" />
    <uses-permission android:name="android.permission.INTERNET" />
    <application android:label="{label}" android:icon="@mipmap/icon" android:theme="@style/App" android:hasCode="false"
        android:extractNativeLibs="true" android:enableOnBackInvokedCallback="false">
        <activity android:name="android.app.NativeActivity" android:exported="true"
            android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|uiMode|density">
            <meta-data android:name="android.app.lib_name" android:value="main" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
            id = xml(&id),
            code = version_code(&version, info.build.as_deref()),
            version = xml(&version),
            label = xml(&name),
        )
        .as_bytes(),
    )?;
    let icon = super::icon::Icon::of_program(Some(path), manifest::icon_path(found.as_ref()).as_deref())?;
    for (density, size) in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)] {
        write(&work.join(format!("res/mipmap-{density}/icon.png")), &icon.png_at(size))?;
    }

    // The window before the app has drawn, and the status bar's icons: dark
    // on a light app, light on a dark one. An app that follows the phone's
    // theme has both, and the system picks.
    let style = |dark: bool| {
        let (parent, light) = if dark { ("Theme.DeviceDefault.NoActionBar", "false") } else { ("Theme.DeviceDefault.Light.NoActionBar", "true") };
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="App" parent="@android:style/{parent}">
        <item name="android:windowLightStatusBar">{light}</item>
        <item name="android:windowLightNavigationBar">{light}</item>
    </style>
</resources>
"#
        )
    };
    let appearance = appearance(&analysis.program);
    write(&work.join("res/values/styles.xml"), style(appearance == Some(true)).as_bytes())?;
    if appearance.is_none() {
        write(&work.join("res/values-night/styles.xml"), style(true).as_bytes())?;
    }

    let resources = work.join("resources.zip");
    run("aapt2", Command::new(tools.tool("aapt2")).args(["compile", "--dir"]).arg(work.join("res")).arg("-o").arg(&resources))?;
    let unsigned = work.join("unsigned.apk");
    run(
        "aapt2",
        Command::new(tools.tool("aapt2"))
            .args(["link", "-o"])
            .arg(&unsigned)
            .arg("-I")
            .arg(&tools.platform.0)
            .arg("--manifest")
            .arg(&manifest_file)
            .arg(&resources),
    )?;
    // (`aapt add` names the file in the package by the path it's given.)
    run("aapt", Command::new(tools.tool("aapt")).current_dir(work).args(["add", "unsigned.apk", "lib/arm64-v8a/libmain.so"]))?;
    let aligned = work.join("aligned.apk");
    run("zipalign", Command::new(tools.tool("zipalign")).args(["-f", "-p", "4"]).arg(&unsigned).arg(&aligned))?;

    let key = debug_key(&tools.java)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("can't create `{}`: {e}", parent.display()))?;
    }
    run(
        "apksigner",
        Command::new(&tools.java)
            .arg("-jar")
            .arg(tools.build_tools.join("lib/apksigner.jar"))
            .args(["sign", "--ks"])
            .arg(&key)
            .args(["--ks-pass", "pass:android", "--key-pass", "pass:android", "--ks-key-alias", "androiddebugkey", "--out"])
            .arg(out)
            .arg(&aligned),
    )?;
    let _ = std::fs::remove_file(out.with_extension("apk.idsig"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_become_numbers() {
        assert_eq!(version_code("1.4.2", None), 1_004_002);
        assert_eq!(version_code("2.0", None), 2_000_000);
        assert_eq!(version_code("1.0", Some("17")), 17);
        assert_eq!(version_code("", None), 1);
    }
}
