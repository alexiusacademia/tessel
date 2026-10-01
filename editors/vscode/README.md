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
- Comment toggling, bracket matching, auto-closing pairs and indentation.

A program is a folder of `.tsl` files, and the whole folder is checked
together, including the modules it imports.

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
