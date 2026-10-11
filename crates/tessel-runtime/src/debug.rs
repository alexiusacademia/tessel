//! The debugger's half inside a program (`tessel debug`).
//!
//! A program compiled for the debugger says where it is as it runs: each
//! function as it's entered and left, each statement's line, each variable
//! as it comes into being (see the compiler's `codegen::debug`). This keeps
//! the stack of calls from that, stops the program where it's asked to, and
//! reads variables out of memory while it's stopped.
//!
//! It talks to `tessel debug` over a connection the program opens when it
//! starts (to the port in `TESSEL_DEBUG`), a line of JSON at a time. Without
//! that variable none of this does anything: the program just runs.
//!
//! To the program:
//!
//! - `{"cmd":"breakpoints","file":…,"lines":[…]}`: the lines to stop at in a file
//! - `{"cmd":"run"}` once, to start; then `continue`, `next` (the next line
//!   here), `step` (into what's called), `out` (of this function), `pause`
//! - `{"cmd":"vars","frame":n,"id":…}`, `{"cmd":"children","ref":…,"id":…}`
//!   and `{"cmd":"eval","frame":n,"expr":"a.b[0]","id":…}`, while it's stopped
//!
//! From the program:
//!
//! - `{"event":"stopped","reason":…,"frames":[{"name","file","line"}…]}`
//! - `{"reply":id,"vars":[{"name","type","value","ref","count"}…]}`: `ref`
//!   (when not 0) asks for the value's parts with `children`

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CStr};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Condvar, Mutex, OnceLock, RwLock};

use serde_json::{json, Value};

use crate::{Dict, List, Str, BOX_DATA};

/// A function, as the compiler describes it.
#[repr(C)]
pub struct Function {
    name: *const c_char,
    file: *const c_char,
}

/// A type, as the compiler describes it (its `dbg_type`).
#[repr(C)]
pub struct Type {
    kind: u64,
    name: *const c_char,
    size: u64,
    /// How many `items` there are; for an optional, where its value is.
    count: u64,
    /// A struct's fields, or an enum's cases.
    items: *const Field,
    /// What a list or set holds, an optional's value, a dictionary's keys.
    first: *const Type,
    /// A dictionary's values.
    second: *const Type,
}

/// A struct's field (where it is in the struct), or an enum's case (`ty`:
/// the case's values as a struct, null for none).
#[repr(C)]
pub struct Field {
    name: *const c_char,
    offset: u64,
    ty: *const Type,
}

// The kinds of type (as in the compiler).
const INT: u64 = 0;
const FLOAT: u64 = 1;
const BOOL: u64 = 2;
const STR: u64 = 3;
const DATA: u64 = 4;
const LIST: u64 = 5;
const DICT: u64 = 6;
const SET: u64 = 7;
const OPT: u64 = 8;
const STRUCT: u64 = 9;
const ENUM: u64 = 10;
const FUNCTION: u64 = 11;
const VIEW: u64 = 12;
const IFACE: u64 = 13;
const RANGE: u64 = 14;

/// A variable of a call: its name, and how to find its value.
struct Local {
    name: *const c_char,
    /// 0: `a` is where the value is. 1: `a` holds a list and `b` an index:
    /// the value is that item. 2: `a` holds a locator (see `located`) of
    /// such a list. 3: `a` holds a locator of the value.
    kind: i32,
    a: *const u8,
    b: *const u8,
    ty: *const Type,
}

/// A call in progress.
pub struct Frame {
    /// A function that waited is done, and only still here because what
    /// waited for it goes on inside it: not shown among the calls.
    done: bool,
    id: u64,
    function: *const Function,
    line: i32,
    locals: Vec<Local>,
}

thread_local! {
    /// This thread's calls, outermost first.
    static STACK: RefCell<Vec<*mut Frame>> = const { RefCell::new(Vec::new()) };
}

/// Whether a debugger is there at all.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether lines have anything to look at (see `watch`).
static WATCH: AtomicBool = AtomicBool::new(false);
static PAUSE: AtomicBool = AtomicBool::new(false);
static NEXT_FRAME: AtomicU64 = AtomicU64::new(1);

/// How the program was last told to go on.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// Until a breakpoint.
    None,
    /// To the next line, wherever it is.
    In,
    /// To the next line of this call (or of its caller, once it's over).
    Over(u64),
    /// Until this call is over.
    Out(u64),
}

