//! The Tessel compiler: source text in, native object code out.
//!
//! Pipeline (see docs/design.md): lexer -> parser -> type checker -> LLVM IR.

pub mod ast;
pub mod builtins;
pub mod check;
pub mod codegen;
pub mod design;
pub mod diag;
pub mod generics;
pub mod jit;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod source;
pub mod types;

use diag::Diagnostics;
use source::SourceMap;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The result of running the front end over a program.
pub struct Analysis {
    pub program: ast::Program,
    pub checked: check::Checked,
    pub diags: Diagnostics,
}

/// Parses every file in `sources` (and the modules they import, which are
/// added to it) and type-checks them together as one program.
/// `require_entry` reports an error if there is no `app` or `fn main()`.
pub fn analyze(sources: &mut SourceMap, require_entry: bool) -> Analysis {
    let mut diags = Diagnostics::default();
    let mut next_id = 0;
    let mut root = Vec::new();
    for (id, file) in sources.files() {
        root.push((id, parser::parse_file(id, &file.text, &mut diags, &mut next_id)));
    }
    // (Adds the files of imported modules to `sources`.)
    let mut program = modules::resolve(sources, root, &mut diags, &mut next_id);
    program.items.extend(tuple_structs());
    add_prelude(&mut program, &mut diags, &mut next_id);
    program.next_id = next_id;
    let checked = check::check_program(&program, &mut diags, require_entry);
    diags.list.sort_by_key(|d| (d.span().file, d.span().lo));
    Analysis { program, checked, diags }
}

/// The function a REPL entry's statements are checked and compiled as.
pub const REPL_FN: &str = "__repl";

/// A variable of a REPL session: declared by an earlier entry, kept in a
/// global of the code loaded then.
#[derive(Clone, Debug)]
pub struct ReplVar {
    pub name: String,
    /// Its type, as written in Tessel.
    pub ty: String,
    /// Declared with `var`.
    pub mutable: bool,
    /// The name of its global.
    pub symbol: String,
}

/// The front end's result for a REPL entry.
pub struct ReplAnalysis {
    pub analysis: Analysis,
    /// The entry's declarations and imports, which stay for later entries.
    pub decls: Vec<parser::ReplDecl>,
    /// The entry's last statement, if it's an expression (its value is shown).
    pub last_expr: Option<source::Span>,
}

/// What to do with the value of a REPL entry's last expression.
#[derive(Clone, Copy, PartialEq)]
pub enum ReplShow {
    /// Nothing (it's computed, and dropped).
    No,
    /// Print it, as `print` does.
    Print,
    /// Print it as JSON (for what `print` can't show: structs, optionals).
    Json,
}

/// `name(arg)`, where `arg` is in the source (so places in it stay right).
fn call(name: &str, arg: ast::Expr, next_id: &mut ast::ExprId) -> ast::Expr {
    let span = arg.span;
    let mut id = || {
        *next_id += 1;
        *next_id - 1
    };
    let callee = ast::Expr { id: id(), kind: ast::ExprKind::Name(name.into()), span };
    let args = vec![ast::Arg { label: None, value: arg }];
    ast::Expr { id: id(), kind: ast::ExprKind::Call { callee: Box::new(callee), args, trailing: None }, span }
}

