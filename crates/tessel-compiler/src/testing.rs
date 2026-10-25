//! `tessel test`: a program's `test "name" { … }` blocks, run one at a time.
//!
//! The parser makes each test a function (`__test_<file>_<n>`, listed in
//! `Program::tests`); every build checks them, and none calls them. For
//! `tessel test`, `lower` makes the program a test runner instead:
//!
//! - `expect(condition)` in the program's own files learns where it is and
//!   what it says (`__expectTrue(condition, "math.tsl:12", "total > 0")`),
//!   and a comparison, `expect(left == right)`, works out each side once and
//!   shows both (as JSON) when it fails (`__expectCompare`). `fail(message)` learns
//!   where it is too.
//! - The program's own `fn main` and `app` give way to a `main` that runs
//!   the test the `TESSEL_TEST` environment variable names.
//!
//! A failure is a line on standard error (see `__testFailed` in the
//! prelude), which `tessel test` reads; each test runs in a process of its
//! own, so one that crashes doesn't stop the others.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::{Diagnostic, Diagnostics};
use crate::source::{SourceMap, Span, FILE_MASK};

/// Makes `program` (whose own files are `root`) run its tests: see above.
pub fn lower(program: &mut Program, root: &HashSet<u32>, sources: &mut SourceMap, diags: &mut Diagnostics, next_id: &mut ExprId) {
    // Two tests with one name can't be told apart in the report.
    let mut seen: HashMap<&str, Span> = HashMap::new();
    for t in &program.tests {
        if let Some(first) = seen.insert(&t.name, t.span) {
            let mut d = Diagnostic::error(t.span, format!("there's already a test named \"{}\"", t.name))
                .with_help("give each test a name of its own");
            d.note_at(first, "the first one");
            diags.push(d);
        }
    }
    // A program with its own `expect` or `fail` keeps them as they are.
    let own: HashSet<String> = program
        .items
        .iter()
        .filter(|i| root.contains(&(i.name().span.file & FILE_MASK)))
        .map(|i| i.name().name.clone())
        .collect();
    let mut lowering = Lowering { sources, next_id, temps: 0, expect: !own.contains("expect"), fail: !own.contains("fail") };
    for item in &mut program.items {
        if !root.contains(&(item.name().span.file & FILE_MASK)) {
            continue;
        }
        match item {
            Item::Fn(f) => lowering.block(&mut f.body),
            Item::Struct(d) => d.methods.iter_mut().chain(d.statics.iter_mut()).for_each(|m| lowering.block(&mut m.body)),
            Item::Enum(d) => d.methods.iter_mut().chain(d.statics.iter_mut()).for_each(|m| lowering.block(&mut m.body)),
            _ => {}
        }
    }
    // The program's entry gives way to the tests'.
    program.items.retain(|i| match i {
        Item::App(_) => false,
        Item::Fn(f) => !(f.name.name == "main" && root.contains(&(f.name.span.file & FILE_MASK))),
        _ => true,
    });
    let mut text = String::from("fn main() {\n    let which = environment(\"TESSEL_TEST\") ?? \"\"\n");
    for t in &program.tests {
        text += &format!("    if which == \"{0}\" {{\n        {0}()\n    }}\n", t.function);
    }
    text += "}\n";
    let id = lowering.sources.add("<tests>", text.clone());
    let entry = crate::parser::parse_file(id, &text, diags, lowering.next_id);
    program.items.extend(entry.items);
}

struct Lowering<'a> {
    sources: &'a mut SourceMap,
    next_id: &'a mut ExprId,
    /// For naming the values a comparison is worked out into.
    temps: usize,
    /// Whether `expect` and `fail` are the prelude's.
    expect: bool,
    fail: bool,
}

