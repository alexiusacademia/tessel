# Changelog

## 0.4.0

- A debugger: breakpoints in `.tsl` files, stepping, the calls that led to a
  line, and variables with their parts. F5 (or the bug button) debugs the
  program of the file being edited. Needs Tessel 0.2.0 or later.
- Highlighting for `async`, `await` and `static`.

## 0.3.0

- Commands to run (also with hot reload), check and build the program, and
  to pin the project's version of Tessel; a ▶ button runs it.
- `tessel.toml`: completion, descriptions and checking of its keys, with
  the Even Better TOML extension.
- The language server runs in the project's folder, so a version pinned in
  `tessel.toml` is used, and starts again when the file (or `tessel.path`)
  changes.
- Highlighting for `self`, `is` and `as`.

## 0.2.0

- Code completion, hover types and errors as you type, all from `tessel lsp`
  (completion needs Tessel 0.1.8 or later).
- An icon, and the extension's details for the Marketplace.

## 0.1.0

- Syntax highlighting, and errors as you type.
