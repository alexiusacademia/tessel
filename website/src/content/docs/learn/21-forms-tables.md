---
title: 21. Forms, tables and rich text
description: Lay out forms with Grid, choose with radio buttons and menus, search, link, cut text short or let it be copied, edit tables in place, write with styles in a RichTextEditor, and turn and scale views.
---

Lesson 19 built windows from stacks, buttons and text fields, and lesson 20
built a whole app from them. Real apps need a few more pieces: forms whose
fields line up, ways to choose that fit the choice, tables people edit
where they are, and text with bold and colors in it.

In this lesson you'll learn:

- how a `Grid` lines up a form's labels and fields
- the three ways a `Picker` can look: segments, radio buttons and a menu
- `SearchField` and `Link`
- how to cut text short with `.lineLimit`, and let it be copied with `.selectable`
- how to edit a table's cells in place
- how `RichTextEditor` and `RichText` hold text with styles
- how to turn and scale a view
- split windows, long rows, and scrolling from code

You'll finish with a small notes app that uses most of them.

## Forms that line up

A form is labels and fields. With stacks, each row is an `HStack`, and the
labels are only as wide as their text, so the fields start at different
places. A **`Grid`** lines them up in columns:

```tessel
app Signup(width: 420, height: 260) {
    state name = ""
    state email = ""
    state age = ""

    Grid(spacing: 10) {
        GridRow {
            Text("Name").gridColumnAlignment(.trailing)
            TextField("", text: name)
        }
        GridRow {
            Text("Email address")
            TextField("", text: email)
        }
        GridRow {
            Text("Age")
            TextField("", text: age).frame(width: 60)
        }
    }
    .padding(20)
}
```

Each `GridRow` is a row, and each view in it a cell. A column is as wide as
its widest cell, so all the fields start after "Email address".
`.gridColumnAlignment(.trailing)` on one label lines up its whole column on
the right, which is how forms usually look. The text fields share the room
that's left, so a wider window has wider fields.

Two more things a grid does:

- `.gridCellColumns(2)` makes a cell take two columns, for a note or a
  button across the form.
- A view that isn't in a `GridRow`, like a `Divider()`, is a row of its own
  as wide as the grid.

## Three kinds of choice

A `Picker` chooses one value out of a few. Its `style:` says how it looks:

```tessel
enum Size { small, medium, large }
enum Color3 { red, green, blue }

app Choices(width: 420, height: 300) {
    state size = Size.medium
    state color = Color3.red
    state city = "Manila"

    VStack(spacing: 16, alignment: .leading) {
        Picker(selection: size) {
            Text("Small").tag(Size.small)
            Text("Medium").tag(Size.medium)
            Text("Large").tag(Size.large)
        }
        Picker(selection: color, style: .radio) {
            Text("Red").tag(Color3.red)
            Text("Green").tag(Color3.green)
            Text("Blue").tag(Color3.blue)
        }
        Picker(selection: city, style: .menu) {
            for c in ["Manila", "Cebu", "Davao", "Baguio"] {
                Text(c).tag(c)
            }
        }
        Text("{size}, {color}, {city}")
    }
    .padding(20)
}
```

- **Segments** (the default) suit two to five short options that are
  always visible, like a view mode.
- **Radio buttons** (`.radio`) put each option on a line of its own. They
  suit options that need words, in forms.
- **A menu** (`.menu`) shows only the chosen option until it's clicked.
  It suits long lists, like the cities above, which a `for` makes.

In each, `.tag(…)` says which value an option stands for, and the arrow
keys change the choice once the picker has the keyboard.

## Searching and linking

A **`SearchField`** is a text field for what to look for, with a
magnifying glass and a button that clears it:

```tessel
app Contacts(width: 320, height: 300) {
    state query = ""
    let names = ["Ada Lovelace", "Alan Turing", "Grace Hopper", "Katherine Johnson"]
    let shown = names.filter { n in query == "" || n.lowercase().contains(query.lowercase()) }

    VStack(alignment: .leading) {
        SearchField("Search contacts", text: query)
        for name in shown {
            Text(name)
        }
        if shown.isEmpty {
            Text("No one called \"{query}\"").color(.secondary)
        }
        Spacer()
        Link("About these people", url: "https://en.wikipedia.org/wiki/Women_in_computing")
    }
    .padding(12)
}
```

