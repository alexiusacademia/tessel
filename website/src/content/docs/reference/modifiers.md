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
.font(size: Float = inherited, weight: Weight = inherited)
```

Sets the text size (in points) and weight.

```tessel
Text("Title").font(size: 24, weight: .bold)
Text("Note").font(size: 11)
```

- Only the parts you pass change; the rest come from the view's container.
  In a window, text starts at **14 points**, `.regular`.
- `weight` is one of `.regular`, `.medium`, `.semibold`, `.bold` (see
  [Weight](/reference/colors/#weight)).
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
  own size and sits at that position in the frame. Other views, like buttons
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

- `key` is a letter, digit or other character, like `"s"`, `"1"` or `","`.
  Upper and lower case are the same, and Shift isn't taken into account:
  `.shortcut("s")` also runs on Shift+Cmd+S. The key names `"enter"`,
  `"escape"`, `"tab"`, `"backspace"`, `"delete"`, `"left"`, `"right"`,
  `"up"`, `"down"`, `"home"` and `"end"` work too.
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

:::note
Only views you can click show a hover background: buttons, toggles, text
fields, code editors, pickers, tab views, and views with
[`.onTap`](#ontap). On other views it never appears.
:::

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
