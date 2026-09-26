use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use tessel_compiler::codegen;
use tessel_compiler::diag::{render, Diagnostic};
use tessel_compiler::source::SourceMap;

const USAGE: &str = "usage: tessel <command>

commands:
  check <path>   check a program for errors; <path> is a .tsl file or a folder
  build <path> [-o <output>]   compile a program to a native executable
  run <path>     build and run a program
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
            let out = match args.get(2).map(String::as_str) {
                Some("-o") => args.get(3).map(PathBuf::from),
                _ => None,
            };
            let out = out.unwrap_or_else(|| default_output(Path::new(path)));
            match build(Path::new(path), &out) {
                Ok(()) => {
                    println!("built {}", out.display());
                    ExitCode::SUCCESS
                }
                Err(code) => code,
            }
        }
        (Some("run"), Some(path)) => run(Path::new(path), &args[2..]),
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
    PathBuf::from(stem.unwrap_or_else(|| "app".as_ref()))
}

/// The runtime library, found next to the `tessel` executable.
fn runtime_lib() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let lib = exe.with_file_name("libtessel_runtime.a");
    if lib.exists() {
        Ok(lib)
    } else {
        Err(format!("can't find the runtime library at `{}`; run `cargo build` in the tessel repository", lib.display()))
    }
}

fn build(path: &Path, out: &Path) -> Result<(), ExitCode> {
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
    let status = Command::new("cc")
        .arg(&obj)
        .arg(&runtime)
        .args(platform_link_args())
        .arg("-o")
        .arg(out)
        .status()
        .map_err(|e| fail(format!("can't run the system linker `cc`: {e}")))?;
    let _ = std::fs::remove_file(&obj);
    if !status.success() {
        return Err(fail("linking failed".into()));
    }
    Ok(())
}

/// System libraries the runtime needs (windowing, GPU, fonts).
fn platform_link_args() -> Vec<&'static str> {
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
    let exe = std::env::temp_dir().join(format!("tessel-run-{}", std::process::id()));
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