The list is worked out from `query` each time the body runs, so it changes
as you type. A **`Link`** is text that opens a web page (or runs an
`action:`) when it's clicked.

## Text that fits, and text to copy

Two modifiers change how a `Text` behaves:

- **`.lineLimit(n)`** shows at most `n` lines, and ends the last in "…"
  when there's more. Use it for file paths, names and previews in narrow
  places.
- **`.selectable()`** lets the user select the text with the mouse and copy
  it. Use it for results, error messages and ids people want to paste
  somewhere else.

```tessel
app Details(width: 300, height: 160) {
    VStack(spacing: 8, alignment: .leading) {
        Text("/Users/ada/Documents/Projects/Engine/notes/2026/october.txt")
            .lineLimit(1)
        Text("Order number: 4417-2209-A").selectable()
    }
    .padding(16)
}
```

## Tables you edit in place

A [`Table`](/ui/tables/) shows rows in columns. A column with **`onEdit:`**
lets the user double-click a cell, type, and press Enter, like a
spreadsheet. `onEdit` gets the row and what was typed, and puts it in your
data:

```tessel
struct Item {
    name: String
    count: Int
}

app Stock(width: 400, height: 260) {
    state items = [Item(name: "Bolts", count: 120), Item(name: "Nuts", count: 80)]
    state selected: [Int] = []

    VStack {
        Table(rows: items.count, selection: selected) {
            TableColumn("Item", onEdit: { i, text in items[i].name = text }) { i in
                Text(items[i].name)
            }
            TableColumn("Count", width: 80, alignment: .trailing, onEdit: { i, text in
                if let n = Int(text) {
                    items[i].count = n
                }
            }) { i in
                Text("{items[i].count}")
            }
        }
        Button("Add a row") { items.append(Item(name: "New item", count: 0)) }
    }
    .padding(12)
}
```

The count column only takes numbers: when `Int(text)` is `nil`, nothing
changes, and the cell shows its old value again. Escape drops what was
typed, and Tab goes on to the next cell that can be edited.

## Text with styles

A **`RichTextEditor`** is a field to write in with bold, italic,
underline, strikethrough, sizes, colors and fonts, with a bar of buttons for
them. What's written is a **`RichText`**: the text, and the stretches of it
that have styles (its *runs*).

```tessel
app Note(width: 520, height: 360) {
    state note = RichText("Write something, then make part of it bold.")

    VStack(alignment: .leading) {
        RichTextEditor(text: note)
        Text(note.markdown).lineLimit(1).color(.secondary)
    }
    .padding(12)
}
```

`note.markdown` is the text in Markdown (`**bold**`, `*italic*`), which is
handy for showing a preview or saving to a `.md` file. To keep everything,
sizes and colors too, save the `RichText` itself with `toJson`, as
[lesson 16](/learn/16-files-data/) saved other structs. See
[Rich text](/ui/rich-text/) for more.

## Turning and scaling

`.rotation(degrees)` turns a view, clockwise, and `.scale(…)` makes it
larger or smaller. The view takes the room of its new shape, so the views
around it move to fit:

```tessel
app Compass(width: 260, height: 260) {
    state heading = 45.0

    VStack(spacing: 16) {
        Icon(.arrowUp)
            .font(size: 40)
            .rotation(heading)
        Slider(value: heading, from: 0, to: 360)
        Text("{Int(heading)}°").scale(1.5)
    }
    .padding(20)
}
```

`.scale(x: 2, y: 1)` stretches across only. Clicks follow the view's new
shape.

## Split windows and long rows

Three more pieces for laying out a bigger window:

- **`HSplit`** puts two views side by side with a line between them that
  the user drags; **`VSplit`** puts one above the other. `size:` can bind
  the first one's width.
