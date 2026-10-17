---
title: Colors, weights and alignment
description: The built-in Color, Weight, Alignment and Appearance enums, with every case and what it does.
---

Tessel has four small built-in enums for styling: `Color`, `Weight`,
`Alignment` and `Appearance`. Like any enum, you usually write a case with a leading dot where
the type is known, as in `.color(.red)`, and you can use the types in your own
code:

```tessel
view Badge(label: String, tint: Color = .blue, align: Alignment = .center) {
    Text(label)
        .color(.white)
        .padding(horizontal: 8, vertical: 2)
        .background(tint, radius: 8)
        .frame(width: 120, alignment: align)
}
```

(Another built-in enum, `IconName`, is listed on the [Icons](/reference/icons/) page.)

## Color

Used by [`.color`](/reference/modifiers/#color),
[`.background`](/reference/modifiers/#background),
[`.border`](/reference/modifiers/#border) and
[`.hoverBackground`](/reference/modifiers/#hoverbackground).

| Case | Light | | Dark | | Notes |
|---|---|---|---|---|---|
| `primary` | <span class="swatch" style="background:#1d1d1f"></span> | `#1d1d1f` | <span class="swatch" style="background:#f5f5f7"></span> | `#f5f5f7` | The default text color. |
| `secondary` | <span class="swatch" style="background:#6e6e73"></span> | `#6e6e73` | <span class="swatch" style="background:#98989d"></span> | `#98989d` | Less important text. |
| `gray` | <span class="swatch" style="background:#8e8e93"></span> | `#8e8e93` | <span class="swatch" style="background:#8e8e93"></span> | `#8e8e93` |  |
| `red` | <span class="swatch" style="background:#ff3b30"></span> | `#ff3b30` | <span class="swatch" style="background:#ff453a"></span> | `#ff453a` |  |
| `orange` | <span class="swatch" style="background:#ff9500"></span> | `#ff9500` | <span class="swatch" style="background:#ff9f0a"></span> | `#ff9f0a` |  |
| `yellow` | <span class="swatch" style="background:#ffcc00"></span> | `#ffcc00` | <span class="swatch" style="background:#ffd60a"></span> | `#ffd60a` |  |
| `green` | <span class="swatch" style="background:#34c759"></span> | `#34c759` | <span class="swatch" style="background:#30d158"></span> | `#30d158` | Also the color of a toggle that's on. |
| `blue` | <span class="swatch" style="background:#007aff"></span> | `#007aff` | <span class="swatch" style="background:#0a84ff"></span> | `#0a84ff` | Also the accent color: button labels, the text caret, the selected tab. |
| `purple` | <span class="swatch" style="background:#af52de"></span> | `#af52de` | <span class="swatch" style="background:#bf5af2"></span> | `#bf5af2` |  |
| `pink` | <span class="swatch" style="background:#ff2d55"></span> | `#ff2d55` | <span class="swatch" style="background:#ff375f"></span> | `#ff375f` |  |
| `white` | <span class="swatch" style="background:#ffffff"></span> | `#ffffff` | <span class="swatch" style="background:#ffffff"></span> | `#ffffff` | The same in both appearances. |
| `black` | <span class="swatch" style="background:#000000"></span> | `#000000` | <span class="swatch" style="background:#000000"></span> | `#000000` | The same in both appearances. |
| `clear` | <span class="swatch"></span> | transparent | <span class="swatch"></span> | transparent | Draws nothing. |
| `lightGray` | <span class="swatch" style="background:#e5e5ea"></span> | `#e5e5ea` | <span class="swatch" style="background:#3a3a3c"></span> | `#3a3a3c` | Subtle backgrounds, hover highlights. |
| `sidebar` | <span class="swatch" style="background:#f3f3f6"></span> | `#f3f3f6` | <span class="swatch" style="background:#252527"></span> | `#252527` | Side panels; also the tab bar of a `TabView`. |
| `highlight` | <span class="swatch" style="background:#d6e4fc"></span> | `#d6e4fc` | <span class="swatch" style="background:#223e68"></span> | `#223e68` | A selected row. |
| `separator` | <span class="swatch" style="background:#d8d8de"></span> | `#d8d8de` | <span class="swatch" style="background:#3e3e42"></span> | `#3e3e42` | Thin lines; also the color of a `Divider`. |
| `background` | <span class="swatch" style="background:#ffffff"></span> | `#ffffff` | <span class="swatch" style="background:#1c1c1e"></span> | `#1c1c1e` | The window's background. |
| `rgb(red, green, blue)` | | yours | | yours | A color of your own. Each value is an `Int` from 0 to 255. |
| `rgba(red, green, blue, opacity)` | | yours | | yours | The same, see-through: `opacity` is a `Float` from 0 (invisible) to 1 (solid). |

Each named color has a value for the light and for the dark appearance, and
the app uses the one for how it's showing (see
[Light and dark](/ui/styling/#light-and-dark)). `white`, `black` and the
colors you make with `rgb` and `rgba` are the same in both.

```tessel
let brand: Color = .rgb(255, 136, 0)
Text("Sale").background(brand, radius: 6)
Text("Note").background(.rgba(255, 136, 0, 0.2), radius: 6)
```

- `rgb` and `rgba` take their values without labels. Values outside their
  range are brought into it: `.rgb(300, -5, 0)` is `.rgb(255, 0, 0)`.
- The named colors are fully opaque except `clear`. Besides `rgba`,
  [`.opacity`](/reference/modifiers/#opacity) fades a whole view.
- Printing a color gives its case's name: `red`, or `rgb` for one of your own.

A typical sidebar row, using several of them:

```tessel
HStack {
    Icon(.fileText).color(.secondary)
    Text("main.tsl")
    Spacer()
}
.padding(horizontal: 8, vertical: 4)
.background(.highlight, radius: 4)
```

## Weight

Used by [`.font(weight:)`](/reference/modifiers/#font).

| Case | Font weight |
|---|---|
| `regular` | 400, the default |
| `medium` | 500 |
| `semibold` | 600 |
| `bold` | 700 |

```tessel
Text("Heading").font(size: 20, weight: .semibold)
```

## Alignment

Used by the `alignment:` of [`VStack`](/reference/views/#vstack),
[`HStack`](/reference/views/#hstack), [`ZStack`](/reference/views/#zstack)
and [`.frame`](/reference/modifiers/#frame), and by
[`.textAlignment`](/reference/modifiers/#textalignment).

An alignment names a position in a box: a horizontal part (leading, center
or trailing) and a vertical part (top, center or bottom). "Leading" is the
left side and "trailing" the right side.

| Case | Horizontal | Vertical |
|---|---|---|
| `topLeading` | leading | top |
| `top` | center | top |
| `topTrailing` | trailing | top |
| `leading` | leading | center |
| `center` | center | center |
| `trailing` | trailing | center |
| `bottomLeading` | leading | bottom |
| `bottom` | center | bottom |
| `bottomTrailing` | trailing | bottom |

Where it's used decides which parts count:

| Used in | Default | What counts |
|---|---|---|
| `ZStack(alignment:)` | `.center` | Both parts: where each layer sits in the stack. |
| `.frame(alignment:)` | see [frame](/reference/modifiers/#frame) | Both parts: where the content sits in a larger frame. |
| `VStack(alignment:)` | `.center` | Only the horizontal part: how the views line up across the stack. `.topLeading` acts like `.leading`, and `.top` and `.bottom` like `.center`. |
| `HStack(alignment:)` | `.center` | Only the vertical part: `.topLeading` acts like `.top`, `.bottomTrailing` like `.bottom`. `.leading` and `.trailing` act like `.top` and `.bottom`. |
| `.textAlignment(…)` | `.leading` | Only the horizontal part: how the lines of a text line up. |

```tessel
VStack(alignment: .leading) {
    Text("Name").font(weight: .bold)
    Text("Ada Lovelace")
}
ZStack(alignment: .bottomTrailing) {
    Text("Photo").frame(width: 120, height: 80).background(.lightGray)
    Text("new").font(size: 10).padding(4)
}
```

See [Alignment](/ui/alignment/) for a guided tour with pictures.

## Appearance

Used by the `appearance:` of [`app`](/reference/views/#app).

| Case | The app looks… |
|---|---|
| `system` | light or dark as the system does, and changes with it. The default. |
| `light` | always light. |
| `dark` | always dark. |

```tessel
app Player(appearance: .dark) {
    Text("Now playing")
}
```

[`isDarkMode()`](/reference/stdlib/#isdarkmode) tells which one the app is
showing in.

## CursorStyle

Used by [`.cursor`](/reference/modifiers/#cursor): the pointer shown over a
view.

| Case | The pointer |
|---|---|
| `arrow` | The usual arrow. |
| `hand` | A pointing hand, for links. |
| `crosshair` | A cross, for picking a point or drawing. |
| `grab`, `grabbing` | An open hand, and a closed one, for things that are moved. |
| `text` | The I-beam of text. |
| `move` | Arrows in four directions. |
| `wait` | Busy. |
| `notAllowed` | A crossed circle. |
| `resizeHorizontal`, `resizeVertical` | Arrows sideways, and up and down. |
| `zoomIn`, `zoomOut` | A magnifying glass with a plus or a minus. |

## BlendMode

Used by [`g.beginLayer`](/reference/views/#canvas): how a layer's colors mix
with the colors under it. The names and the arithmetic are the ones photo
editors and CSS use.

| Case | The result |
|---|---|
| `normal` | The layer's colors, over what's under it (the default). |
| `multiply` | The two multiplied: darker. White changes nothing. |
| `screen` | The opposite of multiply: lighter. Black changes nothing. |
| `overlay` | Multiply where what's under is dark, screen where it's light. |
| `darken`, `lighten` | The darker, or the lighter, of the two. |
| `colorDodge`, `colorBurn` | What's under brightened, or darkened, by the layer. |
| `hardLight`, `softLight` | Multiply or screen depending on the layer's colors: a harsh light, and a soft one. |
| `difference` | The lighter minus the darker. White inverts; black changes nothing. |
| `exclusion` | Like difference, with less contrast. |
| `hue`, `saturation`, `color`, `luminosity` | One part of the layer's color (its hue; its saturation; its hue and saturation; its lightness), with the rest from what's under it. |

## Edge

A side of a view, for [`.popover(edge:)`](/reference/modifiers/#popover):
`top`, `bottom`, `leading` (the left) and `trailing` (the right).

## RibbonSize

How big a ribbon's [button, toggle or menu](/reference/views/#ribbonbutton)
is: `small` (its icon next to its label; the default) or `large` (its icon
over its label, as tall as its group).

## Curve

How an [`.animation`](/reference/modifiers/#animation) moves from one state
to the next: `easeInOut` (slow, fast, slow; the default), `easeIn`,
`easeOut`, `linear`, `spring` (a little past the end, and back) and `bouncy`
(further past, and back and forth). See
[How a change moves](/ui/styling/#how-a-change-moves).

## PointerEvent

What [`.onPointer`](/reference/modifiers/#onpointer) tells its block. A
struct with these fields:

| Field | Type | |
|---|---|---|
| `kind` | `PointerKind` | What happened (below). |
| `x`, `y` | `Float` | Where the mouse is, in points from the view's top left corner (inside its padding). During a drag they can be outside the view, and negative. |
| `windowX`, `windowY` | `Float` | Where the mouse is, in points from the window's top left corner: for placing a popup of your own, or comparing with other views. |
| `dx`, `dy` | `Float` | For `.move` and `.drag`: how far the mouse moved since the event before. For `.scroll`: how far to scroll, across and down (positive `dy`: towards what's further down). For `.pinch`: `dy` is how much larger (0.1: a tenth larger; negative: smaller). |
| `button` | `PointerButton` | `.left`, `.right` or `.middle`, for `.down`, `.drag` and `.up`. |
| `clicks` | `Int` | For `.down` and `.up`: 1 for a click, 2 for the second click of a double click, and so on. |
| `shift`, `alt`, `command` | `Bool` | Whether the key was held. `command` is Cmd on macOS and Ctrl on Windows; `alt` is Option on macOS. |

`PointerKind` has the cases `down`, `up`, `move`, `drag`, `scroll`,
`pinch` and `leave`:

| Case | When |
|---|---|
| `down` | A button was pressed on the view. |
| `drag` | The mouse moved with that button held, on the view or off it. |
| `up` | The button was let go. |
| `move` | The mouse moved over the view with no button held. |
| `leave` | The mouse left the view (or the window). |
| `scroll` | The mouse wheel, or two fingers on a trackpad. |
| `pinch` | Two fingers pinched on a trackpad (macOS only). |

```tessel
.onPointer { e in
    match e.kind {
        .down -> { start = (e.x, e.y) }
        .drag -> { offset = (offset.0 + e.dx, offset.1 + e.dy) }
        .scroll -> { zoom = zoom * exp(-e.dy * 0.01) }
        _ -> {}
    }
}
```

## KeyEvent

What [`.onKeyEvent`](/reference/modifiers/#onkeyevent) tells its block. A
struct with these fields:

| Field | Type | |
|---|---|---|
| `key` | `String` | The key, without the modifiers: `"a"` for Shift+A, `"1"` for Shift+1, or a key's name: `"enter"`, `"escape"`, `"tab"`, `"space"`, `"backspace"`, `"delete"`, `"left"`, `"right"`, `"up"`, `"down"`, `"home"`, `"end"`, `"pageup"`, `"pagedown"`, `"insert"`, `"f1"` … `"f12"`. |
| `text` | `String` | What the key types (`"A"` for Shift+A), or `""` for a key that types nothing. |
| `shift`, `alt`, `command`, `control` | `Bool` | Whether the key was held. `command` is Cmd on macOS and Ctrl on Windows and Linux; `control` is the Ctrl key on every system; `alt` is Option on macOS. |
| `repeated` | `Bool` | True for the presses a key held down sends after the first one. |
| `up` | `Bool` | True when the key was let go. Only with [`.onKeyEvent(releases: true)`](/reference/modifiers/#onkeyevent). |

```tessel
.onKeyEvent { e in
    if e.key == "delete" && !e.repeated {
        deleteSelected()
    } else if e.key == "a" && e.command {
        selectAll()
    }
}
```
