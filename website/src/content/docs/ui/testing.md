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
| `click shift "Alan"` | Clicks with Shift held (or `cmd`): for selecting a [table](/ui/tables/)'s rows. |
| `click button 2` | Clicks the 2nd view of a kind: `button`, `toggle`, `field`, `text`, `editor`, `slider`, `stepper` or `colorpicker`. Counting starts at 1. |
| `hover "Save"` | Moves the mouse pointer over the first view with this label, to show [hover backgrounds](/ui/styling/#hover-backgrounds). |
| `hover 40 60` | Moves the pointer to a point. |
| `close "main.tsl"` | Clicks the close button of the tab with this title. |
| `type hello world` | Types text into the text field or code editor being edited. |
| `type "two words "` | The same, in quotes to keep spaces at the start or end. |
| `key enter` | Presses a key, optionally with modifiers (see below). `key shift` presses a modifier on its own. |
| `keyup space` | Lets a key go, for [`.onKeyEvent(releases: true)`](/reference/modifiers/#onkeyevent). |
| `drag 150 100 40 0` | Presses the mouse at a point and moves it by 40 points across and 0 down, for views with [`.onDrag`](/reference/modifiers/#ondrag), sliders and scroll bars. |
| `dragto "Roads" "Parcels"` | Drags the view with [`.draggable`](/reference/modifiers/#draggable) at the first label onto the second, and lets go there (or `dragto 40 60 40 200`, with points). |
| `scroll 0 120` | Scrolls the scroll view (or code editor) under the middle of the window down by 120 points. A negative number scrolls up. |
| `wheel 40 60 0 120` | The same, for what's at a point: here (40, 60). |
| `press 40 60` | Presses the mouse on a view with [`.onPointer`](/reference/modifiers/#onpointer) or [`.draggable`](/reference/modifiers/#draggable) (also `press "Label"`). Add `right` or `middle` for another button. |
| `move 80 60` | Moves the mouse there (or `move "Label"`): a drag while it's pressed. |
| `release` | Lets the mouse button go (`release 90 60`: somewhere else). |
| `rightclick 40 60` | A click with the right button, on a view with `.onPointer`, or one with a [context menu](/ui/menus-windows/#context-menus), which opens (`rightclick "Roads"`: on a labelled view). |
| `doubleclick 40 60` | Two clicks in a row there. |
| `pinch 40 60 0.2` | A trackpad pinch that makes things a fifth larger. |
| `mods shift+alt` | Holds modifier keys for the mouse commands that follow; `mods none` lets them go. |
| `drop "/tmp/a.csv"` | Drops a file on the window, for [`.onDrop`](/reference/modifiers/#ondrop). |
| `open "/tmp/a.note"` | The system asks the app to open a file, for [`.onOpenFile`](/reference/modifiers/#onopenfile). |
| `menu "File" "Save"` | Chooses an item of the [menu bar](/ui/menus-windows/#the-menu-bar), by the titles of its menus and its own. |
| `menus` | Prints the menu bar: every menu and item, with shortcuts. |
| `menubar drawn` | Draws the menu bar in the window, as on Windows (`menubar native`: as on macOS, outside the window). Scripts start as the platform does. |
| `close window` | The user closes the window: [`.onCloseRequest`](/reference/modifiers/#oncloserequest) runs, or it closes. |
| `quit` | The user quits the app (Cmd+Q). |
| `window "Table"` | Sends the commands after it to another [window](/reference/views/#window) (`window main`: back to the main one). |
| `answer "notes/a.note"` | Sets the path the next [file dialog](#file-dialogs) returns. |
| `answer cancel` | Makes the next file dialog return `nil`, as if the user cancelled. |
| `wait 1.5` | Lets 1.5 seconds pass, running any timers that are due. |
| `waitfor "Done"` | Waits until a view with this label is there (for up to a minute): for what a program started with [`runCommand`](/reference/stdlib/#runcommand) and `input: true` brings about, which `wait` doesn't wait for. |
| `repeat 20 key down` | Runs a command several times. |
| `compose "ni"` | Text an [input method](/ui/controls/#input-methods) is composing in the field being edited; `tree` shows it. A `type` after it is what it became. |
| `reload` | With [`tessel run --hot`](/start/cli/#hot-reload): takes the program's new code now, if its files changed. (A scripted run doesn't watch the files by itself.) |
| `appearance dark` | Switches to the dark (or `light`) [appearance](/ui/styling/#light-and-dark). Scripted runs start light, whatever the system's setting, unless the `app` asks for one. |
| `animations on` | Turns [animations](/ui/styling/#animation) on. Scripted runs have them off, so views are at once where they end up; with them on, `wait` lets them move, and `dump` and `snapshot` show them on their way. |
| `snapshot out.png` | Renders the window to a PNG file, at twice the size in pixels (like a Retina screen). |
| `layout out.json` | Writes the views on screen to a file as JSON: each one's depth in the tree, kind and place. [`tessel preview`](/start/cli/#tessel-preview) uses it. |
| `tree` | Prints the view tree, with the open menu after it, the menu bar's titles first, and the app's other open windows last. |
| `dump` | Prints the view tree with each view's position and size. |
| `accessibility` | Prints what a [screen reader](/ui/accessibility/) is told is in the window: each control and text, with what it's called and what it holds. |

Before each command, the app is brought up to date: state changes from the
previous command are applied, the views are rebuilt and laid out. `.onAppear`
blocks have already run when the first command starts.

### Clicking

`click "label"` looks for the first view, from top to bottom through the tree,
whose label is exactly the given text:

- a `Text`'s text, a `Button`'s or `Toggle`'s label;
- a `TextField`'s placeholder;
- an `Icon`'s name, such as `click "trash"` for `Icon(.trash)`;
- a tab's title in a `TabView`, or a `DisclosureGroup`'s title;
- a color in an open `ColorPicker`'s palette, by its hex code, like
  `click "#000000"`;
- failing all of those, any view's
  [`.accessibilityLabel`](/reference/modifiers/#accessibilitylabel) or
  [`.tooltip`](/reference/modifiers/#tooltip), for views with no text, like
  a toolbar of `Svg` pictures or a `Canvas`.

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
gets the same automatic indentation as real typing does. The text goes in as
one edit, as a paste does, so the field's `onChange` runs once for it. `click`
on a toggle or a picker's option, and `key` on one with the keyboard, run their
`onChange` as a person's clicks and keys do. While no field is
being edited, each character is a key press instead, as it would be on a
keyboard (with Shift for capitals and for symbols like `%`), so it reaches
`.onKeyEvent` and `.onKey`.

`key` presses one key: `backspace`, `delete`, `left`, `right`, `up`, `down`,
`home`, `end`, `pageup`, `pagedown`, `enter`, `tab`, `escape`, `space`, `f1`
to `f12`, or a character, which types it in a field being edited. Add
modifiers in front, joined with `+`:

```text
key shift+left       select one character to the left
key alt+backspace    delete a word
key cmd+a            select all
key cmd+shift+z      redo
key cmd+s            run a .shortcut("s")
key shift+a          type "A" (or an .onKeyEvent gets "a" with shift)
key shift+5          type "%", as on a US keyboard (an .onKeyEvent gets "5")
key repeat+down      a key held down (repeated is true)
```

`cmd` and `ctrl` mean the same thing: the platform's shortcut key (Cmd on
macOS, Ctrl on Windows). `alt` is Option on macOS. So scripts behave the same on
both systems. `control` is the Ctrl key itself, as
[`KeyEvent`](/reference/colors/#keyevent)'s `control` sees it.

The clipboard in a headless run is private to the script: `cmd+c` and `cmd+v`
work, but never touch your real clipboard. `copyToClipboard` and
`clipboardText` use that same private clipboard, so text copied with one can
be pasted with the other. `openURL` doesn't open anything; it prints
`(open …)` instead.

### Time and background work

In a headless run, the clock stands still unless you move it. `wait 2` moves it
forward by two seconds and runs every `after` and `every` timer that comes
due, in order (including timers those timers start). So a test of a
10-second countdown finishes instantly.

`now()` reads the same clock: it starts at the real time when the script
starts, and moves only with `wait`, so each timer sees the time it came due
at. Code that works out how far along something is from `now()`, like an
animation of its own, is tested the same way as one that counts timer
ticks. (A loop that waits for `now()` to move never ends in a headless run;
use `after` instead.)

`wait` also first lets any running `fetch`, `request`, `download`,
`runCommand` and `background` calls finish and delivers their callbacks. Use
`wait 0` after starting one to wait for its result, without moving the
clock:

```text
click "Check"
wait 0
tree
```

An open WebSocket doesn't finish by itself, so `wait` doesn't wait for it to
close. Instead, while one is open, `wait` gives its messages real time to
arrive: half a second, or the wait's length if that's longer.

### File dialogs

A headless run never opens a window, so it can't show an
[open or save dialog](/ui/async/#documents-and-custom-file-types) either.
Instead, `answer` says in advance what the next `openFile` or `saveFile`
call returns:

```text
answer "tests/notes/groceries.note"
click "Save…"
answer cancel
click "Open…"
tree
```

Here the save dialog "chooses" `tests/notes/groceries.note`, and the open
dialog is cancelled. Each `answer` is used by one dialog, in order, so you can
queue several before a click that opens more than one. A dialog with no
`answer` waiting returns `nil`, like a cancelled one.

A relative path is relative to the folder the app was started from. Like the
real save dialog, a `saveFile` answer without the file type's extension gets
it added (`answer "report"` with `types: ["och"]` saves `report.och`).

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
Slider = 0.7 focused
Stepper = 1.5
ColorPicker = #3478f6 open
DisclosureGroup "Layers" open
LazyVStack 100000 rows, built 1990..2020
HStack dragged
```

`focused` marks the control the keyboard is on; during a drag, `dragged`
marks the view being dragged and `drop target` the one under the mouse.

`dump` adds `[x,y widthxheight]` in points after each view, which is useful for
checking a [layout](/ui/layout/). Anything your program prints with `print`
appears in the same output, in order.

A [`Canvas`](/reference/views/#canvas) shows what its block drew, like
`Canvas (2 fills, 1 stroke, 3 texts)`, and a [tooltip](/reference/modifiers/#tooltip)
that's showing is printed after the tree: `Tooltip "Delete the selected rows"`
(it comes up when a `wait` of 0.6 seconds or more follows a `hover`).

On a view with `.onPointer`, `click`, `drag` and `hover` send it the events a
real mouse would: a press and a release, a press, a move and a release, or a
move. `key x` (or `type x`) while no field is being edited goes to
[`.onKeyEvent`](/reference/modifiers/#onkeyevent) and
[`.onKey`](/reference/modifiers/#onkey).

`press` on a view with `.draggable`, then `move`, holds it over what's
there (a `tree` or `snapshot` shows the drag on its way), and `release`
drops it; `key escape` before the release cancels it. `dragto` does it all
in one go:

```text
dragto "Roads" "Parcels"
press "Imagery"
move "Rivers"
snapshot dragging.png
release
```

`snapshot` renders exactly what the window would show, including the text
cursor and selection (the screenshots on this site were made with it).

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
