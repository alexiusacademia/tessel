mod lsp;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use tessel_compiler::codegen;
use tessel_compiler::diag::{render, Diagnostic};
use tessel_compiler::source::SourceMap;

const USAGE: &str = "usage: tessel <command>

commands:
  check <path>   check a program for errors; <path> is a .tsl file or a folder
  build <path> [-o <output>] [--bundle]
                 compile a program to a native executable; with --bundle,
                 an app bundle you can double-click (macOS: Name.app)
  run <path>     build and run a program
  lsp            start the language server (used by editors)
  version        print the compiler version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("version" | "--version" | "-V"), _) => {
            println!("tessel {}", tessel_compiler::VERSION);
            ExitCode::SUCCESS
        }
        (Some("check"), Some(path)) => check(Path::new(path)),
        (Some("build"), Some(path)) => {
            let rest = &args[2..];
            let out = rest.iter().position(|a| a == "-o").and_then(|i| rest.get(i + 1)).map(PathBuf::from);
            let bundle = rest.iter().any(|a| a == "--bundle");
            let out = out.unwrap_or_else(|| default_output(Path::new(path)));
            match build(Path::new(path), &out) {
                Ok(app_name) if bundle => match make_bundle(&out, app_name.as_deref()) {
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
        (Some("run"), Some(path)) => run(Path::new(path), &args[2..]),
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

fn report(diags: &[Diagnostic], sources: &SourceMap) {
    let color = std::io::stderr().is_terminal();
    for d in diags {
        eprintln!("{}", render(d, sources, color));
    }
    eprintln!("{} found", plural(diags.len(), "error"));
}

fn check(path: &Path) -> ExitCode {
    let (sources, count) = match load(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let analysis = tessel_compiler::analyze(&sources, true);
    if analysis.diags.is_empty() {
        println!("ok: {} checked, no errors", plural(count, "file"));
        ExitCode::SUCCESS
    } else {
        report(&analysis.diags.list, &sources);
        ExitCode::FAILURE
    }
}

fn default_output(path: &Path) -> PathBuf {
    let stem = if path.is_dir() { path.file_name() } else { path.file_stem() };
    let mut name = stem.unwrap_or_else(|| "app".as_ref()).to_os_string();
    name.push(std::env::consts::EXE_SUFFIX);
    PathBuf::from(name)
}

/// File name of the runtime static library on this platform.
const RUNTIME_LIB: &str = if cfg!(target_env = "msvc") { "tessel_runtime.lib" } else { "libtessel_runtime.a" };

/// The runtime library, found next to the `tessel` executable.
fn runtime_lib() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let lib = exe.with_file_name(RUNTIME_LIB);
    if lib.exists() {
        Ok(lib)
    } else {
        Err(format!("can't find the runtime library at `{}`; run `cargo build` in the tessel repository", lib.display()))
    }
}

/// Compiles and links a program. Returns the `app`'s name, if it has one.
fn build(path: &Path, out: &Path) -> Result<Option<String>, ExitCode> {
    let (sources, _) = load(path)?;
    let analysis = tessel_compiler::analyze(&sources, true);
    if !analysis.diags.is_empty() {
        report(&analysis.diags.list, &sources);
        return Err(ExitCode::FAILURE);
    }
    let fail = |msg: String| {
        eprintln!("tessel: {msg}");
        ExitCode::FAILURE
    };
    let runtime = runtime_lib().map_err(fail)?;
    let obj = std::env::temp_dir().join(format!("tessel-{}.o", std::process::id()));
    if let Err(diags) = codegen::compile(&analysis.program, &analysis.checked, &sources, &obj, true) {
        report(&diags, &sources);
        return Err(ExitCode::FAILURE);
    }
    let app_name = analysis.program.items.iter().find_map(|i| match i {
        tessel_compiler::ast::Item::App(a) => Some(a.name.name.clone()),
        _ => None,
    });
    let output = linker(&obj, &runtime, out, app_name.is_some())
        .map_err(fail)?
        .output()
        .map_err(|e| fail(format!("can't run the system linker: {e}")))?;
    let _ = std::fs::remove_file(&obj);
    if !output.status.success() {
        // MSVC's link.exe reports errors on stdout.
        let details = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return Err(fail(format!("linking failed ({}):\n{}", output.status, details.trim())));
    }
    Ok(app_name)
}

/// Wraps a built app in a platform app bundle. On macOS: `Name.app`, next
/// to the executable (which moves inside it). Returns the bundle's path.
fn make_bundle(exe: &Path, app_name: Option<&str>) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        // Windows apps are already a single double-clickable .exe.
        return Ok(exe.to_path_buf());
    }
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let name = app_name.unwrap_or(stem);
    let bundle = exe.with_file_name(format!("{name}.app"));
    let macos = bundle.join("Contents/MacOS");
    if bundle.exists() {
        std::fs::remove_dir_all(&bundle).map_err(|e| format!("can't replace `{}`: {e}", bundle.display()))?;
    }
    std::fs::create_dir_all(&macos).map_err(|e| format!("can't create `{}`: {e}", bundle.display()))?;
    std::fs::rename(exe, macos.join(name)).map_err(|e| format!("can't move the executable into the bundle: {e}"))?;
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
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    );
    std::fs::write(bundle.join("Contents/Info.plist"), plist).map_err(|e| format!("can't write Info.plist: {e}"))?;
    Ok(bundle)
}

