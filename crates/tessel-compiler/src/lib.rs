//! The Tessel compiler: source text in, native object code out.
//!
//! Pipeline (see docs/design.md): lexer -> parser -> type checker -> LLVM IR.

pub mod ast;
pub mod builtins;
pub mod check;
pub mod codegen;
pub mod diag;
pub mod lexer;
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

/// Parses every file in `sources` and type-checks them together as one
/// program. `require_entry` reports an error if there is no `app` or `fn main()`.
pub fn analyze(sources: &SourceMap, require_entry: bool) -> Analysis {
    let mut diags = Diagnostics::default();
    let mut program = ast::Program::default();
    let mut next_id = 0;
    for (id, file) in sources.files() {
        let parsed = parser::parse_file(id, &file.text, &mut diags, &mut next_id);
        program.items.extend(parsed.items);
    }
    let checked = check::check_program(&program, &mut diags, require_entry);
    diags.list.sort_by_key(|d| (d.span().file, d.span().lo));
    Analysis { program, checked, diags }
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
        analyze(&map, false).checked.completion
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
