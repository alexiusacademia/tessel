---
title: Standard library
description: Every built-in Tessel function, and every property and method of String, lists and dictionaries.
---

This page lists everything built into Tessel that isn't a view or a
modifier: functions like `print` and `readFile`, conversions, and the
properties and methods of `String`, lists (`[T]`) and dictionaries
(`[K: V]`). The outputs shown in comments come from running the examples.

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
print(_ value: Int, Float, Bool, String, Range or an enum)
```

Writes a value to standard output, followed by a new line.

```tessel
print("Hello")        // Hello
print(42)             // 42
print(2.0)            // 2.0
print(true)           // true
print(Mode.grid)      // grid
print(0..3)           // 0..3
```

- A `Float` always shows a decimal point (`2.0`), so it reads differently
  from an `Int`.
- An enum value prints as its case name, and a range as `start..end`.
- Lists, dictionaries, structs and optionals can't be printed directly.
  Print a property instead (`list.count`), join a `[String]` with
  [`joined`](#joined), or give an optional a default with `??`.
- In an app, the output goes to the terminal the app was started from (or to
  the console of the [Tessel IDE](/start/ide/)).

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

## The environment and other programs

### environment

```text
environment(name: String) -> String?
```

The value of an environment variable, or `nil` if it isn't set.

```tessel
let home = environment("HOME") ?? "unknown"
```

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

## Background work and timers

The blocks of `fetch`, `after` and `runCommand` run later, on the UI thread in
an app. They work in a program with only `fn main()` too: after `main`
returns, the program keeps running until no timers or background work are
left, then exits. See
[Files, commands and timers](/ui/async/#callbacks-in-a-program-without-an-app).

### fetch

```text
fetch(url: String, done: fn(String?))
```

Loads a URL in the background, then calls `done` with the text, or `nil` if
loading failed. `done` runs on the UI thread, so it can change `state`
directly.

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
code finishes. To repeat, call `after` again from inside `action`.

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

Strings can't be indexed with `[…]`; use `substring`, `split` or `find`.

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

Other list operations:

- `list[i]` reads the item at index `i` (from 0), and `list[i] = value`
  replaces it. An index outside the list stops the program with an error.
- `a + b` is a new list with the items of `a` then `b`; `list += [x]`
  appends.
- `==` and `!=` compare two lists item by item.
- `for item in list { … }` loops over the items. See
  [Collections](/language/collections/).

Methods that change the list (`append`, `insert`, `remove`, `removeAll`)
need a list you can change: a `var`, a `state`, or a `bind` parameter, or a
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
| `index … is out of range` | `list[i]`, `remove(at:)` or `insert(at:)` with an index outside the list. |
| `integer overflow` | `Int` arithmetic whose result is too big for an `Int`. |
| `division by zero` | `Int` division or remainder (`/`, `%`) by `0`. |

`Float` arithmetic never stops the program: dividing by `0.0` gives an
infinite value (printed as `inf`).
