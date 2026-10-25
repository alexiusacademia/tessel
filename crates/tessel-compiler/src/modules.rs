//! Modules: folders of code that a program (or another module) imports.
//!
//! `import geometry` loads the `.tsl` files of the folder `geometry` next to
//! the importing file (`import "../shared/geometry"` names any folder). Its
//! declarations are used by name, `geometry.Circle`, and only those marked
//! `public` can be. Inside the module, its files see each other's
//! declarations as usual.
//!
//! This pass gives each module declaration its full name (`Circle` in module
//! `geometry` becomes `geometry.Circle`) and rewrites every use to match, so
//! the type checker sees one program with unique names. It also checks
//! `public` and top-level `private` (a name only its own file can use).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::diag::{suggest, Diagnostic, Diagnostics};
use crate::source::{SourceMap, Span, FILE_MASK};

struct Module {
    name: String,
    /// Its top-level declarations.
    decls: HashMap<String, Decl>,
}

#[derive(Clone, Copy)]
struct Decl {
    public: bool,
    private: bool,
    span: Span,
}

/// Parses the program's files (`root`, already parsed) and the modules they
/// import (added to `sources`), and returns them as one program with
/// module names made full.
pub fn resolve(sources: &mut SourceMap, root: Vec<(u32, Program)>, diags: &mut Diagnostics, next_id: &mut ExprId) -> Program {
    let root_dirs: HashSet<PathBuf> =
        root.iter().filter_map(|(id, _)| folder_of(&sources.file(*id).name).canonicalize().ok()).collect();
    let program = root.first().map(|(id, _)| folder_of(&sources.file(*id).name));
    let mut loader = Loader { modules: Vec::new(), by_dir: HashMap::new(), root_dirs, program, diags, next_id };
    // (file, its parse, its module; None for the program's own files)
    let mut files: Vec<(u32, Program, Option<usize>)> = root.into_iter().map(|(id, p)| (id, p, None)).collect();
    let mut imports: HashMap<u32, HashMap<String, usize>> = HashMap::new();
    let mut i = 0;
    while i < files.len() {
        let (id, module) = (files[i].0, files[i].2);
        let base = folder_of(&sources.file(id).name);
        let mut seen: HashMap<String, usize> = HashMap::new();
        for imp in files[i].1.imports.clone() {
            let Some(m) = loader.load(&imp, &base, sources, &mut files) else { continue };
            if Some(m) == module {
                loader.diags.push(Diagnostic::error(imp.span, "a module can't import itself"));
                continue;
            }
            // (The file uses the module by its alias, when it gives one.)
            let name = imp.alias.clone().unwrap_or_else(|| loader.modules[m].name.clone());
            if seen.insert(name.clone(), m).is_some() {
                loader.diags.push(Diagnostic::error(imp.span, format!("`{name}` is imported twice in this file")));
            }
        }
        imports.insert(id, seen);
        i += 1;
    }
    let Loader { mut modules, diags, .. } = loader;

    // Every file's top-level declarations, by module.
    let mut root_decls: HashMap<String, Decl> = HashMap::new();
    for (_, prog, module) in &files {
        for item in &prog.items {
            let name = item.name();
            let decl = Decl { public: prog.public.contains(&name.span), private: prog.private.contains(&name.span), span: name.span };
            if let (Item::App(_), Some(m)) = (item, module) {
                let mname = modules[*m].name.clone();
                diags.push(
                    Diagnostic::error(name.span, format!("module `{mname}` can't have an `app`"))
                        .with_help("the `app` belongs to the program that imports the module; make this a `view` and mark it `public`"),
                );
            }
            match module {
                Some(m) => modules[*m].decls.entry(name.name.clone()).or_insert(decl),
                None => root_decls.entry(name.name.clone()).or_insert(decl),
            };
        }
    }

    let mut program = Program::default();
    for (id, mut prog, module) in files {
        let mut r = Resolver {
            module,
            imports: imports.remove(&id).unwrap_or_default(),
            modules: &modules,
            root: &root_decls,
            scopes: Vec::new(),
            types: Vec::new(),
            statics: None,
            diags,
            sources,
        };
        for item in &mut prog.items {
            r.item(item);
        }
        program.items.extend(prog.items);
        program.private.extend(prog.private);
        program.public.extend(prog.public);
        // (A module's tests are its own: they run with `tessel test` in its
        // folder, not in the programs that import it.)
        if module.is_none() {
            program.tests.extend(prog.tests);
        }
    }
    program
}

