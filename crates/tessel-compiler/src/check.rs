//! Type checker: resolves names, infers and checks types, and enforces the
//! rules for views, state and bindings. Records the type of every expression
//! for code generation.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::builtins::{self, Builtins};
use crate::diag::{suggest, Diagnostic, Diagnostics};
use crate::source::Span;
use crate::types::*;

/// Result of checking a program: everything code generation needs.
pub struct Checked {
    pub types: HashMap<ExprId, Ty>,
    pub calls: HashMap<ExprId, CallInfo>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    /// Signatures of top-level functions.
    pub fns: HashMap<String, FnSig>,
    /// Type of each local variable, keyed by the span of its name.
    pub locals: HashMap<Span, Ty>,
    /// `for` loop variables (by name span) that are places in the list,
    /// so changing them changes the list.
    pub loop_places: HashSet<Span>,
    /// Parameters of each user view.
    pub views: HashMap<String, Vec<ParamSig>>,
    /// Signatures of functions declared inside views: (view, function).
    pub view_fns: HashMap<(String, String), FnSig>,
}

impl Checked {
    /// A type as written in Tessel, like `[Todo]` or `fn(Int) -> Bool`.
    pub fn show(&self, t: &Ty) -> String {
        match t {
            Ty::Error => "?".into(),
            Ty::Void => "()".into(),
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Data => "Data".into(),
            Ty::Range => "Range".into(),
            Ty::View => "View".into(),
            Ty::Any => "any value".into(),
            Ty::List(t) => format!("[{}]", self.show(t)),
            Ty::Dict(k, v) => format!("[{}: {}]", self.show(k), self.show(v)),
            Ty::Opt(t) if matches!(**t, Ty::Fn(..)) => format!("({})?", self.show(t)),
            Ty::Opt(t) => format!("{}?", self.show(t)),
            Ty::Fn(ps, r) => {
                let ps: Vec<String> = ps.iter().map(|p| self.show(p)).collect();
                let ret = if **r == Ty::Void { String::new() } else { format!(" -> {}", self.show(r)) };
                format!("fn({}){ret}", ps.join(", "))
            }
            Ty::Struct(id) => self.structs[*id].name.clone(),
            Ty::Enum(id) => self.enums[*id].name.clone(),
        }
    }
}

/// What a call expression resolved to.
#[derive(Clone, Debug)]
pub enum Callee {
    /// A top-level user function.
    Fn(String),
    /// A struct or enum method; the receiver is the callee's object.
    Method { owner: Ty, name: String },
    /// A method called by bare name inside another method of the same type.
    SelfMethod { owner: Ty, name: String },
    /// A function declared inside a view.
    ViewFn(String),
    /// A method of a built-in type (`[T]`, `String`, `[K: V]`).
    Builtin { recv: Ty, name: String },
    /// `print`, `fetch`, `after`, `Int(…)`, `Float(…)`, `String(…)`.
    BuiltinFn(String),
    StructInit(usize),
    /// Enum id and case index.
    EnumCase(usize, usize),
    View(String),
    BuiltinView(String),
    Modifier(String),
    /// Calling a value of function type.
    Value,
}

/// Where each parameter's value comes from, in parameter order.
#[derive(Clone, Copy, Debug)]
pub enum ArgRef {
    /// Index into the call's argument list.
    Arg(usize),
    /// The trailing block.
    Trailing,
}

#[derive(Clone, Debug)]
pub struct CallInfo {
    pub callee: Callee,
    /// `None` means the parameter's default value is used.
    pub args: Vec<Option<ArgRef>>,
}

#[derive(Default)]
struct CallFrame {
    callee: Option<Callee>,
    args: Vec<Option<ArgRef>>,
}

/// Checks a whole program. With `require_entry`, it must have an `app` or a `fn main()`.
pub fn check_program(program: &Program, diags: &mut Diagnostics, require_entry: bool) -> Checked {
    let enums = builtins::enums();
    let mut globals = HashMap::new();
    for (i, e) in enums.iter().enumerate() {
        globals.insert(e.name.clone(), Global::Enum(i));
    }
    let builtins = Builtins::standard();
    let mut c = Checker {
        diags,
        builtins,
        structs: Vec::new(),
        enums,
        fns: Vec::new(),
        views: Vec::new(),
        globals,
        user_spans: HashMap::new(),
        scopes: Vec::new(),
        returns: Vec::new(),
        tag_ty: None,
        types: HashMap::new(),
        calls: HashMap::new(),
        frames: Vec::new(),
        locals: HashMap::new(),
        loop_places: HashSet::new(),
        view_fns: HashMap::new(),
    };
    let ids = c.collect(program, require_entry);
    c.signatures(program, &ids);
    c.check_main(program, require_entry);
    c.check_recursive_structs();
    c.bodies(program, &ids);
    let fns = program
        .items
        .iter()
        .zip(&ids)
        .filter_map(|(item, id)| match (item, id) {
            (Item::Fn(f), Some(id)) => Some((f.name.name.clone(), c.fns[*id].clone())),
            _ => None,
        })
        .collect();
    Checked {
        types: c.types,
        calls: c.calls,
        structs: c.structs,
        enums: c.enums,
        fns,
        locals: c.locals,
        loop_places: c.loop_places,
        views: program
            .items
            .iter()
            .zip(&ids)
            .filter_map(|(item, id)| match (item, id) {
                (Item::View(v), Some(id)) => Some((v.name.name.clone(), c.views[*id].clone())),
                _ => None,
            })
            .collect(),
        view_fns: c.view_fns,
    }
}

#[derive(Clone, Copy, Debug)]
enum Global {
    Struct(usize),
    Enum(usize),
    Fn(usize),
    View(usize),
    App,
}

#[derive(Clone, Debug)]
enum Kind {
    Let,
    Var,
    State,
    Bind,
    Param,
    /// Loop variable over a list that can be changed: itself changeable.
    LoopPlace,
    /// Loop variable over a value that can't be changed.
    LoopLet,
    Field,
    SelfValue,
    Fn(FnSig, FnOrigin),
}

#[derive(Clone, Debug)]
enum FnOrigin {
    Method(Ty),
    View,
}

#[derive(Clone, Debug)]
struct Local {
    name: String,
    ty: Ty,
    kind: Kind,
    span: Span,
}

enum PlaceErr {
    NotPlace,
    Const { what: String, decl: Span, help: &'static str },
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Value,
    Content,
}

const PRIMITIVES: &[&str] = &["Int", "Float", "Bool", "String", "Data", "Range", "View"];
const MUTATING: &[&str] =
    &["append", "insert", "remove", "removeAll", "removeValue", "removeFirst", "removeLast", "appendAll"];

struct Checker<'a> {
    diags: &'a mut Diagnostics,
    builtins: Builtins,
    structs: Vec<StructDef>,
    enums: Vec<EnumDef>,
    fns: Vec<FnSig>,
    views: Vec<Vec<ParamSig>>,
    globals: HashMap<String, Global>,
    /// Declaration spans of user-defined globals.
    user_spans: HashMap<String, Span>,
    scopes: Vec<Vec<Local>>,
    /// Return type of each enclosing function or closure.
    returns: Vec<Ty>,
    /// Inside a `Picker`'s content: the type `.tag(…)` expects.
    tag_ty: Option<Ty>,
    types: HashMap<ExprId, Ty>,
    calls: HashMap<ExprId, CallInfo>,
    /// One frame per call being checked, innermost last.
    frames: Vec<CallFrame>,
    locals: HashMap<Span, Ty>,
    loop_places: HashSet<Span>,
    view_fns: HashMap<(String, String), FnSig>,
}

