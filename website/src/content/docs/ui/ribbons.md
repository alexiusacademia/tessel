---
title: Ribbons
description: "A ribbon of tabs and groups of commands, as office apps have: large and small buttons, toggles, menus, and other views, which shrink and then fold into buttons to fit a narrow window."
---

A ribbon is the band of commands across the top of an office app: tabs
along the top (Home, Insert, …), and under the selected one, groups of
buttons (Clipboard, Font, …), each with its name underneath. It suits an
app with many commands, like a spreadsheet or a map editor, better than a
menu bar alone: the common ones are a click away, with their icons.

## A first ribbon

```tessel
app Sheet(width: 640, height: 190) {
    state tab = "Home"
    state bold = false
    state italic = false
    state status = "Ready"

    VStack(spacing: 0) {
        Ribbon(selection: tab) {
            RibbonTab("Home") {
                RibbonGroup("Clipboard") {
                    RibbonButton("Paste", icon: .paste, size: .large) { status = "Pasted" }
                    RibbonButton("Cut", icon: .cut) { status = "Cut" }
                    RibbonButton("Copy", icon: .copy) { status = "Copied" }
                }
                RibbonGroup("Font") {
                    RibbonToggle("Bold", icon: .bold, isOn: bold)
                    RibbonToggle("Italic", icon: .italic, isOn: italic)
                    RibbonMenu("Borders", icon: .borders) {
                        MenuItem("All Borders") { status = "All borders" }
                        MenuItem("No Border") { status = "No border" }
                    }
                }
                RibbonGroup("Editing") {
                    RibbonButton("Sum", icon: .sum, size: .large) { status = "Summed" }
                    RibbonMenu("Sort", icon: .sortAscending, size: .large) {
                        MenuItem("A to Z") { status = "Sorted A to Z" }
                        MenuItem("Z to A") { status = "Sorted Z to A" }
                    }
                }
            }
            RibbonTab("Insert") {
                RibbonGroup("Charts") {
                    RibbonButton("Column", icon: .chart, size: .large) { status = "Column chart" }
                    RibbonButton("Line", icon: .chartLine) { status = "Line chart" }
                    RibbonButton("Pie", icon: .chartPie) { status = "Pie chart" }
                }
            }
        }
        Text(status).padding(10)
        Spacer()
    }
}
```

![A ribbon with the tabs Home (selected) and Insert; under them the groups Clipboard (a large Paste button, and Cut and Copy), Font (Bold, switched on, Italic and Borders with an arrow) and Editing (large Sum and Sort buttons); under the ribbon, the text Ready](../../../assets/screens/ui-ribbon.png)

- `Ribbon(selection:)` holds the tabs. `selection` is a `String` `state`
  with the title of the tab that's shown; clicking a tab sets it, and
  setting it shows another tab.
- `RibbonTab("Home")` holds the tab's groups, and `RibbonGroup("Clipboard")`
  a group's items. The group's name goes under them.
- A ribbon is as wide as the space it's in, and as tall as it needs to be
  (126 points). Put it first in a `VStack(spacing: 0)` above the app's
  content.

## Buttons, toggles and menus

Each item has a label and one of the [built-in icons](/reference/icons/)
(they include cut, paste, bold, italic, the alignments, table, chart, sort,
filter, sum, function and more, for ribbons and toolbars):

- `RibbonButton("Cut", icon: .cut) { … }` runs its block when clicked.
- `RibbonToggle("Bold", icon: .bold, isOn: bold)` switches a `Bool` on and
  off, and shows when it's on.
- `RibbonMenu("Sort", icon: .sortAscending) { … }` opens a menu of
  `MenuItem`s, `MenuDivider`s and submenus, as a
  [context menu](/ui/menus-windows/#context-menus) does.

`size: .large` makes an item large: its icon over its label, the height of
the group. Items are small unless they say so: the icon next to the label,
three to a column. Large items are for the commands used most, like Paste.

A group can hold other views too, such as a `Picker` of fonts or a
`TextField` for a cell's value. They go in the group's columns like small
items, taking the height they need.

## A ribbon in several files

A large ribbon doesn't have to be written in one place. A view of your own
can make a tab, a group, or some of a group's items, and the ribbon uses
them as if they were written in it:

```tessel
// home.tsl
view HomeTab() {
    RibbonTab("Home") {
        ClipboardGroup()
        RibbonGroup("Font") {
            StyleButtons()
        }
    }
}

view ClipboardGroup() {
    RibbonGroup("Clipboard") {
        RibbonButton("Paste", icon: .paste, size: .large) { paste() }
    }
}

view StyleButtons() {
    RibbonButton("Bold", icon: .bold) { toggleBold() }
    RibbonButton("Italic", icon: .italic) { toggleItalic() }
}
```

```tessel
Ribbon(selection: tab) {
    HomeTab()
    InsertTab()
    ViewTab()
}
```

## When the window is narrow

When the groups don't fit, the ribbon makes room in two steps, each starting
from the last group:

1. **Smaller:** a group's large items become small, and stack three to a
   column.
2. **Folded:** a group becomes one button, with its icon and its title. A
   click on it opens the group under it, at full size; choosing an item
   there, clicking elsewhere, or Escape closes it again. (A group that's
   no wider small than its button would be, like one with a single column
   of items, stays small.)

So every command stays within reach however narrow the window is. Put the
groups used most first: they keep their room longest.

A folded group's button shows its first item's icon. `icon:` chooses
another, one of the [built-in icons](/reference/icons/):

```tessel
RibbonGroup("Editing", icon: .sum) {
    RibbonButton("Sum", icon: .sum, size: .large) { sum() }
    RibbonButton("Find", icon: .search, size: .large) { find() }
}
```

![A narrow window's ribbon: the Clipboard group with small Paste, Cut and Copy; the Font and Editing groups folded into buttons with a B and a Σ; the Editing button is open, with large Sum and Find buttons and small Sort, Filter and Clear under it, in a card titled Editing](../../../assets/screens/ui-ribbon-folded.png)

A text field or a picker in an open group keeps it open while it's used; a
`RibbonButton`, `RibbonToggle` or `RibbonMenu` closes it once it's chosen.

## Testing

In a [test script](/ui/testing/), `click "Insert"` selects a tab and
`click "Paste"` clicks an item, by their titles. A `RibbonMenu`'s menu
opens with `click "Sort"`, and `click "A to Z"` chooses its item. `tree`
shows the ribbon with its selected tab:

```text
Ribbon ["Home", "Insert"] selected "Home"
  RibbonTab "Home"
    RibbonGroup "Clipboard"
      RibbonButton "Paste" large
      RibbonButton "Cut"
      RibbonButton "Copy"
    RibbonGroup "Font"
      RibbonToggle "Bold" = true
      …
```

A group shown smaller says `small`, a folded one `collapsed` (and `open`
while it's open). `click "Editing"` opens a folded group by its title, and
its items can be clicked once it's open.