/// Among `items` (declarations that only use each other, like the
/// prelude's), gives those named in `names` the full name `owner.name`,
/// and rewrites their uses in `items` to match: they stay what `items`
/// mean by those names, whatever else is declared under them.
pub fn set_apart(items: &mut [Item], names: &HashSet<String>, owner: &str, sources: &SourceMap, diags: &mut Diagnostics) {
    let decls = items
        .iter()
        .map(|i| i.name())
        .filter(|n| names.contains(&n.name))
        .map(|n| (n.name.clone(), Decl { public: false, private: false, span: n.span }))
        .collect();
    let modules = [Module { name: owner.to_string(), decls }];
    let root = HashMap::new();
    let mut r = Resolver { module: Some(0), imports: HashMap::new(), modules: &modules, root: &root, scopes: Vec::new(), types: Vec::new(), statics: None, diags, sources };
    let prefix = format!("{owner}.");
    for item in items {
        r.item(item);
        // (The resolver names every declaration after the module: only
        // the ones set apart keep that.)
        let name = item.name_mut();
        if let Some(own) = name.name.strip_prefix(&prefix).filter(|own| !names.contains(*own)) {
            name.name = own.to_string();
        }
    }
}

/// The names Tessel gives a meaning to: its built-in functions, views and
/// types, and what the prelude declares. A module's public declaration of
/// one of them is used by its full name (`m.Color`), so the bare name keeps
/// meaning Tessel's.
fn tessel_names() -> &'static HashSet<String> {
    static NAMES: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        use crate::builtins;
        let mut names: HashSet<String> = HashSet::new();
        let lists: [&[&str]; 4] = [builtins::FUNCTION_NAMES, builtins::VIEW_NAMES, builtins::MENU_VIEWS, builtins::YIELDING_VIEWS];
        names.extend(lists.iter().flat_map(|l| l.iter().map(|n| n.to_string())));
        names.extend(builtins::enums().into_iter().map(|e| e.name));
        names.extend(builtins::structs().into_iter().map(|s| s.name));
        names.extend(["Int", "Float", "Bool", "String", "Data", "Range", "View", "Set", "pi"].map(String::from));
        let (mut diags, mut next_id) = (Diagnostics::default(), 0);
        let prelude = crate::parser::parse_file(crate::source::PRELUDE_FILE, crate::source::PRELUDE, &mut diags, &mut next_id);
        names.extend(prelude.items.iter().map(|i| i.name().name.clone()));
        names
    })
}

/// The folder a source file is in.
fn folder_of(file: &str) -> PathBuf {
    match Path::new(file).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

struct Loader<'a> {
    modules: Vec<Module>,
    by_dir: HashMap<PathBuf, usize>,
    root_dirs: HashSet<PathBuf>,
    /// The program's folder, for finding the libraries it depends on.
    program: Option<PathBuf>,
    diags: &'a mut Diagnostics,
    next_id: &'a mut ExprId,
}