impl Checker<'_> {
    // ===== errors and type display =====

    fn error(&mut self, span: Span, msg: impl Into<String>) -> &mut Diagnostic {
        self.diags.push(Diagnostic::error(span, msg));
        self.diags.list.last_mut().unwrap()
    }

    fn show(&self, t: &Ty) -> String {
        match t {
            Ty::Error => "?".into(),
            Ty::Void => "()".into(),
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Str => "String".into(),
            Ty::Data => "Data".into(),
            Ty::Range => "Range".into(),
            Ty::View => "View".into(),
            Ty::Any => "any value".into(),
            Ty::List(t) => format!("[{}]", self.show(t)),
            Ty::Dict(k, v) => format!("[{}: {}]", self.show(k), self.show(v)),
            Ty::Opt(t) if matches!(**t, Ty::Fn(..)) => format!("({})?", self.show(t)),
            Ty::Opt(t) => format!("{}?", self.show(t)),
            Ty::Fn(ps, r) => {
                let ps: Vec<String> = ps.iter().map(|p| self.show(p)).collect();
                let ret = if **r == Ty::Void { String::new() } else { format!(" -> {}", self.show(r)) };
                format!("fn({}){ret}", ps.join(", "))
            }
            Ty::Struct(id) => self.structs[*id].name.clone(),
            Ty::Enum(id) => self.enums[*id].name.clone(),
        }
    }

    /// "`Int`", or "no value" for `()`.
    fn a(&self, t: &Ty) -> String {
        if *t == Ty::Void { "no value".into() } else { format!("`{}`", self.show(t)) }
    }

    fn mismatch(&mut self, span: Span, expected: &Ty, found: &Ty) {
        if expected.has_error() || found.has_error() {
            return;
        }
        let msg = format!("expected {}, found {}", self.a(expected), self.a(found));
        let label = if *found == Ty::Void {
            "this doesn't produce a value".to_string()
        } else {
            format!("this is {}", self.a(found))
        };
        let help = match (expected, found) {
            (Ty::Float, Ty::Int) => Some("convert it with `Float(…)`".to_string()),
            (Ty::Int, Ty::Float) => Some("convert it with `Int(…)`, which drops the fraction".to_string()),
            (Ty::Str, Ty::Int | Ty::Float | Ty::Bool) => Some("turn it into text with \"{…}\"".to_string()),
            (e, Ty::Opt(inner)) if fits(inner, e) => Some(
                "this value might be `nil`; unwrap it with `if let`, or give a default with `?? …`".to_string(),
            ),
            _ => None,
        };
        let d = self.error(span, msg);
        d.label(label);
        if let Some(h) = help {
            d.help(h);
        }
    }

    // ===== scopes =====

    fn lookup(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|l| l.name == name)
    }

    fn declare(&mut self, name: &Ident, ty: Ty, kind: Kind) {
        if name.name == "_" {
            return;
        }
        self.locals.insert(name.span, ty.clone());
        if matches!(kind, Kind::LoopPlace) {
            self.loop_places.insert(name.span);
        }
        let scope = self.scopes.last_mut().expect("no scope");
        if let Some(prev) = scope.iter().find(|l| l.name == name.name) {
            let prev = prev.span;
            self.error(name.span, format!("`{}` is already declared here", name.name))
                .label("declared again")
                .note_at(prev, "first declared here")
                .help("use a different name");
        }
        self.scopes.last_mut().unwrap().push(Local { name: name.name.clone(), ty, kind, span: name.span });
    }

    fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(Vec::new());
        let r = f(self);
        self.scopes.pop();
        r
    }

    /// Every name visible here, for "did you mean" suggestions.
    fn visible_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.scopes.iter().flatten().map(|l| l.name.clone()).collect();
        names.extend(self.globals.keys().cloned());
        names.extend(builtins::VIEW_NAMES.iter().chain(builtins::FUNCTION_NAMES).map(|s| s.to_string()));
        names
    }

    fn unknown_name(&mut self, name: &str, span: Span) {
        let names = self.visible_names();
        let hint = suggest(name, names.iter().map(String::as_str)).map(|s| format!("did you mean `{s}`?"));
        let d = self.error(span, format!("cannot find `{name}`"));
        d.label("not found");
        if let Some(h) = hint {
            d.help(h);
        }
    }

    // ===== pass 1: collect global names =====

    fn collect(&mut self, program: &Program, require_entry: bool) -> Vec<Option<usize>> {
        let mut ids = Vec::new();
        let mut app: Option<Span> = None;
        for item in &program.items {
            let name = item.name();
            let builtin = PRIMITIVES.contains(&name.name.as_str())
                || builtins::VIEW_NAMES.contains(&name.name.as_str())
                || builtins::FUNCTION_NAMES.contains(&name.name.as_str())
                || (self.globals.contains_key(&name.name) && !self.user_spans.contains_key(&name.name));
            if builtin {
                self.error(name.span, format!("`{}` is a built-in name and can't be redeclared", name.name))
                    .help("choose a different name");
                ids.push(None);
                continue;
            }
            if let Some(prev) = self.user_spans.get(&name.name).copied() {
                self.error(name.span, format!("`{}` is declared more than once", name.name))
                    .label("declared again")
                    .note_at(prev, "first declared here")
                    .help("names must be unique across all files in the program");
                ids.push(None);
                continue;
            }
            let (global, id) = match item {
                Item::Struct(d) => {
                    let span = d.name.span;
                    self.structs.push(StructDef { name: d.name.name.clone(), span, fields: vec![], methods: vec![] });
                    (Global::Struct(self.structs.len() - 1), Some(self.structs.len() - 1))
                }
                Item::Enum(d) => {
                    let span = Some(d.name.span);
                    self.enums.push(EnumDef { name: d.name.name.clone(), span, cases: vec![], methods: vec![] });
                    (Global::Enum(self.enums.len() - 1), Some(self.enums.len() - 1))
                }
                Item::Fn(_) => {
                    self.fns.push(FnSig { params: vec![], ret: Ty::Void });
                    (Global::Fn(self.fns.len() - 1), Some(self.fns.len() - 1))
                }
                Item::View(_) => {
                    self.views.push(Vec::new());
                    (Global::View(self.views.len() - 1), Some(self.views.len() - 1))
                }
                Item::App(_) => {
                    if let Some(first) = app {
                        self.error(name.span, "a program can only have one `app`")
                            .note_at(first, "the first `app` is here")
                            .help("turn this one into a `view` and show it from the app");
                    }
                    app.get_or_insert(name.span);
                    (Global::App, None)
                }
            };
            self.globals.insert(name.name.clone(), global);
            self.user_spans.insert(name.name.clone(), name.span);
            ids.push(id);
        }
        let has_main = program.items.iter().any(|i| matches!(i, Item::Fn(f) if f.name.name == "main"));
        if require_entry && app.is_none() && !has_main {
            self.error(Span::default(), "this program has no entry point")
                .help("add an `app`, like `app Hello { Text(\"Hello\") }`, or a `fn main() { … }`");
        }
        if let (Some(app), true) = (app, has_main) {
            self.error(app, "a program can have an `app` or a `fn main()`, but not both")
                .help("remove `fn main()`; the `app` is where the program starts");
        }
        ids
    }

    // ===== pass 2: signatures =====

    fn resolve(&mut self, t: &TypeExpr) -> Ty {
        match t {
            TypeExpr::Named(id) => match id.name.as_str() {
                "Int" => Ty::Int,
                "Float" => Ty::Float,
                "Bool" => Ty::Bool,
                "String" => Ty::Str,
                "Data" => Ty::Data,
                "Range" => Ty::Range,
                "View" => Ty::View,
                name => match self.globals.get(name) {
                    Some(Global::Struct(i)) => Ty::Struct(*i),
                    Some(Global::Enum(i)) => Ty::Enum(*i),
                    Some(Global::View(_)) => {
                        self.error(id.span, format!("`{name}` is a view, not a type"))
                            .help("use `View` for a parameter that accepts any view");
                        Ty::Error
                    }
                    Some(Global::Fn(_) | Global::App) => {
                        self.error(id.span, format!("`{name}` is not a type"));
                        Ty::Error
                    }
                    None => {
                        let mut names: Vec<&str> = PRIMITIVES.to_vec();
                        let types: Vec<String> = self
                            .globals
                            .iter()
                            .filter(|(_, g)| matches!(g, Global::Struct(_) | Global::Enum(_)))
                            .map(|(n, _)| n.clone())
                            .collect();
                        names.extend(types.iter().map(String::as_str));
                        let hint = suggest(name, names).map(|s| format!("did you mean `{s}`?"));
                        let d = self.error(id.span, format!("unknown type `{name}`"));
                        d.label("not a known type");
                        if let Some(h) = hint {
                            d.help(h);
                        }
                        Ty::Error
                    }
                },
            },
            TypeExpr::List(t, _) => Ty::list(self.resolve(t)),
            TypeExpr::Dict(kt, v, _) => {
                let k = self.resolve(kt);
                self.check_key(&k, kt.span());
                let v = self.resolve(v);
                Ty::Dict(Box::new(k), Box::new(v))
            }
            TypeExpr::Optional(t, _) => Ty::opt(self.resolve(t)),
            TypeExpr::Fn(ps, r, _) => {
                let ps = ps.iter().map(|p| self.resolve(p)).collect();
                let r = r.as_ref().map_or(Ty::Void, |r| self.resolve(r));
                Ty::Fn(ps, Box::new(r))
            }
        }
    }

    fn params_sig(&mut self, params: &[Param], allow_bind: bool) -> Vec<ParamSig> {
        let mut seen: HashMap<&str, Span> = HashMap::new();
        let mut out = Vec::new();
        for p in params {
            if let Some(prev) = seen.insert(&p.name.name, p.name.span) {
                self.error(p.name.span, format!("parameter `{}` is declared twice", p.name.name))
                    .note_at(prev, "first declared here");
            }
            if p.bind && !allow_bind {
                self.error(p.span, "`bind` parameters are only allowed on views")
                    .help("remove `bind`; functions receive a copy of the value");
            }
            let ty = self.resolve(&p.ty);
            if p.bind && ty == Ty::View {
                self.error(p.span, "a `View` can't be bound").help("remove `bind`");
            }
            out.push(ParamSig {
                label: p.name.name.clone(),
                unlabeled: p.unlabeled,
                ty,
                has_default: p.default.is_some(),
                bind: p.bind && allow_bind,
                sets_tag: false,
                tag_fn: false,
            });
        }
        out
    }

    fn fn_sig(&mut self, f: &FnDecl) -> FnSig {
        let params = self.params_sig(&f.params, false);
        let ret = f.ret.as_ref().map_or(Ty::Void, |t| self.resolve(t));
        FnSig { params, ret }
    }

    fn methods(&mut self, decls: &[FnDecl], fields: &HashSet<String>, owner: &str) -> Vec<MethodDef> {
        let mut out: Vec<MethodDef> = Vec::new();
        for m in decls {
            if fields.contains(&m.name.name) || out.iter().any(|o| o.name == m.name.name) {
                self.error(m.name.span, format!("`{owner}` already has a member named `{}`", m.name.name));
                continue;
            }
            let sig = self.fn_sig(m);
            let mut roots = fields.clone();
            roots.insert("self".into());
            out.push(MethodDef {
                name: m.name.name.clone(),
                sig,
                mutating: block_mutates(&m.body, &roots),
                span: m.name.span,
            });
        }
        out
    }

    fn signatures(&mut self, program: &Program, ids: &[Option<usize>]) {
        for (item, id) in program.items.iter().zip(ids) {
            let Some(id) = *id else {
                if let Item::App(_) = item {
                    // Apps have no signature.
                }
                continue;
            };
            match item {
                Item::Struct(d) => {
                    let mut fields: Vec<FieldDef> = Vec::new();
                    for f in &d.fields {
                        if let Some(prev) = fields.iter().find(|x| x.name == f.name.name) {
                            let prev = prev.span;
                            self.error(f.name.span, format!("field `{}` is declared twice", f.name.name))
                                .note_at(prev, "first declared here");
                            continue;
                        }
                        let ty = self.resolve(&f.ty);
                        if ty == Ty::View {
                            self.error(f.ty.span(), "struct fields can't hold views")
                                .help("store the data the view needs, and build the view in a `view`");
                        }
                        fields.push(FieldDef {
                            name: f.name.name.clone(),
                            ty,
                            has_default: f.default.is_some(),
                            span: f.name.span,
                        });
                    }
                    let names = fields.iter().map(|f| f.name.clone()).collect();
                    let methods = self.methods(&d.methods, &names, &d.name.name);
                    self.structs[id].fields = fields;
                    self.structs[id].methods = methods;
                }
                Item::Enum(d) => {
                    let mut cases: Vec<CaseDef> = Vec::new();
                    for c in &d.cases {
                        if cases.iter().any(|x| x.name == c.name.name) {
                            self.error(c.name.span, format!("case `{}` is declared twice", c.name.name));
                            continue;
                        }
                        let fields = self.params_sig(&c.fields, false);
                        cases.push(CaseDef { name: c.name.name.clone(), fields });
                    }
                    let names = cases.iter().map(|c| c.name.clone()).collect();
                    let methods = self.methods(&d.methods, &names, &d.name.name);
                    self.enums[id].cases = cases;
                    self.enums[id].methods = methods;
                }
                Item::Fn(f) => self.fns[id] = self.fn_sig(f),
                Item::View(v) => self.views[id] = self.params_sig(&v.params, true),
                Item::App(_) => {}
            }
        }
        self.propagate_mutation(program, ids);
    }

    /// A method also changes its value if it calls one of its own type's
    /// changing methods (`increment()`, `self.increment()`) or a changing
    /// method of one of its fields (`counter.increment()`), however deep.
    fn propagate_mutation(&mut self, program: &Program, ids: &[Option<usize>]) {
        // (is a struct, type id, method index, the calls it makes)
        let mut calls: Vec<(bool, usize, usize, Vec<MethodCall>)> = Vec::new();
        for (item, id) in program.items.iter().zip(ids) {
            let Some(id) = *id else { continue };
            let (is_struct, decls, roots): (bool, &[FnDecl], HashSet<String>) = match item {
                Item::Struct(d) => (true, &d.methods, self.structs[id].fields.iter().map(|f| f.name.clone()).collect()),
                Item::Enum(d) => (false, &d.methods, HashSet::new()),
                _ => continue,
            };
            let methods = if is_struct { &self.structs[id].methods } else { &self.enums[id].methods };
            for decl in decls {
                // Methods rejected as duplicates have no MethodDef.
                let Some(index) = methods.iter().position(|m| m.name == decl.name.name && m.span == decl.name.span) else {
                    continue;
                };
                calls.push((is_struct, id, index, method_calls(&decl.body, &roots)));
            }
        }
        loop {
            let mut changed = false;
            for (is_struct, id, index, made) in &calls {
                let methods = if *is_struct { &self.structs[*id].methods } else { &self.enums[*id].methods };
                if methods[*index].mutating {
                    continue;
                }
                let mutates = made.iter().any(|c| match c {
                    MethodCall::Own(name) => methods.iter().any(|m| m.name == *name && m.mutating),
                    MethodCall::Field(field, name) => {
                        let ty = self.structs[*id].fields.iter().find(|f| f.name == *field).map(|f| &f.ty);
                        match ty {
                            Some(Ty::Struct(s)) => self.structs[*s].methods.iter().any(|m| m.name == *name && m.mutating),
                            Some(Ty::Enum(e)) => self.enums[*e].methods.iter().any(|m| m.name == *name && m.mutating),
                            _ => false,
                        }
                    }
                    // A method of a field's field: any changing method of that name.
                    MethodCall::Deep(name) => self
                        .structs
                        .iter()
                        .flat_map(|s| &s.methods)
                        .chain(self.enums.iter().flat_map(|e| &e.methods))
                        .any(|m| m.name == *name && m.mutating),
                });
                if mutates {
                    let methods = if *is_struct { &mut self.structs[*id].methods } else { &mut self.enums[*id].methods };
                    methods[*index].mutating = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn check_main(&mut self, program: &Program, require_entry: bool) {
        if !require_entry {
            return;
        }
        for item in &program.items {
            if let Item::Fn(f) = item {
                if f.name.name == "main" && (!f.params.is_empty() || f.ret.is_some()) {
                    self.error(f.name.span, "`main` can't take parameters or return a value")
                        .help("declare it as `fn main() { … }`");
                }
            }
        }
    }

    /// A struct can't contain itself (directly or through other structs or
    /// optionals): its values would be infinitely large. Lists are fine.
    fn check_recursive_structs(&mut self) {
        fn reaches(structs: &[StructDef], from: usize, target: usize, seen: &mut HashSet<usize>) -> bool {
            fn inline_structs(t: &Ty, out: &mut Vec<usize>) {
                match t {
                    Ty::Struct(id) => out.push(*id),
                    Ty::Opt(inner) => inline_structs(inner, out),
                    _ => {}
                }
            }
            let mut next = Vec::new();
            for f in &structs[from].fields {
                inline_structs(&f.ty, &mut next);
            }
            next.into_iter().any(|id| id == target || (seen.insert(id) && reaches(structs, id, target, seen)))
        }
        for id in 0..self.structs.len() {
            if reaches(&self.structs, id, id, &mut HashSet::new()) {
                let (span, name) = (self.structs[id].span, self.structs[id].name.clone());
                self.error(span, format!("`{name}` contains itself, so its values would be infinitely large"))
                    .help("use a list (`[T]`) for nested items, like `children: [Node]`");
            }
        }
    }

    // ===== pass 3: bodies =====

    fn bodies(&mut self, program: &Program, ids: &[Option<usize>]) {
        for (item, id) in program.items.iter().zip(ids) {
            match item {
                Item::App(app) => {
                    let params = vec![
                        ParamSig::new("title", Ty::Str).default(),
                        ParamSig::new("width", Ty::Float).default(),
                        ParamSig::new("height", Ty::Float).default(),
                    ];
                    self.with_scope(|c| c.args("`app`", &params, &app.args, None, app.args_span));
                    self.view_body(&[], &[], &app.body, &app.name);
                }
                _ if id.is_none() => {}
                Item::Struct(d) => {
                    let id = id.unwrap();
                    for f in &d.fields {
                        if let Some(default) = &f.default {
                            let ty = self.resolve(&f.ty);
                            self.default_value(default, &ty);
                        }
                    }
                    let fields: Vec<Local> = self.structs[id]
                        .fields
                        .iter()
                        .map(|f| Local { name: f.name.clone(), ty: f.ty.clone(), kind: Kind::Field, span: f.span })
                        .collect();
                    let methods = self.structs[id].methods.clone();
                    self.method_bodies(&d.methods, &methods, Ty::Struct(id), fields);
                }
                Item::Enum(d) => {
                    let id = id.unwrap();
                    let methods = self.enums[id].methods.clone();
                    self.method_bodies(&d.methods, &methods, Ty::Enum(id), Vec::new());
                }
                Item::Fn(f) => {
                    let sig = self.fns[id.unwrap()].clone();
                    self.fn_body(f, &sig);
                }
                Item::View(v) => {
                    let sig = self.views[id.unwrap()].clone();
                    self.view_body(&v.params, &sig, &v.body, &v.name);
                }
            }
        }
    }

    fn method_bodies(&mut self, decls: &[FnDecl], methods: &[MethodDef], self_ty: Ty, fields: Vec<Local>) {
        for decl in decls {
            let Some(m) = methods.iter().find(|m| m.name == decl.name.name) else { continue };
            self.scopes.push(fields.clone());
            let scope = self.scopes.last_mut().unwrap();
            scope.push(Local { name: "self".into(), ty: self_ty.clone(), kind: Kind::SelfValue, span: decl.name.span });
            for other in methods {
                scope.push(Local {
                    name: other.name.clone(),
                    ty: fn_type(&other.sig),
                    kind: Kind::Fn(other.sig.clone(), FnOrigin::Method(self_ty.clone())),
                    span: other.span,
                });
            }
            self.fn_body(decl, &m.sig);
            self.scopes.pop();
        }
    }

    fn fn_body(&mut self, f: &FnDecl, sig: &FnSig) {
        self.with_scope(|c| {
            for (p, ps) in f.params.iter().zip(&sig.params) {
                if let Some(default) = &p.default {
                    c.default_value(default, &ps.ty);
                }
                c.declare(&p.name, ps.ty.clone(), Kind::Param);
            }
            c.returns.push(sig.ret.clone());
            c.block(&f.body, Some(&sig.ret));
            c.returns.pop();
        });
    }

    /// Default values are evaluated where the call is made, so they can only
    /// use global names, not other parameters or local variables.
    fn default_value(&mut self, e: &Expr, ty: &Ty) {
        let saved = std::mem::take(&mut self.scopes);
        self.scopes.push(Vec::new());
        self.expect(e, ty);
        self.scopes = saved;
    }

    fn view_body(&mut self, params: &[Param], sig: &[ParamSig], body: &Block, name: &Ident) {
        self.with_scope(|c| {
            for (p, ps) in params.iter().zip(sig) {
                if let Some(default) = &p.default {
                    c.default_value(default, &ps.ty);
                }
                let kind = if ps.bind { Kind::Bind } else { Kind::Param };
                c.declare(&p.name, ps.ty.clone(), kind);
            }
            for s in &body.stmts {
                if let Stmt::State(d) = s {
                    c.local_decl(d, Kind::State);
                }
            }
            let mut fns = Vec::new();
            for s in &body.stmts {
                if let Stmt::Fn(f) = s {
                    let sig = c.fn_sig(f);
                    c.view_fns.insert((name.name.clone(), f.name.name.clone()), sig.clone());
                    c.declare(&f.name, fn_type(&sig), Kind::Fn(sig.clone(), FnOrigin::View));
                    fns.push((f, sig));
                }
            }
            for (f, sig) in fns {
                c.fn_body(f, &sig);
            }
            let shows_something = body.stmts.iter().any(|s| !matches!(s, Stmt::State(_) | Stmt::Fn(_)));
            if !shows_something {
                c.error(name.span, format!("`{}` doesn't show anything", name.name))
                    .help("add a view to its body, like `Text(\"Hello\")`");
            }
            c.content_block(body, true);
        });
    }

    // ===== statements =====

    fn local_decl(&mut self, d: &LocalDecl, kind: Kind) {
        let declared = d.ty.as_ref().map(|t| self.resolve(t));
        let ty = match declared {
            Some(t) => {
                self.expect(&d.value, &t);
                t
            }
            None => {
                let t = self.expr(&d.value, None);
                if t == Ty::Void {
                    self.error(d.value.span, format!("`{}` can't be set to this, because it produces no value", d.name.name))
                        .label("produces no value");
                    Ty::Error
                } else {
                    t
                }
            }
        };
        self.declare(&d.name, ty, kind);
    }

    /// Checks a block. With `expected = Some(t)` the last line must produce a
    /// `t` (unless `t` is `()`, which discards it); with `None` the block's
    /// type is inferred from its last line.
    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> Ty {
        self.with_scope(|c| {
            let n = b.stmts.len();
            let mut result = Ty::Void;
            for (i, s) in b.stmts.iter().enumerate() {
                match s {
                    Stmt::Expr(e) if i + 1 == n => {
                        result = match expected {
                            Some(t) => {
                                c.expect_or_discard(e, t);
                                t.clone()
                            }
                            None => c.expr(e, None),
                        }
                    }
                    s => c.stmt(s),
                }
            }
            if let Some(t) = expected {
                let ends_ok = matches!(b.stmts.last(), Some(Stmt::Expr(_) | Stmt::Return { .. }));
                if *t != Ty::Void && !t.has_error() && !ends_ok {
                    let brace = Span { lo: b.span.hi.saturating_sub(1), ..b.span };
                    let what = c.a(t);
                    c.error(brace, format!("this block must end with {what}"))
                        .label(format!("expected {what} before this"))
                        .help("put the value on the last line, or use `return …`");
                }
                result = t.clone();
            }
            result
        })
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(d) => self.local_decl(d, Kind::Let),
            Stmt::Var(d) => self.local_decl(d, Kind::Var),
            Stmt::State(d) => {
                self.error(d.span, "`state` can only be declared directly inside a `view` or `app`")
                    .help("use `var` for a local variable");
                self.local_decl(d, Kind::Var);
            }
            Stmt::Fn(f) => {
                self.error(f.name.span, "functions can't be declared here")
                    .help("declare it at the top level of a file, or directly inside a `view`");
            }
            Stmt::Assign { target, op, value, span } => self.assign(target, *op, value, *span),
            Stmt::Return { value, span } => {
                let Some(ret) = self.returns.last().cloned() else {
                    self.error(*span, "`return` can only be used inside a function");
                    return;
                };
                match value {
                    Some(v) if ret == Ty::Void => {
                        self.expr(v, None);
                        self.error(v.span, "this function doesn't return a value")
                            .help("remove the value, or declare a return type like `-> Int`");
                    }
                    Some(v) => {
                        self.expect(v, &ret);
                    }
                    None if ret != Ty::Void && !ret.has_error() => {
                        let what = self.a(&ret);
                        self.error(*span, format!("this function must return {what}"))
                            .help(format!("write `return` followed by {what}"));
                    }
                    None => {}
                }
            }
            Stmt::For { var, iter, body, .. } => {
                let (elem, place) = self.iter_type(iter);
                self.with_scope(|c| {
                    c.declare(var, elem, if place { Kind::LoopPlace } else { Kind::LoopLet });
                    c.block(body, Some(&Ty::Void));
                });
            }
            Stmt::Expr(e) => self.expect_or_discard(e, &Ty::Void),
        }
    }

    fn assign(&mut self, target: &Expr, op: AssignOp, value: &Expr, span: Span) {
        let tt = self.expr(target, None);
        if !tt.has_error()
            && let Err(err) = self.place(target) {
                self.report_place(err, target.span, "change");
            }
        match op {
            AssignOp::Set => {
                self.expect(value, &tt);
            }
            AssignOp::Add if matches!(tt, Ty::Int | Ty::Float | Ty::Str | Ty::List(_) | Ty::Error) => {
                self.expect(value, &tt);
            }
            _ if tt.is_numeric() => {
                self.expect(value, &tt);
            }
            _ => {
                let sym = match op {
                    AssignOp::Add => "+=",
                    AssignOp::Sub => "-=",
                    AssignOp::Mul => "*=",
                    _ => "/=",
                };
                let what = self.a(&tt);
                self.error(span, format!("`{sym}` can't be used on {what}"))
                    .help(if op == AssignOp::Add { "`+=` works on numbers, text and lists" } else { "this works on numbers" });
                self.expr(value, None);
            }
        }
    }

    /// Can `e` be written to? Also used for `bind` arguments and mutating calls.
    fn place(&self, e: &Expr) -> Result<(), PlaceErr> {
        match &e.kind {
            ExprKind::Name(n) => match self.lookup(n) {
                Some(l) => match l.kind {
                    Kind::Var | Kind::State | Kind::Bind | Kind::LoopPlace | Kind::Field | Kind::SelfValue => Ok(()),
                    Kind::Let => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a constant"),
                        decl: l.span,
                        help: "declare it with `var` to be able to change it",
                    }),
                    Kind::Param => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a parameter"),
                        decl: l.span,
                        help: "parameters are copies and can't be changed; copy it into a `var`, or (in a view) declare it as `bind`",
                    }),
                    Kind::LoopLet => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a loop variable"),
                        decl: l.span,
                        help: "loop over a `var` or `state` list to be able to change its items",
                    }),
                    Kind::Fn(..) => Err(PlaceErr::NotPlace),
                },
                None => Err(PlaceErr::NotPlace),
            },
            ExprKind::Member { object, optional: false, .. } => self.place(object),
            ExprKind::Index(object, _) => self.place(object),
            _ => Err(PlaceErr::NotPlace),
        }
    }

    fn report_place(&mut self, err: PlaceErr, span: Span, action: &str) {
        match err {
            PlaceErr::NotPlace => {
                self.error(span, format!("this can't {action}"))
                    .label("this is a value, not a variable")
                    .help("only variables, `state`, and their fields and items can be changed");
            }
            PlaceErr::Const { what, decl, help } => {
                self.error(span, format!("can't {action} {what}")).note_at(decl, "declared here").help(help);
            }
        }
    }

    /// Element type of a `for` loop, and whether the loop variable is a place.
    fn iter_type(&mut self, iter: &Expr) -> (Ty, bool) {
        let t = self.expr(iter, None);
        match t {
            Ty::List(el) => (*el, self.place(iter).is_ok()),
            Ty::Range => (Ty::Int, false),
            Ty::Error => (Ty::Error, false),
            Ty::Dict(..) => {
                self.error(iter.span, "a dictionary can't be looped over directly")
                    .help("loop over its `.keys` or `.values`");
                (Ty::Error, false)
            }
            Ty::Opt(_) => {
                self.error(iter.span, "this might be `nil`, so it can't be looped over")
                    .help("unwrap it first with `if let`, or give a default with `?? []`");
                (Ty::Error, false)
            }
            t => {
                let what = self.a(&t);
                self.error(iter.span, format!("can't loop over {what}"))
                    .help("loop over a list like `todos`, or a range like `0..10`");
                (Ty::Error, false)
            }
        }
    }

    // ===== view content =====

    fn content_block(&mut self, b: &Block, top: bool) {
        self.with_scope(|c| {
            for s in &b.stmts {
                match s {
                    Stmt::Expr(e) => c.content_expr(e),
                    Stmt::For { var, iter, body, .. } => {
                        let (elem, place) = c.iter_type(iter);
                        c.with_scope(|c| {
                            c.declare(var, elem, if place { Kind::LoopPlace } else { Kind::LoopLet });
                            c.content_block(body, false);
                        });
                    }
                    Stmt::Let(d) => c.local_decl(d, Kind::Let),
                    Stmt::Var(d) => {
                        c.error(d.span, "`var` can't be used while building a view")
                            .help("use `let` here, or `state` at the top of the view for values that change");
                        c.local_decl(d, Kind::Let);
                    }
                    Stmt::State(_) | Stmt::Fn(_) if top => {}
                    Stmt::State(d) => {
                        c.error(d.span, "`state` must be declared directly inside the view, not in a nested block")
                            .help("move it to the top of the view");
                    }
                    Stmt::Fn(f) => {
                        c.error(f.name.span, "functions must be declared directly inside the view, not in a nested block");
                    }
                    Stmt::Assign { span, .. } => {
                        c.error(*span, "views can't change values while they're being built")
                            .help("change values in an action instead, like `Button(\"Add\") { count += 1 }`");
                    }
                    Stmt::Return { span, .. } => {
                        c.error(*span, "`return` can't be used in a view body")
                            .help("use `if` to choose which views to show");
                    }
                }
            }
        });
    }

    fn content_expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::If { cond, then, otherwise } => {
                self.with_scope(|c| {
                    c.cond(cond);
                    c.content_block(then, false);
                });
                match otherwise {
                    Some(Else::Block(b)) => self.content_block(b, false),
                    Some(Else::If(e)) => self.content_expr(e),
                    None => {}
                }
                self.types.insert(e.id, Ty::View);
            }
            ExprKind::Match { subject, arms } => {
                self.match_expr(subject, arms, None, e.span, Mode::Content);
                self.types.insert(e.id, Ty::View);
            }
            ExprKind::Block(b) => self.content_block(b, false),
            _ => {
                let t = self.expr(e, Some(&Ty::View));
                if t != Ty::View && !t.has_error() {
                    let msg = if t == Ty::Void {
                        "only views can appear here".to_string()
                    } else {
                        format!("expected a view, found {}", self.a(&t))
                    };
                    let help = match t {
                        Ty::Str => "show text with `Text(…)`",
                        Ty::Void => "to run code when something happens, use an action like `Button(\"Go\") { … }`",
                        _ => "only views like `Text(…)` or `Button(…)` can go in a view body",
                    };
                    self.error(e.span, msg).help(help);
                }
            }
        }
    }

    fn content_closure(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Closure { params, body } => {
                if let Some(p) = params.first() {
                    self.error(p.span, "a view's content block doesn't take parameters")
                        .help("remove the names before `in`");
                }
                self.content_block(body, false);
                self.types.insert(e.id, Ty::View);
            }
            _ => {
                self.expect(e, &Ty::View);
            }
        }
    }

    // ===== expressions =====

    fn expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        let t = self.expr_inner(e, expected);
        self.types.insert(e.id, t.clone());
        t
    }

    /// Checks `e` against `expected`, reporting a mismatch.
    fn expect(&mut self, e: &Expr, expected: &Ty) -> Ty {
        let t = self.expr(e, Some(expected));
        if !fits(&t, expected) {
            self.mismatch(e.span, expected, &t);
        }
        t
    }

    /// Like `expect`, but `()` means "any value, ignored".
    fn expect_or_discard(&mut self, e: &Expr, expected: &Ty) {
        if *expected == Ty::Void {
            let passes_down = matches!(e.kind, ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Block(_));
            self.expr(e, if passes_down { Some(&Ty::Void) } else { None });
        } else {
            self.expect(e, expected);
        }
    }

    fn expr_inner(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => {
                if matches!(expected.map(strip_opt), Some(Ty::Float)) { Ty::Float } else { Ty::Int }
            }
            ExprKind::Float(_) => Ty::Float,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Nil => match expected {
                Some(t @ Ty::Opt(_)) => t.clone(),
                Some(t) if !t.has_error() && *t != Ty::Any => {
                    let what = self.show(t);
                    self.error(e.span, format!("`nil` can't be used as `{what}`"))
                        .label(format!("expected `{what}`"))
                        .help(format!("only optional values can be `nil`; make the type optional: `{what}?`"));
                    Ty::Error
                }
                Some(_) => Ty::Error,
                None => {
                    self.error(e.span, "can't tell what type `nil` is here")
                        .help("give it a type, like `var name: String? = nil`");
                    Ty::Error
                }
            },
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        let t = self.expr(x, None);
                        self.showable(&t, x.span);
                    }
                }
                Ty::Str
            }
            ExprKind::Name(n) => self.name_expr(n, e.span),
            ExprKind::DotName(n) => self.dot_name(n, e.span, expected, false),
            ExprKind::List(items) => {
                let elem = match expected.map(strip_opt) {
                    Some(Ty::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                match (elem, items.first()) {
                    (Some(t), _) => {
                        for item in items {
                            self.expect(item, &t);
                        }
                        Ty::list(t)
                    }
                    (None, None) => {
                        self.error(e.span, "can't tell what type this empty list is")
                            .help("give it a type, like `var names: [String] = []`");
                        Ty::Error
                    }
                    (None, Some(first)) => {
                        let t = self.expr(first, None);
                        for item in &items[1..] {
                            self.expect(item, &t);
                        }
                        Ty::list(t)
                    }
                }
            }
            ExprKind::Dict(pairs) => {
                let kv = match expected.map(strip_opt) {
                    Some(Ty::Dict(k, v)) => Some(((**k).clone(), (**v).clone())),
                    _ => None,
                };
                let (k, v) = match (kv, pairs.first()) {
                    (Some(kv), _) => kv,
                    (None, None) => {
                        self.error(e.span, "can't tell what type this empty dictionary is")
                            .help("give it a type, like `var ages: [String: Int] = [:]`");
                        return Ty::Error;
                    }
                    (None, Some((k, v))) => {
                        let kt = self.expr(k, None);
                        self.check_key(&kt, k.span);
                        (kt, self.expr(v, None))
                    }
                };
                let skip = if expected.is_some() { 0 } else { 1 };
                for (pk, pv) in &pairs[skip..] {
                    self.expect(pk, &k);
                    self.expect(pv, &v);
                }
                Ty::Dict(Box::new(k), Box::new(v))
            }
            ExprKind::Unary(UnaryOp::Not, x) => {
                self.expect(x, &Ty::Bool);
                Ty::Bool
            }
            ExprKind::Unary(UnaryOp::Neg, x) => {
                let t = self.expr(x, expected.filter(|t| strip_opt(t) == &Ty::Float));
                if !t.is_numeric() {
                    let what = self.a(&t);
                    self.error(x.span, format!("`-` needs a number, found {what}"));
                    return Ty::Error;
                }
                t
            }
            ExprKind::Binary(op, l, r) => self.binary(*op, l, r, expected),
            ExprKind::Call { callee, args, trailing } => {
                self.frames.push(CallFrame::default());
                let t = self.call(callee, args, trailing.as_deref(), e.span, expected);
                let frame = self.frames.pop().unwrap();
                if let Some(callee) = frame.callee {
                    self.calls.insert(e.id, CallInfo { callee, args: frame.args });
                }
                t
            }
            ExprKind::Member { object, name, optional } => self.member(object, name, *optional),
            ExprKind::Index(object, index) => {
                let t = self.expr(object, None);
                match t {
                    Ty::List(el) => {
                        self.expect(index, &Ty::Int);
                        *el
                    }
                    Ty::Dict(k, v) => {
                        self.expect(index, &k);
                        Ty::opt(*v)
                    }
                    // A byte, 0–255.
                    Ty::Data => {
                        self.expect(index, &Ty::Int);
                        Ty::Int
                    }
                    Ty::Error => Ty::Error,
                    Ty::Opt(_) => {
                        self.error(object.span, "this might be `nil`, so it can't be indexed")
                            .help("unwrap it first with `if let`");
                        Ty::Error
                    }
                    Ty::Str => {
                        self.error(e.span, "text can't be indexed by number")
                            .help("use `split`, `contains` or `hasPrefix` instead");
                        Ty::Error
                    }
                    t => {
                        let what = self.a(&t);
                        self.error(object.span, format!("{what} can't be indexed with `[…]`"));
                        Ty::Error
                    }
                }
            }
            ExprKind::Closure { params, body } => self.closure(params, body, expected, e.span),
            ExprKind::If { cond, then, otherwise } => self.if_expr(cond, then, otherwise.as_ref(), expected),
            ExprKind::Match { subject, arms } => self.match_expr(subject, arms, expected, e.span, Mode::Value),
            ExprKind::Block(b) => self.block(b, expected),
            ExprKind::Error => Ty::Error,
        }
    }

    fn check_key(&mut self, t: &Ty, span: Span) {
        let ok = match t {
            Ty::Int | Ty::Str | Ty::Bool | Ty::Error => true,
            Ty::Enum(id) => self.enums[*id].cases.iter().all(|c| c.fields.is_empty()),
            _ => false,
        };
        if !ok {
            let what = self.a(t);
            self.error(span, format!("{what} can't be used as a dictionary key"))
                .help("keys can be `Int`, `String`, `Bool`, or an enum whose cases have no values");
        }
    }

    fn showable(&mut self, t: &Ty, span: Span) {
        match t {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Range | Ty::Enum(_) | Ty::Error => {}
            Ty::Opt(_) => {
                self.error(span, "this might be `nil`, so it can't be shown as text directly")
                    .help("give it a default with `??`, like \"{name ?? \"none\"}\"");
            }
            Ty::List(_) => {
                self.error(span, "a list can't be shown as text directly")
                    .help("show its size with `.count`, or join a `[String]` with `.joined(separator: \", \")`");
            }
            t => {
                let what = self.a(t);
                self.error(span, format!("{what} can't be shown as text"));
            }
        }
    }

    fn name_expr(&mut self, n: &str, span: Span) -> Ty {
        if let Some(l) = self.lookup(n) {
            return l.ty.clone();
        }
        match self.globals.get(n).copied() {
            Some(Global::Fn(i)) => fn_type(&self.fns[i]),
            Some(Global::Struct(_)) => {
                self.error(span, format!("`{n}` is a struct; to create one, call it: `{n}(…)`"));
                Ty::Error
            }
            Some(Global::Enum(id)) => {
                let example = self.enums[id].cases.first().map_or("case".to_string(), |c| c.name.clone());
                self.error(span, format!("`{n}` is an enum; pick one of its cases, like `{n}.{example}`"));
                Ty::Error
            }
            Some(Global::View(_)) => {
                self.error(span, format!("`{n}` is a view; to show it, call it: `{n}(…)`"));
                Ty::Error
            }
            Some(Global::App) => {
                self.error(span, format!("`{n}` is the app and can't be used as a value"));
                Ty::Error
            }
            // The one built-in constant.
            None if n == "pi" => Ty::Float,
            None if builtins::VIEW_NAMES.contains(&n) || builtins::FUNCTION_NAMES.contains(&n) => {
                self.error(span, format!("`{n}` must be called: `{n}(…)`"));
                Ty::Error
            }
            None => {
                self.unknown_name(n, span);
                Ty::Error
            }
        }
    }

    /// `.name` resolved against the expected enum type.
    fn dot_name(&mut self, n: &str, span: Span, expected: Option<&Ty>, called: bool) -> Ty {
        match expected.map(strip_opt) {
            Some(Ty::Enum(id)) => {
                let id = *id;
                self.enum_case(id, n, span, called);
                Ty::Enum(id)
            }
            Some(Ty::Float) if n == "infinity" => Ty::Float,
            Some(t) if t.has_error() => Ty::Error,
            Some(Ty::Float) => {
                self.error(span, format!("`Float` has no `.{n}`")).help("the only `Float` shorthand is `.infinity`");
                Ty::Error
            }
            Some(t) if *t != Ty::Any && *t != Ty::Void => {
                let what = self.a(t);
                self.error(span, format!("`.{n}` is an enum case, but {what} is expected here"));
                Ty::Error
            }
            _ => {
                self.error(span, format!("can't tell which type `.{n}` belongs to"))
                    .help(format!("write the type in front, like `Filter.{n}`"));
                Ty::Error
            }
        }
    }

    /// Checks that enum `id` has case `n`; reports missing values if `!called`.
    fn enum_case(&mut self, id: usize, n: &str, span: Span, called: bool) -> Option<CaseDef> {
        let def = &self.enums[id];
        match def.case(n) {
            Some(case) => {
                let case = case.clone();
                if !called && !case.fields.is_empty() {
                    let labels: Vec<String> = case.fields.iter().map(|f| format!("{}: …", f.label)).collect();
                    self.error(span, format!("`.{n}` needs values"))
                        .help(format!("write `.{n}({})`", labels.join(", ")));
                }
                Some(case)
            }
            None => {
                let enum_name = def.name.clone();
                let hint = suggest(n, def.cases.iter().map(|c| c.name.as_str())).map(|s| format!("did you mean `.{s}`?"));
                let cases: Vec<String> = def.cases.iter().map(|c| format!("`.{}`", c.name)).collect();
                let d = self.error(span, format!("`{enum_name}` has no case `.{n}`"));
                d.label("unknown case");
                d.help(hint.unwrap_or_else(|| format!("its cases are {}", cases.join(", "))));
                None
            }
        }
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr, expected: Option<&Ty>) -> Ty {
        match op {
            BinOp::And | BinOp::Or => {
                self.expect(l, &Ty::Bool);
                self.expect(r, &Ty::Bool);
                Ty::Bool
            }
            BinOp::Eq | BinOp::Ne => {
                let (lt, rt) = self.operand_pair(l, r, None);
                if !(fits(&rt, &lt) || fits(&lt, &rt)) {
                    let (a, b) = (self.a(&lt), self.a(&rt));
                    self.error(l.span.to(r.span), format!("can't compare {a} with {b}"))
                        .help("both sides of `==` must have the same type");
                } else if !equatable(&lt) && !is_nil(l) && !is_nil(r) {
                    // (Anything optional can be compared with `nil`.)
                    let a = self.a(&lt);
                    self.error(l.span, format!("{a} values can't be compared with `{}`", op.symbol()));
                }
                Ty::Bool
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (lt, rt) = self.operand_pair(l, r, None);
                if lt.has_error() || rt.has_error() {
                    return Ty::Bool;
                }
                if lt != rt {
                    let (a, b) = (self.a(&lt), self.a(&rt));
                    self.error(l.span.to(r.span), format!("can't compare {a} with {b}"))
                        .help("convert one side with `Float(…)` or `Int(…)`");
                } else if !matches!(lt, Ty::Int | Ty::Float | Ty::Str) {
                    let a = self.a(&lt);
                    self.error(l.span, format!("`{}` works on numbers and text, not {a}", op.symbol()));
                }
                Ty::Bool
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let hint = expected.map(strip_opt).filter(|t| t.is_numeric() || **t == Ty::Str);
                let (lt, rt) = self.operand_pair(l, r, hint);
                if lt.has_error() || rt.has_error() {
                    return Ty::Error;
                }
                let ok = lt == rt
                    && (lt.is_numeric() || (op == BinOp::Add && matches!(lt, Ty::Str | Ty::List(_))));
                if ok {
                    return lt;
                }
                let (a, b) = (self.a(&lt), self.a(&rt));
                let help = match (&lt, &rt) {
                    (Ty::Opt(_), _) | (_, Ty::Opt(_)) => {
                        "one side might be `nil`: give it a default with `??` (like `count ?? 0`), or unwrap it with `if let`"
                    }
                    (Ty::Str, _) | (_, Ty::Str) => "to put a value into text, use \"{…}\", like \"Total: {total}\"",
                    (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => "convert one side with `Float(…)` or `Int(…)`",
                    _ => "arithmetic works on two numbers of the same type",
                };
                self.error(l.span.to(r.span), format!("can't use `{}` on {a} and {b}", op.symbol())).help(help);
                Ty::Error
            }
            BinOp::Coalesce => {
                let lt = self.expr(l, expected.map(|t| Ty::opt(t.clone())).as_ref());
                match lt {
                    Ty::Opt(inner) => {
                        let rt = self.expr(r, Some(&inner));
                        if fits(&rt, &inner) {
                            *inner
                        } else if rt == Ty::Opt(inner.clone()) {
                            rt
                        } else {
                            self.mismatch(r.span, &inner, &rt);
                            *inner
                        }
                    }
                    Ty::Error => {
                        self.expr(r, None);
                        Ty::Error
                    }
                    t => {
                        self.error(l.span, "the left side of `??` is never `nil`")
                            .help("remove `?? …`; it only applies to optional values");
                        self.expr(r, Some(&t));
                        t
                    }
                }
            }
            BinOp::Range => {
                self.expect(l, &Ty::Int);
                self.expect(r, &Ty::Int);
                Ty::Range
            }
        }
    }

    /// Checks two operands so a literal (`1`, `nil`, `.all`) takes its type from the other side.
    fn operand_pair(&mut self, l: &Expr, r: &Expr, hint: Option<&Ty>) -> (Ty, Ty) {
        if needs_context(l) && !needs_context(r) {
            let rt = self.expr(r, hint);
            let lt = self.expr(l, Some(&rt));
            (lt, rt)
        } else {
            let lt = self.expr(l, hint);
            let rt = self.expr(r, Some(&lt));
            (lt, rt)
        }
    }

    fn cond(&mut self, cond: &Cond) {
        match cond {
            Cond::Expr(e) => {
                self.expect(e, &Ty::Bool);
            }
            Cond::Let(name, value) => {
                let t = self.expr(value, None);
                let inner = match t {
                    Ty::Opt(inner) => *inner,
                    Ty::Error => Ty::Error,
                    t => {
                        let what = self.a(&t);
                        self.error(value.span, format!("`if let` needs an optional value, but this is {what}"))
                            .help("use a plain `if`, or `let` to name the value");
                        t
                    }
                };
                self.declare(name, inner, Kind::Let);
            }
        }
    }

    fn if_expr(&mut self, cond: &Cond, then: &Block, otherwise: Option<&Else>, expected: Option<&Ty>) -> Ty {
        let then_t = self.with_scope(|c| {
            c.cond(cond);
            c.block(then, expected)
        });
        let t = expected.cloned().unwrap_or(then_t);
        match otherwise {
            Some(Else::Block(b)) => {
                self.block(b, Some(&t));
            }
            Some(Else::If(e)) => {
                self.expr(e, Some(&t));
            }
            None if t != Ty::Void && !t.has_error() => {
                let what = self.a(&t);
                let span = then.span;
                self.error(span, format!("this `if` has no `else`, so it doesn't always produce {what}"))
                    .help("add an `else { … }` branch");
            }
            None => {}
        }
        t
    }

    fn match_expr(&mut self, subject: &Expr, arms: &[Arm], expected: Option<&Ty>, span: Span, mode: Mode) -> Ty {
        let st = self.expr(subject, None);
        let mut covered: HashSet<String> = HashSet::new();
        let mut wildcard: Option<Span> = None;
        let mut had_error = false;
        let mut result = expected.cloned();
        for arm in arms {
            if let Some(w) = wildcard {
                self.error(arm.pattern.span(), "this arm can never run")
                    .note_at(w, "`_` above already matches everything")
                    .help("move `_` to the last arm");
            }
            self.with_scope(|c| {
                match &arm.pattern {
                    Pattern::Case { name, bindings, span } => match &st {
                        Ty::Enum(id) => {
                            if let Some(case) = c.enum_case(*id, &name.name, name.span, true) {
                                if !covered.insert(name.name.clone()) {
                                    c.error(*span, format!("`.{}` is already handled above", name.name));
                                }
                                if !bindings.is_empty() && bindings.len() != case.fields.len() {
                                    c.error(
                                        *span,
                                        format!(
                                            "`.{}` has {} value(s), but {} name(s) are given",
                                            name.name,
                                            case.fields.len(),
                                            bindings.len()
                                        ),
                                    );
                                }
                                for (b, f) in bindings.iter().zip(&case.fields) {
                                    c.declare(b, f.ty.clone(), Kind::Let);
                                }
                            }
                            for b in bindings.iter().skip(case_field_count(c, *id, &name.name)) {
                                c.declare(b, Ty::Error, Kind::Let);
                            }
                        }
                        Ty::Error => {
                            for b in bindings {
                                c.declare(b, Ty::Error, Kind::Let);
                            }
                        }
                        t => {
                            let what = c.a(t);
                            c.error(*span, format!("`.{}` patterns only work on enums, but this is {what}", name.name));
                        }
                    },
                    Pattern::Wildcard(s) => wildcard = Some(*s),
                    Pattern::Error(_) => had_error = true,
                    Pattern::Literal(e) => {
                        c.expect(e, &st);
                        if let ExprKind::Bool(b) = e.kind {
                            covered.insert(b.to_string());
                        }
                    }
                }
                match mode {
                    Mode::Content => match &arm.body.kind {
                        ExprKind::Block(b) => c.content_block(b, false),
                        _ => c.content_expr(&arm.body),
                    },
                    Mode::Value => match &result {
                        Some(t) => {
                            let t = t.clone();
                            c.expect_or_discard(&arm.body, &t);
                        }
                        None => result = Some(c.expr(&arm.body, None)),
                    },
                }
            });
        }
        if wildcard.is_none() && !had_error && !st.has_error() {
            let missing: Vec<String> = match &st {
                Ty::Enum(id) => self.enums[*id]
                    .cases
                    .iter()
                    .filter(|c| !covered.contains(&c.name))
                    .map(|c| format!("`.{}`", c.name))
                    .collect(),
                Ty::Bool => ["true", "false"].iter().filter(|b| !covered.contains(**b)).map(|b| format!("`{b}`")).collect(),
                _ => vec!["_".into()],
            };
            if missing == ["_"] {
                let what = self.a(&st);
                self.error(subject.span, format!("this `match` must handle every possible {what}"))
                    .help("add a `_ -> …` arm for all other values");
            } else if !missing.is_empty() {
                let _ = span;
                self.error(subject.span, format!("this `match` doesn't handle {}", missing.join(", ")))
                    .help("add arms for them, or `_ -> …` to handle the rest");
            }
        }
        if mode == Mode::Content { Ty::View } else { result.unwrap_or(Ty::Void) }
    }

    fn closure(&mut self, params: &[Ident], body: &Block, expected: Option<&Ty>, span: Span) -> Ty {
        let (ptys, ret) = match expected.map(strip_opt) {
            Some(Ty::Fn(ps, r)) => (ps.clone(), Some((**r).clone())),
            _ if params.is_empty() => (Vec::new(), None),
            _ => {
                self.error(span, "can't tell the types of this block's parameters")
                    .help("pass it where a function is expected, or give it a type: `let f: fn(Int) -> Int = { x in … }`");
                (vec![Ty::Error; params.len()], None)
            }
        };
        if params.len() != ptys.len() {
            if params.is_empty() {
                self.error(span, format!("this block receives {} value(s); name them at the start, like `{{ x in … }}`", ptys.len()));
            } else {
                self.error(span, format!("this block should take {} parameter(s), but {} are named", ptys.len(), params.len()));
            }
        }
        self.with_scope(|c| {
            for (i, p) in params.iter().enumerate() {
                c.declare(p, ptys.get(i).cloned().unwrap_or(Ty::Error), Kind::Let);
            }
            let ret = match ret {
                Some(r) if r != Ty::Any => {
                    c.returns.push(r.clone());
                    c.block(body, Some(&r));
                    r
                }
                _ => {
                    c.returns.push(Ty::Error);
                    
                    c.block(body, None)
                }
            };
            c.returns.pop();
            Ty::Fn(ptys.clone(), Box::new(ret))
        })
    }

    fn member(&mut self, object: &Expr, name: &Ident, optional: bool) -> Ty {
        // `Filter.all`: a case through its enum's name.
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none() {
                match self.globals.get(tn).copied() {
                    Some(Global::Enum(id)) => {
                        self.enum_case(id, &name.name, name.span, false);
                        return Ty::Enum(id);
                    }
                    Some(Global::Struct(_)) => {
                        self.error(object.span, format!("`{tn}` is a type; create a value with `{tn}(…)` first"));
                        return Ty::Error;
                    }
                    _ => {}
                }
            }
        let ot = self.expr(object, None);
        let optional = optional || object.in_optional_chain();
        let Some((recv, wrap)) = self.receiver(ot, object.span, optional, &name.name) else { return Ty::Error };
        let n = name.name.as_str();
        let t = match &recv {
            Ty::Error => Ty::Error,
            Ty::Struct(id) => {
                let def = &self.structs[*id];
                if let Some(f) = def.fields.iter().find(|f| f.name == n) {
                    f.ty.clone()
                } else if def.methods.iter().any(|m| m.name == n) {
                    self.error(name.span, format!("`{n}` is a method; call it with `{n}()`"));
                    Ty::Error
                } else {
                    let sname = def.name.clone();
                    let names: Vec<String> = def.fields.iter().map(|f| f.name.clone()).collect();
                    self.no_member(name, &format!("`{sname}` has no field `{n}`"), names);
                    Ty::Error
                }
            }
            Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Float | Ty::Int | Ty::Data => match builtins::property(&recv, n) {
                Some(t) => t,
                None if builtins::method_of(&recv, n).is_some() => {
                    self.error(name.span, format!("`{n}` is a method; call it with `{n}(…)`"));
                    Ty::Error
                }
                None => {
                    let what = self.a(&recv);
                    let names = builtins::member_names(&recv).iter().map(|s| s.to_string()).collect();
                    self.no_member(name, &format!("{what} has no property `{n}`"), names);
                    Ty::Error
                }
            },
            Ty::View if self.builtins.modifier(n).is_some() => {
                self.error(name.span, format!("`{n}` is a modifier; call it with `.{n}(…)`"));
                Ty::Error
            }
            Ty::Enum(id) if self.enums[*id].methods.iter().any(|m| m.name == n) => {
                self.error(name.span, format!("`{n}` is a method; call it with `{n}()`"));
                Ty::Error
            }
            t => {
                let what = self.a(t);
                self.error(name.span, format!("{what} has no property `{n}`"));
                Ty::Error
            }
        };
        if wrap { Ty::opt(t) } else { t }
    }

    fn no_member(&mut self, name: &Ident, msg: &str, candidates: Vec<String>) {
        let hint = suggest(&name.name, candidates.iter().map(String::as_str)).map(|s| format!("did you mean `{s}`?"));
        let d = self.error(name.span, msg);
        d.label("unknown");
        if let Some(h) = hint {
            d.help(h);
        }
    }

    /// Handles `?.` on a member access or call. Returns (receiver type, wrap result in `?`).
    fn receiver(&mut self, ot: Ty, span: Span, optional: bool, member: &str) -> Option<(Ty, bool)> {
        match (ot, optional) {
            (Ty::Opt(inner), true) => Some((*inner, true)),
            (Ty::Opt(_), false) => {
                self.error(span, "this might be `nil`")
                    .label("optional value")
                    .help(format!("use `?.{member}` to use it only when there is a value, or unwrap it with `if let`"));
                None
            }
            (Ty::Error, _) => Some((Ty::Error, false)),
            (t, true) => Some((t, false)),
            (t, false) => Some((t, false)),
        }
    }

    // ===== calls =====

    fn set_callee(&mut self, callee: Callee) {
        if let Some(frame) = self.frames.last_mut() {
            frame.callee = Some(callee);
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Arg], trailing: Option<&Expr>, span: Span, expected: Option<&Ty>) -> Ty {
        match &callee.kind {
            ExprKind::Name(n) => self.call_name(n, callee.span, args, trailing, span, expected),
            ExprKind::DotName(n) => {
                let t = self.dot_name(n, callee.span, expected, true);
                if let Ty::Enum(id) = t {
                    self.case_call(id, n, callee.span, args, trailing, span);
                }
                t
            }
            ExprKind::Member { object, name, optional } => {
                self.method_call(object, name, *optional, args, trailing, span)
            }
            _ => {
                let t = self.expr(callee, None);
                self.set_callee(Callee::Value);
                self.call_value(&t, "this", args, trailing, span)
            }
        }
    }

    fn call_name(
        &mut self,
        n: &str,
        name_span: Span,
        args: &[Arg],
        trailing: Option<&Expr>,
        span: Span,
        expected: Option<&Ty>,
    ) -> Ty {
        let what = format!("`{n}`");
        if let Some(l) = self.lookup(n).cloned() {
            return match l.kind {
                Kind::Fn(sig, origin) => {
                    self.set_callee(match origin {
                        FnOrigin::Method(owner) => Callee::SelfMethod { owner, name: n.to_string() },
                        FnOrigin::View => Callee::ViewFn(n.to_string()),
                    });
                    self.args(&what, &sig.params, args, trailing, span);
                    sig.ret
                }
                _ => {
                    self.set_callee(Callee::Value);
                    self.call_value(&l.ty, &what, args, trailing, span)
                }
            };
        }
        match self.globals.get(n).copied() {
            Some(Global::Fn(i)) => {
                let sig = self.fns[i].clone();
                self.set_callee(Callee::Fn(n.to_string()));
                self.args(&what, &sig.params, args, trailing, span);
                sig.ret
            }
            Some(Global::Struct(id)) => {
                self.set_callee(Callee::StructInit(id));
                let params: Vec<ParamSig> = self.structs[id]
                    .fields
                    .iter()
                    .map(|f| ParamSig { has_default: f.has_default, ..ParamSig::new(&f.name, f.ty.clone()) })
                    .collect();
                self.args(&what, &params, args, trailing, span);
                Ty::Struct(id)
            }
            Some(Global::View(i)) => {
                let params = self.views[i].clone();
                self.set_callee(Callee::View(n.to_string()));
                self.args(&what, &params, args, trailing, span);
                Ty::View
            }
            Some(Global::Enum(id)) => {
                let example = self.enums[id].cases.first().map_or("case".to_string(), |c| c.name.clone());
                self.error(name_span, format!("enums can't be called; pick a case, like `{n}.{example}`"));
                Ty::Error
            }
            Some(Global::App) => {
                self.error(name_span, "the app can't be shown inside a view")
                    .help("move its content into a `view` and show that");
                Ty::Error
            }
            None => {
                if let Some(params) = self.builtins.view(n) {
                    self.set_callee(Callee::BuiltinView(n.to_string()));
                    let saved = self.tag_ty.take();
                    self.args(&what, &params, args, trailing, span);
                    self.tag_ty = saved;
                    return Ty::View;
                }
                if builtins::FUNCTION_NAMES.contains(&n) {
                    self.set_callee(Callee::BuiltinFn(n.to_string()));
                }
                match n {
                    "Int" | "Float" | "String" => return self.conversion(n, args, trailing, span),
                    "Data" => return self.data_init(args, trailing, span),
                    "toJson" | "toBinary" | "fromJson" | "fromBinary" => {
                        return self.codec_call(n, args, trailing, span, expected);
                    }
                    "abs" | "min" | "max" => return self.numeric_fn(n, args, trailing, span),
                    "print" => {
                        let sig = self.builtins.function(n).unwrap();
                        self.args(&what, &sig.params, args, trailing, span);
                        if let [arg] = args {
                            let t = self.types.get(&arg.value.id).cloned().unwrap_or(Ty::Error);
                            self.showable(&t, arg.value.span);
                        }
                        return Ty::Void;
                    }
                    _ => {}
                }
                if let Some(sig) = self.builtins.function(n) {
                    self.args(&what, &sig.params, args, trailing, span);
                    return sig.ret;
                }
                self.unknown_name(n, name_span);
                Ty::Error
            }
        }
    }

    /// `Data()`, `Data(bytes: [Int])`, `Data(text: String)`.
    fn data_init(&mut self, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let label = args.first().and_then(|a| a.label.as_ref()).map(|l| l.name.as_str());
        match (args, label) {
            ([], _) if trailing.is_none() => {}
            ([a], Some("bytes")) => {
                self.expect(&a.value, &Ty::list(Ty::Int));
            }
            ([a], Some("text")) => {
                self.expect(&a.value, &Ty::Str);
            }
            _ => {
                for a in args {
                    self.expr(&a.value, None);
                }
                self.error(span, "make Data with `Data()`, `Data(bytes: [Int])` or `Data(text: String)`");
            }
        }
        Ty::Data
    }

    /// `toJson(value, pretty:)`, `toBinary(value)`: any value that can be
    /// saved. `fromJson(text)`, `fromBinary(data)`: the type comes from where
    /// the result goes (`let p: Project? = fromJson(text)`).
    fn codec_call(&mut self, n: &str, args: &[Arg], trailing: Option<&Expr>, span: Span, expected: Option<&Ty>) -> Ty {
        let encode = n.starts_with("to");
        let input = if n == "fromBinary" { Ty::Data } else { Ty::Str };
        let Some(first) = args.first().filter(|a| a.label.is_none() && trailing.is_none()) else {
            self.error(span, format!("write it as `{n}(value)`"));
            return Ty::Error;
        };
        if encode {
            let t = self.expr(&first.value, None);
            if let Err(why) = self.saveable(&t) {
                let what = self.a(&t);
                self.error(first.value.span, format!("{what} can't be saved with `{n}`")).help(why);
            }
            match &args[1..] {
                [] => {}
                [p] if n == "toJson" && p.label.as_ref().is_some_and(|l| l.name == "pretty") => {
                    self.expect(&p.value, &Ty::Bool);
                }
                _ => {
                    self.error(span, if n == "toJson" { "write it as `toJson(value)` or `toJson(value, pretty: true)`" } else { "write it as `toBinary(value)`" });
                }
            }
            return if n == "toJson" { Ty::Str } else { Ty::Data };
        }
        self.expect(&first.value, &input);
        if args.len() > 1 {
            self.error(span, format!("write it as `{n}(value)`"));
        }
        match expected.map(strip_opt) {
            Some(t) if !t.has_error() && *t != Ty::Any && *t != Ty::Void => {
                let t = t.clone();
                if let Err(why) = self.saveable(&t) {
                    let what = self.a(&t);
                    self.error(span, format!("{what} can't be read with `{n}`")).help(why);
                }
                Ty::opt(t)
            }
            Some(t) if t.has_error() => Ty::Error,
            _ => {
                self.error(span, format!("can't tell what type `{n}` should read"))
                    .help(format!("give the result a type, like `let project: Project? = {n}(…)`"));
                Ty::Error
            }
        }
    }

    /// Can values of `t` be written as JSON or binary? Err says why not.
    fn saveable(&self, t: &Ty) -> Result<(), String> {
        match t {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Data | Ty::Range | Ty::Error => Ok(()),
            Ty::List(e) | Ty::Opt(e) => self.saveable(e),
            Ty::Dict(k, v) => {
                if !matches!(**k, Ty::Str | Ty::Int | Ty::Bool) && !matches!(**k, Ty::Enum(_)) {
                    return Err("dictionary keys must be String, Int, Bool or an enum".into());
                }
                self.saveable(v)
            }
            Ty::Struct(id) => {
                for f in &self.structs[*id].fields {
                    self.saveable(&f.ty).map_err(|e| format!("field `{}`: {e}", f.name))?;
                }
                Ok(())
            }
            Ty::Enum(id) => {
                for c in &self.enums[*id].cases {
                    for f in &c.fields {
                        self.saveable(&f.ty).map_err(|e| format!("case `{}`: {e}", c.name))?;
                    }
                }
                Ok(())
            }
            Ty::Fn(..) => Err("functions can't be saved".into()),
            Ty::View => Err("views can't be saved".into()),
            Ty::Void | Ty::Any => Err("this has no value to save".into()),
        }
    }

    /// `abs(x)`, `min(a, b)`, `max(a, b)`: on two `Int`s or two `Float`s.
    fn numeric_fn(&mut self, n: &str, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let want = if n == "abs" { 1 } else { 2 };
        if args.len() != want || trailing.is_some() || args.iter().any(|a| a.label.is_some()) {
            let shape = if n == "abs" { "abs(x)" } else { &format!("{n}(a, b)") };
            self.error(span, format!("write it as `{shape}`"));
            for a in args {
                self.expr(&a.value, None);
            }
            return Ty::Error;
        }
        // The type comes from the first value that isn't a plain number, so
        // `max(0, x)` with a Float `x` is a Float (and `0` is taken as `0.0`).
        let literal = |e: &Expr| matches!(e.kind, ExprKind::Int(_) | ExprKind::Float(_));
        let lead = args.iter().position(|a| !literal(&a.value)).unwrap_or(0);
        let hint = args.iter().any(|a| matches!(a.value.kind, ExprKind::Float(_))).then_some(Ty::Float);
        let t = self.expr(&args[lead].value, hint.as_ref());
        if !t.is_numeric() {
            let what = self.a(&t);
            self.error(args[lead].value.span, format!("`{n}` works on numbers, not {what}"));
            return Ty::Error;
        }
        for (i, a) in args.iter().enumerate() {
            if i != lead {
                self.expect(&a.value, &t);
            }
        }
        t
    }

    /// `Int(x)`, `Float(x)`, `String(x)`.
    fn conversion(&mut self, n: &str, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let [arg] = args else {
            self.error(span, format!("`{n}(…)` takes exactly one value"));
            return Ty::Error;
        };
        if trailing.is_some() || arg.label.is_some() {
            self.error(span, format!("write the conversion as `{n}(value)`"));
        }
        let t = self.expr(&arg.value, None);
        let (target, from_text) = match n {
            "Int" => (Ty::Int, Ty::opt(Ty::Int)),
            "Float" => (Ty::Float, Ty::opt(Ty::Float)),
            _ => {
                self.showable(&t, arg.value.span);
                return Ty::Str;
            }
        };
        match t {
            Ty::Int | Ty::Float => target,
            // Parsing text can fail, so it gives an optional.
            Ty::Str => from_text,
            Ty::Error => target,
            t => {
                let what = self.a(&t);
                self.error(arg.value.span, format!("can't convert {what} to `{n}`"));
                target
            }
        }
    }

    /// Calling a value of function type: arguments have no labels.
    fn call_value(&mut self, t: &Ty, what: &str, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let Ty::Fn(ps, ret) = t else {
            if !t.has_error() {
                let ty = self.a(t);
                self.error(span, format!("{what} is {ty}, not a function"));
            }
            return Ty::Error;
        };
        let values: Vec<&Expr> = args.iter().map(|a| &a.value).chain(trailing).collect();
        if let Some(label) = args.iter().find_map(|a| a.label.as_ref()) {
            self.error(label.span, "function values take their arguments without labels")
                .help(format!("remove `{}:`", label.name));
        }
        if values.len() != ps.len() {
            self.error(span, format!("{what} takes {} argument(s), but {} were given", ps.len(), values.len()));
        }
        for (v, p) in values.iter().zip(ps) {
            self.expect(v, p);
        }
        for v in values.iter().skip(ps.len()) {
            self.expr(v, None);
        }
        (**ret).clone()
    }

    fn case_call(&mut self, id: usize, n: &str, name_span: Span, args: &[Arg], trailing: Option<&Expr>, span: Span) {
        let Some(case) = self.enum_case(id, n, name_span, true) else { return };
        let index = self.enums[id].cases.iter().position(|c| c.name == n).unwrap();
        self.set_callee(Callee::EnumCase(id, index));
        if case.fields.is_empty() {
            self.error(span, format!("`.{n}` doesn't take values")).help(format!("write `.{n}` without parentheses"));
            return;
        }
        self.args(&format!("`.{n}`"), &case.fields, args, trailing, span);
    }

    fn method_call(
        &mut self,
        object: &Expr,
        name: &Ident,
        optional: bool,
        args: &[Arg],
        trailing: Option<&Expr>,
        span: Span,
    ) -> Ty {
        let n = name.name.as_str();
        // `Shape.circle(radius: 1)`
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none()
                && let Some(Global::Enum(id)) = self.globals.get(tn).copied() {
                    self.case_call(id, n, name.span, args, trailing, span);
                    return Ty::Enum(id);
                }
        let ot = self.expr(object, None);
        let optional = optional || object.in_optional_chain();
        let Some((recv, wrap)) = self.receiver(ot, object.span, optional, n) else { return Ty::Error };
        let what = format!("`{n}`");
        let ret = match &recv {
            Ty::Error => Ty::Error,
            Ty::View => {
                self.set_callee(Callee::Modifier(n.to_string()));
                self.modifier_call(name, args, trailing, span)
            }
            Ty::Struct(_) | Ty::Enum(_) => {
                let (methods, owner, fields) = match &recv {
                    Ty::Struct(id) => {
                        let d = &self.structs[*id];
                        (d.methods.clone(), d.name.clone(), d.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>())
                    }
                    Ty::Enum(id) => {
                        let d = &self.enums[*id];
                        (d.methods.clone(), d.name.clone(), Vec::new())
                    }
                    _ => unreachable!(),
                };
                match methods.iter().find(|m| m.name == n) {
                    Some(m) => {
                        self.set_callee(Callee::Method { owner: recv.clone(), name: n.to_string() });
                        if m.mutating && !optional {
                            self.mutating_receiver(object, n);
                        }
                        self.args(&what, &m.sig.params, args, trailing, span);
                        m.sig.ret.clone()
                    }
                    None if fields.iter().any(|f| f == n) => {
                        self.error(name.span, format!("`{n}` is a field, not a method"))
                            .help("remove the parentheses");
                        Ty::Error
                    }
                    None => {
                        let mut names: Vec<String> = methods.iter().map(|m| m.name.clone()).collect();
                        names.extend(fields);
                        self.no_member(name, &format!("`{owner}` has no method `{n}`"), names);
                        Ty::Error
                    }
                }
            }
            Ty::List(elem) if n == "map" => {
                self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                self.map_call(elem, args, trailing, span)
            }
            Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Float | Ty::Int | Ty::Data => match builtins::method_of(&recv, n) {
                Some(m) => {
                    self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                    if m.mutating {
                        self.mutating_receiver(object, n);
                    }
                    self.args(&what, &m.sig.params, args, trailing, span);
                    if let (Ty::List(elem), "sorted") = (&recv, n) {
                        let by_given = trailing.is_some() || args.iter().any(|a| a.label.is_some());
                        if !by_given && !matches!(**elem, Ty::Int | Ty::Float | Ty::Str | Ty::Error) {
                            let el = self.a(elem);
                            self.error(span, format!("{el} values have no natural order, so `sorted()` needs a rule"))
                                .help("pass one, like `sorted(by: { a, b in a.title < b.title })`");
                        }
                    }
                    m.sig.ret
                }
                None if builtins::property(&recv, n).is_some() => {
                    self.error(name.span, format!("`{n}` is a property, not a method"))
                        .help("remove the parentheses");
                    Ty::Error
                }
                None => {
                    let ty = self.a(&recv);
                    let names = builtins::member_names(&recv).iter().map(|s| s.to_string()).collect();
                    self.no_member(name, &format!("{ty} has no method `{n}`"), names);
                    Ty::Error
                }
            },
            t => {
                let ty = self.a(t);
                self.error(name.span, format!("{ty} has no method `{n}`"));
                Ty::Error
            }
        };
        if wrap && ret != Ty::Void { Ty::opt(ret) } else { ret }
    }

    /// Methods like `append` change their receiver, so it must be a place.
    fn mutating_receiver(&mut self, object: &Expr, method: &str) {
        match self.place(object) {
            Ok(()) => {}
            Err(PlaceErr::NotPlace) => {
                self.error(object.span, format!("`{method}` changes its value, but this is a temporary value"))
                    .help("store it in a `var` first");
            }
            Err(PlaceErr::Const { what, decl, help }) => {
                self.error(object.span, format!("can't call `{method}` on {what}, because `{method}` changes it"))
                    .note_at(decl, "declared here")
                    .help(help);
            }
        }
    }

    fn map_call(&mut self, elem: &Ty, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let f = match (args, trailing) {
            ([], Some(t)) => t,
            ([a], None) if a.label.is_none() => &a.value,
            _ => {
                self.error(span, "`map` takes one function, like `map { x in x * 2 }`");
                return Ty::Error;
            }
        };
        let expected = Ty::func(vec![elem.clone()], Ty::Any);
        match self.expr(f, Some(&expected)) {
            Ty::Fn(_, r) if *r == Ty::Void => {
                self.error(f.span, "the `map` block must produce a value for each item");
                Ty::Error
            }
            Ty::Fn(ps, r) if ps.len() == 1 && fits(elem, &ps[0]) => Ty::list(*r),
            Ty::Error => Ty::Error,
            t => {
                let (e, found) = (self.show(elem), self.a(&t));
                self.error(f.span, format!("`map` needs a function that takes `{e}`, found {found}"));
                Ty::Error
            }
        }
    }

    fn modifier_call(&mut self, name: &Ident, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let n = name.name.as_str();
        if n == "tag" || n == "id" {
            let value = match (args, trailing) {
                ([a], None) if a.label.is_none() => &a.value,
                _ => {
                    self.error(span, format!("`.{n}` takes one value, like `.{n}(…)`"));
                    return Ty::View;
                }
            };
            if n == "tag" {
                match self.tag_ty.clone() {
                    Some(t) => {
                        self.expect(value, &t);
                    }
                    None => {
                        self.expr(value, None);
                        self.error(name.span, "`.tag(…)` only works on the options inside a `Picker` or the pages of a `TabView`");
                    }
                }
            } else {
                let t = self.expr(value, None);
                if !matches!(t, Ty::Int | Ty::Str | Ty::Error) {
                    let what = self.a(&t);
                    self.error(value.span, format!("`.id` needs an `Int` or `String`, found {what}"))
                        .help("use a field that is unique for each item, like `.id(item.id)`");
                }
            }
            return Ty::View;
        }
        match self.builtins.modifier(n) {
            Some(params) => {
                self.args(&format!("`.{n}`"), &params, args, trailing, span);
                if n == "shortcut" {
                    self.shortcut_key(args);
                }
                Ty::View
            }
            None => {
                let names = builtins::member_names(&Ty::View).iter().map(|s| s.to_string()).collect();
                self.no_member(name, &format!("views have no modifier `.{n}`"), names);
                Ty::Error
            }
        }
    }

    /// `.shortcut("s")`: the key must be one the runtime can match (it
    /// adds Cmd, or Ctrl on Windows, itself).
    fn shortcut_key(&mut self, args: &[Arg]) {
        let Some(arg) = args.first() else { return };
        let ExprKind::Str(parts) = &arg.value.kind else { return };
        let [StrPart::Text(key)] = parts.as_slice() else { return };
        const NAMED: &[&str] =
            &["enter", "escape", "tab", "backspace", "delete", "left", "right", "up", "down", "home", "end"];
        let mut chars = key.chars();
        let single = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_whitespace() && !c.is_uppercase());
        if single || NAMED.contains(&key.as_str()) {
            return;
        }
        let help = if let Some(rest) = key.strip_prefix("cmd+").or_else(|| key.strip_prefix("ctrl+")) {
            format!("Cmd (Ctrl on Windows) is added for you: write `.shortcut(\"{rest}\")`")
        } else if key.chars().count() == 1 {
            format!("use the lowercase letter: `.shortcut(\"{}\")`", key.to_lowercase())
        } else {
            "use one character, like \"s\", or one of: enter, escape, tab, backspace, delete, left, right, up, down, home, end".to_string()
        };
        self.error(arg.value.span, format!("`{key}` isn't a key a shortcut can use")).help(help);
    }

    /// Matches arguments to parameters by label and checks each value.
    fn args(&mut self, what: &str, params: &[ParamSig], args: &[Arg], trailing: Option<&Expr>, span: Span) {
        let n = params.len();
        let mut assigned: Vec<Option<&Expr>> = vec![None; n];
        let mut positional_end = n;
        if let Some(tr) = trailing {
            match params.last() {
                Some(p) if matches!(p.ty, Ty::Fn(..) | Ty::View) => {
                    assigned[n - 1] = Some(tr);
                    positional_end = n - 1;
                }
                _ => {
                    let open = Span { hi: tr.span.lo + 1, ..tr.span };
                    self.error(open, format!("{what} doesn't take a block after the call"))
                        .label("unexpected block");
                    self.expr(tr, None);
                }
            }
        }
        let mut pi = 0;
        for arg in args {
            match &arg.label {
                Some(l) => {
                    if let Some(j) = (pi..positional_end).find(|&j| params[j].label == l.name && !params[j].unlabeled) {
                        assigned[j] = Some(&arg.value);
                        pi = j + 1;
                        continue;
                    }
                    match params.iter().position(|p| p.label == l.name) {
                        Some(j) if params[j].unlabeled => {
                            self.error(l.span, format!("{what} takes this argument without a label"))
                                .help(format!("remove `{}:`", l.name));
                            if assigned[j].is_none() {
                                assigned[j] = Some(&arg.value);
                                pi = pi.max(j + 1);
                            } else {
                                self.expr(&arg.value, None);
                            }
                        }
                        Some(j) if j >= positional_end => {
                            self.error(l.span, format!("`{}:` is already given by the block after the call", l.name));
                            self.expr(&arg.value, None);
                        }
                        Some(j) if assigned[j].is_some() => {
                            self.error(l.span, format!("`{}:` is given twice", l.name));
                            self.expr(&arg.value, None);
                        }
                        Some(j) => {
                            let order: Vec<String> =
                                params.iter().filter(|p| !p.unlabeled).map(|p| format!("`{}:`", p.label)).collect();
                            self.error(l.span, format!("`{}:` is in the wrong position", l.name))
                                .help(format!("arguments go in this order: {}", order.join(", ")));
                            assigned[j] = Some(&arg.value);
                        }
                        None => {
                            let labels: Vec<&str> = params.iter().filter(|p| !p.unlabeled).map(|p| p.label.as_str()).collect();
                            let guess = suggest(&l.name, labels.iter().copied());
                            // Treat a likely typo as the label it resembles, so the
                            // value is still checked and no "missing" error follows.
                            let guess_slot = guess.and_then(|g| params.iter().position(|p| p.label == g));
                            if let Some(j) = guess_slot.filter(|&j| j < positional_end && assigned[j].is_none()) {
                                assigned[j] = Some(&arg.value);
                                pi = pi.max(j + 1);
                            }
                            let help = match guess {
                                Some(s) => format!("did you mean `{s}:`?"),
                                None if labels.is_empty() => format!("{what} takes no labelled arguments"),
                                None => format!(
                                    "{what} takes {}",
                                    labels.iter().map(|l| format!("`{l}:`")).collect::<Vec<_>>().join(", ")
                                ),
                            };
                            self.error(l.span, format!("{what} has no parameter `{}:`", l.name))
                                .label("unknown label")
                                .help(help);
                            if guess_slot.is_none() {
                                self.expr(&arg.value, None);
                            }
                        }
                    }
                }
                None => {
                    if pi < positional_end {
                        let p = &params[pi];
                        if !(p.unlabeled || (n == 1 && pi == 0)) {
                            self.error(arg.value.span, format!("this argument needs its label `{}:`", p.label))
                                .help(format!("write `{}: …`", p.label));
                        }
                        assigned[pi] = Some(&arg.value);
                        pi += 1;
                    } else {
                        let count = match positional_end {
                            0 => "no arguments".to_string(),
                            1 => "1 argument".to_string(),
                            k => format!("{k} arguments"),
                        };
                        self.error(arg.value.span, format!("too many arguments: {what} takes {count}"));
                        self.expr(&arg.value, None);
                    }
                }
            }
        }
        let missing: Vec<String> = params
            .iter()
            .zip(&assigned)
            .filter(|(p, a)| a.is_none() && !p.has_default)
            .map(|(p, _)| if p.unlabeled { format!("the {} value", p.label) } else { format!("`{}:`", p.label) })
            .collect();
        if !missing.is_empty() {
            let list = join_and(&missing);
            self.error(span, format!("{what} is missing {list}"))
                .help(if missing.len() == 1 { "add it to the call" } else { "add them to the call" });
        }
        let refs = assigned
            .iter()
            .map(|a| {
                a.map(|v| match args.iter().position(|arg| std::ptr::eq(&arg.value, v)) {
                    Some(i) => ArgRef::Arg(i),
                    None => ArgRef::Trailing,
                })
            })
            .collect();
        if let Some(frame) = self.frames.last_mut() {
            frame.args = refs;
        }
        for (p, a) in params.iter().zip(assigned) {
            if let Some(v) = a {
                self.arg_value(what, p, v);
            }
        }
    }

    fn arg_value(&mut self, what: &str, p: &ParamSig, v: &Expr) {
        if p.ty == Ty::View {
            self.content_closure(v);
            return;
        }
        if p.tag_fn {
            let t = self.tag_ty.clone().unwrap_or(Ty::Error);
            self.expect(v, &Ty::func(vec![t], Ty::Void));
            return;
        }
        let t = if p.ty == Ty::Any {
            self.expr(v, None)
        } else if p.bind {
            let t = self.expr(v, Some(&p.ty));
            if t != p.ty && !t.has_error() && !p.ty.has_error() {
                self.mismatch(v.span, &p.ty, &t);
            }
            t
        } else {
            self.expect(v, &p.ty)
        };
        if p.bind && !t.has_error() {
            match self.place(v) {
                Ok(()) => {}
                Err(PlaceErr::NotPlace) => {
                    self.error(v.span, format!("{what}'s `{}:` needs something it can change", p.label))
                        .label("this is a value, not a variable")
                        .help("pass a `state` or `var`, or a field of one");
                }
                Err(PlaceErr::Const { what: name, decl, help }) => {
                    self.error(v.span, format!("{what}'s `{}:` changes its value, so it can't be given {name}", p.label))
                        .note_at(decl, "declared here")
                        .help(help);
                }
            }
        }
        if p.sets_tag {
            self.tag_ty = Some(t);
        }
    }
}

