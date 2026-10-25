//! `tessel debug`: runs a program in the debugger.
//!
//! The program is built so that it says where it is as it runs (`tessel
//! build --debug`), and its runtime stops it where it's asked to (the
//! runtime's `debug`, which also describes what's said between the two).
//! This starts the program, listens for it, and is one of three faces to
//! it:
//!
//! - typed commands in a terminal (`terminal`);
//! - lines for an editor to write and read (`--lines`, see `lines`), which
//!   the Tessel IDE uses;
//! - the Debug Adapter Protocol (`--dap`, see `dap`), which VS Code and
//!   other editors speak.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

/// What reaches the debugger: from the program, or from whoever drives it.
enum Message {
    /// The program stopped: why, and its calls, innermost first.
    Stopped { reason: String, message: Option<String>, frames: Vec<Frame> },
    Running,
    /// The answer to a question (`Session::ask`), by the question's number.
    Reply(Value),
    /// A line the program printed.
    Output(String),
    Exited(i32),
    /// A line (or, for `dap`, a message) from whoever drives the debugger.
    Input(String),
    /// No more input: the terminal or the editor is gone.
    Closed,
}

#[derive(Clone)]
struct Frame {
    name: String,
    file: String,
    line: i64,
}

/// A program being debugged.
struct Session {
    child: Arc<Mutex<Child>>,
    to_program: std::net::TcpStream,
    exe: PathBuf,
    /// The program's files: as the program names them, and where they are.
    files: Vec<(String, PathBuf)>,
    asked: u64,
}

impl Session {
    /// Builds the program at `path` for the debugger and starts it. It
    /// waits to be told to run. What it prints (when `piped`), and what
    /// happens to it, is sent to `to`; `said` gets what building it printed.
    fn start(path: &Path, piped: bool, to: Sender<Message>, said: &mut dyn FnMut(String)) -> Result<Session, String> {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = std::env::temp_dir().join(format!("tessel-debug-{}{}", std::process::id(), std::env::consts::EXE_SUFFIX));
        let mut build = Command::new(&me);
        build.arg("build").arg(path).arg("--debug").arg("-o").arg(&exe);
        if piped {
            let output = build.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
            for line in String::from_utf8_lossy(&output.stderr).lines() {
                said(line.to_string());
            }
            if !output.status.success() {
                return Err("the program has errors".into());
            }
        } else if !build.stdout(Stdio::null()).status().map_err(|e| e.to_string())?.success() {
            return Err(String::new());
        }
        let files = crate::program_files(path)
            .unwrap_or_default()
            .into_iter()
            .map(|f| (f.display().to_string(), f.canonicalize().unwrap_or(f)))
            .collect();

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let mut program = Command::new(&exe);
        program.env("TESSEL_DEBUG", port.to_string()).env("TESSEL_EXE", &me).stdin(Stdio::null());
        crate::program_env(&mut program, &exe, path);
        if piped {
            program.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = program.spawn().map_err(|e| format!("can't start the program: {e}"))?;
        let pipes: Vec<Box<dyn Read + Send>> = [
            child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .collect();

        // The program connects as it starts.
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let waited = std::time::Instant::now();
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) if waited.elapsed().as_secs() < 20 && child.try_wait().ok().flatten().is_none() => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = std::fs::remove_file(&exe);
                    return Err("the program didn't start".into());
                }
            }
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let from_program = stream.try_clone().map_err(|e| e.to_string())?;

