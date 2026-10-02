mod icon;
mod hot;
mod lsp;
mod preview;
mod repl;
mod update;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use tessel_compiler::codegen;
use tessel_compiler::diag::{render, Diagnostic};
use tessel_compiler::source::SourceMap;

const USAGE: &str = "usage: tessel [command]

With no command, opens the Tessel IDE on the current folder.

commands:
  ide [folder]   open the Tessel IDE on a folder (default: the current one)
  check <path> [--format lines]
                 check a program for errors; <path> is a .tsl file or a folder.
                 With --format lines, prints one error per line for tools:
                 file, line, column, end line, end column, message (tab-separated)
  build <path> [-o <output>] [--bundle]
                 compile a program to a native executable; with --bundle,
                 an app bundle you can double-click (macOS: Name.app).
                 An icon.png next to the program is the app's icon
  run <path> [--hot]
                 build and run a program; with --hot, an app takes its new
                 code when its files change, and keeps its state
  repl           type code and see what it does, a line at a time
  complete <file> <offset> [--text <path>]
                 completion suggestions at byte <offset> of <file> (with the
                 other .tsl files in its folder); --text reads the file's
                 current, unsaved text from <path>. Prints `start <offset>`,
                 then one suggestion per line: kind, label, detail (tab-separated)
  preview <path> --out <prefix> [--text <file> <from>]...
                 a picture of an app's window as it first appears, for
                 editors: writes <prefix>.png, and what's where in it and in
                 the code (<prefix>.layout.json, <prefix>.views.json); --text
                 reads a file's current, unsaved text from <from>
  update [version] [--check]
                 update Tessel to the latest release (or to <version>, like
                 v0.1.2); with --check, only say whether there's a newer one
  lsp            start the language server (used by editors)
  help           show this help
  version        print the compiler version";

/// The IDE's source, built into `tessel` so it works wherever `tessel` is installed.
const IDE_SOURCES: &[(&str, &str)] = &[
    ("design.tsl", include_str!("../../../ide/design.tsl")),
    ("help.tsl", include_str!("../../../ide/help.tsl")),
    ("main.tsl", include_str!("../../../ide/main.tsl")),
    ("project.tsl", include_str!("../../../ide/project.tsl")),
    ("widgets.tsl", include_str!("../../../ide/widgets.tsl")),
];

// The documentation that comes with `tessel`: `DOCS`, (path, bytes).
include!(concat!(env!("OUT_DIR"), "/docs.rs"));

