---
title: Modifiers
description: Every Tessel view modifier, with its exact parameters, defaults and behavior.
---

A modifier changes how a view looks or behaves. You call it on a view with a
dot, and you can chain as many as you like:

```tessel
Text("Hi")
    .font(size: 18, weight: .semibold)
    .color(.secondary)
    .padding(8)
    .background(.blue, radius: 6)
```

A line that starts with `.` continues the expression on the line before, so
long chains can be spread over several lines. Signatures on this page use the
same notation as [Views](/reference/views/#reading-the-signatures): `_` means
the argument has no label, and `= value` is a default.

## How modifiers combine

**Every modifier works on every view**, built-in or your own. On a custom
view, it applies to the view's whole body.

**The order of modifiers doesn't matter.** Each view has one set of settings,
and each modifier fills in some of them. These two lines give the same
result: a 100-point-wide box, with 10 points of padding inside it and a
yellow background behind all of it:

```tessel
Text("A").padding(10).frame(width: 100).background(.yellow)
Text("A").background(.yellow).frame(width: 100).padding(10)
```

:::note
This is different from SwiftUI, where each modifier wraps the view in a new
one and the order changes the result. In Tessel, a background always covers
the padding, and a frame's size always includes the padding.
:::

**Repeating a modifier replaces the earlier value**, with two exceptions:

- [`padding`](#padding) adds up: `.padding(4).padding(horizontal: 20)` gives
  4 points above and below and 24 on each side.
- [`font`](#font) and [`frame`](#frame) only change the settings you pass:
  `.font(size: 20).font(weight: .bold)` is the same as
  `.font(size: 20, weight: .bold)`.

**Some settings are inherited.** [`font`](#font), [`color`](#color),
[`textAlignment`](#textalignment) and [`disabled`](#disabled) on a stack (or
any view with content) apply to everything inside it, unless a view inside
sets its own:

```tessel
VStack {
    Text("Both lines are red and 18 points")
    Text("This one is bold too").font(weight: .bold)
}
.font(size: 18)
.color(.red)
```

[`opacity`](#opacity) and [`hidden`](#hidden) aren't inherited, but they
affect everything drawn inside the view. All other modifiers affect only the
view they're on.

## padding

```text
.padding(_ amount: Float = 8, horizontal: Float, vertical: Float,
         top: Float, leading: Float, bottom: Float, trailing: Float)
```

Adds empty space around the view's content, inside its frame and background.

```tessel
Text("Default").padding()
Text("All sides").padding(16)
Text("Sides and ends").padding(horizontal: 24, vertical: 4)
Text("Two edges").padding(top: 20, leading: 40)
Text("8, but 20 below").padding(8, bottom: 20)
```

- With no arguments, every edge gets **8** points.
- With arguments, each edge takes the most specific value given: its own
  edge (`top:`, `leading:`, `bottom:`, `trailing:`), then its direction
  (`vertical:` for top and bottom, `horizontal:` for leading and trailing),
  then the unlabeled `amount`. Edges with no value get **0**, so
  `.padding(horizontal: 30)` adds nothing above or below.
- Paddings add up: `.padding(8).padding(8)` is the same as `.padding(16)`.

## font

```text
.font(size: Float = inherited, weight: Weight = inherited, italic: Bool = inherited,
      family: String = inherited)
```

Sets the text size (in points), weight, slant and font family.

```tessel
Text("Title").font(size: 24, weight: .bold)
Text("Note").font(size: 11)
Text("Chapter One").font(size: 20, italic: true, family: "Georgia")
Text("=SUM(A1:A9)").font(family: "monospaced")
```

- Only the parts you pass change; the rest come from the view's container.
  In a window, text starts at **14 points**, `.regular`.
- `weight` is one of `.regular`, `.medium`, `.semibold`, `.bold` (see
  [Weight](/reference/colors/#weight)).
- `family` is the name of a font family installed on the computer (see
  [`fontFamilies`](/reference/stdlib/#fontfamilies)), or `"system"` for the
  system's font (the one text starts with) or `"monospaced"` for the
  system's monospaced font. A family that isn't installed is drawn in the
  system's font, so a program never fails over a missing font.
- `italic: true` uses the family's italic, or slants its letters when it has
  none.
- Inherited: set it on a stack to change all the text inside.
- Icons are sized by the font size too. A `CodeEditor` always uses its own
  13-point monospaced font.

## color

```text
.color(_ color: Color)
```

Sets the color of text, icons, and `currentColor` in SVGs.

```tessel
Text("Saved").color(.green)
Icon(.warning).color(.orange)
```

- The default is `.primary` (near-black). See [Colors](/reference/colors/)
  for all colors.
- Inherited: set it on a stack to color everything inside.
- A button's label is blue unless `.color` is set **on the button itself**; a
  color inherited from a container doesn't change it.

## background

```text
.background(_ color: Color, radius: Float = 0)
```

Fills the view's whole area, padding included, with a color. `radius` rounds
the corners.

```tessel
Text("Tag")
    .padding(horizontal: 8, vertical: 2)
    .background(.lightGray, radius: 6)
```

- The background is drawn behind the view's content.
- On a `Button`, it replaces the button's own gray background.

## border

```text
.border(_ color: Color, width: Float = 1, radius: Float = 0)
```

Draws a line around the view's edge. `width` is the line's thickness and
`radius` rounds the corners.

```tessel
Text("Outlined")
    .padding(8)
    .border(.separator, width: 1, radius: 6)
```

- The border follows the view's outer edge (padding and frame included) and
  is drawn on top of the content.
- It doesn't take up space: it's centered on the edge, so a thick border
  overlaps the content a little.

## frame

```text
.frame(width: Float?, height: Float?,
       minWidth: Float?, maxWidth: Float?,
       minHeight: Float?, maxHeight: Float?,
       alignment: Alignment)
```

Controls the view's size. All arguments are optional; pass only the ones you
need, in this order.

```tessel
Text("Fixed").frame(width: 120, height: 40)
Button("Full width") { save() }.frame(maxWidth: .infinity)
Button("OK") { save() }.frame(minWidth: 100)
Text("Right").frame(maxWidth: .infinity, alignment: .trailing)
```

| Argument | Effect |
|---|---|
| `width`, `height` | A fixed size. The view is exactly this big, and no longer flexible in that direction. |
| `maxWidth`, `maxHeight` | The view becomes flexible: it takes all the space it's offered, up to this size, but never less than its content. Use `.infinity` for "as much as possible". |
| `minWidth`, `minHeight` | The view is never smaller than this. It still grows with its content. |
| `alignment` | Where the content sits when the frame is bigger than it. |

- The size includes the view's padding.
- `.infinity` is the only shorthand for a `Float`; you can write it wherever
  a `Float` is expected, but it's meant for `maxWidth:` and `maxHeight:`.
- **Alignment.** `Text`, `Icon`, `Svg` and SVG `Image` content is centered in a
  larger frame unless `alignment:` says otherwise. A stack with a frame
  spreads its children over the whole frame; with `alignment:`, it keeps its
  own size and sits at that position in the frame. So does the body of a
  view of your own, like a stack. Other views, like buttons
  and text fields, fill their frame.
- Text wraps to fit the frame's width.
- Giving a `width` to an SVG or `Image` scales the drawing to fit.

## opacity

```text
.opacity(_ value: Float)
```

Makes the view and everything in it see-through: `0` is invisible, `1` is
fully visible. Values outside 0…1 are clamped.

```tessel
Text("Faded").opacity(0.4)
```

An invisible view still takes up space and still responds to clicks. To
remove a view from the layout, use [`hidden`](#hidden).

## hidden

```text
.hidden(_ isHidden: Bool = true)
```

Hides the view when `isHidden` is `true`.

```tessel
Text("Only when on").hidden(!isOn)
```

- A hidden view **takes up no space**, as if it wasn't there: stacks give it
  no spacing either. (In SwiftUI, a hidden view keeps its space.)
- It isn't drawn, doesn't respond to clicks, and its
  [`shortcut`](#shortcut) doesn't run.
- It is still built, so `state` inside it is kept while it's hidden. Using
  `if` instead removes the view, and its state, entirely.

## disabled

```text
.disabled(_ isDisabled: Bool = true)
```

Turns off interaction with the view when `isDisabled` is `true`.

```tessel
Button("Send") { save() }.disabled(name.isEmpty)
```

- A disabled view ignores clicks, and its [`shortcut`](#shortcut) doesn't
  run. A disabled text field can't be edited.
- Text, controls and icons in it are drawn at 40% opacity.
- Inherited: disabling a stack disables everything inside.

## onTap

```text
.onTap(_ action: fn())
```

Runs `action` when the view is clicked. Usually written with a block after
the call.

```tessel
Text("Click me").onTap {
    count += 1
}
```

- A click counts when the mouse button is pressed and released over the same
  view.
- Controls inside the view (buttons, toggles, text fields) handle their own
  clicks; clicks on other parts of the view run `action`.
- Doesn't run while the view is disabled or hidden.

## onDrag

```text
.onDrag(_ action: fn(Float, Float))
```

Runs `action` again and again while the mouse is moved with its button held
down, starting on the view. It gets how far the mouse moved since the last
time, across and down, in points.

```tessel
Spacer()
    .frame(width: 6, maxHeight: .infinity)
    .background(.separator)
    .onDrag { dx, dy in
        sidebar = (sidebar + dx).clamped(min: 120, max: 400)
    }
```

- The drag goes on until the button is released, also when the mouse leaves
  the view (as it does at once, with a thin bar).
- Add the movements up to follow the mouse: a width that's dragged is
  `width + dx`. Keep the result in bounds yourself, as with `clamped` above.
- Over the view, the pointer shows that it can be dragged: sideways arrows
  over a tall, thin view, up-and-down arrows over a wide, flat one, and a
  hand otherwise.
- A view with `onDrag` doesn't get [`onTap`](#ontap) clicks.
- Doesn't run while the view is disabled or hidden.

## onPointer

```text
.onPointer(_ action: fn(PointerEvent))
```

Runs `action` with everything the mouse does on the view: pressing and
letting go (any button), moving, dragging, scrolling and pinching. See
[Following the mouse](/ui/drawing/#following-the-mouse).

```tessel
Canvas { g in g.fillCircle(x: x, y: y, radius: 6, color: .red) }
    .onPointer { e in
        if e.kind == .down || e.kind == .drag {
            x = e.x
            y = e.y
        }
    }
```

- The block gets a [`PointerEvent`](/reference/colors/#pointerevent): what
  happened (`kind`), where (`x`, `y`, from the view's top left corner inside
  its padding, and `windowX`, `windowY`, from the window's), how far (`dx`,
  `dy`), which `button`, how many `clicks`, and whether `shift`, `alt` or
  `command` was held.
- From `.down` to `.up`, the view has the mouse: `.drag` events keep coming
  when the mouse leaves the view, and the `.up` comes wherever it is let go.
- A scroll over the view goes to it (as a `.scroll` event) rather than to a
  `Scroll` around it.
- A view with `onPointer` doesn't get [`onTap`](#ontap) or
  [`onDrag`](#ondrag). Controls on top of it (in a `ZStack`) still get their
  own clicks.
- Doesn't run while the view is disabled or hidden.

## onKey

```text
.onKey(_ action: fn(String))
```

Runs `action` with each key pressed while no text is being edited.

```tessel
VStack {
    Text("Arrows move the piece")
}
.onKey { key in
    match key {
        "left" -> { column -= 1 }
        "right" -> { column += 1 }
        " " -> { drop() }
        _ -> {}
    }
}
```

- `key` is the character typed (`"a"`, `"A"`, `"+"`, `" "`), or a key's
  name: `"enter"`, `"escape"`, `"tab"`, `"backspace"`, `"delete"`,
  `"left"`, `"right"`, `"up"`, `"down"`, `"home"`, `"end"`.
- While a text field or code editor is being edited, keys go to it, not to
  `onKey`. Keys pressed with Cmd (Ctrl on Windows) go to
  [`shortcut`](#shortcut).
- If several views have `onKey`, the first one (from the top of the view
  tree) that is shown and enabled gets the key.
- A click inside a view with `onKey` (anywhere in it, except in a text
  field or code editor) gives it the keyboard: keys go to it rather than to
  the first view with `onKey`, until a text field or code editor is clicked.
  Pressing a view with [`onPointer`](#onpointer) also ends text editing.
- While a view has the keyboard, keys pressed with Cmd (Ctrl on Windows)
  that no [`shortcut`](#shortcut) takes come to it too, as `"cmd+c"` or
  `"cmd+shift+k"`. Cmd+Z and Cmd+Shift+Z stay undo and redo.
- While a view has the keyboard, a code editor whose
  [`line:`](/ui/code-editor/#following-and-moving-the-cursor) changes only
  scrolls to it, without taking the keyboard.
- When some view has [`onKeyEvent`](#onkeyevent), keys go there instead.

## onKeyEvent

```text
.onKeyEvent(releases: Bool = false, _ action: fn(KeyEvent))
```

Like [`onKey`](#onkey), with the modifier keys held: runs `action` with a
[`KeyEvent`](/reference/colors/#keyevent) for each key pressed while no text
is being edited.

```tessel
Canvas { g in drawMap(g) }
    .onKeyEvent { e in
        let step = if e.shift { 100.0 } else { 10.0 }
        match e.key {
            "left" -> { panX -= step }
            "right" -> { panX += step }
            "z" -> {
                if e.command && e.shift {
                    redo()
                } else if e.command {
                    undo()
                }
            }
            _ -> {}
        }
    }
```

- `e.key` is the key without the modifiers (`"a"` for Shift+A, `"1"` for
  Shift+1) or a key's name: `"enter"`, `"escape"`, `"tab"`, `"space"`,
  `"backspace"`, `"delete"`, `"left"`, `"right"`, `"up"`, `"down"`,
  `"home"`, `"end"`, `"pageup"`, `"pagedown"`, `"insert"`, `"f1"` … `"f12"`.
- `e.text` is what the key types (`"A"` for Shift+A), or `""` for keys that
  type nothing, like the arrows.
- `e.shift`, `e.alt`, `e.command` and `e.control` say which modifier keys
  were held. `command` is Cmd on macOS and Ctrl on Windows and Linux (the
  key shortcuts use); `control` is the Ctrl key itself, on every system.
- `e.repeated` is true for the presses a key held down sends after the first.
- With `releases: true`, `action` also runs when a key is let go (with
  `e.up` true), and for the modifier keys pressed and let go on their own
  (`e.key` is `"shift"`, `"alt"`, `"control"` or `"command"`). That's for
  what's done while a key is held, like panning while Space is down:

  ```tessel
  .onKeyEvent(releases: true) { e in
      if e.key == "space" && !e.repeated {
          panning = !e.up
      }
  }
  ```
- [`shortcut`](#shortcut)s come first: a key with Cmd that a shortcut takes
  doesn't come here. Keys with Cmd that no shortcut takes do.
- While a text field or code editor is being edited, keys go to it, not to
  `onKeyEvent`.
- If several views have `onKeyEvent`, the first one (from the top of the
  view tree) that is shown and enabled gets the key. When a view has
  `onKeyEvent`, no view's [`onKey`](#onkey) gets keys.
- A click inside a view with `onKeyEvent` gives it the keyboard, as for
  [`onKey`](#onkey): keys go to it rather than to the first view with
  `onKeyEvent`, until a text field or code editor is clicked. So two canvases
  side by side, each with its own `onKeyEvent`, each get the keys after a
  click in them.

## cursor

```text
.cursor(_ style: CursorStyle)
```

The pointer shown while the mouse is over the view.

```tessel
Text("Read more").color(.blue).cursor(.hand).onTap { open() }
```

`style` is one of `.arrow`, `.hand`, `.crosshair`, `.grab`, `.grabbing`,
`.text`, `.move`, `.wait`, `.notAllowed`, `.resizeHorizontal`,
`.resizeVertical`, `.zoomIn`, `.zoomOut` (see
[CursorStyle](/reference/colors/#cursorstyle)). The innermost view under
the mouse that has a cursor decides.

## id

```text
.id(_ value: Int or String)
```

Gives the view an identity of its own, instead of its position among its
siblings.

```tessel
for item in items {
    Text(item.name).id(item.id)
}
```

Tessel keeps each view's `state`, text field focus and undo history by
identity. By default the identity is the view's position, so if a list is
reordered, the state stays at the old positions. With `.id(…)`, the state
follows the item. The value must be an `Int` or a `String`, and should be
unique among the views next to it. See [Lists](/ui/lists/).

## tag

```text
.tag(_ value: T)
```

Marks an option in a [`Picker`](/reference/views/#picker) or a page in a
[`TabView`](/reference/views/#tabview). `value` must have the same type as
the picker's or tab view's `selection`.

```tessel
Picker(selection: mode) {
    Text("List").tag(Mode.list)
    Text("Grid").tag(Mode.grid)
}
```

`.tag` is only allowed on the views directly inside a `Picker` or `TabView`
block; anywhere else it's an error.

## shortcut

```text
.shortcut(_ key: String, action: fn())
```

Runs `action` when the user presses Cmd + `key` (Ctrl + `key` on Windows).
The action is usually a block after the call.

```tessel
Button("Save") { save() }
    .shortcut("s") { save() }
```

- `key` is one character that isn't an uppercase letter or whitespace, like
  `"s"`, `"1"` or `","`. The key names `"enter"`, `"escape"`, `"tab"`,
  `"backspace"`, `"delete"`, `"left"`, `"right"`, `"up"`, `"down"`,
  `"home"` and `"end"` work too.
- Put `shift+` or `alt+` (or both) before the key to need Shift or Alt
  (Option) as well: `"shift+z"`, `"alt+shift+s"`. A shortcut without them
  also runs with them held, unless another shortcut asks for exactly those
  keys: with both `"z"` and `"shift+z"`, Shift+Cmd+Z runs the second.
- Any other key, such as `"F5"`, `"S"` or `"cmd+s"`, is an error. Don't
  write Cmd or Ctrl: it's added for you.
- A view can have several shortcuts; each `.shortcut` adds one.
- The shortcut works only while its view is shown and enabled. In a
  `TabView`, only the shortcuts on the selected page work.
- The shortcut belongs to the view it's on, not to a button: attaching it to
  a button doesn't press the button. Put the same code in both, or call one
  function from both.
- While text is being edited, Cmd+A, Cmd+C, Cmd+X, Cmd+V and Cmd+Z keep
  their editing meaning.
- If several views have the same shortcut, the first one (from the top of
  the view tree) runs.

## onAppear

```text
.onAppear(_ action: fn())
```

Runs `action` once when the view appears.

```tessel
Text("{count} files").onAppear {
    count = listFolder(currentFolder())?.count ?? 0
}
```

- "Appears" means the view is there now but wasn't in the previous update:
  when the app starts, or when an `if` or a `for` loop adds it. If the view
  goes away and comes back, `action` runs again. A view made invisible with
  [`.hidden()`](#hidden) is still there, so hiding and showing it doesn't
  run `action`.
- It runs before the view is first drawn. It may change `state`; the screen
  is then updated before anything is drawn.

## onSubmit

```text
.onSubmit(_ action: fn())
```

On a [`TextField`](/reference/views/#textfield): runs `action` when the user
presses Enter while editing it.

```tessel
TextField("New item", text: name)
    .onSubmit { save() }
```

Enter also stops editing the field. (In a `CodeEditor`, Enter starts a new
line instead, so `.onSubmit` doesn't apply.)

## tabItem

```text
.tabItem(_ title: String)
```

The title of a page's tab in a [`TabView`](/reference/views/#tabview). Pages
without one are titled "Untitled".

```tessel
TabView(selection: tab) {
    Text("Welcome!").tabItem("Home").tag("home")
}
```

## autofocus

```text
.autofocus()
```

On a [`TextField`](/reference/views/#textfield) or
[`CodeEditor`](/reference/views/#codeeditor): starts editing it as soon as it
appears, so the user can type right away.

```tessel
TextField("Search", text: name).autofocus()
```

It takes effect when the view appears (see [`onAppear`](#onappear)), not
on every update. If several views appear with `.autofocus()` at the same
time, the last one gets the focus.

## animation

```text
.animation(_ seconds: Float = 0.25, curve: Curve = .easeInOut)
```

Changes to the view, and to the views inside it, take `seconds` to happen
instead of happening at once: a view moves and resizes to its new place, its
[`opacity`](#opacity) and [`background`](#background) color change gradually,
a view that appears fades in, and one that disappears fades out.

```tessel
VStack {
    Text("Details").frame(height: if expanded { 120 } else { 24 })
    Button("More") { expanded = !expanded }
}
.animation()
```

- Inherited: it covers everything inside the view. `.animation(0)` turns it
  off again for a part.
- A change starts slowly, speeds up and slows down at the end; `curve:`
  picks another way, like `.spring` (see [`Curve`](/reference/colors/#curve)).
  A change that comes while another is under way starts from where the view
  is.
- A view that disappears fades out where it was, as it last looked.
  Scrolling and resizing the window are never animated.
- Views are told apart by their position, or by [`.id`](#id): give the views
  of a list an id so that the right ones move when one is added or removed.
- Clicks go to where a view will end up, not to where it's drawn on the way.

## offset

```text
.offset(x: Float = 0, y: Float = 0)
```

Shows the view (and everything in it) that far from where layout put it,
without moving anything else.

```tessel
Text("3").background(.red, radius: 8).offset(x: 10, y: -6)
```

- The space the view takes in its stack stays where it was; only the view
  is shifted. It may end up over its neighbors, or outside its container.
- Clicks go to where the view is shown.
- Unlike `padding`, changing an offset doesn't change any view's size, so a
  [`Canvas(redraw:)`](/reference/views/#canvas) that's shifted keeps its
  drawing.

## clipped

```text
.clipped(_ isClipped: Bool = true)
```

Nothing of what's in the view shows outside its frame.

```tessel
ZStack {
    Image("photo.jpg").offset(x: panX, y: panY)
}
.frame(width: 240, height: 160)
.background(.black, radius: 12)
.clipped()
```

- The corners follow the view's [`background`](#background) or
  [`border`](#border) radius, so content can be given rounded corners.
- What's cut away doesn't get clicks either.

## onResize

```text
.onResize(_ action: fn(Float, Float))
```

Runs `action` with the view's width and height (inside its padding) when it
is first laid out, and again whenever its size changes.

```tessel
Canvas { g in drawMap(g) }
    .onResize { width, height in
        mapWidth = width
        mapHeight = height
    }
```

It may change `state`; the views are then built and laid out again before
anything is drawn.

## onDrop

```text
.onDrop(_ action: fn(String))
```

Runs `action` with the path of each file dropped on the window.

```tessel
VStack {
    Text(if path.isEmpty { "Drop a file here" } else { path.fileName })
}
.onDrop { dropped in path = dropped }
```

The window is one place to drop on: the first view (from the top of the
view tree) with `onDrop` that is shown and enabled gets the files, wherever
in the window they land. Several files dropped together arrive one by one.
To drag views inside the app, see [`draggable`](#draggable).

## draggable

```text
.draggable(_ payload: String)
```

The view can be dragged with the mouse onto a view with
[`dropTarget`](#droptarget), which gets `payload`. See
[Reordering by dragging](/ui/lists/#reordering-by-dragging).

```tessel
Text(layer.name).draggable(layer.id)
```

- The drag starts once the mouse has moved a few points with the button
  held; a press that doesn't move is still a click (for an
  [`onTap`](#ontap) or a button in the view).
- While it's dragged, a see-through copy of the view follows the mouse.
  Escape cancels the drag.
- The payload is a `String`: usually an id, or a few values joined together,
  that tells the drop target what's coming.
- A view with [`onPointer`](#onpointer) gets the mouse first, so it can't
  also be dragged this way. Doesn't work while the view is disabled or hidden.

## dropTarget

```text
.dropTarget(isTargeted: bind Bool = …, _ action: fn(String, Float, Float))
```

Runs `action` when a view with [`draggable`](#draggable) is dropped on this
one, with its payload and where it was dropped (`x`, `y`, from this view's
top left corner inside its padding). The action is usually a block after the
call.

```tessel
HStack { Text(name) }
    .background(if targeted { .highlight } else { .clear })
    .dropTarget(isTargeted: targeted) { payload, x, y in
        moveLayer(payload, before: index)
    }
```

- `isTargeted:` is bound to a `Bool` that is true while something is dragged
  over the view, so it can show where the drop will go. Bind it to a `state`
  of a [custom view](/ui/apps-views/) to give each row of a list its own.
- When drop targets are inside each other, the innermost one under the mouse
  gets the drop.
- Doesn't work while the view is disabled or hidden.

## contextMenu

```text
.contextMenu(_ items: View)
```

A menu that opens where the view is right-clicked (or Ctrl-clicked, on
macOS). Its items are [`MenuItem`](/reference/views/#menuitem)s,
[`MenuDivider`](/reference/views/#menudivider)s and submenus
([`Menu`](/reference/views/#menu)).

```tessel
Text(layer)
    .contextMenu {
        MenuItem("Rename…") { rename(layer) }
        MenuDivider()
        MenuItem("Remove") { remove(layer) }
    }
```

- The menu is drawn in the window, over everything. Arrow keys, Enter and
  Escape work in it; a click outside closes it.
- Shortcuts of its items are only shown.
- On a view with [`onPointer`](#onpointer) too, the right button opens the
  menu and the others go to `onPointer`.

See [Context menus](/ui/menus-windows/#context-menus).

## menuBar

```text
.menuBar(_ items: View)
```

The app's menus, each a [`Menu`](/reference/views/#menu): the menu bar at
the top of the screen on macOS (with the app, Edit and Window menus Tessel
adds), drawn at the top of the window on Windows.

```tessel
VStack { … }
    .menuBar {
        Menu("File") {
            MenuItem("Open…", shortcut: "cmd+o") { open() }
            MenuItem("Save", shortcut: "cmd+s") { save() }.disabled(!edited)
        }
    }
```

Put it on any view of the main window; the first one that has it counts. The
items' shortcuts work wherever the keyboard focus is. See
[The menu bar](/ui/menus-windows/#the-menu-bar).

## popover

```text
.popover(isPresented: bind Bool, edge: Edge = .bottom, content: View)
```

Shows `content` next to the view, over the rest of the window, while
`isPresented` is `true`.

```tessel
Button("Style…") { showStyle = true }
    .popover(isPresented: showStyle) {
        Toggle("Dashed", isOn: dashed)
    }
```

- [`edge`](/reference/colors/#edge) is the side it shows on: `.bottom`,
  `.top`, `.leading` or `.trailing`. When there's no room there, it goes on
  the other side; it always stays in the window.
- A click outside it, or Escape, closes it, setting `isPresented` to `false`.

See [Popovers](/ui/menus-windows/#popovers).

## sheet

```text
.sheet(isPresented: bind Bool, content: View)
```

Shows `content` in a card in the middle of the window while `isPresented` is
`true`. The rest of the window is dimmed and takes no clicks until the
sheet closes.

```tessel
Button("New…") { creating = true }
    .sheet(isPresented: creating) {
        VStack {
            TextField("Name", text: name)
            Button("Create") { dismiss() }
        }
        .frame(width: 280)
    }
```

- [`dismiss()`](/reference/stdlib/#dismiss), Escape, or setting
  `isPresented` to `false` closes it. A click outside it doesn't.
- It's as large as its content.

See [Sheets](/ui/dialogs/#sheets).

## onOpenFile

```text
.onOpenFile(_ action: fn(String))
```

Runs `action` with the path of each file the system asks the app to open:
opened with the app in Finder, dropped on its Dock icon, or the one it was
started to open (on Windows: files it's started with).

```tessel
VStack { … }
    .onOpenFile { path in open(path) }
```

The first view of the main window with `onOpenFile` that's shown and enabled
gets the files; files that come before there's one wait for it. List the
extensions in the app's [`documentTypes:`](/reference/views/#app) for the
system to offer the app for them. See
[Opening documents](/ui/menus-windows/#opening-documents).

## onCloseRequest

```text
.onCloseRequest(_ action: fn())
```

Runs instead of closing when the user closes the window or quits the app.
The window stays open until the app calls
[`closeWindow()`](/reference/stdlib/#closewindow) or
[`quit()`](/reference/stdlib/#quit).

```tessel
VStack { … }
    .onCloseRequest {
        if edited { askToSave = true } else { quit() }
    }
```

On the main window, it also decides what quitting does. In a
[`Window`](/reference/views/#window), it decides for that window. See
[Asking before closing](/ui/menus-windows/#asking-before-closing).

## tooltip

```text
.tooltip(_ text: String)
```

A short text that shows next to the pointer when the mouse rests on the
view for a moment.

```tessel
Icon(.trash).onTap { remove() }.tooltip("Delete the selected rows")
```

It goes away when the mouse leaves the view, and on a click, a key or a
scroll. An empty text shows nothing.

A tooltip is also what a screen reader calls a view that has no words of
its own (see [`accessibilityLabel`](#accessibilitylabel)).

## accessibilityLabel

```text
.accessibilityLabel(_ text: String)
```

What a screen reader (VoiceOver on macOS, Narrator on Windows) calls the
view. Controls with a title are called by it already; this is for the ones
without words, or with words that only make sense to someone looking:

```tessel
Icon(.trash).onTap { remove() }.accessibilityLabel("Delete the selected rows")
Slider(value: volume, from: 0, to: 1).accessibilityLabel("Volume")
```

See [Accessibility](/ui/accessibility/).

## hoverBackground

```text
.hoverBackground(_ color: Color, radius: Float = 0)
```

A background that's shown only while the mouse is over the view. `radius`
rounds the corners.

```tessel
Text("files")
    .padding(4)
    .onTap { save() }
    .hoverBackground(.lightGray, radius: 4)
```

It's drawn on top of a normal [`background`](#background).

Any view can have a hover background. If views with hover backgrounds are
nested, only the innermost one under the pointer shows it.

## textAlignment

```text
.textAlignment(_ alignment: Alignment)
```

Lines up the lines of multi-line text inside the text's own box: `.leading`
(the default), `.center` or `.trailing`.

```tessel
Text("Welcome to Tessel.\nLet's build something.")
    .textAlignment(.center)
```

- Only the horizontal part of the alignment counts: `.topTrailing` acts like
  `.trailing`, and `.top` or `.bottom` like `.center`.
- It doesn't move the text's box. Where the box goes is up to its stack's
  `alignment:` or its frame's.
- Inherited: set it on a stack to align all the text inside.

## rotation

```text
.rotation(_ degrees: Float)
```

Turns the view (and what's in it) clockwise around its middle. It takes
the room of the box around it as it shows, so the views around it move to
make room; clicks follow its shape.

```tessel
Icon(.arrowUp).rotation(heading)
```

## scale

```text
.scale(_ factor: Float = 1, x: Float = 1, y: Float = 1)
```

Makes the view larger or smaller around its middle: across by
`factor * x`, down by `factor * y`. `.scale(2)` doubles it;
`.scale(x: -1)` mirrors it. Like `.rotation`, it takes the room of its new
size.

## trayIcon

```text
.trayIcon(_ icon: IconName, title: String = "", keepRunning: Bool = false, items: View)
```

The app's icon in the menu bar (macOS) or the taskbar's tray (Windows),
with a menu of `MenuItem`s, `MenuDivider`s and `Menu`s. `title` shows beside
it (macOS) or as its tooltip (Windows). With `keepRunning`, closing the
main window hides it and the app goes on; `showWindow()` brings it back.
See [The desktop](/ui/desktop/#a-tray-icon).

## dragFile

```text
.dragFile(_ path: String)
```

The view can be dragged out of the app as the file at `path`: onto the
desktop, into a folder, or into another app. See
[Dragging files out](/ui/desktop/#dragging-files-out).

## lineLimit

```text
.lineLimit(_ lines: Int)
```

Shows at most `lines` lines of text. When there's more, the last line
ends in "…" where it's cut.

```tessel
Text(path).lineLimit(1)
```

- It counts the lines the text wraps onto, and those `\n` starts.
- Inherited: set it on a stack to limit every text inside. `0` is no limit.
- A selectable text that's cut short selects and copies what shows.

## selectable

```text
.selectable(_ isSelectable: Bool = true)
```

The text can be selected with the mouse (a double click picks a word, a
triple click all of it) and the keyboard (Shift with the arrows), and copied
with Cmd+C (Ctrl+C on Windows). It can't be changed.

```tessel
Text("Error 4417: the file is locked").selectable()
```

- Inherited: set it on a stack to make every text inside selectable;
  `.selectable(false)` on one inside takes it away.
- A text with `.onTap` or `.onPointer` takes clicks for those instead.

## gridCellColumns

```text
.gridCellColumns(_ count: Int)
```

In a [`GridRow`](/reference/views/#grid-and-gridrow): the cell takes
`count` columns, the next cell starting after them.

## gridColumnAlignment

```text
.gridColumnAlignment(_ alignment: Alignment)
```

In a [`GridRow`](/reference/views/#grid-and-gridrow): lines up the cells of
the cell's whole column across: `.leading`, `.center` or `.trailing` (only
the horizontal part counts). One cell of the column is enough. A cell over
several columns lines up as it says itself.
