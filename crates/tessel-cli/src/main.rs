use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tessel_compiler::diag::render;
use tessel_compiler::source::SourceMap;

const USAGE: &str = "usage: tessel <command>

commands:
  check <path>   check a program for errors; <path> is a .tsl file or a folder
  build <path>   compile a program to a native app (not implemented yet)
  run <path>     build and run a program (not implemented yet)
  version        print the compiler version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("version" | "--version" | "-V"), _) => {
            println!("tessel {}", tessel_compiler::VERSION);
            ExitCode::SUCCESS
        }
        (Some("check"), Some(path)) => check(Path::new(path)),
        (Some(cmd @ ("build" | "run")), Some(_)) => {
            eprintln!("tessel: `{cmd}` is not implemented yet");
            ExitCode::FAILURE
        }
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

fn check(path: &Path) -> ExitCode {
    let files = match program_files(path) {
        Ok(files) => files,
        Err(msg) => {
            eprintln!("tessel: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let mut sources = SourceMap::new();
    for file in &files {
        match std::fs::read_to_string(file) {
            Ok(text) => {
                sources.add(file.display().to_string(), text);
            }
            Err(e) => {
                eprintln!("tessel: can't read `{}`: {e}", file.display());
                return ExitCode::FAILURE;
            }
        }
    }
    let analysis = tessel_compiler::analyze(&sources, true);
    let color = std::io::stderr().is_terminal();
    for d in &analysis.diags.list {
        eprintln!("{}", render(d, &sources, color));
    }
    let n = analysis.diags.list.len();
    let plural = |k: usize, word: &str| format!("{k} {word}{}", if k == 1 { "" } else { "s" });
    if n == 0 {
        println!("ok: {} checked, no errors", plural(files.len(), "file"));
        ExitCode::SUCCESS
    } else {
        eprintln!("{} found", plural(n, "error"));
        ExitCode::FAILURE
    }
}