/// Writes the documentation to `folder` (for the IDE's Docs panel, which
/// reads it from there).
fn write_docs(folder: &Path) -> std::io::Result<()> {
    // (The contents last: its being there means everything is.)
    let contents = |name: &str| name == "index.txt";
    for (name, bytes) in DOCS.iter().filter(|(n, _)| !contents(n)).chain(DOCS.iter().filter(|(n, _)| contents(n))) {
        let file = folder.join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, bytes)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1)) {
        (None, _) => ide(Path::new(".")),
        (Some("ide"), folder) => ide(Path::new(folder.map_or(".", String::as_str))),
        (Some("help" | "--help" | "-h"), _) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        (Some("version" | "--version" | "-V"), _) => {
            println!("tessel {}", tessel_compiler::VERSION);
            ExitCode::SUCCESS
        }
        (Some("check"), Some(path)) => {
            let lines = args.windows(2).any(|w| w[0] == "--format" && w[1] == "lines");
            check(Path::new(path), lines)
        }
        (Some("build"), Some(path)) => {
            let rest = &args[2..];
            let out = rest.iter().position(|a| a == "-o").and_then(|i| rest.get(i + 1)).map(PathBuf::from);
            let bundle = rest.iter().any(|a| a == "--bundle");
            let mut out = out.unwrap_or_else(|| default_output(Path::new(path)));
            if cfg!(windows) && out.extension().is_none() {
                out.set_extension("exe");
            }
            match build(Path::new(path), &out) {
                Ok(app_name) if bundle => match make_bundle(&out, app_name.as_deref(), Some(Path::new(path))) {
                    Ok(bundle) => {
                        println!("built {}", bundle.display());
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("tessel: {e}");
                        ExitCode::FAILURE
                    }
                },
                Ok(_) => {
                    println!("built {}", out.display());
                    ExitCode::SUCCESS
                }
                Err(code) => code,
            }
        }
        (Some("run"), Some(path)) if args[2..].iter().any(|a| a == "--hot") => {
            let rest: Vec<String> = args[2..].iter().filter(|a| *a != "--hot").cloned().collect();
            hot::run(Path::new(path), &rest)
        }
        (Some("run"), Some(path)) => run(Path::new(path), &args[2..]),
        (Some("repl"), _) => repl::run(),
        (Some("complete"), Some(file)) => {
            let offset = args.get(2).and_then(|o| o.parse().ok());
            let text = args.windows(2).find(|w| w[0] == "--text").map(|w| PathBuf::from(&w[1]));
            match offset {
                Some(offset) => complete(Path::new(file), offset, text.as_deref()),
                None => {
                    eprintln!("{USAGE}");
                    ExitCode::FAILURE
                }
            }
        }
        (Some("preview"), Some(path)) => {
            let rest = &args[2..];
            let out = rest.iter().position(|a| a == "--out").and_then(|i| rest.get(i + 1));
            let texts: Vec<(PathBuf, PathBuf)> = rest
                .windows(3)
                .filter(|w| w[0] == "--text")
                .map(|w| (PathBuf::from(&w[1]), PathBuf::from(&w[2])))
                .collect();
            match out {
                Some(out) => preview::run(Path::new(path), out, &texts),
                None => {
                    eprintln!("{USAGE}");
                    ExitCode::FAILURE
                }
            }
        }
        (Some("update"), _) => update::run(&args[1..]),
        (Some("lsp"), _) => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("tessel lsp: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// A program is one `.tsl` file, or every `.tsl` file in a folder.
fn program_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("can't read folder `{}`: {e}", path.display()))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "tsl"))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(format!("no .tsl files in `{}`", path.display()));
        }
        Ok(files)
    } else if path.exists() {
        Ok(vec![path.to_path_buf()])
    } else {
        Err(format!("`{}` doesn't exist", path.display()))
    }
}

fn plural(k: usize, word: &str) -> String {
    format!("{k} {word}{}", if k == 1 { "" } else { "s" })
}

fn load(path: &Path) -> Result<(SourceMap, usize), ExitCode> {
    let files = program_files(path).map_err(|msg| {
        eprintln!("tessel: {msg}");
        ExitCode::FAILURE
    })?;
    let mut sources = SourceMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file).map_err(|e| {
            eprintln!("tessel: can't read `{}`: {e}", file.display());
            ExitCode::FAILURE
        })?;
        sources.add(file.display().to_string(), text);
    }
    Ok((sources, files.len()))
}