struct State {
    breakpoints: RwLock<HashMap<String, HashSet<i32>>>,
    /// How to go on, and the thread that was told to.
    step: Mutex<(Step, Option<std::thread::ThreadId>)>,
    /// Whether a thread is stopped: the others wait at their next line.
    stopped: Mutex<bool>,
    resumed: Condvar,
    out: Mutex<Option<std::net::TcpStream>>,
    /// What the debugger asks, for the thread that's stopped.
    commands: Mutex<Option<Receiver<Value>>>,
    main: Mutex<Option<std::thread::ThreadId>>,
}

fn state() -> &'static State {
    static STATE: OnceLock<State> = OnceLock::new();
    STATE.get_or_init(|| State {
        breakpoints: RwLock::new(HashMap::new()),
        step: Mutex::new((Step::None, None)),
        stopped: Mutex::new(false),
        resumed: Condvar::new(),
        out: Mutex::new(None),
        commands: Mutex::new(None),
        main: Mutex::new(None),
    })
}

/// Lines only look further when there's something to find: a breakpoint
/// somewhere, a step being taken, a pause asked for, or a thread stopped.
fn watch() {
    let s = state();
    let on = PAUSE.load(Ordering::Relaxed)
        || !s.breakpoints.read().unwrap().values().all(HashSet::is_empty)
        || s.step.lock().unwrap().0 != Step::None
        || *s.stopped.lock().unwrap();
    WATCH.store(on, Ordering::Relaxed);
}

fn send(message: &Value) {
    if let Some(out) = state().out.lock().unwrap().as_mut() {
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }
}

/// Before the program runs: if a debugger started it, connects to it and
/// waits to be told to run (after it has said where to stop).
pub fn start() {
    let Some(port) = std::env::var("TESSEL_DEBUG").ok().and_then(|p| p.parse::<u16>().ok()) else { return };
    let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        eprintln!("tessel: can't reach the debugger");
        return;
    };
    let _ = stream.set_nodelay(true);
    let Ok(input) = stream.try_clone() else { return };
    let s = state();
    *s.out.lock().unwrap() = Some(stream);
    *s.main.lock().unwrap() = Some(std::thread::current().id());
    let (to_stopped, commands): (Sender<Value>, Receiver<Value>) = std::sync::mpsc::channel();
    *s.commands.lock().unwrap() = Some(commands);
    ACTIVE.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(input).lines().map_while(Result::ok) {
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            match message["cmd"].as_str() {
                Some("breakpoints") => {
                    let file = message["file"].as_str().unwrap_or_default().to_string();
                    let lines = message["lines"].as_array().map(|l| l.iter().filter_map(|n| n.as_i64()).map(|n| n as i32).collect());
                    state().breakpoints.write().unwrap().insert(file, lines.unwrap_or_default());
                    watch();
                }
                Some("pause") => {
                    PAUSE.store(true, Ordering::SeqCst);
                    watch();
                }
                _ => {
                    let _ = to_stopped.send(message);
                }
            }
        }
        // The debugger is gone: so is what it was debugging.
        std::process::exit(0);
    });
    send(&json!({ "event": "started", "pid": std::process::id() }));
    // (Breakpoints are set by the thread above, as they come.)
    let commands = s.commands.lock().unwrap();
    while let Ok(message) = commands.as_ref().unwrap().recv() {
        if message["cmd"] == "run" {
            break;
        }
    }
}

// ===== what the program says =====

/// A function was entered. Gives its record, for `tsl_debug_resume`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_enter(function: *const Function) -> *mut Frame {
    if !ACTIVE.load(Ordering::Relaxed) {
        return std::ptr::null_mut();
    }
    let frame = Box::into_raw(Box::new(Frame { done: false, id: NEXT_FRAME.fetch_add(1, Ordering::Relaxed), function, line: 0, locals: Vec::new() }));
    STACK.with(|s| s.borrow_mut().push(frame));
    frame
}

/// The function on top returned.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_leave() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(frame) = STACK.with(|s| s.borrow_mut().pop()) else { return };
    let frame = unsafe { Box::from_raw(frame) };
    // A step over this call's last line, or out of the call, ends at the
    // caller's next line.
    if WATCH.load(Ordering::Relaxed) {
        let mut step = state().step.lock().unwrap();
        if matches!(step.0, Step::Over(id) | Step::Out(id) if id == frame.id) {
            step.0 = Step::In;
        }
    }
}

