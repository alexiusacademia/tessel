---
title: 16. Files and saving data
description: Read and write files, keep your program's data between runs as JSON, and split a bigger program into several files.
---

Every program you've written so far forgets everything when it ends. Run it
again, and it starts from scratch. Real programs remember things: your
settings, your notes, your best score. They do that by saving data in
**files**.

In this lesson you'll learn:

- what files and folders are, and how a program finds them
- how to read, write and add to a text file
- how to check whether a file exists, and build paths with `joinPath`
- where a program should keep its own files, with `appDataFolder`
- how to save your structs as JSON and load them back
- how to split a program into several files in a folder

At the end, you'll build a high-score table that remembers every score
between runs.

## Files and folders

A **file** is a named piece of data on the disk: a photo, a song, a text
document. Unlike the variables in your program, a file stays there after the
program ends, and after the computer is switched off.

Files live in **folders** (also called directories), and folders can hold
other folders. A **path** is the address of a file: the folders you go
through to reach it, separated by `/`:

```text
/Users/ada/Documents/notes.txt
```

That's a *full* path: it starts at the top of the disk. A path like
`notes.txt` or `notes/today.txt`, which doesn't start with `/`, is a
*relative* path: it's relative to the **current folder**, the folder you were
in when you started the program. When you type `tessel run hello.tsl` in a
terminal, the current folder is the one the terminal is in.

In Tessel, paths are ordinary strings.

## Writing and reading a file

`writeFile` saves text into a file, and `readFile` reads it back:

```tessel
fn main() {
    let ok = writeFile(path: "hello.txt", text: "Hello, file!\n")
    print("saved: {ok}")
    if let text = readFile("hello.txt") {
        print("the file says: {text.trim()}")
    }
}
```

```text
saved: true
the file says: Hello, file!
```

After you run this, there's a new file called `hello.txt` next to your
program. Open it in a text editor: it holds exactly the text you wrote.

A few things to notice:

- `writeFile` **replaces** the file if it already exists. The old contents
  are gone.
- `writeFile` returns a `Bool`: `true` if it worked. Saving can fail (the
  disk is full, the folder doesn't exist, you're not allowed to write
  there), so it tells you.
- `readFile` returns a `String?`, an optional, like `readLine()` does. It
  gives `nil` when the file doesn't exist or can't be read. That's why the
  example uses `if let`.
- The text ends with `"\n"`, a line break, because text files usually end
  with one. `trim()` removes it before printing, so there's no empty line.

Why an optional? Because a missing file is *normal*. The first time a program
runs, it hasn't saved anything yet. Tessel makes you decide what to do in
that case:

```tessel
fn main() {
    if let text = readFile("missing.txt") {
        print(text)
    } else {
        print("no such file")
    }
    let notes = readFile("missing.txt") ?? ""
    print("{notes.count} characters")
}
```

```text
no such file
0 characters
```

`?? ""` is often the simplest choice: "the file's text, or nothing if there
isn't one yet".

### Finding out why: `readText`