        let events = to.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(from_program).lines().map_while(Result::ok) {
                let Ok(said) = serde_json::from_str::<Value>(&line) else { continue };
                let message = match said["event"].as_str() {
                    Some("stopped") => Message::Stopped {
                        reason: said["reason"].as_str().unwrap_or_default().to_string(),
                        message: said["message"].as_str().map(str::to_string),
                        frames: said["frames"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|f| Frame {
                                name: f["name"].as_str().unwrap_or_default().to_string(),
                                file: f["file"].as_str().unwrap_or_default().to_string(),
                                line: f["line"].as_i64().unwrap_or(0),
                            })
                            .collect(),
                    },
                    Some("running") => Message::Running,
                    Some(_) => continue,
                    None => Message::Reply(said),
                };
                if events.send(message).is_err() {
                    break;
                }
            }
        });
        let readers: Vec<_> = pipes
            .into_iter()
            .map(|pipe| {
                let output = to.clone();
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                        let _ = output.send(Message::Output(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string()));
                    }
                })
            })
            .collect();
        let child = Arc::new(Mutex::new(child));
        let waiting = child.clone();
        std::thread::spawn(move || {
            let code = loop {
                if let Ok(Some(status)) = waiting.lock().unwrap().try_wait() {
                    break status.code().unwrap_or(1);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            };
            // (What it printed last comes before the news that it ended.)
            for reader in readers {
                let _ = reader.join();
            }
            let _ = to.send(Message::Exited(code));
        });
        Ok(Session { child, to_program: stream, exe, files, asked: 0 })
    }

    fn tell(&mut self, message: Value) {
        let _ = writeln!(self.to_program, "{message}");
        let _ = self.to_program.flush();
    }

    /// Asks the (stopped) program something; the answer comes as a
    /// `Message::Reply` with the number this gives.
    fn ask(&mut self, mut question: Value) -> u64 {
        self.asked += 1;
        question["id"] = json!(self.asked);
        self.tell(question);
        self.asked
    }

    /// The name the program knows a file by: `given` may be a path to it,
    /// or just its name.
    fn source(&self, given: &str) -> String {
        let path = Path::new(given);
        let full = path.canonicalize().ok();
        let by_path = self.files.iter().find(|(_, at)| Some(at) == full.as_ref());
        let by_name = || {
            let mut named = self.files.iter().filter(|(_, at)| at.file_name() == path.file_name());
            named.next().filter(|_| named.next().is_none())
        };
        by_path.or_else(by_name).map_or(given.to_string(), |(name, _)| name.clone())
    }

    /// Where the file the program calls `name` is. (A full path stays as
    /// it's spelled: an editor that gave the program's path that way knows
    /// its files by it, and another spelling of the same place, through a
    /// link, would be another file to it.)
    fn path_of(&self, name: &str) -> String {
        if Path::new(name).is_absolute() {
            return name.to_string();
        }
        let known = self.files.iter().find(|(n, _)| n == name).map(|(_, at)| at.clone());
        known.or_else(|| Path::new(name).canonicalize().ok()).map_or(name.to_string(), |p| p.display().to_string())
    }

    fn breakpoints(&mut self, file: &str, lines: &BTreeSet<i64>) {
        self.tell(json!({ "cmd": "breakpoints", "file": file, "lines": lines }));
    }

    fn end(&mut self) {
        let _ = self.child.lock().unwrap().kill();
        let _ = std::fs::remove_file(&self.exe);
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let path = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from);
    match (path, flag("--dap"), flag("--lines")) {
        (_, true, _) => dap(),
        (Some(path), _, true) => lines(&path),
        (Some(path), ..) => terminal(&path),
        (None, ..) => {
            eprintln!("usage: tessel debug <path>");
            ExitCode::FAILURE
        }
    }
}

/// Reads lines from this process's input into `to`.
fn read_input(to: Sender<Message>) {
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if to.send(Message::Input(line)).is_err() {
                return;
            }
        }
        let _ = to.send(Message::Closed);
    });
}

// ===== in a terminal =====

const HELP: &str = "commands (the first letter is enough for most):
  break <line>          stop at a line (of another file: break other.tsl:12)
  delete <line>         don't stop there any more (delete all: every line)
  breakpoints           list the lines to stop at
  run, continue         start the program, or let it go on
  next                  run this line, and stop at the next one
  step                  like next, but stop inside a function this line calls
  out                   go on until this function returns
  where                 the calls that led here
  up, down, frame <n>   look at another of those calls
  locals                the variables here
  print <variable>      one variable, or a part of one: print order.items[0].name
  quit                  end the program and the debugger";

