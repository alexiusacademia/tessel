---
title: Ribbons
description: "A ribbon of tabs and groups of commands, as office apps have: large and small buttons, toggles, menus, and other views, which shrink to fit a narrow window."
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

## When the window is narrow

When the groups don't fit, the ribbon shows them smaller, starting from the
last group: their large items become small, and stack three to a column.
Groups that still don't fit are cut off at the window's edge, so put the
groups used most first.

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

A group shown smaller says `small`.