/// The function on top, one that waits, is done: what waited for it goes
/// on next, before this one has returned. A step over its last line, or
/// out of it, ends there.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_done() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(top) = STACK.with(|s| {
        s.borrow().last().map(|&f| unsafe {
            (*f).done = true;
            (*f).id
        })
    }) else {
        return;
    };
    if !WATCH.load(Ordering::Relaxed) {
        return;
    }
    let mut step = state().step.lock().unwrap();
    if matches!(step.0, Step::Over(id) | Step::Out(id) if id == top) {
        step.0 = Step::In;
    }
}

/// The function on top waits (`await`): it's off the stack until it goes on.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_park() {
    if ACTIVE.load(Ordering::Relaxed) {
        STACK.with(|s| s.borrow_mut().pop());
    }
}

/// A function that waited goes on.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_resume(frame: *mut Frame) {
    if ACTIVE.load(Ordering::Relaxed) && !frame.is_null() {
        STACK.with(|s| s.borrow_mut().push(frame));
    }
}

/// Variable `index` of the function on top (the ones before it stay, any
/// after it are gone).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_local(index: i32, name: *const c_char, kind: i32, a: *const u8, b: *const u8, ty: *const Type) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    STACK.with(|s| {
        let Some(&frame) = s.borrow().last() else { return };
        let locals = unsafe { &mut (*frame).locals };
        locals.truncate(index as usize);
        locals.push(Local { name, kind, a, b, ty });
    });
}

/// The function on top is about to run the statement on `line`, with its
/// first `live` variables in scope. This is where a program stops.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_debug_line(line: i32, live: i32) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Some(frame) = STACK.with(|s| s.borrow().last().copied()) else { return };
    unsafe {
        (*frame).line = line;
        (*frame).locals.truncate(live as usize);
    }
    if WATCH.load(Ordering::Relaxed) {
        look(unsafe { &*frame });
    }
}

/// Decides whether to stop at the line `frame` is on.
fn look(frame: &Frame) {
    let s = state();
    // Another thread is stopped: the whole program is.
    {
        let mut stopped = s.stopped.lock().unwrap();
        while *stopped {
            stopped = s.resumed.wait(stopped).unwrap();
        }
    }
    let me = std::thread::current().id();
    let file = unsafe { CStr::from_ptr((*frame.function).file) }.to_string_lossy();
    let at_breakpoint = s.breakpoints.read().unwrap().get(file.as_ref()).is_some_and(|lines| lines.contains(&frame.line));
    let stepped = {
        let step = s.step.lock().unwrap();
        step.1 == Some(me)
            && match step.0 {
                Step::In => true,
                Step::Over(id) => id == frame.id,
                Step::Out(_) | Step::None => false,
            }
    };
    let paused = PAUSE.swap(false, Ordering::SeqCst);
    let reason = match (at_breakpoint, stepped, paused) {
        (true, ..) => "breakpoint",
        (_, true, _) => "step",
        (_, _, true) => "pause",
        _ => {
            if paused {
                watch();
            }
            return;
        }
    };
    stop(reason, None);
}

/// A runtime error is about to end the program: the debugger shows where,
/// and what the variables were, before it does.
pub(crate) fn failed(message: &str) {
    if ACTIVE.load(Ordering::Relaxed) && STACK.with(|s| !s.borrow().is_empty()) {
        // (An error in what the debugger itself asked for mustn't stop twice.)
        static FAILED: AtomicBool = AtomicBool::new(false);
        if !FAILED.swap(true, Ordering::SeqCst) {
            stop("error", Some(message));
        }
    }
}

