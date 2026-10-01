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
        // A program is a file, or a folder of files (with modules in it).
        .filter(|p| p.extension().is_some_and(|e| e == "tsl") || p.join("main.tsl").is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let rel = if path.is_dir() { format!("tests/run/{name}") } else { format!("tests/run/{name}.tsl") };
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
        cmd.current_dir(repo()).env("TESSEL_EXE", env!("CARGO_BIN_EXE_tessel"));
        // UI apps run headless, driven by `<name>.script`.
        if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
            cmd.env("TESSEL_SCRIPT", script);
        }
        // Programs that read input get `<name>.stdin` (and no input otherwise).
        let stdin = std::fs::read(dir.join(format!("{name}.stdin"))).unwrap_or_default();
        let out = run_with_input(&mut cmd, &stdin);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let expected_out = std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap_or_default();
        let expected_err = std::fs::read_to_string(dir.join(format!("{name}.err"))).ok();
        // `<name>.stderr`: what a program that succeeds writes to stderr.
        let expected_stderr = std::fs::read_to_string(dir.join(format!("{name}.stderr"))).unwrap_or_default();
        if stdout != expected_out {
            failures.push(format!("{name}: stdout differs\n--- expected\n{expected_out}--- got\n{stdout}"));
        }
        match &expected_err {
            Some(err) if out.status.success() => failures.push(format!("{name}: expected failure, but it succeeded")),
            Some(err) if stderr != *err => {
                failures.push(format!("{name}: stderr differs\n--- expected\n{err}--- got\n{stderr}"))
            }
            None if !out.status.success() => failures.push(format!("{name}: failed ({}):\n{stderr}", out.status)),
            None if stderr != expected_stderr => {
                failures.push(format!("{name}: stderr differs\n--- expected\n{expected_stderr}--- got\n{stderr}"))
            }
            _ => {}
        }
        if cfg!(target_os = "macos") && expected_err.is_none() {
            let mut leaks = Command::new("leaks");
            leaks.args(["--atExit", "--"]).arg(&exe).current_dir(repo());
            if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
                leaks.env("TESSEL_SCRIPT", script);
            }
            let leaks = Some(run_with_input(&mut leaks, &stdin)).ok_or(());
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

/// Runs `cmd` with `input` on its standard input.
fn run_with_input(cmd: &mut Command, input: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    out
}

/// Feeds `tests/repl/<name>.in` to `tessel repl` and compares what it prints
/// with `<name>.out` (stdout) and `<name>.err` (stderr).
#[test]
fn repl_sessions() {
    let dir = repo().join("tests/repl");
    let mut inputs: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "in")).collect();
    inputs.sort();
    assert!(!inputs.is_empty());
    for input in inputs {
        let name = input.file_stem().unwrap().to_string_lossy().to_string();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tessel"));
        cmd.arg("repl").current_dir(repo());
        let out = run_with_input(&mut cmd, &std::fs::read(&input).unwrap());
        assert!(out.status.success(), "{name}: the REPL failed ({})", out.status);
        for (kind, got) in [("out", &out.stdout), ("err", &out.stderr)] {
            let expected = std::fs::read_to_string(dir.join(format!("{name}.{kind}"))).unwrap_or_default();
            let got = String::from_utf8_lossy(got);
            assert!(got == expected, "{name}: std{kind} differs\n--- expected\n{expected}--- got\n{got}");
        }
    }
}