/// The command that links the program's object file with the runtime.
fn linker(obj: &Path, runtime: &Path, out: &Path, gui: bool) -> Result<Command, String> {
    if cfg!(target_env = "msvc") {
        // MSVC's link.exe, found the way Rust finds it (Visual Studio Build Tools).
        let target = if cfg!(target_arch = "aarch64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" };
        let mut cmd = cc::windows_registry::find(target, "link.exe")
            .ok_or("can't find the MSVC linker (link.exe); install Visual Studio Build Tools with the C++ workload")?;
        cmd.arg("/NOLOGO").arg(obj).arg(runtime).args(platform_link_args());
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

/// System libraries the runtime needs (windowing, GPU, fonts, networking).
fn platform_link_args() -> Vec<&'static str> {
    if cfg!(windows) {
        return [
            "kernel32.lib", "advapi32.lib", "bcrypt.lib", "ntdll.lib", "userenv.lib", "ws2_32.lib",
            "synchronization.lib", "user32.lib", "gdi32.lib", "ole32.lib", "oleaut32.lib", "shell32.lib",
            "uuid.lib", "dwmapi.lib", "uxtheme.lib", "imm32.lib", "comctl32.lib", "winmm.lib", "dxgi.lib",
            "d3d12.lib", "d3dcompiler.lib", "opengl32.lib", "propsys.lib", "runtimeobject.lib", "secur32.lib",
            "crypt32.lib", "ncrypt.lib", "dbghelp.lib", "pathcch.lib",
        ]
        .to_vec();
    }
    if cfg!(target_os = "macos") {
        let frameworks = [
            "AppKit", "ApplicationServices", "Carbon", "CoreFoundation", "CoreGraphics", "CoreText", "CoreVideo",
            "Foundation", "Metal", "QuartzCore",
        ];
        let mut args: Vec<&str> = frameworks.iter().flat_map(|f| ["-framework", *f]).collect();
        args.extend(["-lobjc", "-liconv", "-Wl,-dead_strip", "-mmacosx-version-min=11.0"]);
        args
    } else {
        vec![]
    }
}

fn run(path: &Path, program_args: &[String]) -> ExitCode {
    let exe = std::env::temp_dir().join(format!("tessel-run-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    if let Err(code) = build(path, &exe) {
        return code;
    }
    let status = Command::new(&exe).args(program_args).status();
    let _ = std::fs::remove_file(&exe);
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("tessel: can't start the program: {e}");
            ExitCode::FAILURE
        }
    }
}
