---
title: Testing your UI
description: Run a Tessel app without a window, drive it with a script of clicks and key presses, and check the view tree or take screenshots.
---

Every Tessel app can run *headless*: without opening a window, driven by a
script of clicks and key presses. It can print the view tree so you can check
what's on screen, and render screenshots. That makes UI tests easy to write and
fast to run, and it's how Tessel tests its own UI and IDE.

## A first session

Take the counter app:

```tessel
app Counter(width: 320, height: 200) {
    state count = 0

    VStack(spacing: 12) {
        Text("Count: {count}")
            .font(size: 32, weight: .bold)

        HStack(spacing: 8) {
            Button("-") { count -= 1 }
            Button("+") { count += 1 }
        }
    }
    .padding(24)
}
```

Build it, then run it with the `TESSEL_SCRIPT` environment variable set to a
script. Commands are separated by `;` or new lines:

```sh
tessel build counter -o counter-app
TESSEL_SCRIPT='tree; click "+"; click "+"; dump' ./counter-app
```

Instead of opening a window, the app runs the script and prints:

```text
Root
  View
    VStack
      Text "Count: 0"
      HStack
        Button "-"
        Button "+"
Root [0,0 320x200]
  View [73,37 174x126]
    VStack [73,37 174x126]
      Text "Count: 2" [97,61 126x38]
      HStack [121,111 78x28]
        Button "-" [121,111 34x28]
        Button "+" [163,111 36x28]
```

`tree` printed the views, `click "+"` pressed the plus button twice, and `dump`
printed the views again, this time with their positions. When the script ends,
the app exits.

`tessel run` passes the environment on to your program, so
`TESSEL_SCRIPT='…' tessel run counter` works too. Without `TESSEL_SCRIPT`, it
opens a window as usual.

## Script commands

