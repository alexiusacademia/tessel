---
title: Views
description: Every built-in Tessel view, with its exact parameters, defaults and layout behavior.
---

This page lists every built-in view, plus the `app` declaration. For a gentler
introduction, see [Apps and views](/ui/apps-views/) and [Controls](/ui/controls/).

## Reading the signatures

Each view's signature is written the way you would declare a view's
parameters in Tessel yourself:

```text
TextField(_ placeholder: String, text: bind String)
```

- `_` in front of a name means you pass that argument **without a label**:
  `TextField("Name", text: name)`.
- Every other argument is passed **with its label**, in the order shown.
- `= value` marks a parameter with a **default**; you can leave it out.
- `bind` means the view both reads and changes the value, so you must pass
  something it can change: a `state`, a `var`, a `bind` parameter, or a field
  or item of one. See [State and bindings](/ui/state/).
- A last parameter of type `View` (named `content`) is the **block after the
  call**: `VStack { … }`. A last parameter of function type, like a button's
  `action`, can also be written as a block after the call:
  `Button("Save") { save() }`.

Modifiers such as `.padding()` and `.font(size:)` work on every view; they are
listed in [Modifiers](/reference/modifiers/).

## How views take up space

Tessel lays views out like SwiftUI. Some views have a fixed natural size
(text, buttons, toggles, icons) and some are **flexible**: they grow to fill
the space they are offered.

| Flexible… | Views |
|---|---|
| horizontally only | `TextField` |
| in both directions | `Scroll`, `CodeEditor`, `TabView`, `Canvas` |
| along its stack's direction | `Spacer` |
| across its stack | `Divider` |
| when a child is flexible | `VStack`, `HStack`, `ZStack`, custom views |
| when given a maximum size | any view with `.frame(maxWidth:)` or `.frame(maxHeight:)` |

A fixed `.frame(width:)` or `.frame(height:)` makes a view inflexible in that
direction. See [Layout](/ui/layout/) for how stacks share out space.

## app

```text
app Name(title: String = "Name", width: Float = 640, height: Float = 480,
         appearance: Appearance = .system) { … }
```

Declares the program's window. The arguments are optional, but when you give
them they need their labels and must be in this order.

