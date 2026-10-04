//! Type checker: resolves names, infers and checks types, and enforces the
//! rules for views, state and bindings. Records the type of every expression
//! for code generation.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
    /// Functions declared inside functions, by the span of their name: the
    /// signature, and the parameters (for their default values).
    pub local_fns: HashMap<Span, (FnSig, Vec<Param>)>,
    pub ifaces: Vec<IfaceDef>,
    /// The type each `is` / `as?` checks for.
    pub cast_targets: HashMap<ExprId, Ty>,
    /// Where each `try` sends a failure.
    pub tries: HashMap<ExprId, TryTarget>,
    /// Which struct or enum conforms to which interface.
    pub conforms: Vec<(Ty, usize)>,
    /// What fits where the completion mark is (see `complete`), if the
    /// checker got there.
    pub completion: Option<Vec<Suggestion>>,
    /// Specialized copies of generic declarations, to compile along with the
    /// program's own items.
    pub instances: Vec<Rc<Item>>,
    /// For a REPL entry: what its statements declare and produce.
    pub repl: Option<ReplChecked>,
}

/// What the statements of a REPL entry leave behind.
#[derive(Clone, Debug, Default)]
pub struct ReplChecked {
    /// The variables they declare (name, type, declared with `var`), which
    /// later entries can use.
    pub vars: Vec<(String, Ty, bool)>,
    /// The type of the last statement, if it's an expression (`()` otherwise).
    pub result: Ty,
}

impl Default for Ty {
    fn default() -> Ty {
        Ty::Void
    }
}

/// The identifier `tessel complete` puts where the caret is, so the checker
/// can say what fits there.
pub const COMPLETION_MARK: &str = "__tessel_complete__";

/// Where a failure goes from `try`: returned (as the enclosing function's
/// `Result` type, or as its `Error?`), or, in `fn main`, reported with an
/// exit status of 1.
#[derive(Clone, Debug)]
pub enum TryTarget {
    Result(Ty),
    ErrorOpt(Ty),
    Exit,
}

/// A completion suggestion.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    /// A type or signature, shown next to the label.
    pub detail: String,
    pub kind: SuggestionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionKind {
    Variable,
    Field,
    Method,
    Function,
    View,
    Modifier,
    Type,
    Case,
    Keyword,
}

impl SuggestionKind {
    pub fn name(self) -> &'static str {
        match self {
            SuggestionKind::Variable => "variable",
            SuggestionKind::Field => "field",
            SuggestionKind::Method => "method",
            SuggestionKind::Function => "function",
            SuggestionKind::View => "view",
            SuggestionKind::Modifier => "modifier",
            SuggestionKind::Type => "type",
            SuggestionKind::Case => "case",
            SuggestionKind::Keyword => "keyword",
        }
    }
}

const KEYWORDS: &[&str] = &[
    "let", "var", "state", "bind", "if", "else", "for", "in", "match", "return", "try", "while", "break", "continue", "fn", "struct", "enum", "interface", "view",
    "app", "true", "false", "nil",
];

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
            Ty::Set(t) => format!("Set<{}>", self.show(t)),
            Ty::Opt(t) if matches!(**t, Ty::Fn(..)) => format!("({})?", self.show(t)),
            Ty::Opt(t) => format!("{}?", self.show(t)),
            Ty::Fn(ps, r) => {
                let ps: Vec<String> = ps.iter().map(|p| self.show(p)).collect();
                let ret = if **r == Ty::Void { String::new() } else { format!(" -> {}", self.show(r)) };
                format!("fn({}){ret}", ps.join(", "))
            }
            Ty::Struct(id) => self.structs[*id].name.clone(),
            Ty::Enum(id) => self.enums[*id].name.clone(),
            Ty::Iface(id) => self.ifaces[*id].name.clone(),
            Ty::Generic(_) => "T".into(),
        }
    }
}

/// What a block of views is the content of, which decides what may go in it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Content {
    /// A custom view's body (or content it's given): it may be shown anywhere.
    View,
    /// The `app`'s body: it may declare `Window`s.
    App,
    /// A container that lays out views (`VStack`, a popover, …).
    Layout,
    /// A menu's items (`.contextMenu`, `Menu`).
    Menu,
    /// `.menuBar`'s menus.
    MenuBar,
}

/// What a call expression resolved to.
#[derive(Clone, Debug)]
pub enum Callee {
    /// A top-level user function.
    Fn(String),
    /// A struct or enum method; the receiver is the callee's object.
    Method { owner: Ty, name: String },
    /// A method called through an interface value: the interface and the
    /// method's index in it.
    IfaceMethod { iface: usize, index: usize },
    /// A method called by bare name inside another method of the same type.
    SelfMethod { owner: Ty, name: String },
    /// A function declared inside a view.
    ViewFn(String),
    /// A function declared inside a function (`decl` is the span of its name).
    LocalFn { name: String, decl: Span },
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
    check(program, diags, require_entry, None)
}

/// Checks the program of a REPL entry, whose statements are the body of the
/// function `REPL_FN` and whose parameters are the session's variables
/// (those in `mutable` were declared with `var`).
pub fn check_repl(program: &Program, diags: &mut Diagnostics, mutable: HashSet<String>) -> Checked {
    check(program, diags, false, Some(mutable))
}

fn check(program: &Program, diags: &mut Diagnostics, require_entry: bool, repl: Option<HashSet<String>>) -> Checked {
    let enums = builtins::enums();
    let structs = builtins::structs();
    let mut globals = HashMap::new();
    for (i, e) in enums.iter().enumerate() {
        globals.insert(e.name.clone(), Global::Enum(i));
    }
    for (i, s) in structs.iter().enumerate() {
        globals.insert(s.name.clone(), Global::Struct(i));
    }
    let builtins = Builtins::standard();
    let mut c = Checker {
        diags,
        builtins,
        structs,
        enums,
        fns: Vec::new(),
        views: Vec::new(),
        globals,
        user_spans: HashMap::new(),
        scopes: Vec::new(),
        returns: Vec::new(),
        tag_ty: None,
        content: Content::View,
        next_content: None,
        types: HashMap::new(),
        calls: HashMap::new(),
        frames: Vec::new(),
        locals: HashMap::new(),
        loop_places: HashSet::new(),
        view_fns: HashMap::new(),
        local_fns: HashMap::new(),
        completion: None,
        ifaces: Vec::new(),
        cast_targets: HashMap::new(),
        conforms: Vec::new(),
        loops: 0,
        outer_loops: 0,
        view_lets: HashSet::new(),
        in_main: false,
        tries: HashMap::new(),
        templates: Vec::new(),
        type_params: Vec::new(),
        specialized: HashMap::new(),
        instance_of: HashMap::new(),
        pending: std::collections::VecDeque::new(),
        instances: Vec::new(),
        fn_items: Vec::new(),
        template_copy: false,
        next_id: 0,
        private: program.private.clone(),
        public: program.public.clone(),
        repl_mutable: repl,
        repl: None,
    };
    c.next_id = program.next_id;
    let ids = c.collect(&program.items, require_entry);
    c.check_templates();
    c.signatures(&program.items, &ids);
    c.check_main(&program.items, require_entry);
    c.bodies(&program.items, &ids);
    c.check_pending();
    c.check_recursive_structs();
    c.check_conformance();
    let fns = c.fn_items.iter().map(|(name, id)| (name.clone(), c.fns[*id].clone())).collect();
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
        local_fns: c.local_fns,
        conforms: c.conforms.iter().map(|(t, i, _)| (t.clone(), *i)).collect(),
        ifaces: c.ifaces,
        cast_targets: c.cast_targets,
        tries: c.tries,
        completion: c.completion,
        instances: c.instances,
        repl: c.repl,
    }
}

/// A generic declaration: its parameters, and whether checking it on its own
/// found errors (then its copies aren't checked again, to not repeat them).
struct Template {
    item: Rc<Item>,
    params: Vec<usize>,
    had_errors: bool,
}

/// A type parameter (`Ty::Generic(index)`).
struct TypeParam {
    name: String,
    /// The interface it must conform to.
    constraint: Option<usize>,
}

/// A specialized copy whose bodies still have to be checked.
struct Pending {
    item: Rc<Item>,
    id: Option<usize>,
    template: usize,
    /// All its type arguments are real types (it's compiled).
    concrete: bool,
    name: String,
    /// Where it was first needed.
    used_at: Span,
}

