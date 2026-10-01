//! `tessel repl`: type code, and see what it does.
//!
//! Each entry is compiled to machine code and run inside this process (see
//! `tessel_compiler::jit`). Declarations (functions, structs, …) are kept as
//! source and compiled again with every entry; variables live in the code
//! loaded for the entry that declared them.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use tessel_compiler::codegen::{self, ReplEntry};
use tessel_compiler::diag::render;
use tessel_compiler::jit::Jit;
use tessel_compiler::parser::{self, DeclKind, ReplDecl};
use tessel_compiler::source::SourceMap;
use tessel_compiler::types::Ty;
use tessel_compiler::{analyze_repl, ReplAnalysis, ReplShow, ReplVar};
// (Linked for the functions in RUNTIME_SYMBOLS.)
use tessel_runtime as _;

include!(concat!(env!("OUT_DIR"), "/runtime_symbols.rs"));

/// The runtime's functions, for code loaded into this process to call.
pub fn runtime_symbols() -> Vec<(&'static str, *const u8)> {
    RUNTIME_SYMBOLS.iter().map(|(name, f)| (*name, *f as *const u8)).collect()
}

const HELP: &str = "Type Tessel code and press Enter to run it:

  1 + 2                      the value of an expression is shown
  let name = \"Ada\"           variables stay for the lines that follow
  fn double(n: Int) -> Int { n * 2 }
                             so do functions, structs, enums and interfaces;
                             declare one again to replace it
  for i in 0..3 {            a line that opens a bracket goes on
      print(double(i))       until the bracket is closed
  }

Commands:
  :vars    the variables so far, with their types
  :help    this text
  :quit    leave (also Ctrl+D)

Views and apps need a window: put them in a file, and use `tessel run`.";

// ===== running entries =====

/// How an entry ended.
enum Outcome {
    Done,
    /// A runtime error (its message is already printed).
    Failed,
}

/// Where the thread running entries reports to. A runtime error reports
/// from its hook (see `on_error`).
static REPORT: Mutex<Option<Sender<Outcome>>> = Mutex::new(None);

fn report(outcome: Outcome) {
    if let Some(to) = REPORT.lock().unwrap().as_ref() {
        let _ = to.send(outcome);
    }
}

/// After a runtime error: the code that failed can't go on, and can't be
/// unwound either, so its thread stays here for good.
fn on_error() {
    let _ = std::io::stdout().flush();
    report(Outcome::Failed);
    loop {
        std::thread::park();
    }
}

/// The thread that runs entries (one after another, so timers and other
/// state of the runtime carry over).
struct Worker {
    entries: Sender<usize>,
    outcomes: Receiver<Outcome>,
}

impl Worker {
    fn start() -> Worker {
        let (entries, todo) = channel::<usize>();
        let (report_to, outcomes) = channel();
        *REPORT.lock().unwrap() = Some(report_to);
        std::thread::Builder::new()
            .name("repl".into())
            .stack_size(8 << 20)
            .spawn(move || {
                tessel_runtime::guard_stack();
                for address in todo {
                    let entry: extern "C" fn() = unsafe { std::mem::transmute(address) };
                    entry();
                    // Timers, loads and other background work the entry started.
                    tessel_runtime::ui::run_until_idle();
                    let _ = std::io::stdout().flush();
                    report(Outcome::Done);
                }
            })
            .expect("can't start a thread");
        Worker { entries, outcomes }
    }

    /// Runs the entry at `address`. `false` if it failed (the worker is then
    /// of no more use).
    fn run(&self, address: *const u8) -> bool {
        let _ = self.entries.send(address as usize);
        matches!(self.outcomes.recv(), Ok(Outcome::Done))
    }
}

// ===== the session =====

/// The declarations of one entry: its text with everything else blanked
/// (so places in it stay the same), and where each declaration is.
struct Declared {
    file: String,
    text: String,
    items: Vec<ReplDecl>,
}

struct Session {
    declared: Vec<Declared>,
    vars: Vec<ReplVar>,
    /// The number of the next entry.
    index: usize,
    jit: Jit,
    worker: Worker,
    color: bool,
}

/// `text` with the bytes in `lo..hi` turned into spaces (line ends stay).
fn blank(text: &mut String, lo: usize, hi: usize) {
    let blanked: String = text[lo..hi].chars().map(|c| if c == '\n' { "\n".to_string() } else { " ".repeat(c.len_utf8()) }).collect();
    text.replace_range(lo..hi, &blanked);
}

/// Does `text` use the name `word`?
fn mentions(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(is_word) && !text[at + word.len()..].chars().next().is_some_and(is_word)
    })
}

impl Session {
    fn new() -> Result<Session, String> {
        let symbols = runtime_symbols();
        tessel_runtime::on_error(on_error);
        Ok(Session {
            declared: Vec::new(),
            vars: Vec::new(),
            index: 1,
            jit: Jit::new(&symbols)?,
            worker: Worker::start(),
            color: std::io::stderr().is_terminal(),
        })
    }

