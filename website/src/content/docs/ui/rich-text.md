---
title: Rich text
description: Let people write text in bold, italic, underline, strikethrough, sizes, colors and fonts with RichTextEditor, and keep it as a RichText value you can save, read and turn into Markdown.
---

A `RichTextEditor` is a field to write in with styles: bold, italic,
underline, strikethrough, sizes, colors and fonts. What's written is a
`RichText` value in your app's state:

```tessel
app Notes(width: 560, height: 360) {
    state note = RichText("Write here.")

    VStack(alignment: .leading) {
        RichTextEditor(text: note)
        Text("{note.text.count} characters").color(.secondary)
    }
    .padding(12)
}
```

Above the text is a bar of buttons: **B**, *I*, U and S for bold, italic,
underline and strikethrough; A− and A+ for a smaller or larger size; a
color, a font, and Plain to take the styles off. They change what's
selected; with nothing selected, they change what's typed next. A button
shows as pressed when all of what's selected has its style.

Cmd+B, Cmd+I and Cmd+U (Ctrl on Windows) do the same from the keyboard.
Undo and redo work as in a text field.

## Copying and pasting

What's copied keeps its styles:

- Pasted in another `RichTextEditor`, it's exactly as it was.
- Pasted in apps that take text with styles (Pages, Word, Mail, a web
  page's editor), it keeps its bold, italic, underline, strikethrough,
  sizes, colors and fonts. Elsewhere, and in a `TextField`, it's the text.
- Text with styles copied in other apps keeps them when it's pasted in the
  editor, as far as the editor has them. Black text takes the editor's
  color, so it shows in dark mode too.

Plain text pasted takes the style of where it goes, like typing.

- `toolbar: false` leaves the bar out, for an editor with only the
  keyboard's styles, or with buttons of your own around it.
- `onChange:` runs after each change, for saving as the user writes.
- The editor takes all the space it's offered, wraps its text at its
  width, and scrolls when the text is longer. `.font(size:)` and
  `.color(…)` on it set the size and color of text without styles of
  its own.

## RichText

A `RichText` is the text, and the stretches of it that have styles, its
*runs*:

```tessel
struct RichText {
    text: String
    runs: [TextRun] = []
}

struct TextRun {
    start: Int
    end: Int
    bold: Bool = false
    italic: Bool = false
    underline: Bool = false
    strikethrough: Bool = false
    size: Float = 0.0
    color: Color = .primary
    font: String = ""
}
```

A run is on the characters from `start` up to `end`, counted as
`String.count` counts them. A `size` of 0, the color `.primary` and the font
`""` mean the editor's own. `RichText("Hello")` is a text without styles.

You can make one with styles in code, and read its runs:

```tessel
fn main() {
    var doc = RichText("Total: 42 kg")
    doc.runs.append(TextRun(start: 7, end: 12, bold: true, color: .blue))
    for run in doc.runs {
        print(doc.text.substring(from: run.start, to: run.end))
    }
}
```

```text
42 kg
```

The editor keeps the runs in order, apart and without plain ones. Runs you
make yourself may overlap; where they do, the later one's style counts.

## Saving

A `RichText` is a struct like any other, so [`toJson`](/reference/stdlib/#tojson)
and `fromJson` save and load it, with every style:

```tessel
fn save(_ doc: RichText, path: String) -> Error? {
    writeText(path: path, text: toJson(doc))
}

fn load(_ path: String) -> RichText {
    let text = readText(path).value() ?? ""
    let doc: RichText? = fromJson(text)
    return doc ?? RichText("")
}
```

## Markdown

`markdown` is the text in [Markdown](https://commonmark.org): bold as
`**…**`, italic as `*…*` and strikethrough as `~~…~~`. Markdown has no
underline, sizes, colors or fonts, so those are left out.
`RichText.fromMarkdown(…)` reads those three back:

```tessel
fn main() {
    var doc = RichText("Hello brave world")
    doc.runs.append(TextRun(start: 0, end: 5, bold: true))
    doc.runs.append(TextRun(start: 6, end: 11, italic: true))
    print(doc.markdown)

    let back = RichText.fromMarkdown("**Hello** *brave* world")
    print(back.text)
    print(back.runs.count)
}
```

```text
**Hello** *brave* world
Hello brave world
2
```

A `*` that isn't a mark stays a star: `2 * 3` is read as it is, and `\*`
is always a star. The [`Markdown`](/ui/styling/#longer-text-with-markdown)
view shows it.

## In tests

In a [headless run](/ui/testing/), `tree` shows the editor's text and runs,
and what's selected in it:

```text
RichTextEditor "Hello world" runs ["0..5 bold"] selected 0..5
```

The bar's buttons are clicked by their labels, like `click "B"`, and the
keys work as typed: `key cmd+b`.
