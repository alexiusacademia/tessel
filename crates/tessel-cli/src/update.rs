//! `tessel update`: replaces this installation of Tessel with the latest
//! release (or a chosen version) from the release bucket.
//!
//! A release is `<base>/<version>/tessel-<version>-<platform>.<ext>`, listed in
//! `<base>/<version>/SHA256SUMS`, and `<base>/latest-<platform>.txt` names the
//! latest version. The package holds what goes in the installation folder
//! (the folder `tessel` is in): `tessel`, the runtime library, `sdk/`, and a
//! README. It's unpacked next to the installation first, and only swapped in
//! once it's complete and verified.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use sha2::{Digest, Sha256};

const BASE: &str = "https://storage.googleapis.com/tessel-releases";

/// This platform's name in release file names, and its package extension.
fn platform() -> Option<(&'static str, &'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(("macos", "macos-arm64", "tar.gz")),
        ("windows", "x86_64") => Some(("windows", "windows-x64", "zip")),
        _ => None,
    }
}

fn base() -> String {
    std::env::var("TESSEL_RELEASES").unwrap_or_else(|_| BASE.to_string()).trim_end_matches('/').to_string()
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(600))).build().into()
}

fn get_text(url: &str) -> Result<String, String> {
    let mut r = agent().get(url).call().map_err(|e| format!("can't reach {url}: {e}"))?;
    r.body_mut().read_to_string().map_err(|e| format!("can't read {url}: {e}"))
}

/// `v0.1.2` → (0, 1, 2); anything after the numbers (like `-beta`) is ignored.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split(['.', '-', '+']);
    let mut n = || parts.next()?.parse().ok();
    Some((n()?, n()?, n()?))
}

/// The latest version for this platform, like `v0.1.3`.
pub fn latest() -> Result<String, String> {
    let (os, _, _) = platform().ok_or("updates aren't available for this platform")?;
    let v = get_text(&format!("{}/latest-{os}.txt", base()))?.trim().to_string();
    parse_version(&v).ok_or_else(|| format!("the release list names an odd version: `{v}`"))?;
    Ok(v)
}

/// `tessel update [version] [--check]`.
pub fn run(args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let wanted = args.iter().find(|a| !a.starts_with("--"));
    match update(wanted.map(String::as_str), check) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tessel update: {e}");
            ExitCode::FAILURE
        }
    }
}