    /// The session's files with `entry` as the last one, and its number.
    fn sources(&self, declared: &[Declared], entry: &str) -> (SourceMap, u32) {
        let mut sources = SourceMap::new();
        for d in declared {
            sources.add(d.file.clone(), d.text.clone());
        }
        let id = sources.add(format!("repl.{}", self.index), entry.to_string());
        (sources, id)
    }

    fn eval(&mut self, entry: &str) {
        // What the entry declares again replaces what was declared before.
        let found = parser::parse_repl(0, entry, &mut Default::default(), &mut 0).decls;
        if found.iter().any(|d| d.kind == DeclKind::View) {
            eprintln!("error: the REPL has no window for views and apps\n = help: put them in a file, and use `tessel run`");
            return;
        }
        let mut declared: Vec<Declared> = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        for old in &self.declared {
            let mut text = old.text.clone();
            let mut items = Vec::new();
            for item in &old.items {
                if item.kind != DeclKind::Import && found.iter().any(|d| d.kind != DeclKind::Import && d.name == item.name) {
                    blank(&mut text, item.span.lo as usize, item.span.hi as usize);
                    if item.kind == DeclKind::Type {
                        changed.push(item.name.clone());
                    }
                } else {
                    items.push(item.clone());
                }
            }
            declared.push(Declared { file: old.file.clone(), text, items });
        }
        // Values of a type that changed (or of a type made of one) don't fit
        // the new type: their variables go.
        loop {
            let more: Vec<String> = declared
                .iter()
                .flat_map(|d| d.items.iter().map(move |i| (d, i)))
                .filter(|(d, i)| {
                    let text = &d.text[i.span.lo as usize..i.span.hi as usize];
                    i.kind == DeclKind::Type && !changed.contains(&i.name) && changed.iter().any(|c| mentions(text, c))
                })
                .map(|(_, i)| i.name.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            changed.extend(more);
        }
        let (vars, dropped): (Vec<ReplVar>, Vec<ReplVar>) =
            self.vars.iter().cloned().partition(|v| !changed.iter().any(|c| mentions(&v.ty, c)));

        let analyze = |show: ReplShow| {
            let (mut sources, id) = self.sources(&declared, entry);
            let analysis = analyze_repl(&mut sources, id, &vars, show);
            (sources, analysis)
        };
        let (mut sources, mut analysis) = analyze(ReplShow::No);
        if !analysis.analysis.diags.is_empty() {
            for d in &analysis.analysis.diags.list {
                eprintln!("{}", render(d, &sources, self.color));
            }
            self.index += 1;
            return;
        }
        // The value of a last expression is shown: printed as `print` would,
        // or as JSON (structs, optionals), or else only its type is.
        let result = analysis.analysis.checked.repl.as_ref().map_or(Ty::Void, |r| r.result.clone());
        let mut show_type = None;
        if analysis.last_expr.is_some() && result != Ty::Void {
            let shown = [ReplShow::Print, ReplShow::Json].into_iter().find_map(|show| {
                let (s, a) = analyze(show);
                a.analysis.diags.is_empty().then_some((s, a))
            });
            match shown {
                Some((s, a)) => (sources, analysis) = (s, a),
                None => show_type = Some(analysis.analysis.checked.show(&result)),
            }
        }
        let ReplAnalysis { analysis, decls, .. } = analysis;
        let object = codegen::compile_repl(
            &analysis.program,
            &analysis.checked,
            &sources,
            ReplEntry { index: self.index, vars: &vars },
        );
        let object = match object {
            Ok(o) => o,
            Err(diags) => {
                for d in &diags {
                    eprintln!("{}", render(d, &sources, self.color));
                }
                self.index += 1;
                return;
            }
        };
        let symbol = codegen::repl_entry_symbol(self.index);
        let address = match self.jit.add_object(&object).and_then(|()| self.jit.lookup(&symbol)) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("error: can't load the compiled code: {e}");
                self.index += 1;
                return;
            }
        };

