//! `tessel lsp`: a language server for editors (Language Server Protocol).
//!
//! It reports the same errors as `tessel check` while you type, shows types
//! on hover, suggests completions (the same as the Tessel IDE's), goes to a
//! name's declaration, finds its uses, and renames it. A program is
//! every `.tsl` file in a folder, so on each change the whole folder is
//! checked, with open (possibly unsaved) files taking the place of the saved
//! versions.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_server::{Connection, Message, Notification};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, PublishDiagnosticsParams, Range,
    Uri,
};
use tessel_compiler::diag::Diagnostic as TslDiagnostic;
use tessel_compiler::source::{SourceMap, Span, FILE_MASK, PRELUDE_FILE};

pub fn run() -> Result<(), String> {
    let (conn, io) = Connection::stdio();
    let caps = serde_json::json!({
        // 1 = the client sends the full text on every change.
        "textDocumentSync": { "openClose": true, "change": 1, "save": true },
        "hoverProvider": true,
        "completionProvider": { "triggerCharacters": ["."] },
        "definitionProvider": true,
        "referencesProvider": true,
        "documentHighlightProvider": true,
        "renameProvider": { "prepareProvider": true },
    });
    conn.initialize(caps).map_err(|e| e.to_string())?;
    let mut server = Server { open: HashMap::new(), published: HashMap::new() };
    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req).map_err(|e| e.to_string())? {
                    break;
                }
                let result = match req.method.as_str() {
                    "textDocument/hover" => serde_json::from_value::<lsp_types::HoverParams>(req.params)
                        .ok()
                        .and_then(|p| server.hover(&p))
                        .map_or(serde_json::Value::Null, |h| serde_json::to_value(h).unwrap()),
                    "textDocument/completion" => serde_json::from_value::<lsp_types::CompletionParams>(req.params)
                        .ok()
                        .and_then(|p| server.completion(&p))
                        .map_or(serde_json::Value::Null, |c| serde_json::to_value(c).unwrap()),
                    "textDocument/definition" | "textDocument/references" | "textDocument/documentHighlight"
                    | "textDocument/prepareRename" | "textDocument/rename" => match server.navigate(&req.method, &req.params) {
                        Ok(found) => found,
                        Err(why) => {
                            // (-32803: the request failed, as the protocol numbers it.)
                            let failed = lsp_server::Response::new_err(req.id, -32803, why);
                            let _ = conn.sender.send(Message::Response(failed));
                            continue;
                        }
                    },
                    _ => serde_json::Value::Null,
                };
                let _ = conn.sender.send(Message::Response(lsp_server::Response::new_ok(req.id, result)));
            }
            Message::Notification(n) => server.notification(&conn, n),
            Message::Response(_) => {}
        }
    }
    drop(conn);
    io.join().map_err(|e| e.to_string())
}

