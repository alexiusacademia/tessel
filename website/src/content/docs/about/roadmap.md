---
title: Status and roadmap
description: What works in Tessel today, its known limitations, and what is planned next.
---

Tessel is at version 0.3. It's young: the core of the language and its UI
toolkit work, and they're enough to write real apps (the
[Tessel IDE](/start/ide/) is one), but plenty is missing, and details of the
language may still change. This page is an honest list of where things stand.

## How Tessel is made

Tessel is developed with the help of an AI coding assistant, which is why
its first releases came so quickly. The dates on
[What's new](/about/whats-new/) are the real ones.

## What works today

- **The language:** structs and enums (with methods), functions and
  closures, lists and dictionaries, optionals, `if`, `for` and `match`, and
  value semantics with copy-on-write. Programs made of several files.
- **The compiler:** type checking with clear error messages, and native code
  through LLVM. Built apps are single executables that need nothing else
  installed.
- **Platforms:** macOS (11 and later) and Windows. Both are built and tested
  on every change. Apps can also be built for
  [Android](/guides/android/) (11 and later), which is newer and has so far
  been tried only on an emulator. There's no Linux or iOS support yet.
- **UI:** windows, HiDPI screens, `state` and `bind`, stacks, `Scroll`,
  `Text`, `Button`, `TextField`, `Toggle`, `Picker`, `TabView`,
  `CodeEditor` with syntax colors, `Divider`, over 100 built-in icons, SVG,
  PNG and JPEG pictures, text written in Markdown, keyboard shortcuts, hover
  and pressed states, and the layout and styling modifiers. Every control
  works from the keyboard, and screen readers are told what's in a window. Drawing of your
  own in a `Canvas` (shapes, paths, text and pictures), the mouse in full
  with `.onPointer` (every button, moves, the wheel, pinches), keys with
  `.onKey`, tooltips, pointers, files dropped on the window, and drawings
  saved as pictures. Solid shapes in three dimensions in a `Scene3D`, with
  a camera and lights, drawn by the graphics card. Ribbons of tabs and groups of commands. Context menus, popovers, the menu bar (the system's on
  macOS, drawn in the window elsewhere), more than one window, files opened
  with the app from Finder, and asking before the app closes.
- **Looks:** light and dark appearance that follows the system, colors of
  your own, and animation of views that move, resize, appear or disappear,
  with springs.
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
  language server for other editors, a [debugger](/guides/debugging/) in
  the IDE, VS Code and the terminal, app bundles on macOS, and a headless
  mode for testing apps and taking screenshots.

## Known limitations

:::note
These are the main gaps you may run into today.
:::

- **Debugging:** the [debugger](/guides/debugging/) stops at lines, steps
  and shows variables, but can't change a value, work out an expression, or
  stop on a condition, and doesn't reach apps on Android.
- **Accessibility:** apps tell [screen readers](/ui/accessibility/) what's
  in their windows on macOS and Windows, but text can't be read or edited
  piece by piece through one, menus drawn in the window aren't described,
  and it hasn't been tried with Narrator on Windows. Not on Android yet.
- **Packaging:** `tessel build --bundle` makes a `.app` on macOS, with your
  `icon.png` as its icon, but apps aren't code signed and there are no
  installers.
- **Editor support:** the language server doesn't format code, and the
  Tessel IDE itself doesn't have go-to-definition or rename yet (VS Code
  and other editors do, through the language server).
- **Installation:** packages exist for Apple silicon Macs and 64-bit
  Windows only (no Intel Macs yet). The macOS package is signed and
  notarized; the Windows one isn't signed yet, so Windows SmartScreen may
  warn about a downloaded `tessel.exe`.

## Planned

These are the next areas of work. None of them has a date.

| Area | Plans |
|---|---|
| App polish | Fuller accessibility: reading and editing text through a screen reader, and Android. |
| Standard library | Filling gaps as apps need them. |
| Language | Packages: installing shared modules by name, with versions. |
| Packaging | Code signing and installers. |
| Debugging | Changing values and conditions on breakpoints in the debugger. |

Tessel's goals and the reasons for its design are on
[Tessel's design](/about/design/).