        // From here on the entry counts: its declarations stay even if its
        // statements fail.
        for v in &dropped {
            eprintln!("note: `{}` is gone, because its type (`{}`) was declared again", v.name, v.ty);
        }
        if !decls.is_empty() {
            let mut text = entry.to_string();
            let mut at = 0;
            for d in &decls {
                blank(&mut text, at, d.span.lo as usize);
                at = d.span.hi as usize;
            }
            let end = text.len();
            blank(&mut text, at, end);
            declared.push(Declared { file: format!("repl.{}", self.index), text, items: decls });
        }
        self.declared = declared;
        self.vars = vars;
        let ran = self.worker.run(address);
        if ran {
            for (name, ty, mutable) in analysis.checked.repl.iter().flat_map(|r| &r.vars) {
                self.vars.retain(|v| v.name != *name);
                self.vars.push(ReplVar {
                    name: name.clone(),
                    ty: analysis.checked.show(ty),
                    mutable: *mutable,
                    symbol: codegen::repl_var_symbol(self.index, name),
                });
            }
            if let Some(ty) = show_type {
                println!("({ty})");
            }
        } else {
            self.worker = Worker::start();
        }
        self.index += 1;
    }

    /// A `:command`. Returns whether to leave.
    fn command(&self, line: &str) -> bool {
        match line {
            ":quit" | ":q" | ":exit" => return true,
            ":help" | ":h" => println!("{HELP}"),
            ":vars" => {
                if self.vars.is_empty() {
                    println!("(no variables yet)");
                }
                for v in &self.vars {
                    println!("{} {}: {}", if v.mutable { "var" } else { "let" }, v.name, v.ty);
                }
            }
            _ => eprintln!("error: unknown command `{line}`\n = help: the commands are `:vars`, `:help` and `:quit`"),
        }
        false
    }
}

/// Does `text` close every bracket it opens? (If not, the entry goes on.)
fn closed(text: &str) -> bool {
    let mut depth = 0i32;
    // Inside a string; and the brackets of the `{…}` in strings around.
    let (mut in_string, mut strings) = (false, Vec::new());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_string => {
                chars.next();
            }
            '"' => in_string = !in_string,
            '\n' => in_string = false,
            '{' if in_string => {
                strings.push(depth);
                in_string = false;
                depth += 1;
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|c| *c != '\n') {
                    chars.next();
                }
            }
            '(' | '[' | '{' if !in_string => depth += 1,
            ')' | ']' | '}' if !in_string => {
                depth -= 1;
                if strings.last() == Some(&depth) {
                    strings.pop();
                    in_string = true;
                }
            }
            _ => {}
        }
    }
    depth <= 0
}

/// Where the lines come from: the keyboard, with line editing, or a file or
/// another program (no prompts then).
enum Input {
    Keyboard(Box<rustyline::DefaultEditor>),
    Piped(std::io::Lines<std::io::StdinLock<'static>>),
}

impl Input {
    /// The next line; `None` at the end of the input (or Ctrl+D), and an
    /// empty line for Ctrl+C.
    fn line(&mut self, prompt: &str) -> Option<String> {
        match self {
            Input::Keyboard(editor) => match editor.readline(prompt) {
                Ok(line) => Some(line),
                Err(rustyline::error::ReadlineError::Interrupted) => Some(String::new()),
                Err(_) => None,
            },
            Input::Piped(lines) => lines.next().and_then(Result::ok),
        }
    }
}

pub fn run() -> ExitCode {
    let mut session = match Session::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tessel: can't start the REPL: {e}");
            return ExitCode::FAILURE;
        }
    };
    let keyboard = std::io::stdin().is_terminal().then(rustyline::DefaultEditor::new).and_then(Result::ok);
    let mut input = match keyboard {
        Some(editor) => {
            println!("Tessel {} (:help for help, :quit to leave)", tessel_compiler::VERSION);
            Input::Keyboard(Box::new(editor))
        }
        None => Input::Piped(std::io::stdin().lock().lines()),
    };
    while let Some(first) = input.line("> ") {
        let mut entry = first;
        if entry.trim().is_empty() {
            continue;
        }
        if entry.trim_start().starts_with(':') {
            if session.command(entry.trim()) {
                break;
            }
            continue;
        }
        // An open bracket: the entry goes on with the next lines.
        while !closed(&entry) {
            match input.line(". ") {
                Some(more) => entry = format!("{entry}\n{more}"),
                None => break,
            }
        }
        if let Input::Keyboard(editor) = &mut input {
            let _ = editor.add_history_entry(entry.as_str());
        }
        session.eval(&entry);
        let _ = std::io::stdout().flush();
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_go_on_until_brackets_close() {
        assert!(closed("let x = 1"));
        assert!(!closed("fn f() {"));
        assert!(closed("fn f() {\n}"));
        assert!(!closed("print([1,\n2"));
        // Brackets in strings and comments don't count; interpolations do.
        assert!(closed("print(\"(\")"));
        assert!(closed("let a = 1 // {"));
        assert!(!closed("print(\"{f(\""));
        assert!(closed("print(\"{f(1)} and {g([2])}\")\nlet x = 1"));
    }

    #[test]
    fn blanking_keeps_places() {
        let mut text = "let é = 1\nfn f() {}\nlet b = 2".to_string();
        let lo = text.find("fn").unwrap();
        let before = text.len();
        blank(&mut text, 0, lo);
        assert_eq!(text.len(), before);
        assert_eq!(text.find("fn"), Some(lo));
        assert!(text.starts_with("          \nfn f() {}"));
        assert!(mentions("[Point]?", "Point") && !mentions("[PointList]", "Point"));
    }
}
