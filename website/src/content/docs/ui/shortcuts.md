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

- **One character** that isn't an uppercase letter or a space, such as
  `"s"`, `"1"` or `","`. Write letters in lowercase.
- **The name of a special key:** `"enter"`, `"tab"`, `"escape"`,
  `"backspace"`, `"delete"`, `"left"`, `"right"`, `"up"`, `"down"`, `"home"`
  or `"end"`.

Before the key, `shift+` or `alt+` (Option on a Mac), or both, ask for those
keys to be held too: `.shortcut("shift+z")` runs on Shift+Cmd+Z. A shortcut
without them also runs when they're held (`.shortcut("s")` runs on
Shift+Cmd+S too), unless another shortcut asks for exactly the keys held.

Only Cmd (or Ctrl) combinations can be shortcuts. For keys on their own, or
with other modifiers, use [`.onKeyEvent`](/reference/modifiers/#onkeyevent).
Items of the [menu bar](/ui/menus-windows/#the-menu-bar) have shortcuts too
(`MenuItem("Save As…", shortcut: "cmd+shift+s")`), and menu bar shortcuts
come before `.shortcut`s.

## Several shortcuts on one view

Each `.shortcut` adds one, so one view can carry a whole menu's worth:

```tessel
app Editor(width: 400, height: 200) {
    state log: [String] = []

    VStack(alignment: .leading) {
        for line in log {
            Text(line)
        }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .padding(12)
    .shortcut("z") { log.append("undo") }
    .shortcut("shift+z") { log.append("redo") }
    .shortcut("s") { log.append("save") }
    .shortcut("alt+shift+s") { log.append("save a copy") }
}
```

Cmd+Z undoes and Shift+Cmd+Z redoes: the `"shift+z"` shortcut asks for
exactly Shift, so it wins over `"z"` when Shift is held. Shift+Cmd+S still
saves, as no shortcut asks for `"shift+s"`.

`tessel check` checks the key, so a shortcut that could never be pressed is
an error. Cmd is added for you, so don't write it:

```text
error: `cmd+s` isn't a key a shortcut can use
  = help: Cmd (Ctrl on Windows) is added for you: write `.shortcut("s")`
```

`"S"` gets a hint to use the lowercase letter, and anything else, such as
`"F5"`, lists the keys you can use.

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
`key cmd+s` (`cmd` and `ctrl` mean the same there), or `key cmd+shift+z`.