/// `tessel complete`: suggestions for the IDE's editor.
fn complete(file: &Path, offset: usize, text_from: Option<&Path>) -> ExitCode {
    let Ok(text) = std::fs::read_to_string(text_from.unwrap_or(file)) else {
        eprintln!("tessel: can't read `{}`", text_from.unwrap_or(file).display());
        return ExitCode::FAILURE;
    };
    let same = |p: &Path| p.canonicalize().ok().is_some_and(|p| Some(p) == file.canonicalize().ok());
    let folder = file.parent().filter(|f| !f.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut sources = SourceMap::new();
    let id = sources.add(file.display().to_string(), text);
    for other in program_files(folder).unwrap_or_default() {
        if !same(&other)
            && let Ok(t) = std::fs::read_to_string(&other)
        {
            sources.add(other.display().to_string(), t);
        }
    }
    let c = tessel_compiler::complete(&sources, id, offset);
    let mut out = format!("start {}\n", c.start);
    for item in c.items {
        out += &format!("{}\t{}\t{}\n", item.kind.name(), item.label, item.detail);
    }
    print!("{out}");
    ExitCode::SUCCESS
}

fn report(diags: &[Diagnostic], sources: &SourceMap) {
    let color = std::io::stderr().is_terminal();
    for d in diags {
        eprintln!("{}", render(d, sources, color));
    }
    eprintln!("{} found", plural(diags.len(), "error"));
}

fn check(path: &Path, lines: bool) -> ExitCode {
    let (mut sources, count) = match load(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let analysis = tessel_compiler::analyze(&mut sources, true);
    if lines {
        for d in &analysis.diags.list {
            let file = sources.file(d.span().file);
            let (line, col) = file.line_col(d.span().lo as usize);
            let (end_line, end_col) = file.line_col(d.span().hi as usize);
            let mut message = d.message.replace(['\t', '\n'], " ");
            if let Some(help) = &d.help {
                message += &format!(" (help: {})", help.replace(['\t', '\n'], " "));
            }
            println!("{}\t{line}\t{col}\t{end_line}\t{end_col}\t{message}", file.name);
        }
        return if analysis.diags.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    if analysis.diags.is_empty() {
        println!("ok: {} checked, no errors", plural(count, "file"));
        ExitCode::SUCCESS
    } else {
        report(&analysis.diags.list, &sources);
        ExitCode::FAILURE
    }
}

/// `todos/` or `todos.tsl` → `todos` in the current folder. If that's a
/// folder (building `todos/` from its parent), `todos-app` instead.
fn default_output(path: &Path) -> PathBuf {
    // `tessel build .` is named after the folder, not `.`.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let stem = if path.is_dir() { path.file_name() } else { path.file_stem() };
    let stem = stem.unwrap_or_else(|| "app".as_ref()).to_os_string();
    let mut name = stem.clone();
    name.push(std::env::consts::EXE_SUFFIX);
    if Path::new(&name).is_dir() {
        name = stem;
        name.push("-app");
        name.push(std::env::consts::EXE_SUFFIX);
    }
    PathBuf::from(name)
}

/// File name of the runtime static library on this platform.
const RUNTIME_LIB: &str = if cfg!(target_env = "msvc") { "tessel_runtime.lib" } else { "libtessel_runtime.a" };

/// The runtime library, found next to the `tessel` executable.
fn runtime_lib() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // `tessel` may be a symlink (e.g. in /usr/local/bin) to the real one.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    // With the built-in linker, Windows apps use the MinGW build of the runtime.
    let name = if cfg!(windows) && bundled_sdk().is_some() { "libtessel_runtime.a" } else { RUNTIME_LIB };
    let lib = exe.with_file_name(name);
    if lib.exists() {
        Ok(lib)
    } else {
        Err(format!("can't find the runtime library at `{}`; run `cargo build` in the tessel repository", lib.display()))
    }
}

/// Compiles and links a program. Returns the `app`'s name, if it has one.
fn build(path: &Path, out: &Path) -> Result<Option<String>, ExitCode> {
    let (mut sources, _) = load(path)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("tessel: can't create `{}`: {e}", parent.display());
            return Err(ExitCode::FAILURE);
        }
    }
    build_sources(&mut sources, out, Some(path))
}

/// `program` is where the sources are from, for the app's icon (see `icon::Icon::of`).
fn build_sources(sources: &mut SourceMap, out: &Path, program: Option<&Path>) -> Result<Option<String>, ExitCode> {
    let analysis = tessel_compiler::analyze(sources, true);
    if !analysis.diags.is_empty() {
        report(&analysis.diags.list, sources);
        return Err(ExitCode::FAILURE);
    }
    let fail = |msg: String| {
        eprintln!("tessel: {msg}");
        ExitCode::FAILURE
    };
    let runtime = runtime_lib().map_err(fail)?;
    let obj = std::env::temp_dir().join(format!("tessel-{}.o", std::process::id()));
    if let Err(diags) = codegen::compile(&analysis.program, &analysis.checked, sources, &obj, true, target_triple().as_deref()) {
        report(&diags, sources);
        return Err(ExitCode::FAILURE);
    }
    let app_name = analysis.program.items.iter().find_map(|i| match i {
        tessel_compiler::ast::Item::App(a) => Some(a.name.name.clone()),
        _ => None,
    });
    // On Windows, an app's icon is linked into its .exe.
    let windows = target_triple().map_or(cfg!(windows), |t| t.contains("windows"));
    let resources = (windows && app_name.is_some()).then(|| obj.with_extension("res"));
    let linked = match &resources {
        Some(res) => icon::Icon::of(program)
            .and_then(|icon| std::fs::write(res, icon.windows_resources()).map_err(|e| format!("can't write `{}`: {e}", res.display())))
            .and_then(|()| link(&obj, &runtime, out, true, Some(res))),
        None => link(&obj, &runtime, out, app_name.is_some(), None),
    };
    let _ = std::fs::remove_file(&obj);
    if let Some(res) = &resources {
        let _ = std::fs::remove_file(res);
    }
    linked.map_err(fail)?;
    Ok(app_name)
}