// ===== helpers =====

fn fn_type(sig: &FnSig) -> Ty {
    Ty::Fn(sig.params.iter().map(|p| p.ty.clone()).collect(), Box::new(sig.ret.clone()))
}

fn strip_opt(t: &Ty) -> &Ty {
    match t {
        Ty::Opt(inner) => inner,
        t => t,
    }
}

/// Can a value of type `found` be used where `expected` is wanted?
fn fits(found: &Ty, expected: &Ty) -> bool {
    if found.has_error() || expected.has_error() || *expected == Ty::Any || found == expected {
        return true;
    }
    match (found, expected) {
        (_, Ty::Opt(inner)) => fits(found, inner),
        (Ty::Fn(fp, fr), Ty::Fn(ep, er)) => {
            fp.len() == ep.len() && fp.iter().zip(ep).all(|(a, b)| a == b || a.has_error()) && fits(fr, er)
        }
        _ => false,
    }
}

fn equatable(t: &Ty) -> bool {
    match t {
        Ty::Fn(..) | Ty::View | Ty::Any => false,
        Ty::List(t) | Ty::Opt(t) => equatable(t),
        Ty::Dict(k, v) => equatable(k) && equatable(v),
        _ => true,
    }
}

fn is_nil(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Nil)
}

/// Expressions whose type comes from context.
fn needs_context(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Nil | ExprKind::DotName(_) | ExprKind::Int(_) => true,
        ExprKind::List(items) => items.is_empty(),
        ExprKind::Dict(pairs) => pairs.is_empty(),
        _ => false,
    }
}