fn update(wanted: Option<&str>, check: bool) -> Result<(), String> {
    let current = format!("v{}", tessel_compiler::VERSION);
    let target = match wanted {
        Some(v) => normalize(v)?,
        None => latest()?,
    };
    let newer = parse_version(&target) > parse_version(&current);
    if check {
        if newer {
            println!("Tessel {target} is available (you have {current}). Run `tessel update` to install it.");
        } else {
            println!("Tessel {current} is up to date.");
        }
        return Ok(());
    }
    if target == current {
        println!("Tessel {current} is up to date.");
        return Ok(());
    }
    if wanted.is_none() && !newer {
        println!("Tessel {current} is newer than the latest release ({target}); nothing to do.");
        return Ok(());
    }

    let dir = install_dir()?;
    let parent = dir.parent().ok_or("the installation folder has no parent")?;
    // Left over from earlier updates (on Windows, the old tessel.exe can
    // only be deleted once it has stopped running).
    for e in std::fs::read_dir(parent).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(".tessel-update-") {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let staging = parent.join(format!(".tessel-update-{}", std::process::id()));
    let say = || println!("Updating Tessel {current} → {target} in {}", dir.display());
    let result = fetch(&target, &staging, &say).and_then(|unpacked| swap_in(&unpacked, &dir, &staging));
    let _ = std::fs::remove_dir_all(&staging);
    result?;
    println!("Updated to Tessel {target}.");
    crate::telemetry::installed(&target);
    Ok(())
}

/// Downloads release `target` (like `v0.1.3`) into `staging`, checks it and
/// unpacks it: the folder it returns holds what goes in an installation.
/// `found` is called once the release is found.
fn fetch(target: &str, staging: &Path, found: &dyn Fn()) -> Result<PathBuf, String> {
    let (_, name, ext) = platform().ok_or("Tessel releases aren't made for this platform")?;
    let file = format!("tessel-{target}-{name}.{ext}");
    let base = base();
    // The expected checksum first, so a missing release fails fast.
    let sums = get_text(&format!("{base}/{target}/SHA256SUMS")).map_err(|_| format!("there's no release {target}"))?;
    let expected = sums
        .lines()
        .find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*') == file))
        .map(|(h, _)| h.to_lowercase())
        .ok_or_else(|| format!("release {target} has no package for this platform"))?;
    found();
    let _ = std::fs::remove_dir_all(staging);
    let parent = staging.parent().unwrap_or(staging);
    std::fs::create_dir_all(staging).map_err(|e| writable(parent, e))?;
    let package = staging.join(&file);
    download(&format!("{base}/{target}/{file}"), &package, &expected)?;
    let unpacked = staging.join("new");
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(&package)
        .arg("-C")
        .arg(&unpacked)
        .status()
        .map_err(|e| format!("can't run tar: {e}"))?;
    if !status.success() {
        return Err(format!("can't unpack {file}"));
    }
    if !unpacked.join(exe_name()).is_file() {
        return Err(format!("{file} has no {}", exe_name()));
    }
    Ok(unpacked)
}

/// Downloads a part of release `target` that isn't in its package (`file`,
/// a `.tar.gz`, checked against the release's checksums) and unpacks it
/// into `dir`.
pub fn fetch_part(target: &str, file: &str, dir: &Path) -> Result<(), String> {
    let base = base();
    let sums = get_text(&format!("{base}/{target}/SHA256SUMS")).map_err(|_| format!("there's no release {target}"))?;
    let expected = sums
        .lines()
        .find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*') == file))
        .map(|(h, _)| h.to_lowercase())
        .ok_or_else(|| format!("release {target} has no `{file}`"))?;
    std::fs::create_dir_all(dir).map_err(|e| writable(dir, e))?;
    let package = dir.join(file);
    download(&format!("{base}/{target}/{file}"), &package, &expected)?;
    let status =
        Command::new("tar").arg("-xf").arg(&package).arg("-C").arg(dir).status().map_err(|e| format!("can't run tar: {e}"))?;
    let _ = std::fs::remove_file(&package);
    if !status.success() {
        return Err(format!("can't unpack {file}"));
    }
    Ok(())
}

fn exe_name() -> String {
    format!("tessel{}", std::env::consts::EXE_SUFFIX)
}

/// Whether release `target` (like `v0.1.3`) is installed next to this one
/// (see `installed`, which installs it if not).
pub fn is_installed(target: &str) -> bool {
    home().is_ok_and(|h| h.join("versions").join(target).join(exe_name()).is_file())
}

/// `v0.1.3` for `0.1.3` or `v0.1.3`; an error for what isn't a version.
pub fn normalize(v: &str) -> Result<String, String> {
    let v = v.trim();
    let v = if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
    match parse_version(&v) {
        Some(_) if v.trim_start_matches('v').split('.').count() == 3 => Ok(v),
        _ => Err(format!("`{}` isn't a version; write all three numbers, like 0.1.32", v.trim_start_matches('v'))),
    }
}

/// The folder Tessel lives in: `TESSEL_HOME`, or the one around the `bin`
/// folder this `tessel` is installed in, or where the install scripts put
/// it (`~/.tessel`, or `%LOCALAPPDATA%\Tessel` on Windows).
pub fn home() -> Result<PathBuf, String> {
    if let Some(h) = std::env::var_os("TESSEL_HOME") {
        return Ok(PathBuf::from(h));
    }
    if let Ok(dir) = install_dir() {
        if let (Some(parent), true) = (dir.parent(), dir.file_name().is_some_and(|n| n == "bin")) {
            return Ok(parent.to_path_buf());
        }
    }
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        var("LOCALAPPDATA").map(|d| d.join("Tessel")).ok_or_else(|| "can't find the LOCALAPPDATA folder".into())
    } else {
        var("HOME").map(|d| d.join(".tessel")).ok_or_else(|| "can't find the home folder".into())
    }
}

