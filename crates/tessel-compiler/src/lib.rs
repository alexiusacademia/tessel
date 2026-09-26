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