struct Server {
    /// Open documents: their current text, by path.
    open: HashMap<PathBuf, String>,
    /// Files we've published diagnostics for, by folder, so fixed errors get cleared.
    published: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Server {
    fn notification(&mut self, conn: &Connection, n: Notification) {
        let path = match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(n.params) else { return };
                let Some(path) = uri_to_path(&p.text_document.uri) else { return };
                self.open.insert(path.clone(), p.text_document.text);
                path
            }
            DidChangeTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(n.params) else { return };
                let Some(path) = uri_to_path(&p.text_document.uri) else { return };
                if let Some(change) = p.content_changes.into_iter().last() {
                    self.open.insert(path.clone(), change.text);
                }
                path
            }
            DidSaveTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidSaveTextDocumentParams>(n.params) else { return };
                let Some(path) = uri_to_path(&p.text_document.uri) else { return };
                path
            }
            DidCloseTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(n.params) else { return };
                let Some(path) = uri_to_path(&p.text_document.uri) else { return };
                self.open.remove(&path);
                path
            }
            _ => return,
        };
        self.check_folder(conn, &path);
    }

    /// The type of the expression under the cursor.
    fn hover(&self, p: &lsp_types::HoverParams) -> Option<lsp_types::Hover> {
        let pos = &p.text_document_position_params;
        let path = uri_to_path(&pos.text_document.uri)?;
        let (mut sources, files) = self.program(&path);
        let analysis = tessel_compiler::analyze(&mut sources, false);
        let file = files.iter().position(|f| *f == path)? as u32;
        let offset = offset_of(&sources, file, pos.position)?;
        // The innermost expression with a meaningful type (a method name or a
        // case name has none of its own; its call or member expression does).
        use tessel_compiler::types::Ty;
        let (e, t) = analysis.program.exprs_at(file, offset).into_iter().find_map(|e| {
            let t = analysis.checked.types.get(&e.id)?;
            (!matches!(t, Ty::Error | Ty::Void)).then_some((e, t))
        })?;
        let snippet = sources.snippet(e.span).lines().next().unwrap_or("").to_string();
        let shown = if snippet.chars().count() > 40 { String::new() } else { format!("{snippet}: ") };
        Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: format!("```tessel\n{shown}{}\n```", analysis.checked.show(t)),
            }),
            range: Some(range(&sources, e.span)),
        })
    }

    /// Go to definition, find references, highlight, and rename: all about
    /// the name under the cursor, where it's declared and where it's used
    /// (see the compiler's `navigate`). An error is why a name can't be
    /// renamed.
    fn navigate(&self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        use serde_json::{json, Value};
        let none = Ok(Value::Null);
        let Some(uri) = params["textDocument"]["uri"].as_str().and_then(|u| Uri::from_str(u).ok()) else { return none };
        let Ok(at) = serde_json::from_value::<Position>(params["position"].clone()) else { return none };
        let Some(path) = uri_to_path(&uri) else { return none };
        let (mut sources, files) = self.program(&path);
        let analysis = tessel_compiler::analyze(&mut sources, false);
        let Some(file) = files.iter().position(|f| *f == path) else { return none };
        let Some(offset) = offset_of(&sources, file as u32, at) else { return none };
        let Some(symbol) = tessel_compiler::navigate::symbol_at(&analysis, &sources, file as u32, offset) else {
            return match method {
                "textDocument/prepareRename" | "textDocument/rename" => Err("This can't be renamed: it isn't a name the program declares.".into()),
                _ => none,
            };
        };
        let place = |span: Span| {
            let uri = path_to_uri(Path::new(&sources.file(span.file).name))?;
            Some(json!({ "uri": uri, "range": range(&sources, span) }))
        };
        let renamed = || match symbol.fixed {
            Some(why) => Err(format!("`{}` can't be renamed: {why}.", symbol.name)),
            None => Ok(()),
        };
        Ok(match method {
            "textDocument/definition" => place(symbol.declared).unwrap_or(Value::Null),
            "textDocument/references" => {
                let declaration = params["context"]["includeDeclaration"].as_bool().unwrap_or(true);
                let places = symbol.places.iter().filter(|s| declaration || **s != symbol.declared);
                json!(places.filter_map(|s| place(*s)).collect::<Vec<_>>())
            }
            // (In this file: what an editor marks as the cursor rests on a name.)
            "textDocument/documentHighlight" => {
                let here = symbol.places.iter().filter(|s| s.file & FILE_MASK == file as u32);
                json!(here.map(|s| json!({ "range": range(&sources, *s) })).collect::<Vec<_>>())
            }
            "textDocument/prepareRename" => {
                renamed()?;
                let on = symbol.places.iter().find(|s| s.file & FILE_MASK == file as u32 && s.lo <= offset && offset <= s.hi);
                json!({ "range": range(&sources, *on.unwrap_or(&symbol.declared)), "placeholder": symbol.name })
            }
            _ => {
                renamed()?;
                let to = params["newName"].as_str().unwrap_or_default();
                if !tessel_compiler::navigate::is_name(to) {
                    return Err(format!("`{to}` can't be a name: names are letters, digits and `_`, don't start with a digit, and aren't keywords."));
                }
                let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
                for span in &symbol.places {
                    let Some(uri) = path_to_uri(Path::new(&sources.file(span.file).name)) else { continue };
                    changes.entry(uri.as_str().to_string()).or_default().push(json!({ "range": range(&sources, *span), "newText": to }));
                }
                json!({ "changes": changes })
            }
        })
    }

    /// What could go at the cursor: the same suggestions as the IDE's editor.
    fn completion(&self, p: &lsp_types::CompletionParams) -> Option<lsp_types::CompletionList> {
        use lsp_types::{CompletionItem, CompletionItemKind as K, CompletionTextEdit, TextEdit};
        use tessel_compiler::check::SuggestionKind as S;
        let pos = &p.text_document_position;
        let path = uri_to_path(&pos.text_document.uri)?;
        let (sources, files) = self.program(&path);
        let file = files.iter().position(|f| *f == path)? as u32;
        let offset = offset_of(&sources, file, pos.position)?;
        let c = tessel_compiler::complete(&sources, file, offset as usize);
        // The suggestion replaces the word typed so far.
        let replace = Range { start: position(&sources, file, c.start), end: pos.position };
        let items = c
            .items
            .into_iter()
            .enumerate()
            .map(|(i, s)| CompletionItem {
                kind: Some(match s.kind {
                    S::Variable => K::VARIABLE,
                    S::Field => K::FIELD,
                    S::Method | S::Modifier => K::METHOD,
                    S::Function => K::FUNCTION,
                    S::View => K::CLASS,
                    S::Type => K::STRUCT,
                    S::Case => K::ENUM_MEMBER,
                    S::Keyword => K::KEYWORD,
                }),
                detail: (!s.detail.is_empty()).then(|| s.detail.clone()),
                // Keep the checker's order (closest names first).
                sort_text: Some(format!("{i:05}")),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit { range: replace, new_text: s.label.clone() })),
                label: s.label,
                ..Default::default()
            })
            .collect();
        Some(lsp_types::CompletionList { is_incomplete: false, items })
    }

    /// Every file of the program `file` belongs to (its folder), with open files' current text.
    fn program(&self, file: &Path) -> (SourceMap, Vec<PathBuf>) {
        let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_tsl(p)).collect())
            .unwrap_or_default();
        for open in self.open.keys() {
            if open.parent() == Some(folder.as_path()) && !files.contains(open) {
                files.push(open.clone());
            }
        }
        if !files.contains(&file.to_path_buf()) && is_tsl(file) {
            files.push(file.to_path_buf());
        }
        files.sort();
        let mut sources = SourceMap::new();
        for f in &files {
            let text = match self.open.get(f) {
                Some(t) => t.clone(),
                None => std::fs::read_to_string(f).unwrap_or_default(),
            };
            sources.add(f.display().to_string(), text);
        }
        (sources, files)
    }

    /// Checks the program `file` belongs to and publishes diagnostics for each of its files.
    fn check_folder(&mut self, conn: &Connection, file: &Path) {
        let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let (mut sources, files) = self.program(file);
        // Don't insist on an `app` or `main`: a file may be part of a larger program.
        let analysis = tessel_compiler::analyze(&mut sources, false);

        let mut by_file: BTreeMap<PathBuf, Vec<Diagnostic>> = files.iter().map(|f| (f.clone(), Vec::new())).collect();
        for d in &analysis.diags.list {
            // (Errors in the prelude aren't shown in any file.)
            if d.span().file & FILE_MASK == PRELUDE_FILE {
                continue;
            }
            let path = PathBuf::from(&sources.file(d.span().file).name);
            by_file.entry(path).or_default().push(to_lsp(d, &sources));
        }
        // Clear files that had errors before but aren't in the program any more.
        let previous = self.published.insert(folder, by_file.keys().cloned().collect()).unwrap_or_default();
        for p in previous {
            by_file.entry(p).or_default();
        }
        for (path, diagnostics) in by_file {
            let Some(uri) = path_to_uri(&path) else { continue };
            let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
            let n = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
            let _ = conn.sender.send(Message::Notification(n));
        }
    }
}

