//! `tessel query`: what the Tessel IDE asks about the code as it's edited,
//! one question per run: the errors, what a name is (hover), where it's
//! declared, where it's used, and how to rename it. The answers are the
//! language server's (`lsp`), as lines of tab-separated fields, with
//! positions as byte offsets into the text (what the IDE's `CodeEditor`
//! counts in) as well as lines and columns.
//!
//! `--text <file> <from>` (any number of times) reads a file's current,
//! unsaved text from `<from>`, so the answers are about the text in the
//! editor, saved or not.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tessel_compiler::source::{SourceMap, Span, FILE_MASK, PRELUDE_FILE};

use crate::lsp;

const USAGE: &str = "usage: tessel query errors <path>
       tessel query hover|definition|uses|highlights <file> <offset>
       tessel query rename <file> <offset> [<new name>]
       (each with any number of --text <file> <from>)";

pub fn run(args: &[String]) -> ExitCode {
    // `--text <file> <from>`: the rest are the question.
    let mut texts: Vec<(PathBuf, String)> = Vec::new();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--text" && i + 2 < args.len() {
            match std::fs::read_to_string(&args[i + 2]) {
                Ok(text) => texts.push((PathBuf::from(&args[i + 1]), text)),
                Err(e) => return fail(&format!("can't read `{}`: {e}", args[i + 2])),
            }
            i += 3;
        } else {
            words.push(&args[i]);
            i += 1;
        }
    }
    let offset = words.get(2).and_then(|o| o.parse::<usize>().ok());
    match (words.first().copied(), words.get(1), offset) {
        (Some("errors"), Some(path), _) => errors(Path::new(path), &texts),
        (Some(what @ ("hover" | "definition" | "uses" | "highlights" | "rename")), Some(file), Some(offset)) => {
            ask(what, Path::new(file), offset, words.get(3).copied(), &texts)
        }
        _ => fail(USAGE),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("tessel: {message}");
    ExitCode::FAILURE
}

