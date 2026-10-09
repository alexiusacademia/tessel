---
title: Programs and files
description: How a Tessel program is organized - a single .tsl file or a folder of files sharing one namespace, modules with import, public and private, and an app or fn main as its entry point.
---

A Tessel program is either **one `.tsl` file**, or **a folder of `.tsl`
files**. All the files in the folder form one program, with nothing to set
up. Code you want to keep apart, or share between programs, goes in a
[module](#modules): another folder, which the program `import`s.

## One file

The simplest program is a single file:

```tessel
// hello.tsl
fn main() {
    print("Hello!")
}
```

```sh
tessel run hello.tsl
```

## A folder of files

As a program grows, split it into several files in one folder:

```text
todo-app/
  main.tsl
  todo.tsl
```

```tessel
// todo.tsl
struct Todo {
    title: String
    done: Bool = false
}

fn describe(todo: Todo) -> String {
    let mark = if todo.done { "x" } else { " " }
    "[{mark}] {todo.title}"
}
```

```tessel
// main.tsl
fn main() {
    let todo = Todo(title: "Write docs")
    print(describe(todo: todo))
}
```

Pass the folder to `tessel`:

```sh
tessel run todo-app
```

```text
[ ] Write docs
```

`main.tsl` uses `Todo` and `describe` from `todo.tsl` without importing
anything. Every file in the folder sees every declaration in every other
file, and the order of the files and of the declarations in them doesn't
matter. The file names are up to you; `main.tsl` is just a convention.

Only files ending in `.tsl` directly inside the folder are part of the
program. Files in subfolders are not included, unless the program imports
the subfolder as a [module](#modules).

:::caution
If you run a single file from a multi-file program, like
`tessel run todo-app/main.tsl`, only that file is compiled, so names from the
other files will be missing. Run the folder instead.
:::

## How files work together

A small notes app shows how the pieces of a bigger program usually spread
over files, and how they reach each other:

```text
notes/
  model.tsl     the data: a struct, and functions on it
  storage.tsl   saving and loading it
  views.tsl     reusable parts of the window
  main.tsl      the app: its state, and the window
```

```tessel
// The data, and the functions that work on it.

struct Note {
    title: String
    body: String = ""
    pinned: Bool = false
}

fn sortedNotes(notes: [Note]) -> [Note] {
    notes.sorted(by: { a, b in a.pinned && !b.pinned })
}

fn summary(notes: [Note]) -> String {
    let pinned = notes.filter { n in n.pinned }.count
    "{notes.count} notes, {pinned} pinned"
}
```

```tessel
// Saving and loading notes as a JSON file.

fn notesFile() -> String {
    joinPath(appDataFolder("Notes"), "notes.json")
}

fn loadNotes() -> [Note] {
    let text = readFile(notesFile()) ?? "[]"
    let notes: [Note] = fromJson(text) ?? []
    notes
}

fn saveNotes(notes: [Note]) -> Bool {
    writeFile(path: notesFile(), text: toJson(notes, pretty: true))
}
```

```tessel
// Reusable pieces of the window.

view NoteRow(note: bind Note) {
    HStack(spacing: 8) {
        Toggle("", isOn: note.pinned)
        TextField("Title", text: note.title)
    }
}

view Footer(text: String) {
    Text(text).font(size: 12).color(.secondary)
}
```

```tessel
// The app: its state, and the window built from the other files' views.

app Notes(width: 420, height: 320) {
    state notes: [Note] = []
    state status = ""

    VStack(spacing: 10, alignment: .leading) {
        for note in notes {
            NoteRow(note: note)
        }
        HStack {
            Button("Add") { notes.append(Note(title: "New note")) }
            Button("Save") { save() }
        }
        Footer(text: "{summary(notes: notes)} {status}")
    }
    .padding(16)
    .onAppear { notes = loadNotes() }

    fn save() {
        notes = sortedNotes(notes: notes)
        status = if saveNotes(notes: notes) { "· saved" } else { "· couldn't save" }
    }
}
```

```sh
tessel run notes
```

Here's what each file uses from the others:

- `storage.tsl` uses the `Note` type from `model.tsl`: `[Note]` in its
  signatures, and `fromJson` builds `Note` values.
- `views.tsl` takes a `Note` as a [binding](/ui/state/#bindings-bind-parameters)
  (`note: bind Note`), so its toggle and text field change the note that
  belongs to the app.
- `main.tsl` puts it all together: it shows `NoteRow` and `Footer` from
  `views.tsl`, calls `summary` and `sortedNotes` from `model.tsl`, and
  `loadNotes` and `saveNotes` from `storage.tsl`.

None of this needs an import: a name declared in any file can be used in any
other.

### How data moves between files

There are no global variables in Tessel, so files don't share data that
changes by reaching into each other. (They can share
[constants](/language/basics/#constants), which never change.) Data moves
the same way it does inside one file:

- **Down, as values.** The app owns the data (in `state`) and passes it to
  functions and views as arguments: `summary(notes: notes)`,
  `Footer(text: …)`. The function or view gets a copy.
- **Back up, as results.** Functions return new values, and the caller
  stores them: `notes = sortedNotes(notes: notes)`.
- **Changed in place, with `bind`.** A view or function that changes the
  caller's data takes it as a `bind` parameter, like
  `NoteRow(note: bind Note)` or `fn archive(_ note: bind Note)`. In the app,
  the list's items are passed straight in: `NoteRow(note: note)` inside
  `for note in notes`.
- **Through the disk or the network.** `storage.tsl` saves and loads files;
  other files only call its functions.

So one file (usually `main.tsl`, with the `app`) owns the program's
changing data, and the other files provide types, functions and views that
work on what they're given. That keeps each file easy to understand on its
own.

### Everything is visible everywhere

- **Order doesn't matter.** A file can use a type or function declared
  further down, or in a file that comes later alphabetically.
- **Types are shared.** A struct, enum or [interface](/language/interfaces/)
  declared in one file can be used in the others, and a struct in one file can
  conform to an interface declared in another.
- **Methods travel with their type.** Methods are declared inside their
  struct or enum, in the same file; every file can call them.
- **Functions inside a view stay there.** A `fn` declared inside a `view` or
  the `app` belongs to it and can use its `state`; other files can't call it.
  Put functions that others need at the top level of a file.
- **Unless it's `private`.** A declaration marked
  [`private`](#private-declarations) can only be used in its own file.

### Organizing a program

- Group by what things are about: a file per feature or per kind of data
  (`invoice.tsl` for the `Invoice` struct, its functions and its views) works
  well; so does splitting into model, storage and views as above.
- Keep the `app` short: its state, the main layout, and the actions that
  change the state. Move the rest into views and functions in other files.
- Since names are shared, give top-level names that are clear across the
  whole program: `loadNotes` rather than `load`, `NoteRow` rather than `Row`.
- Each `.tsl` file directly inside the folder is part of the program. Keep
  experiments and old versions outside the folder (or rename them to another
  extension), or they'll be compiled too.
- When a part of the program grows large or is useful elsewhere, move it
  into a [module](#modules) with a small `public` surface.

The [Tessel IDE](/start/ide/) works on a folder in the same way: its
explorer shows the files, **Check** and **Run** use the whole folder, and
[code completion](/start/ide/#the-editor) suggests names from all the files.

## What goes at the top level

A file contains only declarations:

| Declaration | For |
|---|---|
| `fn` | a function, see [Functions](/language/functions/) |
| `let` | a constant, see [Constants](/language/basics/#constants) |
| `struct` | a struct type, see [Structs and enums](/language/structs-enums/) |
| `enum` | an enum type, see [Structs and enums](/language/structs-enums/) |
| `interface` | methods and properties that several types share, see [Interfaces](/language/interfaces/) |
| `view` | a piece of user interface, see [Apps and views](/ui/apps-views/) |
| `app` | the entry point of a program with a window |

Statements and variables live inside these. A `var` at the top level of a
file is an error: values that change belong to the app's `state`, or to a
function.

## Names are shared

Because all files share one namespace, each top-level name must be unique
across the whole program. Declaring the same name twice, even in different
files, is an error that points at both places:

```text
error: `describe` is declared more than once
  = help: names must be unique across all files in the program
```

Built-in names such as `Int`, `String`, `Text`, `Button` and `print` can't
be redeclared either.

Names inside a struct or enum (fields, cases and methods) belong to that
type, so two types can both have a method called `describe` without any
conflict.

## Private declarations

`private` before a top-level declaration, or before a field or method, means
only its own file can use it. It keeps helpers from being used by accident,
and says to the reader "this is a detail of this file":

```tessel
// account.tsl
struct Account {
    owner: String
    private balance: Int = 0

    fn deposit(_ amount: Int) {
        balance += checked(amount)
    }

    fn summary() -> String {
        "{owner}: {balance}"
    }
}

private fn checked(_ amount: Int) -> Int {
    max(amount, 0)
}
```

Another file can call `deposit` and `summary`, but using `balance` or
`checked` there is an error:

```text
error: field `balance` is private to the file that declares `Account`
```

A private name is still a top-level name of the program, so it must be
unique like any other.

## Modules

A **module** is a folder of `.tsl` files that other code imports. Use one
to keep a part of a program to itself (its data model, a set of shared
views), or to share code between programs.

```text
shapes-app/
  main.tsl
  geometry/
    shapes.tsl
    math.tsl
```

`import geometry` at the top of a file loads the folder `geometry` next to
that file. The module's declarations are then used by their full name,
`geometry.` and the name, so it's always clear where a name comes from:

```tessel
// main.tsl
import geometry

fn main() {
    let shapes: [geometry.Shape] = [geometry.Circle(radius: 1.0), geometry.Square(side: 2.0)]
    print(geometry.totalArea(shapes))
}
```

```text
7.141592653589793
```

### `public`: what a module shares

Only declarations marked `public` can be used outside the module. The rest
are the module's own details, which its files share with each other as
usual:

```tessel
// geometry/shapes.tsl
public interface Shape {
    fn area() -> Float
}

public struct Circle: Shape {
    radius: Float
    fn area() -> Float { pi * square(radius) }
}

public struct Square: Shape {
    side: Float
    fn area() -> Float { square(side) }
}
```

```tessel
// geometry/math.tsl
public fn totalArea(_ shapes: [Shape]) -> Float {
    var total = 0.0
    for s in shapes {
        total += s.area()
    }
    total
}

// Not public: only the module's files can use it.
fn square(_ x: Float) -> Float { x * x }
```

Inside the module, names are written without `geometry.` (`Shape`,
`square`). The fields and methods of a public struct or enum can be used
wherever the type can, except the ones marked `private`.

Using something that isn't public says so:

```text
error: `square` isn't public in module `geometry`
  = help: to use it outside the module, add `public` before its declaration in math.tsl
```

### What a module sees

A module sees its own declarations, the built-in ones, and the modules it
imports itself. It doesn't see the program that imports it, which keeps
it independent, so the same module works in any program.

Everything from a module keeps its full name: its types in type
annotations (`[geometry.Shape]`, `geometry.Stack<Int>`), and interfaces
after `:` (`struct Hexagon: geometry.Shape`). Enum cases work as usual:
`geometry.Kind.round`, or `.round` where the type is known. A module can
declare a name the program also uses, like `square` above, without any
conflict.

Each file imports what it uses. A module is loaded once, however many files
import it.

### Sharing a module between programs

`import` also takes a path to a folder, relative to the importing file. The
module's name is the folder's name:

```tessel
import "../shared/textkit"

fn main() {
    print(textkit.shout("hello"))
}
```

This way several programs can use one copy of the code. (There's no
package manager yet: the folder is found on disk.)

A module can't have an `app`: that belongs to the program. Put the views
the program needs in the module, mark them `public`, and show them from the
program's `app`.

## The entry point

Every program has exactly one place where it starts: either an `app` or a
`fn main()`.

- **`fn main()`** is for programs without a window, such as command-line
  tools, scripts and tests. It runs from top to bottom, and the program ends
  when it returns. `main` takes no parameters and returns nothing.
- **`app`** is for programs with a window. It declares the main window and
  what it shows, and the program keeps running until the window is closed:

```tessel
app Hello(title: "Hello", width: 320, height: 200) {
    Text("Hello from Tessel")
        .padding(24)
}
```

A program can't have both. With neither, `tessel check` reports
`this program has no entry point`. Building apps is covered in
[Your first app](/start/first-app/) and [Apps and views](/ui/apps-views/).

## Checking, running and building

The `tessel` command takes a file or a folder:

```sh
tessel check todo-app          # find errors without building
tessel run todo-app            # build and run
tessel build todo-app -o todo  # build a native executable called todo
```

`tessel check` type-checks the whole program and lists every error with its
file, line and column. `tessel run` compiles the program to native code and
starts it. When the program ends normally, `tessel run` exits with status 0;
after a [runtime error](/language/optionals/#runtime-errors), it exits with
status 101.

See [Command line](/start/cli/) for all the options.
