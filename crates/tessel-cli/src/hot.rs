//! `tessel run <path> --hot`: runs an app, and when its files change, gives
//! the running app its new code, keeping its state. (A program without a
//! window has no state to keep on screen: it starts again instead.)
//!
//! The app runs inside `tessel` (as the REPL's code does: see
//! `tessel_compiler::jit`), so that code compiled later can be loaded next
//! to it. The runtime keeps the views' state; the new code's views find it
//! again, unless they keep different state than before (see the runtime's
//! `tsl_ui_view_begin`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use tessel_compiler::ast::Item;
use tessel_compiler::codegen;
use tessel_compiler::jit::Jit;

/// The program's files (and the modules it imports): path, when changed, size.
type Stamp = Vec<(PathBuf, Option<SystemTime>, u64)>;

struct Hot {
    path: PathBuf,
    jit: Jit,
    /// How many times the program was compiled.
    generation: usize,
    /// The files of the last compilation, as they were then.
    stamp: Stamp,
}

static HOT: Mutex<Option<Hot>> = Mutex::new(None);

fn stamp(program: &Path, used: &[PathBuf]) -> Stamp {
    let mut files = crate::program_files(program).unwrap_or_default();
    files.extend(used.iter().cloned());
    files.sort();
    files.dedup();
    files
        .into_iter()
        .map(|f| {
            let meta = std::fs::metadata(&f).ok();
            (f, meta.as_ref().and_then(|m| m.modified().ok()), meta.map_or(0, |m| m.len()))
        })
        .collect()
}

impl Hot {
    /// Compiles and loads the program as it is now. The address of its entry
    /// point; `None` (with the errors reported) if it can't be compiled.
    fn compile(&mut self) -> Option<usize> {
        let mut sources = match crate::load(&self.path) {
            Ok((sources, _)) => sources,
            Err(_) => {
                self.stamp = stamp(&self.path, &[]);
                return None;
            }
        };
        let analysis = tessel_compiler::analyze(&mut sources, true);
        // (Imported modules' files are among the sources now.)
        let used: Vec<PathBuf> = sources.files().map(|(_, f)| PathBuf::from(&f.name)).collect();
        self.stamp = stamp(&self.path, &used);
        if !analysis.diags.is_empty() {
            crate::report(&analysis.diags.list, &sources);
            return None;
        }
        if !analysis.program.items.iter().any(|i| matches!(i, Item::App(_))) {
            eprintln!("tessel: hot reload needs the program to have an `app`");
            return None;
        }
        let generation = self.generation + 1;
        let object = match codegen::compile_hot(&analysis.program, &analysis.checked, &sources, generation) {
            Ok(o) => o,
            Err(diags) => {
                crate::report(&diags, &sources);
                return None;
            }
        };
        let loaded = self.jit.add_object(&object).and_then(|()| self.jit.lookup(&codegen::hot_entry_symbol(generation)));
        match loaded {
            Ok(address) => {
                self.generation = generation;
                Some(address as usize)
            }
            Err(e) => {
                eprintln!("tessel: can't load the compiled code: {e}");
                None
            }
        }
    }
}

/// If the program's files changed: compiles it again, and returns the
/// address of the new code's entry point (to call on the UI thread).
/// Otherwise, and when the new code has errors, 0.
fn reload() -> usize {
    let mut hot = HOT.lock().unwrap();
    let Some(hot) = hot.as_mut() else { return 0 };
    let used: Vec<PathBuf> = hot.stamp.iter().map(|(f, ..)| f.clone()).collect();
    if stamp(&hot.path, &used) == hot.stamp {
        return 0;
    }
    match hot.compile() {
        Some(address) => {
            eprintln!("tessel: reloaded");
            address
        }
        None => {
            eprintln!("tessel: not reloaded; the app goes on with the code it has");
            0
        }
    }
}

/// A program without an `app`: runs it as `tessel run` does, and when its
/// files change while it's running, stops it and runs it again. Ends when
/// the program does.
fn restart_on_change(path: &Path, program_args: &[String]) -> ExitCode {
    let exe = std::env::temp_dir().join(format!("tessel-run-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    let code = loop {
        if let Err(code) = crate::build(path, &exe) {
            break code;
        }
        let before = stamp(path, &[]);
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(program_args);
        if let Ok(me) = std::env::current_exe() {
            cmd.env("TESSEL_EXE", me);
        }
        crate::program_env(&mut cmd, &exe, path);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("tessel: can't start the program: {e}");
                break ExitCode::FAILURE;
            }
        };
        let status = loop {
            if let Ok(Some(status)) = child.try_wait() {
                break Some(status);
            }
            if stamp(path, &[]) != before {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("tessel: restarted");
                break None;
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        if let Some(status) = status {
            break ExitCode::from(status.code().unwrap_or(1) as u8);
        }
    };
    let _ = std::fs::remove_file(&exe);
    code
}

pub fn run(path: &Path, program_args: &[String]) -> ExitCode {
    let jit = match Jit::new(&crate::repl::runtime_symbols()) {
        Ok(jit) => jit,
        Err(e) => {
            eprintln!("tessel: can't run with hot reload: {e}");
            return ExitCode::FAILURE;
        }
    };
    let has_app = crate::load(path).is_ok_and(|(mut sources, _)| {
        tessel_compiler::analyze(&mut sources, true).program.items.iter().any(|i| matches!(i, Item::App(_)))
    });
    if !has_app {
        return restart_on_change(path, program_args);
    }
    let mut hot = Hot { path: path.to_path_buf(), jit, generation: 0, stamp: Vec::new() };
    let Some(entry) = hot.compile() else { return ExitCode::FAILURE };
    *HOT.lock().unwrap() = Some(hot);

    // Programs can find the `tessel` command through TESSEL_EXE, as with `tessel run`.
    if let Ok(me) = std::env::current_exe() {
        // (No other thread is running yet.) The app runs in this process,
        // so `appPath()` is its source when the executable is this one.
        unsafe {
            std::env::set_var("TESSEL_RUN_EXE", &me);
            std::env::set_var("TESSEL_RUN_SOURCE", crate::source_path(path));
            std::env::set_var("TESSEL_EXE", me);
        }
    }
    if std::env::var_os("TESSEL_SCRIPT").is_some() {
        // A scripted run reloads when its script says `reload`.
        tessel_runtime::ui::on_reload(reload);
    } else {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(250));
            let entry = reload();
            if entry != 0 {
                tessel_runtime::ui::run_on_ui(entry);
            }
        });
    }
    tessel_runtime::guard_stack();
    let entry: extern "C" fn() = unsafe { std::mem::transmute(entry) };
    // (Doesn't return: closing the window ends the process.)
    entry();
    let _ = std::io::stdout().flush();
    ExitCode::SUCCESS
}
