//! `tessel preview <path> --out <prefix>`: a picture of a program's window
//! as it first appears, for an editor to show next to the code, with what
//! the editor needs to connect the picture to the code:
//!
//! - `<prefix>.png`: the window, at twice its size in pixels;
//! - `<prefix>.layout.json`: the views in it: each one's depth in the tree,
//!   kind, place, and which view expression of the source it is;
//! - `<prefix>.views.json`: the files, and those view expressions: where
//!   each is written, and where its arguments and modifiers are.
//!
//! The app runs inside `tessel` without a window, as with `tessel run --hot`
//! and a script (see the runtime's `TESSEL_SCRIPT`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tessel_compiler::ast::Item;
use tessel_compiler::codegen;
use tessel_compiler::jit::Jit;
use tessel_compiler::source::SourceMap;

/// `texts`: (file, the file to read its text from instead), for text an
/// editor hasn't saved.
pub fn run(path: &Path, out: &str, texts: &[(PathBuf, PathBuf)]) -> ExitCode {
    let files = match crate::program_files(path) {
        Ok(files) => files,
        Err(msg) => {
            eprintln!("tessel: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let same = |a: &Path, b: &Path| a == b || a.canonicalize().ok().is_some_and(|a| Some(a) == b.canonicalize().ok());
    let mut sources = SourceMap::new();
    for file in &files {
        let from = texts.iter().find(|(of, _)| same(of, file)).map_or(file, |(_, from)| from);
        match std::fs::read_to_string(from) {
            Ok(text) => sources.add(file.display().to_string(), text),
            Err(e) => {
                eprintln!("tessel: can't read `{}`: {e}", from.display());
                return ExitCode::FAILURE;
            }
        };
    }
    let analysis = tessel_compiler::analyze(&mut sources, true);
    if !analysis.diags.is_empty() {
        crate::report(&analysis.diags.list, &sources);
        return ExitCode::FAILURE;
    }
    if !analysis.program.items.iter().any(|i| matches!(i, Item::App(_))) {
        eprintln!("tessel: nothing to preview: the program has no `app`");
        return ExitCode::FAILURE;
    }
    let (object, views) = match codegen::compile_design(&analysis.program, &analysis.checked, &sources) {
        Ok(compiled) => compiled,
        Err(diags) => {
            crate::report(&diags, &sources);
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::write(format!("{out}.views.json"), views.to_json(&sources)) {
        eprintln!("tessel: can't write `{out}.views.json`: {e}");
        return ExitCode::FAILURE;
    }
    let entry = Jit::new(&crate::repl::runtime_symbols()).and_then(|mut jit| {
        jit.add_object(&object)?;
        let entry = jit.lookup(&codegen::hot_entry_symbol(1))?;
        // (The code stays loaded for as long as the process lives.)
        std::mem::forget(jit);
        Ok(entry)
    });
    let entry = match entry {
        Ok(entry) => entry,
        Err(e) => {
            eprintln!("tessel: can't load the compiled code: {e}");
            return ExitCode::FAILURE;
        }
    };
    // (No other thread is running yet.)
    unsafe {
        std::env::set_var("TESSEL_SCRIPT", format!("snapshot {out}.png\nlayout {out}.layout.json"));
        if let Ok(me) = std::env::current_exe() {
            std::env::set_var("TESSEL_EXE", me);
        }
    }
    tessel_runtime::guard_stack();
    let entry: extern "C" fn() = unsafe { std::mem::transmute(entry) };
    // (Doesn't return: the end of the script ends the process.)
    entry();
    let _ = std::io::stdout().flush();
    ExitCode::SUCCESS
}
