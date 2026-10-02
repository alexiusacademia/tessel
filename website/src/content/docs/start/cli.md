---
title: Command line
description: Every tessel subcommand and option, what errors look like, and the environment variables Tessel reads.
---

Everything in Tessel goes through one command, `tessel`. Run `tessel help`
to see this summary:

```
usage: tessel [command]

With no command, opens the Tessel IDE on the current folder.

commands:
  ide [folder]   open the Tessel IDE on a folder (default: the current one)
  check <path> [--format lines]
                 check a program for errors; <path> is a .tsl file or a folder.
                 With --format lines, prints one error per line for tools:
                 file, line, column, end line, end column, message (tab-separated)
  build <path> [-o <output>] [--bundle]
                 compile a program to a native executable; with --bundle,
                 an app bundle you can double-click (macOS: Name.app).
                 An icon.png next to the program is the app's icon
  run <path> [--hot]
                 build and run a program; with --hot, an app takes its new
                 code when its files change, and keeps its state
  repl           type code and see what it does, a line at a time
  lsp            start the language server (used by editors)
  help           show this help
  version        print the compiler version
```

## Programs: a file or a folder

Wherever a command takes a `<path>`, it can be a single `.tsl` file or a
folder. A folder is one program made of **every `.tsl` file directly in
it**; files in subfolders are not included. The files share one namespace,
so there are no imports (see [Programs and files](/language/programs/)).

A program has exactly one entry point: an `app` (a program with a window)
or a `fn main()` (a program that runs in the terminal).

## `tessel check`

```sh
tessel check <path>
tessel check <path> --format lines
```

Checks a program for mistakes without compiling it: syntax, types, names,
missing `match` cases, and so on. It's fast, so run it often.

```sh
tessel check examples/todos
```

```
ok: 1 file checked, no errors
```

When there are errors, each one shows where it is and, often, how to fix it.
For this program:

```tessel
fn main() {
    let count = 3
    let label = "Items: " + count
    print(labl)
}
```

`tessel check broken` prints:

```
error: can't use `+` on `String` and `Int`
 --> broken/main.tsl:3:17
  |
3 |     let label = "Items: " + count
  |                 ^^^^^^^^^^^^^^^^^
  |
  = help: to put a value into text, use "{…}", like "Total: {total}"

error: cannot find `labl`
 --> broken/main.tsl:4:11
  |
4 |     print(labl)
  |           ^^^^ not found
  |
  = help: did you mean `label`?

2 errors found
```

In a terminal, the errors are shown in color. The command exits with status
0 when there are no errors and 1 when there are, so you can use it in
scripts.

### `--format lines`

With `--format lines`, `tessel check` prints one line per error, for other
programs to read. The fields are separated by tabs: file, line, column, end
line, end column, and the message (with any help added in parentheses). For
the program above:

```
broken/main.tsl	3	17	3	34	can't use `+` on `String` and `Int` (help: to put a value into text, use "{…}", like "Total: {total}")
broken/main.tsl	4	11	4	15	cannot find `labl` (help: did you mean `label`?)
```

Nothing is printed when there are no errors. The [Tessel IDE](/start/ide/)
uses this format for its Problems list.

## `tessel run`

```sh
tessel run <path> [--hot]
```

Compiles the program to a temporary executable and runs it. A `fn main()`
program prints to the terminal; an `app` opens its window, and `tessel run`
returns when you close it. The temporary executable is deleted afterwards.

If the program has errors, they're shown the same way as with
`tessel check`, and nothing runs.

If the program makes a mistake while running, such as reading past the end
of a list, it stops with a message and the place in your code:

```
error: index 2 is out of range for a list of 2 items
 --> rt/main.tsl:3:11
```

### Hot reload

```sh
tessel run todos --hot
```

With `--hot`, an app keeps running while you work on it. Whenever you save
one of its files, the running app takes the new code, and keeps its state:
the window stays where it is, with the same text in its fields, the same
tab selected, the same items in its lists. `tessel run` says so:

```text
tessel: reloaded
```

- **What's kept.** Every `state` value, of the `app` and of your views,
  along with what the text fields, scroll views and code editors keep
  themselves (the cursor, the scroll position, undo history).