/// Whether two paths name the same file.
fn same(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// The program's files (`files`), with the unsaved texts in place of the saved ones.
fn sources(files: &[PathBuf], texts: &[(PathBuf, String)]) -> Result<SourceMap, String> {
    let mut sources = SourceMap::new();
    for file in files {
        let text = match texts.iter().find(|(p, _)| same(p, file)) {
            Some((_, t)) => t.clone(),
            None => std::fs::read_to_string(file).map_err(|e| format!("can't read `{}`: {e}", file.display()))?,
        };
        sources.add(file.display().to_string(), text);
    }
    Ok(sources)
}

/// One line of text: tabs and line breaks become spaces.
fn flat(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// `tessel query errors <path>`: the errors `tessel check <path>` finds, one
/// per line: file, line, column, end line, end column, start and end (byte
/// offsets), message.
fn errors(path: &Path, texts: &[(PathBuf, String)]) -> ExitCode {
    let found = match crate::use_manifest(path) {
        Ok(found) => found,
        Err(code) => return code,
    };
    let files = match crate::program_files(path) {
        Ok(files) => files,
        Err(e) => return fail(&e),
    };
    let mut sources = match sources(&files, texts) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    // (A library has no `main` or `app`: the programs that use it do.)
    let library = found.is_some_and(|f| f.manifest.library.is_some());
    let analysis = tessel_compiler::analyze(&mut sources, !library);
    let mut out = String::new();
    for d in &analysis.diags.list {
        let span = d.span();
        let file = sources.file(span.file);
        let (line, col) = file.line_col(span.lo as usize);
        let (end_line, end_col) = file.line_col(span.hi as usize);
        let mut message = flat(&d.message);
        if let Some(help) = &d.help {
            message += &format!(" (help: {})", flat(help));
        }
        out += &format!("{}\t{line}\t{col}\t{end_line}\t{end_col}\t{}\t{}\t{message}\n", file.name, span.lo, span.hi);
    }
    print!("{out}");
    if analysis.diags.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// The questions about the name or expression at byte `offset` of `file`,
/// which is part of the program in its folder.
fn ask(what: &str, file: &Path, offset: usize, new_name: Option<&str>, texts: &[(PathBuf, String)]) -> ExitCode {
    // (Only Tessel files have names to ask about.)
    if file.extension().is_none_or(|e| e != "tsl") {
        if what == "rename" {
            println!("error\t{}", lsp::NOT_A_NAME);
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    let folder = file.parent().filter(|f| !f.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let found = crate::manifest::find(folder).ok().flatten();
    tessel_compiler::set_app_info(crate::manifest::app_info(found.as_ref(), folder));
    let mut files = crate::program_files(folder).unwrap_or_default();
    if !files.iter().any(|f| same(f, file)) {
        files.push(file.to_path_buf());
    }
    let Some(id) = files.iter().position(|f| same(f, file)) else { return fail(USAGE) };
    let mut sources = match sources(&files, texts) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let own = files.len() as u32;
    // (Don't insist on an `app` or `main`: the answers are the same.)
    let analysis = tessel_compiler::analyze(&mut sources, false);
    let id = id as u32;
    let text = &sources.file(id).text;
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let offset = offset as u32;
    let place = |span: Span| {
        let f = sources.file(span.file);
        let (line, col) = f.line_col(span.lo as usize);
        format!("{}\t{line}\t{col}\t{}\t{}", f.name, span.lo, span.hi)
    };
    let mut out = String::new();
    if what == "hover" {
        if let Some(info) = lsp::hover_info(&analysis, &sources, id, offset) {
            out += &format!("{}\t{}\n{}\n", info.span.lo, info.span.hi, info.code);
            if !info.doc.is_empty() {
                out += &format!("\n{}\n", info.doc);
            }
        }
        print!("{out}");
        return ExitCode::SUCCESS;
    }
    let symbol = tessel_compiler::navigate::symbol_at(&analysis, &sources, id, offset);
    let symbol = symbol.filter(|s| s.declared.file & FILE_MASK != PRELUDE_FILE);
    match (what, symbol) {
        ("rename", None) => {
            println!("error\t{}", lsp::NOT_A_NAME);
            return ExitCode::FAILURE;
        }
        (_, None) => {}
        ("definition", Some(symbol)) => out += &format!("{}\n", place(symbol.declared)),
        ("uses", Some(symbol)) => {
            for span in &symbol.places {
                let f = sources.file(span.file);
                let line = f.text.lines().nth(f.line_index(span.lo as usize)).unwrap_or("");
                out += &format!("{}\t{}\n", place(*span), flat(line.trim()));
            }
        }
        // (In this file: what the editor marks as the caret rests on a name.)
        ("highlights", Some(symbol)) => {
            for span in symbol.places.iter().filter(|s| s.file & FILE_MASK == id) {
                out += &format!("{}\t{}\n", span.lo, span.hi);
            }
        }
        (_, Some(symbol)) => {
            // `rename`: what it's called (to ask for its new name), or the
            // places to change.
            let library = symbol.declared.file & FILE_MASK >= own;
            let checked = match library {
                true => Err(format!("`{}` can't be renamed: a library declares it.", symbol.name)),
                false => lsp::rename_check(&symbol, new_name),
            };
            if let Err(why) = checked {
                println!("error\t{why}");
                return ExitCode::FAILURE;
            }
            match new_name {
                None => out += &format!("name\t{}\n", symbol.name),
                Some(_) => {
                    for span in symbol.places.iter().filter(|s| s.file & FILE_MASK < own) {
                        out += &format!("{}\t{}\t{}\n", sources.file(span.file).name, span.lo, span.hi);
                    }
                }
            }
        }
    }
    print!("{out}");
    ExitCode::SUCCESS
}
