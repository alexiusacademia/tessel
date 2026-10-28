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
  new <folder> [--template app|blank|cli|library]
                 start a project in a new folder: a program to build on, its
                 tessel.toml, a README and a .gitignore. Templates: app (the
                 default; a to-do list app), blank (one window), cli and
                 library (one to publish for other programs)
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
  test [path] [--filter <text>] [--timeout <seconds>]
                 run the program's tests (`test "name" { … }` blocks), each in a
                 process of its own; with --filter, those whose name has <text>
  add <library>[@<version>]
                 use a published library: adds it to [dependencies] in
                 tessel.toml, works out its version (and those of the
                 libraries it uses) into tessel.lock, and downloads it. Then
                 `import <library>` in the code. --git <url> --tag <tag> takes
                 it from a git repository; --path <folder> from a folder
  remove <library>
                 stop using a library
  deps [update [library...]]
                 list the libraries the project uses; with update, move them
                 (or the ones named) to the newest versions tessel.toml allows
  search [words] list the published libraries (that match the words)
  publish [--dry-run]
                 publish the library in this folder (its tessel.toml has a
                 [library] table): checks it, tags its git commit v<version>,
                 pushes the tag and proposes it to the library index
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

## `tessel new`

```sh
tessel new my-tasks                      # a to-do list app to build on
tessel new my-app --template blank       # an app with one window
tessel new tool --template cli           # a command-line program
tessel new textkit --template library    # a library to publish
```

Starts a project in a new folder (or in a folder without a program yet, like
`tessel new .` in an empty one), from a template:

| Template | What you get |
|---|---|
| `app` (the default) | A small to-do list app, split the way a bigger app is: `main.tsl` (the window, a menu, what the buttons do), `task.tsl` (the data, and saving it as JSON between runs) and `views.tsl` (a view of its own). |
| `blank` | An app with one window, a text and a button, in `main.tsl`. |
| `cli` | A program with `fn main` that greets the names it's given: `tessel run . Ana Ben`. |
| `library` | A [library](/guides/libraries/) for other programs: its code, and a `[library]` table ready for `tessel publish`. |

Each also gets:

- a `tessel.toml` that [pins the project](#tessel-pin) to the version of
  Tessel that made it, and for apps an `[app]` table with the app's name,
  identifier and version (see [The project file](/start/project/)). The
  folder gives the names: `my-tasks` makes the app `My Tasks`, with the
  identifier `com.example.my-tasks`. Change them before you share the app.
- a `README.md` that says how to run and build it, and what's in each file;
- a `.gitignore` that leaves out what `tessel build` makes.

Files already in the folder are kept, not replaced. Then run it:

```sh
cd my-tasks
tessel run .
```

In the [IDE](/start/ide/), a folder with no Tessel files offers the same
templates.

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

To stop at a line and look at the variables instead, use
[`tessel debug`](#tessel-debug).

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

## `tessel debug`

```sh
tessel debug <path>
```

Runs the program in the debugger: it stops at the lines you say, shows its
variables there, and goes on a line at a time. [Debugging](/guides/debugging/)
explains it, with the commands to type.

The program is built for the debugger first, which is what
`tessel build <path> --debug` does on its own: such a program tells the
debugger where it is as it runs. Started by anything else, it just runs,
a little slower than a normal build.

Editors use two other forms:

- `tessel debug --dap` is the debugger as an adapter of the
  [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/),
  on its standard input and output. The `launch` request's `program` is the
  file or folder to debug. The VS Code extension uses this.
- `tessel debug <path> --lines` reads commands and prints what happens, a
  line at a time, with tabs between a line's parts. The Tessel IDE uses
  this.

With `--lines`, the commands are `breakpoints <file> <line,line,…>` (every
line to stop at in a file), `run` (once, to start), `continue`, `next`,
`step`, `out`, `pause`, `vars <call>`, `children <ref>`,
`eval <call> <expression>` and `stop`. It prints:

| Line | When |
|---|---|
| `ready` | The program is built and waits for `run`. `failed`: it has errors (printed before, as `output` lines). |
| `output <text>` | The program printed a line. |
| `frames`, then `frame <n> <function> <file> <line>` for each call (innermost first), then `stopped <reason> <message>` | The program stopped. The reason is `breakpoint`, `step`, `pause` or `error` (with the error's message). |
| `running` | It goes on. |
| `vars <what was asked>`, then `var <name> <type> <value> <ref> <count>` for each, then `end` | The answer to `vars`, `children` or `eval`. A `ref` that isn't 0 has `count` parts, to ask for with `children <ref>`. |
| `exited <code>` | The program ended. |

## `tessel repl`

```sh
tessel repl
```

Runs code as you type it, a line at a time: a quick way to try something
out. Variables, functions and types stay for the lines that follow.

```text
Tessel 0.17.0 (:help for help, :quit to leave)
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
  executable moves inside it. On Windows (and with `--windows` on a Mac)
  it's the app's installer, `Name Setup.exe`, with the executable inside it
  (see [The installer of a Windows app](#the-installer-of-a-windows-app)).

On Windows, apps (programs with an `app`) are built without a console
window.

### For Windows, on a Mac

```sh
tessel build <path> --windows [-o <output>]
```

On a Mac, `--windows` builds the program for Windows: an `.exe` for 64-bit
Windows 10 and 11, named after the program (`tessel build todos --windows`
makes `todos.exe`). It's the same app the Windows version of Tessel makes,
with its icon and the details from [`tessel.toml`](/start/project/) inside,
and needs nothing installed on the computer it runs on.

What Windows apps are built with (Tessel's runtime for Windows and the
Windows libraries it links with, about 45 MB) isn't in the Mac's package:
the first `--windows` build downloads it, once for each version of Tessel.

The app isn't signed, so Windows may say it's from an unknown publisher the
first time it's opened (**More info**, then **Run anyway**). Signing a
Windows app needs a code-signing certificate and is done on Windows.

### The installer of a Windows app

```sh
tessel build <path> --windows --bundle
```

With `--bundle`, a Windows build is the app's installer: `Name Setup.exe`,
named after your `app` (or the `name` in [`tessel.toml`](/start/project/)),
with the app, its icon and its details inside. Built on a Mac with
`--windows` or on Windows, it's the same installer. Opened, it asks whether
to install the app and then:

- puts it in `AppData\Local\Programs\Name` (it's installed for the person
  who runs the installer, so Windows doesn't ask for an administrator),
- adds it to the Start menu, and to the desktop if they tick **Add a
  shortcut to the desktop**,
- lists it in **Settings → Apps → Installed apps**, from where it can be
  uninstalled,
- makes Explorer open the app's [own document types](/start/project/#the-apps-own-documents)
  (`owned = true`) with it, showing their icons, and offers the app in
  **Open with** for the other kinds of files it opens
  ([`documentTypes:`](/ui/menus-windows/#opening-documents) among them).

Opening a newer installer updates the app in the same place, even while it's
running (it's the new version from its next start). Uninstalling removes
the app, its shortcuts and its file types; the documents made with it stay.

`Name Setup.exe /S` installs without asking anything, and the uninstaller
(`uninstall.exe` in the app's folder) removes the app without asking with
`/uninstall /S`, for installing on many computers. The exit code is 0 when
it worked.

The installer adds less than 1 MB, and the app inside it is compressed, so
it's often smaller than the app itself (`Todos Setup.exe` is 2 MB, for a
2.3 MB `todos.exe`). Like the app, it isn't signed (see above).

### For Android

```sh
tessel build <path> --android [--bundle] [-o <output>]
tessel android doctor
```

`--android` builds an app as an Android package: an `.apk` to install on a
phone, or with `--bundle` an `.aab` for Google Play. It needs the Android
SDK and NDK; `tessel android doctor` checks that they're installed and says
how to get what's missing. See [Building for Android](/guides/android/).

On macOS, an app whose `app` lists [`documentTypes:`](/ui/menus-windows/#opening-documents)
(like `app Notes(documentTypes: ["note"])`) gets them in its bundle, so
Finder offers it in Open With for those files. The app's own types
(`owned = true` in [`tessel.toml`](/start/project/#the-apps-own-documents))
are declared in the bundle as its own, with their icons, so Finder opens
their files with the app.

### How big an app is

The executable is one file with all of Tessel it needs inside, so it runs
on a computer without Tessel. Only the parts of Tessel a program uses are in
it: icons and SVGs, pictures, Markdown, the network, dates and file dialogs
each add their code only to programs that use them.

- **macOS:** an app with a window is about 1 MB. It draws, lays out text
  and reads pictures with what comes with macOS (Core Graphics, Core Text
  and ImageIO), so none of that is in the app.
- **Windows:** an app with a window is about 1.3 MB. It draws, lays out
  text and reads pictures with what comes with Windows (Direct2D,
  DirectWrite and WIC).

A program without a window is smaller still.

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
sits next to the `tessel` executable, using its own built-in linker. Set
`TESSEL_LINKER=system` to use the system's linker instead.

## `tessel` and `tessel ide`

```sh
tessel
tessel ide [folder]
```

Opens the [Tessel IDE](/start/ide/) on a folder: the current one, or the one
you name. The first time, `tessel` compiles the IDE and caches it in your
system's temporary folder; it's compiled again only when `tessel` itself
changes.

## `tessel test`

```sh
tessel test [path] [--filter <text>] [--timeout <seconds>]
```

Runs the program's [tests](/guides/testing/) (its `test "name" { … }`
blocks), each in a process of its own, several at a time, and says which
passed and why the others failed. `path` is a file or a folder (the
current folder if left out). `--filter` runs the tests whose name has the
text in it; `--timeout` stops a test after that many seconds (60 unless
given). It ends with exit code 1 if a test failed.

## `tessel add`, `remove` and `deps`

```sh
tessel add textkit                    # the newest published textkit
tessel add textkit@1.2                # 1.2 or a later 1.x
tessel add geokit --git https://github.com/someone/geokit --tag v0.3.0
tessel add mine --path ../mine
tessel remove textkit
tessel deps                           # what the project uses
tessel deps update [library…]         # newer versions, as tessel.toml allows
```

Manage the [libraries](/guides/libraries/) a project uses: `add` and
`remove` change the `[dependencies]` of its `tessel.toml`, and Tessel
works out their versions into `tessel.lock` and downloads them. `check`,
`build`, `run` and the editors do the same when `tessel.toml` and
`tessel.lock` don't match, so a fresh copy of a project builds as it is.

## `tessel search`

```sh
tessel search            # every published library
tessel search text       # those whose name or description has "text"
```

Lists the libraries in the index, each with its newest version and its
description.

## `tessel publish`

```sh
tessel publish --dry-run
tessel publish
```

Publishes the library in the current folder: checks it, tags its commit
`v<version>`, pushes the tag, and opens the page that proposes it to the
library index. See [Publishing](/guides/libraries/#publishing).
`--dry-run` only checks the library. Any other option is an error, and
publishes nothing; `tessel publish --help` shows what it takes.

## `tessel update`

```sh
tessel update            # to the latest release
tessel update --check    # only say whether there's a newer one
tessel update v0.1.2     # to a specific version
```

Replaces the installation this `tessel` runs from (the folder it's in) with
another release, after checking the download against the release's
checksums. See [Updating](/start/installation/#updating). An option it
doesn't know is an error, and updates nothing.

## `tessel telemetry`

```sh
tessel telemetry [on|off]
```

Says what Tessel [reports](/start/installation/#what-tessel-reports) (that
it was installed or updated, and at most once a day that it's in use) and
whether that's on. `tessel telemetry off` turns it off for this
installation, and `tessel telemetry on` turns it back on.

## `tessel pin`

```sh
tessel pin               # this project uses the Tessel you're running
tessel pin 0.1.30        # this project uses Tessel 0.1.30
```

Makes a project use one version of Tessel, whatever version is installed.
It writes the version into a `tessel.toml` file in the current folder (or
updates the one the project already has):

```toml
tessel = "0.1.30"
```

From then on, `tessel check`, `build`, `run`, `preview`, `complete`, `repl`
and `lsp` look for a `tessel.toml` next to the program (or in a folder above
it). When it names another version than the one installed, they hand the
command to that version. The first time, it's downloaded, checked, and
installed next to yours, in the `versions` folder of Tessel's folder
(`~/.tessel` on macOS, `%LOCALAPPDATA%\Tessel` on Windows). Editors and the
IDE use the project's version too, since they run these commands.

`tessel version` isn't handed over: it prints the version installed, and
adds which one the project uses, like `tessel 0.1.36 (this project uses
0.1.38)`.

Commit `tessel.toml` with your code, so everyone working on the project
builds it with the same Tessel. To move the project to a newer version, run
`tessel pin` again with that version.

To try another version for one command without changing the file, put it
first with a `+`, or set `TESSEL_VERSION`:

```sh
tessel +0.1.32 run main.tsl
TESSEL_VERSION=0.1.32 tessel run main.tsl
```

A project without a `tessel.toml` uses the installed Tessel, as before.
The same file says what the app says about itself (its name, version,
icon, …): see [The project file](/start/project/).

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

## `tessel preview`

```sh
tessel preview todos --out /tmp/todos
tessel preview todos --out /tmp/todos --text todos/main.tsl /tmp/unsaved.tsl
```

Makes a picture of an app's window as it first appears, without opening
it, and writes what an editor needs to connect the picture to the code. The
[Tessel IDE](/start/ide/#the-design-panel) uses it for its Design panel.
With `--out /tmp/todos` it writes three files:

| File | |
|---|---|
| `/tmp/todos.png` | The window, at twice its size in pixels. |
| `/tmp/todos.layout.json` | The views in the window, outermost first: each one's `depth` in the tree, `kind` (`Text`, `VStack`, …), place (`x`, `y`, `width`, `height`, in points; a view that doesn't show has no width or height) and `source`: the number of the view expression it was made by, or -1. |
| `/tmp/todos.views.json` | The program's `files`, and its view expressions (`views`), numbered from 0 in the order they're listed. |

A view expression is a view with its modifiers, like
`Text("Hi").font(size: 20)`. For each one, `views.json` gives its `file`
(a number in `files`), where it starts and ends (`lo`, `hi`), its `line`
and `endLine`, its own call (`base`) and its modifiers (`mods`) with the
places of their arguments, for a stack the block that holds its views
(`blockLo`, `blockHi`), and where a new `state` of the `view` or `app` it's
in could be written (`stateAt`, with `indentAt` a statement whose line gives
the indentation; -1 when there's none). Places are counted in characters from the start of
the file, as [`substring`](/reference/stdlib/#string) counts them.

`--text file from` reads a file's text from another file, for editors with
unsaved changes; it can be given several times. `--dark` makes the picture
in the dark appearance (unless the `app` asks for one).

The app's code runs up to its first frame (including `.onAppear` blocks),
inside `tessel`. A program without an `app`, or with errors, makes no
picture: the errors are printed and the command fails.

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

That's the version installed. In a project that [uses another
version](#tessel-pin), the line says so too, since the project's commands
run with that one:

```
tessel 0.1.36 (this project uses 0.1.38)
tessel 0.1.36 (this project uses 0.1.38, not installed yet)
```

The line always starts with `tessel` and the installed version, so tools
that read it aren't affected.

An unknown command, or a command missing its `<path>`, prints the usage
summary and exits with status 1.

## Environment variables

| Variable | Used by | What it does |
|---|---|---|
| `TESSEL_SCRIPT` | Built apps | Runs the app without a window, driven by a script of commands: click, type, press keys, wait, print the view tree, or save a screenshot. See [Testing apps](/ui/testing/). |
| `TESSEL_EXE` | Set by `tessel run` and `tessel ide` | The path of the `tessel` command that started the program. A program can read it with `environment("TESSEL_EXE")`, which is how the IDE finds `tessel`. |
| `TESSEL_PROJECT` | The IDE | The project folder. `tessel ide` sets it; you don't normally need it. |
| `TESSEL_RELEASES` | `tessel update`, `tessel pin` | A folder of releases to download from instead of GitHub's (`<url>/<version>/<file>` and `<url>/latest-<platform>.txt`, for testing a release). |
| `TESSEL_NO_STATS` | every command, the install scripts | Set to `1` to [report nothing](/start/installation/#what-tessel-reports): not the install or update, and not that Tessel is in use. `DO_NOT_TRACK=1` does the same. |
| `TESSEL_VERSION` | Every command `tessel pin` covers | Runs the command with this version of Tessel instead of the project's (installing it the first time). See [`tessel pin`](#tessel-pin). |
| `ANDROID_HOME` | `tessel build --android` | The Android SDK's folder, if it isn't in the usual place. `ANDROID_NDK_HOME` and `JAVA_HOME` name the NDK and the JDK the same way. |
| `TESSEL_ANDROID_KEYSTORE_PASSWORD` | `tessel build --android` | The password of the keystore `tessel.toml` names; `TESSEL_ANDROID_KEY_PASSWORD` is the key's, if it differs. See [Publishing on Google Play](/guides/android/#publishing-on-google-play). |
| `TESSEL_WINDOWS_PARTS` | `tessel build --windows` | A folder with Tessel's runtime for Windows (`libtessel_runtime.a`) and the Windows libraries (`sdk/`) to build with, instead of downloading them. |
| `TESSEL_HOME` | `tessel pin`, libraries | Tessel's folder, where other versions are installed (in `versions/`) and libraries are downloaded (in `packages/`). The install scripts use it too. |
| `TESSEL_INDEX` | Libraries | Another [library index](/guides/libraries/) instead of the public one: a folder on this computer (then `tessel publish` writes its entries there, for trying libraries out) or the address of a `.tar.gz` of one. |
| `TESSEL_NO_UPDATE_CHECK` | The IDE | When set, the IDE doesn't check for a newer Tessel when it opens. |
| `TESSEL_DEBUG_EVENTS` | Built apps | When set, an app prints every window event it receives (mouse, keyboard, resizing) to standard error. Useful when something doesn't react to input. |
| `LLVM_SYS_221_PREFIX` | Building Tessel | Where LLVM 22 is installed; see [Installation](/start/installation/). |

For example, this saves a screenshot of the counter after two clicks,
without opening a window:

```sh
tessel build examples/counter -o counter
TESSEL_SCRIPT='click "+"; click "+"; snapshot counter.png' ./counter
```