/// The variables in a reply, as lines of text.
fn show_vars(reply: &Value, indent: &str) -> Vec<String> {
    let vars = reply["vars"].as_array().cloned().unwrap_or_default();
    vars.iter()
        .map(|v| {
            let (name, ty, value) = (v["name"].as_str().unwrap_or(""), v["type"].as_str().unwrap_or(""), v["value"].as_str().unwrap_or(""));
            format!("{indent}{name}: {ty} = {value}")
        })
        .collect()
}

fn terminal(path: &Path) -> ExitCode {
    let (to, from): (Sender<Message>, Receiver<Message>) = std::sync::mpsc::channel();
    let mut session = match Session::start(path, false, to, &mut |_| {}) {
        Ok(session) => session,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("tessel: {e}");
            }
            return ExitCode::FAILURE;
        }
    };
    let interactive = std::io::stdin().is_terminal();
    if interactive {
        println!("The program is ready. `break <line>` says where to stop, `run` starts it, `help` lists the rest.");
    }
    let main_file = session.files.first().map(|(name, _)| name.clone()).unwrap_or_default();
    let mut breakpoints: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    // Where the program is stopped (none: it hasn't started, or has ended),
    // and which of its calls is looked at.
    let mut stopped: Option<Vec<Frame>> = None;
    let (mut started, mut ended, mut looking) = (false, None::<i32>, 0usize);

    // Waits for the program to stop or end, and says which.
    let wait = |session: &mut Session, stopped: &mut Option<Vec<Frame>>, ended: &mut Option<i32>| {
        *stopped = None;
        for message in &from {
            match message {
                Message::Stopped { reason, frames, .. } => {
                    let Some(top) = frames.first() else { continue };
                    let why = match reason.as_str() {
                        "breakpoint" => "at a breakpoint",
                        "error" => "where the error happened",
                        "pause" => "paused",
                        _ => "stopped",
                    };
                    println!("{why}: {}:{} in {}", top.file, top.line, top.name);
                    let text = std::fs::read_to_string(session.path_of(&top.file)).unwrap_or_default();
                    if let Some(code) = text.lines().nth((top.line - 1).max(0) as usize) {
                        println!("{:>5} | {code}", top.line);
                    }
                    *stopped = Some(frames);
                    return;
                }
                Message::Exited(code) => {
                    println!("the program ended{}", if code == 0 { String::new() } else { format!(" (with code {code})") });
                    *ended = Some(code);
                    return;
                }
                _ => {}
            }
        }
    };
    // The answer to a question asked of the stopped program.
    let answer = |id: u64| -> Value {
        for message in &from {
            match message {
                Message::Reply(reply) if reply["reply"] == json!(id) => return reply,
                Message::Exited(_) => break,
                _ => {}
            }
        }
        Value::Null
    };

    let mut input = std::io::stdin().lock();
    loop {
        if interactive {
            print!("(tessel) ");
            let _ = std::io::stdout().flush();
        }
        let mut line = String::new();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim();
        let (command, rest) = line.split_once(' ').map_or((line, ""), |(c, r)| (c, r.trim()));
        // `12`, or `file.tsl:12`.
        let place = |session: &Session, text: &str| -> Option<(String, i64)> {
            let (file, line) = match text.rsplit_once(':') {
                Some((file, line)) => (session.source(file), line),
                None => (stopped.as_ref().and_then(|f| f.get(looking)).map_or(main_file.clone(), |f| f.file.clone()), text),
            };
            Some((file, line.trim().parse().ok()?))
        };
        let running = started && stopped.is_none() && ended.is_none();
        match command {
            "" => {}
            "help" | "h" | "?" => println!("{HELP}"),
            "quit" | "q" | "exit" => break,
            "break" | "b" => match place(&session, rest) {
                Some((file, line)) => {
                    let lines = breakpoints.entry(file.clone()).or_default();
                    lines.insert(line);
                    session.breakpoints(&file, lines);
                    println!("will stop at {file}:{line}");
                }
                None => println!("which line? `break 12`, or `break other.tsl:12`"),
            },
            "delete" | "d" | "clear" if rest == "all" || rest.is_empty() => {
                for (file, lines) in breakpoints.iter_mut() {
                    lines.clear();
                    session.breakpoints(file, lines);
                }
                println!("no breakpoints left");
            }
            "delete" | "d" | "clear" => match place(&session, rest) {
                Some((file, line)) => {
                    let lines = breakpoints.entry(file.clone()).or_default();
                    lines.remove(&line);
                    session.breakpoints(&file, lines);
                    println!("won't stop at {file}:{line}");
                }
                None => println!("which line? `delete 12`, or `delete all`"),
            },
            "breakpoints" => {
                let all: Vec<String> = breakpoints.iter().flat_map(|(f, lines)| lines.iter().map(move |l| format!("{f}:{l}"))).collect();
                println!("{}", if all.is_empty() { "no breakpoints".to_string() } else { all.join("\n") });
            }
            _ if ended.is_some() => println!("the program has ended; `quit`, and start it again"),
            "run" | "r" | "continue" | "c" | "next" | "n" | "step" | "s" | "out" | "o" | "finish" => {
                let how = match command {
                    _ if !started => "run",
                    "next" | "n" => "next",
                    "step" | "s" => "step",
                    "out" | "o" | "finish" => "out",
                    _ => "continue",
                };
                started = true;
                looking = 0;
                session.tell(json!({ "cmd": how }));
                wait(&mut session, &mut stopped, &mut ended);
            }
            _ if !started => println!("the program hasn't started: `run` starts it"),
            _ if running => {}
            "where" | "w" | "bt" | "backtrace" => {
                for (i, f) in stopped.iter().flatten().enumerate() {
                    println!("{} {i}: {} at {}:{}", if i == looking { "*" } else { " " }, f.name, f.file, f.line);
                }
            }
            "up" | "down" | "frame" | "f" => {
                let count = stopped.as_ref().map_or(0, Vec::len);
                let to = match command {
                    "up" => Some(looking + 1),
                    "down" => looking.checked_sub(1),
                    _ => rest.parse().ok(),
                };
                match to.filter(|n| *n < count) {
                    Some(n) => {
                        looking = n;
                        let f = &stopped.as_ref().unwrap()[n];
                        println!("{n}: {} at {}:{}", f.name, f.file, f.line);
                    }
                    None => println!("there's no such call (see `where`)"),
                }
            }
            "locals" | "l" | "vars" => {
                let id = session.ask(json!({ "cmd": "vars", "frame": looking }));
                let lines = show_vars(&answer(id), "");
                println!("{}", if lines.is_empty() { "no variables here".to_string() } else { lines.join("\n") });
            }
            "print" | "p" => {
                let id = session.ask(json!({ "cmd": "eval", "frame": looking, "expr": rest }));
                let reply = answer(id);
                match reply["vars"].get(0) {
                    Some(v) => {
                        println!("{}", show_vars(&reply, "").join("\n"));
                        // Its parts, one level down.
                        if let Some(parts) = v["ref"].as_u64().filter(|r| *r > 0) {
                            let id = session.ask(json!({ "cmd": "children", "ref": parts, "count": 50 }));
                            let lines = show_vars(&answer(id), "  ");
                            println!("{}", lines.join("\n"));
                            if v["count"].as_u64().unwrap_or(0) > 50 {
                                println!("  … ({} in all)", v["count"]);
                            }
                        }
                    }
                    None => println!("there's no `{rest}` here (see `locals`)"),
                }
            }
            _ => println!("`{command}` isn't a command (see `help`)"),
        }
    }
    session.end();
    ExitCode::from(ended.unwrap_or(0) as u8)
}