fn is_tsl(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "tsl")
}

fn to_lsp(d: &TslDiagnostic, sources: &SourceMap) -> Diagnostic {
    let mut message = d.message.clone();
    if !d.primary.text.is_empty() {
        message += &format!(" ({})", d.primary.text);
    }
    if let Some(help) = &d.help {
        message += &format!("\nhelp: {help}");
    }
    let related = d
        .secondary
        .iter()
        .filter_map(|l| {
            let uri = path_to_uri(Path::new(&sources.file(l.span.file).name))?;
            Some(DiagnosticRelatedInformation {
                location: Location { uri, range: range(sources, l.span) },
                message: l.text.clone(),
            })
        })
        .collect::<Vec<_>>();
    Diagnostic {
        range: range(sources, d.span()),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("tessel".into()),
        message,
        related_information: (!related.is_empty()).then_some(related),
        ..Default::default()
    }
}

/// Byte offset of an LSP position (UTF-16 based) in `file`.
fn offset_of(sources: &SourceMap, file: u32, pos: Position) -> Option<u32> {
    let text = &sources.file(file).text;
    let line_start = if pos.line == 0 {
        0
    } else {
        text.match_indices('\n').nth(pos.line as usize - 1).map(|(i, _)| i + 1)?
    };
    let mut units = 0;
    for (i, c) in text[line_start..].char_indices() {
        if units >= pos.character as usize || c == '\n' {
            return Some((line_start + i) as u32);
        }
        units += c.len_utf16();
    }
    Some(text.len() as u32)
}

