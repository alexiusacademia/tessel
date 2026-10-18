//! Runs each `tests/run/*.tsl` program with `tessel run` and compares its
//! output with `<name>.out` (stdout) and `<name>.err` (stderr). A program
//! with a `.err` file must fail; one without must succeed with no stderr.
//! On macOS, every successful program is also checked for memory leaks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        // (With TESSEL_TEST_DEBUG set, every program is built as `tessel
        // debug` builds it, and must do the same as otherwise.)
        let how: &[&str] = if std::env::var_os("TESSEL_TEST_DEBUG").is_some() { &["--debug"] } else { &[] };
        let built = Command::new(env!("CARGO_BIN_EXE_tessel"))
            .args(["build", &rel, "-o"])
            .arg(&exe)
            .args(how)
            .current_dir(repo())
            .output()
            .unwrap();
        if !built.status.success() {
            // A program meant not to build (a mistake in its tessel.toml) has
            // the error as its `.err`.
            if let Ok(expected) = std::fs::read_to_string(dir.join(format!("{name}.err"))) {
                let got = String::from_utf8_lossy(&built.stderr).replace('\\', "/").replace("\r\n", "\n");
                if got != expected {
                    failures.push(format!("{name}: the build's error differs\n--- expected\n{expected}--- got\n{got}"));
                }
                continue;
            }
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
        // `<name>.env`: what the program finds in its environment (`NAME=value` lines).
        let env_file = std::fs::read_to_string(dir.join(format!("{name}.env"))).unwrap_or_default();
        let env: Vec<(&str, &str)> = env_file.lines().filter_map(|l| l.trim().split_once('=')).collect();
        cmd.envs(env.iter().copied());
        // UI apps run headless, driven by `<name>.script`.
        if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
            cmd.env("TESSEL_SCRIPT", script);
        }
        // Programs that read input get `<name>.stdin` (and no input otherwise).
        let stdin = std::fs::read(dir.join(format!("{name}.stdin"))).unwrap_or_default();
        let out = run_with_input(&mut cmd, &stdin);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        // (Text is laid out by the system, so a program whose output has
        // text sizes in it may expect `<name>.macos.out` or
        // `<name>.windows.out` instead.)
        let own_out = dir.join(format!("{name}.{}.out", std::env::consts::OS));
        let out_file = if own_out.exists() { own_out } else { dir.join(format!("{name}.out")) };
        let expected_out = std::fs::read_to_string(out_file).unwrap_or_default();
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
            leaks.envs(env.iter().copied());
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
        assert!(
            out.status.success(),
            "{name}: the REPL failed ({})\n--- stdout\n{}--- stderr\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        for (kind, got) in [("out", &out.stdout), ("err", &out.stderr)] {
            let expected = std::fs::read_to_string(dir.join(format!("{name}.{kind}"))).unwrap_or_default();
            // (Paths in messages have the system's separators.)
            let got = String::from_utf8_lossy(got);
            let got = if cfg!(windows) { got.replace('\\', "/") } else { got.to_string() };
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
/// `tessel build --android`: an app as an `.apk` and as an `.aab`. Only
/// where Android's tools and Tessel's runtime for Android are (see
/// `tessel android doctor`): elsewhere there's nothing to test with.
#[test]
fn android_packages() {
    let tessel = env!("CARGO_BIN_EXE_tessel");
    let doctor = Command::new(tessel).args(["android", "doctor"]).output().unwrap();
    let report = String::from_utf8_lossy(&doctor.stdout);
    if !doctor.status.success() || !report.contains("ok       Tessel's runtime for Android") {
        eprintln!("skipped: not set up for Android\n{report}");
        return;
    }
    let dir = std::env::temp_dir().join(format!("tessel-test-android-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(repo().join("tests/run/app_await.tsl"), dir.join("main.tsl")).unwrap();
    std::fs::write(dir.join("tessel.toml"), "[app]\nname = \"Await\"\nid = \"dev.tessel.test-await\"\nversion = \"2.1.0\"\n").unwrap();
    for (flags, file) in [(vec!["--android"], "main.apk"), (vec!["--android", "--bundle"], "main.aab")] {
        let out = Command::new(tessel).arg("build").arg(".").args(&flags).args(["-o", file]).current_dir(&dir).output().unwrap();
        assert!(out.status.success(), "tessel build {flags:?} failed:\n{}", String::from_utf8_lossy(&out.stderr));
        // A zip with the app's library in it (stored whole or compressed,
        // its name is there).
        let bytes = std::fs::read(dir.join(file)).unwrap();
        assert!(bytes.starts_with(b"PK"), "{file} isn't a package");
        let has = |name: &str| bytes.windows(name.len()).any(|w| w == name.as_bytes());
        assert!(has("lib/arm64-v8a/libmain.so"), "{file} has no library");
        assert!(has("AndroidManifest.xml"), "{file} has no manifest");
        assert!(has("classes.dex"), "{file} has no Java part");
        assert!(bytes.len() > 1_000_000, "{file} is only {} bytes", bytes.len());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `tessel debug`: each program in `tests/debug`, driven by the commands in
/// its `.in` file (an app also by its `.script`), prints what its `.out`
/// file says: where it stops, the calls that led there, and its variables.
#[test]
fn debugger_sessions() {
    let dir = repo().join("tests/debug");
    let mut failed = Vec::new();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().to_string_lossy().strip_suffix(".in").map(str::to_string))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in names {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tessel"));
        cmd.current_dir(&dir).args(["debug", &format!("{name}.tsl")]);
        if let Ok(script) = std::fs::read_to_string(dir.join(format!("{name}.script"))) {
            cmd.env("TESSEL_SCRIPT", script);
        }
        let out = run_with_input(&mut cmd, &std::fs::read(dir.join(format!("{name}.in"))).unwrap());
        // (What the program and the debugger print, and the program's errors.)
        let got = format!("{}--- errors\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let expected = std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap_or_default();
        if got != expected {
            std::fs::write(dir.join(format!("{name}.actual")), &got).unwrap();
            failed.push(format!("debugger session `{name}` differs; see tests/debug/{name}.actual\n--- got\n{got}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `tessel debug --dap`: the debugger as editors drive it, with the Debug
/// Adapter Protocol. A breakpoint, the calls there, a variable and its
/// parts, a step, and the program's end.
#[test]
fn debug_adapter() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = repo().join("tests/debug");
    let mut child = Command::new(env!("CARGO_BIN_EXE_tessel"))
        .current_dir(&dir)
        .args(["debug", "--dap"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut to = child.stdin.take().unwrap();
    let mut from = BufReader::new(child.stdout.take().unwrap());
    let mut seq = 0;
    let mut send = |command: &str, arguments: serde_json::Value| {
        seq += 1;
        let body = serde_json::json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments }).to_string();
        write!(to, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        to.flush().unwrap();
    };
    // The next message that's the response to `command`, or the event `event`.
    let mut until = |kind: &str, name: &str| -> serde_json::Value {
        loop {
            let mut length = 0;
            loop {
                let mut header = String::new();
                assert!(from.read_line(&mut header).unwrap() > 0, "the adapter closed before `{name}`");
                match header.trim().strip_prefix("Content-Length:") {
                    Some(n) => length = n.trim().parse().unwrap(),
                    None if header.trim().is_empty() => break,
                    None => {}
                }
            }
            let mut body = vec![0; length];
            from.read_exact(&mut body).unwrap();
            let message: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if message["type"] == kind && (message["command"] == name || message["event"] == name) {
                return message;
            }
        }
    };
    let source = dir.join("values.tsl").canonicalize().unwrap();
    send("initialize", serde_json::json!({ "adapterID": "tessel" }));
    until("event", "initialized");
    send("launch", serde_json::json!({ "program": source }));
    send("setBreakpoints", serde_json::json!({ "source": { "path": source }, "breakpoints": [{ "line": 16 }] }));
    send("configurationDone", serde_json::json!({}));
    assert_eq!(until("event", "stopped")["body"]["reason"], "breakpoint");
    send("stackTrace", serde_json::json!({ "threadId": 1 }));
    let calls = until("response", "stackTrace");
    let calls = calls["body"]["stackFrames"].as_array().unwrap();
    assert_eq!((calls[0]["name"].as_str(), calls[0]["line"].as_i64()), (Some("total"), Some(16)));
    assert_eq!(calls[1]["name"], "main");
    assert_eq!(calls[0]["source"]["path"], serde_json::json!(source));
    send("scopes", serde_json::json!({ "frameId": 0 }));
    let scope = until("response", "scopes")["body"]["scopes"][0]["variablesReference"].clone();
    send("variables", serde_json::json!({ "variablesReference": scope }));
    let vars = until("response", "variables");
    let vars = vars["body"]["variables"].as_array().unwrap();
    let item = vars.iter().find(|v| v["name"] == "item").expect("the loop's variable");
    assert_eq!(item["value"], "Item(name: \"tea\", price: 2.5, tags: [\"hot\", \"drink\"])");
    send("variables", serde_json::json!({ "variablesReference": item["variablesReference"] }));
    let parts = until("response", "variables");
    assert_eq!(parts["body"]["variables"][1]["name"], "price");
    assert_eq!(parts["body"]["variables"][1]["value"], "2.5");
    send("evaluate", serde_json::json!({ "expression": "item.tags[1]", "frameId": 0 }));
    assert_eq!(until("response", "evaluate")["body"]["result"], "\"drink\"");
    send("setBreakpoints", serde_json::json!({ "source": { "path": source }, "breakpoints": [] }));
    send("stepOut", serde_json::json!({ "threadId": 1 }));
    assert_eq!(until("event", "stopped")["body"]["reason"], "step");
    send("continue", serde_json::json!({ "threadId": 1 }));
    let printed = until("event", "output");
    assert!(printed["body"]["output"].as_str().unwrap().starts_with("total 6.5"), "{printed}");
    assert_eq!(until("event", "exited")["body"]["exitCode"], 0);
    send("disconnect", serde_json::json!({}));
    until("response", "disconnect");
    assert!(child.wait().unwrap().success());
}

/// `tessel preview`: the picture of an app, and what it tells an editor
/// about the views in it (see `tests/preview`): from the files as saved,
/// then with text that isn't saved.
#[test]
fn preview() {
    let root = repo();
    let prefix = std::env::temp_dir().join(format!("tessel-test-preview-{}", std::process::id())).display().to_string();
    let mut got = String::new();
    for unsaved in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tessel"));
        cmd.current_dir(&root).args(["preview", "tests/preview/app", "--out", &prefix]);
        if unsaved {
            cmd.args(["--text", "tests/preview/app/main.tsl", "tests/preview/unsaved.txt"]);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "tessel preview failed:\n{}", String::from_utf8_lossy(&out.stderr));
        let read = |ext: &str| {
            let file = format!("{prefix}.{ext}");
            let bytes = std::fs::read(&file).unwrap_or_default();
            let _ = std::fs::remove_file(&file);
            bytes
        };
        assert!(read("png").starts_with(b"\x89PNG"), "no picture");
        let layout: serde_json::Value = serde_json::from_slice(&read("layout.json")).expect("the layout as JSON");
        got += if unsaved { "--- with unsaved text\n" } else { "--- as saved\n" };
        // (Whole points: the last decimal may differ between computers.)
        for n in layout.as_array().unwrap() {
            let at = |key: &str| n[key].as_f64().unwrap().round();
            got += &format!(
                "{}{} [{},{} {}x{}] {}\n",
                "  ".repeat(n["depth"].as_u64().unwrap() as usize),
                n["kind"].as_str().unwrap(),
                at("x"),
                at("y"),
                at("width"),
                at("height"),
                n["source"]
            );
        }
        if !unsaved {
            got += &String::from_utf8_lossy(&read("views.json"));
        } else {
            read("views.json");
        }
    }
    let own = root.join(format!("tests/preview/expected.{}.out", std::env::consts::OS));
    let expected_file = if own.exists() { own } else { root.join("tests/preview/expected.out") };
    let expected = std::fs::read_to_string(expected_file).unwrap_or_default();
    if got != expected {
        std::fs::write(root.join("tests/preview/actual.out"), &got).unwrap();
        panic!("tessel preview differs; see tests/preview/actual.out\n--- got\n{got}");
    }
}

#[test]
fn ide_session() {
    let root = repo();
    let exe = std::env::temp_dir().join(format!("tessel-test-ide-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    let built = Command::new(env!("CARGO_BIN_EXE_tessel")).arg("build").arg(root.join("ide")).arg("-o").arg(&exe).output().unwrap();
    assert!(built.status.success(), "IDE build failed:\n{}{}", String::from_utf8_lossy(&built.stdout), String::from_utf8_lossy(&built.stderr));

    // Each session: a script, run on a copy of a project. `session` edits
    // files; `design` uses the Design panel (and shows the code it wrote).
    let mut failed = Vec::new();
    // `new` starts a project from a template in an empty folder.
    for (name, folder, show) in
        [
            ("session", "project", None),
            ("design", "design", Some("main.tsl")),
            ("new", "empty", Some("README.md")),
            // The debugger: a breakpoint, the calls and variables, stepping.
            ("debug", "debugging", None),
        ]
    {
        let base = std::env::temp_dir().join(format!("tessel-ide-test-{}-{name}", std::process::id()));
        let project = base.join("project");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&project).unwrap();
        for entry in std::fs::read_dir(root.join("tests/ide").join(folder)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), project.join(entry.file_name())).unwrap();
        }
        let script = std::fs::read_to_string(root.join(format!("tests/ide/{name}.script"))).unwrap();
        let out = Command::new(&exe)
            .env("TESSEL_SCRIPT", script)
            .env("TESSEL_PROJECT", &project)
            // (A small documentation of its own, for the Docs panel.)
            .env("TESSEL_DOCS", root.join("tests/ide/docs"))
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
        let mut got = format!("{stdout}--- files: {}\n", files.join(", "));
        if let Some(file) = show {
            got += &format!("--- {file}\n{}", std::fs::read_to_string(project.join(file)).unwrap_or_default());
        }
        let expected = std::fs::read_to_string(root.join(format!("tests/ide/{name}.out"))).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&base);
        if got != expected {
            std::fs::write(root.join(format!("tests/ide/{name}.actual")), &got).unwrap();
            failed.push(format!("IDE session `{name}` differs; see tests/ide/{name}.actual\n--- got\n{got}"));
        }
    }
    let _ = std::fs::remove_file(&exe);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// `tessel --version` says which version a pinned project uses (issue #13).
#[test]
fn version_in_a_pinned_project() {
    let dir = std::env::temp_dir().join(format!("tessel-test-version-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join("home/versions/v0.0.2")).unwrap();
    std::fs::write(dir.join("home/versions/v0.0.2").join(format!("tessel{}", std::env::consts::EXE_SUFFIX)), "").unwrap();
    let version = |pinned: Option<&str>| {
        match pinned {
            Some(v) => std::fs::write(dir.join("tessel.toml"), format!("tessel = \"{v}\"\n")).unwrap(),
            None => drop(std::fs::remove_file(dir.join("tessel.toml"))),
        }
        let out = Command::new(env!("CARGO_BIN_EXE_tessel"))
            .arg("--version")
            .current_dir(dir.join("sub"))
            .env("TESSEL_HOME", dir.join("home"))
            .env_remove("TESSEL_VERSION")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let own = format!("tessel {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(version(None), own);
    assert_eq!(version(Some(env!("CARGO_PKG_VERSION"))), own);
    assert_eq!(version(Some("0.0.1")), format!("{own} (this project uses 0.0.1, not installed yet)"));
    assert_eq!(version(Some("v0.0.2")), format!("{own} (this project uses 0.0.2)"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An app bundle says what its tessel.toml does (macOS).
#[cfg(target_os = "macos")]
#[test]
fn manifest_bundle() {
    let dir = std::env::temp_dir().join(format!("tessel-test-bundle-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_tessel"))
        .current_dir(repo())
        .args(["build", "tests/run/app_manifest", "--bundle", "-o"])
        .arg(dir.join("tally"))
        .output()
        .unwrap();
    assert!(out.status.success(), "build failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let plist = std::fs::read_to_string(dir.join("Tally Pro.app/Contents/Info.plist")).unwrap();
    for line in [
        "<key>CFBundleName</key><string>Tally Pro</string>",
        "<key>CFBundleIdentifier</key><string>com.example.tally</string>",
        "<key>CFBundleShortVersionString</key><string>1.2.0</string>",
        "<key>CFBundleVersion</key><string>42</string>",
        "<key>CFBundleTypeName</key><string>Tally Workbook</string>",
        "<array><string>tly</string></array>",
        "<key>NSHumanReadableCopyright</key><string>© 2026 Alexius Academia</string>",
        "<key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>",
    ] {
        assert!(plist.contains(line), "Info.plist has no `{line}`:\n{plist}");
    }
    assert!(dir.join("Tally Pro.app/Contents/MacOS/Tally Pro").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An app asks macOS for nothing that Apple keeps to itself (the Mac App
/// Store refuses an app that does): the functions it names, of the kind
/// the windowing library once brought in, are all public.
#[test]
fn apps_name_no_private_functions() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let exe = std::env::temp_dir().join(format!("tessel-test-private-{}", std::process::id()));
    let built = Command::new(env!("CARGO_BIN_EXE_tessel"))
        .args(["build", "examples/counter", "-o"])
        .arg(&exe)
        .current_dir(repo())
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let Ok(names) = Command::new("nm").arg("-u").arg(&exe).output() else { return };
    let names = String::from_utf8_lossy(&names.stdout).into_owned();
    let _ = std::fs::remove_file(&exe);
    // (`CGS…`, `CGSMainConnectionID` and its like, is the window server's
    // own interface; `CGShieldingWindowLevel` is `CG`'s, and public.)
    let private: Vec<&str> = names.lines().map(str::trim).filter(|n| n.starts_with("_CGS") && *n != "_CGShieldingWindowLevel").collect();
    assert!(private.is_empty(), "the app names private functions: {private:?}");
}

/// Libraries: publishing two (one using the other) to an index in a
/// folder, using them from a program, and what goes wrong.
#[test]
fn libraries() {
    let dir = std::env::temp_dir().join(format!("tessel-test-libraries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let write = |path: &str, text: &str| {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    std::fs::create_dir_all(dir.join("index/packages")).unwrap();
    let tessel = |folder: &str, args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_tessel"))
            .args(args)
            .current_dir(dir.join(folder))
            .env("TESSEL_HOME", dir.join("home"))
            .env("TESSEL_INDEX", dir.join("index"))
            .env_remove("TESSEL_VERSION")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.success(), text.replace('\\', "/"))
    };
    write("geokit/tessel.toml", "[library]\nname = \"geokit\"\nversion = \"0.3.1\"\ndescription = \"Areas\"\nlicense = \"MIT\"\nrepository = \".\"\n");
    write("geokit/area.tsl", "public fn circleArea(_ r: Float) -> Float {\n    return 3.0 * r * r\n}\n");
    write(
        "textkit/tessel.toml",
        "[library]\nname = \"textkit\"\nversion = \"1.2.0\"\ndescription = \"Text helpers\"\nlicense = \"MIT\"\nrepository = \".\"\n\n[dependencies]\ngeokit = \"0.3\"\n",
    );
    write(
        "textkit/text.tsl",
        "import geokit\n\npublic fn shout(_ s: String) -> String {\n    return s.uppercase() + \"!\"\n}\n\npublic fn area(_ r: Float) -> Float {\n    return geokit.circleArea(r)\n}\n",
    );
    write("app/main.tsl", "import textkit\n\nfn main() {\n    print(textkit.shout(\"hi\"))\n    print(textkit.area(2.0))\n}\n");

    // Publishing: textkit needs geokit published first.
    let (ok, out) = tessel("textkit", &["publish"]);
    assert!(!ok && out.contains("there's no library `geokit` in the index"), "{out}");
    assert!(tessel("geokit", &["publish"]).0);
    let (ok, out) = tessel("textkit", &["publish"]);
    assert!(ok && out.contains("Published textkit 1.2.0"), "{out}");
    let (ok, out) = tessel("textkit", &["publish"]);
    assert!(!ok && out.contains("textkit 1.2.0 is published already"), "{out}");
    let (_, out) = tessel("app", &["search", "text"]);
    assert_eq!(out, "textkit 1.2.0  Text helpers\n");

    // Using them.
    let (ok, out) = tessel("app", &["add", "textkit"]);
    assert!(ok && out.contains("Added textkit 1.2.0"), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("app/tessel.toml")).unwrap(), "[dependencies]\ntextkit = \"1.2\"\n");
    let lock = std::fs::read_to_string(dir.join("app/tessel.lock")).unwrap();
    assert!(lock.contains("name = \"geokit\"") && lock.contains("name = \"textkit\""), "{lock}");
    let (ok, out) = tessel("app", &["run", "."]);
    assert!(ok, "{out}");
    assert_eq!(out, "HI!\n12.0\n");
    let (_, out) = tessel("app", &["deps"]);
    assert_eq!(out, "textkit 1.2.0\n  └─ geokit 0.3.1\n");

    // Only what the program lists is its to import.
    write("app/more.tsl", "import geokit\n\nfn twice() -> Float {\n    return geokit.circleArea(1.0)\n}\n");
    let (ok, out) = tessel("app", &["check", "."]);
    assert!(!ok && out.contains("`geokit` is a library that another library uses, not one this project depends on"), "{out}");
    std::fs::remove_file(dir.join("app/more.tsl")).unwrap();

    // Asking for what doesn't exist changes nothing.
    let (ok, out) = tessel("app", &["add", "geokit@0.2"]);
    assert!(!ok && out.contains("no version of `geokit` is what everyone asks for"), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("app/tessel.toml")).unwrap(), "[dependencies]\ntextkit = \"1.2\"\n");

    // Code changed after publishing isn't used.
    for e in std::fs::read_dir(dir.join("home/packages")).unwrap() {
        std::fs::remove_dir_all(e.unwrap().path()).unwrap();
    }
    write("geokit/area.tsl", "public fn circleArea(_ r: Float) -> Float {\n    return 4.0 * r * r\n}\n");
    let (ok, out) = tessel("app", &["run", "."]);
    assert!(!ok && out.contains("the code of geokit 0.3.1 isn't what was published"), "{out}");

    let (ok, _) = tessel("app", &["remove", "textkit"]);
    assert!(ok && !dir.join("app/tessel.lock").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
