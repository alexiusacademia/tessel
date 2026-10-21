//! `tessel test [path] [--filter text] [--timeout seconds]`: runs a
//! program's `test "name" { … }` blocks (see `tessel_compiler::testing`).
//!
//! The program is built once as a test runner, and each test runs in a
//! process of its own, several at a time: a test that crashes or hangs
//! fails alone. A test passes when its process ends normally without a
//! failure on standard error; what a failing test printed is shown with it.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A test, and where it is.
struct Test {
    name: String,
    function: String,
    at: String,
}

/// How a test went.
struct Outcome {
    passed: bool,
    /// From `expect` and `fail`: where, what, the message, and the details.
    failures: Vec<[String; 4]>,
    /// What it printed (its failures left out).
    output: String,
    /// Why it stopped, when it didn't end normally.
    stopped: Option<String>,
    time: Duration,
}

pub fn run(args: &[String]) -> ExitCode {
    let fail = |msg: String| {
        eprintln!("tessel test: {msg}");
        ExitCode::FAILURE
    };
    let opt = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1].clone());
    let mut rest = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")));
    let path = PathBuf::from(rest.next().map_or(".", |(_, a)| a.as_str()));
    let filter = opt("--filter");
    let timeout = match opt("--timeout").map(|t| t.parse::<f64>()) {
        None => 60.0,
        Some(Ok(t)) if t > 0.0 => t,
        _ => return fail("--timeout takes a number of seconds, like --timeout 10".into()),
    };

    tessel_compiler::set_test_mode(true);
    let Ok((mut sources, _)) = crate::load(&path) else { return ExitCode::FAILURE };
    let analysis = tessel_compiler::analyze(&mut sources, false);
    if !analysis.diags.is_empty() {
        crate::report(&analysis.diags.list, &sources);
        return ExitCode::FAILURE;
    }
    let tests: Vec<Test> = analysis
        .program
        .tests
        .iter()
        .filter(|t| filter.as_ref().is_none_or(|f| t.name.to_lowercase().contains(&f.to_lowercase())))
        .map(|t| {
            let file = sources.file(t.span.file & tessel_compiler::source::FILE_MASK);
            let (line, _) = file.line_col(t.span.lo as usize);
            let name = Path::new(&file.name).file_name().map_or(file.name.clone(), |n| n.to_string_lossy().into_owned());
            Test { name: t.name.clone(), function: t.function.clone(), at: format!("{name}:{line}") }
        })
        .collect();
    if tests.is_empty() {
        match &filter {
            Some(f) if !analysis.program.tests.is_empty() => println!("No test's name has \"{f}\" in it."),
            _ => println!("No tests in {}. A test is written `test \"what it checks\" {{ … }}`.", path.display()),
        }
        return ExitCode::SUCCESS;
    }

    let exe = std::env::temp_dir().join(format!("tessel-test-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
    if crate::build(&path, &exe).is_err() {
        return ExitCode::FAILURE;
    }
    // (Once with no test first: the system looks a new program over the
    // first time it starts, which would otherwise hold up every test.)
    let _ = Command::new(&exe).env("TESSEL_TEST", "").stdout(Stdio::null()).stderr(Stdio::null()).status();
    let started = Instant::now();
    let outcomes = run_all(&exe, &tests, Duration::from_secs_f64(timeout));
    let _ = std::fs::remove_file(&exe);

    let failed = outcomes.iter().filter(|o| !o.passed).count();
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    println!(
        "\n{} passed, {} failed, in {:.2} s",
        tests.len() - failed,
        if failed > 0 { paint("1;31", &failed.to_string()) } else { "0".into() },
        started.elapsed().as_secs_f64()
    );
    if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Runs every test, several at a time, printing each one's result in the
/// tests' order as soon as it and those before it are done.
fn run_all(exe: &Path, tests: &[Test], timeout: Duration) -> Vec<Outcome> {
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get()).min(tests.len()).max(1);
    let (tx, rx) = mpsc::channel::<(usize, Outcome)>();
    let next = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let functions: Vec<String> = tests.iter().map(|t| t.function.clone()).collect();
    let mut workers = Vec::new();
    for _ in 0..jobs {
        let (tx, next, functions, exe) = (tx.clone(), next.clone(), functions.clone(), exe.to_path_buf());
        workers.push(std::thread::spawn(move || {
            loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(function) = functions.get(i) else { break };
                if tx.send((i, run_one(&exe, function, timeout))).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);
    let mut done: Vec<Option<Outcome>> = (0..tests.len()).map(|_| None).collect();
    let mut shown = 0;
    for (i, outcome) in rx {
        done[i] = Some(outcome);
        while shown < tests.len() {
            let Some(o) = &done[shown] else { break };
            show(&tests[shown], o);
            shown += 1;
        }
    }
    for w in workers {
        let _ = w.join();
    }
    done.into_iter().map(|o| o.unwrap()).collect()
}

fn run_one(exe: &Path, function: &str, timeout: Duration) -> Outcome {
    let start = Instant::now();
    let child = Command::new(exe)
        .env("TESSEL_TEST", function)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return Outcome { passed: false, failures: Vec::new(), output: String::new(), stopped: Some(format!("couldn't start: {e}")), time: start.elapsed() }
        }
    };
    // (Read as it runs, so a test that prints a lot doesn't block.)
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    };
    let out = read(Box::new(child.stdout.take().unwrap()));
    let err = read(Box::new(child.stderr.take().unwrap()));
    let mut stopped = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                stopped = Some(format!("it took longer than {} s, and was stopped", timeout.as_secs_f64()));
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => {
                stopped = Some(e.to_string());
                break None;
            }
        }
    };
    let time = start.elapsed();
    let mut output = out.join().unwrap_or_default();
    let mut failures = Vec::new();
    for line in err.join().unwrap_or_default().lines() {
        match line.strip_prefix("tessel-test-failure: ").and_then(|j| serde_json::from_str::<[String; 4]>(j).ok()) {
            Some(f) => failures.push(f),
            None => {
                output.push_str(line);
                output.push('\n');
            }
        }
    }
    if let Some(s) = status.filter(|s| !s.success()) {
        stopped = Some(match s.code() {
            Some(code) => format!("it stopped with exit code {code}"),
            None => "it crashed".into(),
        });
    }
    Outcome { passed: stopped.is_none() && failures.is_empty(), failures, output, stopped, time }
}

fn show(t: &Test, o: &Outcome) {
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, s: &str| if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() };
    let ms = o.time.as_secs_f64() * 1000.0;
    if o.passed {
        println!("{}  {}  {}", paint("32", "ok  "), t.name, paint("2", &format!("({ms:.0} ms)")));
        return;
    }
    println!("{}  {}  {}", paint("1;31", "FAIL"), t.name, paint("2", &format!("({}, {ms:.0} ms)", t.at)));
    for [at, what, message, detail] in &o.failures {
        let at = if at.is_empty() { String::new() } else { format!("{at}: ") };
        println!("      {at}{what}");
        if !message.is_empty() {
            println!("        {message}");
        }
        for line in detail.lines() {
            println!("        {line}");
        }
    }
    if let Some(why) = &o.stopped {
        println!("      {why}");
    }
    let output = o.output.trim_end();
    if !output.is_empty() {
        println!("      it printed:");
        for line in output.lines() {
            println!("        {line}");
        }
    }
}