/// The target to compile for: this computer (`None`), except that Windows
/// apps linked by the built-in linker use the MinGW runtime. TESSEL_TARGET
/// overrides it, for cross-compiling tests.
fn target_triple() -> Option<String> {
    if let Ok(t) = std::env::var("TESSEL_TARGET") {
        return Some(t);
    }
    (cfg!(windows) && bundled_sdk().is_some()).then(|| "x86_64-pc-windows-gnu".to_string())
}

/// Links a compiled program with the runtime: with the built-in linker when
/// this `tessel` has one, otherwise with the system's. `resources` is a
/// Windows resource file (the app's icon) to link in.
fn link(obj: &Path, runtime: &Path, out: &Path, gui: bool, resources: Option<&Path>) -> Result<(), String> {
    if let Some(sdk) = bundled_sdk() {
        return bundled::link(obj, runtime, out, &sdk, gui, resources);
    }
    let output = linker(obj, runtime, out, gui, resources)?.output().map_err(|e| format!("can't run the system linker: {e}"))?;
    if !output.status.success() {
        // MSVC's link.exe reports errors on stdout.
        let details = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return Err(format!("linking failed ({}):\n{}", output.status, details.trim()));
    }
    Ok(())
}

/// The system library stubs the built-in linker links against (`sdk/`, next
/// to `tessel` in release packages). `None` if this `tessel` has no built-in
/// linker, the stubs are missing, or TESSEL_LINKER=system asks for the system's.
fn bundled_sdk() -> Option<PathBuf> {
    if !cfg!(feature = "bundled-linker") || std::env::var_os("TESSEL_LINKER").is_some_and(|v| v == "system") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let sdk = exe.with_file_name("sdk");
    sdk.is_dir().then_some(sdk)
}

#[cfg(not(feature = "bundled-linker"))]
mod bundled {
    use std::path::Path;

    pub fn link(_: &Path, _: &Path, _: &Path, _: &Path, _: bool, _: Option<&Path>) -> Result<(), String> {
        unreachable!("bundled_sdk() is None without the bundled-linker feature")
    }
}

/// LLVM's linker, lld, built into `tessel` (see build.rs and lld.cpp).
#[cfg(feature = "bundled-linker")]
mod bundled {
    use std::ffi::{CStr, CString, c_char, c_int};
    use std::path::Path;

    unsafe extern "C" {
        fn tessel_lld_link(args: *const *const c_char, count: c_int, output: *mut *mut c_char) -> c_int;
        fn tessel_lld_free(output: *mut c_char);
    }

    pub fn link(obj: &Path, runtime: &Path, out: &Path, sdk: &Path, gui: bool, resources: Option<&Path>) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let args = {
            let _ = (gui, resources);
            macos_args(obj, runtime, out, sdk)?
        };
        #[cfg(not(target_os = "macos"))]
        let args = mingw_args(obj, runtime, out, sdk, gui, resources);
        let args: Vec<CString> = args.into_iter().map(|a| CString::new(a).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        let ptrs: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        let mut output: *mut c_char = std::ptr::null_mut();
        let code = unsafe { tessel_lld_link(ptrs.as_ptr(), ptrs.len() as c_int, &mut output) };
        let messages = if output.is_null() {
            String::new()
        } else {
            let text = unsafe { CStr::from_ptr(output) }.to_string_lossy().into_owned();
            unsafe { tessel_lld_free(output) };
            text
        };
        if code != 0 {
            return Err(format!("linking failed:\n{}", messages.trim()));
        }
        Ok(())
    }