fn case_field_count(c: &Checker, id: usize, name: &str) -> usize {
    c.enums[id].case(name).map_or(0, |case| case.fields.len())
}

fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// A call inside a method body that may change the method's value.
enum MethodCall {
    /// `name(…)` or `self.name(…)`: one of the type's own methods.
    Own(String),
    /// `field.name(…)`: a method of one of the struct's fields.
    Field(String, String),
    /// `field.a.name(…)`: a method of something inside a field.
    Deep(String),
}

/// The calls in a method body that could change its value, for
/// propagate_mutation. `roots` are the struct's field names.
fn method_calls(b: &Block, roots: &HashSet<String>) -> Vec<MethodCall> {
    let mut out = Vec::new();
    crate::ast::block_exprs(b, &mut |e| {
        let ExprKind::Call { callee, .. } = &e.kind else { return };
        match &callee.kind {
            ExprKind::Name(name) => out.push(MethodCall::Own(name.clone())),
            ExprKind::Member { object, name, .. } => match &object.kind {
                ExprKind::Name(n) if n == "self" => out.push(MethodCall::Own(name.name.clone())),
                ExprKind::Name(n) if roots.contains(n) => out.push(MethodCall::Field(n.clone(), name.name.clone())),
                _ if root_name(object).is_some_and(|n| roots.contains(n) || n == "self") => {
                    out.push(MethodCall::Deep(name.name.clone()))
                }
                _ => {}
            },
            _ => {}
        }
    });
    out
}