/// Stops this thread (and with it the program) until the debugger says how
/// to go on, answering what it asks meanwhile.
fn stop(reason: &str, message: Option<&str>) {
    let s = state();
    {
        let mut stopped = s.stopped.lock().unwrap();
        while *stopped {
            stopped = s.resumed.wait(stopped).unwrap();
        }
        *stopped = true;
    }
    *s.step.lock().unwrap() = (Step::None, None);
    PAUSE.store(false, Ordering::SeqCst);
    watch();
    let me = std::thread::current().id();
    let frames: Vec<*mut Frame> = STACK.with(|s| s.borrow().iter().rev().copied().filter(|&f| unsafe { !(*f).done }).collect());
    let shown: Vec<Value> = frames
        .iter()
        .map(|&f| unsafe {
            let function = &*(*f).function;
            json!({ "name": text(function.name), "file": text(function.file), "line": (*f).line })
        })
        .collect();
    let thread = if *s.main.lock().unwrap() == Some(me) { "main" } else { "background" };
    send(&json!({ "event": "stopped", "reason": reason, "message": message, "thread": thread, "frames": shown }));

    // The values the debugger was shown the parts of: where each is.
    let mut parts: Vec<(*const u8, *const Type)> = Vec::new();
    let step = {
        let commands = s.commands.lock().unwrap();
        // (What was asked while the program ran was asked of a program that
        // wasn't stopped here: a second `continue` mustn't skip this stop.)
        while commands.as_ref().unwrap().try_recv().is_ok() {}
        loop {
            let Ok(ask) = commands.as_ref().unwrap().recv() else { break Step::None };
            let top = frames.first().map_or(0, |&f| unsafe { (*f).id });
            let frame = ask["frame"].as_u64().and_then(|n| frames.get(n as usize)).copied();
            let reply = |vars: Vec<Value>| send(&json!({ "reply": ask["id"], "vars": vars }));
            match ask["cmd"].as_str() {
                Some("continue") | Some("run") => break Step::None,
                Some("step") => break Step::In,
                Some("next") => break Step::Over(top),
                Some("out") => break Step::Out(top),
                Some("vars") => {
                    let locals = frame.map(|f| unsafe { &(*f).locals });
                    let vars = locals.into_iter().flatten().filter_map(|l| unsafe {
                        Some(shown_as(&text(l.name), place(l)?, l.ty, &mut parts))
                    });
                    reply(vars.collect());
                }
                Some("children") => {
                    let (start, count) = (ask["start"].as_u64().unwrap_or(0) as usize, ask["count"].as_u64().unwrap_or(100) as usize);
                    let found = ask["ref"].as_u64().and_then(|r| parts.get(r as usize - 1).copied());
                    let children = found.map_or(Vec::new(), |(at, ty)| unsafe { children(at, ty, start, count) });
                    reply(children.into_iter().map(|(name, at, ty)| unsafe { shown_as(&name, at, ty, &mut parts) }).collect());
                }
                Some("eval") => {
                    let found = frame.and_then(|f| unsafe { find(&(*f).locals, ask["expr"].as_str().unwrap_or_default()) });
                    match found {
                        Some((at, ty)) => reply(vec![unsafe { shown_as(ask["expr"].as_str().unwrap_or_default(), at, ty, &mut parts) }]),
                        None => send(&json!({ "reply": ask["id"], "error": "there's no such variable here" })),
                    }
                }
                _ => {}
            }
        }
    };
    *s.step.lock().unwrap() = (step, Some(me));
    *s.stopped.lock().unwrap() = false;
    s.resumed.notify_all();
    watch();
    send(&json!({ "event": "running" }));
}

// ===== reading values =====