/// The `tessel` of version `target` (like `v0.1.3`), installed next to this
/// one (in `versions/` of Tessel's folder) the first time it's asked for.
pub fn installed(target: &str) -> Result<PathBuf, String> {
    let versions = home()?.join("versions");
    let dir = versions.join(target);
    let exe = dir.join(exe_name());
    if exe.is_file() {
        return Ok(exe);
    }
    let staging = versions.join(format!(".installing-{}", std::process::id()));
    // (On stderr: `tessel lsp`'s output is for its editor.)
    let say = || eprintln!("Installing Tessel {target} into {}", dir.display());
    let result = fetch(target, &staging, &say).and_then(|unpacked| {
        // (Another `tessel` may have installed it meanwhile: that's as good.)
        if std::fs::rename(&unpacked, &dir).is_err() && !exe.is_file() {
            return Err(format!("can't install Tessel {target} into {}", dir.display()));
        }
        Ok(exe.clone())
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn writable(dir: &Path, e: std::io::Error) -> String {
    format!("can't write to {}: {e}", dir.display())
}

/// The folder this `tessel` is installed in.
fn install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("can't find tessel itself: {e}"))?;
    let dir = exe.parent().ok_or("can't find tessel's folder")?.to_path_buf();
    // A development build (target/debug or target/release) isn't an installation.
    let runtime = if cfg!(windows) { "tessel_runtime.lib" } else { "libtessel_runtime.a" };
    if std::env::var_os("TESSEL_UPDATE_ANY_FOLDER").is_none() && (dir.join("deps").is_dir() || !dir.join(runtime).is_file()) {
        return Err(format!(
            "{} isn't an installed Tessel (it looks like a development build); install with the install script instead",
            dir.display()
        ));
    }
    Ok(dir)
}

/// Downloads `url` to `to`, showing progress, and checks its SHA-256.
fn download(url: &str, to: &Path, expected: &str) -> Result<(), String> {
    use std::io::Write;
    let mut r = agent().get(url).call().map_err(|e| format!("can't download {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("can't download {url}: status {}", r.status()));
    }
    let total: Option<u64> = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let mut reader = r.body_mut().with_config().limit(u64::MAX).reader();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut shown) = (0u64, u64::MAX);
    let tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if let (true, Some(total)) = (tty, total) {
            let pct = done * 100 / total.max(1);
            if pct != shown {
                shown = pct;
                eprint!("\r  downloading: {pct:3}% of {:.0} MB", total as f64 / 1e6);
            }
        }
    }
    if tty && total.is_some() {
        eprintln!();
    }
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != expected {
        return Err(format!("the download is damaged (checksum {got}, expected {expected}); nothing was changed"));
    }
    Ok(())
}

/// Moves everything in `new` into `dir`, replacing what's there. Old files
/// go to `trash` first (a running `tessel.exe` can be renamed on Windows, not
/// replaced); if a move fails, what was moved is put back.
fn swap_in(new: &Path, dir: &Path, trash: &Path) -> Result<(), String> {
    let old = trash.join("old");
    std::fs::create_dir_all(&old).map_err(|e| e.to_string())?;
    let entries: Vec<PathBuf> =
        std::fs::read_dir(new).map_err(|e| e.to_string())?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for entry in &entries {
        let name = entry.file_name().unwrap();
        let target = dir.join(name);
        let backup = old.join(name);
        let moved_away = target.exists() && std::fs::rename(&target, &backup).is_ok();
        if target.exists() {
            undo(&done);
            return Err(format!("can't replace {} (is it open in another program?)", target.display()));
        }
        if let Err(e) = std::fs::rename(entry, &target) {
            if moved_away {
                let _ = std::fs::rename(&backup, &target);
            }
            undo(&done);
            return Err(writable(dir, e));
        }
        done.push((target, moved_away.then_some(backup)));
    }
    Ok(())
}

fn undo(done: &[(PathBuf, Option<PathBuf>)]) {
    for (target, backup) in done.iter().rev() {
        if std::fs::remove_dir_all(target).is_err() {
            let _ = std::fs::remove_file(target);
        }
        if let Some(b) = backup {
            let _ = std::fs::rename(b, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(parse_version("v0.1.10") > parse_version("v0.1.9"));
        assert!(parse_version("0.2.0") > parse_version("v0.1.99"));
        assert_eq!(parse_version("v1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("latest"), None);
    }
}