/// Like `analyze`, for a REPL session: file `entry` of `sources` is what was
/// just typed (declarations and statements), and the other files are the
/// declarations typed before. `vars` are the session's variables, which the
/// entry's statements can use. `show` says what becomes of the value of a
/// last expression.
pub fn analyze_repl(sources: &mut SourceMap, entry: u32, vars: &[ReplVar], show: ReplShow) -> ReplAnalysis {
    let mut diags = Diagnostics::default();
    let mut next_id = 0;
    let mut root = Vec::new();
    let mut found = None;
    for (id, file) in sources.files() {
        if id == entry {
            let mut e = parser::parse_repl(id, &file.text, &mut diags, &mut next_id);
            let span = source::Span::new(id, 0, file.text.len());
            found = Some((std::mem::take(&mut e.stmts), std::mem::take(&mut e.decls), span));
            root.push((id, e.program));
        } else {
            root.push((id, parser::parse_file(id, &file.text, &mut diags, &mut next_id)));
        }
    }
    let (mut stmts, decls, span) = found.expect("the entry is one of the files");
    let last_expr = match stmts.last() {
        Some(ast::Stmt::Expr(e)) => Some(e.span),
        _ => None,
    };
    if show != ReplShow::No && last_expr.is_some() {
        if let Some(ast::Stmt::Expr(e)) = stmts.pop() {
            let shown = if show == ReplShow::Json { call("toJson", e, &mut next_id) } else { e };
            stmts.push(ast::Stmt::Expr(call("print", shown, &mut next_id)));
        }
    }
    // The statements become the body of a function whose parameters are the
    // session's variables (so they have their types, and their names resolve).
    let params: Vec<String> = vars.iter().map(|v| format!("{}: {}", v.name, v.ty)).collect();
    let header = sources.add("session", format!("fn {REPL_FN}({}) {{\n}}\n", params.join(", ")));
    let mut wrapper = parser::parse_file(header, &sources.file(header).text.clone(), &mut diags, &mut next_id);
    if let Some(ast::Item::Fn(f)) = wrapper.items.first_mut() {
        f.body = ast::Block { stmts, span };
    }
    root.push((header, wrapper));
    // An import is for one file, but a session is one conversation: what
    // any entry imported, every entry (and the statements above) can use.
    let mut imports: Vec<ast::Import> = Vec::new();
    for (_, program) in &root {
        for import in &program.imports {
            if !imports.iter().any(|i| i.path == import.path && i.quoted == import.quoted) {
                imports.push(import.clone());
            }
        }
    }
    for (_, program) in &mut root {
        program.imports = imports.clone();
    }

    let mut program = modules::resolve(sources, root, &mut diags, &mut next_id);
    program.items.extend(tuple_structs());
    add_prelude(&mut program, &mut diags, &mut next_id);
    program.next_id = next_id;
    let mutable = vars.iter().filter(|v| v.mutable).map(|v| v.name.clone()).collect();
    let checked = check::check_repl(&program, &mut diags, mutable);
    // ("Declared here" would point at the function made up above.)
    for d in &mut diags.list {
        d.secondary.retain(|label| label.span.file != header);
    }
    diags.list.sort_by_key(|d| (d.span().file, d.span().lo));
    // (A mistake in an import would be reported for every file it's now in.)
    diags.list.dedup_by(|a, b| a.span() == b.span() && a.message == b.message);
    ReplAnalysis { analysis: Analysis { program, checked, diags }, decls, last_expr }
}

/// Adds the prelude's declarations (see `prelude.tsl`) that the program
/// doesn't declare itself. A program with its own `Error` or `Result` gets
/// none of them, since they all use those two.
fn add_prelude(program: &mut ast::Program, diags: &mut Diagnostics, next_id: &mut ast::ExprId) {
    let own: std::collections::HashSet<String> = program.items.iter().map(|i| i.name().name.clone()).collect();
    if own.contains("Error") || own.contains("Result") {
        return;
    }
    let prelude = parser::parse_file(source::PRELUDE_FILE, source::PRELUDE, diags, next_id);
    program.items.extend(prelude.items.into_iter().filter(|i| !own.contains(&i.name().name)));
}

/// The generic structs behind tuples: `(,)<A, B>` with fields `0` and `1`
/// for pairs, and so on up to `ast::MAX_TUPLE` items.
fn tuple_structs() -> Vec<ast::Item> {
    let span = source::Span { file: source::PRELUDE_FILE, lo: 0, hi: 0 };
    let ident = |name: String| ast::Ident { name, span };
    (2..=ast::MAX_TUPLE)
        .map(|n| {
            let params: Vec<String> = (0..n).map(|i| ((b'A' + i as u8) as char).to_string()).collect();
            ast::Item::Struct(ast::StructDecl {
                name: ident(ast::tuple_name(n)),
                generics: params.iter().map(|p| ast::GenericParam { name: ident(p.clone()), constraint: None }).collect(),
                conforms: Vec::new(),
                fields: params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ast::FieldDecl { name: ident(i.to_string()), ty: ast::TypeExpr::Named(ident(p.clone())), default: None })
                    .collect(),
                methods: Vec::new(),
            })
        })
        .collect()
}

/// Completion suggestions for the caret at byte `offset` in file `file`.
pub struct Completion {
    /// Where the word being completed starts (the suggestion replaces the
    /// text from here to the caret).
    pub start: usize,
    pub items: Vec<check::Suggestion>,
}

