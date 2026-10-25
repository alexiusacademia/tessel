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

## 0.16.1

*October 25, 2026*

Fixed:

- On macOS, a window no longer blinks while it's resized: AppKit redrew
  it, empty, between Tessel's frames. A larger window shows its
  background where the next frame isn't drawn yet, rather than the last
  frame stretched.

## 0.16.0

*October 25, 2026*

- **An app's own documents.** A `[[app.documents]]` in `tessel.toml` with
  `owned = true` is the app's own kind of file: double-clicking one opens
  it with the app, and `icon = "note.png"` gives the files an icon of
  their own (the app's icon without it), on macOS and, through the
  installer, on Windows. See [The app's own documents](/start/project/#the-apps-own-documents).
- **Changed:** kinds of files that aren't marked `owned`, those in
  `documentTypes:` among them, only offer the app (in Open With), and keep
  the app and icon they have. On Windows the installer used to make the app
  open them. Add `owned = true` to your app's own formats.
- **Ribbons in a narrow window.** After their items become small, ribbon
  groups fold into one button each, which opens the group under it, so
  every command stays in reach. `RibbonGroup(icon:)` chooses the button's
  icon. See [When the window is narrow](/ui/ribbons/#when-the-window-is-narrow).
- A ribbon section in the course: [21. Forms, tables and rich
  text](/learn/21-forms-tables/#a-ribbon-of-commands).

## 0.15.3

*October 25, 2026*

- **Homebrew.** `brew install alexiusacademia/tessel/tessel` installs
  Tessel on a Mac, and `brew upgrade tessel` updates it. In a Homebrew
  installation, `tessel update` (and the IDE's update) runs Homebrew. See
  [With Homebrew](/start/installation/#with-homebrew).

## 0.15.2

*October 25, 2026*

- **Releases are on GitHub.** Tessel's packages are now
  [GitHub releases](https://github.com/alexiusacademia/tessel/releases),
  and the install commands have new addresses (see
  [Installation](/start/installation/)). `tessel update` and the
  installers download from there, and still find the versions up to
  0.15.1 in the old place. Earlier versions of Tessel update to this one
  as before.

## 0.15.1

*October 25, 2026*

Fixed:

- On macOS, the caret stands after spaces at the end of a line. After Enter
  in a block, the IDE's editor showed it at the line's start instead of
  after the new indent, and a space typed at the end of a text field
  didn't move it.

## 0.15.0

*October 25, 2026*

- **Grids for forms.** `Grid` with `GridRow`s lines up labels and fields in
  columns; `.gridColumnAlignment` and `.gridCellColumns` fine-tune it. See
  [Grids](/ui/layout/#grids).
- **Rich text.** `RichTextEditor(text:)` writes in bold, italic, underline,
  strikethrough, sizes, colors and fonts, with a bar of buttons and
  Cmd+B/I/U. It edits a `RichText` (text and styled runs) that saves with
  `toJson`, and turns into Markdown with `.markdown`. Copying keeps the
  styles, into other apps too, and pasting from them keeps theirs. See
  [Rich text](/ui/rich-text/).
- **Tables edited in place.** `TableColumn(onEdit:)`: double-click a cell,
  type, Enter. See [Editing cells](/ui/tables/#editing-cells).
- **More controls.** `Picker(style: .radio)` and `Picker(style: .menu)`,
  `SearchField`, `Link`, `LazyHStack`. Text can be cut short with
  `.lineLimit(n)` and copied with `.selectable()`.
- **Turning and scaling.** `.rotation(degrees)` and `.scale(…)`; the views
  around make room. See [Turning and scaling](/ui/layout/#turning-and-scaling).
- **Sound and video.** `playSound` gives a `Sound` to pause, stop and seek;
  `VideoPlayer` shows a video with controls Tessel draws. See
  [Sound and video](/ui/media/).
- **Web pages.** `WebView(url:)` and `WebView(html:)`, with the system's
  browser engine. See [Web pages](/ui/web/).
- **The desktop.** A tray or menu-bar icon with a menu, which can keep the
  app running when its window is closed (`.trayIcon`, `showWindow()`);
  `showNotification`; `setFullScreen`; and files dragged out of the app
  with `.dragFile`. See [The desktop](/ui/desktop/).
- **Two new lessons** in the course: [Forms, tables and rich
  text](/learn/21-forms-tables/) and [Sound, video, the web and the
  desktop](/learn/22-media-desktop/). `examples/desktop-demo` tries all of
  it.

## 0.14.0

*October 25, 2026*

- **Functions that take any number of values.** A parameter written
  `_ values: Float...` takes as many values as the call gives, and is a
  `[Float]` inside the function. `average(scores...)` passes a list you
  already have. `print` takes any number of values too
  (`print("total:", total)`), and so do `min` and `max`. See
  [Any number of values](/language/functions/#any-number-of-values).
- **The app's own location.** `appPath()` is the app's executable and
  `appFolder()` the folder it's in, so an app finds the files shipped next
  to it wherever it's installed. Under `tessel run` they point at the
  program's source. See [appPath and appFolder](/reference/stdlib/#apppath-and-appfolder).
- **Scrolling sideways, or both ways.** `Scroll(.horizontal)` and
  `Scroll(.both)`. The wheel goes to the innermost scroll view that can move
  that way. See [Across, and both ways](/ui/layout/#across-and-both-ways).
- **Scrolling from code.** `scrollTo(id, align:)` brings the view with that
  `.id` into sight, even one added in the same click, or a row of a
  `LazyVStack` that isn't built yet. See
  [Scrolling from code](/ui/layout/#scrolling-from-code).
- **Split views.** `HSplit` and `VSplit` hold two panes with a divider to
  drag. `size:` can bind the first pane's width or height, and the divider
  stops at each pane's minimum size. See [Split views](/ui/layout/#split-views).

## 0.13.0

*October 24, 2026*

- **An installer for your Windows app.** `tessel build --bundle` for
  Windows (on Windows, or with `--windows` on a Mac) makes
  `<Name> Setup.exe` with the app inside. It installs for the person who
  runs it, with no administrator prompt. It adds a Start menu shortcut
  (and a desktop one if they tick the box), lists the app in Settings so
  it can be removed there, and opens the app's document types. Running a
  newer installer updates the app, even while it's open. `/S` installs
  without asking. See
  [The installer of a Windows app](/start/cli/#the-installer-of-a-windows-app).

## 0.12.0

*October 22, 2026*

- **PDF pages of different sizes.** `g.newPage(width:height:)` starts a
  page of its own size, so one document can hold A1, A3 and A4 sheets;
  `g.width` and `g.height` follow the page being drawn. See
  [Making a PDF](/ui/drawing/#making-a-pdf).
- **Android on Mali phones.** Apps draw on the graphics card on phones with
  a Mali (most phones that aren't Snapdragon ones), where they used to
  work out most of each frame on the processor: scrolling a long list went
  from 60 ms a frame to 7 ms on a Pixel 10a.
- **Background work is quick to start.** `background` hands the values it
  uses, and its result, to the other thread without copying them: a list
  of 40,000 structs goes over in about 1 ms instead of 25.
- **Faster drawing on macOS** of a path made of many small overlapping
  pieces (a dense hatch seen from far): 2.2 s became 34 ms.
- A mistake in an imported module is now listed before the mistakes it
  causes in the program that uses it.

## 0.11.0

*October 21, 2026*

- **Tests.** Write `test "what it checks" { … }` next to your code, with
  `expect(…)` for what must be true, and run them with `tessel test`. A
  failed comparison shows both sides; each test runs in a process of its
  own, so a crash or a hang fails only that test. See
  [Testing your code](/guides/testing/).

## 0.10.0

*October 20, 2026*

- **Windows apps, built on a Mac.** `tessel build todos --windows` makes
  `todos.exe`, the same app the Windows version of Tessel makes, icon and
  details included. What it's built with is downloaded the first time. See
  [For Windows, on a Mac](/start/cli/#for-windows-on-a-mac).

## 0.9.1

*October 19, 2026 · 0.9.0 to 0.9.1*

- **Another name for a module.** `import numeric as num` lets a file use a
  module or library by a shorter name: `num.brent(…)`. It works for
  folders too: `import "../shared/textkit" as text`. See
  [Another name for a module](/language/programs/#another-name-for-a-module).

Fixed:

- The library index's check of a proposed version could fail to find its
  tag, because GitHub limits lookups made without logging in. It now logs
  in when it can, and asks `git` when GitHub refuses.
- A module or library that declares a name Tessel has too, like `Color` or
  `writeText`, no longer hides Tessel's in the programs that import it;
  its own is `m.Color`.
- `dict[key]?.append(x)` changes the list where it is: it took longer the
  longer the list was, copying it whole each time.
- `g.text` turns with `g.rotate`, as lines do. Its size stays in points
  whatever `g.scale` says, and a flip doesn't mirror it.

## 0.8.0

*October 18, 2026 · 0.7.0 to 0.8.0*

- **Libraries.** Publish a module for anyone to use, and use other
  people's: `tessel add textkit` adds a library to the project, and
  `import textkit` uses it. Tessel picks versions that suit every library
  involved, keeps them in `tessel.lock`, and checks each download against
  what was published. `tessel publish` publishes a library from its git
  repository to the [library index](https://github.com/alexiusacademia/tessel-packages),
  and `tessel new --template library` starts one. See
  [Libraries](/guides/libraries/).
- **Tables and trees.** [`Table`](/ui/tables/) shows rows of data in
  columns that can be sorted, resized and selected, and [`Tree`](/ui/tables/#a-tree) shows nested
  data opened level by level.
- **Text as a shape.** [`g.textPath`](/reference/views/#graphics) adds the
  outline of text to the path, to fill with a gradient, stroke or clip by,
  and [`textOutline`](/reference/stdlib/#textoutline) gives it as path
  commands to keep: the font's own curves, exactly where `g.text` draws
  them. See [Text as a shape](/ui/drawing/#text-as-a-shape).
- **Where each glyph goes.** [`textGlyphs`](/reference/stdlib/#textglyphs)
  places each glyph as `g.text` does, after kerning and ligatures, with the
  characters it shows: for text along a path, or a caret between letters.
- **Font metrics.** [`fontMetrics`](/reference/stdlib/#fontmetrics) gives a
  font's ascent, descent, line gap, cap height and x-height at a size.
- **A family's faces.** [`fontStyles`](/reference/stdlib/#fontstyles)
  lists them (Light, Medium, Condensed Black…) with their weights, for a
  style picker; and [`Weight`](/reference/colors/#weight) has `thin`,
  `ultraLight`, `light`, `heavy` and `black`.
- **Kerning and tracking.** `g.text` and `g.textWidth` take `kerning:` and
  `tracking:`.

Fixed:

- Lists of numbers that Tessel's own functions give compared as equal
  whatever was in them.

## 0.6.1

*October 17, 2026 · 0.6.0 and 0.6.1*

- **Text fields of several lines.** `TextField("Notes", text: notes,
  lines: 5)` is five lines high: the text wraps, Enter starts a new line,
  and it scrolls when there's more. Cmd+Enter (Ctrl+Enter on Windows) runs
  its `.onSubmit`. See [Several lines](/ui/controls/#several-lines).
- **Keys let go.** `.onKeyEvent(releases: true)` also hears a key being
  let go (`e.up`) and the modifier keys on their own, for what's done while
  a key is held, like panning while Space is down. See
  [onKeyEvent](/reference/modifiers/#onkeyevent).
- **Settings… in the app menu.** On macOS, a `Menu` with the app's name
  adds its items to the app menu, after About. See
  [Menus, windows and documents](/ui/menus-windows/).
- **Android: PDFs.** `writePDF` writes PDFs on Android too. See
  [PDFs](/guides/android/#pdfs).
- **Faster.** A function with `bind` parameters is about as fast to call
  as a method when what it's given is a variable or a field of one, or (for
  a function with one `bind`) an item of a list; it was some 15 to 25 times
  slower. And views scrolled out of sight aren't drawn.
- **Testing apps:** `click` and `hover` find a view by its
  `.accessibilityLabel` or `.tooltip`, for pictures with no text; and
  `keyup` lets a key go.

Fixed:

- Android apps stopped at their first frame on phones with a Mali graphics
  card (a Pixel 10a). They run now, though slower than they should: see
  [What doesn't work yet](/guides/android/#what-doesnt-work-yet).
- On Android, `writePDF` always returned `false`, and `platform()` said
  `"linux"`; it says `"android"`.

## 0.5.1

*October 13, 2026 · 0.5.0 and 0.5.1*

- **Android: where the phone is.** After
  [`requestPermission(.location)`](/reference/stdlib/#requestpermission)
  shows Android's question and `startLocation()` starts listening,
  [`location()`](/reference/stdlib/#location) gives the latitude,
  longitude, altitude, accuracy, speed and course.
- **Android: the satellites.** [`satellites()`](/reference/stdlib/#satellites)
  lists every navigation satellite the phone hears: its system (GPS,
  GLONASS, Galileo, BeiDou and the others), where it is in the sky, how
  strong its signal is, and whether the phone's place is worked out from
  it.
- **Android: the compass.** [`heading()`](/reference/stdlib/#heading) says
  which way the phone points, from magnetic north and from true north.
- **Trying these on a computer.** `TESSEL_SENSORS=simulated` makes up a
  place, a sky of satellites and a heading, to build an app's screens
  without a phone. See [Where the phone is](/guides/android/#where-the-phone-is).

Fixed:

- The Mac App Store refused apps built by Tessel: each named two
  functions of macOS that Apple keeps to itself, which no Tessel app
  used. They're gone from apps.
- On Windows, `tessel android doctor` never finished when another program
  (the IDE, a script) read its output.

## 0.4.0

*October 12, 2026*

- **3D scenes.** A new view, [`Scene3D`](/reference/views/#scene3d), shows
  solid shapes in three dimensions: boxes, spheres, cylinders, cones,
  surfaces over a grid, and meshes of your own triangles (with a color
  for each point, if you like), with lines and labels. Shapes can be
  see-through. You place a camera (or let it orbit a point as the mouse drags),
  add lights, and move, turn and size things as on a canvas. The graphics
  card draws it, nearer things in front, shaded, with smooth edges. See
  [3D scenes](/ui/3d/).
- **Every control works from the keyboard.** Tab reaches buttons, toggles,
  pickers and tabs as well as fields; Space or Enter presses, and the arrow
  keys choose. The control that has the keyboard shows a ring.
- **Screen readers.** VoiceOver on macOS and Narrator on Windows are told
  what's in a window: each control, what it's called, its value, and what
  can be done with it. `.accessibilityLabel` names what has no text of its
  own. See [Accessibility](/ui/accessibility/).
- **Animation with springs.** `.animation` takes a
  [`curve:`](/reference/colors/#curve): `.easeInOut`, `.easeIn`,
  `.easeOut`, `.linear`, `.spring` or `.bouncy`. A view that goes away now
  fades out, as one that appears fades in. See
  [Animation](/ui/styling/#animation).
- **Editors: go to definition, find references, rename.** The language
  server (so VS Code and other editors) takes you from a name to where it's
  declared, lists everywhere it's used, and renames it everywhere at once.

Fixed:

- An `Icon` or `Svg` given `.clear` (or a see-through color) was drawn
  black.
- A program that declared a function or a type with the name of one of
  Tessel's own, like `dateParts`, got errors pointing into a file it
  couldn't see.

## 0.3.0

*October 11, 2026 · 0.2.0 and 0.3.0*

From these releases on, the second number goes up when a release adds
features, and the third when it only fixes things.

- **Tessel says that it's in use.** So that installations can be counted,
  Tessel reports when it's installed or updated, and at most once a day
  that it's in use: its version, the system, and a random id for the
  installation. Nothing about you, your files or your code. You're told
  once, and `tessel telemetry off` turns it off. See
  [What Tessel reports](/start/installation/#what-tessel-reports).

- **A debugger.** Stop a program at a line, step through its code, and look
  at its variables: lists, dictionaries, structs and enum values open up to
  show their parts. A runtime error stops the program where it happened,
  with its variables still there. It works in three places:
  - the **Tessel IDE**: click beside a line for a breakpoint and press
    **Debug**; a new Debug tab shows the calls that led there and the
    variables;
  - **VS Code**, with version 0.4.0 of the extension: set a breakpoint and
    press F5;
  - **a terminal**: `tessel debug app.tsl`.

  It's part of Tessel, with nothing else to install, and works in apps and
  across `await`. See [Debugging](/guides/debugging/).
- **Android: choosing files.** `openFile`, `openFiles` and `saveFile` open
  Android's file chooser. What the app writes to a chosen file goes to the
  document it stands for. See
  [Files the user chooses](/guides/android/#files-the-user-chooses).
- **Android: the keyboard, dialogs and the clipboard.** The on-screen
  keyboard has its suggestions, corrections (of any word in a field), swipe
  typing and emoji. `alert` and `confirm` are the system's dialogs, and
  `copyToClipboard` and `clipboardText` use the phone's clipboard. Two
  fingers pinch, menus are sized for fingers, and a field the keyboard
  would cover is scrolled into view.
- **`runCommand` can be written to.** With `input: true`, the new
  [`sendToCommand`](/reference/stdlib/#sendtocommand) writes a line to the
  program while it runs.
- **`CodeEditor` for debuggers.** `breakpoints:`, `stoppedLine:` and
  `onBreakpoint:` mark lines in its margin. See
  [For a debugger](/ui/code-editor/#for-a-debugger).
- **Testing apps:** `waitfor "label"` in a script waits until a view with
  that label is there.

Fixed:

- On Android, Enter did nothing in a code editor, and a text field's
  `.onSubmit` never ran.

## 0.1.43

*October 10, 2026 · 0.1.42 and 0.1.43*

- **Apps for Android.** `tessel build --android` makes an app an Android
  package from the same code: an `.apk` to install on a phone, or with
  `--bundle` an `.aab` for Google Play, signed with your own key. On a
  phone, a finger taps and scrolls, a long press opens context menus, the
  app stays clear of the system's bars and the on-screen keyboard, follows
  the dark theme, and Back closes what's on top. `tessel android doctor`
  checks that Android's tools are installed. This is a first version: text
  input is basic, and system dialogs aren't there yet. See
  [Building for Android](/guides/android/).

- **`async` and `await`.** Steps that wait for something (a download, a
  timer, work in the background) can be written one after another, where
  they used to nest in blocks:
  `let text = await fetch(url: address)`. `await` works on every function
  that ends with a completion block, so `fetch`, `after`, `request`,
  `download`, `runCommand`, `background` and your own such functions can be
  awaited as they are. Functions that wait are declared with `async fn`, and
  a button's action (or any block that gives no value) can `await` directly.
  See [Waiting with async and await](/language/async/).
- **Constants.** A `let` at the top of a file is a constant the whole
  program can use: `let maxPlayers = 4`. See
  [Constants](/language/basics/#constants).
- **Computed properties.** `area: Float { width * height }` in a struct or
  enum is read like a field, `rect.area`, and worked out each time. See
  [Computed properties](/language/structs-enums/#computed-properties).
- **Static members.** `static let` and `static fn` belong to a type and are
  used through its name: `Rect.zero`, `Rect.square(side: 2)`. See
  [Static members](/language/structs-enums/#static-members).
- `async`, `await` and `static` are only keywords where they're used this
  way, so programs that use them as names still build.

## 0.1.41

*October 8, 2026 · 0.1.40 and 0.1.41*

- **Smaller apps and faster builds for programs with large structs.** A
  struct of more than about 16 fields is now moved as one block of memory,
  where each use of it used to cost code for every field. An app of
  98,000 lines built around one large struct went from 73 MB of code and
  12 minutes to build, to 13.6 MB and under a minute. Smaller structs are
  as before.
- **A function kept in a struct's field is called like a method:**
  `rule.applies(4)`. See
  [Function types](/language/closures/#function-types).
- **Fixed:**
  - A struct with a field holding a function crashed the compiler when it
    was passed to a `bind` parameter.
  - Escape didn't close a popover (or a sheet) in an app with
    `.onKeyEvent`: the app got the key.
- `tessel --version` says which version a project uses, when the project
  is [pinned](/start/cli/#tessel-pin) to another one than is installed:
  `tessel 0.1.36 (this project uses 0.1.38)`.
- **Fixed:** an index that's a call changing the list it indexes, through
  a `bind` parameter (`list[grow(list)]`), read the list as it was before
  the call, and could stop the program.

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