    /// ld64.lld arguments: the program, the runtime, and every system
    /// library in the stub SDK (their stubs list only what Tessel apps use).
    #[cfg(target_os = "macos")]
    fn macos_args(obj: &Path, runtime: &Path, out: &Path, sdk: &Path) -> Result<Vec<String>, String> {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let sdk_version = std::fs::read_to_string(sdk.join("SDKVersion")).unwrap_or_else(|_| "11.0".into());
        let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" };
        let mut args: Vec<String> = ["ld64.lld", "-arch", arch, "-platform_version", "macos", "11.0", sdk_version.trim()]
            .map(String::from)
            .to_vec();
        args.extend(["-syslibroot".into(), path(sdk), "-o".into(), path(out), path(obj), path(runtime)]);
        // No local symbol names or debug information: smaller apps.
        args.extend(["-dead_strip", "-x", "-S"].map(String::from));
        let list = |dir: &Path| -> Result<Vec<String>, String> {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .map_err(|e| format!("can't read the linker stubs in `{}`: {e}", dir.display()))?
                .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect();
            names.sort();
            Ok(names)
        };
        for lib in list(&sdk.join("usr/lib"))? {
            if let Some(name) = lib.strip_prefix("lib").and_then(|l| l.strip_suffix(".tbd")) {
                args.push(format!("-l{name}"));
            }
        }
        for framework in list(&sdk.join("System/Library/Frameworks"))? {
            if let Some(name) = framework.strip_suffix(".framework") {
                args.extend(["-framework".into(), name.into()]);
            }
        }
        Ok(args)
    }

    /// ld.lld arguments in MinGW mode (Windows): the program and the runtime
    /// with MinGW-w64's C runtime and Windows import libraries from `sdk/`,
    /// in the order clang's MinGW driver uses.
    #[cfg(not(target_os = "macos"))]
    fn mingw_args(obj: &Path, runtime: &Path, out: &Path, sdk: &Path, gui: bool, resources: Option<&Path>) -> Vec<String> {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let file = |name: &str| path(&sdk.join(name));
        let mut args: Vec<String> = vec!["ld.lld".into(), "-m".into(), "i386pep".into()];
        args.extend(["--subsystem".into(), if gui { "windows" } else { "console" }.into()]);
        // No debug information: much smaller apps.
        args.push("--strip-all".into());
        args.extend(["-o".into(), path(out), file("crt2.o"), file("crtbegin.o")]);
        args.push(format!("-L{}", path(sdk)));
        args.extend([path(obj), path(runtime)]);
        args.extend(resources.map(path));
        args.extend(super::WINDOWS_LIBS.iter().map(|l| format!("-l{l}")));
        let builtins = file("libclang_rt.builtins-x86_64.a");
        for _ in 0..2 {
            args.extend(["-lunwind", "-lmingw32"].map(String::from));
            args.push(builtins.clone());
            args.extend(["-lunwind", "-lmoldname", "-lmingwex", "-lmsvcrt", "-lkernel32"].map(String::from));
        }
        args.push(file("crtend.o"));
        args
    }
}

/// Wraps a built app in a platform app bundle. On macOS: `Name.app`, next
/// to the executable (which moves inside it), with the icon of `program`
/// (see `icon::Icon::of`). Returns the bundle's path.
fn make_bundle(exe: &Path, app_name: Option<&str>, program: Option<&Path>) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        // Windows apps are already a single double-clickable .exe, with the icon inside.
        return Ok(exe.to_path_buf());
    }
    let icon = icon::Icon::of(program)?;
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let name = app_name.unwrap_or(stem);
    let bundle = exe.with_file_name(format!("{name}.app"));
    let contents = bundle.join("Contents");
    if bundle.exists() {
        std::fs::remove_dir_all(&bundle).map_err(|e| format!("can't replace `{}`: {e}", bundle.display()))?;
    }
    for folder in ["MacOS", "Resources"] {
        std::fs::create_dir_all(contents.join(folder)).map_err(|e| format!("can't create `{}`: {e}", bundle.display()))?;
    }
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleExecutable</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>dev.tessel.{id}</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    );
    std::fs::write(contents.join("Info.plist"), plist).map_err(|e| format!("can't write Info.plist: {e}"))?;
    std::fs::write(contents.join("Resources/AppIcon.icns"), icon.icns()).map_err(|e| format!("can't write the app's icon: {e}"))?;
    // Last, so a bundle with its executable is a complete one.
    std::fs::rename(exe, contents.join("MacOS").join(name)).map_err(|e| format!("can't move the executable into the bundle: {e}"))?;
    Ok(bundle)
}