/// What could go at `offset` in `file`: the members after `x.`, the enum
/// cases after `.`, and the names in scope elsewhere. The word around the
/// caret is replaced by a marker the type checker recognizes. Nothing is
/// suggested inside strings and comments, or after a number.
pub fn complete(sources: &SourceMap, file: u32, offset: usize) -> Completion {
    let text = &sources.file(file).text;
    let offset = offset.min(text.len());
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..offset].char_indices().rev().take_while(|(_, c)| word(*c)).last().map_or(offset, |(i, _)| i);
    let end = offset + text[offset..].chars().take_while(|c| word(*c)).map(char::len_utf8).sum::<usize>();
    let none = Completion { start, items: Vec::new() };
    if text[start..].starts_with(|c: char| c.is_ascii_digit()) || in_string_or_comment(&text[..start]) {
        return none;
    }
    let with = |replacement: &str| {
        let mut map = SourceMap::new();
        for (id, f) in sources.files() {
            if id == file {
                map.add(f.name.clone(), replacement.to_string());
            } else {
                map.add(f.name.clone(), f.text.clone());
            }
        }
        analyze(&mut map, false).checked.completion
    };
    let marked = format!("{}{}{}", &text[..start], check::COMPLETION_MARK, &text[end..]);
    if let Some(items) = with(&marked) {
        return Completion { start, items };
    }
    // The line is half written (`Text(na`): close what it opened, keeping
    // the rest of the line, or else ending the line right there.
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let mut closers = String::new();
    for c in text[line_start..start].chars() {
        match c {
            '(' => closers.insert(0, ')'),
            '[' => closers.insert(0, ']'),
            ')' | ']' if !closers.is_empty() => {
                closers.remove(0);
            }
            _ => {}
        }
    }
    if closers.is_empty() {
        return none;
    }
    let marked = format!("{}{}{closers}{}", &text[..start], check::COMPLETION_MARK, &text[end..]);
    if let Some(items) = with(&marked) {
        return Completion { start, items };
    }
    let marked = format!("{}{}{closers}{}", &text[..start], check::COMPLETION_MARK, &text[line_end..]);
    Completion { start, items: with(&marked).unwrap_or_default() }
}

/// Whether the end of `before` is inside a string (outside an interpolation) or a comment.
fn in_string_or_comment(before: &str) -> bool {
    let line = before.rsplit('\n').next().unwrap_or("");
    // Strings: track quotes and `{…}` interpolations, which are code again.
    let mut in_str = false;
    let mut depth = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_str => {
                chars.next();
            }
            '"' => in_str = !in_str,
            '{' if in_str => {
                depth.push(true);
                in_str = false;
            }
            '{' => depth.push(false),
            '}' => {
                if depth.pop() == Some(true) {
                    in_str = true;
                }
            }
            '/' if !in_str && chars.peek() == Some(&'/') => return true,
            _ => {}
        }
    }
    in_str
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(src: &str) -> (usize, Vec<String>) {
        let offset = src.find('|').unwrap();
        let text = src.replace('|', "");
        let mut map = SourceMap::new();
        let f = map.add("main.tsl", text);
        let c = complete(&map, f, offset);
        (c.start, c.items.into_iter().map(|i| i.label).collect())
    }

    #[test]
    fn members_names_and_cases() {
        let src = "struct Doc {\n    path: String\n    line: Int = 0\n}\nenum Shape {\n    circle\n    square\n}\nfn main() {\n    let doc = Doc(path: \"a\")\n    let shape = Shape.circle\n    print(doc.|)\n}\n";
        let (_, l) = labels(src);
        assert_eq!(l, ["path", "line"]);
        let (_, l) = labels(&src.replace("print(doc.|)", "print(doc.pa|th)"));
        assert_eq!(l, ["path", "line"]);
        let (start, l) = labels(&src.replace("print(doc.|)", "print(sh|)"));
        assert!(l.contains(&"shape".to_string()) && l.contains(&"doc".to_string()) && l.contains(&"Text".to_string()), "{l:?}");
        assert_eq!(start, src.find("print(doc.").unwrap() + 6);
        let (_, l) = labels(&src.replace("print(doc.|)", "let s: Shape = .|"));
        assert_eq!(l, ["circle", "square"]);
        let (_, l) = labels(&src.replace("print(doc.|)", "print(Shape.|)"));
        assert_eq!(l, ["circle", "square"]);
        let (_, l) = labels(&src.replace("print(doc.|)", "print(doc.path.upp|)"));
        assert!(l.contains(&"uppercase".to_string()), "{l:?}");
        // Half-written lines still work.
        let (_, l) = labels(&src.replace("print(doc.|)", "print(doc.|"));
        assert_eq!(l, ["path", "line"]);
        let (_, l) = labels(&src.replace("print(doc.|)\n}", "print(doc.|}"));
        assert_eq!(l, ["path", "line"]);
        // Not in strings or comments.
        assert!(labels(&src.replace("print(doc.|)", "print(\"doc.|\")")).1.is_empty());
        assert!(!labels(&src.replace("print(doc.|)", "print(\"{doc.|}\")")).1.is_empty());
        assert!(labels(&src.replace("print(doc.|)", "// doc.|")).1.is_empty());
    }

    #[test]
    fn modifiers_and_views() {
        let src = "app A {\n    state count = 0\n    VStack {\n        Text(\"a\")\n            .|\n    }\n}\n";
        let (_, l) = labels(src);
        assert!(l.contains(&"padding".to_string()) && l.contains(&"font".to_string()), "{l:?}");
        let (_, l) = labels(&src.replace(".|", "Te|"));
        assert!(l.contains(&"Text".to_string()) && l.contains(&"count".to_string()), "{l:?}");
        let (_, l) = labels(&src.replace(".|", ".color(.|)"));
        assert!(l.contains(&"blue".to_string()), "{l:?}");
    }
}
