//! What `tessel preview` tells a visual editor about a program's views:
//! where each view expression is written, and where its arguments and
//! modifiers are, so that an editor can change them in the text.
//!
//! The code generator lists the view expressions as it compiles them (see
//! `codegen::compile_design`), and has each view on screen say which one it
//! is (the runtime's `tsl_ui_source`).

use std::collections::HashMap;

use crate::ast::{Arg, Block, Expr, ExprKind, Stmt};
use crate::check::{ArgRef, Callee, Checked};
use crate::source::{SourceMap, Span, FILE_MASK, PRELUDE_FILE};
use crate::types::Ty;

/// A view expression: a view with its modifiers, like
/// `Text("Hi").font(size: 20).padding(8)`. Positions are bytes in the file.
pub struct View {
    pub file: u32,
    pub lo: u32,
    pub hi: u32,
    /// Where the block it's a statement of starts.
    pub parent: u32,
    /// The view's content block (`{ … }`), for views that hold others.
    pub block: Option<(u32, u32)>,
    /// A `view` of the program, rather than a built-in one.
    pub custom: bool,
    pub base: Call,
    /// The modifiers, in the order they're written.
    pub mods: Vec<Call>,
    /// For a new `state` of the `view` (or `app`) it's in: where it can be
    /// written (after the last one, or at the start of the body), and a
    /// statement of the body that gives the indentation. `None` for a view
    /// that isn't in one (in a function, say).
    pub state_at: Option<u32>,
    pub indent_at: Option<u32>,
}

/// The view's own call (`Text("Hi")`), or a modifier (`.padding(8)`).
pub struct Call {
    pub name: String,
    /// For a modifier, `lo` is where the view before it ends.
    pub lo: u32,
    pub hi: u32,
    pub name_hi: u32,
    /// What's between the parentheses; `None` for a call without them.
    pub parens: Option<(u32, u32)>,
    pub args: Vec<CallArg>,
}

pub struct CallArg {
    /// `""` for an argument without a label.
    pub label: String,
    pub lo: u32,
    pub value_lo: u32,
    pub hi: u32,
}

/// The view expressions of a program, numbered in the order they're found.
#[derive(Default)]
pub struct Views {
    pub list: Vec<View>,
    known: HashMap<(u32, u32, u32), u32>,
}

impl Views {
    /// The number of the view expression `e` (a statement of the block that
    /// starts at `parent`); `None` if it isn't one that can be described.
    pub fn number(&mut self, e: &Expr, parent: Span, body: Option<&Block>, checked: &Checked, sources: &SourceMap) -> Option<u32> {
        let file = e.span.file & FILE_MASK;
        if file == PRELUDE_FILE {
            return None;
        }
        let key = (file, e.span.lo, e.span.hi);
        if let Some(n) = self.known.get(&key) {
            return Some(*n);
        }
        let mut view = describe(e, parent, checked, sources)?;
        if let Some(body) = body.filter(|b| b.span.file & FILE_MASK == file) {
            let last_state = body.stmts.iter().rev().find_map(|s| match s {
                Stmt::State(d) => Some(d),
                _ => None,
            });
            view.state_at = Some(match last_state {
                Some(d) => d.span.hi.max(d.value.span.hi),
                None => body.span.lo + 1,
            });
            view.indent_at = match last_state {
                Some(d) => Some(d.span.lo),
                None => body.stmts.iter().find_map(|s| match s {
                    Stmt::Expr(e) => Some(e.span.lo),
                    Stmt::Let(d) | Stmt::Var(d) => Some(d.span.lo),
                    _ => None,
                }),
            };
        }
        self.list.push(view);
        let n = (self.list.len() - 1) as u32;
        self.known.insert(key, n);
        Some(n)
    }