/// Runs `tests/net/servers.tsl` against two servers on this computer: one
/// that sends a file very slowly (for cancelling a download and a request),
/// and a WebSocket server that echoes (for binary messages).
#[test]
fn network_with_servers() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let slow = TcpListener::bind("127.0.0.1:0").unwrap();
    let slow_port = slow.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in slow.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4000000\r\nContent-Type: application/octet-stream\r\n\r\n");
                // 20 KB every 50 ms: over three minutes, unless the other side gives up.
                let piece = [7u8; 20_000];
                for _ in 0..200 {
                    if stream.write_all(&piece).is_err() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            });
        }
    });
    let echo = TcpListener::bind("127.0.0.1:0").unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in echo.incoming().flatten() {
            std::thread::spawn(move || {
                let Ok(mut socket) = tungstenite::accept(stream) else { return };
                while let Ok(message) = socket.read() {
                    if (message.is_text() || message.is_binary()) && socket.send(message).is_err() {
                        break;
                    }
                }
            });
        }
    });

    let started = std::time::Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_tessel"))
        .args(["run", "tests/net/servers.tsl"])
        .current_dir(repo())
        .env("SLOW_PORT", slow_port.to_string())
        .env("ECHO_PORT", echo_port.to_string())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "failed ({}):\n{stdout}{}", out.status, String::from_utf8_lossy(&out.stderr));
    // (The order of the lines depends on timing.)
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    let expected = [
        "closed: closed",
        "data: [0, 1, 254, 255]",
        "download under way: true",
        "file: false, part: false",
        "sent: true true",
        "text: hello",
    ];
    assert_eq!(lines, expected, "got:\n{stdout}");
    // Cancelled, the program doesn't wait for the slow server.
    assert!(started.elapsed() < std::time::Duration::from_secs(60), "took {:?}", started.elapsed());
}

/// Runs a copy of `tests/hot` with `tessel run --hot` and its script: the app
/// replaces its own source, and `reload` gives it the new code.
#[test]
fn hot_reload() {
    let from = repo().join("tests/hot");
    let dir = std::env::temp_dir().join(format!("tessel-hot-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["main.tsl", "next.txt", "broken.txt", "last.txt"] {
        std::fs::copy(from.join(name), dir.join(name)).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_tessel"))
        .args(["run", "main.tsl", "--hot"])
        .current_dir(&dir)
        .env("TESSEL_SCRIPT", std::fs::read_to_string(from.join("session.script")).unwrap())
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "failed ({}):\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    for (kind, got) in [("out", &out.stdout), ("err", &out.stderr)] {
        let expected = std::fs::read_to_string(from.join(format!("session.{kind}"))).unwrap();
        let got = String::from_utf8_lossy(got);
        assert!(got == expected, "std{kind} differs\n--- expected\n{expected}--- got\n{got}");
    }
}

/// Builds the IDE (`ide/`) and drives it through `tests/ide/session.script`
/// on a copy of `tests/ide/project`, then compares its output and the
/// project's files with what's expected.
#[test]
fn ide_session() {
    let root = repo();
    let exe = std::env::temp_dir().join(format!("tessel-test-ide-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    let built = Command::new(env!("CARGO_BIN_EXE_tessel")).arg("build").arg(root.join("ide")).arg("-o").arg(&exe).output().unwrap();
    assert!(built.status.success(), "IDE build failed:\n{}{}", String::from_utf8_lossy(&built.stdout), String::from_utf8_lossy(&built.stderr));

    let base = std::env::temp_dir().join(format!("tessel-ide-test-{}", std::process::id()));
    let project = base.join("project");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&project).unwrap();
    for entry in std::fs::read_dir(root.join("tests/ide/project")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), project.join(entry.file_name())).unwrap();
    }
    let script = std::fs::read_to_string(root.join("tests/ide/session.script")).unwrap();
    let out = Command::new(&exe)
        .env("TESSEL_SCRIPT", script)
        .env("TESSEL_PROJECT", &project)
        .env("TESSEL_EXE", env!("CARGO_BIN_EXE_tessel"))
        .env("TESSEL_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "IDE failed:\n{}", String::from_utf8_lossy(&out.stderr));
    // Paths in the output depend on where the project was copied.
    let stdout = String::from_utf8_lossy(&out.stdout).replace(&project.display().to_string(), "PROJECT");
    let mut files: Vec<String> =
        std::fs::read_dir(&project).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    files.sort();
    let got = format!("{stdout}--- files: {}\n", files.join(", "));
    let expected = std::fs::read_to_string(root.join("tests/ide/session.out")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&exe);
    if got != expected {
        std::fs::write(root.join("tests/ide/session.actual"), &got).unwrap();
        panic!("IDE session differs; see tests/ide/session.actual\n--- got\n{got}");
    }
}