// ===== for an editor, in lines =====

/// `tessel debug <path> --lines`: the debugger for an editor that writes
/// commands to it and reads what happens, a line at a time, with tabs
/// between a line's parts.
///
/// Commands: `breakpoints <file> <line,line,…>` (all the lines to stop at in
/// a file), `run` (once, to start), `continue`, `next`, `step`, `out`,
/// `pause`, `vars <call>`, `children <ref>`, `eval <call> <expression>`,
/// `stop`.
///
/// What it prints:
///
/// - `ready` once the program can be started; `failed` if it has errors
/// - `output <text>`: a line the program printed
/// - `frames`, then for each call (innermost first) `frame <n> <function>
///   <file> <line>`, then `stopped <reason> <message>`
/// - `running`, and `exited <code>`
/// - after `vars`, `children` and `eval`: `vars <what was asked>`, then
///   `var <name> <type> <value> <ref> <count>` for each (a `ref` that isn't
///   0 has parts to ask for with `children`), then `end`
fn lines(path: &Path) -> ExitCode {
    let (to, from): (Sender<Message>, Receiver<Message>) = std::sync::mpsc::channel();
    let say = |line: String| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    };
    let mut session = match Session::start(path, true, to.clone(), &mut |line| say(format!("output\t{line}"))) {
        Ok(session) => session,
        Err(_) => {
            say("failed".into());
            return ExitCode::FAILURE;
        }
    };
    say("ready".into());
    read_input(to);
    // The questions asked of the program: what each was, by its number.
    let mut asked: HashMap<u64, String> = HashMap::new();
    let mut code = 0;
    for message in from {
        match message {
            Message::Output(text) => say(format!("output\t{text}")),
            Message::Running => say("running".into()),
            Message::Stopped { reason, message, frames } => {
                say("frames".into());
                for (i, f) in frames.iter().enumerate() {
                    say(format!("frame\t{i}\t{}\t{}\t{}", f.name, session.path_of(&f.file), f.line));
                }
                say(format!("stopped\t{reason}\t{}", message.unwrap_or_default().replace(['\t', '\n'], " ")));
            }
            Message::Reply(reply) => {
                let what = reply["reply"].as_u64().and_then(|id| asked.remove(&id)).unwrap_or_default();
                say(format!("vars\t{what}"));
                for v in reply["vars"].as_array().into_iter().flatten() {
                    let part = |key: &str| v[key].as_str().unwrap_or_default().replace(['\t', '\n'], " ");
                    say(format!("var\t{}\t{}\t{}\t{}\t{}", part("name"), part("type"), part("value"), v["ref"], v["count"]));
                }
                say("end".into());
            }
            Message::Exited(status) => {
                say(format!("exited\t{status}"));
                code = status;
                break;
            }
            Message::Closed => break,
            Message::Input(line) => {
                let mut parts = line.split('\t');
                let command = parts.next().unwrap_or_default();
                let mut next = || parts.next().unwrap_or_default().to_string();
                match command {
                    "breakpoints" => {
                        let (file, lines) = (session.source(&next()), next());
                        session.breakpoints(&file, &lines.split(',').filter_map(|l| l.trim().parse().ok()).collect());
                    }
                    "run" | "continue" | "next" | "step" | "out" | "pause" => session.tell(json!({ "cmd": command })),
                    "vars" => {
                        let call = next();
                        let id = session.ask(json!({ "cmd": "vars", "frame": call.parse::<u64>().unwrap_or(0) }));
                        asked.insert(id, call);
                    }
                    "children" => {
                        let parts = next();
                        let id = session.ask(json!({ "cmd": "children", "ref": parts.parse::<u64>().unwrap_or(0), "count": 500 }));
                        asked.insert(id, parts);
                    }
                    "eval" => {
                        let (call, expression) = (next(), next());
                        let id = session.ask(json!({ "cmd": "eval", "frame": call.parse::<u64>().unwrap_or(0), "expr": expression }));
                        asked.insert(id, expression);
                    }
                    "stop" => break,
                    _ => {}
                }
            }
        }
    }
    session.end();
    ExitCode::from(code as u8)
}