impl Loader<'_> {
    fn error(&mut self, span: Span, msg: impl Into<String>, help: impl Into<String>) {
        self.diags.push(Diagnostic::error(span, msg).with_help(help));
    }

    /// The module `imp` names (from a file in folder `base`), loaded the
    /// first time: its files are added to `sources` and to `files`.
    fn load(
        &mut self,
        imp: &Import,
        base: &Path,
        sources: &mut SourceMap,
        files: &mut Vec<(u32, Program, Option<usize>)>,
    ) -> Option<usize> {
        let mut dir = base.join(&imp.path);
        let mut shown = dir.display().to_string();
        let mut found = dir.canonicalize().ok().filter(|c| c.is_dir());
        // A name that isn't a folder here may be a library the program uses.
        if found.is_none()
            && !imp.quoted
            && let (Some(find), Some(program)) = (crate::package_finder(), &self.program)
        {
            match find(program, base, &imp.path) {
                Ok(Some(lib)) => {
                    shown = lib.display().to_string();
                    found = lib.canonicalize().ok();
                    dir = lib;
                }
                Ok(None) => {}
                Err(msg) => {
                    self.diags.push(Diagnostic::error(imp.span, msg));
                    return None;
                }
            }
        }
        let Some(canon) = found else {
            let (msg, help) = if imp.quoted {
                (format!("can't find the folder `{shown}`"), "the path is relative to the folder of this file".to_string())
            } else {
                (
                    format!("can't find module `{}`", imp.path),
                    format!(
                        "a module is a folder of .tsl files; `import {}` looks for the folder `{shown}`, or a library the project uses (`tessel add {}` adds one)",
                        imp.path, imp.path
                    ),
                )
            };
            self.error(imp.span, msg, help);
            return None;
        };
        if let Some(&m) = self.by_dir.get(&canon) {
            return Some(m);
        }
        if self.root_dirs.contains(&canon) {
            self.error(imp.span, "a program can't import its own folder", "a module is a separate folder, like a subfolder of the program");
            return None;
        }
        let name = if imp.quoted {
            canon.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            imp.path.clone()
        };
        if !is_name(&name) {
            let fixed: String = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).enumerate()
                .map(|(i, w)| if i == 0 { w.to_string() } else { w[..1].to_uppercase() + &w[1..] })
                .collect();
            self.error(
                imp.span,
                format!("the folder name `{name}` can't be used as a module name"),
                format!("module names are written in code, so the folder's name must be a name, like `{fixed}`"),
            );
            return None;
        }
        if self.modules.iter().any(|m| m.name == name) {
            self.error(
                imp.span,
                format!("two different folders are both imported as module `{name}`"),
                "module names must be unique in a program; rename one of the folders",
            );
            return None;
        }
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&canon)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "tsl")).collect())
            .unwrap_or_default();
        paths.sort();
        if paths.is_empty() {
            self.error(imp.span, format!("module `{name}` has no .tsl files"), format!("add its code to `{shown}`"));
            return None;
        }
        let m = self.modules.len();
        self.modules.push(Module { name, decls: HashMap::new() });
        self.by_dir.insert(canon, m);
        for p in paths {
            let text = match std::fs::read_to_string(&p) {
                Ok(t) => t,
                Err(e) => {
                    self.error(imp.span, format!("can't read `{}`: {e}", p.display()), "check the file's permissions");
                    continue;
                }
            };
            let file_name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let id = sources.add(dir.join(file_name).display().to_string(), text.clone());
            let parsed = crate::parser::parse_file(id, &text, self.diags, self.next_id);
            files.push((id, parsed, Some(m)));
        }
        Some(m)
    }
}

struct Resolver<'a> {
    module: Option<usize>,
    /// The modules this file imports, by name.
    imports: HashMap<String, usize>,
    modules: &'a [Module],
    /// The program's own declarations.
    root: &'a HashMap<String, Decl>,
    /// Local names (variables, parameters, fields in methods, …).
    scopes: Vec<Vec<String>>,
    /// Type parameters of the generic declarations being resolved.
    types: Vec<Vec<String>>,
    /// Inside a struct or enum: its name, and its `static` members, which
    /// its own code uses by their bare names (`zero` for `Rect.zero`).
    statics: Option<(String, Vec<String>)>,
    diags: &'a mut Diagnostics,
    sources: &'a SourceMap,
}

