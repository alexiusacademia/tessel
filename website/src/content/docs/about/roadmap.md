---
title: Status and roadmap
description: What works in Tessel today, its known limitations, and what is planned next.
---

Tessel is at version 0.1. It's young: the core of the language and its UI
toolkit work, and they're enough to write real apps (the
[Tessel IDE](/start/ide/) is one), but plenty is missing, and details of the
language may still change. This page is an honest list of where things stand.

## What works today

- **The language:** structs and enums (with methods), functions and
  closures, lists and dictionaries, optionals, `if`, `for` and `match`, and
  value semantics with copy-on-write. Programs made of several files.
- **The compiler:** type checking with clear error messages, and native code
  through LLVM. Built apps are single executables that need nothing else
  installed.
- **Platforms:** macOS (11 and later) and Windows. Both are built and tested
  on every change. There's no Linux support yet.
- **UI:** windows, HiDPI screens, `state` and `bind`, stacks, `Scroll`,
  `Text`, `Button`, `TextField`, `Toggle`, `Picker`, `TabView`,
  `CodeEditor` with syntax colors, `Divider`, about 60 built-in icons, SVG,
  PNG and JPEG pictures, text written in Markdown, keyboard shortcuts, hover
  and pressed states, and the layout and styling modifiers. Drawing of your
  own in a `Canvas` (shapes, paths, text and pictures), the mouse in full
  with `.onPointer` (every button, moves, the wheel, pinches), keys with
  `.onKey`, tooltips, pointers, files dropped on the window, and drawings
  saved as pictures. Context menus, popovers, the menu bar (the system's on
  macOS, drawn in the window elsewhere), more than one window, files opened
  with the app from Finder, and asking before the app closes.
- **Looks:** light and dark appearance that follows the system, colors of
  your own, and animation of views that move, resize or fade.
- **Text editing:** selecting with the mouse and keyboard, word and line
  movement, copy, cut, paste, undo and redo, Tab between fields, and input
  methods (Chinese, Japanese, Korean, accents with dead keys).
- **Background work:** timers (`after` and `every`), HTTP requests,
  downloads and WebSockets, running other programs, and heavy computations on
  another thread (`background`), all with completion blocks that run on the
  UI thread. They work in programs with only `fn main` too.
- **Language:** structs, enums with values, interfaces, generics, closures, optionals,
  errors with `Result` and `try`, tuples and sets, `for` and `while` loops
  with `break` and `continue`, and programs split over any number of files,
  with modules (`import`, `public`, `private`) for code shared between
  programs.
- **Standard library:** files and folders, JSON and binary encoding of your
  own types, `Data` for raw bytes (with the numbers that binary files hold,
  and compression), dates and times with time zones and
  calendar arithmetic, random numbers, the
  clipboard, reading standard input and writing to standard error, and many
  methods for strings, numbers and lists.
- **Tools:** the `tessel` command, a REPL (`tessel repl`), hot reload
  (`tessel run --hot`, and "Reload on save" in the IDE), the Tessel IDE
  (with this documentation built in, a Design panel for building a
  window by pointing at it, and a preview for Markdown files), a VS Code extension, a
  language server for other editors, app bundles on macOS, and a headless
  mode for testing apps and taking screenshots.

## Known limitations

:::note
These are the main gaps you may run into today.
:::

- **Debugging:** there is no debugger with breakpoints or stepping yet.
  Runtime errors show a message and the place in your code, which you can
  click in the IDE's console, and `print` output appears there too.
- **Keyboard:** Tab moves between text fields, but buttons, toggles and
  pickers can't be used from the keyboard.
- **Appearance:** animation covers moving, resizing and fading; there are no
  springs or custom curves, and a view that disappears goes at once. There's
  no accessibility support (screen readers) yet.
- **Packaging:** `tessel build --bundle` makes a `.app` on macOS, with your
  `icon.png` as its icon, but apps aren't code signed and there are no
  installers.
- **Editor support:** the language server reports errors, shows types on
  hover and completes code, but doesn't offer go-to-definition or rename.
- **Installation:** packages exist for Apple silicon Macs and 64-bit
  Windows only (no Intel Macs yet). The macOS package is signed and
  notarized; the Windows one isn't signed yet, so Windows SmartScreen may
  warn about a downloaded `tessel.exe`.

## Planned

These are the next areas of work. None of them has a date.

| Area | Plans |
|---|---|
| App polish | Accessibility, using every control from the keyboard, richer animation. |
| Standard library | Filling gaps as apps need them. |
| Language | Packages: installing shared modules by name, with versions. |
| Packaging | Code signing and installers. |
| Debugging | Stepping through code with breakpoints, building on the clickable errors and console the IDE has today. |

Tessel's goals and the reasons for its design are on
[Tessel's design](/about/design/).
