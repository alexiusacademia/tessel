---
title: Programs and files
description: How a Tessel program is organized - a single .tsl file or a folder of files sharing one namespace, with an app or fn main as its entry point.
---

A Tessel program is either **one `.tsl` file**, or **a folder of `.tsl`
files**. There are no import statements, packages or modules to set up: all
the files in the folder form one program.

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
program. Files in subfolders are not included.

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

There are no global variables in Tessel, so files don't share data by
reaching into each other. Data moves the same way it does inside one file:

- **Down, as values.** The app owns the data (in `state`) and passes it to
  functions and views as arguments: `summary(notes: notes)`,
  `Footer(text: …)`. The function or view gets a copy.
- **Back up, as results.** Functions return new values, and the caller
  stores them: `notes = sortedNotes(notes: notes)`.
- **Changed in place, with `bind`.** A view that changes the caller's data
  takes it as a `bind` parameter, like `NoteRow(note: bind Note)`. In the app,
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

The [Tessel IDE](/start/ide/) works on a folder in the same way: its
explorer shows the files, **Check** and **Run** use the whole folder, and
[code completion](/start/ide/#the-editor) suggests names from all the files.

## What goes at the top level

A file contains only declarations:

| Declaration | For |
|---|---|
| `fn` | a function, see [Functions](/language/functions/) |
| `struct` | a struct type, see [Structs and enums](/language/structs-enums/) |
| `enum` | an enum type, see [Structs and enums](/language/structs-enums/) |
| `interface` | methods and properties that several types share, see [Interfaces](/language/interfaces/) |
| `view` | a piece of user interface, see [Apps and views](/ui/apps-views/) |
| `app` | the entry point of a program with a window |

Statements and variables live inside these. A `let` or `var` at the top
level of a file is an error; for a value the whole program needs, write a
function that returns it.

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