`readFile` tells you *that* reading failed, but not *why*. The file might not
exist, or you might not have permission to read it, or it might be a folder.
When the reason matters (to show the user, say), use `readText` instead. It
returns a `Result<String>`, the built-in enum from
[lesson 15](/learn/15-generics/#generic-enums-a-clean-way-to-report-errors):

```tessel
fn main() {
    match readText("missing.txt") {
        .ok(text) -> print(text)
        .failure(e) -> print(e.message)
    }
}
```

```text
can't read missing.txt: there's no such file or folder
```

And inside a function that returns a `Result`, `try` passes the reason on to
the caller:

```tessel
fn wordCount(path: String) -> Result<Int> {
    let text = try readText(path)
    Result.ok(value: text.words().count)
}

fn main() {
    writeFile(path: "note.txt", text: "three short words")
    for path in ["note.txt", "missing.txt"] {
        match wordCount(path: path) {
            .ok(n) -> print("{path}: {n} words")
            .failure(e) -> print(e.message)
        }
    }
}
```

```text
note.txt: 3 words
can't read missing.txt: there's no such file or folder
```

`writeText` and `appendText` work the same way for writing. Since they have
no value to give back, they return an `Error?`: `nil` if it worked, or the
reason it didn't. `try writeText(path: p, text: t)` passes that on too.

### Adding to a file

To add text at the end of a file instead of replacing it, use `appendFile`.
If the file doesn't exist yet, it's created. This is handy for a log, a
diary, or anything that grows line by line:

```tessel
fn main() {
    writeFile(path: "log.txt", text: "Log started\n")
    appendFile(path: "log.txt", text: "Opened the door\n")
    appendFile(path: "log.txt", text: "Closed the door\n")
    print((readFile("log.txt") ?? "").trim())
}
```

```text
Log started
Opened the door
Closed the door
```

Each line ends with `\n`. Without it, the three pieces of text would run
together on one line.

To go through a file line by line, read it and split it with `lines()`,
which you met in [lesson 8](/learn/08-text/):

```tessel
for line in (readFile("log.txt") ?? "").lines() {
    print("> {line}")
}
```

```text
> Log started
> Opened the door
> Closed the door
```

### Reading part of a file

Pictures, sounds and videos aren't text. `writeData` and `readData` work
with their raw bytes, as `Data`. A big file, like a film, can be larger than
your computer's memory, so `readData` can also read just a part: `count`
bytes, starting at byte `from`. Bytes are counted from 0:

```tessel
fn main() {
    writeData(path: "letters.bin", data: Data(text: "abcdefghij"))
    print(readData("letters.bin", from: 0, count: 3)?.text ?? "")
    print(readData("letters.bin", from: 7, count: 100)?.text ?? "")
    print(fileSize("letters.bin") ?? 0)
}
```

```text
abc
hij
10
```

The second read asks for more than is there, so it gives the last three
bytes. `fileSize` tells you where the file ends, so you can read it a piece
at a time.

## Folders, and checking what's there

`fileExists` tells you whether a file (or folder) exists, `createFolder`
makes a folder, and `listFolder` gives the names of what's inside one.

To build a path from a folder and a name, use `joinPath`. You could glue
strings together with `"{folder}/{name}"`, but `joinPath` gets the details
right: it doesn't add a second `/` when the folder already ends in one, and
on Windows it uses `\`, which is what Windows expects.

```tessel
fn main() {
    createFolder("notes")
    let path = joinPath("notes", "today.txt")
    print(path)
    print(writeFile(path: path, text: "Buy bread\n"))
    print(fileExists(path))
    print(fileExists("notes/tomorrow.txt"))
    print(isFolder("notes"))
    for name in listFolder("notes") ?? [] {
        print("- {name}")
    }
}
```

```text
notes/today.txt
true
true
false
true
- today.txt
```

`createFolder` is needed because `writeFile` doesn't create folders for you.
Without the folder, the write fails, and `writeFile` returns `false`:

```tessel
fn main() {
    let ok = writeFile(path: "drafts/today.txt", text: "Buy bread\n")
    print("saved: {ok}")
}
```

```text
saved: false
```

There are more file functions (`deleteFile`, `renameFile`, `copyFile`,
`fileSize` and others). They're all listed under
[Files in the reference](/reference/stdlib/#files).

## Where to keep your program's files

Saving `scores.json` in the current folder works, but it has a problem:
the current folder changes. Start your program from another folder, and it
can't find its file any more, and it leaves files lying around wherever it
was run.

Programs usually keep their own data in a special folder for that purpose.
`appDataFolder` gives you one, named after your program. It creates the
folder if it doesn't exist yet:

```tessel
fn main() {
    let folder = appDataFolder("HighScores")
    print(folder)
    print(isFolder(folder))
}
```

On a Mac this prints something like:

```text
/Users/ada/Library/Application Support/HighScores
true
```

(with your own user name instead of `ada`). The file you'd save there is
then `joinPath(appDataFolder("HighScores"), "scores.json")`.

Use a name that belongs to your program, so different programs don't mix up
each other's files.

### Files that come with your program

Some files aren't the user's data but part of the program: a word list, a
help page, a sound. Keep them next to the program, and find them with
`appFolder()`, the folder the program is in (its source folder, under
`tessel run`):

```tessel
fn main() {
    let words = readText(joinPath(appFolder(), "words.txt")).value() ?? ""
    print("{words.lines().count} words")
}
```

`appPath()` is the program file itself. Unlike the current folder, these
are the same wherever the program is started from, and wherever it's
installed.

## Saving structs as JSON

Saving plain text is easy. But your data is usually more than text: a list
of structs, say. How do you turn a `[Pet]` into text, and back?

You could write the fields one per line and read them back yourself, but
that's a lot of fiddly code for every type. Instead, Tessel can convert
your values to **JSON**, a standard text format for data that almost every
programming language can read. `toJson` makes the text:

```tessel
struct Pet {
    name: String
    kind: String
    age: Int
}

fn main() {
    let pets = [
        Pet(name: "Rex", kind: "dog", age: 3),
        Pet(name: "Tom", kind: "cat", age: 5),
    ]
    print(toJson(pets[0]))
    let text = toJson(pets, pretty: true)
    print(text)
    writeFile(path: "pets.json", text: text)
}
```

```text
{"name":"Rex","kind":"dog","age":3}
[
  {
    "name": "Rex",
    "kind": "dog",
    "age": 3
  },
  {
    "name": "Tom",
    "kind": "cat",
    "age": 5
  }
]
```

A struct becomes an *object*, written in `{ }`, with each field's name and
value. A list becomes an *array*, written in `[ ]`. With `pretty: true`,
the text is spread over lines and indented, so it's easy for people to read
too.

`fromJson` goes the other way. It can't know by itself what the text is
supposed to be (a pet? a list of numbers?), so you tell it by giving the
result a type. Because the text might not match that type, it returns an
optional:

```tessel
struct Pet {
    name: String
    kind: String
    age: Int
}

fn main() {
    let text = readFile("pets.json") ?? "[]"
    let pets: [Pet] = fromJson(text) ?? []
    for pet in pets {
        print("{pet.name} the {pet.kind} is {pet.age}")
    }
}
```

```text
Rex the dog is 3
Tom the cat is 5
```

This is a second program: it knows nothing about the first one except the
file it left behind. That's what saving data is for.

The pattern `readFile(…) ?? "[]"` then `fromJson(text) ?? []` means: if
there's no file yet, or it can't be read as a list of pets, start with an
empty list. Your program works the very first time, with no special case.

`fromJson` gives `nil` when the text isn't what you expected, without saying
why. Its partner `decodeJson` returns a `Result` instead, with a reason:

```tessel
struct Pet {
    name: String
    age: Int
}

fn main() {
    let texts = ["[\{\"name\": \"Rex\", \"age\": 3}]", "[\{\"name\": \"Rex\"}]", "[\{\"name\": \"Rex\""]
    for text in texts {
        let result: Result<[Pet]> = decodeJson(text)
        match result {
            .ok(pets) -> print("{pets.count} pet(s)")
            .failure(e) -> print(e.message)
        }
    }
}
```

```text
1 pet(s)
the JSON doesn't have the shape of this type
this isn't valid JSON: EOF while parsing an object at line 1 column 15
```

The second text is valid JSON, but its pet has no `age`. The third isn't
complete JSON at all. (Here the `Result<[Pet]>` type on `result` tells
`decodeJson` what to read, just as with `fromJson`.)

If the text doesn't fit the type, for example a number where the struct
expects text, `fromJson` gives `nil`. Note the `\{` in these strings: a
plain `{` in a string starts [interpolation](/learn/08-text/), so a literal
brace is written `\{`.

```tessel
struct Pet {
    name: String
    age: Int
}

fn main() {
    let good: Pet? = fromJson("\{\"name\": \"Rex\", \"age\": 3}")
    let bad: Pet? = fromJson("\{\"name\": \"Rex\", \"age\": \"three\"}")
    print(good == nil)
    print(bad == nil)
}
```

```text
false
true
```

JSON handles numbers, `Bool`, `String`, lists, dictionaries, optionals,
enums, and structs made of those, nested as deeply as you like. The
[Saving data as JSON](/guides/json/) guide has the details, including what
happens when you add a field to a struct later.

:::tip[Smaller files]
JSON is text, which makes it easy to read and to share. When size or speed
matter more, `toBinary` and `fromBinary` save the same values as compact
bytes instead. See [Binary instead of JSON](/guides/json/#binary-instead-of-json).
:::

## Splitting a program into files

As programs grow, one long file gets hard to find your way around. You can
split a program into several `.tsl` files in one **folder**:

```text
highscores/
  score.tsl      the Score struct, and functions on scores
  storage.tsl    saving and loading
  main.tsl       fn main
```

Then run the whole folder instead of one file:

```sh
tessel run highscores
```

All the `.tsl` files in the folder are one program. There's nothing to
import: every file can use every struct and function from every other file,
in any order. The file names are up to you; `main.tsl` is just a common name
for the file with `fn main`.

Because the files share one set of names, each struct and function name can
only be used once in the whole folder. The
[Programs and files](/language/programs/) page tells you more.

## Worked example: a high-score table

Now let's put it all together. This program asks for your name and your
points, adds them to a table of the five best scores, saves the table, and
prints it. Next time you run it, the old scores are still there.

The first file describes a score, and what you can do with a list of them:

```tessel
// score.tsl: a score, and functions that work on lists of scores.

struct Score {
    name: String
    points: Int
}

fn bestFirst(_ scores: [Score]) -> [Score] {
    scores.sorted(by: { a, b in a.points > b.points })
}

fn printTable(_ scores: [Score]) {
    print("--- High scores ---")
    var place = 1
    for score in scores {
        let name = score.name.padEnd(10)
        print("{place}. {name} {score.points}")
        place += 1
    }
}
```

`padEnd(10)` adds spaces after the name until it's 10 characters long, so
the points line up in a column.

The second file does the saving and loading. It's the only part of the
program that knows scores are stored in a JSON file:

```tessel
// storage.tsl: saving and loading the table as JSON.

fn scoresFile() -> String {
    joinPath(appDataFolder("HighScores"), "scores.json")
}

fn loadScores() -> [Score] {
    let text = readFile(scoresFile()) ?? "[]"
    let scores: [Score] = fromJson(text) ?? []
    scores
}

fn saveScores(_ scores: [Score]) -> Bool {
    writeFile(path: scoresFile(), text: toJson(scores, pretty: true))
}
```

The last file is the program itself:

```tessel
// main.tsl

fn main() {
    var scores = loadScores()

    print("Your name?")
    let name = (readLine() ?? "").trim()
    print("Your points?")
    let points = Int((readLine() ?? "").trim()) ?? 0

    if name.isEmpty {
        print("No name, no score!")
    } else {
        scores.append(Score(name: name, points: points))
        scores = bestFirst(scores).prefix(5)
        if !saveScores(scores) {
            print("Couldn't save the scores.")
        }
    }
    printTable(scores)
}
```

`prefix(5)` keeps only the first five scores, so the table never grows
beyond the top five.

Run it three times. Here the answers are piped in with `printf` (each `\n`
is an Enter), so you can see all three runs at once. Piped answers don't
appear on the screen the way typed ones do:

```sh
printf 'Ada\n120\n' | tessel run highscores
printf 'Linus\n300\n' | tessel run highscores
printf 'Grace\n250\n' | tessel run highscores
```

```text
Your name?
Your points?
--- High scores ---
1. Ada        120
```

```text
Your name?
Your points?
--- High scores ---
1. Linus      300
2. Ada        120
```

```text
Your name?
Your points?
--- High scores ---
1. Linus      300
2. Grace      250
3. Ada        120
```

Each run started fresh, yet the table kept growing: the scores were in the
file. Here's what `scores.json` holds after the third run:

```text
[
  {
    "name": "Linus",
    "points": 300
  },
  {
    "name": "Grace",
    "points": 250
  },
  {
    "name": "Ada",
    "points": 120
  }
]
```

Notice how the files split the work. `main.tsl` doesn't know how scores are
stored; it just calls `loadScores()` and `saveScores(…)`. If you later
decide to store them differently, only `storage.tsl` changes.

## Common mistakes

### Using the file's text without unwrapping it

`readFile` returns an
optional, so you can't use its result as a `String` straight away:

```tessel
fn main() {
    let text = readFile("hello.txt")
    print(text.count)
}
```

```text
error: this might be `nil`
 --> main.tsl:3:11
  |
3 |     print(text.count)
  |           ^^^^ optional value
  |
  = help: use `?.count` to use it only when there is a value, or unwrap it with `if let`
```

Unwrap it with `if let`, or give a default with `??`:
`let text = readFile("hello.txt") ?? ""`.

### Not telling `fromJson` what to read

Without a type, Tessel can't tell
what the JSON should become:

```tessel
let pets = fromJson(readFile("pets.json") ?? "[]")
```

```text
error: can't tell what type `fromJson` should read
 --> main.tsl:8:16
  |
8 |     let pets = fromJson(readFile("pets.json") ?? "[]")
  |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = help: give the result a type, like `let project: Project? = fromJson(…)`
```

Write `let pets: [Pet] = fromJson(…) ?? []`.

### Leaving out the labels

`writeFile` has two parameters, and both need
their labels:

```tessel
writeFile("hello.txt", "Hi")
```

```text
error: this argument needs its label `path:`
 --> main.tsl:2:15
  |
2 |     writeFile("hello.txt", "Hi")
  |               ^^^^^^^^^^^
  |
  = help: write `path: …`

error: this argument needs its label `text:`
 --> main.tsl:2:28
  |
2 |     writeFile("hello.txt", "Hi")
  |                            ^^^^
  |
  = help: write `text: …`
```

(`readFile("hello.txt")` is fine without a label: a function with only one
parameter lets you leave it out.)

### Running one file of a folder program

If you run only `main.tsl`, the
other files aren't part of the program, so their names are missing:

```sh
tessel run highscores/main.tsl
```

```text
error: cannot find `loadScores`
 --> highscores/main.tsl:2:18
  |
2 |     var scores = loadScores()
  |                  ^^^^^^^^^^ not found
```

Run the folder instead: `tessel run highscores`.

## Exercises

**1. A shopping list.** Write a program that saves three items (one per
line) to `shopping.txt`, reads the file back, and prints how many items
there are, then each item with a `-` in front.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    writeFile(path: "shopping.txt", text: "eggs\nflour\nmilk\n")
    let text = readFile("shopping.txt") ?? ""
    let items = text.lines()
    print("{items.count} things to buy:")
    for item in items {
        print("- {item}")
    }
}
```

```text
3 things to buy:
- eggs
- flour
- milk
```

</details>

**2. Counting runs.** Write a program that remembers how many times it has
been run. Keep the number in `count.txt`. The first run prints
`This is your first run!`, later ones `You've run this program 2 times.`
and so on.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let path = "count.txt"
    let text = readFile(path) ?? "0"
    let count = (Int(text.trim()) ?? 0) + 1
    writeFile(path: path, text: "{count}")
    if count == 1 {
        print("This is your first run!")
    } else {
        print("You've run this program {count} times.")
    }
}
```

Run three times:

```text
This is your first run!
You've run this program 2 times.
You've run this program 3 times.
```

If there's no file yet, `readFile` gives `nil`, `?? "0"` turns that into
`"0"`, and the count starts at 1.

</details>

**3. A journal.** Write a program that reads lines with `readLine()` until
the input ends, adds each non-empty line to `journal.txt` with
`appendFile`, and then prints how many entries the journal has in total.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let path = "journal.txt"
    while let line = readLine() {
        if !line.trim().isEmpty {
            appendFile(path: path, text: "{line.trim()}\n")
        }
    }
    let all = readFile(path) ?? ""
    print("The journal has {all.lines().count} entries.")
}
```

```sh
printf 'Went for a walk\nRead a book\n' | tessel run journal.tsl
printf 'Learned about files\n' | tessel run journal.tsl
```

```text
The journal has 2 entries.
The journal has 3 entries.
```

</details>

**4. Settings with defaults.** Make a struct `Settings` with
`name: String = "friend"` and `runs: Int = 0`. Load it from
`settings.json` (or use `Settings()` if there's nothing to load), add one
to `runs`, save it as pretty JSON, and print
`Hello, friend! This is run number 1.` Then edit the name in the JSON file
by hand and run the program again.

<details>
<summary>Solution</summary>

```tessel
struct Settings {
    name: String = "friend"
    runs: Int = 0
}

fn loadSettings(_ path: String) -> Settings {
    if let text = readFile(path) {
        if let settings: Settings = fromJson(text) {
            return settings
        }
    }
    Settings()
}

fn main() {
    let path = "settings.json"
    var settings = loadSettings(path)
    settings.runs += 1
    writeFile(path: path, text: toJson(settings, pretty: true))
    print("Hello, {settings.name}! This is run number {settings.runs}.")
}
```

```text
Hello, friend! This is run number 1.
Hello, friend! This is run number 2.
```

After the second run, `settings.json` holds:

```text
{
  "name": "friend",
  "runs": 2
}
```

Change `"friend"` to your own name, and the next run greets you.

</details>

**5. One line per player.** In the high-score table, the same player can
appear several times. Change `main.tsl` so each name appears only once: if
the name is already in the table, keep whichever score is higher. (Hint:
`firstIndex(where:)` finds the position of the first item that matches.)

<details>
<summary>Solution</summary>

Only `main.tsl` changes:

```tessel
fn main() {
    var scores = loadScores()

    print("Your name?")
    let name = (readLine() ?? "").trim()
    print("Your points?")
    let points = Int((readLine() ?? "").trim()) ?? 0

    if name.isEmpty {
        print("No name, no score!")
    } else {
        if let i = scores.firstIndex(where: { s in s.name == name }) {
            if points > scores[i].points {
                scores[i].points = points
            }
        } else {
            scores.append(Score(name: name, points: points))
        }
        scores = bestFirst(scores).prefix(5)
        if !saveScores(scores) {
            print("Couldn't save the scores.")
        }
    }
    printTable(scores)
}
```

Ada scores 120, then 90, then 200. After the third run:

```text
Your name?
Your points?
--- High scores ---
1. Ada        200
```

</details>

## Summary

- Files keep data after your program ends. A path like `notes/today.txt` is
  relative to the current folder; one starting with `/` is a full path.
- `writeFile(path:text:)` replaces a file, `appendFile(path:text:)` adds to
  its end, and both return whether they worked.
- `readText(path)` returns a `Result<String>` with the reason if it fails;
  `decodeJson` does the same for JSON.
- `readFile(path)` returns a `String?`: `nil` if there's no file. Use
  `if let` or `??` for the first run.
- `fileExists`, `createFolder` and `listFolder` look after folders;
  `joinPath` builds paths correctly on every system.
- `appDataFolder("Name")` is the right place for a program's own files.
- `toJson(value, pretty: true)` turns your structs and lists into text;
  `let x: Type? = fromJson(text)` reads them back.
- A folder of `.tsl` files is one program: run it with
  `tessel run folder`. Every file sees every name, and each name must be
  unique.

**Next:** [17. Algorithms](/learn/17-algorithms/)
