//! `tessel build --android`: an app as an Android package (`.apk`).
//!
//! The program is compiled for Android and linked with the Android build
//! of the runtime into a shared library, which an activity loads: Android's
//! `NativeActivity`, with a little Java of Tessel's own on top (see
//! `TesselActivity.java`) for the keyboard, dialogs and the clipboard.
//! Compiling the program needs nothing but `tessel`; linking and packaging
//! use Android's own tools (the NDK's linker, and `aapt2`, `d8`, `zipalign`
//! and `apksigner` of the SDK's build tools) and the JDK's, which is why
//! the Android SDK and NDK and a JDK have to be installed.

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
    match runtime_here() {
        Ok(lib) => line("Tessel's runtime for Android", Ok(shown(lib))),
        Err(_) => println!("  later    Tessel's runtime for Android: the first Android build downloads it (about 50 MB)"),
    }
    // Not needed to build: only to try the app.
    println!("To run an app:");
    match sdk.as_ref().ok().map(|s| s.join("platform-tools").join(format!("adb{}", std::env::consts::EXE_SUFFIX))) {
        Some(adb) if adb.is_file() => {
            // (Listing the devices starts adb's server, which stays, and on
            // Windows keeps this command's output open: whoever reads it
            // would wait for the server to end. One started here is stopped.)
            let port = std::env::var("ANDROID_ADB_SERVER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(5037u16);
            let running = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
            let nothing = std::process::Stdio::null;
            // (Started with no output of its own, or the list below would wait for it too.)
            let _ = Command::new(&adb).arg("start-server").stdin(nothing()).stdout(nothing()).stderr(nothing()).status();
            let listed = Command::new(&adb).arg("devices").stdin(nothing()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            let devices: Vec<String> = listed
                .unwrap_or_default()
                .lines()
                .skip(1)
                .filter_map(|l| l.split_once('\t'))
                .filter(|(_, state)| state.trim() == "device")
                .map(|(name, _)| name.to_string())
                .collect();
            if cfg!(windows) && !running {
                let _ = Command::new(&adb).arg("kill-server").stdin(nothing()).stdout(nothing()).stderr(nothing()).status();
            }
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

/// The file of a release that holds the runtime for Android.
fn runtime_package() -> String {
    format!("tessel-v{}-android-arm64.tar.gz", tessel_compiler::VERSION)
}

/// Where the Android build of the runtime library is, if it's here:
/// `android/libtessel_runtime.a` next to `tessel` (or, in the repository,
/// where cargo builds it), or in Tessel's folder, where `runtime_lib`
/// downloads it to.
fn runtime_here() -> Result<PathBuf, PathBuf> {
    if let Some(lib) = env_path("TESSEL_ANDROID_RUNTIME") {
        return Ok(lib);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let home = super::update::home().unwrap_or_else(|_| exe.with_file_name("android"));
    let downloaded = home.join("android").join(tessel_compiler::VERSION).join("libtessel_runtime.a");
    let places = [
        exe.with_file_name("android").join("libtessel_runtime.a"),
        exe.with_file_name("..").join(format!("android/{TRIPLE}/release/libtessel_runtime.a")),
        downloaded.clone(),
    ];
    places.into_iter().find(|p| p.is_file()).ok_or(downloaded)
}

/// The Android build of the runtime library. It isn't in Tessel's package
/// (most people never build for Android): it's downloaded the first time,
/// once for each version of Tessel.
fn runtime_lib() -> Result<PathBuf, String> {
    let to = match runtime_here() {
        Ok(lib) => return Ok(lib),
        Err(to) => to,
    };
    eprintln!("Getting Tessel's runtime for Android (once for each version of Tessel)…");
    let version = format!("v{}", tessel_compiler::VERSION);
    let dir = to.parent().unwrap_or(Path::new("."));
    super::update::fetch_part(&version, &runtime_package(), dir).map_err(|e| format!("can't get the runtime for Android: {e}"))?;
    match to.is_file() {
        true => Ok(to),
        false => Err(format!("`{}` has no runtime library in it", runtime_package())),
    }
}

/// The Java part of every app (see the file).
const ACTIVITY_JAVA: &str = include_str!("TesselActivity.java");

/// The Java part of an app, compiled for Android (`classes.dex`). It's the
/// same for every app, so it's compiled once (for each version of it) and
/// kept in Tessel's folder.
fn activity_dex(tools: &Tools, work: &Path) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(ACTIVITY_JAVA.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    let kept = super::update::home().ok().map(|h| h.join("android").join(format!("activity-{hash}.dex")));
    if let Some(dex) = kept.as_ref().filter(|d| d.is_file()) {
        return Ok(dex.clone());
    }
    let write = |file: &Path, text: &str| {
        std::fs::create_dir_all(file.parent().unwrap_or(work))
            .and_then(|()| std::fs::write(file, text))
            .map_err(|e| format!("can't write `{}`: {e}", file.display()))
    };
    let source = work.join("java/dev/tessel/TesselActivity.java");
    write(&source, ACTIVITY_JAVA)?;
    let classes = work.join("classes");
    run(
        "javac",
        Command::new(jdk_tool(&tools.java, "javac")?)
            .args(["--release", "11", "-nowarn", "-cp"])
            .arg(&tools.platform.0)
            .arg("-d")
            .arg(&classes)
            .arg(&source),
    )?;
    let compiled: Vec<PathBuf> = files_under(&classes).into_iter().filter(|f| f.extension().is_some_and(|e| e == "class")).collect();
    let dex_dir = work.join("dex");
    std::fs::create_dir_all(&dex_dir).map_err(|e| format!("can't create `{}`: {e}", dex_dir.display()))?;
    run(
        "d8",
        Command::new(&tools.java)
            .arg("-cp")
            .arg(tools.build_tools.join("lib/d8.jar"))
            .args(["com.android.tools.r8.D8", "--release", "--min-api", &MIN_SDK.to_string(), "--lib"])
            .arg(&tools.platform.0)
            .arg("--output")
            .arg(&dex_dir)
            .args(&compiled),
    )?;
    let dex = dex_dir.join("classes.dex");
    // (Kept for the next build, if Tessel's folder can be written.)
    if let Some(kept) = kept {
        let _ = std::fs::create_dir_all(kept.parent().unwrap_or(work));
        if std::fs::copy(&dex, &kept).is_ok() {
            return Ok(kept);
        }
    }
    Ok(dex)
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

/// What signs a package: a keystore, the key in it, and where its
/// passwords are (an environment variable, or Android's debug password).
struct Signing {
    keystore: PathBuf,
    key: String,
    /// `None`: Android's debug key.
    password_vars: Option<(&'static str, &'static str)>,
}

/// The environment variables that hold a release keystore's passwords.
const STORE_PASSWORD: &str = "TESSEL_ANDROID_KEYSTORE_PASSWORD";
const KEY_PASSWORD: &str = "TESSEL_ANDROID_KEY_PASSWORD";

impl Signing {
    /// The app's own key (`[android]` in `tessel.toml`), or the debug key.
    fn of(found: Option<&manifest::Found>, java: &Path) -> Result<Signing, String> {
        let Some((keystore, key)) = manifest::keystore(found) else {
            return Ok(Signing { keystore: debug_key(java)?, key: "androiddebugkey".into(), password_vars: None });
        };
        if !keystore.is_file() {
            return Err(format!(
                "can't find the keystore `{}` that tessel.toml names\n = help: make one with `keytool -genkeypair -keystore {} -alias upload -keyalg RSA -keysize 2048 -validity 10000`",
                keystore.display(),
                keystore.display()
            ));
        }
        let key = key.ok_or("tessel.toml's `[android]` names a keystore but not the key in it\n = help: add `key = \"upload\"` (the alias the key was made with)")?;
        if env_path(STORE_PASSWORD).is_none() {
            return Err(format!(
                "the keystore's password isn't set\n = help: set {STORE_PASSWORD} (and {KEY_PASSWORD}, if the key has another) in the environment; passwords don't go in tessel.toml"
            ));
        }
        let key_var = if env_path(KEY_PASSWORD).is_some() { KEY_PASSWORD } else { STORE_PASSWORD };
        Ok(Signing { keystore, key, password_vars: Some((STORE_PASSWORD, key_var)) })
    }
}

/// A tool of the JDK that `java` belongs to.
fn jdk_tool(java: &Path, name: &str) -> Result<PathBuf, String> {
    let tool = java.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    // (`java` found on the PATH: so is the tool, if it's a JDK.)
    let tool = if java.parent().is_some_and(|p| !p.as_os_str().is_empty()) { tool } else { PathBuf::from(name) };
    match Command::new(&tool).arg("--version").output() {
        Ok(_) => Ok(tool),
        Err(_) => Err(format!("can't find `{name}`\n = help: it comes with a JDK (a Java runtime alone isn't enough); install one, or set JAVA_HOME")),
    }
}

/// The permissions a package has to declare for what its program does,
/// as lines of the manifest. A program that calls `startLocation` may ask
/// where the phone is; one that doesn't can't, and its users aren't told
/// it might. (`program` is its compiled code, which names what it calls.)
fn permissions(program: &[u8]) -> String {
    let calls = |name: &str| program.windows(name.len()).any(|w| w == name.as_bytes());
    let mut lines = String::new();
    if calls("tsl_location_start") {
        for name in ["ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION"] {
            lines += &format!("    <uses-permission android:name=\"android.permission.{name}\" />\n");
        }
    }
    lines
}

/// An Android package name from an app's id: what Android allows (letters,
/// digits and `_`, in at least two parts that start with a letter).
fn package_name(id: &str) -> Result<String, String> {
    let name = id.replace('-', "_");
    let part_ok = |p: &str| p.starts_with(|c: char| c.is_ascii_alphabetic()) && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if name.split('.').count() >= 2 && name.split('.').all(part_ok) {
        return Ok(name);
    }
    Err(format!(
        "the app's id `{id}` can't name an Android package\n = help: give it at least two parts that start with a letter, like `id = \"com.example.tally\"` in tessel.toml"
    ))
}

/// Builds the program at `path` as an Android package at `out`: an `.aab`
/// (for Google Play) if `out` ends in that, an `.apk` otherwise.
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
    let id = package_name(&info.id.clone().unwrap_or_else(|| tessel_compiler::default_app_id(&name)))?;
    let signing = Signing::of(found.as_ref(), &tools.java)?;
    let bundle = out.extension().is_some_and(|e| e == "aab");
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
{permissions}    <application android:label="{label}" android:icon="@mipmap/icon" android:theme="@style/App" android:hasCode="true"
        android:extractNativeLibs="true" android:enableOnBackInvokedCallback="false">
        <activity android:name="dev.tessel.TesselActivity" android:exported="true"
            android:windowSoftInputMode="adjustNothing"
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
            permissions = permissions(&std::fs::read(&obj).unwrap_or_default()),
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
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("can't create `{}`: {e}", parent.display()))?;
    }
    let link = |proto: bool, to: &Path| {
        let mut aapt2 = Command::new(tools.tool("aapt2"));
        aapt2.args(["link", "-o"]).arg(to).arg("-I").arg(&tools.platform.0).arg("--manifest").arg(&manifest_file);
        if proto {
            aapt2.arg("--proto-format");
        }
        run("aapt2", aapt2.arg(&resources))
    };
    let dex = activity_dex(tools, work)?;
    if bundle {
        return app_bundle(out, tools, &signing, work, &lib, &dex, &link);
    }
    std::fs::copy(&dex, work.join("classes.dex")).map_err(|e| format!("can't copy `{}`: {e}", dex.display()))?;

    let unsigned = work.join("unsigned.apk");
    link(false, &unsigned)?;
    // (`aapt add` names the file in the package by the path it's given.)
    run(
        "aapt",
        Command::new(tools.tool("aapt")).current_dir(work).args(["add", "unsigned.apk", "lib/arm64-v8a/libmain.so", "classes.dex"]),
    )?;
    let aligned = work.join("aligned.apk");
    run("zipalign", Command::new(tools.tool("zipalign")).args(["-f", "-p", "4"]).arg(&unsigned).arg(&aligned))?;

    let mut apksigner = Command::new(&tools.java);
    apksigner.arg("-jar").arg(tools.build_tools.join("lib/apksigner.jar")).args(["sign", "--ks"]).arg(&signing.keystore);
    match signing.password_vars {
        Some((store, key)) => apksigner.args(["--ks-pass", &format!("env:{store}"), "--key-pass", &format!("env:{key}")]),
        None => apksigner.args(["--ks-pass", "pass:android", "--key-pass", "pass:android"]),
    };
    run("apksigner", apksigner.args(["--ks-key-alias", &signing.key, "--out"]).arg(out).arg(&aligned))?;
    let _ = std::fs::remove_file(out.with_extension("apk.idsig"));
    Ok(())
}

/// The settings of an app bundle (`BundleConfig.pb`: the version of the
/// bundle format's tool it follows), and what says which processors its
/// libraries are for (`native.pb`: `lib/arm64-v8a` is for 64-bit ARM).
/// Both are the same for every app, as Google's `bundletool` writes them.
const BUNDLE_CONFIG: &[u8] = b"\x0a\x08\x12\x061.18.1";
const BUNDLE_NATIVE: &[u8] = b"\x0a\x15\x0a\x0dlib/arm64-v8a\x12\x04\x0a\x02\x08\x03";

/// Every file in `dir`, and in the folders in it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        match entry.is_dir() {
            true => found.extend(files_under(&entry)),
            false => found.push(entry),
        }
    }
    found
}

