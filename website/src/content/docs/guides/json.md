---
title: Saving data as JSON
description: Save your app's structs, lists and enums to a file as JSON or compact binary, and load them back, with toJson, fromJson, toBinary and fromBinary.
---

Most apps need to remember something between runs: a list of tasks, the
user's settings, a document. Tessel can turn your own structs and enums into
[JSON](https://www.json.org) text and back, with no extra code: you don't
write any conversion for each type. This guide shows how to save data, load
it again, and keep old files working as your app grows.

## From a value to text and back

Start with the types your app already has:

```tessel
enum Priority { low, normal, high }

struct Task {
    title: String
    done: Bool = false
    priority: Priority = Priority.normal
}

struct TaskList {
    name: String
    tasks: [Task] = []
}
```

`toJson` turns any such value into text. With `pretty: true`, the text is
indented, which makes it easier to read (and to look at in a text editor):

```tessel
var list = TaskList(name: "Home")
list.tasks.append(Task(title: "Water the plants"))
list.tasks.append(Task(title: "Pay the rent", priority: Priority.high))
list.tasks[0].done = true

print(toJson(list, pretty: true))
```

```text
{
  "name": "Home",
  "tasks": [
    {
      "title": "Water the plants",
      "done": true,
      "priority": "normal"
    },
    {
      "title": "Pay the rent",
      "done": false,
      "priority": "high"
    }
  ]
}
```

Each struct becomes an object with a key for each field, lists become arrays,
and enum cases become their names.

`fromJson` does the opposite. Since text could hold anything, you tell it
which type to read by giving the result a type, and it returns an optional:
`nil` if the text doesn't match that type.

```tessel
let text = toJson(list)
if let loaded: TaskList = fromJson(text) {
    print("{loaded.name}: {loaded.tasks.count} tasks")    // Home: 2 tasks
}
```

The type can be written in a few ways, whichever reads best:

```tessel
let a: TaskList? = fromJson(text)               // an optional
let b: [Task] = fromJson(text) ?? []            // with a default
if let c: TaskList = fromJson(text) {           // only when it worked
    print(c.name)
}
```

## Saving to a file and loading it back

JSON is just text, so [`writeFile`](/reference/stdlib/#writefile) and
[`readFile`](/reference/stdlib/#readfile) save and load it. This app keeps
its tasks in a file, loads them when it starts, and saves when a task is
added or the user clicks **Save**:

```tessel
struct Task {
    title: String
    done: Bool = false
}

fn tasksFile() -> String {
    joinPath(appDataFolder("Tasks"), "tasks.json")
}

app Tasks(width: 360, height: 400) {
    state tasks: [Task] = []
    state draft = ""

    VStack(spacing: 8, alignment: .leading) {
        TextField("New task", text: draft).onSubmit { add() }
        for i in tasks.indices {
            Toggle(tasks[i].title, isOn: tasks[i].done)
        }
        Button("Save") { save() }
    }
    .padding(16)
    .onAppear { load() }

    fn add() {
        if !draft.trim().isEmpty {
            tasks.append(Task(title: draft.trim()))
            draft = ""
            save()
        }
    }

    fn load() {
        if let text = readFile(tasksFile()) {
            if let saved: [Task] = fromJson(text) {
                tasks = saved
            }
        }
    }

    fn save() {
        writeFile(path: tasksFile(), text: toJson(tasks, pretty: true))
    }
}
```

A few things to notice:

- [`appDataFolder("Tasks")`](/reference/stdlib/#appdatafolder) is the
  folder where an app keeps its own files (on macOS,
  `~/Library/Application Support/Tasks`). It's created if needed. Use your
  app's name, so apps don't mix up their files.
- The first time the app runs, there's no file yet: `readFile` returns `nil`
  and the list stays empty. No special case is needed.
- If the file can't be read as `[Task]` (someone edited it by hand and made a
  mistake, say), `fromJson` returns `nil` and the app starts empty too. A
  real app might tell the user instead.

For a document the user picks, get the path from the
[save and open dialogs](/ui/async/#documents-and-custom-file-types) instead.

## Settings with defaults

When every field of a struct has a default, `Settings()` makes a value with
all the defaults. That's a good fallback when there's nothing saved yet:

```tessel
enum Theme { light, dark }

struct Settings {
    fontSize: Float = 14.0
    theme: Theme = Theme.light
    recentFiles: [String] = []
}

fn loadSettings(_ path: String) -> Settings {
    if let text = readFile(path) {
        if let settings: Settings = fromJson(text) {
            return settings
        }
    }
    Settings()
}
```

## Changing your types later

Apps change. When you add a field to a struct, files saved by the older
version of your app don't have it. As long as the new field has a **default
value** (or is optional), those files still load: the missing field takes
its default.

```tessel
struct Task {
    title: String
    done: Bool = false
    due: Float? = nil       // new in version 2
}

fn main() {
    let old = "\{\"title\": \"Pay the rent\", \"done\": true}"
    if let task: Task = fromJson(old) {
        print(task.due == nil)      // true
    }
}
```

Note the `\{` in the string: a plain `{` would start
[interpolation](/language/basics/#escapes).

The other rules for reading:

- Keys the struct doesn't have are ignored, so removing a field is safe too.
- A missing field **without** a default, a value of the wrong kind, or an
  enum case that doesn't exist makes the whole result `nil`.
- Renaming a field or an enum case breaks old files, since the names are
  what's saved.

## Enums with values

An enum case without values is saved as its name. A case with values becomes
an object with the case's name as its only key:

```tessel
enum Shape {
    circle(radius: Float)
    rectangle(width: Float, height: Float)
}

fn main() {
    let shapes = [Shape.circle(radius: 1.0), Shape.rectangle(width: 2.0, height: 3.0)]
    print(toJson(shapes))
}
```

```text
[{"circle":{"radius":1.0}},{"rectangle":{"width":2.0,"height":3.0}}]
```

The full list of how each kind of value is written is in the
[reference](/reference/stdlib/#how-values-are-written).

## Binary instead of JSON

JSON is easy to read and to exchange with other programs, but it's wordy.
`toBinary` writes the same values as compact bytes
([MessagePack](https://msgpack.org)), and `fromBinary` reads them back. Save
the bytes with [`writeData`](/reference/stdlib/#writedata) and read them
with [`readData`](/reference/stdlib/#readdata):

```tessel
let tasks = [Task(title: "a"), Task(title: "b", done: true)]
let path = joinPath(tempFolder(), "tasks.bin")
writeData(path: path, data: toBinary(tasks))

if let data = readData(path) {
    let loaded: [Task] = fromBinary(data) ?? []
    print(loaded.count)     // 2
}
```

Binary files follow the same rules as JSON for missing fields and defaults.
Use JSON when people or other programs might read the file, and binary when
size or speed matter more.

## Sending JSON over the network

Web APIs usually speak JSON too. Send a value as the body of a
[`request`](/ui/async/#talking-to-a-web-server-with-request), and read the
answer with `fromJson`:

```tessel
request(url: "https://example.com/api/tasks", method: "POST",
        headers: ["Content-Type": "application/json"],
        body: toJson(Task(title: "Buy milk"))) { response in
    if let saved: Task = fromJson(response.text) {
        tasks.append(saved)
    }
}
```

## What can't be saved

Values that are only data can be saved: numbers, `Bool`, `String`, `Data`,
ranges, optionals, lists, dictionaries (with `String`, `Int`, `Bool` or enum
keys), and structs and enums made of those. Functions and views can't:
`tessel check` tells you which field is the problem.

```text
error: `Counter` can't be saved with `toJson`
 --> main.tsl:8:23
  |
8 |     let text = toJson(c)
  |                       ^
  |
  = help: field `onChange`: functions can't be saved
```