| Command | What it does |
|---------|--------------|
| `size 400 300` | Sets the window size, in points. The app starts at the size its `app` declaration gives. |
| `click "Save"` | Clicks the first view with this label (see below). |
| `click 40 60` | Clicks at a point, in points from the window's top left corner. |
| `click button 2` | Clicks the 2nd view of a kind: `button`, `toggle`, `field`, `text` or `editor`. Counting starts at 1. |
| `hover "Save"` | Moves the mouse pointer over the first view with this label, to show [hover backgrounds](/ui/styling/#hover-backgrounds). |
| `hover 40 60` | Moves the pointer to a point. |
| `close "main.tsl"` | Clicks the close button of the tab with this title. |
| `type hello world` | Types text into the text field or code editor being edited. |
| `type "two words "` | The same, in quotes to keep spaces at the start or end. |
| `key enter` | Presses a key, optionally with modifiers (see below). |
| `scroll 0 120` | Scrolls the scroll view (or code editor) under the middle of the window down by 120 points. A negative number scrolls up. |
| `wait 1.5` | Lets 1.5 seconds pass, running any timers that are due. |
| `repeat 20 key down` | Runs a command several times. |
| `snapshot out.png` | Renders the window to a PNG file, at twice the size in pixels (like a Retina screen). |
| `tree` | Prints the view tree. |
| `dump` | Prints the view tree with each view's position and size. |

Before each command, the app is brought up to date: state changes from the
previous command are applied, the views are rebuilt and laid out. `.onAppear`
blocks have already run when the first command starts.

### Clicking

`click "label"` looks for the first view, from top to bottom through the tree,
whose label is exactly the given text:

- a `Text`'s text, a `Button`'s or `Toggle`'s label;
- a `TextField`'s placeholder;
- an `Icon`'s name, such as `click "trash"` for `Icon(.trash)`;
- a tab's title in a `TabView`.

It clicks the middle of that view. Hidden views, and pages of a `TabView` that
aren't selected, are skipped.

Clicking a text field or code editor starts editing it, as in a real window, so
`click "Name"` followed by `type Sam` fills in the field with the placeholder
"Name".

When there's nothing to click, the script stops with an error.

`hover "label"` finds a view the same way, but only moves the pointer over
its middle without clicking, so a `snapshot` afterwards shows its
[hover background](/ui/styling/#hover-backgrounds).

### Typing and keys

`type` inserts text at the cursor of the field being edited. Everything after
`type ` is typed; put it in quotes to keep leading or trailing spaces. Because
`;` separates commands, typed text can't contain one. Typing into a code editor
gets the same automatic indentation as real typing does.

`key` presses one key: `backspace`, `delete`, `left`, `right`, `up`, `down`,
`home`, `end`, `enter`, `tab` or `escape`, or a letter (for shortcuts). Add
modifiers in front, joined with `+`:

```text
key shift+left       select one character to the left
key alt+backspace    delete a word
key cmd+a            select all
key cmd+shift+z      redo
key cmd+s            run a .shortcut("s")
```

`cmd` and `ctrl` mean the same thing: the platform's shortcut key (Cmd on
macOS, Ctrl on Windows). `alt` is Option on macOS. So scripts behave the same on
both systems.

The clipboard in a headless run is private to the script: `cmd+c` and `cmd+v`
work, but never touch your real clipboard.

### Time and background work

In a headless run, the clock stands still unless you move it. `wait 2` moves it
forward by two seconds and runs every `after` timer that comes due, in order
(including timers those timers start). So a test of a 10-second countdown
finishes instantly.

`wait` also first lets any running `fetch` and `runCommand` calls finish and
delivers their callbacks. Use `wait 0` after starting one to wait for its
result, without moving the clock:

```text
click "Check"
wait 0
tree
```

### Output

`tree` and `dump` print one line per view, indented by nesting. Custom views and
the body of the app show up as `View`. Values are shown where it helps:

```text
TextField "What needs doing?" = "Buy milk"
Toggle "Gift wrap" = true
TabView ["Problems", "Console"] selected 0
CodeEditor errors [2] = "app Hello {\n    Text(\"hi\")\n}"
Icon trash
Text "secret" hidden
Button "Send" disabled
```

`dump` adds `[x,y widthxheight]` in points after each view, which is useful for
checking a [layout](/ui/layout/). Anything your program prints with `print`
appears in the same output, in order.

`snapshot` renders exactly what the window would show, including the text
cursor and selection. It needs a GPU (the screenshots on this site were made
with it).

If a command is unknown, or a `click`, `close` or `hover` finds nothing, the app prints
an error starting with `script:` and exits with code 2. Otherwise it exits with
code 0 when the script is done.

## Writing tests

A simple way to test an app is to keep a script and the output you expect,
and compare:

```sh
tessel build todos -o todos-app
TESSEL_SCRIPT="$(cat todos.script)" ./todos-app > todos.actual
diff todos.out todos.actual
```

That's exactly how Tessel's own tests work. In the repository, each
`tests/run/app_*.tsl` program has:

- `app_name.script`, the script to run;
- `app_name.out`, the expected output.

The test runner (`cargo test`, in `crates/tessel-cli/tests/run.rs`) builds each
program, runs it from the repository root with `TESSEL_SCRIPT` set to the
script, and fails if the output differs from the `.out` file or anything is
printed to standard error. On macOS it also checks for memory leaks. For
example, this is `tests/run/app_todos.script`, which adds to-dos (with a typo
fixed by Backspace), ticks one off and tries the filter:

```text
click "What needs doing?"
type Write the spec
click "Add"
click "What needs doing?"
type Build M3
click "Add"
click "What needs doing?"
type Ship itt
key backspace
key enter
click "Add"
click "Add"
tree
click toggle 1
click "Active"
tree
click "Done"
tree
```

The [Tessel IDE](/start/ide/) is tested the same way:
`tests/ide/session.script` creates a file, types code into the editor, undoes,
saves with `cmd+s`, renames and deletes files, and checks the result.