impl Lowering<'_> {
    fn new_expr(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = *self.next_id;
        *self.next_id += 1;
        Expr { id, kind, span }
    }

    fn text(&mut self, s: String, span: Span) -> Expr {
        self.new_expr(ExprKind::Str(vec![StrPart::Text(s)]), span)
    }

    fn call(&mut self, name: &str, args: Vec<Expr>, span: Span) -> Expr {
        let callee = self.new_expr(ExprKind::Name(name.into()), span);
        let args = args.into_iter().map(|value| Arg { label: None, value }).collect();
        self.new_expr(ExprKind::Call { callee: Box::new(callee), args, trailing: None }, span)
    }

    /// `file.tsl:12`, for a span.
    fn at(&self, span: Span) -> String {
        let file = self.sources.file(span.file & FILE_MASK);
        let (line, _) = file.line_col(span.lo as usize);
        let name = std::path::Path::new(&file.name).file_name().map_or(file.name.clone(), |n| n.to_string_lossy().into_owned());
        format!("{name}:{line}")
    }

    fn source(&self, span: Span) -> String {
        let file = self.sources.file(span.file & FILE_MASK);
        file.text.get(span.lo as usize..span.hi as usize).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// `expect(…)` or `fail(…)`, if `e` is one, made to say where it is.
    fn lowered(&mut self, e: &mut Expr) -> Option<ExprKind> {
        let ExprKind::Call { callee, args, trailing: None } = &mut e.kind else { return None };
        let ExprKind::Name(name) = &callee.kind else { return None };
        let span = e.span;
        let at = self.at(span);
        let unlabeled = args.iter().all(|a| a.label.is_none());
        match name.as_str() {
            "fail" if self.fail && args.len() <= 1 && unlabeled => {
                let message = match args.pop() {
                    Some(a) => a.value,
                    None => self.text(String::new(), span),
                };
                let (at, what, detail) = (self.text(at, span), self.text("fail".into(), span), self.text(String::new(), span));
                Some(self.call("__testFailed", vec![at, what, message, detail], span).kind)
            }
            "expect" if self.expect && (1..=2).contains(&args.len()) && unlabeled => {
                let message = if args.len() == 2 { args.pop().unwrap().value } else { self.text(String::new(), span) };
                let condition = args.pop().unwrap().value;
                let text = self.source(condition.span);
                let (at, text) = (self.text(at, span), self.text(text, span));
                match condition.kind {
                    ExprKind::Binary(op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), left, right) => {
                        // { let l = left; let r = right; __expectCompare(l op r, toJson(l), toJson(r), …) }
                        self.temps += 1;
                        let (l, r) = (format!("__expectLeft{}", self.temps), format!("__expectRight{}", self.temps));
                        let local = |name: &str, value: Expr| {
                            Stmt::Let(LocalDecl { name: Ident { name: name.into(), span: value.span }, ty: None, span: value.span, value })
                        };
                        let stmts = vec![local(&l, *left), local(&r, *right)];
                        let (lv, rv) = (self.new_expr(ExprKind::Name(l.clone()), span), self.new_expr(ExprKind::Name(r.clone()), span));
                        let compared = self.new_expr(ExprKind::Binary(op, Box::new(lv), Box::new(rv)), condition.span);
                        // (As JSON, which nearly every value can be shown as:
                        // structs, enums, lists and optionals too.)
                        let shown = |me: &mut Self, n: &str| {
                            let v = me.new_expr(ExprKind::Name(n.into()), span);
                            me.call("toJson", vec![v], span)
                        };
                        let (ls, rs) = (shown(self, &l), shown(self, &r));
                        let check = self.call("__expectCompare", vec![compared, ls, rs, at, text, message], span);
                        let mut stmts = stmts;
                        stmts.push(Stmt::Expr(check));
                        Some(ExprKind::Block(Block { stmts, span }))
                    }
                    kind => {
                        let condition = Expr { kind, ..condition };
                        Some(self.call("__expectTrue", vec![condition, at, text, message], span).kind)
                    }
                }
            }
            _ => None,
        }
    }

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => self.expr(&mut d.value),
            Stmt::Fn(f) => self.block(&mut f.body),
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Return { value: Some(v), .. } => self.expr(v),
            Stmt::Return { value: None, .. } | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::For { iter, body, .. } => {
                self.expr(iter);
                self.block(body);
            }
            Stmt::While { cond, body, .. } => {
                self.cond(cond);
                self.block(body);
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn cond(&mut self, c: &mut Cond) {
        match c {
            Cond::Expr(e) | Cond::Let(_, _, e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::List(items) => items.iter_mut().for_each(|x| self.expr(x)),
            ExprKind::Dict(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Unary(_, x) | ExprKind::Try(x) | ExprKind::Await(x) | ExprKind::Spread(x) => self.expr(x),
            ExprKind::Cast { value, .. } => self.expr(value),
            ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => {
                self.expr(l);
                self.expr(r);
            }
            ExprKind::Call { callee, args, trailing } => {
                self.expr(callee);
                args.iter_mut().for_each(|a| self.expr(&mut a.value));
                if let Some(t) = trailing {
                    self.expr(t);
                }
            }
            ExprKind::Member { object, .. } => self.expr(object),
            ExprKind::Closure { body, .. } | ExprKind::Block(body) => self.block(body),
            ExprKind::If { cond, then, otherwise } => {
                self.cond(cond);
                self.block(then);
                match otherwise {
                    Some(Else::Block(b)) => self.block(b),
                    Some(Else::If(x)) => self.expr(x),
                    None => {}
                }
            }
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    self.expr(&mut arm.body);
                }
            }
            _ => {}
        }
        if let Some(kind) = self.lowered(e) {
            e.kind = kind;
        }
    }
}