- **What starts afresh.** A view whose `state` declarations or parameters
  changed (one added or removed, renamed, or of another type), or use a
  struct or enum that changed: its old state doesn't fit the new code, so
  that view begins with its initial values. Other views keep theirs. A view
  that moves to another place among its neighbors is a new view, as always
  (see [Identity](/ui/lists/#identity-with-id)).
- **What isn't run again.** State keeps its value even if its initial value
  in the code is different now, and `.onAppear` blocks of views that are
  already showing don't run again. Timers started by the old code go on
  running the old code. To see those changes, close the app and run it again.
- **Errors.** If the saved code doesn't compile, the errors are printed and
  the app goes on with the code it has. Save again once they're fixed.
- The window's title and size, and the `appearance:` the `app` asks for,
  are set when the app starts, and don't change on a reload.

A program without an `app` is restarted from the beginning when its files
change (`tessel: restarted`), for as long as it's running.

With `--hot` the app's code is compiled the same way, but loaded into
`tessel` itself rather than made into an executable, so that new code can
join it. The [Tessel IDE](/start/ide/#reload-on-save) runs programs this way
when **Reload on save** is on.

## `tessel repl`

```sh
tessel repl
```

Runs code as you type it, a line at a time: a quick way to try something
out. Variables, functions and types stay for the lines that follow.

```text
Tessel 0.1.19 (:help for help, :quit to leave)
> let name = "Ada"
> "Hello, {name}!"
Hello, Ada!
> fn double(n: Int) -> Int { n * 2 }
> double(21)
42
> :quit
```

[The REPL](/start/repl/) has its own page: how values are shown, entries of
several lines, what happens on errors, the `:` commands, and running lines
from a file (`tessel repl < lines.txt`).

## `tessel build`

```sh
tessel build <path> [-o <output>] [--bundle]
```

Compiles the program to a native executable that runs without Tessel
installed.

- **`-o <output>`** sets the executable's path. Without it, the executable
  goes in the current folder, named after the program: `tessel build
  hello.tsl` makes `hello`, `tessel build .` is named after the current
  folder, and `tessel build todos` makes `todos` (or `todos-app`, if a
  folder called `todos` is already there). Folders in the `-o` path are
  created if needed. On Windows, `.exe` is added when the name has no
  extension.
- **`--bundle`** makes an app you can double-click. On macOS this is an app
  bundle, `Name.app`, named after your `app` (for a `fn main()` program,
  after the executable), created next to where the executable would go; the
  executable moves inside it. On Windows, an executable is already
  double-clickable, so `--bundle` makes the same `.exe`.

On Windows, apps (programs with an `app`) are built without a console
window.

### The app's icon

Put a square PNG image called `icon.png` next to your program's `.tsl`
files, and it becomes the app's icon: of `Name.app` on macOS (in Finder and
the Dock), and of the `.exe`, its window and its taskbar button on Windows.
1024 × 1024 pixels is best; Tessel makes the smaller sizes from it. Without
an `icon.png`, apps get Tessel's icon.

On macOS only a bundle has an icon, so build with `--bundle`.

```sh
tessel build todos -o todos-app --bundle
```

```
built Todos.app
```

`tessel build` links your program with the Tessel runtime library that
sits next to the `tessel` executable, using its own built-in linker. (A
`tessel` built from source without the `bundled-linker` feature uses the
system linker instead; set `TESSEL_LINKER=system` to force that.)

## `tessel` and `tessel ide`

```sh
tessel
tessel ide [folder]
```

Opens the [Tessel IDE](/start/ide/) on a folder: the current one, or the one
you name. The first time, `tessel` compiles the IDE and caches it in your
system's temporary folder; it's compiled again only when `tessel` itself
changes.

## `tessel update`

```sh
tessel update            # to the latest release
tessel update --check    # only say whether there's a newer one
tessel update v0.1.2     # to a specific version
```

Replaces the installation this `tessel` runs from (the folder it's in) with
another release, after checking the download against the release's
checksums. See [Updating](/start/installation/#updating).

## `tessel complete`

```sh
tessel complete main.tsl 120
tessel complete main.tsl 120 --text /tmp/unsaved.tsl
```

Prints what could be typed at byte offset 120 of `main.tsl`, using the type
checker: the members after `x.`, the enum cases after `.`, and the names in
scope elsewhere. The other `.tsl` files in the same folder are part of the
program. `--text` reads the file's current text from another file, for
editors with unsaved changes.

The first line is `start` and the offset where the word being completed
starts; then one suggestion per line, tab-separated: kind, label, detail.

```text
start 118
field	path	String
field	line	Int
method	rename	(to: String)
```

The [Tessel IDE](/start/ide/) uses it for code completion.

## `tessel lsp`

```sh
tessel lsp
```

Starts a language server that speaks the Language Server Protocol over
standard input and output. You don't run it yourself; editors start it to
show errors as you type and types on hover. See
[Editor support](/start/editors/).

## `tessel help` and `tessel version`

`tessel help` (also `--help` or `-h`) prints the summary at the top of this
page. `tessel version` (also `--version` or `-V`) prints the version:

```
tessel 0.1.0
```

An unknown command, or a command missing its `<path>`, prints the usage
summary and exits with status 1.

## Environment variables

| Variable | Used by | What it does |
|---|---|---|
| `TESSEL_SCRIPT` | Built apps | Runs the app without a window, driven by a script of commands: click, type, press keys, wait, print the view tree, or save a screenshot. See [Testing apps](/ui/testing/). |
| `TESSEL_EXE` | Set by `tessel run` and `tessel ide` | The path of the `tessel` command that started the program. A program can read it with `environment("TESSEL_EXE")`, which is how the IDE finds `tessel`. |
| `TESSEL_PROJECT` | The IDE | The project folder. `tessel ide` sets it; you don't normally need it. |
| `TESSEL_RELEASES` | `tessel update` | Where releases are downloaded from, instead of the official release bucket (for testing a release). |
| `TESSEL_NO_UPDATE_CHECK` | The IDE | When set, the IDE doesn't check for a newer Tessel when it opens. |
| `TESSEL_DEBUG_EVENTS` | Built apps | When set, an app prints every window event it receives (mouse, keyboard, resizing) to standard error. Useful when something doesn't react to input. |
| `LLVM_SYS_221_PREFIX` | Building Tessel | Where LLVM 22 is installed; see [Installation](/start/installation/). |

For example, this saves a screenshot of the counter after two clicks,
without opening a window:

```sh
tessel build examples/counter -o counter
TESSEL_SCRIPT='click "+"; click "+"; snapshot counter.png' ./counter
```
