---
title: Editor support
description: Use Tessel in VS Code, or in any editor that supports the Language Server Protocol, through tessel lsp.
---

Besides the [Tessel IDE](/start/ide/), you can write Tessel in Visual Studio
Code with the Tessel extension, or in any other editor that can talk to a
language server.

## VS Code

The extension lives in the Tessel repository, in `editors/vscode`. It gives
you:

- **Syntax highlighting** for `.tsl` files.
- **Errors as you type.** The same errors `tessel check` reports appear as
  red underlines in the editor and in VS Code's Problems panel, updated on
  every change, before you save.
- **Types on hover.** Hover over an expression to see its type, for example
  `draft: String` or `todos: [Todo]`.
- Comment toggling (`//`), bracket matching, auto-closing of brackets and
  quotes, and automatic indentation after `{`.

Errors and hover types come from `tessel lsp`, so the extension needs the
`tessel` command (see [Installation](/start/installation/)).

### Installing

The extension isn't in the VS Code Marketplace yet. You build a `.vsix`
package and install it from the file. You need [Node.js](https://nodejs.org)
for the build.

```sh
cd editors/vscode
npm install
npm run package
code --install-extension tessel-0.1.0.vsix
```

You can also install the `.vsix` from inside VS Code: open the Extensions
view, click the `...` menu, and choose **Install from VSIX...**.

### Settings

The extension has one setting:

| Setting | Default | Meaning |
|---|---|---|
| `tessel.path` | `tessel` | The `tessel` command used for error checking and hover. |

If `tessel` isn't on your `PATH`, set `tessel.path` to the full path of the
executable, for example `/Users/you/tessel/target/release/tessel`. The value
may contain `${workspaceFolder}`, which is replaced with the first folder
open in VS Code.

If the extension can't start `tessel lsp`, VS Code shows a message saying
so; highlighting keeps working without it.

In a folder that VS Code doesn't trust yet (Restricted Mode), highlighting
works, but a `tessel.path` set in that folder's own settings is ignored until
you trust the folder.

## How checking works

A Tessel program is a folder, so the language server checks the whole
folder whenever a file in it changes, using the text of your open files even
if they're not saved. Errors in other files of the same folder are shown
too.

Unlike `tessel check`, the language server doesn't complain when there's no
`app` or `fn main()` yet, because the file you're editing may be one part of
a larger program.

## Other editors

`tessel lsp` is a standard [Language Server
Protocol](https://microsoft.github.io/language-server-protocol/) server that
talks over standard input and output. Any editor with LSP support, such as
Neovim, Helix, Zed, Sublime Text (with the LSP package) or Emacs (with
eglot or lsp-mode), can use it. Configure it to:

- start the command `tessel lsp`,
- for files ending in `.tsl`.

The server sends diagnostics (errors) when a file is opened, changed, saved
or closed, and answers hover requests with the type of the expression under
the cursor. It doesn't offer completion, go-to-definition or formatting yet.

For syntax highlighting, the extension's TextMate grammar,
`editors/vscode/syntaxes/tessel.tmLanguage.json`, works in any editor that
reads TextMate grammars.