/// LSP positions count UTF-16 code units within a line.
fn position(sources: &SourceMap, file: u32, offset: usize) -> Position {
    let f = sources.file(file);
    let offset = offset.min(f.text.len());
    let line = f.line_index(offset);
    let line_start = offset - f.text[..offset].rfind('\n').map_or(offset, |i| offset - i - 1);
    let character = f.text[line_start..offset].encode_utf16().count();
    Position { line: line as u32, character: character as u32 }
}

fn range(sources: &SourceMap, span: Span) -> Range {
    let start = position(sources, span.file, span.lo as usize);
    let mut end = position(sources, span.file, span.hi as usize);
    if end == start {
        end.character += 1;
    }
    Range { start, end }
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str().strip_prefix("file://")?;
    let decoded = percent_decode(s);
    // Windows: file:///C:/path → C:/path
    let decoded = match decoded.strip_prefix('/') {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => decoded,
    };
    Some(PathBuf::from(decoded))
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    let encoded: String = s
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Uri::from_str(&format!("file://{encoded}")).ok()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip() {
        let p = PathBuf::from("/Users/me/My Apps/todo list.tsl");
        let uri = path_to_uri(&p).unwrap();
        assert_eq!(uri.as_str(), "file:///Users/me/My%20Apps/todo%20list.tsl");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
    }

    #[test]
    fn completes_members_of_unsaved_text() {
        let dir = std::env::temp_dir().join(format!("tessel-lsp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.tsl");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let text = "struct Doc {\n    path: String\n    line: Int = 0\n}\nfn main() {\n    let doc = Doc(path: \"a\")\n    print(doc.pa)\n}\n";
        let server = Server { open: HashMap::from([(path.clone(), text.to_string())]), published: HashMap::new() };
        let uri = path_to_uri(&path).unwrap();
        let params = lsp_types::CompletionParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri },
                position: Position { line: 6, character: 16 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        let list = server.completion(&params).unwrap();
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["path", "line"]);
        let Some(lsp_types::CompletionTextEdit::Edit(edit)) = &list.items[0].text_edit else { panic!() };
        assert_eq!(edit.range.start, Position { line: 6, character: 14 });
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A program that uses its names in every way there is, for the
    /// tests of going to them and renaming them.
    const PROGRAM: &str = r#"interface Shape {
    fn area() -> Float
}

struct Item: Shape {
    label: String
    price: Float = 1.0

    doubled: Float { price * 2.0 }

    fn area() -> Float { price }

    fn costlier(than: Item) -> Bool {
        bump(by: 0.0) > than.price
    }

    fn bump(by: Float) -> Float { price + by }

    static fn free(label: String) -> Item { Item(label: label, price: 0.0) }
}

enum Kind {
    plain
    boxed(size: Int)

    fn roomy() -> Bool {
        match self {
            .plain -> false
            .boxed(size) -> size > 2
        }
    }
}

struct Stack<T> {
    items: [T]
}

let limit = 3

fn total(items: [Item], extra: Float = 0.0) -> Float {
    var sum = extra
    for item in items {
        sum += item.price + item.doubled
    }
    fn show(amount: Float) -> String { "sum {amount}, limit {limit}" }
    print(show(amount: sum))
    let prices = items.map { each in each.price }
    return sum + Float(prices.count)
}

fn largest<T: Shape>(of: [T]) -> Float {
    var most = 0.0
    for shape in of {
        most = max(most, shape.area())
    }
    return most
}

fn main() {
    let first = Item(label: "tea", price: 2.5)
    let free = Item.free(label: "air")
    let kind: Kind = .boxed(size: 3)
    let other = Kind.plain
    let stack = Stack<Item>(items: [first, free])
    let shapes: [Shape] = [first]
    if first.costlier(than: free) && kind.roomy() && other == .plain {
        print(total(items: stack.items, extra: 1.0) + largest(of: stack.items) + Float(shapes.count))
    }
}
"#;

    /// A server with `PROGRAM` open as an unsaved file, and the file's address.
    fn with_program(name: &str) -> (Server, Uri, PathBuf) {
        with_text(name, PROGRAM)
    }

    fn with_text(name: &str, text: &str) -> (Server, Uri, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tessel-lsp-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.tsl");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let server = Server { open: HashMap::from([(path.clone(), text.to_string())]), published: HashMap::new() };
        (server, path_to_uri(&path).unwrap(), dir)
    }

    /// Where the `nth` (from 0) `word` of `PROGRAM` is, with the cursor in it.
    fn cursor(word: &str, nth: usize) -> Position {
        cursor_in(PROGRAM, word, nth)
    }

    fn cursor_in(text: &str, word: &str, nth: usize) -> Position {
        let at = text.match_indices(word).nth(nth).unwrap_or_else(|| panic!("no {word} #{nth}")).0 + 1;
        let line = text[..at].matches('\n').count();
        let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        Position { line: line as u32, character: (at - start) as u32 }
    }

    /// Renames each of `names` in `text` (a name is given by a place it's
    /// written, and how many places a rename must change): the program has
    /// no errors before, and none after each.
    fn check_renames(test: &str, text: &str, names: &[(&str, usize, usize)]) {
        let (server, uri, dir) = with_text(test, text);
        let errors = |text: &str| -> Vec<String> {
            let mut sources = SourceMap::new();
            sources.add("main.tsl", text);
            let analysis = tessel_compiler::analyze(&mut sources, true);
            analysis.diags.list.iter().map(|d| tessel_compiler::diag::render(d, &sources, false)).collect()
        };
        assert!(errors(text).is_empty(), "{}", errors(text).join("\n"));
        let mut wrong = Vec::new();
        for &(word, nth, places) in names {
            // (A type's name starts with a capital, as `Stack<Int>` has to.)
            let new = if word.starts_with(char::is_uppercase) { "RenamedIt" } else { "renamedIt" };
            let params = serde_json::json!({ "textDocument": { "uri": uri }, "position": cursor_in(text, word, nth), "newName": new });
            let prepared = server.navigate("textDocument/prepareRename", &params).unwrap_or_else(|e| panic!("{word}: {e}"));
            let old = prepared["placeholder"].as_str().unwrap().to_string();
            let edit = server.navigate("textDocument/rename", &params).unwrap();
            let count = edit["changes"].as_object().unwrap().values().next().unwrap().as_array().unwrap().len();
            if count != places {
                wrong.push(format!("`{old}` (at `{word}`): {count} places changed, not {places}"));
            }
            let after = errors(&renamed(text, &edit));
            if !after.is_empty() {
                wrong.push(format!("after renaming `{old}` (at `{word}`):\n{}", after.join("\n")));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn renames_in_an_app() {
        let app = r#"struct Todo {
    title: String
    done: Bool
}

view Row(todo: Todo, strike: Bool = false) {
    HStack {
        let shown = if strike { "({todo.title})" } else { todo.title }
        Text(shown)
        if todo.done {
            Text("done")
        }
    }
}

app Todos(width: 300, height: 300) {
    state todos: [Todo] = [Todo(title: "one", done: false)]
    state draft = "two"

    VStack {
        let open = todos.filter { t in !t.done }
        Text("{open.count} open")
        TextField("New", text: draft)
        for todo in todos {
            Row(todo: todo, strike: todo.done)
        }
        Button("Add") { add(titled: draft) }
    }

    fn add(titled: String) {
        todos.append(Todo(title: titled, done: false))
        draft = ""
    }
}
"#;
        check_renames(
            "app",
            app,
            &[
                ("Todo", 0, 5),         // a struct, in a view's parameter and a state's type
                ("title", 0, 5),        // a field
                ("Row", 0, 2),          // a view
                ("todo: Todo", 0, 5),   // a view's parameter, and its label
                ("strike", 0, 3),       // one with a default
                ("shown", 0, 2),        // a `let` in a view
                ("todos", 0, 4),        // state
                ("draft", 0, 4),        // state that's bound, and set
                ("open", 0, 2),         // a `let` in the app
                ("todo in", 0, 3),      // a loop's variable in content
                ("add", 0, 2),          // the app's function
                ("titled", 0, 3),       // its parameter
            ],
        );
    }


    /// `text` with a rename's changes made.
    fn renamed(text: &str, edit: &serde_json::Value) -> String {
        let lines: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let offset = |p: &serde_json::Value| lines[p["line"].as_u64().unwrap() as usize] + p["character"].as_u64().unwrap() as usize;
        let changes = edit["changes"].as_object().unwrap().values().next().unwrap().as_array().unwrap();
        let mut edits: Vec<(usize, usize, &str)> =
            changes.iter().map(|c| (offset(&c["range"]["start"]), offset(&c["range"]["end"]), c["newText"].as_str().unwrap())).collect();
        edits.sort();
        let mut out = text.to_string();
        for (lo, hi, new) in edits.into_iter().rev() {
            out.replace_range(lo..hi, new);
        }
        out
    }

    #[test]
    fn goes_to_a_names_declaration_and_finds_its_uses() {
        let (server, uri, dir) = with_program("go");
        let ask = |method: &str, at: Position| {
            let params = serde_json::json!({ "textDocument": { "uri": uri }, "position": at, "context": { "includeDeclaration": true } });
            server.navigate(method, &params).unwrap()
        };
        // From a use of the field (`than.price`) to its declaration.
        let found = ask("textDocument/definition", cursor(".price", 0));
        assert_eq!(found["range"]["start"], serde_json::to_value(cursor("price: Float", 0)).unwrap()["line"].as_u64().map(|l| serde_json::json!({ "line": l, "character": 4 })).unwrap());
        // The field: its declaration, the six times it's read, and where
        // it's given a value by its label, twice.
        let uses = ask("textDocument/references", cursor("price: Float", 0));
        assert_eq!(uses.as_array().unwrap().len(), 9, "{uses:#}");
        // A function, from its call.
        let found = ask("textDocument/definition", cursor("total(items: stack", 0));
        assert_eq!(found["range"]["start"]["line"], PROGRAM[..PROGRAM.find("fn total").unwrap()].matches('\n').count());
        // Nothing for what the program doesn't declare.
        assert!(ask("textDocument/definition", cursor("print(show", 0)).is_null());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_renamed_program_still_means_the_same() {
        let (server, uri, dir) = with_program("rename");
        let mut sources = SourceMap::new();
        sources.add("main.tsl", PROGRAM);
        let before = tessel_compiler::analyze(&mut sources, true);
        assert!(before.diags.is_empty(), "{}", before.diags.list.iter().map(|d| tessel_compiler::diag::render(d, &sources, false)).collect::<String>());
        // Each name, by a place it's written (a declaration or a use), and
        // how many places a rename must change.
        let names = [
            ("price: Float", 0, 9),     // a field
            (".price", 0, 9),           // the same, from a use
            ("label: String", 0, 3),    // a field given by its label
            ("doubled", 0, 2),          // a computed property
            ("bump", 0, 2),             // a method, called by its bare name too
            ("costlier", 0, 2),         // a method
            ("than", 0, 3),             // a parameter: its uses and its label
            ("free(label", 0, 2),       // a static function
            ("Item", 0, 8),             // a struct
            ("Kind", 0, 3),             // an enum
            ("plain", 0, 4),            // a case
            ("boxed", 0, 3),            // a case with a value
            ("roomy", 0, 2),            // an enum's method
            ("Stack", 0, 2),            // a generic struct
            ("items: [T]", 0, 4),       // its field
            ("limit", 0, 2),            // a constant, used inside a string
            ("total", 0, 2),            // a function
            ("extra", 0, 3),            // a parameter with a default
            ("sum", 0, 4),              // a variable
            ("item in", 0, 3),          // a loop's variable
            ("show", 0, 2),             // a function declared in a function
            ("amount", 0, 3),           // its parameter: in a string, and as a label
            ("each", 0, 2),             // a block's parameter
            ("largest", 0, 2),          // a generic function
            ("Shape", 0, 4),            // an interface: conformance, constraint, type
            ("first", 0, 4),            // a variable in `main`
        ];
        let mut wrong = Vec::new();
        for (word, nth, places) in names {
            let at = cursor(word, nth);
            // (A type's name starts with a capital, as `Stack<Int>` has to.)
            let new = if word.starts_with(char::is_uppercase) { "RenamedIt" } else { "renamedIt" };
            let params = serde_json::json!({ "textDocument": { "uri": uri }, "position": at, "newName": new });
            let prepared = server.navigate("textDocument/prepareRename", &params).unwrap_or_else(|e| panic!("{word}: {e}"));
            let old = prepared["placeholder"].as_str().unwrap().to_string();
            let edit = server.navigate("textDocument/rename", &params).unwrap();
            let count = edit["changes"].as_object().unwrap().values().next().unwrap().as_array().unwrap().len();
            let text = renamed(PROGRAM, &edit);
            if count != places {
                wrong.push(format!("`{old}` (at `{word}`): {count} places changed, not {places}"));
            }
            // The program has no errors after it, and nothing else changed.
            let mut sources = SourceMap::new();
            sources.add("main.tsl", text.clone());
            let analysis = tessel_compiler::analyze(&mut sources, true);
            let errors: Vec<String> = analysis.diags.list.iter().map(|d| tessel_compiler::diag::render(d, &sources, false)).collect();
            if !errors.is_empty() {
                wrong.push(format!("after renaming `{old}` (at `{word}`):\n{}", errors.join("\n")));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        // What an interface asks for by name stays, and so does what the
        // program doesn't declare.
        for (word, nth) in [("area() -> Float { price", 0), ("print(show", 0), ("Float", 0)] {
            let params = serde_json::json!({ "textDocument": { "uri": uri }, "position": cursor(word, nth), "newName": "renamedIt" });
            assert!(server.navigate("textDocument/rename", &params).is_err(), "{word}");
        }
        // A new name has to be a name.
        for bad in ["two words", "9lives", "let", ""] {
            let params = serde_json::json!({ "textDocument": { "uri": uri }, "position": cursor("total", 0), "newName": bad });
            assert!(server.navigate("textDocument/rename", &params).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn positions_count_utf16() {
        let mut sources = SourceMap::new();
        sources.add("a.tsl", "let a = 1\nlet é = 😀x");
        let x = sources.file(0).text.find('x').unwrap();
        assert_eq!(position(&sources, 0, x), Position { line: 1, character: 10 });
    }
}