impl Resolver<'_> {
    fn error(&mut self, span: Span, msg: impl Into<String>) -> &mut Diagnostic {
        self.diags.push(Diagnostic::error(span, msg));
        self.diags.list.last_mut().unwrap()
    }

    fn file_name(&self, span: Span) -> String {
        let name = &self.sources.file(span.file & FILE_MASK).name;
        Path::new(name).file_name().map_or(name.clone(), |n| n.to_string_lossy().to_string())
    }

    fn is_local(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.iter().any(|l| l == n))
    }

    fn declare(&mut self, n: &str) {
        if let Some(s) = self.scopes.last_mut() {
            s.push(n.to_string());
        }
    }

    fn scoped(&mut self, names: Vec<String>, f: impl FnOnce(&mut Self)) {
        self.scopes.push(names);
        f(self);
        self.scopes.pop();
    }

    /// A private declaration used from another file.
    fn check_private(&mut self, n: &str, d: Decl, span: Span) {
        if d.private && (d.span.file & FILE_MASK) != (span.file & FILE_MASK) {
            let file = self.file_name(d.span);
            self.error(span, format!("`{n}` is private to {file}"))
                .note_at(d.span, "declared here")
                .help("remove `private` from its declaration to use it in other files");
        }
    }

    /// The full name of a global used by its bare name here, if it changes.
    fn global(&mut self, n: &str, span: Span) -> Option<String> {
        match self.module {
            Some(m) => {
                let module = &self.modules[m];
                if let Some(d) = module.decls.get(n).copied() {
                    self.check_private(n, d, span);
                    return Some(format!("{}.{n}", module.name));
                }
                if self.root.contains_key(n) {
                    let mname = module.name.clone();
                    self.error(span, format!("cannot find `{n}`"))
                        .label("not found")
                        .help(format!("`{n}` belongs to the program; module `{mname}` only sees its own declarations and the modules it imports"));
                }
                None
            }
            None => {
                if let Some(d) = self.root.get(n).copied() {
                    self.check_private(n, d, span);
                    return None;
                }
                // Tessel's own `Color` or `writeText` stays Tessel's, whatever
                // the modules imported here declare under that name.
                if tessel_names().contains(n) {
                    return None;
                }
                // A module's name used without the module: say where it is.
                let mut found: Vec<&str> = self
                    .imports
                    .iter()
                    .filter(|(_, m)| self.modules[**m].decls.get(n).is_some_and(|d| d.public))
                    .map(|(name, _)| name.as_str())
                    .collect();
                found.sort();
                let first = found.first()?.to_string();
                self.error(span, format!("cannot find `{n}`"))
                    .label("not found")
                    .help(format!("`{n}` is in module `{first}`; write `{first}.{n}`"));
                Some(format!("{first}.{n}"))
            }
        }
    }

    /// `module.name`: checks that the module is imported here and has a
    /// public `name`. Returns the full name, or None if there's no such
    /// declaration (after reporting it).
    fn qualified(&mut self, module: &str, n: &str, span: Span) -> Option<String> {
        let Some(&m) = self.imports.get(module) else {
            // Imported here under another name?
            let alias = self.imports.iter().find(|(_, i)| self.modules[**i].name == module).map(|(a, _)| a.clone());
            if let Some(alias) = alias {
                self.error(span, format!("`{module}` is imported as `{alias}` in this file"))
                    .help(format!("write `{alias}.{n}`"));
                return None;
            }
            let d = self.error(span, format!("`{module}` isn't imported in this file"));
            d.help(format!("add `import {module}` at the top of this file"));
            return None;
        };
        match self.modules[m].decls.get(n).copied() {
            Some(d) if d.public => {}
            Some(d) => {
                let file = self.file_name(d.span);
                self.error(span, format!("`{n}` isn't public in module `{module}`"))
                    .note_at(d.span, "declared here")
                    .help(format!("to use it outside the module, add `public` before its declaration in {file}"));
            }
            None => {
                let public: Vec<&str> =
                    self.modules[m].decls.iter().filter(|(_, d)| d.public).map(|(k, _)| k.as_str()).collect();
                let hint = suggest(n, public.iter().copied()).map(|s| format!("did you mean `{module}.{s}`?"));
                let d = self.error(span, format!("module `{module}` has no `{n}`"));
                d.label("not found");
                if let Some(h) = hint {
                    d.help(h);
                }
                return None;
            }
        }
        // (By the module's own name, whatever this file calls it.)
        Some(format!("{}.{n}", self.modules[m].name))
    }

    /// Is `n` a module name that isn't a local variable here?
    fn module_named(&self, n: &str) -> bool {
        !self.is_local(n) && (self.imports.contains_key(n) || self.modules.iter().any(|m| m.name == n))
    }

    fn type_name(&mut self, id: &mut Ident) {
        if self.types.iter().any(|t| t.contains(&id.name)) {
            return;
        }
        if let Some((module, n)) = id.name.split_once('.') {
            let (module, n) = (module.to_string(), n.to_string());
            // (A type that's already reported resolves quietly to an error.)
            id.name = self.qualified(&module, &n, id.span).unwrap_or_else(|| "$error".into());
        } else if let Some(full) = self.global(&id.name.clone(), id.span) {
            id.name = full;
        }
    }

    fn ty(&mut self, t: &mut TypeExpr) {
        match t {
            TypeExpr::Named(id) => self.type_name(id),
            TypeExpr::Generic(id, args, _) => {
                self.type_name(id);
                args.iter_mut().for_each(|a| self.ty(a));
            }
            TypeExpr::List(x, _) | TypeExpr::Optional(x, _) => self.ty(x),
            TypeExpr::Dict(k, v, _) => {
                self.ty(k);
                self.ty(v);
            }
            TypeExpr::Fn(ps, r, _) => {
                ps.iter_mut().for_each(|p| self.ty(p));
                if let Some(r) = r {
                    self.ty(r);
                }
            }
        }
    }

    fn generics(&mut self, g: &mut [GenericParam]) -> Vec<String> {
        for p in g.iter_mut() {
            if let Some(c) = &mut p.constraint {
                self.type_name(c);
            }
        }
        g.iter().map(|p| p.name.name.clone()).collect()
    }

    fn own_name(&mut self, id: &mut Ident) {
        if let Some(m) = self.module {
            id.name = format!("{}.{}", self.modules[m].name, id.name);
        }
    }

    fn item(&mut self, item: &mut Item) {
        match item {
            Item::Struct(d) => {
                self.own_name(&mut d.name);
                let tps = self.generics(&mut d.generics);
                self.types.push(tps);
                d.conforms.iter_mut().for_each(|c| self.type_name(c));
                for f in &mut d.fields {
                    self.ty(&mut f.ty);
                    if let Some(e) = &mut f.default {
                        self.expr(e);
                    }
                }
                let mut members: Vec<String> = d.fields.iter().map(|f| f.name.name.clone()).collect();
                members.extend(d.methods.iter().map(|m| m.name.name.clone()));
                members.push("self".into());
                self.statics = Some((d.name.name.clone(), d.statics.iter().map(|m| m.name.name.clone()).collect()));
                for m in &mut d.methods {
                    self.scoped(members.clone(), |r| r.func(m));
                }
                d.statics.iter_mut().for_each(|m| self.func(m));
                self.statics = None;
                self.types.pop();
            }
            Item::Enum(d) => {
                self.own_name(&mut d.name);
                let tps = self.generics(&mut d.generics);
                self.types.push(tps);
                d.conforms.iter_mut().for_each(|c| self.type_name(c));
                for c in &mut d.cases {
                    for p in &mut c.fields {
                        self.ty(&mut p.ty);
                        if let Some(e) = &mut p.default {
                            self.expr(e);
                        }
                    }
                }
                let mut members: Vec<String> = d.methods.iter().map(|m| m.name.name.clone()).collect();
                members.push("self".into());
                self.statics = Some((d.name.name.clone(), d.statics.iter().map(|m| m.name.name.clone()).collect()));
                for m in &mut d.methods {
                    self.scoped(members.clone(), |r| r.func(m));
                }
                d.statics.iter_mut().for_each(|m| self.func(m));
                self.statics = None;
                self.types.pop();
            }
            Item::Interface(d) => {
                self.own_name(&mut d.name);
                for m in &mut d.methods {
                    for p in &mut m.params {
                        self.ty(&mut p.ty);
                    }
                    if let Some(r) = &mut m.ret {
                        self.ty(r);
                    }
                }
                for (_, t) in &mut d.props {
                    self.ty(t);
                }
            }
            Item::Fn(f) => {
                self.own_name(&mut f.name);
                let tps = self.generics(&mut f.generics);
                self.types.push(tps);
                self.func(f);
                self.types.pop();
            }
            Item::View(v) => {
                self.own_name(&mut v.name);
                self.scoped(Vec::new(), |r| {
                    for p in &mut v.params {
                        r.param(p);
                    }
                    r.view_body(&mut v.body);
                });
            }
            Item::App(a) => {
                for arg in &mut a.args {
                    self.expr(&mut arg.value);
                }
                self.scoped(Vec::new(), |r| r.view_body(&mut a.body));
            }
        }
    }

    fn param(&mut self, p: &mut Param) {
        self.ty(&mut p.ty);
        if let Some(e) = &mut p.default {
            self.expr(e);
        }
        self.declare(&p.name.name.clone());
    }

    fn func(&mut self, f: &mut FnDecl) {
        self.scoped(Vec::new(), |r| {
            for p in &mut f.params {
                r.param(p);
            }
            if let Some(t) = &mut f.ret {
                r.ty(t);
            }
            r.block(&mut f.body);
        });
    }

    /// A view's (or app's) body: its `state` and functions are visible throughout.
    fn view_body(&mut self, b: &mut Block) {
        let mut names = Vec::new();
        for s in &b.stmts {
            match s {
                Stmt::State(d) => names.push(d.name.name.clone()),
                Stmt::Fn(f) => names.push(f.name.name.clone()),
                _ => {}
            }
        }
        self.scoped(names, |r| {
            for s in &mut b.stmts {
                r.stmt(s);
            }
        });
    }

    fn block(&mut self, b: &mut Block) {
        self.scoped(Vec::new(), |r| {
            for s in &mut b.stmts {
                r.stmt(s);
            }
        });
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => {
                if let Some(t) = &mut d.ty {
                    self.ty(t);
                }
                self.expr(&mut d.value);
                self.declare(&d.name.name.clone());
            }
            Stmt::Fn(f) => {
                self.declare(&f.name.name.clone());
                self.func(f);
            }
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Stmt::For { var, iter, body, .. } => {
                self.expr(iter);
                let name = var.name.clone();
                self.scoped(vec![name], |r| r.block(body));
            }
            Stmt::While { cond, body, .. } => {
                self.scoped(Vec::new(), |r| {
                    r.cond(cond);
                    r.block(body);
                });
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn cond(&mut self, c: &mut Cond) {
        match c {
            Cond::Expr(e) => self.expr(e),
            Cond::Let(n, t, e) => {
                if let Some(t) = t {
                    self.ty(t);
                }
                self.expr(e);
                self.declare(&n.name.clone());
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Nil
            | ExprKind::DotName(_)
            | ExprKind::Continuation
            | ExprKind::Error => {}
            ExprKind::Name(n) => {
                if !self.is_local(n) {
                    // A `static` member of the type this code is in.
                    if let Some((owner, _)) = self.statics.as_ref().filter(|(_, names)| names.contains(n)) {
                        *n = static_name(owner, n);
                        return;
                    }
                    if let Some(full) = self.global(&n.clone(), e.span) {
                        *n = full;
                    } else if self.module_named(n) {
                        let n = n.clone();
                        self.error(e.span, format!("`{n}` is a module; use one of its declarations, like `{n}.something`"));
                        e.kind = ExprKind::Error;
                    }
                }
            }
            ExprKind::GenericName(n, args) => {
                let mut id = Ident { name: n.clone(), span: e.span };
                self.type_name(&mut id);
                args.iter_mut().for_each(|a| self.ty(a));
                if id.name == "$error" {
                    e.kind = ExprKind::Error;
                } else {
                    *n = id.name;
                }
            }
            // `geometry.Circle`: a declaration of an imported module.
            ExprKind::Member { object, name, optional: false }
                if matches!(&object.kind, ExprKind::Name(m) if self.module_named(m)) =>
            {
                let ExprKind::Name(m) = &object.kind else { unreachable!() };
                // `geometry.|` in the editor: the checker suggests the module's names.
                if name.name == crate::check::COMPLETION_MARK {
                    let real = self.imports.get(m).map_or(m.clone(), |&i| self.modules[i].name.clone());
                    e.kind = ExprKind::Name(format!("{real}.{}", name.name));
                    return;
                }
                e.kind = match self.qualified(&m.clone(), &name.name.clone(), name.span) {
                    Some(full) => ExprKind::Name(full),
                    None => ExprKind::Error,
                };
            }
            ExprKind::Member { object, .. } => self.expr(object),
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
            ExprKind::Cast { value, ty, .. } => {
                self.expr(value);
                self.ty(ty);
            }
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
            ExprKind::Closure { params, body } => {
                let names = params.iter().map(|p| p.name.clone()).collect();
                self.scoped(names, |r| r.block(body));
            }
            ExprKind::Block(body) => self.block(body),
            ExprKind::If { cond, then, otherwise } => {
                self.scoped(Vec::new(), |r| {
                    r.cond(cond);
                    r.block(then);
                });
                match otherwise {
                    Some(Else::Block(b)) => self.block(b),
                    Some(Else::If(x)) => self.expr(x),
                    None => {}
                }
            }
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    let mut names = Vec::new();
                    for pattern in &arm.patterns {
                        if let Pattern::Case { bindings, .. } = pattern {
                            names.extend(bindings.iter().map(|b| b.name.clone()));
                        }
                    }
                    self.scoped(names, |r| {
                        for pattern in &mut arm.patterns {
                            if let Pattern::Literal(l) = pattern {
                                r.expr(l);
                            }
                        }
                        r.expr(&mut arm.body);
                    });
                }
            }
        }
    }
}
