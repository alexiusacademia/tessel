# Tessel for VS Code

Language support for [Tessel](https://tessel-lang.syncster.dev), a small,
Swift-like language for native apps and command-line programs.

## Features

- **Syntax highlighting** for `.tsl` files.
- **Errors as you type.** The same errors `tessel check` reports, with their
  help text, appear as you edit, before you save, in the editor and in the
  Problems panel.
- **Types on hover.** Hover over an expression to see its type.
- **Code completion.** Fields and methods after `value.`, enum cases after
  `.`, a module's public names after `geometry.`, and every variable,
  function, type and view in scope. Typing `.` opens the list; Ctrl+Space
  asks for it anywhere.
- **Run and build.** The ▶ button in the editor's title bar runs the program
  (or with hot reload, from its menu), and the Command Palette has **Tessel:
  Run Program**, **Run with Hot Reload**, **Check Program**, **Build App**
  and **Pin Tessel Version**. They run `tessel` in a terminal.
- **`tessel.toml`.** With the [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml)
  extension, a project's `tessel.toml` gets completion, descriptions and
  checking of its keys (`[app]`, `[[app.documents]]`, …). See
  [The project file](https://tessel-lang.syncster.dev/start/project/).
- **The project's version of Tessel.** When `tessel.toml` pins a version,
  errors and completion come from that version; the language server starts
  again when the file changes.
- Comment toggling, bracket matching, auto-closing pairs and indentation.

A program is a folder of `.tsl` files, and the whole folder is checked
together, including the modules it imports. The run commands run the active
file's folder when it has a `main.tsl` or a `tessel.toml`, and otherwise the
file on its own.

## Requirements

Errors, hover and completion come from the `tessel` command, which runs as a
language server (`tessel lsp`). Install Tessel first; see
[Installation](https://tessel-lang.syncster.dev/start/installation/).
Highlighting works without it.

## Settings

| Setting | Default | |
|---|---|---|
| `tessel.path` | `tessel` | The `tessel` command. Set it if `tessel` isn't on your `PATH`. `${workspaceFolder}` is replaced by the open folder. |

In an untrusted workspace, highlighting works, and a folder's own
`tessel.path` setting is ignored until you trust the folder.

## Building from source

```sh
npm install
npm run package          # makes tessel-<version>.vsix
code --install-extension tessel-<version>.vsix
```
