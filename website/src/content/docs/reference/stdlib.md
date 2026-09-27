---
title: Standard library
description: Every built-in Tessel function (math, random numbers, files, JSON and binary encoding, dates, the network, timers and background work) and every property and method of Int, Float, String, Data, lists and dictionaries.
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
- Lists, dictionaries, structs and optionals can't be printed directly.
  Print a property instead (`list.count`), join a `[String]` with
  [`joined`](#joined), or give an optional a default with `??`.
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

- `data[i]` is the byte at index `i` (from 0), as an `Int`. An index outside
  the data stops the program with an error. `Data` can't be changed in place;
  make a new one instead.
- `==` and `!=` compare the bytes.
- Read and write files of bytes with [`readData`](#readdata) and
  [`writeData`](#writedata).

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

Dates are shown and read in the computer's time zone.

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
     hour: Int = 0, minute: Int = 0, second: Int = 0) -> Float
```

The time of a calendar date, in the computer's time zone. Months and days
count from 1. A date that doesn't exist, like 30 February, gives `NaN` (see
[`isNaN`](#float)).

```tessel
let t = date(year: 2026, month: 3, day: 7, hour: 9, minute: 5)
print(formatDate(t))            // 2026-03-07 09:05
```

### formatDate

```text
formatDate(_ time: Float, format: String = "yyyy-MM-dd HH:mm") -> String
```

A time as text. `format` is a pattern where these letters are replaced;
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

### parseDate

```text
parseDate(_ text: String, format: String = "yyyy-MM-dd") -> Float?
```

Reads a time from text written in `format` (the same patterns as
[`formatDate`](#formatdate)), or `nil` if the text doesn't match. Without an
hour, the time is midnight.

```tessel
let day = parseDate("2026-03-07")
let meeting = parseDate("07/03/2026 09:05", format: "dd/MM/yyyy HH:mm")
print(parseDate("tomorrow") == nil)     // true
```

The whole text must match: `parseDate("2026-03-07 09:05")` is `nil`, because
the default format has no time.

## The system and other programs

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

The system the program is running on: `"macos"` or `"windows"`.

```tessel
let shortcut = if platform() == "macos" { "Cmd+S" } else { "Ctrl+S" }
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
runCommand(path: String, args: [String] = [],
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
- The program gets no input.
- Both blocks run on the UI thread, so they can change `state` directly.

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
        body: String = "", done: fn(HttpResponse))
```

Sends an HTTP request in the background, then calls `done` with the
response.

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
download(url: String, to: String, progress: fn(Float)? = nil, done: fn(Bool))
```

Downloads a URL straight into a file, which suits large files. `progress`,
if given, is called now and then with how much has arrived, from `0.0` to
`1.0`. `done` is called at the end with whether it worked.

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

### openWebSocket

```text
openWebSocket(url: String, onMessage: fn(String), onClose: fn(String)) -> Int
```

Connects to a WebSocket server (`ws://` or `wss://`) and returns an id for
[`sendWebSocket`](#sendwebsocket) and [`closeWebSocket`](#closewebsocket).
The connection stays open, and messages can go both ways:

- `onMessage` is called with each message the server sends.
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

Messages are text. A binary message from the server arrives as text too.

### sendWebSocket

```text
sendWebSocket(_ id: Int, text: String) -> Bool
```

Sends a text message. Returns `false` if the WebSocket isn't open (it was
closed, or `id` is wrong). Messages sent before the connection is ready are
sent once it is.

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
background(work: fn() -> T, done: fn(T))
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

## Int

An `Int` is a whole number. The arithmetic operators work on it, and
[`abs`, `min` and `max`](#math) take `Int`s too.

| Member | Returns | |
|---|---|---|
| `isEven` | `Bool` | Whether the number is even. |
| `isOdd` | `Bool` | Whether the number is odd. |
| `clamped(min: Int, max: Int)` | `Int` | The number, moved into the range from `min` to `max`. |

```tessel
print((4).isEven)                       // true
print((-3).isOdd)                       // true
print((15).clamped(min: 0, max: 10))    // 10
let row = 7
let shade = if row.isEven { Color.lightGray } else { Color.white }
```

Write a number literal in parentheses to use a member on it: `(4).isEven`.

## Float

A `Float` is a decimal number. The arithmetic operators work on it, and the
[math functions](#math) take and return `Float`s.

| Member | Returns | |
|---|---|---|
| `formatted(decimals: Int)` | `String` | The number as text, with exactly `decimals` digits after the point. |
| `clamped(min: Float, max: Float)` | `Float` | The number, moved into the range from `min` to `max`. |
| `isNaN` | `Bool` | Whether the value is `NaN`, "not a number" (like `0.0 / 0.0` or `sqrt(-1.0)`). |
| `isFinite` | `Bool` | Whether the value is an ordinary number: not infinite and not `NaN`. |

```tessel
let volume = 1.4
print(volume.clamped(min: 0.0, max: 1.0))   // 1.0
print(sqrt(-1.0).isNaN)                     // true
print((1.0 / 0.0).isFinite)                 // false
```

`NaN` is never equal to anything, not even itself, so `x == x` is `false`
for a `NaN`: use `isNaN` to check for it.

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
| `appendAll(_ items: [T])` | | Adds all of `items` at the end. Changes the list. |
| `removeFirst()` | `T` | Removes and returns the first item. Changes the list. |
| `removeLast()` | `T` | Removes and returns the last item. Changes the list. |

Other list operations:

- `list[i]` reads the item at index `i` (from 0), and `list[i] = value`
  replaces it. An index outside the list stops the program with an error.
- `a + b` is a new list with the items of `a` then `b`; `list += [x]`
  appends.
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
Keys can be `Int`, `String`, `Bool`, or an enum whose cases have no values.

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

A dictionary can't be looped over directly; loop over `keys` or `values`:

```tessel
let stock = ["apples": 3, "pears": 0]
for fruit in stock.keys {
    print("{fruit}: {stock[fruit] ?? 0}")
}
```

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

`Float` arithmetic never stops the program: dividing by `0.0` gives an
infinite value (printed as `inf`).
