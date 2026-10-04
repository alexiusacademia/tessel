---
title: What's new
description: What each release of Tessel added and fixed, newest first.
---

What each release of Tessel brought, newest first. To get the latest, run
`tessel update` (or take the update the IDE offers). See
[Installation](/start/installation/) if you don't have Tessel yet.

## 0.1.33

*October 4, 2026*

- **Much smaller apps on Windows.** Apps draw, lay out text and read
  pictures with what comes with Windows (Direct2D, DirectWrite and WIC), as
  they do on macOS: a one-`Text` app is about 1 MB instead of 7.5 MB. See
  [How big an app is](/start/cli/#how-big-an-app-is).
- **Blend modes on Windows:** layers drawn with
  `g.beginLayer(blend:)` blend as they do on macOS.
- **A project chooses its version of Tessel.** `tessel pin` writes the
  version into the project's `tessel.toml`, and from then on Tessel's
  commands, editors and the IDE use that version for the project,
  installing it the first time. See [`tessel pin`](/start/cli/#tessel-pin).
- **Fixed on Windows:** appending many bytes to a `Data` could damage it.

## 0.1.32

*October 4, 2026*

- **Functions can change the caller's value.** A parameter declared `bind`
  is the caller's variable, field or list item itself, not a copy, as it
  already was for views: `fn addBonus(scores: bind [Int])`. Methods can have
  them too. See
  [Changing the caller's value](/language/values/#changing-the-callers-value).
- **Fixed:** changing a loop's variable through a closure (or a `bind`
  argument) changed a copy of the list, not the list.

## 0.1.31

*October 4, 2026*

- **New controls:** `Slider`, `Stepper`, `ColorPicker`, `DisclosureGroup`
  (for trees) and `LazyVStack` (for long lists). See
  [Controls](/ui/controls/#slider-stepper-and-colorpicker).
- **Drag and drop** between views with `.draggable` and `.dropTarget`, and
  every key with `.onKeyEvent`. Keyboard shortcuts can ask for Shift and
  Alt: `.shortcut("shift+z")`.
- **Background jobs** report their progress and can be cancelled.
- **Drawing:** pictures made from pixels, gradients, layers that blend with
  what's under them, and SVGs on a `Canvas`. See [Drawing](/ui/drawing/).
- **Menus and windows:** context menus, popovers, the menu bar (the
  system's on macOS), more than one window, asking before a window closes,
  and files opened with the app. See
  [Menus, windows and documents](/ui/menus-windows/).

## 0.1.30

*October 4, 2026*

- **Language:** bit operations, hex and binary number literals, `match` arms
  with several patterns, tuples in `if let`, and functions declared inside
  functions. See [Basics](/language/basics/) and
  [Functions](/language/functions/).
- `Data` can have bytes appended to it.

## 0.1.29

*October 3, 2026*

- **Fixed on Windows:** the REPL, hot reload and previews could crash.
- **Fixed on Windows:** endless recursion is reported as a Tessel error, as
  on macOS, instead of ending the program.

## 0.1.28

*October 3, 2026*

- **Much smaller apps on macOS.** Apps draw, lay out text and read pictures
  with what comes with macOS, so a one-`Text` app is about 1 MB instead of
  8.3 MB.
- Apps carry SVG, pictures and Markdown only when they use them, which
  makes Windows apps smaller too. See
  [How big an app is](/start/cli/#how-big-an-app-is).

## 0.1.27

*October 3, 2026*

- **Fixed:** Windows apps carried all of Tessel (a one-`Text` app was
  24 MB). The parts an app doesn't use are left out now.

## 0.1.26

*October 3, 2026*

- **IDE:** copy, cut, paste and duplicate views in the Design panel, and an
  outline of the views.
- A click inside a view with `.onKey` gives it the keyboard.

## 0.1.25

*October 3, 2026*

- **IDE:** the Design panel has an inspector, undo and redo, and more views
  to add (`TextField`, `Toggle`, `Picker`). See
  [The Design panel](/start/ide/#the-design-panel).
- A code editor's undo history includes the changes the program makes to
  its text. New: `undoTextEdit()` and `redoTextEdit()`.
- `tessel preview --dark`.

## 0.1.24

*October 3, 2026*

- **Fixed:** `pow(10, x)` failed to build on macOS.

## 0.1.23

*October 3, 2026*

- **IDE:** a Preview panel shows Markdown files as they'll look, next to
  their text.

## 0.1.22

*October 2, 2026*

- **IDE: the Design panel.** A picture of your app next to its code, where
  views can be selected, changed, added, moved and deleted. Every change is
  written into the code. See [The Design panel](/start/ide/#the-design-panel).
- `tessel preview` makes a picture of an app's window for editors. See
  [`tessel preview`](/start/cli/#tessel-preview).
- `appendData` adds bytes to the end of a file.

## 0.1.21

*October 2, 2026*

- **IDE:** drag the edges of the explorer and the bottom panel to resize
  them.
- `readImage` gives a picture file's size and pixels.

## 0.1.20

*October 2, 2026*

- **Canvas:** draw views of your own with paths, shapes, text and pictures,
  and save drawings as pictures. See [Drawing and the mouse](/ui/drawing/).
- **The mouse and keys:** `.onPointer`, `.onKey`, `.cursor`, `.onDrop`,
  `.onResize`, `.tooltip`, `.offset` and `.clipped`.
- `Data` reads numbers, slices and Latin-1 text, and compresses.
  `list += other` appends in place.
- **Fixed:** stopping `tessel run` left the program it started running.

## 0.1.19

*October 2, 2026*

- **Scroll bars** in scroll views and code editors, which show when there's
  more than fits and can be dragged.
- Releases are for macOS and Windows together from now on.

## 0.1.18

*October 2, 2026*

- **Fixed:** a double click in a code editor or text field could select
  only part of a word.

## 0.1.17

*October 2, 2026*

- `.onDrag`: views that respond to being dragged with the mouse.
- **IDE:** drag the edge of the Docs panel to resize it.

## 0.1.16

*October 2, 2026*

- **IDE:** search in the Docs panel.

## 0.1.15

*October 2, 2026*

- **A `Markdown` view:** text with headings, lists, code, tables, pictures
  and links.
- **IDE:** the documentation comes with it. The Docs button opens it next
  to the editor, without a network.

## 0.1.14

*October 2, 2026*

- `CodeEditor`: taller lines, and more room around the line numbers and
  the code.

## 0.1.13

*October 1, 2026*

- **REPL:** an `import` stays for the rest of the session, so a project's
  modules can be tried out at the prompt. `:keywords` lists the keywords.

## 0.1.12

*October 1, 2026*

- **Time zones** for `date`, `formatDate` and `parseDate`, and calendar
  dates: `dateParts`, `addToDate`, `daysBetween` and more.
- `cancelRequest` gives up a `request` or `download`. WebSockets can send
  and receive bytes.

## 0.1.11

*October 1, 2026*

- **REPL:** errors point at the right column, and every entry has its own
  number. The REPL has [its own page](/start/repl/).

## 0.1.10

*October 1, 2026*

- **`tessel repl`:** type code and see what it does, a line at a time. See
  [The REPL](/start/repl/).
- **Hot reload:** `tessel run --hot` gives a running app its new code when
  you save, and keeps its state. In the IDE, "Reload on save".

## 0.1.9

*October 1, 2026*

- `Image` shows PNG and JPEG files.
- **Light and dark:** apps follow the system, or choose one with
  `app Name(appearance: .dark)`. Colors of your own with `.rgb` and
  `.rgba`.
- `.animation()`: views move, resize and fade to their new state.
- Tab and Shift+Tab move between text fields, and input methods (Chinese,
  Japanese, Korean, accents) work in them.

## 0.1.8

*October 1, 2026*

- `print` shows lists, dictionaries, sets and tuples. `enumerated()`, `zip`,
  and `for (key, value) in dictionary`.
- Code completion in any editor through `tessel lsp`. See
  [Editor support](/start/editors/).
- **App icons:** an `icon.png` next to a program is its app's icon.

## 0.1.7

*September 30, 2026*

- **Tuples** and **sets**. See [Collections](/language/collections/).
- **Modules:** `import geometry`, with `public` and `private`. See
  [Programs and files](/language/programs/).

## 0.1.6

*September 29, 2026*

- **Errors with reasons:** `Result<T>`, `Error` and `try`. See
  [Errors](/language/errors/).
- Clearer error messages for beginners, and a
  [21-lesson course](/learn/) for learning to program with Tessel.

## 0.1.5

*September 28, 2026*

- **Generics:** generic functions, structs and enums, with interface
  constraints. See [Generics](/language/generics/).
- Faster lists, dictionaries, math and text.

## 0.1.4

*September 28, 2026*

- **Interfaces.** See [Interfaces](/language/interfaces/).
- `while`, `while let`, `break` and `continue`, and reading input from the
  command line.

## 0.1.3

*September 27, 2026*

- `tessel update`, and updates offered in the IDE.
- Code completion in the IDE. JSON and binary saving. Work on another
  thread with `background`.
- A much larger [standard library](/reference/stdlib/): text, lists,
  numbers, dates, files, the clipboard, and the network (requests,
  downloads, WebSockets).

## 0.1.2

*September 27, 2026*

- Math functions, and number formatting with `formatted(decimals:)`.
- Open and Save dialogs. Apps about a third smaller.

## 0.1.1

*September 27, 2026*

- Fixes for four compiler crashes and several wrong results.
- Callbacks (timers, requests) work in `fn main` programs, and
  `.hoverBackground` works on any view.
- **IDE:** an empty folder offers to create `main.tsl`.

## 0.1.0

*September 27, 2026*

- The first release: the language, the UI toolkit, the compiler,
  and the IDE.