unsafe fn text(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

/// A locator, as the program keeps one (`{ function, environment }`): the
/// place it gives when called.
unsafe fn located(locator: *const u8) -> *const u8 {
    unsafe {
        let function = *(locator as *const Option<unsafe extern "C" fn(*const c_void) -> *const u8>);
        let environment = *(locator.add(8) as *const *const c_void);
        function.map_or(std::ptr::null(), |f| f(environment))
    }
}

/// Item `index` of the list kept at `slot`; none if there's no such item.
unsafe fn item(slot: *const u8, index: *const u8) -> Option<*const u8> {
    unsafe {
        if slot.is_null() {
            return None;
        }
        let list = *(slot as *const *const List);
        let i = *(index as *const i64);
        (!list.is_null() && i >= 0 && i < (*list).len).then(|| (*list).data.add(i as usize * (*(*list).elem).size as usize) as *const u8)
    }
}

/// Where a variable's value is now.
unsafe fn place(local: &Local) -> Option<*const u8> {
    unsafe {
        let at = match local.kind {
            1 => return item(local.a, local.b),
            2 => return item(located(local.a), local.b),
            3 => located(local.a),
            _ => local.a,
        };
        (!at.is_null()).then_some(at)
    }
}

/// A value for the debugger: its name, type and text, and (when it has
/// parts) the number to ask for them with.
unsafe fn shown_as(name: &str, at: *const u8, ty: *const Type, parts: &mut Vec<(*const u8, *const Type)>) -> Value {
    unsafe {
        let count = count(at, ty);
        let reference = if count > 0 {
            parts.push((at, ty));
            parts.len()
        } else {
            0
        };
        let mut value = String::new();
        write(&mut value, at, ty, 0);
        json!({ "name": name, "type": text((*ty).name), "value": value, "ref": reference, "count": count })
    }
}

/// How long a value's text may get before the rest is left out.
const LONG: usize = 200;

/// Adds the text of the value at `at` to `out`, as it would be written in code.
unsafe fn write(out: &mut String, at: *const u8, ty: *const Type, depth: usize) {
    unsafe {
        let t = &*ty;
        if out.len() > LONG || depth > 6 {
            out.push('…');
            return;
        }
        // The items of a list, set or dictionary, between brackets.
        let listed = |out: &mut String, total: usize, each: &mut dyn FnMut(&mut String, usize)| {
            out.push('[');
            for i in 0..total {
                if i > 0 {
                    out.push_str(", ");
                }
                if out.len() > LONG {
                    out.push('…');
                    break;
                }
                each(out, i);
            }
            out.push(']');
        };
        let fields = |out: &mut String, t: &Type, at: *const u8| {
            for (i, f) in std::slice::from_raw_parts(t.items, t.count as usize).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let name = text(f.name);
                // (A tuple's items, and a case's unnamed values, have numbers for names.)
                if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                    out.push_str(&name);
                    out.push_str(": ");
                }
                write(out, at.add(f.offset as usize), f.ty, depth + 1);
            }
        };
        match t.kind {
            INT => out.push_str(&(*(at as *const i64)).to_string()),
            FLOAT => out.push_str(&crate::format_float(*(at as *const f64))),
            BOOL => out.push_str(if *at != 0 { "true" } else { "false" }),
            STR => {
                let s = *(at as *const *const Str);
                let s = if s.is_null() { "" } else { crate::str_text(s) };
                let cut = s.char_indices().nth(LONG).map_or(s.len(), |(i, _)| i);
                out.push_str(&format!("{:?}", &s[..cut]));
                if cut < s.len() {
                    out.push('…');
                }
            }
            DATA => {
                let s = *(at as *const *const Str);
                let n = if s.is_null() { 0 } else { (*s).len };
                out.push_str(&format!("Data ({n} byte{})", if n == 1 { "" } else { "s" }));
            }
            LIST => {
                let list = *(at as *const *const List);
                let n = if list.is_null() { 0 } else { (*list).len as usize };
                listed(out, n, &mut |out, i| write(out, (*list).data.add(i * (*(*list).elem).size as usize), t.first, depth + 1));
            }
            SET | DICT => {
                let d = *(at as *const *const Dict);
                let entries = if d.is_null() { Vec::new() } else { crate::dict_entries(d) };
                if entries.is_empty() && t.kind == DICT {
                    out.push_str("[:]");
                    return;
                }
                listed(out, entries.len(), &mut |out, i| {
                    write(out, entries[i].0, t.first, depth + 1);
                    if t.kind == DICT {
                        out.push_str(": ");
                        write(out, entries[i].1, t.second, depth + 1);
                    }
                });
            }
            OPT => match *at {
                0 => out.push_str("nil"),
                _ => write(out, at.add(t.count as usize), t.first, depth),
            },
            STRUCT => {
                let name = text(t.name);
                // (A tuple is named by its types: `(Int, String)`.)
                if !name.starts_with('(') {
                    out.push_str(&name);
                }
                out.push('(');
                fields(out, t, at);
                out.push(')');
            }
            ENUM => {
                let tag = *(at as *const i32);
                let cases = std::slice::from_raw_parts(t.items, t.count as usize);
                let Some(case) = cases.get(tag as usize) else {
                    out.push('?');
                    return;
                };
                out.push('.');
                out.push_str(&text(case.name));
                if let Some(values) = payload(at, t, case) {
                    out.push('(');
                    fields(out, &*case.ty, values);
                    out.push(')');
                }
            }
            RANGE => out.push_str(&format!("{}..{}", *(at as *const i64), *(at.add(8) as *const i64))),
            FUNCTION => out.push_str("function"),
            VIEW => out.push_str("view"),
            IFACE => out.push_str(&format!("a {}", text(t.name))),
            _ => out.push('?'),
        }
    }
}

