---
title: Standard library
description: Every built-in Tessel function (math, random numbers, files, pictures, JSON and binary encoding, dates, the network, timers and background work) and every property and method of Int, Float, String, Data, lists and dictionaries.
---

This page lists everything built into Tessel that isn't a view or a
modifier: functions like `print` and `readFile`, conversions, math, random
numbers, files and folders, JSON and binary encoding, dates, the system,
networking, timers and background work, and the properties and methods of
`Int`, `Float`, `String`, `Data`, lists (`[T]`) and dictionaries (`[K: V]`).
The outputs shown in comments come from running the examples.

## Calling built-in functions

Signatures use the same notation as the [Views](/reference/views/#reading-the-signatures)
page: arguments are passed with their labels, in order, except those marked
`_`. For example, `writeFile(path: String, text: String) -> Bool` is called
as `writeFile(path: "notes.txt", text: "Hello")`.

When a function or method has **exactly one parameter**, you may leave out
its label: `readFile("notes.txt")` means `readFile(path: "notes.txt")`, and
`list.remove(0)` means `list.remove(at: 0)`.

A last parameter of function type can be written as a block after the call:

```tessel
after(seconds: 1) {
    print("a second later")
}
```

## Printing and conversions

### print

```text
print(_ value: Int, Float, Bool, String, Range or an enum, terminator: String = "\n")
```

Writes a value to standard output, followed by `terminator` (a new line,
unless you say otherwise).

```tessel
print("Hello")        // Hello
print(42)             // 42
print(2.0)            // 2.0
print(true)           // true
print(Mode.grid)      // grid
print(0..3)           // 0..3
print("Name: ", terminator: "")   // no line break: the answer goes on the same line
```

- A `Float` always shows a decimal point (`2.0`), so it reads differently
  from an `Int`.
- An enum value prints as its case name, and a range as `start..end`.
- Lists, dictionaries, sets and tuples print as they're written in code:
  `[1, 2]`, `["tea": 3]`, `(1, "one")`, with text inside them in quotes and
  `nil` for a missing value. Their items must be printable this way
  (numbers, text, `Bool`s, enums, and lists, dictionaries, sets and tuples
  of those).
- Structs and optionals can't be printed directly. Print a field instead,
  or give an optional a default with `??`.
- In an app, the output goes to the terminal the app was started from (or to
  the console of the [Tessel IDE](/start/ide/)).

### printError

```text
printError(_ value: …, terminator: String = "\n")
```

Like `print`, but writes to standard error. Use it for error messages and
warnings from a command-line program, so they don't mix with its real
output when that's piped into a file or another program:

```tessel
if let text = readFile(path) {
    print(text.uppercase())
} else {
    printError("can't read {path}")
    exit(1)
}
```

### readLine

```text
readLine() -> String?
```

Reads the next line from standard input: what the user types, up to Enter,
or the next line of input piped into the program. The line break isn't
included. At the end of the input, it returns `nil`.

```tessel
fn main() {
    print("What's your name? ", terminator: "")
    let name = readLine() ?? "stranger"
    print("Hello, {name}!")
}
```

Read every line until the input ends with `while let`:

```tessel
fn main() {
    var total = 0
    while let line = readLine() {
        total += Int(line.trim()) ?? 0
    }
    print("total: {total}")
}
```

```sh
printf '1\n2\n3\n' | tessel run sum.tsl      # total: 6
```

### readInput

```text
readInput() -> String
```

Reads all of standard input at once, until it ends; empty if there's none.
It suits programs that process piped text, like
`cat notes.txt | tessel run count.tsl`:

```tessel
fn main() {
    let text = readInput()
    print("{text.lines().count} lines, {text.words().count} words")
}
```

In an app (which has a window, not a terminal) there's usually no input:
`readLine()` returns `nil` and `readInput()` an empty string.

### Int()

```text
Int(_ value: Int or Float) -> Int
Int(_ value: String) -> Int?
```

Converts a number or text to an `Int`.

```tessel
print(Int(3.99))                // 3
print(Int(-3.99))               // -3
print(Int("42") ?? 0)           // 42
print(Int(" 42 ") ?? 0)         // 42
print(Int("4.2") ?? -1)         // -1
```

- From a `Float`, the fraction is dropped (rounding toward zero). Values too
  big for an `Int` become the largest (or smallest) `Int`.
- From a `String`, the result is optional: `nil` if the text isn't a whole
  number. Spaces around the number are ignored; a leading `+` or `-` is
  allowed.

### Float()

```text
Float(_ value: Int or Float) -> Float
Float(_ value: String) -> Float?
```

Converts a number or text to a `Float`.

```tessel
print(Float(3))                 // 3.0
print(Float("4.5") ?? 0.0)      // 4.5
print(Float("1e3") ?? 0.0)      // 1000.0
print(Float("abc") ?? 0.0)      // 0.0
```

From a `String`, the result is `nil` if the text isn't a number. Spaces around
it are ignored, and exponents like `1e3` are understood.

### String()

```text
String(_ value: Int, Float, Bool, String or an enum) -> String
```

Turns a value into text, exactly as [`print`](#print) would show it.

```tessel
let label = String(3.0)
print(label.count)              // 3
```

Usually string interpolation is simpler: `"{value}"` gives the same text.

## Math

```tessel
print(sqrt(16.0))               // 4.0
print(pow(2.0, 10.0))           // 1024.0
print(sin(pi / 2.0))            // 1.0
print(max(0, 2.5))              // 2.5
print(abs(-4))                  // 4
```

Math functions take their arguments **without labels**, in the order shown.

| Function | Returns |
|---|---|
| `sqrt(x)` | The square root of `x`. |
| `pow(x, power)` | `x` raised to `power`: `pow(2.0, 3.0)` is `8.0`. |
| `exp(x)` | *e* raised to `x`. |
| `log(x)` | The natural logarithm (base *e*) of `x`. |
| `log10(x)` | The base-10 logarithm of `x`. |
| `sin(x)`, `cos(x)`, `tan(x)` | The sine, cosine and tangent of the angle `x`. |
| `asin(x)`, `acos(x)`, `atan(x)` | The angle whose sine, cosine or tangent is `x`. |
| `atan2(y, x)` | The angle from the positive x axis to the point (`x`, `y`), from `-pi` to `pi`. Note that `y` comes first. |
| `floor(x)` | `x` rounded down: `floor(-2.7)` is `-3.0`. |
| `ceil(x)` | `x` rounded up: `ceil(2.1)` is `3.0`. |
| `round(x)` | `x` rounded to the nearest whole number; halves round away from zero, so `round(2.5)` is `3.0` and `round(-2.5)` is `-3.0`. |
| `abs(x)` | `x` without its sign. |
| `min(a, b)` | The smaller of `a` and `b`. |
| `max(a, b)` | The larger of `a` and `b`. |
| `pi` | The constant π, `3.141592653589793`. |

- All of these take and return a `Float`, except `abs`, `min` and `max`: they
  work on `Int`s or `Float`s, and return the same type. Both values of `min`
  and `max` must have the same type. A whole-number literal next to a `Float`
  is read as a `Float`, so `max(0, x)` works for a `Float` `x`. To mix an
  `Int` variable with a `Float`, convert it with [`Float()`](#float).
- Whole-number literals are read as `Float`s where a `Float` is expected, so
  `sqrt(16)` works too.
- Angles are in **radians**. To convert from degrees, multiply by
  `pi / 180.0`: `sin(30.0 * pi / 180.0)`.
- `floor`, `ceil` and `round` return a `Float`. Wrap them in
  [`Int()`](#int) to get an `Int`: `Int(round(2.6))` is `3`.
- `pi` is a value, not a function: write `pi`, not `pi()`.
- The results follow the usual floating-point rules and never stop the
  program: `sqrt(-1.0)` is `NaN` ("not a number") and `log(0.0)` is `-inf`.

## Tests

In a [test](/guides/testing/):

| Function | What it does |
|---|---|
| `expect(_ condition: Bool, _ message: String = "")` | Fails the test if `condition` is false. `tessel test` says where it is and what it says, and for a comparison (`==`, `<`, …) the values on each side. |
| `fail(_ message: String = "")` | Fails the test. |

The test goes on after either, and fails when it ends.

## Random numbers

### random

```text
random(_ range: Range) -> Int
```

A random `Int` from the range's start up to, but not including, its end.

```tessel
let die = random(1..7)          // 1, 2, 3, 4, 5 or 6
let index = random(0..names.count)
```

The range must have at least one number: an empty range (like `3..3`, or
`n..n` when `n` is the same) stops the program with a runtime error.

### randomFloat

```text
randomFloat() -> Float
```

A random `Float` from `0.0` up to, but not including, `1.0`.

```tessel
if randomFloat() < 0.1 {
    print("a one-in-ten chance")
}
let angle = randomFloat() * 2.0 * pi
```

To pick or mix up list items, use [`randomElement`](#randomelement-and-shuffled)
and [`shuffled`](#randomelement-and-shuffled).

:::caution
The numbers are good enough for games, simulations and picking things at
random, but they're not safe for secrets such as passwords or keys.
:::

## Files

Paths are ordinary strings. A relative path like `"notes.txt"` is relative to
the program's current folder (see [`currentFolder`](#currentfolder)). Use `/`
between folder names.

### readFile

```text
readFile(path: String) -> String?
```

The whole file as text, or `nil` if it doesn't exist, can't be read, or isn't
valid UTF-8 text.

```tessel
if let text = readFile("notes.txt") {
    print(text.lines().count)
} else {
    print("no notes yet")
}
```

### writeFile

```text
writeFile(path: String, text: String) -> Bool
```

Writes `text` to a file, replacing the file if it exists. Returns whether it
worked. The folder must already exist (see [`createFolder`](#createfolder)).

```tessel
let ok = writeFile(path: "notes.txt", text: "Buy milk\n")
print(ok)                       // true
```

### listFolder

```text
listFolder(path: String) -> [String]?
```

The names of the files and folders in a folder, sorted, or `nil` if the folder
can't be read.

```tessel
for name in listFolder(".") ?? [] {
    print(name)
}
```

- The result has names only, not full paths: join them yourself, like
  `"{folder}/{name}"`.
- Hidden files (names starting with `.`) are included.
- The names are sorted by character code, so `"B"` comes before `"a"`.

### fileExists

```text
fileExists(path: String) -> Bool
```

Whether a file **or folder** exists at `path`.

```tessel
print(fileExists("notes.txt"))
```

### isFolder

```text
isFolder(path: String) -> Bool
```

Whether `path` is an existing folder.

```tessel
print(isFolder("."))            // true
```

### deleteFile

```text
deleteFile(path: String) -> Bool
```

Deletes a file. Returns whether it worked. It doesn't delete folders (it
returns `false` for a folder).

```tessel
if deleteFile("notes.txt") {
    print("deleted")
}
```

### createFolder

```text
createFolder(path: String) -> Bool
```

Creates a folder, and any missing folders above it. Returns whether it worked;
a folder that already exists counts as success.

```tessel
print(createFolder("backups/2026"))   // true
```

### renameFile

```text
renameFile(from: String, to: String) -> Bool
```

Renames or moves a file or folder. Returns whether it worked. An existing
file at `to` may be replaced.

```tessel
let moved = renameFile(from: "notes.txt", to: "backups/notes.txt")
```

### currentFolder

```text
currentFolder() -> String
```

The full path of the program's current folder: the folder it was started
from. Relative paths are relative to this folder.

```tessel
print(currentFolder())          // for example /Users/ada/Projects/notes
```

### appendFile

```text
appendFile(path: String, text: String) -> Bool
```

Adds `text` to the end of a file, creating the file if it doesn't exist.
Returns whether it worked. Handy for logs.

```tessel
writeFile(path: "log.txt", text: "started\n")
appendFile(path: "log.txt", text: "saved\n")
print((readFile("log.txt") ?? "").lines().count)   // 2
```

### copyFile

```text
copyFile(from: String, to: String) -> Bool
```

Copies a file. Returns whether it worked. An existing file at `to` is
replaced. It doesn't copy folders.

```tessel
print(copyFile(from: "log.txt", to: "log-backup.txt"))   // true
```

### fileSize

```text
fileSize(_ path: String) -> Int?
```

The size of a file in bytes, or `nil` if it doesn't exist.

```tessel
print(fileSize("log.txt") ?? 0)         // 14
print(fileSize("missing.txt") ?? -1)    // -1
```

### modifiedTime

```text
modifiedTime(_ path: String) -> Float?
```

When the file last changed, as a [time](#dates-and-time) (seconds since
1970), or `nil` if it doesn't exist.

```tessel
if let time = modifiedTime("log.txt") {
    print("Last changed {formatDate(time)}")
}
```

### readData

```text
readData(_ path: String) -> Data?
```

The whole file as raw bytes ([`Data`](#data)), or `nil` if it can't be read.
Use it for files that aren't text, such as images or files written with
[`toBinary`](#tobinary).

```tessel
if let data = readData("header.bin") {
    print(data.count)
}
```

### writeData

```text
writeData(path: String, data: Data) -> Bool
```

Writes bytes to a file, replacing the file if it exists. Returns whether it
worked.

```tessel
let bytes = Data(bytes: [137, 80, 78, 71])
print(writeData(path: "header.bin", data: bytes))   // true
```

### appendData

```text
appendData(path: String, data: Data) -> Bool
```

Adds bytes to the end of a file, creating the file if it doesn't exist.
Returns whether it worked. It lets a large file be written a piece at a
time, without holding all of it in memory.

```tessel
print(writeData(path: "numbers.bin", data: Data(bytes: [1, 2, 3])))    // true
print(appendData(path: "numbers.bin", data: Data(bytes: [4, 5])))      // true
print(readData("numbers.bin")?.bytes ?? [])                            // [1, 2, 3, 4, 5]
```

## Folders and paths

These functions give you the usual places to keep files, and build paths
that work on every system. To take a path apart, use the `String` properties
[`fileName`, `fileExtension` and `folder`](#paths).

| Function | Returns |
|---|---|
| `homeFolder()` | The user's home folder, like `/Users/ada` (or `C:\Users\ada` on Windows). |
| `documentsFolder()` | The user's Documents folder. |
| `tempFolder()` | A folder for temporary files. |
| `appDataFolder(_ name: String)` | A folder for your app's own files (settings, caches), created if needed. |
| `joinPath(_ folder: String, _ name: String)` | `folder` and `name` joined with the system's separator. |

### appDataFolder

```text
appDataFolder(_ name: String) -> String
```

The folder where an app keeps its own files, like settings, with `name`
added. It's created if it doesn't exist yet.

- On macOS: `~/Library/Application Support/<name>`.
- On Windows: `%APPDATA%\<name>`.

```tessel
let settings = joinPath(appDataFolder("Notes"), "settings.json")
```

Use your app's name for `name`, so apps don't mix up their files.

### joinPath

```text
joinPath(_ folder: String, _ name: String) -> String
```

Joins a folder and a name with `/` (`\` on Windows). Extra separators where
they meet are removed. If `folder` is `""`, the result is just `name`.

```tessel
print(joinPath("/Users/ada/", "notes.txt"))     // /Users/ada/notes.txt
let notes = joinPath(documentsFolder(), "Notes")
```

## File dialogs

`openFile` and `saveFile` show the system's standard dialog for choosing a
file, and return the path the user picked. They're meant for
[apps](/ui/apps-views/): call them from a button's action, a
[shortcut](/ui/shortcuts/) or another block that runs when the user does
something. See [Documents and custom file types](/ui/async/#documents-and-custom-file-types)
for a complete example.

### openFile

```text
openFile(title: String = "", types: [String] = []) -> String?
```

Asks the user to pick an existing file. Returns its full path, or `nil` if
the user cancelled.

```tessel
Button("Open…") {
    if let path = openFile(title: "Open a note", types: ["note"]) {
        text = readFile(path) ?? ""
    }
}
```

- `title` is shown in the dialog's title bar (not on every system).
- `types` limits the choice to files with these extensions. Write them
  without the dot: `["note"]`, or `["txt", "md"]` for several. An empty list
  allows any file.

### saveFile

```text
saveFile(title: String = "", name: String = "", types: [String] = []) -> String?
```

Asks the user where to save a file. Returns the chosen path, or `nil` if the
user cancelled. It doesn't write anything: pass the path to
[`writeFile`](#writefile).

```tessel
Button("Save…") {
    if let path = saveFile(title: "Save the note", name: "Untitled.note", types: ["note"]) {
        writeFile(path: path, text: text)
    }
}
```

- `name` is the file name the dialog suggests.
- `types` works as for `openFile`. If the user types a name without an
  extension, the first extension in `types` is added, so the example above
  always gives a path ending in `.note`.
- The system dialog asks the user before replacing an existing file.

:::note
In a [headless test run](/ui/testing/) no dialog opens. The script's
[`answer` command](/ui/testing/#file-dialogs) decides what the next dialog
returns; without one, it returns `nil`, as if the user cancelled.
:::

### openFiles

```text
openFiles(title: String = "", types: [String] = []) -> [String]
```

Like [`openFile`](#openfile), for choosing several files at once. Returns
their paths: an empty list if the user cancels.

```tessel
for path in openFiles(types: ["png", "jpg"]) {
    pictures.append(path)
}
```

### chooseFolder

```text
chooseFolder(title: String = "") -> String?
```

Shows the system's dialog for choosing a folder (in which one can be made,
too). Returns its path, or `nil` if the user cancels.

```tessel
if let folder = chooseFolder(title: "Export to") {
    writeText(path: joinPath(folder, "report.txt"), text: report)
}
```

## Message dialogs

`alert` and `confirm` show a message or a question in the system's dialog,
and wait for the user. Like the file dialogs, they're meant for apps. See
[Dialogs and sheets](/ui/dialogs/) for these and for dialogs with your own
views.

### alert

```text
alert(_ title: String, message: String = "", kind: AlertKind = .info, buttonLabel: String = "OK")
```

Shows a message with one button, and waits until it's dismissed.

```tessel
alert("Couldn't save", message: error.message, kind: .error)
```

`kind` is `.info`, `.warning` or `.error`: the dialog's icon.

### confirm

```text
confirm(_ title: String, message: String = "", confirmLabel: String = "OK",
        cancelLabel: String = "Cancel", kind: AlertKind = .info) -> Bool
```

Asks a question with two buttons. Returns `true` for the confirm button, and
`false` for Cancel (or Escape).

```tessel
if confirm("Delete 3 items?", message: "This can't be undone.", confirmLabel: "Delete") {
    deleteSelected()
}
```

:::note
In a [headless test run](/ui/testing/) these are printed instead, and the
script's `answer` command decides how a `confirm` ends. See
[Dialogs in test scripts](/ui/dialogs/#in-test-scripts).
:::

## Pictures

[`Image`](/reference/views/#image) shows a picture file, and a
[`Canvas`](/reference/views/#canvas) draws one with `g.image`. These
functions make picture files, and tell their size.

### renderImage

```text
renderImage(path: String, width: Float, height: Float, scale: Float = 1,
            draw: fn(Graphics)) -> Bool
```

Runs a drawing block, as a [`Canvas`](/reference/views/#canvas) of `width`
× `height` points would, and saves what it draws as a picture. Returns
whether it worked. It needs no window, so it works in a program with only
`fn main()` too.

```tessel
fn main() {
    let ok = renderImage(path: "badge.png", width: 200, height: 80, scale: 2) { g in
        g.fillRect(x: 0, y: 0, width: g.width, height: g.height, color: .blue, radius: 16)
        g.text("Tessel", x: g.width / 2, y: g.height / 2, size: 28, color: .white, weight: .bold, align: .center)
    }
    print(ok)       // true
}
```

- The file is a PNG, or a JPEG if `path` ends in `.jpg` or `.jpeg`.
- The picture is `width × scale` by `height × scale` pixels: `scale: 2`
  gives the sharpness of a Retina screen, and a larger one a picture to
  print. Neither side can be more than 16 384 pixels.
- Where nothing is drawn, a PNG is see-through and a JPEG white.
- It's drawn the way a window is.

### writePDF

```text
writePDF(path: String, title: String = "", width: Float = 595.28,
         height: Float = 841.89, draw: fn(Graphics)) -> Bool
```

Runs a drawing block, as [`renderImage`](#renderimage) does, and saves what
it draws as a PDF document of pages `width` × `height` points (A4 unless
they're given; a point is 1/72 inch). In the block, `g.newPage()` ends a
page and starts the next. Returns whether it worked.

```tessel
fn main() {
    let ok = writePDF(path: "hello.pdf", title: "Hello") { g in
        g.text("Hello", x: 72, y: 72, size: 36, weight: .bold)
        g.newPage()
        g.text("A second page", x: 72, y: 72)
    }
    print(ok)       // true
}
```

- Text stays text, with its fonts in the file; shapes stay shapes; pictures
  keep their pixels. `title` is the document's title, which PDF readers
  show.
- `g.width` and `g.height` are the page's size; every page has the same.
- A new page starts with nothing transformed, clipped or in a layer.
- It's drawn in the light appearance, even in an app that's dark: on paper,
  `.primary` is black.
- See [Making a PDF](/ui/drawing/#making-a-pdf).

### printDocument

```text
printDocument(title: String = "", width: Float = 595.28, height: Float = 841.89,
              draw: fn(Graphics)) -> Bool
```

Draws pages as [`writePDF`](#writepdf) does, with `g.newPage()` between
them, and prints them through the system. Returns whether the document was
printed (or handed over to be).

```tessel
printDocument(title: "Invoice 42") { g in
    g.text("Invoice 42", x: 50, y: 50, size: 24, weight: .bold)
}
```

- On macOS, the system's print dialog opens, and `false` means it was
  cancelled.
- On Windows, the app that opens PDF files is asked to print the document;
  if it can't be, the document opens in it.
- See [Printing](/ui/dialogs/#printing).

### writeImage

```text
writeImage(path: String, width: Int, height: Int, pixels: [Int]) -> Bool
```

Saves a picture from its pixels, one number each, row by row from the top
left. A pixel's number is

```text
red × 16777216 + green × 65536 + blue × 256 + opacity
```

with each part from 0 to 255 (255: fully opaque). Returns whether it
worked; `pixels` must have at least `width × height` numbers.

```tessel
fn pixel(_ red: Int, _ green: Int, _ blue: Int) -> Int {
    red * 16777216 + green * 65536 + blue * 256 + 255
}

fn main() {
    var pixels: [Int] = []
    for y in 0..64 {
        for x in 0..64 {
            pixels.append(pixel(x * 4, y * 4, 128))
        }
    }
    print(writeImage(path: "gradient.png", width: 64, height: 64, pixels: pixels))   // true
}
```

Like `renderImage`, it writes a PNG, or a JPEG for a path ending in `.jpg`
or `.jpeg`.

### imageSize

```text
imageSize(_ path: String) -> (Int, Int)?
```

The width and height of a PNG or JPEG file in pixels, or `nil` if it can't
be read as one.

```tessel
if let size = imageSize("gradient.png") {
    print("{size.0} × {size.1}")        // 64 × 64
}
```

### readImage

```text
readImage(_ path: String) -> (Int, Int, [Int])?
```

The width, height and pixels of a PNG or JPEG file, or `nil` if it can't be
read as one. The pixels come row by row from the top left corner, each as
one number, the way `writeImage` takes them: red × 16777216 + green × 65536
+ blue × 256 + opacity (each from 0 to 255).

```tessel
if let picture = readImage("gradient.png") {
    let (width, height, pixels) = picture
    let last = pixels[pixels.count - 1]
    print("{width} × {height}, {pixels.count} pixels")             // 64 × 64, 4096 pixels
    print("red {last / 16777216}, opacity {last % 256}")           // red 252, opacity 255
}
```

## JSON and binary

These four functions turn your values into text or bytes, to save in a file
or send over the network, and read them back. They work with structs, enums,
lists, dictionaries, optionals, ranges, `Data` and the basic types, nested
as deeply as you like. See [Saving data as JSON](/guides/json/) for a
complete example.

```tessel
enum Priority { low, high }

struct Task {
    title: String
    done: Bool = false
    priority: Priority = Priority.low
}

fn main() {
    let task = Task(title: "Write docs", priority: Priority.high)
    let text = toJson(task)
    print(text)     // {"title":"Write docs","done":false,"priority":"high"}

    let back: Task? = fromJson(text)
    print(back == task)     // true
}
```

### toJson

```text
toJson(_ value, pretty: Bool = false) -> String
```

The value as [JSON](https://www.json.org) text. With `pretty: true`, the
text is spread over several lines and indented, which is easier to read:

```tessel
print(toJson(task, pretty: true))
```

```text
{
  "title": "Write docs",
  "done": false,
  "priority": "high"
}
```

### fromJson

```text
fromJson(_ text: String) -> T?
```

Reads a value from JSON text. It gives `nil` if the text isn't valid JSON or
doesn't fit the type.

`fromJson` finds out which type to read from where its result goes, so give
the result a type:

```tessel
let loaded: Task? = fromJson(text)
let ages: [String: Int] = fromJson(text) ?? [:]
if let numbers: [Int] = fromJson("[4, 8, 15]") {
    print(numbers.sum())        // 27
}
```

Without a type, `tessel check` asks for one:

```text
error: can't tell what type `fromJson` should read
 --> main.tsl:2:16
  |
2 |     let data = fromJson(text)
  |                ^^^^^^^^^^^^^^
  |
  = help: give the result a type, like `let project: Project? = fromJson(…)`
```

### toBinary

```text
toBinary(_ value) -> Data
```

The value as compact bytes ([MessagePack](https://msgpack.org)), smaller
and faster to read than JSON, but not readable by people. Save it with
[`writeData`](#writedata).

```tessel
let data = toBinary(task)
print(data.count)               // 38
```

### fromBinary

```text
fromBinary(_ data: Data) -> T?
```

Reads a value back from bytes made by `toBinary`, or `nil` if they don't fit
the type. Like `fromJson`, it needs to know the type:

```tessel
if let data = readData("task.bin") {
    let task: Task? = fromBinary(data)
}
```

### How values are written

| Value | JSON | Example |
|---|---|---|
| `Int`, `Float`, `Bool`, `String` | The same value | `42`, `1.5`, `true`, `"hi"` |
| A struct | An object with a key for each field | `{"x":1.0,"y":2.0}` |
| An enum case without values | Its name, as a string | `"high"` |
| An enum case with values | An object with the case's name as key | `{"custom":{"name":"feet","factor":0.3048}}` |
| A list | An array | `[1,2,3]` |
| A dictionary | An object; `Int` and `Bool` keys are written as strings | `{"ada":36}` |
| An optional | The value, or `null` for `nil` | `null` |
| A range | An array with its start and end | `[0,5]` |
| `Data` | A base64 string | `"aGk="` |

`toBinary` writes the same structure in MessagePack, which can hold a few
things JSON can't, so it keeps them as they are: `Data` as raw bytes (not
base64), `Int` and `Bool` dictionary keys as numbers and booleans, and `NaN`
and infinity as floats. That also makes binary files smaller.

When reading:

- Fields that are missing take their default value (or `nil` for an
  optional field); a missing field without a default makes the result
  `nil`. This lets you add fields with defaults to a struct and still read
  files saved before.
- Keys that the struct doesn't have are ignored.
- A value of the wrong kind (like a number where a string belongs), an
  unknown enum case, or text that isn't valid JSON makes the whole result
  `nil`.
- An `Int` also accepts a whole number written with a decimal point, like
  `2.0`.
- `Float` values that aren't numbers (`NaN`, infinity) have no JSON form;
  they're written as `null`. Reading `null` into a `Float` gives `NaN`, and
  into a `Float?` gives `nil`. (Binary keeps them exactly.)
- A whole document that is just `null` reads as `nil`, whatever the type:
  `let x: Float? = fromJson("null")` is `nil`.

Functions and views can't be saved. `tessel check` points out the field that
is the problem:

```tessel
struct Counter {
    count: Int
    onChange: fn(Int)
}
```

```text
error: `Counter` can't be saved with `toJson`
 --> main.tsl:8:23
  |
8 |     let text = toJson(c)
  |                       ^
  |
  = help: field `onChange`: functions can't be saved
```

Dictionary keys must be `String`, `Int`, `Bool` or an enum.

## Databases

SQLite, built into Tessel. See
[Saving data in a database](/guides/database/).

### openDatabase

```text
openDatabase(_ path: String) -> Result<Database>
```

Opens a SQLite database file, or creates it. `":memory:"` is a database
that lasts only while the program runs. The reason for a failure is
SQLite's.

### Database

A database opened with `openDatabase`:

| Method | What it does |
|---|---|
| `execute(_ sql: String, _ values: [SqlValue] = []) -> Result<Int>` | Runs SQL that changes the database, with `values` for its `?`s; returns how many rows the last statement changed. SQL without values can be several statements, separated by `;`. |
| `query(_ sql: String, _ values: [SqlValue] = []) -> Result<[DatabaseRow]>` | Runs a `SELECT`, with `values` for its `?`s, and returns its rows. |
| `lastInsertId() -> Int` | The `rowid` of the row the last `INSERT` added. |
| `close()` | Closes the database (it's closed when the program ends, too). |

### DatabaseRow

A row of a query: `columns` (the names, as the query gives them) and
`values`. Its methods read a column by name, converting it the way SQLite
does; a name the row doesn't have stops the program.

| Method | |
|---|---|
| `int(_ column: String) -> Int` | A number (a text of one, or 0). |
| `float(_ column: String) -> Float` | A number (a text of one, or 0). |
| `text(_ column: String) -> String` | Text (empty for null). |
| `bool(_ column: String) -> Bool` | `true` for a number that isn't 0. |
| `data(_ column: String) -> Data` | Bytes (a blob, or text's bytes). |
| `isNull(_ column: String) -> Bool` | Whether it's null. |
| `value(_ column: String) -> SqlValue` | The value as it is. |

### SqlValue

A value in a database: `.null`, `.int(value: Int)`, `.float(value:
Float)`, `.text(value: String)` or `.data(value: Data)`. Where one is
wanted, as in `execute`'s and `query`'s values, an `Int`, `Float`,
`String`, `Bool` (1 or 0) or `Data`, an optional one, or `nil` (null) is
given as it is:

```tessel
try db.execute("INSERT INTO t (a, b, c) VALUES (?, ?, ?)", [42, "text", nil])
```

## Data

`Data` holds raw bytes: the contents of a file that isn't text, the result of
[`toBinary`](#tobinary), or a download.

```tessel
let empty = Data()
let bytes = Data(bytes: [72, 105, 33])
let text = Data(text: "héllo")
print(bytes.text ?? "not text")         // Hi!
print(text.count)                       // 6
print(text[1])                          // 195
print(bytes == Data(text: "Hi!"))       // true
```

| Creating | |
|---|---|
| `Data()` | No bytes. |
| `Data(bytes: [Int])` | The given bytes. Each `Int` should be from 0 to 255; other values are wrapped into that range. |
| `Data(text: String)` | The text's bytes in UTF-8. |

| Member | Returns | |
|---|---|---|
| `count` | `Int` | Number of bytes. |
| `isEmpty` | `Bool` | Whether there are no bytes. |
| `bytes` | `[Int]` | The bytes, each from 0 to 255. |
| `text` | `String?` | The bytes read as UTF-8 text, or `nil` if they aren't valid UTF-8. |
| `base64` | `String` | The bytes as [base64](#base64) text. |
| `hex` | `String` | The bytes as hexadecimal digits, two to a byte. See [hash and hex](#hash-and-hex). |
| `latin1` | `String` | Each byte as the character with that number, up to the first zero byte: the text of old files that aren't UTF-8. |
| `slice(from: Int, to: Int)` | `Data` | The bytes from index `from` up to `to`. |
| `int8(at:)`, `int16(at:)`, `uint16(at:)`, `int32(at:)`, `uint32(at:)`, `int64(at:)` | `Int` | The whole number stored in the bytes starting at an index. |
| `float32(at:)`, `float64(at:)` | `Float` | The decimal number stored there. |
| `compressed(_ format: String = "zlib")` | `Data` | The bytes, made smaller. |
| `decompressed(_ format: String = "zlib")` | `Data?` | Compressed bytes, as they were. |
| `hash(_ algorithm: String = "sha256")` | `Data` | The bytes' digest: SHA-256, SHA-1, SHA-512, MD5 and others. See [hash and hex](#hash-and-hex). |
| `append(_ data: Data)` | | Adds the bytes of `data` to the end. Changes the value, so it must be a `var`. |
| `append(byte: Int)` | | Adds one byte (from 0 to 255; other values are wrapped into that range, as in `Data(bytes:)`). |

- `data[i]` is the byte at index `i` (from 0), as an `Int`. An index outside
  the data stops the program with an error. Bytes can't be changed one at a
  time; add to the end, or make a new `Data` (for example from `bytes`).
- `a + b` is a new `Data` with the bytes of both.
- `==` and `!=` compare the bytes.
- Read and write files of bytes with [`readData`](#readdata) and
  [`writeData`](#writedata).

### Building bytes

To build a file's bytes, start with `Data()` and add to the end with `+=`,
`append(_:)` or `append(byte:)`. These change the `Data` in place, with room
to grow, so adding many small pieces takes time in proportion to the bytes
added, as `+=` on a list does. (A list of `Int`s would take 8 bytes of memory
for each byte.)

```tessel
/// A whole number as 4 bytes, the lowest first (little-endian).
fn uint32(_ n: Int) -> Data {
    var out = Data()
    for i in 0..4 {
        out.append(byte: n >> (8 * i) & 0xff)
    }
    return out
}

fn main() {
    var file = Data(text: "TSL")
    file.append(byte: 1)
    file += uint32(258)
    print(file.bytes)              // [84, 83, 76, 1, 2, 1, 0, 0]
    let both = Data(bytes: [1]) + Data(bytes: [2, 3])
    print(both.bytes)              // [1, 2, 3]
}
```

### Numbers in bytes

Files that aren't text (pictures, sound, maps, most formats that come from
other programs) store numbers as bytes: a whole number in 1, 2, 4 or 8 of
them, a decimal number in 4 or 8. These methods read one, starting at the
byte `at`:

```text
data.int8(at: Int) -> Int
data.int16(at: Int, bigEndian: Bool = false) -> Int        (also uint16, int32, uint32, int64)
data.float32(at: Int, bigEndian: Bool = false) -> Float    (also float64)
```

```tessel
let d = Data(bytes: [2, 1, 255, 255, 0, 0, 192, 63])
print(d.uint16(at: 0))                      // 258   (2 + 1 × 256)
print(d.uint16(at: 0, bigEndian: true))     // 513   (2 × 256 + 1)
print(d.int16(at: 2))                       // -1
print(d.uint16(at: 2))                      // 65535
print(d.float32(at: 4))                     // 1.5
```

- `int…` reads a number that can be negative, `uint…` one that can't: the
  bytes 255, 255 are `-1` as an `int16` and `65535` as a `uint16`.
- Most files put a number's smallest byte first ("little-endian"), which is
  what you get. With `bigEndian: true`, the largest byte comes first; a
  format's description says which it uses.
- Bytes that aren't there (an index too close to the end) stop the program
  with an error, like `data[i]`.

To go the other way, `Int` and `Float` have a [`bytes`](#bytes) method, and
`Data(bytes:)` makes the data:

```tessel
var out: [Int] = []
out += (258).bytes(count: 2)                // [2, 1]
out += (1.5).bytes(count: 4)                // [0, 0, 192, 63]
let data = Data(bytes: out)
```

### slice and latin1

```tessel
let d = Data(text: "name: Zoë")
print(d.slice(from: 0, to: 4).text ?? "")   // name
print(d.slice(from: 6, to: 99).count)       // 4   (ë takes two bytes)
print(Data(bytes: [90, 111, 235]).latin1)   // Zoë
print(Data(bytes: [90, 111, 235]).text ?? "not UTF-8")   // not UTF-8
```

`slice` brings its indexes into range, like a list's
[`slice`](#slice): it never stops the program. `latin1` reads text from
before UTF-8, where every byte is one character (it stops at a zero byte,
which such files use to fill fixed-width fields).

### compressed and decompressed

```text
data.compressed(_ format: String = "zlib") -> Data
data.decompressed(_ format: String = "zlib") -> Data?
```

Makes bytes smaller, and gets them back. The format is `"zlib"`,
`"deflate"` (the same without a header and a check) or `"gzip"` (what
`.gz` files are).

```tessel
let text = Data(text: "to be or not to be, that is the question. ".repeated(4))
let small = text.compressed()
print("{text.count} bytes as {small.count}")    // 168 bytes as 54
print(small.decompressed() == text)             // true
print(text.decompressed() == nil)               // true: it isn't compressed
```

`decompressed` gives `nil` if the bytes aren't in that format. An unknown
format name gives no bytes from `compressed`, and `nil` from `decompressed`.

### base64

Base64 turns bytes into plain text, for places that only take text (like a
JSON field or a URL).

```text
data.base64 -> String
decodeBase64(_ text: String) -> Data?
```

```tessel
print(Data(text: "Hi!").base64)         // SGkh
if let decoded = decodeBase64("SGkh") {
    print(decoded.text ?? "")           // Hi!
}
```

`decodeBase64` returns `nil` if the text isn't base64. Spaces and line
breaks around it are ignored.

### hash and hex

```text
data.hash(_ algorithm: String = "sha256") -> Data
data.hex -> String
```

`hash` gives the bytes' digest: a short, fixed-size fingerprint that changes
completely when any byte does, for checking a download or storing a
password's hash. `hex` writes bytes as hexadecimal digits, the usual way to
show a digest.

```tessel
let digest = Data(text: "abc").hash()
print(digest.count)     // 32
print(digest.hex)       // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
print(Data(text: "abc").hash("md5").hex)    // 900150983cd24fb0d6963f7d28e17f72
```

| Algorithm | Bytes | |
|---|---|---|
| `"sha256"` (the default) | 32 | SHA-256. |
| `"sha224"`, `"sha384"`, `"sha512"` | 28, 48, 64 | The other SHA-2 digests. |
| `"sha1"` | 20 | SHA-1: for file formats and protocols that ask for it. |
| `"md5"` | 16 | MD5: for old file formats only. |

SHA-1 and MD5 are broken for security; use them only where a format
requires them. Another algorithm stops the program. Case and dashes don't
matter: `"SHA-256"` is `"sha256"`.

## Dates and time

A point in time is a `Float`: the number of seconds since 1 January 1970
(UTC). That makes times easy to store and compare, and to do arithmetic with:
add `60.0` for a minute later, or `86400.0` for a day.

```tessel
let start = now()
let deadline = date(year: 2026, month: 12, day: 31, hour: 17)
print(formatDate(deadline, format: "EEEE, d MMMM yyyy"))   // Thursday, 31 December 2026
let days = Int((deadline - start) / 86400.0)
```

Dates are shown and read in the computer's time zone, unless you name
another one (see [Time zones](#time-zones)).

### now

```text
now() -> Float
```

The current time. Use it to time something, too:

```tessel
let started = now()
doWork()
print("took {(now() - started).formatted(decimals: 2)} seconds")
```

### date

```text
date(year: Int, month: Int, day: Int,
     hour: Int = 0, minute: Int = 0, second: Int = 0, timeZone: String = "") -> Float
```

The time of a calendar date, in the computer's time zone or the one named.
Months and days count from 1. A date that doesn't exist, like 30 February,
gives `NaN` (see [`isNaN`](#float)).

```tessel
let t = date(year: 2026, month: 3, day: 7, hour: 9, minute: 5)
print(formatDate(t))            // 2026-03-07 09:05
let launch = date(year: 2026, month: 3, day: 7, hour: 9, timeZone: "Asia/Tokyo")
```

A time the clocks skip (when they're set forward for summer time) gives the
moment an hour later; a time they show twice (when they're set back) gives
the first of the two.

### formatDate

```text
formatDate(_ time: Float, format: String = "yyyy-MM-dd HH:mm", timeZone: String = "") -> String
```

A time as text, in the computer's time zone or the one named. `format` is a pattern where these letters are replaced;
anything else is copied as it is:

| Pattern | Means | Example |
|---|---|---|
| `yyyy` | Year | `2026` |
| `yy` | Year, two digits | `26` |
| `MMMM` | Month name | `March` |
| `MMM` | Short month name | `Mar` |
| `MM` | Month, two digits | `03` |
| `M` | Month | `3` |
| `dd` | Day of the month, two digits | `07` |
| `d` | Day of the month | `7` |
| `EEEE` | Weekday | `Saturday` |
| `EEE` | Short weekday | `Sat` |
| `HH` | Hour (0–23), two digits | `09` |
| `H` | Hour (0–23) | `9` |
| `hh` | Hour (1–12), two digits | `09` |
| `h` | Hour (1–12) | `9` |
| `mm` | Minute, two digits | `05` |
| `m` | Minute | `5` |
| `ss` | Second, two digits | `30` |
| `s` | Second | `30` |
| `a` | AM or PM | `AM` |
| `SSS` | Thousandths of a second | `250` |
| `Z` | How far the zone is from UTC | `+0100` |
| `ZZZZZ` | The same, with a colon | `+01:00` |
| `'text'` | The text as it is | `at` |
| `''` | A single quote | `'` |

```tessel
let t = date(year: 2026, month: 3, day: 7, hour: 9, minute: 5, second: 30)
print(formatDate(t, format: "yyyy-MM-dd HH:mm:ss"))   // 2026-03-07 09:05:30
print(formatDate(t, format: "d MMM yy"))              // 7 Mar 26
print(formatDate(t, format: "EEE, MMMM d"))           // Sat, March 7
print(formatDate(t, format: "h:mm a"))                // 9:05 AM
print(formatDate(t, format: "dd/MM/yyyy"))            // 07/03/2026
print(formatDate(t, format: "d MMM 'at' HH:mm"))      // 7 Mar at 09:05
```

Month and weekday names are in English. Put words in single quotes, like
`'at'`, so their letters aren't replaced.

The format `"iso"` gives the form used on the internet and in JSON
(ISO 8601): the date, a `T`, the time, and how far the zone is from UTC.

```tessel
print(formatDate(t, format: "iso", timeZone: "Europe/Paris"))   // 2026-03-07T09:05:30+01:00
print(formatDate(t, format: "iso", timeZone: "UTC"))            // 2026-03-07T08:05:30Z
```

### parseDate

```text
parseDate(_ text: String, format: String = "yyyy-MM-dd", timeZone: String = "") -> Float?
```

Reads a time from text written in `format` (the same patterns as
[`formatDate`](#formatdate)), or `nil` if the text doesn't match. Without an
hour, the time is midnight. The text is taken to be in the computer's time
zone, or the one named, unless it says itself how far from UTC it is (with
`Z` or `ZZZZZ` in the format).

```tessel
let day = parseDate("2026-03-07")
let meeting = parseDate("07/03/2026 09:05", format: "dd/MM/yyyy HH:mm")
print(parseDate("tomorrow") == nil)     // true
```

The whole text must match: `parseDate("2026-03-07 09:05")` is `nil`, because
the default format has no time.

The format `"iso"` reads the internet's form, as servers and JSON usually
send it: with a distance from UTC (`+01:00`, or `Z` for none), with or
without fractions of a second. Without a distance, or with only a date, the
`timeZone:` counts.

```tessel
let sent = parseDate("2026-03-07T09:05:30+01:00", format: "iso")
let utc = parseDate("2026-03-07T08:05:30.250Z", format: "iso")
let noon = parseDate("2026-03-07 12:00", format: "yyyy-MM-dd HH:mm", timeZone: "Asia/Tokyo")
```

### dateParts

```text
dateParts(_ time: Float, timeZone: String = "") -> DateParts
```

What the calendar and the clock say at a moment: a `DateParts` struct.

| Field | Type | |
|---|---|---|
| `year` | `Int` | |
| `month` | `Int` | 1 for January to 12. |
| `day` | `Int` | The day of the month, from 1. |
| `hour` | `Int` | 0 to 23. |
| `minute` | `Int` | |
| `second` | `Int` | |
| `weekday` | `Int` | 1 for Monday to 7 for Sunday. |
| `dayOfYear` | `Int` | 1 for 1 January. |

```tessel
let today = dateParts(now())
if today.weekday >= 6 {
    print("It's the weekend")
}
print("Day {today.dayOfYear} of {today.year}")
```

### addToDate

```text
addToDate(_ time: Float, years: Int = 0, months: Int = 0, days: Int = 0,
          hours: Int = 0, minutes: Int = 0, seconds: Int = 0, timeZone: String = "") -> Float
```

A time that's some years, months or days later on the calendar (or earlier,
with negative numbers), and then some hours, minutes and seconds later.

```tessel
let due = addToDate(now(), days: 14)
let renewal = addToDate(start, years: 1)
let reminder = addToDate(meeting, minutes: -15)
```

Adding `86400.0` seconds isn't always the next day at the same time, and a
month isn't a fixed number of days; `addToDate` follows the calendar:

- A day later is the same time of day, even when the clocks change in
  between (that day is 23 or 25 hours long).
- A month after 31 January is the last day of February: the day is brought
  back into a shorter month.
- Hours, minutes and seconds are simply that much time later.

### daysBetween

```text
daysBetween(_ from: Float, _ to: Float, timeZone: String = "") -> Int
```

How many days later `to` is than `from` on the calendar: how many times the
date changes between them. It's negative if `to` is earlier. Two minutes
that cross midnight are a day apart; 23 hours within one date are not.

```tessel
let left = daysBetween(now(), deadline)
print(if left == 0 { "Due today" } else { "{left} days left" })
```

### Time zones

Every date function has a `timeZone:` argument. Left out, it's the
computer's time zone. Otherwise it's one of:

| Time zone | Example | |
|---|---|---|
| A name | `"Europe/Paris"`, `"America/New_York"`, `"Asia/Tokyo"` | A region and a city, from the time zone database that computers share. It knows that region's summer time, now and in the past. |
| `"UTC"` | | The time the others are counted from. |
| A distance from UTC | `"+05:30"`, `"-08:00"`, `"+2"` | Always that far, with no summer time. |

```tessel
let t = now()
print(formatDate(t, format: "HH:mm", timeZone: "Asia/Tokyo"))
print(formatDate(t, format: "HH:mm", timeZone: "America/New_York"))
```

A time itself has no time zone: it's one moment everywhere. The zone only
decides what the calendar and the clock say at that moment.

A time zone that doesn't exist is treated like a date that doesn't:
`formatDate` gives `""`, `parseDate` gives `nil`, `date` and `addToDate`
give `NaN`, and `dateParts`, `daysBetween` and `timeZoneOffset` give zeros.
Check a name that comes from outside the program with
[`timeZones`](#timezones).

### timeZones

```text
timeZones() -> [String]
```

The names of all time zones, in alphabetical order: about 600 of them. Use
it to offer a choice, or to check a name:

```tessel
if timeZones().contains(name) {
    zone = name
}
```

### localTimeZone

```text
localTimeZone() -> String
```

The name of the computer's time zone, like `"Europe/Paris"`, or `""` if it
can't be found out.

### timeZoneOffset

```text
timeZoneOffset(_ time: Float, timeZone: String = "") -> Int
```

How many seconds the zone's clocks are ahead of UTC at that moment (negative
if they're behind). It depends on the moment, because of summer time.

```tessel
let hours = timeZoneOffset(now(), timeZone: "America/New_York") / 3600   // -5, or -4 in summer
```

## The system and other programs

### appInfo

```text
appInfo() -> AppInfo
```

What the project's [`tessel.toml`](/start/project/) says about the app: a
struct with `name`, `id`, `version`, `build`, `description`, `author` and
`copyright` (all `String`s). What the file leaves out has a default: the
name of the `app` (or of the program), the identifier `dev.tessel.` and the
name, version `"1.0"`, the build its version, and empty text for the rest.

```tessel
let info = appInfo()
print("{info.name} {info.version}")
```

### environment

```text
environment(name: String) -> String?
```

The value of an environment variable, or `nil` if it isn't set.

```tessel
let home = environment("HOME") ?? "unknown"
```

### arguments

```text
arguments() -> [String]
```

The arguments the program was started with, without the program's own name.

```tessel
fn main() {
    let args = arguments()
    if args.isEmpty {
        print("usage: greet <name>")
        exit(1)
    }
    print("Hello, {args[0]}!")
}
```

Run with `tessel run greet.tsl Ada` (or `./greet Ada` once built), this
prints `Hello, Ada!`.

### exit

```text
exit(_ code: Int = 0)
```

Ends the program right away, with an exit code: `0` means success, anything
else a failure. Timers, downloads and other work still waiting are dropped.

### platform

```text
platform() -> String
```

The system the program is running on: `"macos"`, `"windows"` or
`"android"`.

```tessel
let shortcut = if platform() == "macos" { "Cmd+S" } else { "Ctrl+S" }
```

### isDarkMode

```text
isDarkMode() -> Bool
```

Whether the app is showing in its dark appearance: `true` when the system is
dark (or the `app` says `appearance: .dark`). The UI is built again when the
appearance changes, so a view can simply ask:

```tessel
let card: Color = if isDarkMode() { .rgb(44, 44, 46) } else { .rgb(250, 246, 240) }
```

In a program without an `app`, it's `false`. See
[Light and dark](/ui/styling/#light-and-dark).

### fontFamilies

```text
fontFamilies() -> [String]
```

The font families installed on the computer, sorted by name: what
[`.font(family:)`](/reference/modifiers/#font) and `g.text(family:)` can
use. Each in its own font:

```tessel
Scroll {
    VStack(alignment: .leading) {
        for family in fontFamilies() {
            Text(family).font(family: family)
        }
    }
}
```

A family that isn't installed (a document from another computer may ask for
one) is drawn in the system's font.

### textOutline

```text
textOutline(_ text: String, size: Float, weight: Weight = .regular, italic: Bool = false,
            family: String = "system", kerning: Bool = true, tracking: Float = 0) -> [Float]
```

The outline of the text, as [`g.text`](/reference/views/#graphics) draws it,
as path commands from the text's top left corner (y goes down): `0 x y`
moves to a point, `1 x y` draws a line to one, `2 c1x c1y c2x c2y x y` a
curve (the font's own curves, as cubic Béziers), and `3` closes the part.
Text with line breaks is on several lines. No text gives `[]`.

To draw it, add it to a path with [`g.textPath`](/reference/views/#graphics)
instead; this is for keeping the shape, to edit or save it.

### textGlyphs

```text
textGlyphs(_ text: String, size: Float, weight: Weight = .regular, italic: Bool = false,
           family: String = "system", kerning: Bool = true, tracking: Float = 0) -> [TextGlyph]
```

Each glyph of the text as `g.text` places it, after kerning and ligatures,
from the text's top left corner. A `TextGlyph` has:

| Field | |
|---|---|
| `x`, `y: Float` | Where the glyph starts, on its line's baseline. |
| `advance: Float` | How far it moves the next glyph along. |
| `start`, `end: Int` | The characters it shows, as [`substring`](#substring) counts them. A ligature shows several (`fi`); a glyph that shows part of what the one before it does (an accent drawn separately) has `start == end`. |

For placing letters one by one (along a path, say) with the font's own
spacing, or for knowing where a caret goes between two letters.

### fontMetrics

```text
fontMetrics(size: Float, weight: Weight = .regular, italic: Bool = false, family: String = "system") -> FontMetrics
```

How a font is laid out at a size, in points:

| Field | |
|---|---|
| `ascent` | From the baseline up to the top of the tallest letters. |
| `descent` | From the baseline down to the bottom of the deepest (a positive number). |
| `lineGap` | The room the font asks for between lines. |
| `capHeight` | How tall capital letters are. |
| `xHeight` | How tall small letters like x are. |
| `lineHeight` | From one baseline to the next: `ascent + descent + lineGap`. |

`g.text` puts the first baseline `ascent` below the top of the text (with
`.topLeading`), and each next one `lineHeight` further down.

### fontStyles

```text
fontStyles(_ family: String) -> [FontStyle]
```

The faces of an installed family, lightest first: each with its `name` in
the family (`"Light"`, `"Bold Italic"`, `"Condensed Black"`), the
[`Weight`](/reference/colors/#weight) nearest its own, and whether it's
`italic`. A family that isn't installed has none. For a style picker:

```tessel
Picker(selection: style) {
    for (i, s) in fontStyles(family).enumerated() {
        Text(s.name).tag(i)
    }
}
```

Draw in one of them with its `weight` and `italic`. Two faces with the same
weight (a family with condensed ones too) can't be told apart that way.

### setWindowTitle

```text
setWindowTitle(_ title: String)
```

Changes the text in the app's title bar, which starts as the `app`'s
`title:`. A document's name usually goes there:

```tessel
Button("Open…") {
    if let path = openFile() {
        setWindowTitle("Notes — {path.fileName}")
    }
}
```

In a program without a window, it does nothing.

### showNotice

```text
showNotice(_ text: String, seconds: Float = 2.5)
```

Shows a short message near the bottom of the window, which goes by itself
after `seconds`. A new notice takes the place of the one that shows, and an
empty text takes it away.

```tessel
showNotice("Link copied")
```

See [Notices](/ui/dialogs/#notices).

### dismiss

```text
dismiss()
```

Closes the [sheet](/ui/dialogs/#sheets) or
[popover](/ui/menus-windows/#popovers) that's on top, as Escape does: its
`isPresented` becomes `false`. With none open, it does nothing.

```tessel
Button("Cancel") { dismiss() }
```

### undoTextEdit and redoTextEdit

```text
undoTextEdit()
redoTextEdit()
```

Undo and redo the last change to a text, as Cmd+Z and Cmd+Shift+Z do: in
the text field or code editor being edited, or, while none is, in the one
whose text changed last. For Undo and Redo buttons:

```tessel
HStack {
    Button("Undo") { undoTextEdit() }
    Button("Redo") { redoTextEdit() }
}
CodeEditor(text: code)
```

A code editor's history includes the changes the program makes to its text,
so a button that rewrites the code can be undone too. See
[Editing text](/ui/controls/#editing-text). In a program without a window,
they do nothing.

### closeWindow

```text
closeWindow()
```

Closes the window whose event is being handled (the one a button that calls
it is in). Closing the main window ends the app; closing a
[`Window`](/reference/views/#window) sets its `isOpen` to `false`. It doesn't
ask [`.onCloseRequest`](/reference/modifiers/#oncloserequest): it's what
that block calls once it has decided to close.

### quit

```text
quit()
```

Ends the app (all its windows), without asking
[`.onCloseRequest`](/reference/modifiers/#oncloserequest), after the event
being handled. Unlike [`exit`](#exit), the app ends the way it would when the
user closes it.

```tessel
Button("Don't Save") { quit() }
```

### openURL

```text
openURL(_ url: String) -> Bool
```

Opens a web page in the browser, or a file or folder with its usual app.
Returns whether it worked.

```tessel
Button("Help") { openURL("https://tessel-lang.netlify.app") }
Button("Show folder") { openURL(appDataFolder("Notes")) }
```

In a [headless test run](/ui/testing/), nothing opens: it prints
`(open …)` instead.

### copyToClipboard

```text
copyToClipboard(_ text: String) -> Bool
```

Puts text on the system clipboard, as if the user had copied it. Returns
whether it worked.

```tessel
Button("Copy link") { copyToClipboard(link) }
```

### clipboardText

```text
clipboardText() -> String?
```

The text on the clipboard, or `nil` if it has none (for example, when an
image was copied).

```tessel
Button("Paste") { text += clipboardText() ?? "" }
```

A [headless test run](/ui/testing/) uses a clipboard of its own, shared
with its text fields' copy and paste, so tests never touch yours.

### runCommand

```text
runCommand(path: String, args: [String] = [], input: Bool = false,
           onOutput: fn(String), onExit: fn(Int)) -> Int
```

Starts another program and returns right away with an id for
[`stopCommand`](#stopcommand). The program runs in the background:

- `onOutput` is called with each line the program prints, from both its
  standard output and standard error, without the line ending.
- `onExit` is called once, after all the output, with the exit code. The
  code is `-1` if the program couldn't be started or was stopped by a signal.
  If it couldn't be started, `onOutput` first receives a line starting with
  `error: can't start`.

```tessel
app Runner {
    state output: [String] = []

    Button("List files") {
        output = []
        runCommand(path: "ls", args: ["-l"], onOutput: { line in
            output.append(line)
        }) { code in
            output.append("finished with code {code}")
        }
    }
    for line in output {
        Text(line)
    }
}
```

- `path` is the program to run: a path like `/bin/ls`, or a name looked up in
  the folders of the `PATH` environment variable. It isn't run through a
  shell, so each argument goes in `args` separately, and shell features like
  `*` or `|` don't apply.
- The program gets no input, unless you pass `input: true`: then
  [`sendToCommand`](#sendtocommand) writes to it.
- Both blocks run on the UI thread, so they can change `state` directly.

### sendToCommand

```text
sendToCommand(_ id: Int, text: String) -> Bool
```

Writes a line of text (with a line ending after it) to the input of a
program started with [`runCommand`](#runcommand) and `input: true`. Gives
`false` if the program isn't running any more, or wasn't started that way.

```tessel
app Calculator {
    state answers: [String] = []
    state bc = -1

    Button("Start") {
        bc = runCommand(path: "bc", input: true, onOutput: { line in answers.append(line) }) { code in
            bc = -1
        }
    }
    Button("6 × 7") { sendToCommand(bc, text: "6 * 7") }
    for answer in answers {
        Text(answer)
    }
}
```

This is how a program drives another that takes commands as it runs. The
Tessel IDE's debugger talks to `tessel debug --lines` this way.

### stopCommand

```text
stopCommand(id: Int)
```

Stops a program started with [`runCommand`](#runcommand). Its `onExit` block
still runs. Stopping a program that has already finished does nothing.

```tessel
app Watcher {
    state running = -1

    Button("Start") {
        running = runCommand(path: "sleep", args: ["60"], onOutput: { line in print(line) }) { code in
            running = -1
        }
    }
    Button("Stop") { stopCommand(running) }
}
```

## Location, satellites and the compass

Where the device is, the navigation satellites it hears, and which way it
points. These work in apps built [for Android](/guides/android/#where-the-phone-is).
On a computer nothing is allowed and nothing is known (every function
below gives `false`, `nil` or an empty list), unless you
[simulate a device](/guides/android/#trying-it-on-a-computer).

The device is listened to between `startLocation()` and `stopLocation()`
(and `startCompass()` and `stopCompass()`); in between, `location()`,
`satellites()` and `heading()` give the latest that was heard. Read them
when you need them, or in a timer:

```tessel
app Where {
    state place = "Not known yet"

    VStack(spacing: 12) {
        Text(place)
        Button("Find me") {
            if await requestPermission(.location) {
                startLocation()
            }
        }
    }
    .onAppear {
        every(seconds: 1) {
            if let l = location() {
                place = "{l.latitude.formatted(5)}, {l.longitude.formatted(5)}"
            }
        }
    }
}
```

### requestPermission

```text
async requestPermission(_ permission: Permission) -> Bool
hasPermission(_ permission: Permission) -> Bool
```

`requestPermission` asks the user to allow something, and waits for the
answer: `true` if they did. If they've already answered, it says what they
said without asking again (Android stops asking after the second refusal).
`hasPermission` only looks.

`Permission` is `.location` (where the device is, as exactly as it can
tell) or `.approximateLocation` (to within a few kilometers). When asked for
`.location`, the user may grant only the approximate one: then
`requestPermission(.location)` is `false`, and
`hasPermission(.approximateLocation)` is `true`.

Like every `async` function, it's called with `await`, or with a block that
gets the answer:

```tessel
requestPermission(.location) { allowed in
    if allowed { startLocation() }
}
```

Ask when the user does something that needs it, like pressing a button, so
they know why they're asked.

### startLocation

```text
startLocation(precise: Bool = true, interval: Float = 1) -> Bool
stopLocation()
```

Starts listening for where the device is, about every `interval` seconds.
It returns `false` if the user hasn't allowed it (see
[`requestPermission`](#requestpermission)) or the device can't tell.

- With `precise:` (and the `.location` permission) the satellites are
  used. Without it, only the phone network and Wi-Fi, which use less
  battery and are rougher.
- Listening pauses while the app isn't in front, and goes on when it is
  again.
- `stopLocation()` stops it; `location()` is `nil` and `satellites()`
  empty again.

### location

```text
location() -> Location?
```

Where the device was last heard to be, or `nil` before the first answer
(which can take a while outdoors, and may never come indoors). A `Location`
has:

| Field | |
|---|---|
| `latitude: Float` | Degrees north of the equator; south is negative. |
| `longitude: Float` | Degrees east of Greenwich; west is negative. |
| `altitude: Float?` | Meters above the WGS 84 ellipsoid, which is not quite sea level. |
| `accuracy: Float?` | How far off the place may be, in meters. |
| `altitudeAccuracy: Float?` | How far off the altitude may be, in meters. |
| `speed: Float?` | Meters a second. |
| `course: Float?` | The direction of movement, in degrees clockwise from north. |
| `time: Float` | When the device was there (see [`now`](#now)). The first answer can be an old one: the last place the phone knew. |
| `source: String` | `"satellites"`, `"network"` or `"other"`. |

What the device doesn't know is `nil`.

### satellites

```text
satellites() -> [Satellite]
```

The navigation satellites the device hears, after
[`startLocation`](#startlocation) with the `.location` permission. Empty
until the receiver reports. Each `Satellite` has:

| Field | |
|---|---|
| `system: String` | `"GPS"`, `"GLONASS"`, `"Galileo"`, `"BeiDou"`, `"QZSS"`, `"SBAS"`, `"NavIC"` or `"unknown"`. |
| `number: Int` | Its number in its system. |
| `azimuth: Float` | Where it is in the sky: degrees clockwise from north. |
| `elevation: Float` | Degrees above the horizon (90 is overhead). |
| `signal: Float` | How strongly it's heard: the carrier-to-noise density, in dB-Hz. 0 when it isn't heard. |
| `used: Bool` | Whether the device's place is worked out from it. |
| `hasEphemeris: Bool` | Whether the device knows its exact orbit. |
| `hasAlmanac: Bool` | Whether the device knows its rough orbit. |
| `frequency: Float?` | Of the signal, in MHz. A satellite heard on two frequencies is listed twice. |

```tessel
let used = satellites().filter { s in s.used }.count
```

### startCompass

```text
startCompass() -> Bool
stopCompass()
```

Starts listening to the device's compass. `false` on a device without
one. It needs no permission. Like the location, it pauses while the app
isn't in front.

### heading

```text
heading() -> Heading?
```

Which way the top of the screen points with the device lying flat, or `nil`
before [`startCompass`](#startcompass) has a reading. A `Heading` has:

| Field | |
|---|---|
| `magnetic: Float` | Degrees clockwise from magnetic north (where a compass needle points), from 0 up to 360. |
| `geographic: Float?` | Degrees clockwise from the north of maps. It's worked out from where the device is, so it's `nil` until `location()` has an answer. |
| `accuracy: Int` | How sure the compass is, from 0 (not at all: move the phone in a figure of 8) to 3. |

## Callbacks

The functions below finish their work later: the network, timers and
background work. They take a block (a *callback*) that Tessel runs when the
work is done. Callbacks run on the UI thread in an app, so they can change
`state` directly. They work in a program with only `fn main()` too: after
`main` returns, the program keeps running until no timers, requests,
WebSockets or background work are left, then exits. See
[Files, commands and timers](/ui/async/#callbacks-in-a-program-without-an-app).

## Networking

### fetch

```text
fetch(url: String, done: fn(String?))
```

Loads a URL in the background, then calls `done` with the text, or `nil` if
loading failed.

```tessel
app Weather {
    state report = "Loading…"

    Text(report).onAppear {
        fetch(url: "https://example.com/weather.txt") { text in
            report = text ?? "Could not load the weather"
        }
    }
}
```

- `http://` and `https://` URLs are loaded with a GET request. An error
  status (like 404) counts as failure.
- `file://` URLs read a local file: `file:///Users/ada/notes.txt`.
- The response is read as text.

For other methods (like POST), headers, or the status code, use
[`request`](#request).

### request

```text
request(url: String, method: String = "GET", headers: [String: String] = [:],
        body: String = "", done: fn(HttpResponse)) -> Int
```

Sends an HTTP request in the background, then calls `done` with the
response. It returns an id, for [`cancelRequest`](#cancelrequest); most
programs don't need it.

```tessel
struct Todo {
    title: String
    done: Bool = false
}

app Todos {
    state status = ""

    Button("Add") {
        request(url: "https://example.com/api/todos", method: "POST",
                headers: ["Content-Type": "application/json"],
                body: toJson(Todo(title: "Buy milk"))) { response in
            if response.ok {
                status = "Added"
            } else if response.status == 0 {
                status = "Couldn't connect: {response.error}"
            } else {
                status = "The server said {response.status}"
            }
        }
    }
    Text(status)
}
```

- `method` can be any HTTP method: `"GET"`, `"POST"`, `"PUT"`, `"PATCH"`,
  `"DELETE"`, and so on.
- `headers` are sent with the request. `body` is sent as it is; use
  [`toJson`](#tojson) to send a struct as JSON.
- A response with an error status (like 404 or 500) is still a response:
  `done` gets it, with its status and body. Check `ok` or `status`.
- `file://` URLs read a local file, with status 200.

### HttpResponse

What `request` gives its block. It's a built-in struct with these fields:

| Field | Type | |
|---|---|---|
| `status` | `Int` | The HTTP status code, like `200` or `404`. `0` if there was no response at all. |
| `ok` | `Bool` | Whether the status is from 200 to 299: the request worked. |
| `text` | `String` | The body as text. |
| `data` | `Data` | The body as bytes, for images and other files that aren't text. |
| `headers` | `[String: String]` | The response's headers. The names are in lower case: `response.headers["content-type"]`. |
| `error` | `String` | Why there was no response (the server couldn't be reached, the URL is invalid, …), or `""`. |

To read a JSON answer, pass `text` to [`fromJson`](#fromjson):

```tessel
request(url: "https://example.com/api/todos") { response in
    if let todos: [Todo] = fromJson(response.text) {
        items = todos
    }
}
```

### download

```text
download(url: String, to: String, progress: fn(Float)? = nil, done: fn(Bool)) -> Int
```

Downloads a URL straight into a file, which suits large files. `progress`,
if given, is called now and then with how much has arrived, from `0.0` to
`1.0`. `done` is called at the end with whether it worked. It returns an id,
for [`cancelRequest`](#cancelrequest).

```tessel
app Downloader {
    state progress = 0.0
    state status = ""

    Button("Download") {
        let file = joinPath(tempFolder(), "big.zip")
        download(url: "https://example.com/big.zip", to: file, progress: { fraction in
            progress = fraction
        }) { ok in
            status = if ok { "Done" } else { "Download failed" }
        }
    }
    Text("{Int(progress * 100.0)}%  {status}")
}
```

- An error status (like 404) counts as failure.
- While downloading, the data goes to a file with `.part` added to the name;
  it's renamed to `to` only once everything has arrived. So a failed download
  leaves no half-written file behind. An existing file at `to` is replaced.
- `progress` gets its fractions only if the size is known (the server says
  how big the file is, or it's a local file). Either way, it's called with
  `1.0` exactly once, when everything has arrived.
- `http://`, `https://` and `file://` URLs can be downloaded; `file://`
  copies a local file.

### cancelRequest

```text
cancelRequest(_ id: Int)
```

Gives up a [`request`](#request) or a [`download`](#download) that's under
way. Its `done` block is never called (nor `progress`), and a cancelled
download leaves no file behind, not even a partial one.

```tessel
app Downloader {
    state loading: Int? = nil
    state status = ""

    if let id = loading {
        Button("Cancel") {
            cancelRequest(id)
            loading = nil
            status = "Cancelled"
        }
    } else {
        Button("Download") {
            loading = download(url: "https://example.com/big.zip", to: "big.zip") { ok in
                loading = nil
                status = if ok { "Done" } else { "Download failed" }
            }
        }
    }
    Text(status)
}
```

- Since `done` isn't called, do your own tidying up where you cancel, as
  the example does.
- Cancelling something that has finished already, or an id that was never
  given out, does nothing.
- A request that's still waiting for the server to answer is dropped by your
  program at once, though the connection itself is only closed when the
  server answers or gives up.

### openWebSocket

```text
openWebSocket(url: String, onMessage: fn(String), onData: fn(Data)? = nil,
              onClose: fn(String)) -> Int
```

Connects to a WebSocket server (`ws://` or `wss://`) and returns an id for
[`sendWebSocket`](#sendwebsocket) and [`closeWebSocket`](#closewebsocket).
The connection stays open, and messages can go both ways:

- `onMessage` is called with each text message the server sends.
- `onData`, if given, is called with each binary message, as
  [`Data`](#data). Without it, binary messages go to `onMessage`, as text.
- `onClose` is called once, when the connection ends, with the reason:
  `"closed"` if you closed it, the server's reason (or
  `"closed by the server"`), or what went wrong, like
  `"couldn't connect: …"`.

```tessel
app Chat {
    state socket = -1
    state messages: [String] = []
    state draft = ""

    VStack {
        for message in messages {
            Text(message)
        }
        TextField("Message", text: draft).onSubmit {
            sendWebSocket(socket, text: draft)
            draft = ""
        }
    }
    .onAppear {
        socket = openWebSocket(url: "wss://example.com/chat", onMessage: { text in
            messages.append(text)
        }) { reason in
            messages.append("Disconnected: {reason}")
            socket = -1
        }
    }
}
```

A server that sends binary messages (bytes rather than text) needs `onData`:

```tessel
socket = openWebSocket(url: "wss://example.com/feed", onMessage: { text in
    log.append(text)
}, onData: { data in
    received += data.count
}) { reason in
    socket = -1
}
```

### sendWebSocket

```text
sendWebSocket(_ id: Int, text: String) -> Bool
```

Sends a text message. Returns `false` if the WebSocket isn't open (it was
closed, or `id` is wrong). Messages sent before the connection is ready are
sent once it is.

### sendWebSocketData

```text
sendWebSocketData(_ id: Int, data: Data) -> Bool
```

Sends a binary message: the bytes of a [`Data`](#data). Like
[`sendWebSocket`](#sendwebsocket), it returns `false` if the WebSocket isn't
open.

```tessel
sendWebSocketData(socket, data: toBinary(move))
sendWebSocketData(socket, data: Data(bytes: [1, 2, 255]))
```

### closeWebSocket

```text
closeWebSocket(_ id: Int)
```

Closes a WebSocket. Its `onClose` block still runs, with `"closed"`. Closing
one that is already closed does nothing.

## Timers

### after

```text
after(seconds: Float, action: fn())
```

Runs `action` once, after a delay, on the UI thread.

```tessel
app Reminder {
    state showHint = false

    Text("Welcome!").onAppear {
        after(seconds: 2) { showHint = true }
    }
    Text("Tip: press Cmd+S to save").hidden(!showHint)
}
```

A delay of `0` (or less) runs `action` as soon as possible, after the current
code finishes. An `after` timer can't be cancelled; to repeat, use
[`every`](#every).

### every

```text
every(seconds: Float, action: fn()) -> Int
```

Runs `action` again and again, every `seconds`, until it's stopped with
[`stopTimer`](#stoptimer). The first run is one interval from now. Returns an
id for `stopTimer`.

```tessel
app Clock {
    state time = ""

    Text(time).font(size: 32).onAppear {
        every(seconds: 1) {
            time = formatDate(now(), format: "HH:mm:ss")
        }
    }
}
```

### stopTimer

```text
stopTimer(_ id: Int)
```

Stops a timer started with `every`. The timer can stop itself:

```tessel
fn main() {
    var ticks = 0
    var id = 0
    id = every(seconds: 0.5) {
        ticks += 1
        print("tick {ticks}")
        if ticks == 3 {
            stopTimer(id)
        }
    }
}
```

This prints `tick 1`, `tick 2` and `tick 3`, then the program ends, since
nothing is left to wait for. Stopping a timer that has already stopped does
nothing.

## Background work

### background

```text
background(work: fn(BackgroundJob) -> T, progress: fn(Float, String) = …, done: fn(T)) -> BackgroundJob
```

Runs `work` on another thread, so a long computation doesn't freeze the app,
then calls `done` with its result on the UI thread. It's usually written with
`done` as a trailing block:

```tessel
let n = limit
background(work: { primesBelow(limit: n) }) { primes in
    status = "{primes.count} primes"
}
```

The two threads share nothing: `work` gets its own copies of the values it
uses, and `done` gets a copy of the result. That's what makes it safe, and
it leads to a few rules, which `tessel check` enforces:

- `work` can use local values, and call top-level functions (`fn`s outside
  any app, view or struct) and methods of the values it has.
- It can't use `state`, bindings, `self` or its fields, or the functions of
  the app or view: they belong to the UI thread. Copy what it needs into a
  `let` first, as with `n` above.
- The values it uses and its result must be values [`toBinary`](#tobinary)
  can save: not functions or views.
- `work` must end with a value. `return` works inside it too.

Values are copied when `background` is called, so changes made afterwards
don't reach `work`. See [Heavy work in the background](/ui/async/#heavy-work-in-the-background)
for a complete app.

A runtime error in `work` (like an index out of range) stops the program,
as it would anywhere else.

`work` may take the job as its one value, `work: { job in … }`: then it can
report how far it is with `job.progress(…)` and stop early when
`job.isCancelled()`. The `progress:` block gets the reports on the UI thread.
`background` returns the job, for cancelling it:

```tessel
let path = file
job = background(work: { job in
    var rows: [String] = []
    let lines = readFile(path)?.lines() ?? []
    for (i, line) in lines.enumerated() {
        if job.isCancelled() {
            return rows
        }
        rows.append(line.trim())
        job.progress(Float(i + 1) / Float(lines.count), note: "line {i + 1}")
    }
    rows
}, progress: { fraction, note in
    loaded = fraction
}) { rows in
    table = rows
}
```

- The `progress:` block runs at most once each time the app handles its
  events, with the latest report: reports made in between are skipped. The
  last report always arrives before `done`.
- After `job.cancel()`, neither `progress:` nor `done` runs any more, and
  `job.isCancelled()` turns true inside `work`, which should then stop
  (whatever it returns is dropped). Cancelling work that has finished does
  nothing.

### BackgroundJob

A struct for work started with [`background`](#background), with one field,
`id: Int`, and these methods. Being a plain value, it can be kept in `state`
(`state job: BackgroundJob? = nil`).

| Method | Returns | |
|---|---|---|
| `progress(_ fraction: Float, note: String = "")` | | Inside `work`: reports how far it is (usually 0 to 1) to the `progress:` block. |
| `isCancelled()` | `Bool` | Whether `cancel()` was called. |
| `cancel()` | | Cancels the work: its blocks don't run any more. |

## Int

An `Int` is a whole number. The arithmetic operators work on it, and
[`abs`, `min` and `max`](#math) take `Int`s too.

| Member | Returns | |
|---|---|---|
| `isEven` | `Bool` | Whether the number is even. |
| `isOdd` | `Bool` | Whether the number is odd. |
| `clamped(min: Int, max: Int)` | `Int` | The number, moved into the range from `min` to `max`. |
| `bytes(count: Int, bigEndian: Bool = false)` | `[Int]` | The number as `count` bytes (1 to 8), as files store it. See [bytes](#bytes). |
| `wrappingAdd(_ other: Int)` | `Int` | The sum, going round past the largest and smallest `Int`. |
| `wrappingSubtract(_ other: Int)` | `Int` | The difference, going round the same way. |
| `wrappingMultiply(_ other: Int)` | `Int` | The product, going round the same way. |

```tessel
print((4).isEven)                       // true
print((-3).isOdd)                       // true
print((15).clamped(min: 0, max: 10))    // 10
let row = 7
let shade = if row.isEven { Color.lightGray } else { Color.white }
```

Write a number literal in parentheses to use a member on it: `(4).isEven`.

`+`, `-` and `*` stop the program when the result doesn't fit in an `Int`
(it's past 9 223 372 036 854 775 807 or below −9 223 372 036 854 775 808),
since a wrong number would be worse. Hashes and checksums want the other
behavior: the `wrapping` methods keep the lowest 64 bits, so the largest
`Int` plus 1 is the smallest.

```tessel
// An FNV-1a hash.
var h = -3750763034362895579
for b in Data(text: "hello").bytes {
    h = (h ^ b).wrappingMultiply(1099511628211)
}
```

## Float

A `Float` is a decimal number. The arithmetic operators work on it, and the
[math functions](#math) take and return `Float`s.

| Member | Returns | |
|---|---|---|
| `formatted(decimals: Int)` | `String` | The number as text, with exactly `decimals` digits after the point. |
| `clamped(min: Float, max: Float)` | `Float` | The number, moved into the range from `min` to `max`. |
| `isNaN` | `Bool` | Whether the value is `NaN`, "not a number" (like `0.0 / 0.0` or `sqrt(-1.0)`). |
| `isFinite` | `Bool` | Whether the value is an ordinary number: not infinite and not `NaN`. |
| `bytes(count: Int = 8, bigEndian: Bool = false)` | `[Int]` | The number as 8 bytes (or 4, with less precision), as files store it. See [bytes](#bytes). |

```tessel
let volume = 1.4
print(volume.clamped(min: 0.0, max: 1.0))   // 1.0
print(sqrt(-1.0).isNaN)                     // true
print((1.0 / 0.0).isFinite)                 // false
```

`NaN` is never equal to anything, not even itself, so `x == x` is `false`
for a `NaN`: use `isNaN` to check for it.

### bytes

```text
n.bytes(count: Int, bigEndian: Bool = false) -> [Int]      (an Int)
x.bytes(count: Int = 8, bigEndian: Bool = false) -> [Int]  (a Float)
```

A number as the bytes a file stores it in, each from 0 to 255: the opposite
of `Data`'s [`int32(at:)`, `float64(at:)` and the others](#numbers-in-bytes).

```tessel
print((258).bytes(count: 2))                    // [2, 1]
print((258).bytes(count: 4, bigEndian: true))   // [0, 0, 1, 2]
print((-1).bytes(count: 2))                     // [255, 255]
print((1.5).bytes(count: 4))                    // [0, 0, 192, 63]
print((1.5).bytes().count)                      // 8
```

An `Int` that doesn't fit in `count` bytes keeps its lowest bytes. A
`Float` takes 8 bytes, or 4 with about 7 digits of precision.

### formatted

```tessel
let third = 1.0 / 3.0
print(third)                            // 0.3333333333333333
print(third.formatted(decimals: 2))     // 0.33
print((2.0).formatted(decimals: 3))     // 2.000
print((0.666).formatted(decimals: 0))   // 1
let area = pi * 0.5 * 0.5
print("Area: {area.formatted(decimals: 4)} m²")   // Area: 0.7854 m²
```

- The number is rounded to `decimals` places. An exact half goes to the even
  digit, so `(2.5).formatted(decimals: 0)` is `"2"` and
  `(3.5).formatted(decimals: 0)` is `"4"`. Because most decimal fractions
  can't be stored exactly, a value like `1.005` may round down.
- `decimals` can be from 0 to 15; values outside that range are treated as 0
  or 15. With `0`, there's no decimal point.
- A result that would be a negative zero, like `-0.00`, is shown as `0.00`.
- `formatted` is a `Float` method only. For an `Int`, convert first:
  `Float(count).formatted(decimals: 1)`.

## String

A `String` is text. Strings can't be changed in place; methods return new
strings. Join strings with `+` or interpolation (`"{a}{b}"`); `+=` appends to
a `String` variable. `==`, `!=`, `<`, `<=`, `>` and `>=` compare strings;
`<` compares character codes, so `"B" < "a"`.

"Character" below means a Unicode character (code point): `"é".count` is 1.

| Member | Returns | |
|---|---|---|
| `count` | `Int` | Number of characters. |
| `isEmpty` | `Bool` | Whether the text is `""`. |
| `contains(_ text: String)` | `Bool` | Whether `text` appears in it. |
| `hasPrefix(_ text: String)` | `Bool` | Whether it starts with `text`. |
| `hasSuffix(_ text: String)` | `Bool` | Whether it ends with `text`. |
| `split(_ separator: String)` | `[String]` | The parts between separators. |
| `lines()` | `[String]` | The lines, without line endings. |
| `trim()` | `String` | Without spaces and line breaks at both ends. |
| `uppercase()` | `String` | In capital letters. |
| `lowercase()` | `String` | In small letters. |
| `replace(_ text: String, with: String)` | `String` | With every `text` replaced. |
| `substring(from: Int, to: Int)` | `String` | The characters from index `from` up to `to`. |
| `find(_ text: String)` | `Int?` | The index where `text` first appears. |
| `findLast(_ text: String)` | `Int?` | The index where `text` last appears. |
| `character(at: Int)` | `String?` | The character at an index, or `nil`. |
| `characters` | `[String]` | Each character, as a list. |
| `words()` | `[String]` | The words: the parts between spaces, tabs and line breaks. |
| `occurrences(of: String)` | `Int` | How many times a text appears. |
| `trimStart()` | `String` | Without spaces and line breaks at the start. |
| `trimEnd()` | `String` | Without spaces and line breaks at the end. |
| `capitalized()` | `String` | With the first letter of each word in capitals. |
| `reversed()` | `String` | The characters in reverse order. |
| `repeated(_ times: Int)` | `String` | The text repeated `times` times. |
| `padStart(_ length: Int, with: String = " ")` | `String` | Filled at the start up to `length` characters. |
| `padEnd(_ length: Int, with: String = " ")` | `String` | Filled at the end up to `length` characters. |
| `urlEncoded` | `String` | Made safe to put in a URL. |
| `urlDecoded` | `String?` | A `urlEncoded` text turned back. |
| `fileName` | `String` | For a path: the last part. |
| `fileExtension` | `String` | For a path: the file's extension, without the dot. |
| `folder` | `String` | For a path: the folder it's in. |

Strings can't be indexed with `[…]`; use `character(at:)`, `substring`,
`split` or `find`.

### count and isEmpty

```tessel
let word = "héllo"
print(word.count)               // 5
print(word.isEmpty)             // false
print("".isEmpty)               // true
```

### contains, hasPrefix and hasSuffix

```tessel
let file = "main.tsl"
print(file.contains("in"))      // true
print(file.hasPrefix("main"))   // true
print(file.hasSuffix(".tsl"))   // true
print(file.contains("Main"))    // false
```

All three are case-sensitive. Every string contains, starts and ends with
`""`.

### split

```tessel
let parts = "a,,b,".split(",")
print(parts.count)                      // 4
print(parts.joined(separator: "|"))     // a||b|
print("abc".split("").joined(separator: " "))   // a b c
```

- Empty parts are kept: two separators in a row give an empty string between
  them, and a separator at the start or end gives an empty first or last
  part. Splitting `""` gives `[""]`.
- An empty separator splits the text into single characters.

### lines

```tessel
let text = "one\ntwo\r\nthree\n"
print(text.lines().count)       // 3
print(text.lines()[1])          // two
```

Splits at `\n` and at `\r\n`. A line break at the very end doesn't start
another line, and `"".lines()` is empty.

### trim

```tessel
print("[" + "  hi \n".trim() + "]")     // [hi]
```

Removes spaces, tabs and line breaks from both ends, not from the middle.

### uppercase and lowercase

```tessel
print("Hello".uppercase())      // HELLO
print("Hello".lowercase())      // hello
print("straße".uppercase())     // STRASSE
```

They follow Unicode rules, so a result can have a different length.

### replace

```tessel
print("a-b-a".replace("a", with: "x"))  // x-b-x
```

Replaces every occurrence, left to right. Replacing `""` returns the text
unchanged.

### substring

```tessel
let word = "héllo"
print(word.substring(from: 1, to: 3))   // él
print(word.substring(from: 2, to: 100)) // llo
```

- Indexes count characters from 0. `from` is included, `to` is not.
- Out-of-range indexes are clamped to the text, so `substring` never stops
  the program. If `to` isn't after `from`, the result is `""`.

### find

```tessel
let line = "error: missing value"
print(line.find(":") ?? -1)             // 5
print(line.find("warning") ?? -1)       // -1
if let i = line.find(": ") {
    print(line.substring(from: i + 2, to: line.count))  // missing value
}
```

The character index (from 0) of the first place `text` appears, or `nil` if
it doesn't. The index works with `substring`.

### findLast

```tessel
let path = "docs/guide/intro.md"
print(path.findLast("/") ?? -1)         // 10
print("a-b-a".findLast("a") ?? -1)      // 4
```

Like `find`, but the index of the **last** place `text` appears.

### character(at:) and characters

```tessel
let word = "héllo"
print(word.character(at: 1) ?? "?")     // é
print(word.character(at: 9) ?? "none")  // none
print(word.characters.count)            // 5
for letter in "abc".characters {
    print(letter)
}
```

`character(at:)` is `nil` for an index outside the text. `characters` is a
property (no parentheses): a list with each character as a one-character
string.

### words

```tessel
let words = "  one two\tthree\n".words()
print(words.count)                      // 3
print(words.joined(separator: ","))     // one,two,three
```

Splits at runs of spaces, tabs and line breaks, and leaves out empty parts,
unlike [`split`](#split).

### occurrences

```tessel
print("a,b,,c".occurrences(of: ","))    // 3
print("banana".occurrences(of: "ana"))  // 1
```

Counts from left to right without overlapping, so `"ana"` is found once in
`"banana"`. Counting `""` gives `0`.

### trimStart and trimEnd

```tessel
print("[" + "  hi  ".trimStart() + "]")     // [hi  ]
print("[" + "  hi  ".trimEnd() + "]")       // [  hi]
```

Like [`trim`](#trim), but at one end only.

### capitalized

```tessel
print("hello big world".capitalized())      // Hello Big World
```

Makes the first letter of each word a capital. The other letters are left as
they are.

### reversed and repeated

```tessel
print("héllo".reversed())               // olléh
print("ab".repeated(3))                 // ababab
print("=".repeated(10))                 // ==========
```

`repeated` with `0` or less gives `""`.

### padStart and padEnd

```tessel
print("7".padStart(3, with: "0"))       // 007
print("[" + "ab".padEnd(5) + "]")       // [ab   ]
print("12345".padStart(3))              // 12345
```

They add `with` (a space unless you say otherwise) until the text is
`length` characters long, which lines up columns of text and numbers. Text
that is already long enough is left as it is.

### urlEncoded and urlDecoded

```tessel
let query = "fish & chips"
let url = "https://example.com/search?q={query.urlEncoded}"
print(url)          // https://example.com/search?q=fish%20%26%20chips
print("fish%20%26%20chips".urlDecoded ?? "")    // fish & chips
print("%zz".urlDecoded ?? "invalid")            // invalid
```

- `urlEncoded` keeps letters, digits and `- _ . ~`, and writes every other
  character as `%` and a code. Use it for a single part of a URL, like a
  query value or a path segment, not a whole URL.
- `urlDecoded` also reads `+` as a space. It's `nil` if the text has a `%`
  that isn't followed by a valid code.

### Paths

Three properties take a file path apart:

```tessel
let path = "/Users/ada/notes/todo.txt"
print(path.fileName)            // todo.txt
print(path.fileExtension)       // txt
print(path.folder)              // /Users/ada/notes
```

- `fileName` is the part after the last `/` (or `\`), the whole text if
  there's none.
- `fileExtension` is the part of the file name after its last dot:
  `"archive.tar.gz"` gives `"gz"`. It's `""` if there's no dot, and for names
  that start with a dot, like `".profile"`.
- `folder` is the part before the last `/` (or `\`), or `""` if there's
  none.

To put paths together, use [`joinPath`](#joinpath).

## List

A list `[T]` holds values of one type in order. Write one as `[1, 2, 3]`; an
empty list needs its type from context: `var names: [String] = []`.

| Member | Returns | |
|---|---|---|
| `count` | `Int` | Number of items. |
| `isEmpty` | `Bool` | Whether there are no items. |
| `first` | `T?` | The first item, or `nil` if empty. |
| `last` | `T?` | The last item, or `nil` if empty. |
| `append(_ item: T)` | | Adds `item` at the end. Changes the list. |
| `insert(_ item: T, at: Int)` | | Inserts `item` before index `at`. Changes the list. |
| `remove(at: Int)` | `T` | Removes and returns the item at `at`. Changes the list. |
| `removeAll(where: fn(T) -> Bool)` | | Removes every item for which the block is `true`. Changes the list. |
| `filter(_ keep: fn(T) -> Bool)` | `[T]` | The items for which the block is `true`. |
| `first(where: fn(T) -> Bool)` | `T?` | The first item for which the block is `true`. |
| `last(where: fn(T) -> Bool)` | `T?` | The last item for which the block is `true`. |
| `contains(_ item: T)` | `Bool` | Whether an item equals `item`. |
| `reversed()` | `[T]` | The items in reverse order. |
| `map(_ transform: fn(T) -> U)` | `[U]` | Each item transformed by the block. |
| `sorted(by: fn(T, T) -> Bool = …)` | `[T]` | The items in order. |
| `joined(separator: String = "")` | `String` | Only for `[String]`: the items joined into one string. |
| `indices` | `Range` | The valid indexes, `0..count`. |
| `index(of: T)` | `Int?` | The index of the first item equal to the value. |
| `lastIndex(of: T)` | `Int?` | The index of the last item equal to the value. |
| `firstIndex(where: fn(T) -> Bool)` | `Int?` | The index of the first item for which the block is `true`. |
| `count(where: fn(T) -> Bool)` | `Int` | How many items the block is `true` for. |
| `any(where: fn(T) -> Bool)` | `Bool` | Whether the block is `true` for at least one item. |
| `all(where: fn(T) -> Bool)` | `Bool` | Whether the block is `true` for every item. |
| `sum()` | `T` | Only for `[Int]` and `[Float]`: the items added up. |
| `min()` | `T?` | Only for `[Int]`, `[Float]` and `[String]`: the smallest item. |
| `max()` | `T?` | Only for `[Int]`, `[Float]` and `[String]`: the largest item. |
| `prefix(_ count: Int)` | `[T]` | The first `count` items. |
| `suffix(_ count: Int)` | `[T]` | The last `count` items. |
| `dropFirst(_ count: Int = 1)` | `[T]` | Without the first `count` items. |
| `dropLast(_ count: Int = 1)` | `[T]` | Without the last `count` items. |
| `slice(from: Int, to: Int)` | `[T]` | The items from index `from` up to `to`. |
| `unique()` | `[T]` | Without repeated items. |
| `shuffled()` | `[T]` | The items in random order. |
| `randomElement()` | `T?` | A random item, or `nil` if empty. |
| `enumerated()` | `[(Int, T)]` | Each item with its index: `for (i, x) in list.enumerated()`. |
| `appendAll(_ items: [T])` | | Adds all of `items` at the end. Changes the list. |
| `removeFirst()` | `T` | Removes and returns the first item. Changes the list. |
| `removeLast()` | `T` | Removes and returns the last item. Changes the list. |

Other list operations:

- `list[i]` reads the item at index `i` (from 0), and `list[i] = value`
  replaces it. An index outside the list stops the program with an error.
- `a + b` is a new list with the items of `a` then `b`; `list += more`
  adds the items of `more` to `list` where it is.
- `==` and `!=` compare two lists item by item.
- `for item in list { … }` loops over the items. See
  [Collections](/language/collections/).

Methods that change the list (`append`, `appendAll`, `insert`, `remove`,
`removeFirst`, `removeLast`, `removeAll`) need a list you can change: a `var`, a `state`, or a `bind` parameter, or a
field of one. The others return a new list and leave the original alone.

### first and last

```tessel
let scores = [40, 75, 90]
print(scores.first ?? 0)                        // 40
print(scores.last ?? 0)                         // 90
print(scores.first(where: { s in s > 50 }) ?? 0)  // 75
print(scores.last(where: { s in s < 50 }) ?? 0)   // 40
```

Without parentheses, `first` and `last` are properties; with a `where:`
block, they search.

### append, insert and remove

```tessel
var items = ["b", "c"]
items.append("d")
items.insert("a", at: 0)
print(items.joined(separator: ","))     // a,b,c,d
let removed = items.remove(at: 1)
print(removed)                          // b
print(items.joined(separator: ","))     // a,c,d
```

`insert(at:)` accepts indexes from `0` to `count` (the end). `remove(at:)`
needs an index of an existing item. Other indexes stop the program with an
error.

### removeAll

```tessel
var numbers = [1, 5, 2, 8, 3]
numbers.removeAll(where: { n in n > 2 })
print(numbers.count)                    // 2
```

### filter

```tessel
let numbers = [1, 5, 2, 8, 3]
let big = numbers.filter { n in n > 2 }
print(big.count)                        // 3
```

### contains(_:)

```tessel
let tags = ["red", "green"]
print(tags.contains("green"))           // true
print(tags.contains("blue"))            // false
```

### reversed

```tessel
let letters = ["a", "b", "c"]
print(letters.reversed().joined())      // cba
```

### map

```tessel
let numbers = [1, 2, 3]
let labels = numbers.map { n in "#{n}" }
print(labels.joined(separator: " "))    // #1 #2 #3
```

The block's result type decides the new list's type; the block must return a
value.

### sorted

```tessel
let names = ["Linus", "ada", "Grace"]
print(names.sorted().joined(separator: ","))        // Grace,Linus,ada
let byLength = names.sorted(by: { a, b in a.count < b.count })
print(byLength.joined(separator: ","))              // ada,Linus,Grace
```

- Without `by:`, lists of `Int`, `Float` and `String` are sorted smallest
  first. Strings are compared by character code, so capital letters come
  before small ones.
- Other lists (like a list of structs) need `by:`: a block that returns
  `true` when its first value should come before its second.
- The sort is stable: items that are equal keep their order.

### joined

```tessel
let words = ["one", "two", "three"]
print(words.joined(separator: ", "))    // one, two, three
print(words.joined())                   // onetwothree
```

Only lists of `String` have `joined`. To join other values, `map` them to
strings first.

### indices

```tessel
let names = ["Ada", "Grace", "Linus"]
print(names.indices)                    // 0..3
for i in names.indices {
    print("{i + 1}. {names[i]}")
}
```

A range of every valid index, for when you need the index as well as the
item.

### index(of:) and lastIndex(of:)

```tessel
let nums = [4, 8, 15, 8]
print(nums.index(of: 8) ?? -1)          // 1
print(nums.lastIndex(of: 8) ?? -1)      // 3
print(nums.index(of: 99) ?? -1)         // -1
```

`nil` if no item is equal to the value. They work for any list whose items
can be compared with `==`, structs included.

### firstIndex(where:)

```tessel
let nums = [4, 8, 15, 16]
print(nums.firstIndex(where: { n in n > 10 }) ?? -1)    // 2
```

### count(where:), any(where:) and all(where:)

```tessel
let nums = [4, 8, 15, 16, 23]
print(nums.count(where: { n in n.isEven }))     // 3
print(nums.any(where: { n in n > 20 }))         // true
print(nums.all(where: { n in n > 5 }))          // false
```

For an empty list, `any` is `false` and `all` is `true`.

### sum, min and max

```tessel
let nums = [4, 8, 15]
print(nums.sum())                       // 27
print(nums.min() ?? 0)                  // 4
print(nums.max() ?? 0)                  // 15
print(["pear", "apple"].min() ?? "")    // apple
```

- `sum` is for lists of `Int` or `Float`; it's `0` for an empty list.
- `min` and `max` are for lists of `Int`, `Float` or `String`, and are `nil`
  for an empty list. Strings are compared as with `<`.
- For other lists, `map` to numbers first, or use `sorted(by:)`.

### prefix, suffix, dropFirst and dropLast

```tessel
let nums = [1, 2, 3, 4, 5]
print(nums.prefix(2).sum())             // 3   (1 + 2)
print(nums.suffix(2).sum())             // 9   (4 + 5)
print(nums.dropFirst().count)           // 4
print(nums.dropLast(2).sum())           // 6   (1 + 2 + 3)
print(nums.prefix(100).count)           // 5
```

A count bigger than the list is fine: you get the whole list (or an empty
one). None of them stop the program.

### slice

```tessel
let letters = ["a", "b", "c", "d", "e"]
print(letters.slice(from: 1, to: 3).joined())   // bc
print(letters.slice(from: 3, to: 99).joined())  // de
```

The items from index `from` (included) up to `to` (not included), like
[`substring`](#substring) for strings. Indexes outside the list are clamped
to it, and if `to` isn't after `from`, the result is empty.

### unique

```tessel
let tags = ["red", "blue", "red", "green", "blue"]
print(tags.unique().joined(separator: ","))     // red,blue,green
```

Keeps the first of each group of equal items, in their order.

### randomElement and shuffled

```tessel
let names = ["Ada", "Grace", "Linus"]
let winner = names.randomElement() ?? "nobody"
let order = names.shuffled()
```

`randomElement` is `nil` for an empty list. See also
[Random numbers](#random-numbers).

### appendAll, removeFirst and removeLast

```tessel
var queue = ["a", "b"]
queue.appendAll(["c", "d"])
print(queue.removeFirst())              // a
print(queue.removeLast())               // d
print(queue.joined(separator: ","))     // b,c
```

`queue.appendAll(other)` does the same as `queue += other`. `removeFirst`
and `removeLast` need a list with at least one item; on an empty list they
stop the program with an error, like `remove(at:)`.

## Dictionary

A dictionary `[K: V]` maps keys to values. Write one as `["a": 1, "b": 2]`;
an empty one needs its type from context: `var ages: [String: Int] = [:]`.
Keys can be `Int`, `String`, `Bool`, an enum whose cases have no values, or
a tuple or struct made of those.

| Member | Returns | |
|---|---|---|
| `count` | `Int` | Number of entries. |
| `isEmpty` | `Bool` | Whether there are no entries. |
| `keys` | `[K]` | The keys. |
| `values` | `[V]` | The values. |
| `contains(key: K)` | `Bool` | Whether there is an entry for the key. |
| `removeValue(forKey: K)` | `V?` | Removes the entry and returns its value, or `nil` if there was none. Changes the dictionary. |

Reading and writing entries:

- `dict[key]` is the value for `key`, as an optional (`V?`): `nil` if there's
  no entry.
- `dict[key] = value` adds or replaces an entry; `dict[key] = nil` removes it.
- `==` and `!=` compare two dictionaries' entries, in any order.

```tessel
var ages = ["Ada": 36, "Linus": 21]
ages["Grace"] = 85
ages["Linus"] = 22
print(ages["Linus"] ?? 0)               // 22
print(ages["Bob"] ?? 0)                 // 0
print(ages.contains(key: "Ada"))        // true
print(ages.removeValue(forKey: "Ada") ?? 0)   // 36
ages["Grace"] = nil
print(ages.keys.joined(separator: ","))       // Linus
```

`keys` and `values` are in the order the entries were first added.
Replacing a value keeps its place; removing an entry and adding it again
moves it to the end.

A `for` loop over a dictionary gives each entry as a `(key, value)` tuple,
in order:

```tessel
let stock = ["apples": 3, "pears": 0]
for (fruit, count) in stock {
    print("{fruit}: {count}")
}
```

## zip

```text
zip(_ a: [A], _ b: [B]) -> [(A, B)]
```

Pairs up the items at the same positions of two lists, as long as the
shorter list:

```tessel
let names = ["Ada", "Alan"]
let scores = [90, 85, 70]
for (name, score) in zip(names, scores) {
    print("{name}: {score}")       // Ada: 90, then Alan: 85
}
```

## Set

A set `Set<T>` holds values without order and without repeats. Write one
as a list where a set is expected (`let seen: Set<Int> = [1, 2]`), make one
from a list with `Set(list)`, or an empty one with `Set<Int>()`. Items can
be the same types as dictionary keys. `for x in set` goes through the items
in the order they were first added. Two sets are `==` when they have the
same items. JSON writes a set as an array. See
[Collections](/language/collections/#sets).

| Member | Returns | |
|---|---|---|
| `count` | `Int` | Number of items. |
| `isEmpty` | `Bool` | Whether there are no items. |
| `values` | `[T]` | The items, in the order they were first added. |
| `contains(_ item: T)` | `Bool` | Whether the item is in the set. |
| `insert(_ item: T)` | | Adds the item (nothing happens if it's there). Changes the set. |
| `remove(_ item: T)` | | Removes the item (nothing happens if it isn't there). Changes the set. |
| `union(_ other: Set<T>)` | `Set<T>` | The items in either set. |
| `intersection(_ other: Set<T>)` | `Set<T>` | The items in both sets. |
| `subtracting(_ other: Set<T>)` | `Set<T>` | The items that aren't in `other`. |
| `isSubset(of: Set<T>)` | `Bool` | Whether every item is also in the other set. |
| `isSuperset(of: Set<T>)` | `Bool` | Whether every item of the other set is in this one. |
| `isDisjoint(with: Set<T>)` | `Bool` | Whether no item is in both. |
| `filter(_ keep: fn(T) -> Bool)` | `Set<T>` | The items for which `keep` is true. |
| `sorted(by: fn(T, T) -> Bool = …)` | `[T]` | The items as a sorted list; without `by:`, only for `Int`, `Float` and `String` items. |

## Tuples

A tuple groups 2 to 8 values: its type is written `(Int, String)`, a value
`(1, "one")`, and its items are `.0`, `.1` and so on. `let (a, b) = pair`
takes one apart. Tuples compare with `==`, can be dictionary keys and set
items when their items can, and JSON writes them as arrays. See
[Collections](/language/collections/#tuples).

## Reasons for failures

The functions in this section return a [`Result`](/language/errors/) or an
`Error?` instead of an optional, so a failure comes with a message that says
why. They're built in; you don't import anything.

### Result and Error

```text
struct Error { message: String }

enum Result<T> {
    ok(value: T)
    failure(error: Error)
}
```

`Error("reason")` makes an error; the label is optional. `Result` has these
helpers:

| Method | Returns |
| --- | --- |
| `value()` | `T?`: the value, or `nil` on failure |
| `error()` | `Error?`: the error, or `nil` on success |
| `isOk()` | `Bool` |
| `valueOr(_ fallback: T)` | the value, or `fallback` on failure |

`try` unwraps a `Result` (or checks an `Error?`) and passes a failure on to
the caller. See [Errors](/language/errors/).

### readText

```text
readText(_ path: String) -> Result<String>
```

Like [`readFile`](#readfile), but a failure says why, for example
`can't read notes.txt: there's no such file or folder`.

### writeText

```text
writeText(path: String, text: String) -> Error?
```

Like [`writeFile`](#writefile); `nil` means it worked.

### appendText

```text
appendText(path: String, text: String) -> Error?
```

Like [`appendFile`](#appendfile); `nil` means it worked.

### readBytes

```text
readBytes(_ path: String) -> Result<Data>
```

Like [`readData`](#readdata), with a reason on failure.

### writeBytes

```text
writeBytes(path: String, data: Data) -> Error?
```

Like [`writeData`](#writedata); `nil` means it worked.

### parseInt

```text
parseInt(_ text: String) -> Result<Int>
```

Parses a whole number, ignoring spaces around it. On failure the message
is `"abc" isn't a whole number`.

### parseFloat

```text
parseFloat(_ text: String) -> Result<Float>
```

Parses a number. On failure the message is `"abc" isn't a number`.

### decodeJson

```text
decodeJson<T>(_ text: String) -> Result<T>
```

Like [`fromJson`](#fromjson), with the type given by the result you assign
to. The reason is either `this isn't valid JSON: …` or `the JSON doesn't have
the shape of this type`.

```tessel
struct Point { x: Int, y: Int }

let r: Result<Point> = decodeJson("\{\"x\": 1}")
if let e = r.error() {
    print(e.message)    // the JSON doesn't have the shape of this type
}
```

### decodeBinary

```text
decodeBinary<T>(_ data: Data) -> Result<T>
```

Like [`fromBinary`](#frombinary), with a reason on failure.

## Runtime errors

A few mistakes can only be found while the program runs. They stop the
program with a message and the source location:

```text
error: index 5 is out of range for a list of 3 items
 --> main.tsl:4:11
```

| Error | Caused by |
|---|---|
| `index … is out of range` | `list[i]`, `remove(at:)` or `insert(at:)` with an index outside the list, `removeFirst()` or `removeLast()` on an empty list, or `data[i]` outside the data. |
| `integer overflow` | `Int` arithmetic whose result is too big for an `Int`. |
| `division by zero` | `Int` division or remainder (`/`, `%`) by `0`. |
| `negative shift amount` | `a << n` or `a >> n` with `n` less than `0`. |

`Float` arithmetic never stops the program: dividing by `0.0` gives an
infinite value (printed as `inf`).