/// The command that links the program's object file with the runtime.
fn linker(obj: &Path, runtime: &Path, out: &Path, gui: bool, resources: Option<&Path>) -> Result<Command, String> {
    if cfg!(target_env = "msvc") {
        // MSVC's link.exe, found the way Rust finds it (Visual Studio Build Tools).
        let target = if cfg!(target_arch = "aarch64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" };
        let mut cmd = cc::windows_registry::find(target, "link.exe")
            .ok_or("can't find the MSVC linker (link.exe); install Visual Studio Build Tools with the C++ workload")?;
        cmd.arg("/NOLOGO").arg(obj).arg(runtime).args(resources).args(platform_link_args());
        if gui {
            // A window app: no console window next to it.
            cmd.args(["/SUBSYSTEM:WINDOWS", "/ENTRY:mainCRTStartup"]);
        }
        cmd.arg(format!("/OUT:{}", out.display()));
        Ok(cmd)
    } else {
        let mut cmd = Command::new("cc");
        cmd.arg(obj).arg(runtime).args(platform_link_args()).arg("-o").arg(out);
        Ok(cmd)
    }
}

/// Windows libraries the runtime needs (windowing, GPU, fonts, networking).
const WINDOWS_LIBS: &[&str] = &[
    "kernel32", "advapi32", "bcrypt", "ntdll", "userenv", "ws2_32", "synchronization", "user32", "gdi32", "ole32",
    "oleaut32", "shell32", "uuid", "dwmapi", "uxtheme", "imm32", "comctl32", "winmm", "dxgi", "d3d12", "d3dcompiler",
    "opengl32", "propsys", "runtimeobject", "secur32", "crypt32", "ncrypt", "dbghelp", "pathcch",
];

/// System libraries the runtime needs (windowing, GPU, fonts, networking).
fn platform_link_args() -> Vec<String> {
    if cfg!(windows) {
        return WINDOWS_LIBS.iter().map(|l| format!("{l}.lib")).collect();
    }
    if cfg!(target_os = "macos") {
        let frameworks = [
            "AppKit", "ApplicationServices", "Carbon", "CoreFoundation", "CoreGraphics", "CoreText", "CoreVideo",
            "Foundation", "Metal", "QuartzCore",
        ];
        let mut args: Vec<String> = frameworks.iter().flat_map(|f| ["-framework".to_string(), f.to_string()]).collect();
        args.extend(["-lobjc", "-liconv", "-Wl,-dead_strip", "-Wl,-x", "-Wl,-S", "-mmacosx-version-min=11.0"].map(String::from));
        args
    } else {
        vec![]
    }
}

