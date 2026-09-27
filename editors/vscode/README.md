# Tessel for VS Code

Syntax highlighting for `.tsl` files, and Tessel's errors as you type (from
`tessel lsp`).

Set `tessel.path` to the `tessel` executable if it isn't on your `PATH`, e.g.
`/path/to/tessel/target/debug/tessel`.

Build a `.vsix` with `npm install && npm run package`, then install it with
`code --install-extension tessel-0.1.0.vsix`.
