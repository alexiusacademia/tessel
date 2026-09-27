//! Runs each `tests/run/*.tsl` program with `tessel run` and compares its
//! output with `<name>.out` (stdout) and `<name>.err` (stderr). A program
//! with a `.err` file must fail; one without must succeed with no stderr.
//! On macOS, every successful program is also checked for memory leaks.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
fn run_programs() {
    let dir = repo().join("tests/run");
    let mut failures = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsl"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let rel = format!("tests/run/{name}.tsl");
        let exe = std::env::temp_dir().join(format!("tessel-test-{name}-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
        let built = Command::new(env!("CARGO_BIN_EXE_tessel"))
            .args(["build", &rel, "-o"])
            .arg(&exe)
            .current_dir(repo())
            .output()
            .unwrap();
        if !built.status.success() {
            failures.push(format!(
                "{name}: build failed ({}):\n{}{}",
                built.status,
                String::from_utf8_lossy(&built.stdout),
                String::from_utf8_lossy(&built.stderr)
            ));
            continue;
        }
        let mut cmd = Command::new(&exe);
        cmd.current_dir(repo());
        // UI apps run headless, driven by `<name>.script`.
        if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
            cmd.env("TESSEL_SCRIPT", script);
        }
        let out = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let expected_out = std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap_or_default();
        let expected_err = std::fs::read_to_string(dir.join(format!("{name}.err"))).ok();
        if stdout != expected_out {
            failures.push(format!("{name}: stdout differs\n--- expected\n{expected_out}--- got\n{stdout}"));
        }
        match &expected_err {
            Some(err) if out.status.success() => failures.push(format!("{name}: expected failure, but it succeeded")),
            Some(err) if stderr != *err => {
                failures.push(format!("{name}: stderr differs\n--- expected\n{err}--- got\n{stderr}"))
            }
            None if !out.status.success() || !stderr.is_empty() => {
                failures.push(format!("{name}: failed ({}):\n{stderr}", out.status))
            }
            _ => {}
        }
        if cfg!(target_os = "macos") && expected_err.is_none() {
            let mut leaks = Command::new("leaks");
            leaks.args(["--atExit", "--"]).arg(&exe).current_dir(repo());
            if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
                leaks.env("TESSEL_SCRIPT", script);
            }
            let leaks = leaks.output();
            if let Ok(leaks) = leaks {
                let report = String::from_utf8_lossy(&leaks.stdout);
                if !report.contains(" 0 leaks for 0 total leaked bytes") {
                    let summary: Vec<&str> = report.lines().filter(|l| l.contains("leak")).take(12).collect();
                    failures.push(format!("{name}: memory leaks\n{}", summary.join("\n")));
                }
            }
        }
        let _ = std::fs::remove_file(&exe);
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n\n"));
}