| Parameter | Default | Meaning |
|---|---|---|
| `title` | the app's name | The window title. |
| `width` | `640` | The window's starting width, in points. |
| `height` | `480` | The window's starting height, in points. |
| `appearance` | `.system` | [`.light` or `.dark`](/reference/colors/#appearance) to always look that way, whatever the system's setting. |

```tessel
app Notes(title: "My Notes", width: 400, height: 300) {
    state text = ""

    TextField("Write something", text: text)
    Text("{text.count} characters")
}
```

The body works like a view body: it can declare `state` and functions, and
everything else in it must be views. The views are stacked vertically with
8 points between them, like a `VStack`, and centered in the window. A program
has at most one `app`, and can't have both an `app` and a `fn main()`.

## Custom views

A `view` you declare yourself is used like a built-in one. Its body lays out
like a `VStack` with the default spacing (8) and centered alignment. A
modifier on a custom view applies to that whole body. See
[Apps and views](/ui/apps-views/).

## Text

```text
Text(_ text: String)
```

Shows text. Use string interpolation to show values: `Text("Total: {total}")`.

```tessel
Text("Hello, world!")
    .font(size: 20, weight: .semibold)
    .color(.blue)
```

- The text size, weight and color come from [`.font`](/reference/modifiers/#font)
  and [`.color`](/reference/modifiers/#color) (default: 14 points, regular,
  `.primary`).
- Text wraps onto more lines when it's wider than the space it's offered.
  `\n` in the string starts a new line.
- Lines line up on the leading edge; change that with
  [`.textAlignment`](/reference/modifiers/#textalignment).
- In a frame larger than the text, the text is centered unless the frame
  gives an `alignment:`.

## Button

```text
Button(_ label: String, action: fn())
```

A push button. Clicking it runs `action`. The action is almost always written
as a block after the call.

```tessel
Button("Add one") {
    count += 1
}
```

- The label is drawn in the accent blue on a light gray rounded background.
  `.color(…)` on the button changes the label color;
  `.background(…)` replaces the gray background.
- The button's size is its label plus 14 points of space on each side and 6
  above and below. A `.frame(width:)` or `.frame(maxWidth: .infinity)` makes
  the whole button wider.
- A disabled button (`.disabled(true)`) is dimmed and doesn't respond to
  clicks.

## TextField

```text
TextField(_ placeholder: String, text: bind String)
```

A one-line text input. `text` is changed as the user types. The placeholder is
shown in gray while the text is empty.

```tessel
TextField("Your name", text: name)
    .onSubmit { save() }
```

- Click the field to start editing. **Enter** or **Escape** stops editing;
  Enter also runs the field's [`.onSubmit`](/reference/modifiers/#onsubmit)
  action.
- The usual editing keys work: arrows (with Shift to select, Option/Alt for
  words), Cmd+A, Cmd+C, Cmd+X, Cmd+V, and Cmd+Z / Shift+Cmd+Z for undo and
  redo (Ctrl instead of Cmd on Windows).
- A text field is flexible horizontally: it stretches to the width it's
  offered. It's at least 120 points wide when there's room.
- [`.autofocus()`](/reference/modifiers/#autofocus) makes it start editing as
  soon as it appears.

## Toggle

```text
Toggle(_ label: String = "", isOn: bind Bool)
```

An on/off switch. Clicking it flips `isOn`. The label is optional.

```tessel
Toggle("Dark mode", isOn: isOn)
Toggle(isOn: isOn)
```

- The label sits on the leading side and the switch on the trailing side,
  8 points apart. If the toggle is wider than that (for example with
  `.frame(maxWidth: .infinity)`), the label and switch move apart.
- Clicking anywhere on the toggle, label included, flips it.

## Picker

```text
Picker(selection: bind T, content: View)
```

A segmented control for choosing one of several options. Each view in the
block is one option; give each option a [`.tag(value)`](/reference/modifiers/#tag).
The option whose tag equals `selection` is highlighted, and clicking an option
sets `selection` to its tag.

```tessel
Picker(selection: mode) {
    Text("List").tag(Mode.list)
    Text("Grid").tag(Mode.grid)
}
```

- `selection` is usually an enum, a `String` or an `Int`. Every tag must
  have the same type as `selection`. When the tag type is known, you can write enum
  cases as `.tag(.list)`.
- Each view directly inside the block is one option, including views made by
  a `for` loop or an `if`. A `.tag` must be on those views themselves, not on
  something nested inside them.
- The options sit side by side, each with 12 points of space on either side.

## VStack

```text
VStack(spacing: Float = 8, alignment: Alignment = .center, content: View)
```

Arranges its views from top to bottom.

```tessel
VStack(spacing: 4, alignment: .leading) {
    Text("Title").font(weight: .bold)
    Text("Subtitle").color(.secondary)
}
```

- `spacing` is the space between neighboring views. Hidden views get no
  space.
- `alignment` lines views up **across** the stack: `.leading`, `.center` or
  `.trailing`. Only the horizontal part of the alignment counts, so
  `.topLeading` acts like `.leading`, and `.top` or `.bottom` like `.center`.
- Along the stack, the views are packed together in the middle. Use a
  [`Spacer`](#spacer) to push them apart.
- The stack is as tall as its views plus the spacing, and as wide as its
  widest view. If a view inside is flexible (a `Spacer`, a `Scroll`, a
  `TextField`, …), the stack becomes flexible in that direction too.

## HStack

```text
HStack(spacing: Float = 8, alignment: Alignment = .center, content: View)
```

Arranges its views from leading to trailing (left to right).

```tessel
HStack(spacing: 12) {
    Icon(.folder)
    Text("Documents")
    Spacer()
    Text("12 files").color(.secondary)
}
```

- `alignment` lines views up across the stack, vertically: `.top`,
  `.center` or `.bottom`. Only the vertical part counts: `.topLeading` and
  `.topTrailing` act like `.top`, `.bottomLeading` and `.bottomTrailing` like
  `.bottom`.
- `.leading` and `.trailing` aren't vertical positions; in an `HStack` they
  mean `.top` and `.bottom`. Prefer writing `.top` and `.bottom`.
- Otherwise it behaves like `VStack`, turned sideways. Text in an `HStack`
  wraps if the row is too narrow.

## ZStack

```text
ZStack(alignment: Alignment = .center, content: View)
```

Layers its views on top of each other. Later views are drawn on top.

```tessel
ZStack(alignment: .topTrailing) {
    Icon(.bell).font(size: 24)
    Text("3").font(size: 10).color(.white).padding(2).background(.red, radius: 6)
}
```

- The stack is as big as its largest view.
- `alignment` places each view that is smaller than the stack; all nine
  [`Alignment`](/reference/colors/#alignment) cases work.
- A flexible view (like a `Scroll`) fills the whole stack. A `Spacer` in a
  `ZStack` takes no space.

## Spacer

```text
Spacer()
```

Empty space that grows along its stack's direction: vertically in a `VStack`,
horizontally in an `HStack`.

```tessel
HStack {
    Text("Left")
    Spacer()
    Text("Right")
}
.frame(maxWidth: .infinity)
```

- Several spacers in one stack share the leftover space equally.
- A spacer makes its stack flexible, so the stack fills the space its own
  parent offers.
- Outside a `VStack` or `HStack` (for example in a `ZStack`), a spacer takes
  no space.

## Scroll

```text
Scroll(content: View)
```

A vertically scrolling area. Scroll with the mouse wheel or trackpad, or
drag the scroll bar: it shows at the right edge when the content is taller
than the view, and clicking beside its thumb jumps there.

```tessel
Scroll {
    VStack(alignment: .leading) {
        for item in items {
            Text(item.name)
        }
    }
}
```

- A scroll view is flexible in both directions: it takes all the space it's
  offered, and its content can be as tall as it needs.
- Content starts at the top leading corner.
- Scrolling is vertical only. The content gets the scroll view's width, so
  text wraps rather than scroll sideways.

- If you put several views directly inside, they are stacked downwards like
  a leading-aligned `VStack` with spacing 8. Wrap them in your own `VStack`
  to choose a different spacing or alignment.

## Image

```text
Image(_ path: String)
```

Shows an image file. The path is relative to the folder the program is run
from (see [`currentFolder()`](/reference/stdlib/#currentfolder)).

```tessel
Image("logo.svg").frame(width: 48, height: 48)
```

- SVG files (`.svg`) are drawn at the size the SVG declares. With a
  `.frame(width:)` or `.frame(height:)`, the drawing is scaled to fit the
  frame, keeping its proportions. `currentColor` in the SVG is drawn in the
  view's [`.color`](/reference/modifiers/#color).
- PNG and JPEG files are drawn one point for each pixel, or smaller when
  there's less room. A `.frame` scales them to fit it (larger too), keeping
  their proportions. A photo is shown the way up its camera recorded.
- Other files, and files that can't be read, show a 64 × 64 gray placeholder
  box.
- The file is read once, and again when it changes.

## Markdown

```text
Markdown(_ text: String, folder: String = "", onLink: fn(String)? = nil)
```

Shows text written in [Markdown](https://commonmark.org/help/): headings,
paragraphs with **bold**, *italic*, `code` and links, lists, quotes, blocks
of code, tables, pictures and rules. It's for longer or richer text than a
[`Text`](#text): help pages, release notes, a message with a link in it.

```tessel
Markdown("## Welcome\n\nRead the [guide](/guide/) or just *start typing*.", onLink: { link in
    open(page: link)
})
```

- It's as wide as the space it's given, and as tall as its text needs then.
  Put it in a [`Scroll`](#scroll) when it may be longer than the window.
- The text's size and color are the view's ([`.font`](/reference/modifiers/#font),
  [`.color`](/reference/modifiers/#color)); headings are larger in proportion.
- **Links.** A click on a link calls `onLink` with the link's address,
  exactly as written. Without `onLink`, links to the web (`http://`,
  `https://`) open in the browser, and other links do nothing.
- **Pictures** (`![what it shows](picture.png)`) are PNG and JPEG files,
  looked for in `folder` (or in the folder the program runs from). A picture
  that's wider than the view is made to fit. One that can't be read is
  replaced by its description.
- **Code.** A block marked `tessel` gets Tessel's syntax colors. Long lines
  wrap.
- Tables are as wide as their text, and wrap their cells when that's wider
  than the view.
- HTML in the text isn't shown. Text can't be selected.

## Icon

```text
Icon(_ name: IconName)
```

One of Tessel's built-in icons. See [Icons](/reference/icons/) for the full
list.

```tessel
HStack(spacing: 6) {
    Icon(.warning).color(.orange)
    Text("3 problems")
}
```

- An icon is a square whose side is 1.15 times the font size, rounded (16
  points at the default size of 14). Change it with `.font(size:)`.
- It is drawn in the current [`.color`](/reference/modifiers/#color), so it
  matches the text around it.

## Svg

```text
Svg(_ source: String)
```

Draws SVG markup written in the program.

```tessel
Svg("<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><circle cx='12' cy='12' r='10' fill='currentColor'/></svg>")
    .color(.green)
```

- Sizing works like an SVG [`Image`](#image): the SVG's own `width` and
  `height`, scaled to fit a `.frame` if there is one.
- `currentColor` is drawn in the view's `.color`.
- Markup that isn't valid SVG draws nothing (in a 16 × 16 space).

## CodeEditor

```text
CodeEditor(text: bind String, errorLines: [Int] = [], line: bind Int = …,
           completions: [Completion] = [], completionStart: Int = …, onComplete: fn(Int) = …)
```

A multi-line code editor with line numbers and Tessel syntax colors. It's
what the [Tessel IDE](/start/ide/) uses. See [Code editor](/ui/code-editor/).

```tessel
CodeEditor(text: code, errorLines: errors, line: line)
```

| Parameter | Meaning |
|---|---|
| `text` | The code being edited. Changed as the user types. |
| `errorLines` | Line numbers (starting at 1) to mark with a red band and a red bar in the margin. |
| `line` | Optional. The line the caret is on, starting at 1. The editor updates it as the caret moves, and when your program sets it, the caret jumps to the start of that line. |
| `completions`, `completionStart`, `onComplete` | Optional. Suggestions shown as the user types; see [Code completion](/ui/code-editor/#code-completion). |

- Text is drawn in a 13-point monospaced font, which `.font` doesn't change.
- **Enter** keeps the current line's indentation, adding one level (4
  spaces) after a line ending in `{`. **Tab** inserts 4 spaces. Typing `}` on
  a line that is only spaces removes one level.
- The editor is flexible in both directions. When nothing limits it, it is
  480 × 320 points; it is never smaller than 160 × 80.

## TabView

```text
TabView(selection: bind T, onClose: fn(T) = …, content: View)
```

Pages with a row of tabs at the top. Each view in the block is one page. Give
each page a [`.tabItem("Title")`](/reference/modifiers/#tabitem) for its tab
and a [`.tag(value)`](/reference/modifiers/#tag) to identify it.

```tessel
TabView(selection: tab) {
    Text("Welcome!").tabItem("Home").tag("home")
    Text("No settings yet").tabItem("Settings").tag("settings")
}
```

- The page whose tag equals `selection` is shown; clicking a tab sets
  `selection` to that page's tag. If no page matches, the first page is
  shown.
- A page without `.tabItem` gets the title "Untitled".
- With `onClose:`, tabs get a close button (shown on the selected tab and the
  tab under the mouse). Clicking it calls `onClose` with that page's tag.
  The tab view doesn't remove the page itself; remove it from your data in
  `onClose`:

```tessel
TabView(selection: current, onClose: { name in close(name) }) {
    for doc in docs {
        Text(doc.text).tabItem(doc.name).tag(doc.name)
    }
}
```

- The tab bar is 34 points tall. The tab view is flexible in both
  directions; when nothing limits it, it is 480 × 320 points.

## Canvas

```text
Canvas(redraw: Int = …, draw: fn(Graphics))
```

A view the program draws itself. `draw` is almost always the block after
the call; it gets a [`Graphics`](#graphics) to draw with. See
[Drawing and the mouse](/ui/drawing/).

```tessel
Canvas { g in
    g.fillRect(x: 0, y: 0, width: g.width, height: g.height, color: .sidebar)
    g.fillCircle(x: g.width / 2, y: g.height / 2, radius: 20, color: .blue)
    g.text("Hello", x: g.width / 2, y: 8, align: .top)
}
.frame(width: 200, height: 120)
```

- Coordinates are in points from the canvas's top left corner (inside its
  padding); `y` grows downwards. Nothing is drawn outside the canvas.
- A canvas is flexible in both directions. When nothing limits it, it is
  300 × 150 points.
- Without `redraw:`, the block runs each time the window is drawn. With it,
  the drawing is kept, and the block runs again only when the number is
  different from last time, the canvas's size changes, or the appearance
  switches between light and dark.
- The block shouldn't change `state`.
- In a [headless run](/ui/testing/), `tree` shows what was drawn:
  `Canvas (2 fills, 1 text)`.

### Graphics

What a `Canvas`'s block (and [`renderImage`](/reference/stdlib/#renderimage)'s)
draws with. It has two fields, `width` and `height`: the size of the canvas
in points. Its methods draw on the canvas being drawn; called at any other
time, they do nothing. Angles are in radians.

**A path** is an outline, built up and then drawn:

| Method | |
|---|---|
| `beginPath()` | Starts a new, empty path. |
| `moveTo(_ x, _ y)` | Starts a part of the path at a point. |
| `lineTo(_ x, _ y)` | A straight line to a point. |
| `quadTo(_ cx, _ cy, _ x, _ y)` | A curve to (x, y), pulled towards (cx, cy). |
| `curveTo(_ c1x, _ c1y, _ c2x, _ c2y, _ x, _ y)` | A curve to (x, y) that leaves towards the first control point and arrives from the second (a cubic Bézier). |
| `closePath()` | A line back to where the part started. |
| `lines(_ points: [Float], closed: Bool = false)` | A part through many points, given as `x, y, x, y, …`; closed too, with `closed: true`. Points that aren't numbers (NaN) are left out. |
| `rect(x:, y:, width:, height:, radius: = 0)` | A rectangle, with rounded corners if `radius` is given. |
| `circle(x:, y:, radius:)` | A circle around (x, y). |
| `ellipse(x:, y:, radiusX:, radiusY:)` | An ellipse around (x, y). |
| `arc(x:, y:, radius:, from:, to:)` | Part of a circle, from one angle to another (0 is to the right; angles grow clockwise). |
| `fill(_ color: Color, evenOdd: Bool = false)` | Fills the path. With `evenOdd: true`, a part inside another is a hole. |
| `stroke(_ color: Color, width: Float = 1, dash: [Float] = [])` | Draws the path's line. `dash` is the lengths of dashes and gaps in turn, like `[6, 3]`. |

`fill` and `stroke` keep the path, so it can be filled and then outlined.

**Shapes drawn at once** (they leave the path alone):

| Method | |
|---|---|
| `line(_ x1, _ y1, _ x2, _ y2, color:, width: = 1)` | A straight line. |
| `fillRect(x:, y:, width:, height:, color:, radius: = 0)` | A filled rectangle. |
| `strokeRect(x:, y:, width:, height:, color:, lineWidth: = 1, radius: = 0)` | A rectangle's outline. |
| `fillCircle(x:, y:, radius:, color:)` | A filled circle. |
| `strokeCircle(x:, y:, radius:, color:, lineWidth: = 1)` | A circle's outline. |

**Text and pictures:**

| Method | |
|---|---|
| `text(_ text, x:, y:, size: = 14, color: = .primary, weight: = .regular, align: = .topLeading, angle: = 0, halo: = .clear, haloWidth: = 0)` | Text at a point. `align` (an [`Alignment`](/reference/colors/#alignment)) says which part of the text sits on the point; `angle` turns it around the point; a halo is an outline of `haloWidth` points in another color, under the text. |
| `textWidth(_ text, size: = 14, weight: = .regular) -> Float` | How wide `text` would be drawn. |
| `image(_ path, x:, y:, width:, height:, opacity: = 1) -> Bool` | A PNG or JPEG file, stretched to the rectangle. `false` if there's no such picture. |

**The transform**, which says where coordinates land, **and clipping:**

| Method | |
|---|---|
| `translate(_ x, _ y)` | Moves what follows: (0, 0) is now where (x, y) was. |
| `scale(_ x, _ y)` | Makes what follows `x` times as wide and `y` times as tall. A negative number flips it. |
| `rotate(_ angle)` | Turns what follows around (0, 0). |
| `zoom(_ factor)` | Draws what follows `factor` times larger as a whole: positions like `scale(factor, factor)`, and line widths and text sizes too. For a small copy of a drawing, or a page shown at less than its size. |
| `resetTransform()` | Back to the canvas's own coordinates. |
| `save()` | Remembers the transform (and the clips). |
| `restore()` | Goes back to what the matching `save()` remembered. |
| `clip(x:, y:, width:, height:)` | Until the matching `restore()`, only what's inside the rectangle shows. |

The transform applies to the points of paths and shapes, to the corners of
pictures, and to the point a text sits on. Line widths, dashes and text
sizes are always in points: scaling doesn't make lines thicker or text
larger. `zoom` is the exception: it enlarges those too.

## Divider

```text
Divider()
```

A thin gray line (the `.separator` color) that separates views.

```tessel
VStack {
    Text("Above")
    Divider()
    Text("Below")
}
```

- In a `VStack` it runs horizontally across the whole stack; in an `HStack`
  it runs vertically. It is 1 point thick.
- It makes its stack stretch across the space offered, so a `Divider` in an
  `HStack` makes the row as tall as the space available.
