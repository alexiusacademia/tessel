//! The Tessel compiler: source text in, native object code out.
//!
//! Pipeline (see docs/design.md): lexer -> parser -> type checker -> LLVM IR.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