#[derive(Clone, Copy, Debug)]
enum Global {
    Struct(usize),
    Enum(usize),
    Iface(usize),
    /// A generic declaration (index into the templates).
    GenericFn(usize),
    GenericStruct(usize),
    GenericEnum(usize),
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
    /// Declared inside a function; the span of its name.
    Local(Span),
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
    /// Checking a REPL entry: the session's variables declared with `var`.
    repl_mutable: Option<HashSet<String>>,
    repl: Option<ReplChecked>,
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
    /// What the views being checked are the content of.
    content: Content,
    /// What the next content block checked is the content of (set by the
    /// call it's given to).
    next_content: Option<Content>,
    types: HashMap<ExprId, Ty>,
    calls: HashMap<ExprId, CallInfo>,
    /// One frame per call being checked, innermost last.
    frames: Vec<CallFrame>,
    locals: HashMap<Span, Ty>,
    loop_places: HashSet<Span>,
    view_fns: HashMap<(String, String), FnSig>,
    local_fns: HashMap<Span, (FnSig, Vec<Param>)>,
    completion: Option<Vec<Suggestion>>,
    ifaces: Vec<IfaceDef>,
    cast_targets: HashMap<ExprId, Ty>,
    /// (type, interface, where the type says it conforms)
    conforms: Vec<(Ty, usize, Span)>,
    /// How many loops enclose the code being checked, within the current
    /// function or closure (`break` and `continue` need one).
    loops: usize,
    /// Loops outside the closures being checked (which `break` can't leave).
    outer_loops: usize,
    /// `let`s declared in view bodies (changing one needs `state`, not `var`).
    view_lets: HashSet<Span>,
    /// Checking `fn main`'s own body (where `try` exits on a failure).
    in_main: bool,
    tries: HashMap<ExprId, TryTarget>,
    templates: Vec<Template>,
    type_params: Vec<TypeParam>,
    /// Specialized copies made so far, by template and type arguments.
    specialized: HashMap<String, String>,
    /// Each copy's template and type arguments, by the copy's name.
    instance_of: HashMap<String, (usize, Vec<Ty>)>,
    pending: std::collections::VecDeque<Pending>,
    instances: Vec<Rc<Item>>,
    /// Functions to compile: the program's, and specialized copies.
    fn_items: Vec<(String, usize)>,
    /// Set while a generic declaration is registered for checking on its own.
    template_copy: bool,
    next_id: ExprId,
    /// Name spans of members marked `private` (usable only in their own file).
    private: HashSet<Span>,
    /// Name spans of declarations marked `public`.
    public: HashSet<Span>,
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
            Ty::Set(t) => format!("Set<{}>", self.show(t)),
            Ty::Opt(t) if matches!(**t, Ty::Fn(..)) => format!("({})?", self.show(t)),
            Ty::Opt(t) => format!("{}?", self.show(t)),
            Ty::Fn(ps, r) => {
                let ps: Vec<String> = ps.iter().map(|p| self.show(p)).collect();
                let ret = if **r == Ty::Void { String::new() } else { format!(" -> {}", self.show(r)) };
                format!("fn({}){ret}", ps.join(", "))
            }
            Ty::Struct(id) => self.structs[*id].name.clone(),
            Ty::Enum(id) => self.enums[*id].name.clone(),
            Ty::Iface(id) => self.ifaces[*id].name.clone(),
            Ty::Generic(k) => self.type_params[*k].name.clone(),
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
            (e, Ty::Opt(inner)) if self.fits(inner, e) => Some(
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

    /// A `private` member (declared at `decl`) used at `at`, in another file.
    fn check_private_member(&mut self, what: &str, owner: &str, decl: Span, at: Span) {
        let mask = crate::source::FILE_MASK;
        if self.private.contains(&decl) && decl.file & mask != at.file & mask {
            self.error(at, format!("{what} is private to the file that declares `{owner}`"))
                .note_at(decl, "declared here")
                .help("remove `private` from it to use it in other files");
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
        names.extend(self.globals.keys().filter(|n| !n.starts_with('(')).cloned());
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

    fn collect(&mut self, items: &[Item], require_entry: bool) -> Vec<Option<usize>> {
        let mut ids = Vec::new();
        let mut app: Option<Span> = None;
        for item in items {
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
            let generics = match item {
                Item::Fn(f) => Some(&f.generics),
                Item::Struct(d) => Some(&d.generics),
                Item::Enum(d) => Some(&d.generics),
                _ => None,
            };
            if let Some(g) = generics.filter(|g| !g.is_empty()) {
                let t = self.templates.len();
                self.templates.push(Template { item: Rc::new(item.clone()), params: Vec::new(), had_errors: false });
                let _ = g;
                let global = match item {
                    Item::Fn(_) => Global::GenericFn(t),
                    Item::Struct(_) => Global::GenericStruct(t),
                    _ => Global::GenericEnum(t),
                };
                self.globals.insert(name.name.clone(), global);
                self.user_spans.insert(name.name.clone(), name.span);
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
                Item::Interface(d) => {
                    self.ifaces.push(IfaceDef {
                        name: d.name.name.clone(),
                        span: d.name.span,
                        methods: vec![],
                        mutating: vec![],
                        props: vec![],
                    });
                    (Global::Iface(self.ifaces.len() - 1), Some(self.ifaces.len() - 1))
                }
                Item::Fn(f) => {
                    self.fns.push(FnSig { params: vec![], ret: Ty::Void });
                    if !self.template_copy {
                        self.fn_items.push((f.name.name.clone(), self.fns.len() - 1));
                    }
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
        let has_main = items.iter().any(|i| matches!(i, Item::Fn(f) if f.name.name == "main"));
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

    /// Records the interfaces in `: A, B` after a struct or enum name.
    fn conformances(&mut self, names: &[Ident], ty: Ty) {
        for n in names {
            match self.globals.get(&n.name).copied() {
                Some(Global::Iface(i)) => {
                    if self.conforms.iter().any(|(t, j, _)| *t == ty && *j == i) {
                        self.error(n.span, format!("`{}` is listed twice", n.name));
                    } else {
                        self.conforms.push((ty.clone(), i, n.span));
                    }
                }
                Some(_) => {
                    self.error(n.span, format!("`{}` isn't an interface", n.name))
                        .help("only interfaces can go after `:`, like `struct Circle: Shape`");
                }
                None => {
                    self.error(n.span, format!("unknown interface `{}`", n.name))
                        .help(format!("declare it: `interface {} {{ fn … }}`", n.name));
                }
            }
        }
    }

    /// Each type has every method its interfaces require, with the same
    /// parameters and result, and those methods don't change the value.
    fn check_conformance(&mut self) {
        let conforms = self.conforms.clone();
        for (ty, iface, span) in conforms {
            let (tname, methods) = match &ty {
                Ty::Struct(id) => (self.structs[*id].name.clone(), self.structs[*id].methods.clone()),
                Ty::Enum(id) => (self.enums[*id].name.clone(), self.enums[*id].methods.clone()),
                _ => continue,
            };
            let iname = self.ifaces[iface].name.clone();
            for (mname, want) in self.ifaces[iface].methods.clone() {
                let wanted = format!("fn {mname}{}", self.sig_text(&want.params, &want.ret));
                let Some(m) = methods.iter().find(|m| m.name == mname) else {
                    self.error(span, format!("`{tname}` says it's a `{iname}`, but has no method `{mname}`"))
                        .help(format!("add it to `{tname}`: `{wanted} {{ … }}`"));
                    continue;
                };
                let same_params = m.sig.params.len() == want.params.len()
                    && m.sig.params.iter().zip(&want.params).all(|(a, b)| {
                        a.label == b.label && a.unlabeled == b.unlabeled && a.ty == b.ty
                    });
                if !same_params || m.sig.ret != want.ret {
                    let have = format!("fn {mname}{}", self.sig_text(&m.sig.params, &m.sig.ret));
                    self.error(m.span, format!("`{mname}` doesn't match what `{iname}` requires"))
                        .label(format!("this is `{have}`"))
                        .help(format!("`{iname}` requires `{wanted}`"));
                }
            }
            for (pname, pty) in self.ifaces[iface].props.clone() {
                let wanted = format!("{pname}: {}", self.show(&pty));
                let field = match &ty {
                    Ty::Struct(id) => self.structs[*id].fields.iter().find(|f| f.name == pname).map(|f| (f.ty.clone(), f.span)),
                    _ => {
                        self.error(span, format!("`{tname}` is an enum, so it can't have the property `{pname}` that `{iname}` requires"))
                            .help("only structs have fields; make it a struct, or require a method in the interface");
                        continue;
                    }
                };
                match field {
                    None => {
                        self.error(span, format!("`{tname}` says it's a `{iname}`, but has no field `{pname}`"))
                            .help(format!("add it to `{tname}`: `{wanted}`"));
                    }
                    Some((ft, fspan)) if ft != pty => {
                        let have = self.show(&ft);
                        self.error(fspan, format!("`{pname}` doesn't match what `{iname}` requires"))
                            .label(format!("this is `{have}`"))
                            .help(format!("`{iname}` requires `{wanted}`"));
                    }
                    Some(_) => {}
                }
            }
        }
    }

    /// Does `found` fit where `expected` is wanted (including a type that
    /// conforms to an expected interface)?
    fn fits(&self, found: &Ty, expected: &Ty) -> bool {
        fits_with(found, expected, &self.conforms)
    }

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
                "Set" => {
                    self.error(id.span, "`Set` needs the type of its items, like `Set<Int>`");
                    Ty::Error
                }
                "$error" => Ty::Error,
                name if name.starts_with('$') => Ty::Generic(name[1..].parse().unwrap_or(0)),
                name => match self.globals.get(name) {
                    Some(Global::GenericStruct(t) | Global::GenericEnum(t)) => {
                        let t = *t;
                        let params: Vec<String> =
                            self.templates[t].params.iter().map(|&k| self.type_params[k].name.clone()).collect();
                        self.error(id.span, format!("`{name}` needs its type arguments, like `{name}<{}>`", params.join(", ")))
                            .help("write the types for its parameters, like `Stack<Int>`");
                        Ty::Error
                    }
                    Some(Global::GenericFn(_)) => {
                        self.error(id.span, format!("`{name}` is not a type"));
                        Ty::Error
                    }
                    Some(Global::Struct(i)) => Ty::Struct(*i),
                    Some(Global::Enum(i)) => Ty::Enum(*i),
                    Some(Global::Iface(i)) => Ty::Iface(*i),
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
                            .filter(|(_, g)| matches!(g, Global::Struct(_) | Global::Enum(_) | Global::Iface(_)))
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
            TypeExpr::Generic(id, args, span) if id.name == "Set" => self.set_type(args, *span),
            TypeExpr::Generic(id, args, span) => self.generic_name_ty(&id.name, args, *span),
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
                rows: false,
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

    fn signatures(&mut self, items: &[Item], ids: &[Option<usize>]) {
        for (item, id) in items.iter().zip(ids) {
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
                    self.conformances(&d.conforms, Ty::Struct(id));
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
                    self.conformances(&d.conforms, Ty::Enum(id));
                }
                Item::Interface(d) => {
                    let mut methods: Vec<(String, FnSig)> = Vec::new();
                    for m in &d.methods {
                        if methods.iter().any(|(n, _)| *n == m.name.name) {
                            self.error(m.name.span, format!("`{}` already has a method named `{}`", d.name.name, m.name.name));
                            continue;
                        }
                        for p in &m.params {
                            if let Some(d) = &p.default {
                                self.error(d.span, "interface methods can't have default values")
                                    .help("every type's method takes the value; give the default at the call instead");
                            }
                        }
                        let params = self.params_sig(&m.params, false);
                        let ret = m.ret.as_ref().map_or(Ty::Void, |t| self.resolve(t));
                        methods.push((m.name.name.clone(), FnSig { params, ret }));
                    }
                    let mut props: Vec<(String, Ty)> = Vec::new();
                    for (n, t) in &d.props {
                        if props.iter().any(|(p, _)| *p == n.name) || methods.iter().any(|(m, _)| *m == n.name) {
                            self.error(n.span, format!("`{}` already has a member named `{}`", d.name.name, n.name));
                            continue;
                        }
                        let ty = self.resolve(t);
                        props.push((n.name.clone(), ty));
                    }
                    self.ifaces[id].mutating = vec![false; methods.len()];
                    self.ifaces[id].methods = methods;
                    self.ifaces[id].props = props;
                }
                Item::Fn(f) => self.fns[id] = self.fn_sig(f),
                Item::View(v) => self.views[id] = self.params_sig(&v.params, true),
                Item::App(_) => {}
            }
        }
        self.propagate_mutation(items, ids);
    }

    /// A method also changes its value if it calls one of its own type's
    /// changing methods (`increment()`, `self.increment()`) or a changing
    /// method of one of its fields (`counter.increment()`), however deep.
    fn propagate_mutation(&mut self, items: &[Item], ids: &[Option<usize>]) {
        // (is a struct, type id, method index, the calls it makes)
        let mut calls: Vec<(bool, usize, usize, Vec<MethodCall>)> = Vec::new();
        for (item, id) in items.iter().zip(ids) {
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
                            Some(Ty::Iface(i)) => self.iface_method_mutates(*i, name),
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
        for i in 0..self.ifaces.len() {
            let names: Vec<String> = self.ifaces[i].methods.iter().map(|(n, _)| n.clone()).collect();
            self.ifaces[i].mutating = names.iter().map(|n| self.iface_method_mutates(i, n)).collect();
        }
    }

    /// Does method `name` change the value in any type conforming to interface `i`?
    fn iface_method_mutates(&self, i: usize, name: &str) -> bool {
        self.conforms.iter().filter(|(_, j, _)| *j == i).any(|(t, _, _)| {
            let methods = match t {
                Ty::Struct(id) => &self.structs[*id].methods,
                Ty::Enum(id) => &self.enums[*id].methods,
                _ => return false,
            };
            methods.iter().any(|m| m.name == name && m.mutating)
        })
    }

    fn check_main(&mut self, items: &[Item], require_entry: bool) {
        if !require_entry {
            return;
        }
        for item in items {
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

    fn bodies(&mut self, items: &[Item], ids: &[Option<usize>]) {
        for (item, id) in items.iter().zip(ids) {
            match item {
                Item::App(app) => {
                    let params = vec![
                        ParamSig::new("title", Ty::Str).default(),
                        ParamSig::new("width", Ty::Float).default(),
                        ParamSig::new("height", Ty::Float).default(),
                        ParamSig::new("appearance", self.builtins.appearance.clone()).default(),
                        ParamSig::new("documentTypes", Ty::list(Ty::Str)).default(),
                    ];
                    self.with_scope(|c| c.args("`app`", &params, &app.args, None, app.args_span));
                    self.document_types(app);
                    self.content = Content::App;
                    self.view_body(&[], &[], &app.body, &app.name);
                    self.content = Content::View;
                }
                _ if id.is_none() => {}
                Item::Interface(_) => {}
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
                Item::Fn(f) if self.repl_mutable.is_some() && f.name.name == crate::REPL_FN => {
                    let sig = self.fns[id.unwrap()].clone();
                    self.repl_body(f, &sig);
                }
                Item::Fn(f) => {
                    let sig = self.fns[id.unwrap()].clone();
                    self.in_main = f.name.name == "main";
                    self.fn_body(f, &sig);
                    self.in_main = false;
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
            let loops = std::mem::take(&mut c.loops);
            c.block(&f.body, Some(&sig.ret));
            c.loops = loops;
            c.returns.pop();
        });
    }

    /// The statements of a REPL entry. The session's variables are in scope
    /// (as the function's parameters); the variables declared here join
    /// them, and the value of a last expression is shown, not thrown away.
    fn repl_body(&mut self, f: &FnDecl, sig: &FnSig) {
        let mutable = self.repl_mutable.clone().unwrap_or_default();
        self.with_scope(|c| {
            for (p, ps) in f.params.iter().zip(&sig.params) {
                let kind = if mutable.contains(&p.name.name) { Kind::Var } else { Kind::Let };
                c.declare(&p.name, ps.ty.clone(), kind);
            }
            let loops = std::mem::take(&mut c.loops);
            c.with_scope(|c| {
                let n = f.body.stmts.len();
                let mut result = Ty::Void;
                for (i, s) in f.body.stmts.iter().enumerate() {
                    match s {
                        Stmt::Expr(e) if i + 1 == n => result = c.expr(e, None),
                        s => c.stmt(s),
                    }
                }
                let vars = c
                    .scopes
                    .last()
                    .unwrap()
                    .iter()
                    .filter(|l| matches!(l.kind, Kind::Let | Kind::Var) && !l.name.starts_with('$'))
                    .map(|l| (l.name.clone(), l.ty.clone(), matches!(l.kind, Kind::Var)))
                    .collect();
                c.repl = Some(ReplChecked { vars, result });
            });
            c.loops = loops;
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

    /// `app Name(documentTypes: ["txt"])`: `tessel build --bundle` reads the
    /// list from the source, so it must be written out there.
    fn document_types(&mut self, app: &AppDecl) {
        let Some(arg) = app.args.iter().find(|a| a.label.as_ref().is_some_and(|l| l.name == "documentTypes")) else { return };
        let ExprKind::List(items) = &arg.value.kind else {
            self.error(arg.value.span, "`documentTypes` must be a list written out in the app, like `[\"txt\", \"md\"]`")
                .help("`tessel build --bundle` reads it from the source, to tell the system which files the app opens");
            return;
        };
        for item in items {
            let plain = match &item.kind {
                ExprKind::Str(parts) => match parts.as_slice() {
                    [StrPart::Text(t)] => Some(t.clone()),
                    [] => Some(String::new()),
                    _ => None,
                },
                _ => None,
            };
            match plain {
                None => {
                    self.error(item.span, "each document type must be written out as a string, like `\"txt\"`")
                        .help("`tessel build --bundle` reads the list from the source");
                }
                Some(t) if t.is_empty() || t.starts_with('.') || t.contains(['/', ' ']) => {
                    self.error(item.span, format!("`{t}` isn't a file extension"))
                        .help("write extensions without the dot, like `\"txt\"`");
                }
                Some(_) => {}
            }
        }
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
        let ty = self.tuple_to_take_apart(&d.name, ty, d.value.span);
        self.declare(&d.name, ty, kind);
    }

    /// For `let (a, b) = value` (and `if let (a, b) = …`), the value is kept
    /// in a hidden name, which must hold a tuple.
    fn tuple_to_take_apart(&mut self, name: &Ident, t: Ty, span: Span) -> Ty {
        if !name.name.starts_with("$tuple") || t.has_error() {
            return t;
        }
        if let Ty::Struct(id) = &t
            && is_tuple_name(&self.structs[*id].name)
        {
            return t;
        }
        let what = self.a(&t);
        self.error(span, format!("only a tuple can be taken apart into names, but this is {what}"))
            .help("name the whole value instead: `let value = …`");
        Ty::Error
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
                                if *t == Ty::Void {
                                    c.unused_result(e);
                                }
                                t.clone()
                            }
                            None => c.expr(e, None),
                        }
                    }
                    s => c.stmt(s),
                }
            }
            if let Some(t) = expected {
                let ends_ok = matches!(b.stmts.last(), Some(Stmt::Expr(_))) || diverges(b);
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
            Stmt::Fn(f) => self.local_fn(f),
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
                    c.loops += 1;
                    c.block(body, Some(&Ty::Void));
                    c.loops -= 1;
                });
            }
            Stmt::While { cond, body, .. } => {
                self.with_scope(|c| {
                    c.cond(cond);
                    c.loops += 1;
                    c.block(body, Some(&Ty::Void));
                    c.loops -= 1;
                });
            }
            Stmt::Break(span) | Stmt::Continue(span) if self.loops == 0 => {
                let word = if matches!(s, Stmt::Break(_)) { "break" } else { "continue" };
                let in_block = self.outer_loops > 0;
                let d = self.error(*span, format!("`{word}` can only be used inside a `for` or `while` loop"));
                if in_block {
                    d.help("this block (`{ x in … }`) runs on its own, so it can't leave the loop around it; use `return` to leave the block");
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => {
                self.expect_or_discard(e, &Ty::Void);
                self.unused_result(e);
            }
        }
    }

    /// A function declared inside a function (or method, or a view's
    /// function). It can be used after its declaration, and in its own body
    /// (so it can call itself). Like a closure, it uses the variables around
    /// it: `let`s as they were, `var`s as they are when it runs.
    fn local_fn(&mut self, f: &FnDecl) {
        if let Some(g) = f.generics.first() {
            self.error(g.name.span, "a function declared inside another function can't be generic")
                .help("declare it at the top level of the file");
            return;
        }
        let sig = self.fn_sig(f);
        self.local_fns.insert(f.name.span, (sig.clone(), f.params.clone()));
        self.declare(&f.name, fn_type(&sig), Kind::Fn(sig.clone(), FnOrigin::Local(f.name.span)));
        // `break` can't leave a loop around it, and `try` returns from it.
        let (in_main, outer_loops) = (std::mem::take(&mut self.in_main), std::mem::take(&mut self.outer_loops));
        self.fn_body(f, &sig);
        self.in_main = in_main;
        self.outer_loops = outer_loops;
    }

    /// A built-in method or property whose result is thrown away does
    /// nothing: text and lists don't change themselves.
    fn unused_result(&mut self, e: &Expr) {
        let (recv, name, call) = match &e.kind {
            ExprKind::Call { .. } => match self.calls.get(&e.id).map(|c| &c.callee) {
                Some(Callee::Builtin { recv, name }) => (recv.clone(), name.clone(), true),
                _ => return,
            },
            ExprKind::Member { object, name, .. } => {
                let t = self.types.get(&object.id).cloned().unwrap_or(Ty::Error);
                (strip_opt(&t).clone(), name.name.clone(), false)
            }
            _ => return,
        };
        // (The methods of `Graphics` draw: what `image` returns may be ignored.)
        if recv.has_error() || matches!(recv, Ty::View) || recv == builtins::GRAPHICS {
            return;
        }
        let returns_value = match builtins::method_of(&recv, &name) {
            Some(m) if call => !m.mutating && m.sig.ret != Ty::Void,
            _ if !call => builtins::property(&recv, &name).is_some(),
            _ => false,
        };
        if !returns_value {
            return;
        }
        let no_args = matches!(&e.kind, ExprKind::Call { args, trailing: None, .. } if args.is_empty());
        let shown = match (call, no_args) {
            (true, true) => format!("{name}()"),
            (true, false) => format!("{name}(…)"),
            _ => name.clone(),
        };
        let d = self.error(e.span, format!("the result of `{shown}` isn't used"));
        d.label("this makes a new value, and then throws it away");
        match &e.kind {
            ExprKind::Call { callee, .. } if matches!(recv, Ty::Str | Ty::List(_) | Ty::Dict(..) | Ty::Set(_)) => {
                if let ExprKind::Member { object, .. } = &callee.kind
                    && let ExprKind::Name(v) = &object.kind
                {
                    let what = match recv {
                        Ty::Str => "text doesn't change by itself",
                        Ty::List(_) => "a list doesn't change by itself",
                        Ty::Set(_) => "a set doesn't change by itself",
                        _ => "a dictionary doesn't change by itself",
                    };
                    d.help(format!("{what}; store the result: `{v} = {v}.{shown}`"));
                } else {
                    d.help("store the result in a variable, or pass it on");
                }
            }
            _ => {
                d.help("store the result in a variable, or pass it on");
            }
        }
    }

    fn assign(&mut self, target: &Expr, op: AssignOp, value: &Expr, span: Span) {
        if let ExprKind::Name(n) = &target.kind
            && self.lookup(n).is_none()
            && !self.globals.contains_key(n)
            && n != COMPLETION_MARK
        {
            let what = if op == AssignOp::Set { "set" } else { "change" };
            self.error(target.span, format!("can't {what} `{n}`: it hasn't been created"))
                .label("not created yet")
                .help(format!("create it first with `var`, like `var {n} = …`"));
            // Created here as if with `var`, so its later uses aren't reported too.
            let t = self.expr(value, None);
            let t = if op == AssignOp::Set { t } else { Ty::Error };
            self.declare(&Ident { name: n.clone(), span: target.span }, t, Kind::Var);
            return;
        }
        let tt = self.expr(target, None);
        if !tt.has_error()
            && let Err(err) = self.place(target) {
                self.report_place(err, target.span, "change");
            }
        match op {
            // `ages["Bob"] = …`: the entry is optional (`= nil` removes it),
            // but a mismatch is about the value type.
            AssignOp::Set if matches!(&target.kind, ExprKind::Index(o, _) if matches!(self.types.get(&o.id), Some(Ty::Dict(..)))) => {
                let found = self.expr(value, Some(&tt));
                if !self.fits(&found, &tt) {
                    let inner = strip_opt(&tt).clone();
                    self.mismatch(value.span, &inner, &found);
                }
            }
            AssignOp::Set => {
                self.expect(value, &tt);
            }
            AssignOp::Op(BinOp::Add) if matches!(tt, Ty::Int | Ty::Float | Ty::Str | Ty::List(_) | Ty::Data | Ty::Error) => {
                self.expect(value, &tt);
            }
            AssignOp::Op(op) if op.is_bitwise() && matches!(tt, Ty::Int | Ty::Error) => {
                self.expect(value, &tt);
            }
            AssignOp::Op(op) if !op.is_bitwise() && tt.is_numeric() => {
                self.expect(value, &tt);
            }
            AssignOp::Op(bop) => {
                let sym = op.symbol();
                let what = self.a(&tt);
                let help = match bop {
                    BinOp::Add => "`+=` works on numbers, text, lists and `Data`".to_string(),
                    BinOp::BitAnd | BinOp::BitOr if tt == Ty::Bool => {
                        let logic = if bop == BinOp::BitAnd { "&&" } else { "||" };
                        format!("for `Bool`s, write it out: `x = x {logic} y`")
                    }
                    _ if bop.is_bitwise() => format!("`{sym}` works on `Int`s"),
                    _ => "this works on numbers".to_string(),
                };
                self.error(span, format!("`{sym}` can't be used on {what}")).help(help);
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
                    Kind::Let if self.view_lets.contains(&l.span) => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a constant"),
                        decl: l.span,
                        help: "in a view or app, values that change are declared with `state`, at its top: `state count = 0`",
                    }),
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
                    Kind::LoopLet if l.ty == Ty::Int => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a loop variable"),
                        decl: l.span,
                        help: "the loop sets it for each round; to change the number, copy it into a `var` first: `var x = i`",
                    }),
                    Kind::LoopLet => Err(PlaceErr::Const {
                        what: format!("`{n}`, which is a loop variable"),
                        decl: l.span,
                        help: "loop over a `var` or `state` list to be able to change its items (a list made by `filter` or `sorted` is a new copy)",
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
            Ty::Set(el) => (*el, false),
            Ty::Error => (Ty::Error, false),
            // Each entry as a `(key, value)` tuple: `for (name, age) in ages`.
            Ty::Dict(k, v) => (self.tuple_ty(vec![*k, *v], iter.span), false),
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
                    Stmt::Let(d) => {
                        c.view_lets.insert(d.name.span);
                        c.local_decl(d, Kind::Let);
                    }
                    Stmt::Var(d) => {
                        c.error(d.span, "`var` can't be used while building a view")
                            .help("use `let` here, or `state` at the top of the view for values that change");
                        // (Declared as a `var`, so changing it isn't reported again.)
                        c.local_decl(d, Kind::Var);
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
                    Stmt::While { span, .. } => {
                        c.error(*span, "`while` can't be used in a view body")
                            .help("show views for each item with `for item in list { … }`");
                    }
                    Stmt::Break(span) | Stmt::Continue(span) => {
                        c.error(*span, "`break` and `continue` can't be used in a view body")
                            .help("choose what to show with `if`, or filter the list first: `for x in items.filter { … }`");
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
                } else if t == Ty::View {
                    self.check_placement(e);
                }
            }
        }
    }

    /// The built-in view `e` shows (under its modifiers): its name, or
    /// `None` for a custom view or a view value, which may hold anything.
    fn built_in_view(&self, e: &Expr) -> Option<String> {
        match (&e.kind, self.calls.get(&e.id).map(|c| &c.callee)) {
            (_, Some(Callee::BuiltinView(n))) => Some(n.clone()),
            (ExprKind::Call { callee, .. }, Some(Callee::Modifier(_))) => match &callee.kind {
                ExprKind::Member { object, .. } => self.built_in_view(object),
                _ => None,
            },
            _ => None,
        }
    }

    /// Menus' items go only in menus, menus hold only those, and windows
    /// are declared in the app.
    fn check_placement(&mut self, e: &Expr) {
        let Some(name) = self.built_in_view(e) else { return };
        let item = builtins::MENU_VIEWS.contains(&name.as_str());
        match self.content {
            Content::Menu | Content::MenuBar if !item => {
                self.error(e.span, "only `MenuItem`, `MenuDivider` and `Menu` can go in a menu")
                    .help("a menu's items are `MenuItem(\"Title\") { … }`, with `MenuDivider()` between groups and `Menu(\"More\") { … }` for a submenu");
            }
            Content::MenuBar if name != "Menu" => {
                self.error(e.span, format!("the menu bar holds menus, so `{name}` can't go there"))
                    .help("put it in a menu: `Menu(\"File\") { … }`");
            }
            Content::Layout | Content::App if item => {
                self.error(e.span, format!("`{name}` can only be used in a menu"))
                    .help("put it in `.contextMenu { … }`, `.menuBar { … }` or a `Menu(\"…\") { … }`");
            }
            Content::View | Content::Layout if name == "Window" => {
                self.error(e.span, "a `Window` can only be declared in the `app`")
                    .help("declare it in the app's body, next to the main window's views");
            }
            _ => {}
        }
    }

    fn content_closure(&mut self, e: &Expr) {
        let content = self.next_content.take().unwrap_or(Content::Layout);
        match &e.kind {
            ExprKind::Closure { params, body } => {
                if let Some(p) = params.first() {
                    self.error(p.span, "a view's content block doesn't take parameters")
                        .help("remove the names before `in`");
                }
                let outer = std::mem::replace(&mut self.content, content);
                self.content_block(body, false);
                self.content = outer;
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
        if !self.fits(&t, expected) {
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
            ExprKind::GenericName(n, _) => {
                self.error(e.span, format!("`{n}<…>` is a type; to create one, call it: `{n}<…>(…)`"));
                Ty::Error
            }
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
            // `let seen: Set<Int> = [1, 2]`: a set, written like a list.
            ExprKind::List(items) if matches!(expected.map(strip_opt), Some(Ty::Set(_))) => {
                let Some(Ty::Set(t)) = expected.map(strip_opt) else { unreachable!() };
                let t = (**t).clone();
                for item in items {
                    self.expect(item, &t);
                }
                Ty::set(t)
            }
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
                    // A type that's already wrong: no more errors.
                    (None, None) if expected.is_some_and(Ty::has_error) => Ty::Error,
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
                    (None, None) if expected.is_some_and(Ty::has_error) => return Ty::Error,
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
                // The same literal key twice: the first value would be lost.
                let literal = |e: &Expr| match &e.kind {
                    ExprKind::Int(n) => Some(n.to_string()),
                    ExprKind::Bool(b) => Some(b.to_string()),
                    ExprKind::Str(parts) => match parts.as_slice() {
                        [StrPart::Text(t)] => Some(format!("{t:?}")),
                        [] => Some("\"\"".into()),
                        _ => None,
                    },
                    ExprKind::DotName(n) => Some(format!(".{n}")),
                    _ => None,
                };
                let mut seen: HashMap<String, Span> = HashMap::new();
                for (pk, _) in pairs {
                    if let Some(key) = literal(pk) {
                        if let Some(first) = seen.get(&key).copied() {
                            self.error(pk.span, format!("the key {key} appears twice"))
                                .label("repeated here")
                                .note_at(first, "first here")
                                .help("each key can have one value; remove one of them");
                        } else {
                            seen.insert(key, pk.span);
                        }
                    }
                }
                Ty::Dict(Box::new(k), Box::new(v))
            }
            ExprKind::Try(inner) => self.try_expr(e, inner, expected),
            ExprKind::Cast { value, ty, test } => {
                let vt = self.expr(value, None);
                let target = self.resolve(ty);
                self.cast_targets.insert(e.id, target.clone());
                let word = if *test { "is" } else { "as?" };
                match (&vt, &target) {
                    (Ty::Error, _) | (_, Ty::Error) => {}
                    (Ty::Iface(i), Ty::Struct(_) | Ty::Enum(_)) => {
                        if !self.conforms.iter().any(|(t, j, _)| t == &target && j == i) {
                            let (iname, tname) = (self.ifaces[*i].name.clone(), self.show(&target));
                            self.error(ty.span(), format!("a `{iname}` is never a `{tname}`"))
                                .help(format!("`{tname}` doesn't conform to `{iname}`"));
                        }
                    }
                    (Ty::Iface(_), _) => {
                        self.error(ty.span(), format!("`{word}` checks for a struct or enum type"));
                    }
                    _ => {
                        let what = self.a(&vt);
                        self.error(value.span, format!("`{word}` works on interface values, and this is {what}"))
                            .help("its type is already known");
                    }
                }
                if *test { Ty::Bool } else { Ty::opt(target) }
            }
            ExprKind::Unary(UnaryOp::Not, x) => {
                self.expect(x, &Ty::Bool);
                Ty::Bool
            }
            ExprKind::Unary(UnaryOp::BitNot, x) => {
                let t = self.expr(x, None);
                match t {
                    Ty::Int | Ty::Error => {}
                    Ty::Bool => {
                        self.error(e.span, "`~` flips the bits of an `Int`; to negate a `Bool`, use `!`")
                            .help("write `!value`");
                    }
                    t => {
                        let what = self.a(&t);
                        self.error(x.span, format!("`~` needs an `Int`, found {what}"));
                    }
                }
                Ty::Int
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
                            .help("get one character with `text.character(at: 0)`, or all of them as a list with `text.characters`");
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

    /// Can values of `t` be dictionary keys (and set items)?
    fn is_key(&self, t: &Ty) -> bool {
        match t {
            Ty::Int | Ty::Str | Ty::Bool | Ty::Error | Ty::Generic(_) => true,
            Ty::Enum(id) => self.enums[*id].cases.iter().all(|c| c.fields.is_empty()),
            Ty::Struct(id) => self.structs[*id].fields.iter().all(|f| self.is_key(&f.ty)),
            _ => false,
        }
    }

    fn check_key(&mut self, t: &Ty, span: Span) {
        if !self.is_key(t) {
            let what = self.a(t);
            self.error(span, format!("{what} can't be used as a dictionary key"))
                .help("keys can be `Int`, `String`, `Bool`, an enum whose cases have no values, or a tuple or struct of those");
        }
    }

    /// Can a value of `t` be shown inside a list, dictionary, set or tuple?
    fn shows_inside(&self, t: &Ty) -> bool {
        match t {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Range | Ty::Enum(_) | Ty::Error | Ty::Generic(_) => true,
            Ty::Opt(x) | Ty::List(x) | Ty::Set(x) => self.shows_inside(x),
            Ty::Dict(k, v) => self.shows_inside(k) && self.shows_inside(v),
            Ty::Struct(id) if is_tuple_name(&self.structs[*id].name) => {
                self.structs[*id].fields.iter().all(|f| self.shows_inside(&f.ty))
            }
            _ => false,
        }
    }

    fn not_shown_inside(&mut self, t: &Ty, span: Span) {
        let what = self.a(t);
        self.error(span, format!("{what} can't be shown as text directly, because of what it holds"))
            .help("lists, dictionaries, sets and tuples of numbers, text, `Bool`s and enums can be shown; for structs, show their fields, like `items.map { x in x.name }`");
    }

    fn showable(&mut self, t: &Ty, span: Span) {
        match t {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Range | Ty::Enum(_) | Ty::Error | Ty::Generic(_) => {}
            Ty::Opt(_) => {
                self.error(span, "this might be `nil`, so it can't be shown as text directly")
                    .help("give it a default with `??`, like \"{name ?? \"none\"}\"");
            }
            Ty::List(_) | Ty::Dict(..) | Ty::Set(_) if self.shows_inside(t) => {}
            Ty::Struct(id) if is_tuple_name(&self.structs[*id].name) && self.shows_inside(t) => {}
            // (A collection or tuple of something that can't be shown.)
            Ty::List(_) | Ty::Dict(..) | Ty::Set(_) => self.not_shown_inside(t, span),
            Ty::Struct(id) if is_tuple_name(&self.structs[*id].name) => self.not_shown_inside(t, span),
            Ty::Struct(id) => {
                let (name, field) = (self.structs[*id].name.clone(), self.structs[*id].fields.first().map(|f| f.name.clone()));
                let d = self.error(span, format!("`{name}` can't be shown as text directly"));
                if let Some(f) = field {
                    d.help(format!("show its fields, like \"{{value.{f}}}\", or add a method that describes it"));
                }
            }
            t => {
                let what = self.a(t);
                self.error(span, format!("{what} can't be shown as text"));
            }
        }
    }

    fn name_expr(&mut self, n: &str, span: Span) -> Ty {
        if n == COMPLETION_MARK {
            self.complete_names();
            return Ty::Error;
        }
        if let Some(module) = n.strip_suffix(COMPLETION_MARK).and_then(|m| m.strip_suffix('.')) {
            self.complete_module(module);
            return Ty::Error;
        }
        if let Some(l) = self.lookup(n) {
            return l.ty.clone();
        }
        match self.globals.get(n).copied() {
            Some(Global::Fn(i)) => fn_type(&self.fns[i]),
            Some(Global::GenericFn(_)) => {
                self.error(span, format!("`{n}` is generic, so it can't be used as a value; call it"))
                    .help(format!("wrap it in a block with the types you need, like `{{ x in {n}(x) }}`"));
                Ty::Error
            }
            Some(Global::GenericStruct(_) | Global::GenericEnum(_)) => {
                self.error(span, format!("`{n}` is a generic type; to create one, call it: `{n}(…)` or `{n}<Int>(…)`"));
                Ty::Error
            }
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
            Some(Global::Iface(_)) => {
                self.error(span, format!("`{n}` is an interface, a type rather than a value"))
                    .help(format!("use it in a type, like `let shapes: [{n}] = […]`"));
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
        if n == COMPLETION_MARK {
            self.complete_dot(expected);
            return Ty::Error;
        }
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
                if !(self.fits(&rt, &lt) || self.fits(&lt, &rt)) {
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
                    && (lt.is_numeric() || (op == BinOp::Add && matches!(lt, Ty::Str | Ty::List(_) | Ty::Data)));
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
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let (lt, rt) = self.operand_pair(l, r, Some(&Ty::Int));
                if lt.has_error() || rt.has_error() || (lt == Ty::Int && rt == Ty::Int) {
                    return Ty::Int;
                }
                let sym = op.symbol();
                if lt == Ty::Bool && rt == Ty::Bool && op != BinOp::Shl && op != BinOp::Shr {
                    let (logic, help) = match op {
                        BinOp::BitAnd => ("&&", "write `a && b` for \"and\""),
                        BinOp::BitOr => ("||", "write `a || b` for \"or\""),
                        _ => ("!=", "for \"one or the other, but not both\", write `a != b`"),
                    };
                    self.error(l.span.to(r.span), format!("`{sym}` works on the bits of `Int`s; for `Bool`s, use `{logic}`"))
                        .help(help);
                    return Ty::Bool;
                }
                let (a, b) = (self.a(&lt), self.a(&rt));
                self.error(l.span.to(r.span), format!("can't use `{sym}` on {a} and {b}"))
                    .help("bit operations work on two `Int`s");
                Ty::Error
            }
            BinOp::Coalesce => {
                let lt = self.expr(l, expected.map(|t| Ty::opt(t.clone())).as_ref());
                match lt {
                    Ty::Opt(inner) => {
                        let rt = self.expr(r, Some(&inner));
                        if self.fits(&rt, &inner) {
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
                let t = self.expr(e, Some(&Ty::Bool));
                if let Ty::Opt(_) = t {
                    let what = self.a(&t);
                    self.error(e.span, format!("a condition must be `true` or `false`, but this is {what}"))
                        .help("to check that it has a value, write `x != nil`; to use the value, `if let value = x { … }`");
                } else if !self.fits(&t, &Ty::Bool) {
                    self.mismatch(e.span, &Ty::Bool, &t);
                }
            }
            Cond::Let(name, ty, value) => {
                // `if let x: T = value`: value is a `T?`.
                let declared = ty.as_ref().map(|t| self.resolve(t));
                let t = match &declared {
                    Some(d) => {
                        let want = Ty::opt(d.clone());
                        self.expect(value, &want)
                    }
                    None => self.expr(value, None),
                };
                let inner = match t {
                    Ty::Opt(inner) => *inner,
                    Ty::Error => Ty::Error,
                    t => {
                        let what = self.a(&t);
                        self.error(value.span, format!("`let` in a condition needs an optional value, but this is {what}"))
                            .help("use a plain `if`, or `let` to name the value");
                        t
                    }
                };
                let inner = self.tuple_to_take_apart(name, inner, value.span);
                self.declare(name, inner, Kind::Let);
            }
        }
    }

    fn if_expr(&mut self, cond: &Cond, then: &Block, otherwise: Option<&Else>, expected: Option<&Ty>) -> Ty {
        // `if c { continue } else { value }`: the value's type comes from `else`.
        if let (None, true, Some(Else::Block(b))) = (expected, diverges(then), otherwise)
            && !diverges(b)
        {
            let t = self.block(b, None);
            self.with_scope(|c| {
                c.cond(cond);
                c.block(then, Some(&t))
            });
            return t;
        }
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
                self.error(arm.patterns[0].span(), "this arm can never run")
                    .note_at(w, "`_` above already matches everything")
                    .help("move `_` to the last arm");
            }
            // Several patterns: the arm can't know which one matched, so it
            // can't name values.
            let named = arm.patterns.iter().flat_map(|p| match p {
                Pattern::Case { bindings, .. } => bindings.as_slice(),
                _ => &[],
            });
            if arm.patterns.len() > 1
                && let Some(b) = named.into_iter().find(|b| b.name != "_")
            {
                self.error(b.span, format!("an arm with several patterns can't name values, like `{}` here", b.name))
                    .help("give each case its own arm to use its values, or write `_` in place of the name");
            }
            let single = arm.patterns.len() == 1;
            self.with_scope(|c| {
                // (Names given in an arm with several patterns were reported
                // above; they're declared once, with no type, to say no more.)
                let declare = |c: &mut Self, b: &Ident, t: Ty| {
                    if single {
                        c.declare(b, t, Kind::Let);
                    } else if b.name != "_" && c.scopes.last().unwrap().iter().all(|l| l.name != b.name) {
                        c.declare(b, Ty::Error, Kind::Let);
                    }
                };
                for pattern in &arm.patterns {
                    match pattern {
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
                                        declare(c, b, f.ty.clone());
                                    }
                                }
                                for b in bindings.iter().skip(case_field_count(c, *id, &name.name)) {
                                    declare(c, b, Ty::Error);
                                }
                            }
                            Ty::Error => {
                                for b in bindings {
                                    declare(c, b, Ty::Error);
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
        let loops = std::mem::take(&mut self.loops);
        let in_main = std::mem::take(&mut self.in_main);
        self.outer_loops += loops;
        let t = self.with_scope(|c| {
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
        });
        self.outer_loops -= loops;
        self.loops = loops;
        self.in_main = in_main;
        t
    }

    fn member(&mut self, object: &Expr, name: &Ident, optional: bool) -> Ty {
        if name.name == COMPLETION_MARK {
            self.complete_member(object);
            return Ty::Error;
        }
        // `Result<Int>.none`
        if let ExprKind::GenericName(tn, targs) = &object.kind {
            return match self.generic_name_ty(tn, targs, object.span) {
                Ty::Enum(id) => {
                    self.enum_case(id, &name.name, name.span, false);
                    Ty::Enum(id)
                }
                Ty::Error => Ty::Error,
                _ => {
                    self.error(name.span, format!("`{tn}` is a struct, so it has no cases"));
                    Ty::Error
                }
            };
        }
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none()
            && let Some(Global::GenericEnum(t)) = self.globals.get(tn).copied()
        {
            let params: Vec<String> = self.templates[t].params.iter().map(|&k| self.type_params[k].name.clone()).collect();
            self.error(object.span, format!("can't tell `{tn}`'s type arguments here"))
                .help(format!("write them, like `{tn}<{}>.{}`, or use `.{}` where the type is known", params.join(", "), name.name, name.name));
            return Ty::Error;
        }
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
        if recv == Ty::Void {
            self.leading_dot(name);
            return Ty::Error;
        }
        let t = match &recv {
            Ty::Error => Ty::Error,
            Ty::Struct(id) => {
                let def = &self.structs[*id];
                if let Some(f) = def.fields.iter().find(|f| f.name == n) {
                    let (t, decl, owner) = (f.ty.clone(), f.span, def.name.clone());
                    self.check_private_member(&format!("field `{n}`"), &owner, decl, name.span);
                    t
                } else if def.methods.iter().any(|m| m.name == n) || builtins::method_of(&recv, n).is_some() {
                    self.error(name.span, format!("`{n}` is a method; call it with `{n}()`"));
                    Ty::Error
                } else if is_tuple_name(&def.name) {
                    let (sname, count) = (def.name.clone(), def.fields.len());
                    self.error(name.span, format!("`{sname}` has {count} items, so it has no `.{n}`"))
                        .label("no such item")
                        .help(format!("its items are `.0` to `.{}`, counting from 0", count - 1));
                    Ty::Error
                } else {
                    let sname = def.name.clone();
                    let names: Vec<String> = def.fields.iter().map(|f| f.name.clone()).collect();
                    self.no_member(name, &format!("`{sname}` has no field `{n}`"), names);
                    Ty::Error
                }
            }
            Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Set(_) | Ty::Float | Ty::Int | Ty::Data => match builtins::property(&recv, n) {
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
            Ty::Generic(k) => {
                let prop = self.type_params[*k]
                    .constraint
                    .and_then(|i| self.ifaces[i].props.iter().find(|(p, _)| p == n).map(|(_, t)| t.clone()));
                match prop {
                    Some(t) => t,
                    None => {
                        let pname = self.type_params[*k].name.clone();
                        self.error(name.span, format!("`{pname}` has no property `{n}` that every type has"))
                            .help(format!("require an interface with it, like `<{pname}: Named>` for `name: String`"));
                        Ty::Error
                    }
                }
            }
            Ty::Iface(id) => match self.ifaces[*id].props.iter().find(|(p, _)| p == n) {
                Some((_, t)) => t.clone(),
                None if self.ifaces[*id].methods.iter().any(|(m, _)| m == n) => {
                    self.error(name.span, format!("`{n}` is a method; call it with `{n}(…)`"));
                    Ty::Error
                }
                None => {
                    let def = &self.ifaces[*id];
                    let iname = def.name.clone();
                    let names = def.props.iter().map(|(p, _)| p.clone()).collect();
                    self.no_member(name, &format!("`{iname}` has no property `{n}`"), names);
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

    /// `.name` on something with no value: almost always a line starting
    /// with `.` that was meant as an enum case, but continues the line before.
    fn leading_dot(&mut self, name: &Ident) {
        let n = &name.name;
        self.error(name.span, format!("`.{n}` continues the line before it, which has no value"))
            .help(format!("a line starting with `.` continues the previous line (like a modifier); for an enum case, write the enum's name first, like `Light.{n}` for an enum called `Light`"));
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
            ExprKind::Member { object, name, .. } if name.name == COMPLETION_MARK => {
                self.complete_member(object);
                Ty::Error
            }
            ExprKind::Member { object, name, optional } => {
                self.method_call(object, name, *optional, args, trailing, span, expected)
            }
            ExprKind::GenericName(n, targs) => match self.generic_name_ty(n, targs, callee.span) {
                Ty::Set(t) if n == "Set" => self.set_init(Some(*t), args, trailing, span, expected),
                Ty::Struct(id) => {
                    let name = self.structs[id].name.clone();
                    self.call_name(&name, callee.span, args, trailing, span, expected)
                }
                Ty::Enum(_) => {
                    self.error(callee.span, format!("enums can't be called; pick a case, like `{n}<…>.someCase(…)`"));
                    Ty::Error
                }
                _ => {
                    self.args_unchecked(args, trailing);
                    Ty::Error
                }
            },
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
        if n == COMPLETION_MARK {
            self.complete_names();
            return Ty::Error;
        }
        if let Some(module) = n.strip_suffix(COMPLETION_MARK).and_then(|m| m.strip_suffix('.')) {
            self.args_unchecked(args, trailing);
            self.complete_module(module);
            return Ty::Error;
        }
        if self.lookup(n).is_none() {
            match self.globals.get(n).copied() {
                Some(Global::GenericFn(t) | Global::GenericStruct(t)) => {
                    let (params, ret) = self.template_sig(t);
                    let Some(targs) = self.infer(t, &params, ret.as_ref(), args, trailing, expected, span) else {
                        self.args_unchecked(args, trailing);
                        return Ty::Error;
                    };
                    return match self.specialize(t, targs, name_span) {
                        Some(name) => self.call_name(&name, name_span, args, trailing, span, expected),
                        None => {
                            self.args_unchecked(args, trailing);
                            Ty::Error
                        }
                    };
                }
                Some(Global::GenericEnum(_)) => {
                    self.error(name_span, format!("enums can't be called; pick a case, like `{n}.someCase(…)`"));
                    return Ty::Error;
                }
                _ => {}
            }
        }
        let what = format!("`{n}`");
        if let Some(l) = self.lookup(n).cloned() {
            return match l.kind {
                Kind::Fn(sig, origin) => {
                    self.set_callee(match origin {
                        FnOrigin::Method(owner) => Callee::SelfMethod { owner, name: n.to_string() },
                        FnOrigin::View => Callee::ViewFn(n.to_string()),
                        FnOrigin::Local(decl) => Callee::LocalFn { name: n.to_string(), decl },
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
            Some(Global::GenericFn(_) | Global::GenericStruct(_) | Global::GenericEnum(_)) => {
                unreachable!("handled above")
            }
            Some(Global::Fn(i)) => {
                let sig = self.fns[i].clone();
                self.set_callee(Callee::Fn(n.to_string()));
                self.args(&what, &sig.params, args, trailing, span);
                sig.ret
            }
            Some(Global::Struct(id)) => {
                self.set_callee(Callee::StructInit(id));
                let mut params: Vec<ParamSig> = self.structs[id]
                    .fields
                    .iter()
                    .map(|f| ParamSig { has_default: f.has_default, ..ParamSig::new(&f.name, f.ty.clone()) })
                    .collect();
                // The prelude's `Error("reason")` needs no label.
                if Some(id) == self.prelude_error() && args.first().is_some_and(|a| a.label.is_none()) {
                    params[0].unlabeled = true;
                }
                for a in args {
                    let Some(l) = &a.label else { continue };
                    let field = self.structs[id].fields.iter().find(|f| f.name == l.name).map(|f| f.span);
                    if let Some(decl) = field {
                        let owner = self.structs[id].name.clone();
                        self.check_private_member(&format!("field `{}`", l.name), &owner, decl, l.span);
                    }
                }
                self.args(&what, &params, args, trailing, span);
                Ty::Struct(id)
            }
            Some(Global::View(i)) => {
                let params = self.views[i].clone();
                self.set_callee(Callee::View(n.to_string()));
                self.next_content = Some(Content::View);
                self.args(&what, &params, args, trailing, span);
                self.next_content = None;
                Ty::View
            }
            Some(Global::Enum(id)) => {
                let example = self.enums[id].cases.first().map_or("case".to_string(), |c| c.name.clone());
                self.error(name_span, format!("enums can't be called; pick a case, like `{n}.{example}`"));
                Ty::Error
            }
            Some(Global::Iface(_)) => {
                self.error(name_span, format!("`{n}` is an interface, so it can't be created directly"))
                    .help(format!("create a value of a type that conforms to it, like `struct Circle: {n}`"));
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
                    self.next_content = Some(if n == "Menu" { Content::Menu } else { Content::Layout });
                    self.args(&what, &params, args, trailing, span);
                    self.next_content = None;
                    if n == "MenuItem" {
                        self.menu_shortcut(args);
                    }
                    self.tag_ty = saved;
                    // A stepper counts in `Int`s or `Float`s.
                    if n == "Stepper" {
                        if let Some(a) = args.iter().find(|a| a.label.as_ref().is_some_and(|l| l.name == "value")) {
                            let t = self.types.get(&a.value.id).cloned().unwrap_or(Ty::Error);
                            if !matches!(t, Ty::Int | Ty::Float) && !t.has_error() {
                                let what = self.a(&t);
                                self.error(a.value.span, format!("a `Stepper` changes an `Int` or a `Float`, not {what}"));
                            }
                        }
                    }
                    return Ty::View;
                }
                if builtins::FUNCTION_NAMES.contains(&n) {
                    self.set_callee(Callee::BuiltinFn(n.to_string()));
                }
                match n {
                    "Int" | "Float" | "String" => return self.conversion(n, args, trailing, span),
                    "Data" => return self.data_init(args, trailing, span),
                    "Set" => return self.set_init(None, args, trailing, span, expected),
                    "background" => return self.background(args, trailing, span),
                    "toJson" | "toBinary" | "fromJson" | "fromBinary" => {
                        return self.codec_call(n, args, trailing, span, expected);
                    }
                    "abs" | "min" | "max" => return self.numeric_fn(n, args, trailing, span),
                    "zip" => return self.zip(args, trailing, span),
                    "print" | "printError" => {
                        let sig = self.builtins.function(n).unwrap();
                        self.args(&what, &sig.params, args, trailing, span);
                        if let Some(arg) = args.first().filter(|a| a.label.is_none()) {
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

    /// `background(work: { … }) { result in … }`: `work` runs on another
    /// thread with copies of the values it uses, and `done` gets a copy of
    /// its result on the UI thread. So `work` may only use values that can be
    /// copied that way (not `state`, bindings, or the app's functions), and
    /// its result must be something `toBinary` could save.
    ///
    /// `work` may take the job (`work: { job in … }`), to report progress
    /// (`job.progress(0.5)`, to the `progress:` block) and see whether it was
    /// cancelled (`job.isCancelled()`). `background` returns the job.
    fn background(&mut self, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let labeled = |l: &str| args.iter().find(|a| a.label.as_ref().is_some_and(|x| x.name == l));
        let work = labeled("work");
        let progress = labeled("progress");
        let done = trailing.or_else(|| labeled("done").map(|a| &a.value));
        let (Some(work), Some(done)) = (work, done) else {
            self.error(span, "write it as `background(work: { … }) { result in … }`");
            return builtins::JOB;
        };
        for a in args {
            match &a.label {
                Some(l) if matches!(l.name.as_str(), "work" | "progress" | "done") => {}
                Some(l) => {
                    self.error(l.span, format!("`background` has no `{}:`", l.name))
                        .help("it takes `work:`, `progress:` and the block for the result");
                }
                None => {
                    self.error(a.value.span, "`background`'s values need their labels")
                        .help("write it as `background(work: { … }) { result in … }`");
                }
            }
        }
        if let Some(p) = progress {
            self.expect(&p.value, &Ty::func(vec![Ty::Float, Ty::Str], Ty::Void));
        }
        let ExprKind::Closure { params, body } = &work.value.kind else {
            self.error(work.value.span, "`work:` must be a block, like `work: { compute(input) }`");
            return builtins::JOB;
        };
        if params.len() > 1 {
            self.error(work.value.span, "the `work:` block takes at most one value: the job")
                .help("it uses the values around it: `let input = x` then `background(work: { job in compute(input) }) { … }`");
            return builtins::JOB;
        }
        let expected = (!params.is_empty()).then(|| Ty::func(vec![builtins::JOB], Ty::Any));
        let wt = self.expr(&work.value, expected.as_ref());
        let result = match wt {
            Ty::Fn(_, r) => *r,
            _ => Ty::Error,
        };
        if result == Ty::Void {
            self.error(work.value.span, "the `work:` block must end with the value it computes")
                .help("the result goes to the block after it: `background(work: { total(items) }) { sum in … }`");
        } else if let Err(why) = self.saveable(&result) {
            let what = self.a(&result);
            self.error(work.value.span, format!("background work can't return {what}")).help(why);
        }
        // What the work uses from outside: copied in, so it must be plain values.
        let mut free = std::collections::BTreeSet::new();
        crate::codegen::free_block(body, &mut params.iter().map(|p| p.name.clone()).collect(), &mut free);
        for name in &free {
            let Some(local) = self.lookup(name).cloned() else { continue };
            let problem = match &local.kind {
                Kind::State => Some(format!("`{name}` is `state`, which the app keeps changing")),
                Kind::Bind => Some(format!("`{name}` is a binding to the app's data")),
                Kind::Field | Kind::SelfValue => Some(format!("`{name}` belongs to `self`")),
                Kind::Fn(_, FnOrigin::Local(_)) => Some(format!("`{name}` is declared inside a function")),
                Kind::Fn(..) => Some(format!("`{name}` is a function of the app or view")),
                _ => None,
            };
            if let Some(problem) = problem {
                let help = if matches!(local.kind, Kind::Fn(_, FnOrigin::Local(_))) {
                    "move the function to the top level of the file, and pass it what it needs".to_string()
                } else if matches!(local.kind, Kind::Fn(..)) {
                    "move the function out of the `app` or `view` to the top level, and pass it what it needs".to_string()
                } else {
                    format!("copy what it needs first, like `let input = {name}`, and use that")
                };
                self.error(work.value.span, format!("background work can't use `{name}`: {problem}")).help(help);
            } else if let Err(why) = self.saveable(&local.ty) {
                let what = self.a(&local.ty);
                self.error(work.value.span, format!("background work can't use `{name}`, which is {what}")).help(why);
            }
        }
        if result != Ty::Void && !result.has_error() {
            self.expect(done, &Ty::func(vec![result], Ty::Void));
        }
        builtins::JOB
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
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Data | Ty::Range | Ty::Error | Ty::Generic(_) => Ok(()),
            Ty::List(e) | Ty::Opt(e) | Ty::Set(e) => self.saveable(e),
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
            Ty::Iface(i) => {
                // Saved with its type's name, so any conforming type can be loaded back.
                for (t, _, _) in self.conforms.iter().filter(|(_, j, _)| j == i) {
                    self.saveable(t).map_err(|e| format!("`{}` (a `{}`): {e}", self.show(t), self.ifaces[*i].name))?;
                }
                Ok(())
            }
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

    #[allow(clippy::too_many_arguments)]
    fn method_call(
        &mut self,
        object: &Expr,
        name: &Ident,
        optional: bool,
        args: &[Arg],
        trailing: Option<&Expr>,
        span: Span,
        expected: Option<&Ty>,
    ) -> Ty {
        let n = name.name.as_str();
        // `Shape.circle(radius: 1)`
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none()
                && let Some(Global::Enum(id)) = self.globals.get(tn).copied() {
                    self.case_call(id, n, name.span, args, trailing, span);
                    return Ty::Enum(id);
                }
        // `Result<Int>.ok(value: 1)`
        if let ExprKind::GenericName(tn, targs) = &object.kind {
            return match self.generic_name_ty(tn, targs, object.span) {
                Ty::Enum(id) => {
                    self.case_call(id, n, name.span, args, trailing, span);
                    Ty::Enum(id)
                }
                Ty::Error => Ty::Error,
                _ => {
                    self.error(name.span, format!("`{tn}` is a struct, so it has no cases"));
                    Ty::Error
                }
            };
        }
        // `Result.ok(value: 1)`: the type arguments from the case's values.
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none()
            && let Some(Global::GenericEnum(t)) = self.globals.get(tn).copied()
        {
            let Some((fields, ret)) = self.template_case(t, n) else {
                self.error(name.span, format!("`{tn}` has no case `.{n}`"));
                return Ty::Error;
            };
            let Some(targs) = self.infer(t, &fields, Some(&ret), args, trailing, expected, span) else {
                self.args_unchecked(args, trailing);
                return Ty::Error;
            };
            return match self.specialize(t, targs, object.span).and_then(|nm| self.globals.get(&nm).copied()) {
                Some(Global::Enum(id)) => {
                    self.case_call(id, n, name.span, args, trailing, span);
                    Ty::Enum(id)
                }
                _ => Ty::Error,
            };
        }
        let ot = self.expr(object, None);
        let optional = optional || object.in_optional_chain();
        let Some((recv, wrap)) = self.receiver(ot, object.span, optional, n) else { return Ty::Error };
        if recv == Ty::Void {
            self.leading_dot(name);
            return Ty::Error;
        }
        let what = format!("`{n}`");
        let ret = match &recv {
            Ty::Error => Ty::Error,
            Ty::View => {
                self.set_callee(Callee::Modifier(n.to_string()));
                self.modifier_call(name, args, trailing, span)
            }
            // `Graphics`: a built-in struct whose methods are the runtime's.
            t if *t == builtins::GRAPHICS => match builtins::method_of(&recv, n) {
                Some(_) => {
                    // (`g.image(pixels: …)` is another method than `g.image(path, …)`.)
                    let first = args.first().and_then(|a| a.label.as_ref()).map(|l| l.name.as_str());
                    let variant = builtins::graphics_variant(n, first);
                    let m = builtins::method_of(&recv, variant).expect("a variant of a Graphics method");
                    self.set_callee(Callee::Builtin { recv: recv.clone(), name: variant.to_string() });
                    self.args(&what, &m.sig.params, args, trailing, span);
                    m.sig.ret
                }
                None if n == "width" || n == "height" => {
                    self.error(name.span, format!("`{n}` is a field, not a method")).help("remove the parentheses");
                    Ty::Error
                }
                None => {
                    let names = builtins::member_names(&recv).iter().map(|s| s.to_string()).collect();
                    self.no_member(name, &format!("`Graphics` has no method `{n}`"), names);
                    Ty::Error
                }
            },
            // `BackgroundJob`: its methods are the runtime's too.
            t if *t == builtins::JOB => match builtins::method_of(&recv, n) {
                Some(m) => {
                    self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                    self.args(&what, &m.sig.params, args, trailing, span);
                    m.sig.ret
                }
                None if n == "id" => {
                    self.error(name.span, format!("`{n}` is a field, not a method")).help("remove the parentheses");
                    Ty::Error
                }
                None => {
                    let names = builtins::member_names(&recv).iter().map(|s| s.to_string()).collect();
                    self.no_member(name, &format!("`BackgroundJob` has no method `{n}`"), names);
                    Ty::Error
                }
            },
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
                        self.check_private_member(&format!("method `{n}`"), &owner, m.span, name.span);
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
            // A method of `T: Shape`: one of the interface's (checked for real in each copy).
            Ty::Generic(k) => match self.type_params[*k].constraint {
                Some(i) => match self.ifaces[i].methods.iter().find(|(m, _)| m == n).map(|(_, s)| s.clone()) {
                    Some(sig) => {
                        self.set_callee(Callee::Value);
                        self.args(&what, &sig.params, args, trailing, span);
                        sig.ret
                    }
                    None => {
                        let (pname, iname) = (self.type_params[*k].name.clone(), self.ifaces[i].name.clone());
                        let names = self.ifaces[i].methods.iter().map(|(m, _)| m.clone()).collect();
                        self.no_member(name, &format!("`{pname}` is a `{iname}`, which has no method `{n}`"), names);
                        Ty::Error
                    }
                },
                None => {
                    let pname = self.type_params[*k].name.clone();
                    self.error(name.span, format!("`{pname}` could be any type, so it has no method `{n}`"))
                        .help(format!("require an interface that has it, like `<{pname}: Shape>`"));
                    Ty::Error
                }
            },
            Ty::Iface(id) => {
                let def = &self.ifaces[*id];
                match def.methods.iter().position(|(m, _)| m == n) {
                    Some(index) => {
                        let sig = def.methods[index].1.clone();
                        let mutating = def.mutating[index];
                        self.set_callee(Callee::IfaceMethod { iface: *id, index });
                        if mutating && !optional {
                            self.mutating_receiver(object, n);
                        }
                        self.args(&what, &sig.params, args, trailing, span);
                        sig.ret
                    }
                    None => {
                        let (iname, names) = (def.name.clone(), def.methods.iter().map(|(m, _)| m.clone()).collect());
                        self.no_member(name, &format!("`{iname}` has no method `{n}`"), names);
                        Ty::Error
                    }
                }
            }
            Ty::List(elem) if n == "map" => {
                self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                self.map_call(elem, args, trailing, span)
            }
            // `list.enumerated()`: each item with its position, `[(Int, T)]`.
            Ty::List(elem) if n == "enumerated" => {
                self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                if !args.is_empty() || trailing.is_some() {
                    self.args_unchecked(args, trailing);
                    self.error(span, "`enumerated()` takes nothing");
                }
                let pair = self.tuple_ty(vec![Ty::Int, (**elem).clone()], span);
                Ty::list(pair)
            }
            // `data.append(other)`, or one byte: `data.append(byte: 255)`.
            Ty::Data if n == "append" => {
                self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                self.mutating_receiver(object, n);
                let byte = args.first().and_then(|a| a.label.as_ref()).is_some_and(|l| l.name == "byte");
                let p = if byte { ParamSig::new("byte", Ty::Int) } else { ParamSig::unlabeled("data", Ty::Data) };
                self.args(&what, &[p], args, trailing, span);
                Ty::Void
            }
            Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Set(_) | Ty::Float | Ty::Int | Ty::Data => match builtins::method_of(&recv, n) {
                Some(m) => {
                    self.set_callee(Callee::Builtin { recv: recv.clone(), name: n.to_string() });
                    if m.mutating {
                        self.mutating_receiver(object, n);
                    }
                    self.args(&what, &m.sig.params, args, trailing, span);
                    if let (Ty::List(elem) | Ty::Set(elem), "sorted") = (&recv, n) {
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
                    if recv == Ty::Int && builtins::method_of(&Ty::Float, n).is_some() {
                        let d = self.error(name.span, format!("{ty} has no method `{n}`; it's a `Float` method"));
                        d.help(format!("convert it first: `Float(value).{n}(…)`"));
                        return Ty::Error;
                    }
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
            Ty::Fn(ps, r) if ps.len() == 1 && self.fits(elem, &ps[0]) => Ty::list(*r),
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
                self.next_content = Some(match n {
                    "contextMenu" => Content::Menu,
                    "menuBar" => Content::MenuBar,
                    _ => Content::Layout,
                });
                self.args(&format!("`.{n}`"), &params, args, trailing, span);
                self.next_content = None;
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

    /// `MenuItem(…, shortcut: "cmd+shift+s")`: modifiers and a key, joined
    /// by `+`, or after the symbols `⌘`, `⇧` and `⌥` (as the runtime reads
    /// it: `menu::Shortcut::parse`).
    fn menu_shortcut(&mut self, args: &[Arg]) {
        let Some(arg) = args.iter().find(|a| a.label.as_ref().is_some_and(|l| l.name == "shortcut")) else { return };
        let ExprKind::Str(parts) = &arg.value.kind else { return };
        let [StrPart::Text(text)] = parts.as_slice() else { return };
        const NAMED: &[&str] =
            &["enter", "escape", "tab", "backspace", "delete", "left", "right", "up", "down", "home", "end", "space"];
        let rest = text.trim().trim_start_matches(['⌘', '⇧', '⌥']);
        let mut words: Vec<&str> = rest.split('+').collect();
        if rest.ends_with("++") {
            words.truncate(words.len() - 2);
            words.push("+");
        }
        let key = words.pop().unwrap_or("").trim();
        let bad_modifier = words.iter().find(|m| !matches!(m.trim().to_lowercase().as_str(), "cmd" | "command" | "shift" | "alt" | "option" | "opt"));
        let mut chars = key.chars();
        let key_ok = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_whitespace()) || NAMED.contains(&key.to_lowercase().as_str());
        if let Some(m) = bad_modifier {
            let help = if m.trim().eq_ignore_ascii_case("ctrl") {
                "`cmd` is Cmd on macOS and Ctrl on Windows and Linux: write `cmd+…`".to_string()
            } else {
                "the modifiers are `cmd`, `shift` and `alt`, like `cmd+shift+s`".to_string()
            };
            self.error(arg.value.span, format!("`{text}` isn't a shortcut a menu item can have")).help(help);
        } else if text.trim().is_empty() || !key_ok {
            self.error(arg.value.span, format!("`{text}` isn't a shortcut a menu item can have")).help(
                "write modifiers and a key, like `cmd+s` or `⇧⌘S`; the key is one character or one of: enter, escape, tab, backspace, delete, left, right, up, down, home, end, space",
            );
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
        // Shift and Alt may be asked for too: `"shift+s"`, `"alt+shift+left"`.
        let mut bare = key.as_str();
        while let Some(rest) = bare.strip_prefix("shift+").or_else(|| bare.strip_prefix("alt+")).filter(|r| !r.is_empty()) {
            bare = rest;
        }
        let held = &key[..key.len() - bare.len()];
        let mut chars = bare.chars();
        let single = matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_whitespace() && !c.is_uppercase());
        if single || NAMED.contains(&bare) {
            return;
        }
        let help = if let Some(rest) = bare.strip_prefix("cmd+").or_else(|| bare.strip_prefix("ctrl+")) {
            format!("Cmd (Ctrl on Windows) is added for you: write `.shortcut(\"{held}{rest}\")`")
        } else if bare.chars().count() == 1 {
            format!("use the lowercase letter (add `shift+` for Shift): `.shortcut(\"{held}{}\")`", bare.to_lowercase())
        } else {
            "use one character, like \"s\", or one of: enter, escape, tab, backspace, delete, left, right, up, down, home, end; \
             put `shift+` or `alt+` before it to need Shift or Alt too"
                .to_string()
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
        // Content built for each number (`LazyVStack`'s rows): `{ i in … }`.
        if let (Ty::Fn(ps, _), true) = (&p.ty, p.rows) {
            {
                let pt = ps[0].clone();
                match &v.kind {
                    ExprKind::Closure { params, body } if params.len() == 1 => self.with_scope(|c| {
                        c.declare(&params[0], pt, Kind::Let);
                        c.content_block(body, false);
                    }),
                    ExprKind::Closure { .. } => {
                        self.error(v.span, "this block gets each row's number; name it at the start, like `{ i in … }`");
                    }
                    _ => {
                        self.expr(v, None);
                        self.error(v.span, "write the rows as a block, like `{ i in Text(\"Row {i}\") }`");
                    }
                }
                self.types.insert(v.id, p.ty.clone());
                return;
            }
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

// ===== generics =====

impl Checker<'_> {
    /// Gives each generic declaration its type parameters, and makes the copy
    /// in which they stay parameters: the declaration checked on its own.
    fn check_templates(&mut self) {
        for t in 0..self.templates.len() {
            let item = self.templates[t].item.clone();
            let generics = match &*item {
                Item::Fn(f) => &f.generics,
                Item::Struct(d) => &d.generics,
                Item::Enum(d) => &d.generics,
                _ => unreachable!(),
            };
            let mut params = Vec::new();
            for g in generics {
                if params.iter().any(|&k: &usize| self.type_params[k].name == g.name.name) {
                    self.error(g.name.span, format!("type parameter `{}` is listed twice", g.name.name));
                }
                let constraint = g.constraint.as_ref().and_then(|c| match self.globals.get(&c.name) {
                    Some(Global::Iface(i)) => Some(*i),
                    _ => {
                        self.error(c.span, format!("`{}` isn't an interface", c.name))
                            .help("a type parameter can be limited to types that conform to an interface: `<T: Shape>`");
                        None
                    }
                });
                params.push(self.type_params.len());
                self.type_params.push(TypeParam { name: g.name.name.clone(), constraint });
            }
            self.templates[t].params = params;
        }
        // (Every template's parameters are known before any is checked, as
        // one may use another: `fn swap<A, B>(p: (A, B))`.)
        for t in 0..self.templates.len() {
            let item = self.templates[t].item.clone();
            let args = self.templates[t].params.iter().copied().map(Ty::Generic).collect();
            self.specialize(t, args, item.name().span);
        }
    }

    fn template_name(&self, t: usize) -> String {
        self.templates[t].item.name().name.clone()
    }

    /// A type's name, unique even for type parameters (for naming copies).
    fn type_key(&self, t: &Ty) -> String {
        match t {
            Ty::Generic(k) => format!("${k}"),
            Ty::List(e) => format!("[{}]", self.type_key(e)),
            Ty::Dict(k, v) => format!("[{}: {}]", self.type_key(k), self.type_key(v)),
            Ty::Set(e) => format!("Set<{}>", self.type_key(e)),
            Ty::Opt(e) => format!("{}?", self.type_key(e)),
            Ty::Fn(ps, r) => format!(
                "fn({}) -> {}",
                ps.iter().map(|p| self.type_key(p)).collect::<Vec<_>>().join(", "),
                self.type_key(r)
            ),
            t => self.show(t),
        }
    }

    /// A type written out, to put in place of a type parameter in a copy.
    fn type_expr_of(&self, t: &Ty, span: Span) -> TypeExpr {
        let named = |name: String| TypeExpr::Named(Ident { name, span });
        match t {
            Ty::List(e) => TypeExpr::List(Box::new(self.type_expr_of(e, span)), span),
            Ty::Dict(k, v) => TypeExpr::Dict(Box::new(self.type_expr_of(k, span)), Box::new(self.type_expr_of(v, span)), span),
            Ty::Opt(e) => TypeExpr::Optional(Box::new(self.type_expr_of(e, span)), span),
            Ty::Set(e) => TypeExpr::Generic(Ident { name: "Set".into(), span }, vec![self.type_expr_of(e, span)], span),
            Ty::Fn(ps, r) => TypeExpr::Fn(
                ps.iter().map(|p| self.type_expr_of(p, span)).collect(),
                (**r != Ty::Void).then(|| Box::new(self.type_expr_of(r, span))),
                span,
            ),
            Ty::Generic(k) => named(format!("${k}")),
            Ty::Error | Ty::Any | Ty::Void => named("$error".into()),
            t => named(self.show(t)),
        }
    }

    fn contains_generic(t: &Ty) -> bool {
        match t {
            Ty::Generic(_) => true,
            Ty::List(e) | Ty::Opt(e) | Ty::Set(e) => Self::contains_generic(e),
            Ty::Dict(k, v) => Self::contains_generic(k) || Self::contains_generic(v),
            Ty::Fn(ps, r) => ps.iter().any(Self::contains_generic) || Self::contains_generic(r),
            _ => false,
        }
    }

    /// The copy of template `t` for type arguments `args` (made, and its
    /// signature checked, the first time): its name, which is a global.
    fn specialize(&mut self, t: usize, args: Vec<Ty>, span: Span) -> Option<String> {
        let tname = self.template_name(t);
        let params = self.templates[t].params.clone();
        if args.len() != params.len() {
            let want = params.len();
            let list: Vec<String> = params.iter().map(|&k| self.type_params[k].name.clone()).collect();
            self.error(span, format!("`{tname}` takes {want} type argument{}, like `{tname}<{}>`", if want == 1 { "" } else { "s" }, list.join(", ")));
            return None;
        }
        if args.iter().any(Ty::is_error) {
            return None;
        }
        for (&k, a) in params.iter().zip(&args) {
            let Some(c) = self.type_params[k].constraint else { continue };
            let ok = match a {
                Ty::Generic(j) => self.type_params[*j].constraint == Some(c),
                a => *a == Ty::Iface(c) || fits_with(a, &Ty::Iface(c), &self.conforms),
            };
            if !ok {
                let (pname, iname, what) = (self.type_params[k].name.clone(), self.ifaces[c].name.clone(), self.show(a));
                let help = if matches!(a, Ty::Struct(_) | Ty::Enum(_)) && !what.contains('<') {
                    format!("make `{what}` conform: `struct {what}: {iname} {{ … }}` (or `enum`)")
                } else {
                    format!("only structs and enums that conform to `{iname}` can be used here")
                };
                self.error(span, format!("`{tname}` needs a `{iname}` for `{pname}`, but `{what}` isn't one")).help(help);
                return None;
            }
        }
        let keys: Vec<String> = args.iter().map(|a| self.type_key(a)).collect();
        let key = format!("{tname}<{}>", keys.join(", "));
        if let Some(name) = self.specialized.get(&key) {
            return Some(name.clone());
        }
        if keys.iter().map(|k| k.matches('<').count()).max().unwrap_or(0) > 6 {
            self.error(span, format!("`{tname}` is nested too deeply here"))
                .help("a generic type that contains itself with other type arguments never ends");
            return None;
        }
        let concrete = !args.iter().any(Self::contains_generic);
        let name = if is_tuple_name(&tname) {
            // A tuple's copy is named like its type: `(Int, String)`.
            let parts: Vec<String> = args.iter().map(|a| if concrete { self.show(a) } else { self.type_key(a) }).collect();
            format!("({})", parts.join(", "))
        } else if concrete {
            format!("{tname}<{}>", args.iter().map(|a| self.show(a)).collect::<Vec<_>>().join(", "))
        } else {
            key.clone()
        };
        self.specialized.insert(key, name.clone());
        self.instance_of.insert(name.clone(), (t, args.clone()));
        let subst: HashMap<String, TypeExpr> = params
            .iter()
            .zip(&args)
            .map(|(&k, a)| (self.type_params[k].name.clone(), self.type_expr_of(a, span)))
            .collect();
        let template = self.templates[t].item.clone();
        let copy = (self.instance_of.len() as u32).min(0xFFFF);
        let item = Rc::new(crate::generics::specialize(&template, &name, &subst, &mut self.next_id, copy));
        let items = std::slice::from_ref(&*item);
        let saved = std::mem::replace(&mut self.template_copy, !concrete);
        let ids = self.collect(items, false);
        self.signatures(items, &ids);
        self.template_copy = saved;
        self.pending.push_back(Pending { item: item.clone(), id: ids[0], template: t, concrete, name: name.clone(), used_at: span });
        if concrete {
            self.instances.push(item);
        }
        Some(name)
    }

    /// Checks the bodies of the copies made so far (copies they need are
    /// made and checked too). A declaration checked on its own comes before
    /// its copies; if it had errors, they aren't repeated for each copy.
    fn check_pending(&mut self) {
        while let Some(p) = self.pending.pop_front() {
            if p.concrete && self.templates[p.template].had_errors {
                continue;
            }
            let before = self.diags.list.len();
            self.bodies(std::slice::from_ref(&*p.item), &[p.id]);
            let new = self.diags.list.len() - before;
            if !p.concrete {
                if new > 0 {
                    self.templates[p.template].had_errors = true;
                }
            } else {
                for d in &mut self.diags.list[before..] {
                    d.message += &format!(" (in `{}`)", p.name);
                    d.note_at(p.used_at, format!("`{}` is needed here", p.name));
                }
            }
        }
    }

    /// Binds type parameters in `pattern` (of template `params`) by matching
    /// it against an actual type.
    fn unify(&self, pattern: &Ty, actual: &Ty, params: &[usize], binds: &mut HashMap<usize, Ty>) {
        match (pattern, actual) {
            (_, Ty::Error | Ty::Any) => {}
            (Ty::Generic(k), a) if params.contains(k) => {
                binds.entry(*k).or_insert_with(|| a.clone());
            }
            (Ty::List(p), Ty::List(a)) | (Ty::Opt(p), Ty::Opt(a)) | (Ty::Set(p), Ty::Set(a)) => self.unify(p, a, params, binds),
            (Ty::Opt(p), a) => self.unify(p, a, params, binds),
            (Ty::Dict(pk, pv), Ty::Dict(ak, av)) => {
                self.unify(pk, ak, params, binds);
                self.unify(pv, av, params, binds);
            }
            (Ty::Fn(pp, pr), Ty::Fn(ap, ar)) => {
                for (p, a) in pp.iter().zip(ap) {
                    self.unify(p, a, params, binds);
                }
                self.unify(pr, ar, params, binds);
            }
            (Ty::Struct(_) | Ty::Enum(_), Ty::Struct(_) | Ty::Enum(_)) => {
                let (pn, an) = (self.show(pattern), self.show(actual));
                if let (Some((pt, pa)), Some((at, aa))) = (self.instance_of.get(&pn), self.instance_of.get(&an))
                    && pt == at
                {
                    let (pa, aa) = (pa.clone(), aa.clone());
                    for (p, a) in pa.iter().zip(&aa) {
                        self.unify(p, a, params, binds);
                    }
                }
            }
            _ => {}
        }
    }

    /// `t` with the bound type parameters replaced; None if one isn't bound.
    fn subst(&mut self, t: &Ty, binds: &HashMap<usize, Ty>, span: Span) -> Option<Ty> {
        Some(match t {
            Ty::Generic(k) => binds.get(k)?.clone(),
            Ty::List(e) => Ty::list(self.subst(e, binds, span)?),
            Ty::Set(e) => Ty::set(self.subst(e, binds, span)?),
            Ty::Opt(e) => Ty::opt(self.subst(e, binds, span)?),
            Ty::Dict(k, v) => Ty::Dict(Box::new(self.subst(k, binds, span)?), Box::new(self.subst(v, binds, span)?)),
            Ty::Fn(ps, r) => {
                let ps = ps.iter().map(|p| self.subst(p, binds, span)).collect::<Option<Vec<_>>>()?;
                Ty::Fn(ps, Box::new(self.subst(r, binds, span)?))
            }
            Ty::Struct(_) | Ty::Enum(_) if Self::generic_instance(self, t) => {
                let (tpl, args) = self.instance_of[&self.show(t)].clone();
                let args = args.iter().map(|a| self.subst(a, binds, span)).collect::<Option<Vec<_>>>()?;
                let name = self.specialize(tpl, args, span)?;
                match self.globals.get(&name)? {
                    Global::Struct(i) => Ty::Struct(*i),
                    Global::Enum(i) => Ty::Enum(*i),
                    _ => return None,
                }
            }
            t => t.clone(),
        })
    }

    /// Is `t` a copy of a generic type whose arguments still have parameters?
    fn generic_instance(&self, t: &Ty) -> bool {
        self.instance_of.get(&self.show(t)).is_some_and(|(_, args)| args.iter().any(Self::contains_generic))
    }

    /// What to expect for an argument of type `pattern`: fully known types,
    /// or a function whose result isn't known yet (its body tells).
    fn expect_for(&mut self, pattern: &Ty, binds: &HashMap<usize, Ty>, span: Span) -> Option<Ty> {
        if let Some(t) = self.subst(pattern, binds, span) {
            return Some(t);
        }
        if let Ty::Fn(ps, _) = pattern {
            let ps = ps.iter().map(|p| self.subst(p, binds, span)).collect::<Option<Vec<_>>>()?;
            return Some(Ty::Fn(ps, Box::new(Ty::Any)));
        }
        None
    }

    /// Works out the type arguments of a call to template `t`, whose
    /// parameters are `params` and result `ret`, from the arguments and the
    /// expected type. Reports an error if one can't be told.
    #[allow(clippy::too_many_arguments)]
    fn infer(
        &mut self,
        t: usize,
        params: &[ParamSig],
        ret: Option<&Ty>,
        args: &[Arg],
        trailing: Option<&Expr>,
        expected: Option<&Ty>,
        span: Span,
    ) -> Option<Vec<Ty>> {
        let tparams = self.templates[t].params.clone();
        let mut binds: HashMap<usize, Ty> = HashMap::new();
        if let (Some(r), Some(e)) = (ret, expected) {
            self.unify(r, e, &tparams, &mut binds);
        }
        // Which parameter each argument goes to.
        let mut pairs: Vec<(usize, &Expr)> = Vec::new();
        let mut positional = params.iter().enumerate().filter(|(_, p)| p.unlabeled).map(|(i, _)| i);
        for a in args {
            let i = match &a.label {
                Some(l) => params.iter().position(|p| !p.unlabeled && p.label == l.name),
                None => positional.next(),
            };
            if let Some(i) = i {
                pairs.push((i, &a.value));
            }
        }
        if let (Some(tr), Some(last)) = (trailing, params.len().checked_sub(1)) {
            pairs.push((last, tr));
        }
        // Try the arguments quietly (they're checked for real once the types
        // are known); blocks last, when their parameters' types are known.
        let mark = self.diags.list.len();
        for closures in [false, true] {
            for &(i, e) in &pairs {
                if matches!(e.kind, ExprKind::Closure { .. }) != closures {
                    continue;
                }
                let exp = self.expect_for(&params[i].ty, &binds, span);
                let found = self.expr(e, exp.as_ref());
                self.unify(&params[i].ty, &found, &tparams, &mut binds);
            }
        }
        self.diags.list.truncate(mark);
        let mut out = Vec::new();
        for &k in &tparams {
            match binds.get(&k) {
                Some(t) => out.push(t.clone()),
                None if is_tuple_name(&self.template_name(t)) => {
                    self.error(span, "can't tell the types of this tuple's items")
                        .help("give it a type, like `let pair: (Int, String?) = (1, nil)`");
                    return None;
                }
                None => {
                    let (tname, pname) = (self.template_name(t), self.type_params[k].name.clone());
                    let help = match &*self.templates[t].item {
                        Item::Fn(_) => format!("give the result a type, like `let x: Int = {tname}(…)`, or pass values that show what `{pname}` is"),
                        _ => format!("write the type, like `{tname}<Int>(…)`, or give the variable a type: `var x: {tname}<Int> = {tname}(…)`"),
                    };
                    self.error(span, format!("can't tell what `{pname}` is in this use of `{tname}`")).help(help);
                    return None;
                }
            }
        }
        Some(out)
    }

    /// The signature of the copy of template `t` that the checker checks on
    /// its own (in terms of its type parameters).
    fn template_sig(&self, t: usize) -> (Vec<ParamSig>, Option<Ty>) {
        let args: Vec<Ty> = self.templates[t].params.iter().map(|&k| Ty::Generic(k)).collect();
        let keys: Vec<String> = args.iter().map(|a| self.type_key(a)).collect();
        let key = format!("{}<{}>", self.template_name(t), keys.join(", "));
        let Some(name) = self.specialized.get(&key) else { return (Vec::new(), None) };
        match self.globals.get(name) {
            Some(Global::Fn(i)) => (self.fns[*i].params.clone(), Some(self.fns[*i].ret.clone())),
            Some(Global::Struct(i)) => {
                let params = self.structs[*i]
                    .fields
                    .iter()
                    .map(|f| ParamSig { has_default: f.has_default, ..ParamSig::new(&f.name, f.ty.clone()) })
                    .collect();
                (params, Some(Ty::Struct(*i)))
            }
            Some(Global::Enum(i)) => (Vec::new(), Some(Ty::Enum(*i))),
            _ => (Vec::new(), None),
        }
    }

    /// The case `case` of generic enum template `t`, its fields in terms of
    /// the type parameters.
    fn template_case(&self, t: usize, case: &str) -> Option<(Vec<ParamSig>, Ty)> {
        let (_, ret) = self.template_sig(t);
        let Some(Ty::Enum(id)) = ret else { return None };
        let c = self.enums[id].case(case)?;
        Some((c.fields.clone(), Ty::Enum(id)))
    }

    /// The prelude's `Error` struct, if the program uses it.
    fn prelude_error(&self) -> Option<usize> {
        match self.globals.get("Error") {
            Some(Global::Struct(id)) if self.structs[*id].span.file == crate::source::PRELUDE_FILE => Some(*id),
            _ => None,
        }
    }

    /// The prelude's `Result` template, if the program uses it.
    fn prelude_result(&self) -> Option<usize> {
        match self.globals.get("Result") {
            Some(Global::GenericEnum(t)) if self.templates[*t].item.name().span.file == crate::source::PRELUDE_FILE => Some(*t),
            _ => None,
        }
    }

    /// Is `t` a `Result<…>` (of the prelude)? Its value type if so.
    fn result_value(&self, t: &Ty) -> Option<Ty> {
        let tpl = self.prelude_result()?;
        let (t2, args) = self.instance_of.get(&self.show(t))?;
        (*t2 == tpl).then(|| args[0].clone())
    }

    /// `try x`: `x` is a `Result<T>` (giving a `T`) or an `Error?` (giving
    /// nothing); a failure is returned from the enclosing function, which
    /// must return a `Result` or `Error?` (or is `fn main`, which exits).
    fn try_expr(&mut self, e: &Expr, inner: &Expr, expected: Option<&Ty>) -> Ty {
        let (Some(tpl), Some(err_id)) = (self.prelude_result(), self.prelude_error()) else {
            self.expr(inner, None);
            self.error(e.span, "`try` works with the built-in `Result` and `Error`, but this program declares its own")
                .help("rename your own `Result` or `Error` to use `try`");
            return Ty::Error;
        };
        let err_opt = Ty::opt(Ty::Struct(err_id));
        // `let n: Int = try parseInt(s)`: the call is expected to give a `Result<Int>`.
        let want = match expected {
            Some(t) if !t.has_error() && *t != Ty::Void && *t != Ty::Any => {
                self.specialize(tpl, vec![t.clone()], inner.span).and_then(|n| match self.globals.get(&n) {
                    Some(Global::Enum(i)) => Some(Ty::Enum(*i)),
                    _ => None,
                })
            }
            _ => None,
        };
        let it = self.expr(inner, want.as_ref());
        let result = if it.has_error() {
            Ty::Error
        } else if let Some(v) = self.result_value(&it) {
            v
        } else if it == err_opt {
            Ty::Void
        } else {
            let what = self.a(&it);
            self.error(inner.span, format!("`try` works on a `Result` or an `Error?`, and this is {what}"))
                .help("for an optional, use `if let` or `??` instead");
            return Ty::Error;
        };
        let target = match self.returns.last() {
            _ if self.in_main && self.returns.len() == 1 => Some(TryTarget::Exit),
            Some(r) if self.result_value(r).is_some() => Some(TryTarget::Result(r.clone())),
            Some(r) if *r == err_opt => Some(TryTarget::ErrorOpt(r.clone())),
            Some(r) if r.has_error() => None,
            _ => {
                self.error(e.span, "`try` passes a failure on, but this function doesn't return a `Result`")
                    .help("make it return one, like `-> Result<Int>`, or handle the failure here with `match`, `valueOr(…)` or `if let x = r.value()`");
                None
            }
        };
        if let Some(target) = target {
            self.tries.insert(e.id, target);
        }
        result
    }

    /// Checks the arguments of a call that failed before they were matched
    /// to parameters, so errors inside them are still found.
    fn args_unchecked(&mut self, args: &[Arg], trailing: Option<&Expr>) {
        let mark = self.diags.list.len();
        for a in args.iter().map(|a| &a.value).chain(trailing) {
            self.expr(a, None);
        }
        // Only name errors: types can't be known without the parameters.
        let keep: Vec<Diagnostic> =
            self.diags.list.drain(mark..).filter(|d| d.message.starts_with("cannot find")).collect();
        self.diags.list.extend(keep);
    }

    /// The tuple type of `items`: `(Int, String)`.
    fn tuple_ty(&mut self, items: Vec<Ty>, span: Span) -> Ty {
        if items.iter().any(Ty::has_error) {
            return Ty::Error;
        }
        let Some(Global::GenericStruct(t)) = self.globals.get(&tuple_name(items.len())).copied() else { return Ty::Error };
        match self.specialize(t, items, span).and_then(|name| self.globals.get(&name).copied()) {
            Some(Global::Struct(id)) => Ty::Struct(id),
            _ => Ty::Error,
        }
    }

    /// `zip(a, b)`: pairs of the items at the same positions of two lists,
    /// as long as the shorter one.
    fn zip(&mut self, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let ([a, b], None) = (args, trailing) else {
            self.args_unchecked(args, trailing);
            self.error(span, "write it as `zip(firstList, secondList)`");
            return Ty::Error;
        };
        if a.label.is_some() || b.label.is_some() {
            self.error(span, "`zip` takes its two lists without labels: `zip(names, ages)`");
        }
        let mut elems = Vec::new();
        for x in [a, b] {
            match self.expr(&x.value, None) {
                Ty::List(e) => elems.push(*e),
                t if t.has_error() => elems.push(Ty::Error),
                t => {
                    let what = self.a(&t);
                    self.error(x.value.span, format!("`zip` pairs up two lists, and this is {what}"));
                    elems.push(Ty::Error);
                }
            }
        }
        if let Some(f) = self.frames.last_mut() {
            f.args = vec![Some(ArgRef::Arg(0)), Some(ArgRef::Arg(1))];
        }
        match self.tuple_ty(elems, span) {
            Ty::Error => Ty::Error,
            t => Ty::list(t),
        }
    }

    /// `Set<T>`, whose items must be able to be dictionary keys.
    fn set_type(&mut self, targs: &[TypeExpr], span: Span) -> Ty {
        let [item] = targs else {
            self.error(span, "`Set` takes one type, like `Set<Int>`");
            return Ty::Error;
        };
        let t = self.resolve(item);
        if !self.is_key(&t) {
            let what = self.a(&t);
            self.error(item.span(), format!("{what} can't be the items of a set"))
                .help("set items can be `Int`, `String`, `Bool`, an enum whose cases have no values, or a tuple or struct of those");
            return Ty::Error;
        }
        Ty::set(t)
    }

    /// `Set(list)`, `Set<Int>()`, or `Set()` where the type is known.
    fn set_init(&mut self, elem: Option<Ty>, args: &[Arg], trailing: Option<&Expr>, span: Span, expected: Option<&Ty>) -> Ty {
        self.set_callee(Callee::BuiltinFn("Set".into()));
        let elem = elem.or_else(|| match expected.map(strip_opt) {
            Some(Ty::Set(t)) => Some((**t).clone()),
            _ => None,
        });
        match (args, trailing) {
            ([], None) => match elem {
                Some(t) => Ty::set(t),
                None => {
                    self.error(span, "can't tell what type this empty set is")
                        .help("write the type of its items, like `Set<String>()`");
                    Ty::Error
                }
            },
            ([a], None) if a.label.is_none() => {
                let lt = match &elem {
                    Some(t) => self.expect(&a.value, &Ty::list(t.clone())),
                    None => self.expr(&a.value, None),
                };
                if let Some(f) = self.frames.last_mut() {
                    f.args = vec![Some(ArgRef::Arg(0))];
                }
                match lt {
                    Ty::List(t) if self.is_key(&t) => Ty::set(*t),
                    Ty::List(t) => {
                        let what = self.a(&t);
                        self.error(a.value.span, format!("{what} can't be the items of a set"))
                            .help("set items can be `Int`, `String`, `Bool`, an enum whose cases have no values, or a tuple or struct of those");
                        Ty::Error
                    }
                    t if t.has_error() => Ty::Error,
                    t => {
                        let what = self.a(&t);
                        self.error(a.value.span, format!("a set is made from a list, and this is {what}"))
                            .help("write it like `Set([1, 2, 3])`, or `Set<Int>()` for an empty one");
                        Ty::Error
                    }
                }
            }
            _ => {
                self.args_unchecked(args, trailing);
                self.error(span, "make a set with `Set(list)`, or `Set<Int>()` for an empty one");
                Ty::Error
            }
        }
    }

    /// The type of a generic name with its type arguments written out:
    /// `Stack<Int>` (a struct or enum copy).
    fn generic_name_ty(&mut self, n: &str, targs: &[TypeExpr], span: Span) -> Ty {
        if n == "Set" {
            return self.set_type(targs, span);
        }
        let args: Vec<Ty> = targs.iter().map(|a| self.resolve(a)).collect();
        let t = match self.globals.get(n).copied() {
            Some(Global::GenericStruct(t) | Global::GenericEnum(t)) => t,
            Some(Global::GenericFn(_)) => {
                self.error(span, format!("`{n}` works out its types from its arguments; call it without `<…>`"));
                return Ty::Error;
            }
            _ => {
                self.error(span, format!("`{n}` isn't generic, so it takes no `<…>`"));
                return Ty::Error;
            }
        };
        match self.specialize(t, args, span).and_then(|name| self.globals.get(&name).copied()) {
            Some(Global::Struct(i)) => Ty::Struct(i),
            Some(Global::Enum(i)) => Ty::Enum(i),
            _ => Ty::Error,
        }
    }
}

// ===== completion =====

impl Checker<'_> {
    fn suggest(&mut self, items: Vec<Suggestion>) {
        if self.completion.is_none() {
            self.completion = Some(items);
        }
    }

    /// `(label: Type, …) -> Result`.
    fn sig_text(&self, params: &[ParamSig], ret: &Ty) -> String {
        let ps: Vec<String> = params
            .iter()
            .map(|p| {
                let t = self.show(&p.ty);
                if p.unlabeled { t } else { format!("{}: {t}", p.label) }
            })
            .collect();
        let ret = if *ret == Ty::Void { String::new() } else { format!(" -> {}", self.show(ret)) };
        format!("({}){ret}", ps.join(", "))
    }

    fn item(label: &str, detail: String, kind: SuggestionKind) -> Suggestion {
        Suggestion { label: label.to_string(), detail, kind }
    }

    /// Everything a name here could be.
    fn complete_names(&mut self) {
        use SuggestionKind as K;
        let mut items: Vec<Suggestion> = Vec::new();
        let mut seen = HashSet::new();
        let locals: Vec<Local> = self.scopes.iter().rev().flat_map(|s| s.iter().rev()).cloned().collect();
        for l in locals {
            if l.name.starts_with("__") || !seen.insert(l.name.clone()) {
                continue;
            }
            let (detail, kind) = match &l.kind {
                Kind::Fn(sig, _) => (self.sig_text(&sig.params, &sig.ret), K::Function),
                Kind::State => (format!("state {}", self.show(&l.ty)), K::Variable),
                Kind::Field => (format!("field {}", self.show(&l.ty)), K::Field),
                _ => (self.show(&l.ty), K::Variable),
            };
            items.push(Self::item(&l.name, detail, kind));
        }
        let mut globals: Vec<(String, Global)> = self.globals.iter().map(|(n, g)| (n.clone(), *g)).collect();
        globals.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, g) in globals {
            // Not tuple structs or specialized copies, which can't be written by name.
            // (Nor other modules' declarations, which are written `module.name`.)
            if name.starts_with('(') || name.contains(['<', '.']) || !seen.insert(name.clone()) {
                continue;
            }
            let (detail, kind) = match g {
                Global::Fn(i) => (self.sig_text(&self.fns[i].params, &self.fns[i].ret), K::Function),
                Global::View(i) => (format!("view {}", self.sig_text(&self.views[i], &Ty::Void)), K::View),
                Global::Struct(_) => ("struct".to_string(), K::Type),
                Global::Enum(_) => ("enum".to_string(), K::Type),
                Global::Iface(_) => ("interface".to_string(), K::Type),
                Global::GenericFn(_) => ("generic function".to_string(), K::Function),
                Global::GenericStruct(_) => ("generic struct".to_string(), K::Type),
                Global::GenericEnum(_) => ("generic enum".to_string(), K::Type),
                Global::App => continue,
            };
            items.push(Self::item(&name, detail, kind));
        }
        for n in builtins::VIEW_NAMES {
            if seen.insert(n.to_string()) {
                let detail = self.builtins.view(n).map(|ps| format!("view {}", self.sig_text(&ps, &Ty::Void))).unwrap_or_default();
                items.push(Self::item(n, detail, K::View));
            }
        }
        for n in builtins::FUNCTION_NAMES.iter().filter(|n| !n.starts_with("__")) {
            if seen.insert(n.to_string()) {
                let detail = self.builtins.function(n).map(|s| self.sig_text(&s.params, &s.ret)).unwrap_or_default();
                items.push(Self::item(n, detail, K::Function));
            }
        }
        for n in PRIMITIVES {
            if seen.insert(n.to_string()) {
                items.push(Self::item(n, "type".to_string(), K::Type));
            }
        }
        items.push(Self::item("pi", "Float".to_string(), K::Variable));
        for k in KEYWORDS {
            items.push(Self::item(k, String::new(), K::Keyword));
        }
        self.suggest(items);
    }

    /// `object.` — the members of its type, or the cases of an enum.
    /// The public declarations of a module, after `geometry.`.
    fn complete_module(&mut self, module: &str) {
        use SuggestionKind as K;
        let prefix = format!("{module}.");
        let mut found: Vec<(String, Global)> = self
            .globals
            .iter()
            .filter(|(n, _)| n.starts_with(&prefix) && !n.contains('<'))
            .filter(|(n, _)| self.user_spans.get(*n).is_some_and(|s| self.public.contains(s)))
            .map(|(n, g)| (n.clone(), *g))
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let items = found
            .into_iter()
            .map(|(n, g)| {
                let label = &n[prefix.len()..];
                let (detail, kind) = match g {
                    Global::Fn(i) => (self.sig_text(&self.fns[i].params, &self.fns[i].ret), K::Function),
                    Global::View(i) => (format!("view {}", self.sig_text(&self.views[i], &Ty::Void)), K::View),
                    Global::Struct(_) | Global::GenericStruct(_) => ("struct".to_string(), K::Type),
                    Global::Enum(_) | Global::GenericEnum(_) => ("enum".to_string(), K::Type),
                    Global::Iface(_) => ("interface".to_string(), K::Type),
                    Global::GenericFn(_) => ("generic function".to_string(), K::Function),
                    Global::App => (String::new(), K::Type),
                };
                Self::item(label, detail, kind)
            })
            .collect();
        self.suggest(items);
    }

    fn complete_member(&mut self, object: &Expr) {
        if let ExprKind::Name(tn) = &object.kind
            && self.lookup(tn).is_none()
            && let Some(Global::Enum(id)) = self.globals.get(tn).copied()
        {
            let items = self.cases(id);
            self.suggest(items);
            return;
        }
        let t = self.expr(object, None);
        let items = self.members(strip_opt(&t));
        self.suggest(items);
    }

    /// `.name` on its own: the cases of the expected enum.
    fn complete_dot(&mut self, expected: Option<&Ty>) {
        let items = match expected.map(strip_opt) {
            Some(Ty::Enum(id)) => self.cases(*id),
            Some(Ty::Float) => vec![Self::item("infinity", "Float".to_string(), SuggestionKind::Case)],
            // After a view on the line before (`Text("a")` then `.padding`),
            // the parser already made this a member; otherwise modifiers
            // are the likeliest.
            _ => self.members(&Ty::View),
        };
        self.suggest(items);
    }

    fn cases(&self, id: usize) -> Vec<Suggestion> {
        self.enums[id]
            .cases
            .iter()
            .map(|c| {
                let detail = if c.fields.is_empty() {
                    self.enums[id].name.clone()
                } else {
                    format!("{}{}", self.enums[id].name, self.sig_text(&c.fields, &Ty::Void))
                };
                Self::item(&c.name, detail, SuggestionKind::Case)
            })
            .collect()
    }

    fn members(&self, t: &Ty) -> Vec<Suggestion> {
        use SuggestionKind as K;
        let methods = |c: &Self, ms: &[MethodDef]| -> Vec<Suggestion> {
            ms.iter().map(|m| Self::item(&m.name, c.sig_text(&m.sig.params, &m.sig.ret), K::Method)).collect()
        };
        match t {
            Ty::Struct(id) => {
                let def = &self.structs[*id];
                let mut items: Vec<Suggestion> =
                    def.fields.iter().map(|f| Self::item(&f.name, self.show(&f.ty), K::Field)).collect();
                items.extend(methods(self, &def.methods));
                items
            }
            Ty::Enum(id) => methods(self, &self.enums[*id].methods),
            Ty::Iface(id) => {
                let def = &self.ifaces[*id];
                let mut items: Vec<Suggestion> = def.props.iter().map(|(n, t)| Self::item(n, self.show(t), K::Field)).collect();
                items.extend(def.methods.iter().map(|(n, sig)| Self::item(n, self.sig_text(&sig.params, &sig.ret), K::Method)));
                items
            }
            Ty::View => builtins::member_names(t)
                .iter()
                .map(|n| {
                    let detail = self.builtins.modifier(n).map(|ps| self.sig_text(&ps, &Ty::Void)).unwrap_or_default();
                    Self::item(n, detail, K::Modifier)
                })
                .collect(),
            _ => builtins::member_names(t)
                .iter()
                .map(|n| match (builtins::property(t, n), builtins::method_of(t, n)) {
                    (Some(pt), _) => Self::item(n, self.show(&pt), K::Field),
                    (None, Some(m)) => Self::item(n, self.sig_text(&m.sig.params, &m.sig.ret), K::Method),
                    (None, None) => Self::item(n, String::new(), K::Method),
                })
                .collect(),
        }
    }
}

/// Does `b` always leave early (its last statement is `return`, `break` or `continue`)?
fn diverges(b: &Block) -> bool {
    matches!(b.stmts.last(), Some(Stmt::Return { .. } | Stmt::Break(_) | Stmt::Continue(_)))
}

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
/// Does converting `a` to `b` turn structs or enums into an interface?
fn converts_to_iface(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::Struct(_) | Ty::Enum(_), Ty::Iface(_)) => true,
        (Ty::List(a), Ty::List(b)) | (Ty::Opt(a), Ty::Opt(b)) => converts_to_iface(a, b),
        _ => false,
    }
}

fn fits_with(found: &Ty, expected: &Ty, conforms: &[(Ty, usize, Span)]) -> bool {
    if found.has_error() || expected.has_error() || *expected == Ty::Any || found == expected {
        return true;
    }
    match (found, expected) {
        // `[Circle]` as `[Shape]`, `Circle?` as `Shape?`: converted item by item.
        (Ty::List(a), Ty::List(b)) | (Ty::Opt(a), Ty::Opt(b)) if converts_to_iface(a, b) => {
            fits_with(a, b, conforms)
        }
        (_, Ty::Opt(inner)) => fits_with(found, inner, conforms),
        (Ty::Fn(fp, fr), Ty::Fn(ep, er)) => {
            fp.len() == ep.len() && fp.iter().zip(ep).all(|(a, b)| a == b || a.has_error()) && fits_with(fr, er, conforms)
        }
        (Ty::Struct(_) | Ty::Enum(_), Ty::Iface(i)) => conforms.iter().any(|(t, j, _)| t == found && j == i),
        _ => false,
    }
}

fn equatable(t: &Ty) -> bool {
    match t {
        Ty::Fn(..) | Ty::View | Ty::Any => false,
        Ty::List(t) | Ty::Opt(t) | Ty::Set(t) => equatable(t),
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
        Stmt::While { cond, body, .. } => {
            let c = match cond {
                Cond::Expr(c) | Cond::Let(_, _, c) => c,
            };
            expr_mutates(c, roots) || block_mutates(body, roots)
        }
        Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::Expr(e) => expr_mutates(e, roots),
        Stmt::Fn(f) => block_mutates(&f.body, roots),
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
        ExprKind::Unary(_, x) | ExprKind::Cast { value: x, .. } | ExprKind::Try(x) => m(x),
        ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => m(l) || m(r),
        ExprKind::Member { object, .. } => m(object),
        ExprKind::Closure { body, .. } | ExprKind::Block(body) => block_mutates(body, roots),
        ExprKind::If { cond, then, otherwise } => {
            let c = match cond {
                Cond::Expr(x) | Cond::Let(_, _, x) => m(x),
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