    /// As JSON: the files, and the views with their positions in characters
    /// (as a Tessel program counts them) rather than bytes.
    pub fn to_json(&self, sources: &SourceMap) -> String {
        let chars: std::cell::RefCell<HashMap<u32, Vec<u32>>> = Default::default();
        let at = |file: u32, byte: u32| -> u32 {
            let mut chars = chars.borrow_mut();
            let table = chars.entry(file).or_insert_with(|| {
                // For each byte, how many characters start before it.
                let text = &sources.file(file).text;
                let mut table = Vec::with_capacity(text.len() + 1);
                let mut count = 0;
                for i in 0..=text.len() {
                    table.push(count);
                    if i < text.len() && text.is_char_boundary(i) {
                        count += 1;
                    }
                }
                table
            });
            table.get(byte as usize).copied().unwrap_or(0)
        };
        let call = |file: u32, c: &Call| -> String {
            let args: Vec<String> = c
                .args
                .iter()
                .map(|a| {
                    format!(
                        "{{\"label\":{},\"lo\":{},\"valueLo\":{},\"hi\":{}}}",
                        quote(&a.label),
                        at(file, a.lo),
                        at(file, a.value_lo),
                        at(file, a.hi)
                    )
                })
                .collect();
            let (args_lo, args_hi) = match c.parens {
                Some((lo, hi)) => (at(file, lo) as i64, at(file, hi) as i64),
                None => (-1, -1),
            };
            format!(
                "{{\"name\":{},\"lo\":{},\"hi\":{},\"nameHi\":{},\"argsLo\":{args_lo},\"argsHi\":{args_hi},\"args\":[{}]}}",
                quote(&c.name),
                at(file, c.lo),
                at(file, c.hi),
                at(file, c.name_hi),
                args.join(",")
            )
        };
        let views: Vec<String> = self
            .list
            .iter()
            .map(|v| {
                let source = sources.file(v.file);
                let (line, end_line) = (source.line_col(v.lo as usize).0, source.line_col(v.hi as usize).0);
                let base = call(v.file, &v.base);
                let mods: Vec<String> = v.mods.iter().map(|m| call(v.file, m)).collect();
                let (block_lo, block_hi) = match v.block {
                    Some((lo, hi)) => (at(v.file, lo) as i64, at(v.file, hi) as i64),
                    None => (-1, -1),
                };
                let state_at = v.state_at.map_or(-1, |p| at(v.file, p) as i64);
                let indent_at = v.indent_at.map_or(-1, |p| at(v.file, p) as i64);
                format!(
                    "{{\"file\":{},\"lo\":{},\"hi\":{},\"line\":{line},\"endLine\":{end_line},\"parent\":{},\"blockLo\":{block_lo},\"blockHi\":{block_hi},\"stateAt\":{state_at},\"indentAt\":{indent_at},\"custom\":{},\"base\":{base},\"mods\":[{}]}}",
                    v.file,
                    at(v.file, v.lo),
                    at(v.file, v.hi),
                    at(v.file, v.parent),
                    v.custom,
                    mods.join(",")
                )
            })
            .collect();
        let files: Vec<String> = sources.files().map(|(_, f)| quote(&f.name.replace('\\', "/"))).collect();
        format!("{{\"files\":[{}],\"views\":[\n{}\n]}}\n", files.join(","), views.join(",\n"))
    }
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn describe(e: &Expr, parent: Span, checked: &Checked, sources: &SourceMap) -> Option<View> {
    let file = e.span.file & FILE_MASK;
    let text = sources.file(file).text.as_bytes();
    let call = |name: &str, lo: u32, hi: u32, name_hi: u32, args: &[Arg]| -> Call {
        // The parentheses, if the call has them: `(` right after the name,
        // and `)` after the last argument.
        let mut i = name_hi as usize;
        while matches!(text.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let parens = (text.get(i) == Some(&b'(')).then_some(i + 1).and_then(|open| {
            let mut j = args.last().map_or(open, |a| a.value.span.hi as usize);
            while matches!(text.get(j), Some(b' ' | b'\t' | b'\n' | b'\r' | b',')) {
                j += 1;
            }
            (text.get(j) == Some(&b')')).then_some((open as u32, j as u32))
        });
        let args = args
            .iter()
            .map(|a| CallArg {
                label: a.label.as_ref().map_or(String::new(), |l| l.name.clone()),
                lo: a.label.as_ref().map_or(a.value.span.lo, |l| l.span.lo),
                value_lo: a.value.span.lo,
                hi: a.value.span.hi,
            })
            .collect();
        Call { name: name.to_string(), lo, hi, name_hi, parens, args }
    };
    let mut mods = Vec::new();
    let mut cur = e;
    loop {
        let ExprKind::Call { callee, args, trailing } = &cur.kind else { return None };
        let info = checked.calls.get(&cur.id)?;
        match &info.callee {
            Callee::Modifier(name) => {
                let ExprKind::Member { object, name: member, .. } = &callee.kind else { return None };
                mods.push(call(name, object.span.hi, cur.span.hi, member.span.hi, args));
                cur = object;
            }
            Callee::View(name) | Callee::BuiltinView(name) => {
                let custom = matches!(info.callee, Callee::View(_));
                // The block that holds other views: a stack's, or the one a
                // custom view takes as its `View` parameter.
                let holds = match custom {
                    false => matches!(name.as_str(), "VStack" | "HStack" | "ZStack" | "Scroll" | "HSplit" | "VSplit"),
                    true => checked.views.get(name).is_some_and(|sigs| {
                        sigs.iter()
                            .zip(&info.args)
                            .any(|(sig, arg)| matches!(arg, Some(ArgRef::Trailing)) && sig.ty == Ty::View)
                    }),
                };
                let block = match trailing.as_deref() {
                    Some(Expr { kind: ExprKind::Closure { params, .. }, span, .. }) if holds && params.is_empty() => {
                        Some((span.lo, span.hi))
                    }
                    _ => None,
                };
                let base = call(name, cur.span.lo, cur.span.hi, callee.span.hi, args);
                mods.reverse();
                return Some(View {
                    file,
                    lo: e.span.lo,
                    hi: e.span.hi,
                    parent: parent.lo,
                    block,
                    custom,
                    base,
                    mods,
                    state_at: None,
                    indent_at: None,
                });
            }
            _ => return None,
        }
    }
}
