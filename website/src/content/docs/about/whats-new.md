---
title: What's new
description: What each release of Tessel added and fixed, newest first.
---

What each release of Tessel brought, newest first. To get the latest, run
`tessel update` (or take the update the IDE offers). See
[Installation](/start/installation/) if you don't have Tessel yet.

Tessel's first releases came quickly, several on some days, so each day's
are listed together under its last version. See
[How Tessel is made](/about/roadmap/#how-tessel-is-made).

## 0.1.39

*October 7, 2026 · 0.1.38 and 0.1.39*

- **Dialogs.** `alert` and `confirm` show the system's message dialogs,
  `chooseFolder` asks for a folder, and `openFiles` for several files. See
  [Dialogs and sheets](/ui/dialogs/).
- **Sheets.** `.sheet(isPresented:)` shows your own views in a card over the
  dimmed window, and `dismiss()` closes it. `TextPrompt` asks for a text and
  `ProgressPanel` shows work going on, ready-made for a sheet. See
  [Sheets](/ui/dialogs/#sheets).
- **Pickers:** `DatePicker` and `Calendar` for dates, `TimePicker` for the
  hour and minute, and `FontPicker` for a font family. See
  [DatePicker](/reference/views/#datepicker).
- **Notices:** `showNotice("Saved")` shows a short message that goes by
  itself. `ProgressBar` shows how far something is.
- **Printing:** `printDocument` draws pages as `writePDF` does and prints
  them: through the system's print dialog on macOS, and the PDF app on
  Windows. See [Printing](/ui/dialogs/#printing).
- **Ribbons in several files.** A view of your own can make a ribbon's tab,
  a group, or some of a group's buttons, and the ribbon uses them as if
  they were written in it. See
  [A ribbon in several files](/ui/ribbons/#a-ribbon-in-several-files).
- **Fixed:**
  - `g.textWidth` left out the spaces at the end of a text, so a space
    alone was 0 wide.
  - Padding on a `Divider` made the line thicker instead of leaving room
    around it.

## 0.1.37

*October 6, 2026 · 0.1.36 and 0.1.37*

- **`tessel new`** starts a project from a template: a small to-do list app
  in a few files (the default), an app with one window, or a command-line
  program, with a `tessel.toml`, a README and a `.gitignore`. The IDE offers
  the same templates in an empty folder. See
  [`tessel new`](/start/cli/#tessel-new).
- **Databases.** `openDatabase` opens (or creates) an SQLite database file,
  and `execute` and `query` run SQL on it, with values filled in for `?`s.
  SQLite is built into Tessel, so it works the same on macOS and Windows
  with nothing to install. See
  [Saving data in a database](/guides/database/).
- **Font families and italic.** `.font(italic:family:)`, and the same on
  `g.text` and `g.textWidth`: any installed font by name, or
  `"monospaced"`. A font that isn't installed falls back to the system's.
  `fontFamilies()` lists the fonts installed. See
  [font](/reference/modifiers/#font).
- **Password fields:** `TextField(secure: true)` shows a dot for each
  character, and its text can't be copied. See
  [TextField](/reference/views/#textfield).
- **Hashes:** `data.hash()` gives SHA-256, SHA-1, SHA-512, MD5 and other
  digests, and `data.hex` shows bytes as hexadecimal. See
  [hash and hex](/reference/stdlib/#hash-and-hex).
- `wrappingAdd`, `wrappingSubtract` and `wrappingMultiply` on `Int`, for
  hashes and checksums written in Tessel.
- **Fixed:** `==` between two cases of an enum whose cases hold different
  values crashed.

## 0.1.35

*October 5, 2026*

- **Ribbons.** `Ribbon`, `RibbonTab` and `RibbonGroup` lay out tabs of
  grouped commands, as office apps have, with large and small buttons,
  toggles and menus (`RibbonButton`, `RibbonToggle`, `RibbonMenu`). Groups
  shrink to fit a narrow window. See [Ribbons](/ui/ribbons/).
- **48 more built-in icons**, for ribbons and toolbars: cut, paste, bold,
  the alignments, charts, sort, filter, sum and more. See
  [Icons](/reference/icons/).
- **The app's manifest.** The `[app]` table of `tessel.toml` gives the app's
  name, identifier, version, author, copyright, icon, category and the
  files it opens. App bundles on macOS and `.exe` files on Windows carry
  them, the About panel shows them, and `appInfo()` gives them to the
  program. See [The project file](/start/project/).
- **`text += more` is fast:** it adds to the string where it is, as `+=` on
  a list does (200 000 appends: 7 ms instead of 6 s).
- **Fixed:**
  - `toJson` on a type that contains itself (an enum of lists of itself,
    a tree) crashed the compiler.
  - Menu bar shortcuts didn't run in a view with `.onKeyEvent`.
  - `saveFile` with several types added a second extension.
  - In test scripts, `key shift+5` types `%`, and `type` with no field
    edited presses each key.

## 0.1.34

*October 4, 2026 · 0.1.30 to 0.1.34*

- **PDF documents.** `writePDF` draws pages with the same `Graphics` as a
  `Canvas`, and `g.newPage()` starts the next one. Text stays text that can
  be selected and searched, with its fonts in the file; shapes stay sharp
  at any zoom. See [Making a PDF](/ui/drawing/#making-a-pdf) and
  [`writePDF`](/reference/stdlib/#writepdf).
- **Much smaller apps on Windows.** Apps draw, lay out text and read
  pictures with what comes with Windows (Direct2D, DirectWrite and WIC), as
  they do on macOS: a one-`Text` app is about 1.3 MB instead of 7.5 MB. See
  [How big an app is](/start/cli/#how-big-an-app-is).
- **A project chooses its version of Tessel.** `tessel pin` writes the
  version into the project's `tessel.toml`, and from then on Tessel's
  commands, editors and the IDE use that version for the project,
  installing it the first time. See [`tessel pin`](/start/cli/#tessel-pin).
- **Functions can change the caller's value.** A parameter declared `bind`
  is the caller's variable, field or list item itself, not a copy, as it
  already was for views: `fn addBonus(scores: bind [Int])`. Methods can have
  them too. See
  [Changing the caller's value](/language/values/#changing-the-callers-value).
- **New controls:** `Slider`, `Stepper`, `ColorPicker`, `DisclosureGroup`
  (for trees) and `LazyVStack` (for long lists). See
  [Controls](/ui/controls/#slider-stepper-and-colorpicker).
- **Drag and drop** between views with `.draggable` and `.dropTarget`, and
  every key with `.onKeyEvent`. Keyboard shortcuts can ask for Shift and
  Alt: `.shortcut("shift+z")`.
- **Background jobs** report their progress and can be cancelled.
- **Drawing:** pictures made from pixels, gradients, layers that blend with
  what's under them (on Windows too), and SVGs on a `Canvas`. See
  [Drawing](/ui/drawing/).
- **Menus and windows:** context menus, popovers, the menu bar (the
  system's on macOS), more than one window, asking before a window closes,
  and files opened with the app. See
  [Menus, windows and documents](/ui/menus-windows/).
- **Language:** bit operations, hex and binary number literals, `match` arms
  with several patterns, tuples in `if let`, and functions declared inside
  functions. `Data` can have bytes appended to it. See
  [Basics](/language/basics/) and [Functions](/language/functions/).
- **Fixed:**
  - Changing a loop's variable through a closure (or a `bind` argument)
    changed a copy of the list, not the list.
  - Appending many bytes to a `Data` could damage it.

## 0.1.29

*October 3, 2026 · 0.1.23 to 0.1.29*

- **Much smaller apps.** On macOS, apps draw, lay out text and read
  pictures with what comes with macOS, so a one-`Text` app is about 1 MB
  instead of 8.3 MB. Apps carry SVG, pictures and Markdown only when they
  use them, and Windows apps no longer carry all of Tessel (a one-`Text`
  app was 24 MB). See [How big an app is](/start/cli/#how-big-an-app-is).
- **IDE:** the Design panel has an inspector, undo and redo, copy, cut,
  paste and duplicate, an outline of the views, and more views to add
  (`TextField`, `Toggle`, `Picker`). A Preview panel shows Markdown files as
  they'll look, next to their text. See
  [The Design panel](/start/ide/#the-design-panel).
- A code editor's undo history includes the changes the program makes to
  its text. New: `undoTextEdit()` and `redoTextEdit()`.
- A click inside a view with `.onKey` gives it the keyboard.
- `tessel preview --dark`.
- **Fixed:**
  - On Windows, the REPL, hot reload and previews could crash, and endless
    recursion ended the program instead of being reported as a Tessel
    error, as on macOS.
  - `pow(10, x)` failed to build on macOS.

## 0.1.22

*October 2, 2026 · 0.1.14 to 0.1.22*

- **IDE: the Design panel.** A picture of your app next to its code, where
  views can be selected, changed, added, moved and deleted. Every change is
  written into the code. See [The Design panel](/start/ide/#the-design-panel).
- **Canvas:** draw views of your own with paths, shapes, text and pictures,
  and save drawings as pictures. See [Drawing and the mouse](/ui/drawing/).
- **The mouse and keys:** `.onPointer`, `.onKey`, `.onDrag`, `.cursor`,
  `.onDrop`, `.onResize`, `.tooltip`, `.offset` and `.clipped`.
- **A `Markdown` view:** text with headings, lists, code, tables, pictures
  and links.
- **Scroll bars** in scroll views and code editors, which show when there's
  more than fits and can be dragged.
- **IDE:** the documentation comes with it: the Docs button opens it next to
  the editor, without a network, with search. The explorer, the bottom
  panel and the Docs panel can be resized by dragging their edges.
- `tessel preview` makes a picture of an app's window for editors. See
  [`tessel preview`](/start/cli/#tessel-preview).
- `Data` reads numbers, slices and Latin-1 text, and compresses.
  `list += other` appends in place. `readImage` gives a picture file's size
  and pixels, and `appendData` adds bytes to the end of a file.
- `CodeEditor`: taller lines, and more room around the line numbers and the
  code.
- Releases are for macOS and Windows together from now on.
- **Fixed:**
  - Stopping `tessel run` left the program it started running.
  - A double click in a code editor or text field could select only part of
    a word.

## 0.1.13

*October 1, 2026 · 0.1.8 to 0.1.13*

- **`tessel repl`:** type code and see what it does, a line at a time. An
  `import` stays for the rest of the session, so a project's modules can be
  tried out at the prompt. See [The REPL](/start/repl/).
- **Hot reload:** `tessel run --hot` gives a running app its new code when
  you save, and keeps its state. In the IDE, "Reload on save".
- **Light and dark:** apps follow the system, or choose one with
  `app Name(appearance: .dark)`. Colors of your own with `.rgb` and
  `.rgba`.
- `.animation()`: views move, resize and fade to their new state.
- `Image` shows PNG and JPEG files.
- Tab and Shift+Tab move between text fields, and input methods (Chinese,
  Japanese, Korean, accents) work in them.
- **Time zones** for `date`, `formatDate` and `parseDate`, and calendar
  dates: `dateParts`, `addToDate`, `daysBetween` and more.
- `cancelRequest` gives up a `request` or `download`. WebSockets can send
  and receive bytes.
- `print` shows lists, dictionaries, sets and tuples. `enumerated()`, `zip`,
  and `for (key, value) in dictionary`.
- Code completion in any editor through `tessel lsp`. See
  [Editor support](/start/editors/).
- **App icons:** an `icon.png` next to a program is its app's icon.
- **Fixed:** REPL errors point at the right column, and every entry has its
  own number.

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

*September 28, 2026 · 0.1.4 and 0.1.5*

- **Interfaces.** See [Interfaces](/language/interfaces/).
- **Generics:** generic functions, structs and enums, with interface
  constraints. See [Generics](/language/generics/).
- `while`, `while let`, `break` and `continue`, and reading input from the
  command line.
- Faster lists, dictionaries, math and text.

## 0.1.3

*September 27, 2026 · 0.1.0 to 0.1.3*

- The first release: the language, the UI toolkit, the compiler, and the
  IDE.
- A much larger [standard library](/reference/stdlib/): text, lists,
  numbers, math, dates, files, the clipboard, and the network (requests,
  downloads, WebSockets). JSON and binary saving. Work on another thread
  with `background`.
- Open and Save dialogs, and number formatting with `formatted(decimals:)`.
- `tessel update`, and updates offered in the IDE. Code completion in the
  IDE, which offers to create `main.tsl` in an empty folder.
- Apps about a third smaller.
- **Fixed:** four compiler crashes and several wrong results. Callbacks
  (timers, requests) work in `fn main` programs, and `.hoverBackground` works
  on any view.
