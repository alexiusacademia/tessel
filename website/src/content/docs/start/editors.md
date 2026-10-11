---
title: Editor support
description: Use Tessel in VS Code, or in any editor that supports the Language Server Protocol, through tessel lsp.
---

Besides the [Tessel IDE](/start/ide/), you can write Tessel in Visual Studio
Code with the Tessel extension, or in any other editor that can talk to a
language server.

## VS Code

The Tessel extension is in the VS Code Marketplace. It gives you:

- **Syntax highlighting** for `.tsl` files.
- **Errors as you type.** The same errors `tessel check` reports appear as
  red underlines in the editor and in VS Code's Problems panel, updated on
  every change, before you save.
- **Types on hover.** Hover over an expression to see its type, for example
  `draft: String` or `todos: [Todo]`.
- **Code completion.** Suggestions as you type, the same as in the
  [Tessel IDE](/start/ide/): the fields and methods after `todo.`, the cases
  after `.` where an enum is expected, a module's public names after
  `geometry.`, and the variables, functions, types, views and keywords
  visible elsewhere. Press Ctrl+Space to ask for them.
- **Go to definition, find references and rename.** F12 on a name goes to
  where it's declared, Shift+F12 lists where it's used, and F2 renames it
  everywhere in the program: variables, functions, types, fields, methods,
  enum cases, views and parameters (with the labels at their calls).
- **A debugger.** Click beside a line to set a breakpoint and press F5: the
  program runs and stops there, with its variables and the calls that led
  to the line in VS Code's debugging views. See
  [Debugging](/guides/debugging/#in-vs-code).
- Comment toggling (`//`), bracket matching, auto-closing of brackets and
  quotes, and automatic indentation after `{`.

Errors, hover types and completions come from `tessel lsp`, so the
extension needs the `tessel` command (see [Installation](/start/installation/)).

### Installing

Open the Extensions view in VS Code (Cmd+Shift+X, or Ctrl+Shift+X on
Windows), search for **Tessel**, and click **Install** on the one published
by `alexiusacademia`. Or install it from a terminal:

```sh
code --install-extension alexiusacademia.tessel
```

It's also on the Marketplace's website:
[Tessel for VS Code](https://marketplace.visualstudio.com/items?itemName=alexiusacademia.tessel).
VS Code keeps it up to date from there.

Code completion needs Tessel 0.1.8 or later; run
[`tessel update`](/start/installation/#updating) if yours is older.

:::note
Editors built on VS Code that use another marketplace (Cursor, VSCodium and
others) don't find the extension by searching. Build it from its source, as
described next, and install the file.
:::

### Building it from source

The extension's source is in the Tessel repository, in `editors/vscode`. To
build a `.vsix` package and install it from the file, you need
[Node.js](https://nodejs.org):

```sh
cd editors/vscode
npm install
npm run package
code --install-extension tessel-0.2.0.vsix
```

You can also install the `.vsix` from inside VS Code: open the Extensions
view, click the `...` menu, and choose **Install from VSIX...**.

### Settings

The extension has one setting:

| Setting | Default | Meaning |
|---|---|---|
| `tessel.path` | `tessel` | The `tessel` command used for errors, hover and completion. |

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
or closed, answers hover requests with the type of the expression under
the cursor, and answers completion requests (typing `.` asks for them too).
It goes to a name's definition, finds its references, highlights them in
the file, and renames it (with `prepareRename`, so an editor can say at
once when something can't be renamed). It doesn't offer formatting yet.

Rename covers the names a program declares. It refuses a member that an
interface asks for by name (the interface and every type that conforms
would have to change together), and anything built in.

For debugging, set the editor up to use the debug adapter
`tessel debug --dap`: see
[Debugging](/guides/debugging/#for-other-editors).

For syntax highlighting, the extension's TextMate grammar,
`editors/vscode/syntaxes/tessel.tmLanguage.json`, works in any editor that
reads TextMate grammars.
