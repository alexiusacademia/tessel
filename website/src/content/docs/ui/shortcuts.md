---
title: Keyboard shortcuts
description: Add Cmd (macOS) or Ctrl (Windows) keyboard shortcuts to views with .shortcut, and learn which editing shortcuts are built in.
---

`.shortcut(key) { … }` runs a block when the user presses the platform's
shortcut key together with `key`: **Cmd** on macOS, **Ctrl** on Windows.

```tessel
app Editor(width: 400, height: 240) {
    state text = ""
    state status = ""

    HStack {
        Button("Save") { save() }
            .shortcut("s") { save() }
        Text(status).color(.secondary)
    }
    TextField("Text", text: text)

    fn save() {
        status = if writeFile(path: "draft.txt", text: text) { "Saved" } else { "Couldn't save" }
    }
}
```

Here Cmd+S (Ctrl+S on Windows) saves, just like clicking the button.

## How shortcuts work

A shortcut is attached to a view, and it's active while that view is part of
the window:

- A view left out by an `if`, or on a tab that isn't selected in a
  [`TabView`](/ui/tabs/), has no active shortcuts.
- A view that is [hidden or disabled](/ui/styling/#hiding-and-disabling) (or
  inside one that is) doesn't respond to its shortcut either. So
  `.disabled(…)` turns off a button's click and its shortcut together.

The shortcut has its own block; it doesn't click the view it's attached to. It's
usually attached to the button that does the same thing, so it comes and goes
with the button, but you can put it on any view. If two active views have
the same shortcut, the first one in the window (from top to bottom through
your code) wins.

The view doesn't need to be focused or under the mouse: shortcuts work
wherever the keyboard focus is.

## Which keys

The key is a string:

- **A letter, digit or punctuation character**, such as `"s"`, `"1"` or
  `","`. Letters are case-insensitive: `"S"` is the same as `"s"`.
- **The name of a special key:** `"enter"`, `"tab"`, `"escape"`,
  `"backspace"`, `"delete"`, `"left"`, `"right"`, `"up"`, `"down"`, `"home"`
  or `"end"`.

Only Cmd (or Ctrl) combinations can be shortcuts. There's no way to require
Shift or Option as well, or to use a key on its own without Cmd.

:::caution
The key isn't checked when your program is compiled. A key string that doesn't
match anything, such as `"cmd+s"` or `"F5"`, is accepted but never runs.
:::

## Built-in editing shortcuts

While a text field or code editor is being edited, these shortcuts belong to
it and take priority over your own:

| Shortcut | Does |
|----------|------|
| Cmd+A | Select all |
| Cmd+C | Copy |
| Cmd+X | Cut |
| Cmd+V | Paste |
| Cmd+Z | Undo |
| Cmd+Shift+Z | Redo |

When no field is being edited, a `.shortcut("z")` or `.shortcut("c")` of your
own does run.

Cmd with the arrow keys, Backspace and Delete also has an editing meaning in
fields (moving and deleting to the start or end of the line), but a
`.shortcut("left")` or similar takes priority over those. See
[Editing text](/ui/controls/#editing-text) for all the editing keys.

## An example: the IDE's toolbar

The [Tessel IDE](/start/ide/) attaches its shortcuts to its toolbar buttons.
Because Run and Stop are shown one at a time with `if`, Cmd+R runs the
program or stops it, depending on which button is showing:

```tessel
ToolButton(icon: .save, label: "Save") { save() }.shortcut("s") { save() }
ToolButton(icon: .circleCheck, label: "Check") { check() }.shortcut("k") { check() }
if running == nil {
    ToolButton(icon: .play, label: "Run", tint: .green) { runOrStop() }.shortcut("r") { runOrStop() }
} else {
    ToolButton(icon: .stop, label: "Stop", tint: .red) { runOrStop() }.shortcut("r") { runOrStop() }
}
```

To try shortcuts in a [headless test script](/ui/testing/), use
`key cmd+s` (`cmd` and `ctrl` mean the same there).