- **`Scroll(.horizontal)`** scrolls sideways (`Scroll(.both)`, both ways),
  and **`LazyHStack`** builds only the columns of a long row that show, as
  `LazyVStack` does for rows.
- **`scrollTo(id)`** scrolls to the view with that `.id`, or to that row
  (column) of a lazy list.

```tessel
app Calendar365(width: 640, height: 300) {
    state day = 0

    HSplit {
        VStack(alignment: .leading) {
            Text("Day {day + 1}").font(size: 20, weight: .semibold)
            Button("Today") {
                day = 120
                scrollTo(day, align: .center)
            }
        }
        .padding(12)
        .frame(minWidth: 140)
        Scroll(.horizontal) {
            LazyHStack(count: 365, columnWidth: 56) { i in
                Text("{i + 1}")
                    .font(weight: if i == day { .bold } else { .regular })
                    .onTap { day = i }
            }
        }
    }
}
```

See [Layout](/ui/layout/#split-views) and [Lists](/ui/lists/#sideways-with-lazyhstack).

## Worked example: a notes app

Put the pieces together: a table of notes whose titles are edited in
place, a search field to find one, and a rich text editor for the note
that's selected.

```tessel
struct Note {
    title: String
    body: RichText
}

app Notes(width: 760, height: 480) {
    state notes = [
        Note(title: "Shopping", body: RichText("Milk, eggs, bread")),
        Note(title: "Ideas", body: RichText("A calendar for the garden")),
    ]
    state selected: [Int] = [0]
    state query = ""

    // The numbers of the notes whose titles match.
    let shown = notes.enumerated().filter { p in
        query == "" || p.1.title.lowercase().contains(query.lowercase())
    }.map { p in p.0 }

    HSplit {
        VStack(alignment: .leading) {
            SearchField(text: query)
            Table(rows: shown.count, selection: selected) {
                TableColumn("Note", onEdit: { row, text in notes[shown[row]].title = text }) { row in
                    Text(notes[shown[row]].title).lineLimit(1)
                }
            }
            Button("New note") {
                notes.append(Note(title: "Untitled", body: RichText("")))
                query = ""
                selected = [notes.count - 1]
            }
        }
        .padding(8)
        .frame(minWidth: 200)
        VStack(alignment: .leading) {
            let row = selected.first ?? -1
            if row >= 0 && row < shown.count {
                Text(notes[shown[row]].title).font(size: 18, weight: .semibold)
                RichTextEditor(text: notes[shown[row]].body)
            } else {
                Text("Choose a note").color(.secondary)
            }
        }
        .padding(8)
    }
}
```

Some things to notice:

- `shown` holds the **numbers** of the notes that match the search, not
  copies of them: `enumerated()` pairs each note with its number, and
  `p.0` is the number. The table's row `row` is note `shown[row]`, so editing a
  title, or the body, changes the real note.
- The selection is a row number of the table as it shows, so a new note
  clears the search first, then selects the last row.
- The editor is bound straight to the selected note's body:
  `notes[shown[row]].body`.

Check it without a window:

```sh
TESSEL_SCRIPT='click "New note"; doubleclick 40 130; type Plans; key enter; tree' tessel run notes.tsl
```

The tree shows the third note's title, "Plans", in the table and above
the editor.

## Common mistakes

### A view outside `GridRow`

```tessel
Grid {
    Text("Name")
    TextField("", text: name)
}
```

Views directly in a `Grid` are rows of their own, so this is two rows of
one cell each, not a label beside a field. Put a row's cells in a
`GridRow { … }`.

### Tags of the wrong type

Every `.tag` of a picker must have the same type as its `selection`. With
`state city = "Manila"`, `.tag(1)` is an error: the tags are city names,
`.tag("Cebu")`.

### Trusting what's typed in a cell

`onEdit` gets text. For a number, convert it and decide what happens when
it isn't one, as `Int(text)` does above, or your data could end up with
`0` everywhere someone typed a letter.

### Saving rich text as Markdown

`note.markdown` leaves out what Markdown doesn't have: underline, sizes,
colors and fonts. Save with `toJson(note)` to keep everything.

## Exercises

**1. A settings form.** Make a form with a grid: a text field for a name,
radio buttons for a theme (light, dark, automatic), and a menu picker for
a font size (12, 14, 16, 18). Under it, a text says what was chosen.

<details>
<summary>Solution</summary>

```tessel
enum Theme { light, dark, automatic }

app Settings(width: 420, height: 280) {
    state name = ""
    state theme = Theme.automatic
    state size = 14

    VStack(spacing: 16, alignment: .leading) {
        Grid(spacing: 10) {
            GridRow {
                Text("Name").gridColumnAlignment(.trailing)
                TextField("", text: name)
            }
            GridRow {
                Text("Theme")
                Picker(selection: theme, style: .radio) {
                    Text("Light").tag(Theme.light)
                    Text("Dark").tag(Theme.dark)
                    Text("Automatic").tag(Theme.automatic)
                }
            }
            GridRow {
                Text("Font size")
                Picker(selection: size, style: .menu) {
                    for s in [12, 14, 16, 18] {
                        Text("{s} points").tag(s)
                    }
                }
            }
        }
        Text("{name}: {theme}, {size} points").color(.secondary)
    }
    .padding(20)
}
```

</details>

**2. Marks out of 100.** Make a table of students with two columns, name
and mark, both edited in place. A mark only changes when what's typed is a
whole number from 0 to 100. Under the table, show the average.

<details>
<summary>Solution</summary>

```tessel
struct Student {
    name: String
    mark: Int
}

app Marks(width: 400, height: 300) {
    state students = [Student(name: "Ana", mark: 88), Student(name: "Ben", mark: 72)]
    state selected: [Int] = []

    let total = students.map { s in s.mark }.sum()

    VStack {
        Table(rows: students.count, selection: selected) {
            TableColumn("Name", onEdit: { i, text in students[i].name = text }) { i in
                Text(students[i].name)
            }
            TableColumn("Mark", width: 70, alignment: .trailing, onEdit: { i, text in
                if let m = Int(text) {
                    if m >= 0 && m <= 100 {
                        students[i].mark = m
                    }
                }
            }) { i in
                Text("{students[i].mark}")
            }
        }
        Text("Average: {Float(total) / Float(max(students.count, 1))}")
    }
    .padding(12)
}
```

</details>

**3. A styled greeting.** Start a `RichTextEditor` with the text "Hello,
world", with "Hello" in bold and red. Show its Markdown under it.

<details>
<summary>Solution</summary>

```tessel
app Greeting(width: 480, height: 300) {
    state text = RichText("Hello, world", runs: [TextRun(start: 0, end: 5, bold: true, color: .red)])

    VStack(alignment: .leading) {
        RichTextEditor(text: text)
        Text(text.markdown).color(.secondary)
    }
    .padding(12)
}
```

`TextRun(start: 0, end: 5, …)` is on the characters 0 to 4, "Hello". The
Markdown is `**Hello**, world`: Markdown has no colors.

</details>

## Summary

- **`Grid`** with **`GridRow`**s lines up a form's labels and fields;
  `.gridColumnAlignment` and `.gridCellColumns` fine-tune it.
- A **`Picker`** is segments, `.radio` buttons or a `.menu`.
- **`SearchField`** is for what to look for; **`Link`** opens a page.
- **`.lineLimit(n)`** cuts text short; **`.selectable()`** lets it be copied.
- A **`TableColumn`** with **`onEdit:`** edits its cells in place.
- **`RichTextEditor`** edits a **`RichText`**: text and its styled runs,
  saved with `toJson`, or as Markdown with `.markdown`.
- **`.rotation`** and **`.scale`** turn and grow a view, and the views
  around it make room.
- **`HSplit`**/**`VSplit`** share a window between two views;
  **`Scroll(.horizontal)`**, **`LazyHStack`** and **`scrollTo`** handle long rows.

**Next:** [22. Sound, video, the web and the desktop](/learn/22-media-desktop/)
