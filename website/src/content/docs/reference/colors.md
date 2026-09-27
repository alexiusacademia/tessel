---
title: Colors, weights and alignment
description: The built-in Color, Weight and Alignment enums, with every case and what it does.
---

Tessel has three small built-in enums for styling: `Color`, `Weight` and
`Alignment`. Like any enum, you usually write a case with a leading dot where
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

(The fourth built-in enum, `IconName`, is listed on the [Icons](/reference/icons/) page.)

## Color

Used by [`.color`](/reference/modifiers/#color),
[`.background`](/reference/modifiers/#background),
[`.border`](/reference/modifiers/#border) and
[`.hoverBackground`](/reference/modifiers/#hoverbackground).

| | Case | RGB | Notes |
|---|---|---|---|
| <span class="swatch" style="background:#1d1d1f"></span> | `primary` | `#1d1d1f` | The default text color. |
| <span class="swatch" style="background:#6e6e73"></span> | `secondary` | `#6e6e73` | Less important text. |
| <span class="swatch" style="background:#8e8e93"></span> | `gray` | `#8e8e93` | |
| <span class="swatch" style="background:#ff3b30"></span> | `red` | `#ff3b30` | |
| <span class="swatch" style="background:#ff9500"></span> | `orange` | `#ff9500` | |
| <span class="swatch" style="background:#ffcc00"></span> | `yellow` | `#ffcc00` | |
| <span class="swatch" style="background:#34c759"></span> | `green` | `#34c759` | Also the color of a toggle that's on. |
| <span class="swatch" style="background:#007aff"></span> | `blue` | `#007aff` | Also the accent color: button labels, the text caret, the selected tab. |
| <span class="swatch" style="background:#af52de"></span> | `purple` | `#af52de` | |
| <span class="swatch" style="background:#ff2d55"></span> | `pink` | `#ff2d55` | |
| <span class="swatch" style="background:#ffffff"></span> | `white` | `#ffffff` | Also the window background. |
| <span class="swatch" style="background:#000000"></span> | `black` | `#000000` | |
| <span class="swatch"></span> | `clear` | transparent | Draws nothing. |
| <span class="swatch" style="background:#e5e5ea"></span> | `lightGray` | `#e5e5ea` | Subtle backgrounds, hover highlights. |
| <span class="swatch" style="background:#f3f3f6"></span> | `sidebar` | `#f3f3f6` | Side panels; also the tab bar of a `TabView`. |
| <span class="swatch" style="background:#d6e4fc"></span> | `highlight` | `#d6e4fc` | A selected row. |
| <span class="swatch" style="background:#d8d8de"></span> | `separator` | `#d8d8de` | Thin lines; also the color of a `Divider`. |

All colors are fully opaque except `clear`. To make a color lighter, combine
it with [`.opacity`](/reference/modifiers/#opacity), which fades the whole
view.

:::note
These seventeen are the only colors for now: there's no way to make a color
from your own RGB values, and no dark mode.
:::

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