/// Does a method body change any of `roots` (its struct's fields or `self`)?
fn block_mutates(b: &Block, roots: &HashSet<String>) -> bool {
    b.stmts.iter().any(|s| stmt_mutates(s, roots))
}

fn stmt_mutates(s: &Stmt, roots: &HashSet<String>) -> bool {
    match s {
        Stmt::Assign { target, value, .. } => {
            root_name(target).is_some_and(|n| roots.contains(n)) || expr_mutates(value, roots)
        }
        Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => expr_mutates(&d.value, roots),
        Stmt::Return { value, .. } => value.as_ref().is_some_and(|v| expr_mutates(v, roots)),
        Stmt::For { iter, body, .. } => expr_mutates(iter, roots) || block_mutates(body, roots),
        Stmt::Expr(e) => expr_mutates(e, roots),
        Stmt::Fn(_) => false,
    }
}

fn expr_mutates(e: &Expr, roots: &HashSet<String>) -> bool {
    let m = |x: &Expr| expr_mutates(x, roots);
    match &e.kind {
        ExprKind::Call { callee, args, trailing } => {
            let mutating_call = match &callee.kind {
                ExprKind::Member { object, name, .. } => {
                    MUTATING.contains(&name.name.as_str()) && root_name(object).is_some_and(|n| roots.contains(n))
                }
                _ => false,
            };
            mutating_call || m(callee) || args.iter().any(|a| m(&a.value)) || trailing.as_deref().is_some_and(m)
        }
        ExprKind::Str(parts) => parts.iter().any(|p| matches!(p, StrPart::Expr(x) if m(x))),
        ExprKind::List(items) => items.iter().any(m),
        ExprKind::Dict(pairs) => pairs.iter().any(|(k, v)| m(k) || m(v)),
        ExprKind::Unary(_, x) => m(x),
        ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => m(l) || m(r),
        ExprKind::Member { object, .. } => m(object),
        ExprKind::Closure { body, .. } | ExprKind::Block(body) => block_mutates(body, roots),
        ExprKind::If { cond, then, otherwise } => {
            let c = match cond {
                Cond::Expr(x) | Cond::Let(_, x) => m(x),
            };
            let o = match otherwise {
                Some(Else::Block(b)) => block_mutates(b, roots),
                Some(Else::If(x)) => m(x),
                None => false,
            };
            c || block_mutates(then, roots) || o
        }
        ExprKind::Match { subject, arms } => m(subject) || arms.iter().any(|a| m(&a.body)),
        _ => false,
    }
}

/// The variable at the root of `a.b[c].d`.
fn root_name(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Name(n) => Some(n),
        ExprKind::Member { object, .. } | ExprKind::Index(object, _) => root_name(object),
        _ => None,
    }
}
