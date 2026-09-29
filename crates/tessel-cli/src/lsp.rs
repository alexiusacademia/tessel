//! `tessel lsp`: a language server for editors (Language Server Protocol).
//!
//! It reports the same errors as `tessel check` while you type. A program is
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
    fn positions_count_utf16() {
        let mut sources = SourceMap::new();
        sources.add("a.tsl", "let a = 1\nlet é = 😀x");
        let x = sources.file(0).text.find('x').unwrap();
        assert_eq!(position(&sources, 0, x), Position { line: 1, character: 10 });
    }
}