/// An Android App Bundle, the format Google Play takes: the app's one
/// module (`base/`), with its manifest and resources in the bundle's own
/// encoding, signed with the JDK's `jarsigner`.
fn app_bundle(
    out: &Path,
    tools: &Tools,
    signing: &Signing,
    work: &Path,
    lib: &Path,
    dex: &Path,
    link: &dyn Fn(bool, &Path) -> Result<(), String>,
) -> Result<(), Failed> {
    let (jar, jarsigner) = (jdk_tool(&tools.java, "jar")?, jdk_tool(&tools.java, "jarsigner")?);
    let linked = work.join("proto.apk");
    link(true, &linked)?;
    let base = work.join("bundle/base");
    let made = |e: std::io::Error| format!("can't put the bundle together in `{}`: {e}", work.display());
    std::fs::create_dir_all(base.join("manifest")).map_err(made)?;
    std::fs::create_dir_all(base.join("lib/arm64-v8a")).map_err(made)?;
    run("jar", Command::new(&jar).current_dir(&base).arg("xf").arg(&linked))?;
    std::fs::rename(base.join("AndroidManifest.xml"), base.join("manifest/AndroidManifest.xml")).map_err(made)?;
    std::fs::copy(lib, base.join("lib/arm64-v8a/libmain.so")).map_err(made)?;
    std::fs::create_dir_all(base.join("dex")).map_err(made)?;
    std::fs::copy(dex, base.join("dex/classes.dex")).map_err(made)?;
    std::fs::write(base.join("native.pb"), BUNDLE_NATIVE).map_err(made)?;
    std::fs::write(work.join("bundle/BundleConfig.pb"), BUNDLE_CONFIG).map_err(made)?;
    let _ = std::fs::remove_file(out);
    // (`M`: a zip without the manifest a `.jar` has. Each file is named:
    // given a folder, `jar` adds entries for folders too, which a bundle
    // mustn't have.)
    let root = work.join("bundle");
    let mut zip = Command::new(&jar);
    zip.arg("cfM").arg(out);
    for file in files_under(&root) {
        zip.arg("-C").arg(&root).arg(file.strip_prefix(&root).unwrap_or(&file));
    }
    run("jar", &mut zip)?;

    let mut sign = Command::new(&jarsigner);
    sign.arg("-keystore").arg(&signing.keystore);
    match signing.password_vars {
        Some((store, key)) => sign.args(["-storepass:env", store, "-keypass:env", key]),
        None => sign.args(["-storepass", "android", "-keypass", "android"]),
    };
    run("jarsigner", sign.args(["-sigalg", "SHA256withRSA", "-digestalg", "SHA-256"]).arg(out).arg(&signing.key))?;
    if signing.password_vars.is_none() {
        eprintln!(
            "note: signed with Android's debug key, which Google Play doesn't take\n = help: name the app's own keystore in tessel.toml: `[android]`, `keystore = \"upload.jks\"`, `key = \"upload\"`"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_become_package_names() {
        assert_eq!(package_name("com.example.my-app").unwrap(), "com.example.my_app");
        assert_eq!(package_name("dev.tessel.counter").unwrap(), "dev.tessel.counter");
        assert!(package_name("counter").is_err());
        assert!(package_name("com.2fast.app").is_err());
    }

    #[test]
    fn versions_become_numbers() {
        assert_eq!(version_code("1.4.2", None), 1_004_002);
        assert_eq!(version_code("2.0", None), 2_000_000);
        assert_eq!(version_code("1.0", Some("17")), 17);
        assert_eq!(version_code("", None), 1);
    }
}