// ===== the Debug Adapter Protocol =====

/// Variables' numbers for the editor: a call's variables are this plus the
/// call's number; a value's parts are the program's own number for them.
const LOCALS: u64 = 1_000_000;

/// `tessel debug --dap`: the debugger as an adapter of the Debug Adapter
/// Protocol, on this process's input and output. The editor's `launch`
/// request says which program: `{ "program": "<file or folder>" }`.
fn dap() -> ExitCode {
    let (to, from): (Sender<Message>, Receiver<Message>) = std::sync::mpsc::channel();
    // Each message is a header (`Content-Length: n`), a blank line, and n
    // bytes of JSON.
    let input = to.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            let mut length = 0;
            loop {
                let mut header = String::new();
                if stdin.read_line(&mut header).unwrap_or(0) == 0 {
                    let _ = input.send(Message::Closed);
                    return;
                }
                let header = header.trim();
                if header.is_empty() {
                    break;
                }
                if let Some(n) = header.strip_prefix("Content-Length:") {
                    length = n.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            if stdin.read_exact(&mut body).is_err() || input.send(Message::Input(String::from_utf8_lossy(&body).into_owned())).is_err() {
                let _ = input.send(Message::Closed);
                return;
            }
        }
    });
    let mut sent = 0u64;
    let mut write = |mut message: Value| {
        sent += 1;
        message["seq"] = json!(sent);
        let body = message.to_string();
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = out.flush();
    };
    let event = |name: &str, body: Value| json!({ "type": "event", "event": name, "body": body });

    let mut session: Option<Session> = None;
    let mut breakpoints: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    // The program is started once it's launched and the editor has said
    // where to stop (`configurationDone`).
    let (mut configured, mut running) = (false, false);
    let mut frames: Vec<Frame> = Vec::new();
    // The editor's requests waiting for the program's answer: by the number
    // the program was asked with.
    let mut asked: HashMap<u64, Value> = HashMap::new();
    for message in from {
        match message {
            Message::Closed => break,
            Message::Output(text) => write(event("output", json!({ "category": "stdout", "output": format!("{text}\n") }))),
            Message::Running => {}
            Message::Stopped { reason, message, frames: now } => {
                frames = now;
                let reason = match reason.as_str() {
                    "error" => "exception",
                    other => other,
                };
                let mut body = json!({ "reason": reason, "threadId": 1, "allThreadsStopped": true });
                if let Some(message) = message {
                    (body["description"], body["text"]) = (json!("Runtime error"), json!(message));
                }
                write(event("stopped", body));
            }
            Message::Exited(code) => {
                write(event("exited", json!({ "exitCode": code })));
                write(event("terminated", json!({})));
            }
            Message::Reply(reply) => {
                let Some(request) = reply["reply"].as_u64().and_then(|id| asked.remove(&id)) else { continue };
                let shown = |v: &Value| {
                    let (parts, count) = (v["ref"].as_u64().unwrap_or(0), v["count"].as_u64().unwrap_or(0));
                    let listed = v["type"].as_str().is_some_and(|t| t.starts_with('['));
                    json!({
                        "name": v["name"], "value": v["value"], "type": v["type"], "variablesReference": parts,
                        "indexedVariables": if listed { count } else { 0 }, "namedVariables": if listed { 0 } else { count },
                    })
                };
                let vars = reply["vars"].as_array().cloned().unwrap_or_default();
                let (ok, body) = match request["command"].as_str() {
                    Some("evaluate") => match vars.first() {
                        Some(v) => {
                            let v = shown(v);
                            (true, json!({ "result": v["value"], "type": v["type"], "variablesReference": v["variablesReference"] }))
                        }
                        None => (false, json!({})),
                    },
                    _ => (true, json!({ "variables": vars.iter().map(shown).collect::<Vec<_>>() })),
                };
                let mut response =
                    json!({ "type": "response", "request_seq": request["seq"], "command": request["command"], "success": ok, "body": body });
                if !ok {
                    response["message"] = json!("there's no such variable here");
                }
                write(response);
            }
            Message::Input(text) => {
                let Ok(request) = serde_json::from_str::<Value>(&text) else { continue };
                let args = &request["arguments"];
                let command = request["command"].as_str().unwrap_or_default().to_string();
                let mut respond = |ok: bool, body: Value, message: Option<String>| {
                    let mut response =
                        json!({ "type": "response", "request_seq": request["seq"], "command": command, "success": ok, "body": body });
                    if let Some(message) = message {
                        response["message"] = json!(message);
                    }
                    write(response);
                };
                match command.as_str() {
                    "initialize" => {
                        respond(
                            true,
                            json!({ "supportsConfigurationDoneRequest": true, "supportsEvaluateForHovers": true, "supportTerminateDebuggee": true }),
                            None,
                        );
                        write(event("initialized", json!({})));
                    }
                    "launch" => {
                        let program = args["program"].as_str().unwrap_or(".").to_string();
                        let mut said = Vec::new();
                        match Session::start(Path::new(&program), true, to.clone(), &mut |line| said.push(line)) {
                            Ok(mut started) => {
                                for (file, lines) in &breakpoints {
                                    let file = started.source(file);
                                    started.breakpoints(&file, lines);
                                }
                                if configured {
                                    started.tell(json!({ "cmd": "run" }));
                                    running = true;
                                }
                                session = Some(started);
                                respond(true, json!({}), None);
                            }
                            Err(e) => {
                                respond(false, json!({}), Some(e));
                                write(event("output", json!({ "category": "stderr", "output": said.join("\n") + "\n" })));
                                write(event("terminated", json!({})));
                            }
                        }
                    }
                    "setBreakpoints" => {
                        let file = args["source"]["path"].as_str().unwrap_or_default().to_string();
                        let lines: BTreeSet<i64> = args["breakpoints"].as_array().into_iter().flatten().filter_map(|b| b["line"].as_i64()).collect();
                        if let Some(session) = session.as_mut() {
                            let name = session.source(&file);
                            session.breakpoints(&name, &lines);
                        }
                        let set: Vec<Value> = lines.iter().map(|l| json!({ "verified": true, "line": l })).collect();
                        breakpoints.insert(file, lines);
                        respond(true, json!({ "breakpoints": set }), None);
                    }
                    "configurationDone" => {
                        configured = true;
                        if let (Some(session), false) = (session.as_mut(), running) {
                            session.tell(json!({ "cmd": "run" }));
                            running = true;
                        }
                        respond(true, json!({}), None);
                    }
                    "threads" => respond(true, json!({ "threads": [{ "id": 1, "name": "main" }] }), None),
                    "stackTrace" => {
                        let shown: Vec<Value> = frames
                            .iter()
                            .enumerate()
                            .map(|(i, f)| {
                                let path = session.as_ref().map_or(f.file.clone(), |s| s.path_of(&f.file));
                                let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned());
                                json!({ "id": i, "name": f.name, "line": f.line, "column": 1, "source": { "name": name, "path": path } })
                            })
                            .collect();
                        respond(true, json!({ "totalFrames": shown.len(), "stackFrames": shown }), None);
                    }
                    "scopes" => {
                        let call = args["frameId"].as_u64().unwrap_or(0);
                        respond(true, json!({ "scopes": [{ "name": "Variables", "variablesReference": LOCALS + call, "expensive": false }] }), None);
                    }
                    "variables" | "evaluate" => match session.as_mut() {
                        Some(session) => {
                            let wanted = args["variablesReference"].as_u64().unwrap_or(0);
                            let question = if command == "evaluate" {
                                json!({ "cmd": "eval", "frame": args["frameId"].as_u64().unwrap_or(0), "expr": args["expression"] })
                            } else if wanted >= LOCALS {
                                json!({ "cmd": "vars", "frame": wanted - LOCALS })
                            } else {
                                json!({ "cmd": "children", "ref": wanted, "start": args["start"].as_u64().unwrap_or(0),
                                        "count": args["count"].as_u64().filter(|c| *c > 0).unwrap_or(1000) })
                            };
                            let id = session.ask(question);
                            asked.insert(id, request.clone());
                        }
                        None => respond(false, json!({}), None),
                    },
                    "continue" | "next" | "stepIn" | "stepOut" | "pause" => {
                        let how = match command.as_str() {
                            "stepIn" => "step",
                            "stepOut" => "out",
                            other => other,
                        };
                        if let Some(session) = session.as_mut() {
                            session.tell(json!({ "cmd": how }));
                        }
                        respond(true, json!({ "allThreadsContinued": true }), None);
                    }
                    "disconnect" | "terminate" => {
                        respond(true, json!({}), None);
                        break;
                    }
                    _ => respond(true, json!({}), None),
                }
            }
        }
    }
    if let Some(mut session) = session {
        session.end();
    }
    ExitCode::SUCCESS
}