/// Opens the IDE on `folder`, compiling it first if this version of Tessel
/// hasn't yet (it's cached until the IDE or `tessel` itself changes).
fn ide(folder: &Path) -> ExitCode {
    let Ok(folder) = folder.canonicalize() else {
        eprintln!("tessel: `{}` doesn't exist", folder.display());
        return ExitCode::FAILURE;
    };
    let me = std::env::current_exe().ok();
    let stamp = me.as_ref().and_then(|m| m.metadata().ok()?.modified().ok());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&(IDE_SOURCES, tessel_compiler::VERSION, stamp), &mut h);
    let dir = std::env::temp_dir().join(format!("tessel-ide-{:016x}", std::hash::Hasher::finish(&h)));
    // On macOS the IDE is an app bundle, which gives it its icon in the Dock;
    // on Windows the icon is in the .exe.
    let exe = if cfg!(target_os = "macos") {
        dir.join("Tessel.app/Contents/MacOS/Tessel")
    } else {
        dir.join(format!("Tessel{}", std::env::consts::EXE_SUFFIX))
    };
    if !exe.exists() {
        eprintln!("tessel: preparing the IDE (first run only)…");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("tessel: can't create `{}`: {e}", dir.display());
            return ExitCode::FAILURE;
        }
        let mut sources = SourceMap::new();
        for (name, text) in IDE_SOURCES {
            sources.add(format!("ide/{name}"), *text);
        }
        let tmp = dir.join(format!("building{}", std::env::consts::EXE_SUFFIX));
        if let Err(code) = build_sources(&mut sources, &tmp, None) {
            return code;
        }
        let installed = if cfg!(target_os = "macos") {
            make_bundle(&tmp, Some("Tessel"), None).map(drop)
        } else {
            std::fs::rename(&tmp, &exe).map_err(|e| e.to_string())
        };
        if let Err(e) = installed {
            eprintln!("tessel: can't install the IDE: {e}");
            return ExitCode::FAILURE;
        }
    }
    // The documentation, next to the IDE.
    let docs = dir.join("docs");
    if !docs.join("index.txt").exists() {
        if let Err(e) = write_docs(&docs) {
            eprintln!("tessel: can't write the documentation to `{}`: {e}", docs.display());
        }
    }
    let mut cmd = Command::new(&exe);
    cmd.env("TESSEL_PROJECT", &folder);
    cmd.env("TESSEL_DOCS", &docs);
    if let Some(me) = me {
        cmd.env("TESSEL_EXE", me);
    }
    match cmd.status() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(0) as u8),
        Err(e) => {
            eprintln!("tessel: can't start the IDE: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &Path, program_args: &[String]) -> ExitCode {
    let exe = std::env::temp_dir().join(format!("tessel-run-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    if let Err(code) = build(path, &exe) {
        return code;
    }
    // Programs can find the `tessel` command through TESSEL_EXE (the IDE does).
    let mut cmd = Command::new(&exe);
    cmd.args(program_args);
    if let Ok(me) = std::env::current_exe() {
        cmd.env("TESSEL_EXE", me);
    }
    let status = cmd.spawn().and_then(|mut child| {
        pass_signals_to(child.id());
        child.wait()
    });
    let _ = std::fs::remove_file(&exe);
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("tessel: can't start the program: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The program `tessel run` started, for `pass_signal`.
#[cfg(unix)]
static RUNNING: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
    fn kill(pid: i32, signal: i32) -> i32;
}

#[cfg(unix)]
extern "C" fn pass_signal(signal: i32) {
    let pid = RUNNING.load(std::sync::atomic::Ordering::SeqCst);
    if pid > 0 {
        unsafe { kill(pid, signal) };
    }
}

/// Stopping `tessel run` (kill, a timeout, a closed terminal) stops the
/// program it runs too, instead of leaving it running on its own.
#[cfg(unix)]
fn pass_signals_to(child: u32) {
    const SIGHUP: i32 = 1;
    const SIGALRM: i32 = 14;
    const SIGTERM: i32 = 15;
    RUNNING.store(child as i32, std::sync::atomic::Ordering::SeqCst);
    let handler = pass_signal as extern "C" fn(i32) as usize;
    for s in [SIGHUP, SIGALRM, SIGTERM] {
        unsafe { signal(s, handler) };
    }
}

#[cfg(not(unix))]
fn pass_signals_to(_child: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file of the IDE is built into `tessel`, and together they check.
    #[test]
    fn ide_sources_are_complete() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ide");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tsl"))
            .collect();
        on_disk.sort();
        let mut embedded: Vec<String> = IDE_SOURCES.iter().map(|(n, _)| n.to_string()).collect();
        embedded.sort();
        assert_eq!(embedded, on_disk, "add new ide/ files to IDE_SOURCES");

        let mut sources = SourceMap::new();
        for (name, text) in IDE_SOURCES {
            sources.add(format!("ide/{name}"), *text);
        }
        let analysis = tessel_compiler::analyze(&mut sources, true);
        assert!(analysis.diags.is_empty(), "the IDE doesn't check: {:?}", analysis.diags.list);
    }
}