/// Where the values of an enum's case are, if the case has any.
unsafe fn payload(at: *const u8, t: &Type, case: &Field) -> Option<*const u8> {
    unsafe {
        if case.ty.is_null() || t.size <= 4 {
            return None;
        }
        let held = *(at.add(8) as *const *const u8);
        (!held.is_null()).then(|| held.add(BOX_DATA))
    }
}

/// How many parts the value at `at` has to show.
unsafe fn count(at: *const u8, ty: *const Type) -> usize {
    unsafe {
        let t = &*ty;
        match t.kind {
            LIST => {
                let list = *(at as *const *const List);
                if list.is_null() { 0 } else { (*list).len as usize }
            }
            SET | DICT => {
                let d = *(at as *const *const Dict);
                if d.is_null() { 0 } else { crate::tsl_dict_len(d) as usize }
            }
            OPT => match *at {
                0 => 0,
                _ => count(at.add(t.count as usize), t.first),
            },
            STRUCT => t.count as usize,
            ENUM => {
                let cases = std::slice::from_raw_parts(t.items, t.count as usize);
                match cases.get(*(at as *const i32) as usize) {
                    Some(case) if payload(at, t, case).is_some() => (*case.ty).count as usize,
                    _ => 0,
                }
            }
            _ => 0,
        }
    }
}

/// The parts of the value at `at`, from part `start`, at most `most`: each
/// one's name, and where and what it is.
unsafe fn children(at: *const u8, ty: *const Type, start: usize, most: usize) -> Vec<(String, *const u8, *const Type)> {
    unsafe {
        let t = &*ty;
        let fields = |t: &Type, at: *const u8| -> Vec<(String, *const u8, *const Type)> {
            std::slice::from_raw_parts(t.items, t.count as usize)
                .iter()
                .skip(start)
                .take(most)
                .map(|f| (text(f.name), at.add(f.offset as usize), f.ty))
                .collect()
        };
        match t.kind {
            LIST => {
                let list = *(at as *const *const List);
                let n = if list.is_null() { 0 } else { (*list).len as usize };
                let size = if list.is_null() { 0 } else { (*(*list).elem).size as usize };
                (start..n).take(most).map(|i| (format!("[{i}]"), (*list).data.add(i * size) as *const u8, t.first)).collect()
            }
            SET | DICT => {
                let d = *(at as *const *const Dict);
                let entries = if d.is_null() { Vec::new() } else { crate::dict_entries(d) };
                entries
                    .into_iter()
                    .enumerate()
                    .skip(start)
                    .take(most)
                    .map(|(i, (key, value))| {
                        if t.kind == SET {
                            return (format!("[{i}]"), key, t.first);
                        }
                        let mut name = String::new();
                        write(&mut name, key, t.first, 5);
                        (format!("[{name}]"), value, t.second)
                    })
                    .collect()
            }
            OPT if *at != 0 => children(at.add(t.count as usize), t.first, start, most),
            STRUCT => fields(t, at),
            ENUM => {
                let cases = std::slice::from_raw_parts(t.items, t.count as usize);
                match cases.get(*(at as *const i32) as usize) {
                    Some(case) => payload(at, t, case).map_or(Vec::new(), |values| fields(&*case.ty, values)),
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }
}

/// The value `path` names among `locals`: a variable, then any of `.field`,
/// `[3]`, `["key"]` and `.0` after it.
unsafe fn find(locals: &[Local], path: &str) -> Option<(*const u8, *const Type)> {
    unsafe {
        let path = path.trim();
        let end = path.find(['.', '[']).unwrap_or(path.len());
        // (The innermost variable of that name: the last one.)
        let local = locals.iter().rev().find(|l| text(l.name) == path[..end])?;
        let (mut at, mut ty) = (place(local)?, local.ty);
        let mut rest = &path[end..];
        while !rest.is_empty() {
            let (name, after) = if let Some(inner) = rest.strip_prefix('[') {
                let close = inner.find(']')?;
                (format!("[{}]", inner[..close].trim()), &inner[close + 1..])
            } else {
                let inner = rest.strip_prefix('.')?;
                let end = inner.find(['.', '[']).unwrap_or(inner.len());
                (inner[..end].to_string(), &inner[end..])
            };
            let parts = children(at, ty, 0, usize::MAX);
            let (_, part, part_ty) = parts.into_iter().find(|(n, ..)| *n == name)?;
            (at, ty, rest) = (part, part_ty, after);
        }
        Some((at, ty))
    }
}
