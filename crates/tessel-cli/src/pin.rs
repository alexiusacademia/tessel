//! Which version of Tessel a project uses. A `tessel.toml` in the project's
//! folder (or a folder above it) can name one:
//!
//! ```toml
//! tessel = "0.1.32"
//! ```
//!
//! When it names another version than this one, the command is handed to
//! that version, which is installed next to this one the first time (see
//! `update::installed`). `tessel +0.1.30 …` or `TESSEL_VERSION=0.1.30`
//! choose a version for one command instead. `tessel pin` writes the file.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::update;

pub const FILE: &str = "tessel.toml";

/// Set for a `tessel` started by another one for a project's version, so
/// it doesn't hand the command on again.
const HANDED_ON: &str = "TESSEL_PINNED";

/// The version a `tessel.toml` names (like `v0.1.32`), if it names one.
/// Other keys and `[sections]` are left for later versions of Tessel.
pub fn read(file: &Path) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    parse(&text).map_err(|(line, msg)| format!("{}:{line}: {msg}", file.display()))
}

fn parse(text: &str) -> Result<Option<String>, (usize, String)> {
    let mut section = false;
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = true;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if section || key.trim() != "tessel" {
            continue;
        }
        let value = value.trim();
        let Some(v) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            return Err((i + 1, format!("write the version in quotes, like tessel = \"{}\"", tessel_compiler::VERSION)));
        };
        return update::normalize(v).map(Some).map_err(|e| (i + 1, e));
    }
    Ok(None)
}

/// The nearest `tessel.toml` for a program at `path` (a file or a folder):
/// in its folder, or a folder above.
pub fn find(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    let start = if start.as_os_str().is_empty() { PathBuf::from(".") } else { start };
    let start = start.canonicalize().ok()?;
    start.ancestors().map(|d| d.join(FILE)).find(|f| f.is_file())
}

/// For `tessel --version`: what to add when the folder it's run in belongs
/// to a project that uses another version than this one, like
/// ` (this project uses 0.1.38)`. The project's commands run with that
/// version; `version` itself isn't handed on, so it says so.
pub fn version_note() -> String {
    let Some(file) = find(Path::new(".")) else { return String::new() };
    let Ok(Some(wanted)) = read(&file) else { return String::new() };
    let number = wanted.trim_start_matches('v');
    if number == tessel_compiler::VERSION {
        return String::new();
    }
    let waiting = if update::is_installed(&wanted) { "" } else { ", not installed yet" };
    format!(" (this project uses {number}{waiting})")
}

/// Where a command's project is: the program it's given, or for `repl` and
/// `lsp` the folder it's run in. `None` for commands that aren't about a
/// project (`update`, `version`, `help`, `ide`, `pin`).
fn project(args: &[String]) -> Option<PathBuf> {
    match args.first().map(String::as_str) {
        Some("check" | "build" | "run" | "preview" | "complete") => args.get(1).map(PathBuf::from),
        Some("repl" | "lsp") => Some(PathBuf::from(".")),
        _ => None,
    }
}

/// Hands the command to the version it should run with, if that isn't this
/// one: then its exit code. `args` loses a leading `+version`.
pub fn hand_on(args: &mut Vec<String>) -> Option<ExitCode> {
    let fail = |msg: String| {
        eprintln!("tessel: {msg}");
        Some(ExitCode::FAILURE)
    };
    let mut why = String::new();
    let wanted = if let Some(v) = args.first().and_then(|a| a.strip_prefix('+')).map(str::to_string) {
        args.remove(0);
        Some(v)
    } else if std::env::var_os(HANDED_ON).is_some() {
        None
    } else if let Some(v) = std::env::var("TESSEL_VERSION").ok().filter(|v| !v.is_empty()) {
        why = "TESSEL_VERSION asks for".into();
        Some(v)
    } else {
        let file = project(args).and_then(|p| find(&p))?;
        match read(&file) {
            Ok(v) => {
                why = format!("{} asks for", file.display());
                v
            }
            Err(e) => return fail(e),
        }
    };
    let wanted = match wanted.map(|v| update::normalize(&v)) {
        None => return None,
        Some(Ok(v)) => v,
        Some(Err(e)) => return fail(e),
    };
    if wanted.trim_start_matches('v') == tessel_compiler::VERSION {
        return None;
    }
    let exe = match update::installed(&wanted) {
        Ok(exe) => exe,
        Err(e) if why.is_empty() => return fail(format!("can't use Tessel {wanted}: {e}")),
        Err(e) => return fail(format!("{why} Tessel {wanted}, which can't be used: {e}")),
    };
    let mut cmd = Command::new(&exe);
    cmd.args(args.iter()).env(HANDED_ON, &wanted);
    // On macOS the other version takes this process's place (its input and
    // output stay the same, which `lsp` needs); Windows has no such call.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        fail(format!("can't run {}: {e}", exe.display()))
    }
    #[cfg(not(unix))]
    match cmd.status() {
        Ok(status) => Some(ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8)),
        Err(e) => fail(format!("can't run {}: {e}", exe.display())),
    }
}

/// `tessel pin [version]`: writes the version (this one if none is named)
/// into the `tessel.toml` of the current folder, keeping what else it holds.
pub fn run(args: &[String]) -> ExitCode {
    let result = (|| -> Result<String, String> {
        let version = match args.first() {
            Some(v) => update::normalize(v)?,
            None => format!("v{}", tessel_compiler::VERSION),
        };
        // (The project's file, when this is a folder inside the project.)
        let file = find(Path::new(".")).unwrap_or_else(|| PathBuf::from(FILE));
        let file = file.as_path();
        let old = if file.exists() { std::fs::read_to_string(file).map_err(|e| e.to_string())? } else { String::new() };
        let line = format!("tessel = \"{}\"", version.trim_start_matches('v'));
        let mut replaced = false;
        let mut lines: Vec<String> = old
            .lines()
            .map(|l| {
                let key = l.split('#').next().unwrap_or("").split_once('=').map(|(k, _)| k.trim());
                if !replaced && key == Some("tessel") {
                    replaced = true;
                    line.clone()
                } else {
                    l.to_string()
                }
            })
            .collect();
        if !replaced {
            lines.insert(0, line);
        }
        std::fs::write(file, lines.join("\n") + "\n").map_err(|e| format!("can't write {}: {e}", file.display()))?;
        Ok(format!("{version} (in {})", file.display()))
    })();
    match result {
        Ok(v) => {
            println!("This project uses Tessel {v}.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("tessel pin: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version() {
        assert_eq!(parse("tessel = \"0.1.32\"\n"), Ok(Some("v0.1.32".into())));
        assert_eq!(parse("# a comment\ntessel = \"v0.1.2\" # pinned\n"), Ok(Some("v0.1.2".into())));
        assert_eq!(parse("name = \"x\"\n[other]\ntessel = \"0.1.1\"\n"), Ok(None));
        assert_eq!(parse(""), Ok(None));
        assert!(parse("tessel = 0.1.32").is_err());
        assert!(parse("tessel = \"0.1\"").is_err());
    }
}
