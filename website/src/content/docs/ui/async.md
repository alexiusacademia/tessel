---
title: Files, commands and timers
description: Read and write files, let the user choose files with open and save dialogs, run other programs, download text and run code later. Callbacks run on the UI thread and may change state.
---

Apps need to do more than draw: save files, run tools, load data from the
network, do something a few seconds from now. This page covers the built-in
functions for that.

Some of them finish right away (the file functions). The others take a
*callback*, a block Tessel runs later, when the work is done. Callbacks always
run on the UI thread, between frames, so they can change `state` directly, and
the window updates afterwards:

```tessel
app Weather(width: 360, height: 200) {
    state report = "Press Refresh"

    Text(report)
    Button("Refresh") {
        report = "Loading…"
        fetch(url: "https://example.com/weather.txt") { text in
            report = text ?? "Could not load the weather"
        }
    }
}
```

While the download runs, the app stays responsive.

## Files

The file functions work immediately and return a result you can check:

| Function | Returns | What it does |
|----------|---------|--------------|
| `readFile(path)` | `String?` | The file's text, or `nil` if it can't be read. |
| `writeFile(path:text:)` | `Bool` | Writes `text` to the file, replacing what was there. `true` if it worked. |
| `fileExists(path)` | `Bool` | Whether a file or folder exists at `path`. |
| `isFolder(path)` | `Bool` | Whether `path` is a folder. |
| `listFolder(path)` | `[String]?` | The names of the files and folders inside, sorted; `nil` if the folder can't be read. |
| `createFolder(path)` | `Bool` | Creates a folder, and any missing folders above it. |
| `renameFile(from:to:)` | `Bool` | Renames or moves a file or folder. |
| `deleteFile(path)` | `Bool` | Deletes a file. It doesn't delete folders. |
| `currentFolder()` | `String` | The folder the program was started from. |

Relative paths are relative to `currentFolder()`, the folder the program was
started from.

A simple notes app that loads its text when it starts and saves on request:

```tessel
app Notes(width: 400, height: 240) {
    state text = ""
    state status = ""

    VStack {
        TextField("Write something", text: text)
        HStack {
            Button("Save") { save() }
            Text(status).color(.secondary)
        }
    }
    .padding(16)
    .onAppear {
        text = readFile("notes.txt") ?? ""
    }

    fn save() {
        status = if writeFile(path: "notes.txt", text: text) { "Saved" } else { "Couldn't save" }
    }
}
```

`readFile` and `listFolder` return optionals, because a missing file is
normal; use `??` or `if let` to handle it (see [Optionals](/language/optionals/)).
Listing a folder:

```tessel
let names = listFolder(currentFolder()) ?? []
for name in names {
    if !name.hasPrefix(".") {
        print(name)
    }
}
```

The file functions work in any program, including one with `fn main()`
instead of an app.

## Documents and custom file types

Many apps work on documents: the user saves their work to a file, and opens
it again later. `openFile` and `saveFile` show the system's standard open and
save dialogs and give you the path the user picked, or `nil` if they
cancelled. You then read or write the file with `readFile` and `writeFile`.

This notes app saves each note in its own file, with the extension `.note`:

```tessel
app Notes(width: 420, height: 220) {
    state title = ""
    state body = ""
    state status = "New note"

    VStack(spacing: 8, alignment: .leading) {
        TextField("Title", text: title)
        TextField("Text", text: body)
        HStack {
            Button("Open…") { open() }
            Button("Save…") { save() }
            Text(status).color(.secondary)
        }
    }
    .padding(16)

    fn save() {
        let text = "title={title}\nbody={body}\n"
        if let path = saveFile(title: "Save note", name: "{title}.note", types: ["note"]) {
            status = if writeFile(path: path, text: text) { "Saved" } else { "Couldn't save" }
        }
    }

    fn open() {
        if let path = openFile(title: "Open note", types: ["note"]) {
            if let text = readFile(path) {
                for line in text.lines() {
                    if line.hasPrefix("title=") {
                        title = line.substring(from: 6, to: line.count)
                    } else if line.hasPrefix("body=") {
                        body = line.substring(from: 5, to: line.count)
                    }
                }
                status = "Opened"
            } else {
                status = "Couldn't read the file"
            }
        }
    }
}
```

A few things to notice:

- `types: ["note"]` makes the open dialog show only `.note` files, and makes
  the save dialog add `.note` when the user types a name without it. Write
  extensions without the dot. You can list several; the first one is the one
  added when saving.
- `name:` suggests a file name in the save dialog.
- If the user cancels, the dialog returns `nil`, so the `if let` does
  nothing.
- The file format is up to you. Here each line is `key=value`, which is easy
  to write with string interpolation and to read back with
  [`lines`](/reference/stdlib/#lines), [`hasPrefix`](/reference/stdlib/#contains-hasprefix-and-hassuffix)
  and [`substring`](/reference/stdlib/#substring). A saved note looks like
  this:

  ```text
  title=Groceries
  body=milk, eggs
  ```

  This simple format holds one line per field, which is why the note's text
  is a single-line `TextField`.

The dialogs are for apps: call them from something the user does, like a
button or a [keyboard shortcut](/ui/shortcuts/). In a
[headless test](/ui/testing/#file-dialogs), no dialog opens; the test script
says what each dialog returns instead. See
[File dialogs](/reference/stdlib/#file-dialogs) for the details.

## Environment variables

`environment(name)` returns an environment variable's value, or `nil` if it
isn't set:

```tessel
let home = environment("HOME") ?? currentFolder()
let tessel = environment("TESSEL_EXE") ?? "tessel"
```

## Running other programs

`runCommand` starts another program and hands you its output line by line:

```tessel
app Runner(width: 420, height: 300) {
    state output: [String] = []
    state running: Int? = nil

    HStack {
        Button("List files") { start() }.disabled(running != nil)
        Button("Stop") { stop() }.disabled(running == nil)
    }
    Scroll {
        VStack(spacing: 2, alignment: .leading) {
            for line in output {
                Text(line).font(size: 12)
            }
        }
    }

    fn start() {
        output = []
        running = runCommand(path: "ls", args: ["-1"], onOutput: { line in
            output.append(line)
        }) { code in
            output.append("Finished with exit code {code}")
            running = nil
        }
    }

    fn stop() {
        if let id = running {
            stopCommand(id)
        }
    }
}
```

`runCommand(path:args:onOutput:onExit:)` takes:

- `path:` the program to run. It's started directly, not through a shell, so
  write the program and its arguments separately (no pipes or `*` patterns). A
  name without a folder, like `"ls"`, is looked up in the system's `PATH`.
- `args:` the arguments, as a list of strings. Optional.
- `onOutput:` called with each line the program prints, without the line break.
  Both normal output and error output arrive here.
- `onExit:` called with the program's exit code once it has finished, after
  all of its output. By convention, 0 means success.

`onExit` is the last parameter, so it's usually written as a trailing block.
Instead of blocks, you can also pass functions declared in the app:

```tessel
Button("List files") {
    runCommand(path: "ls", onOutput: remember, onExit: finished)
}

fn remember(_ line: String) {
    output.append(line)
}

fn finished(_ code: Int) {
    output.append("Finished with exit code {code}")
}
```

The program runs in the background. It can't read input from the user (its
standard input is empty). `runCommand` returns an `Int` id right away; pass it
to `stopCommand(id)` to stop the program. Its `onExit` still runs (on macOS,
with the exit code `-1`).

If the program can't be started at all (for example, it doesn't exist),
`onOutput` receives one line starting with `error: can't start`, and `onExit`
gets `-1`.

The [Tessel IDE](/start/ide/) uses `runCommand` to run `tessel check`,
`tessel build` and your program, and shows their output in its console.

## Downloading text with `fetch`

`fetch(url:done:)` loads a URL in the background and calls `done` with the text,
or with `nil` if it failed:

```tessel
fetch(url: "https://example.com/data.txt") { text in
    if let text = text {
        lines = text.lines()
    } else {
        status = "Download failed"
    }
}
```

It supports `http://` and `https://` URLs, and `file://` for a local file.

:::note
`fetch` is simple for now: it only makes GET requests, can't send headers,
and gives you just the text of the response: any failure, including an error
status from the server, gives `nil`.
:::

## Running code later with `after`

`after(seconds:action:)` runs a block once, after a delay:

```tessel
Button("Save") {
    save()
    showSaved = true
    after(seconds: 2) { showSaved = false }
}
```

The delay is a `Float`, so `after(seconds: 0.5) { … }` works. To repeat
something, start the next timer from inside the block:

```tessel
app Countdown(width: 300, height: 180) {
    state seconds = 10

    Text("{seconds}").font(size: 40, weight: .bold)
    Button("Start") { tick() }

    fn tick() {
        after(seconds: 1) {
            seconds -= 1
            if seconds > 0 {
                tick()
            }
        }
    }
}
```

A timer can't be cancelled once started. If it might no longer be wanted when
it fires, have the block check a `state` value first.

## Callbacks in a program without an app

`fetch`, `runCommand` and `after` also work in a program with `fn main()` and
no `app`. When `main` returns, the program doesn't end right away: it keeps
running until no timers, downloads or commands are left, running their
callbacks as they come in, and then exits.

```tessel
fn main() {
    after(seconds: 1) {
        print("one second later")
    }
    runCommand(path: "echo", args: ["hello"], onOutput: { line in
        print("echo printed: {line}")
    }) { code in
        print("echo finished with {code}")
    }
    print("main is done")
}
```

This prints:

```text
main is done
echo printed: hello
echo finished with 0
one second later
```

## Testing code that waits

When you test an app with a [headless script](/ui/testing/), time doesn't pass
by itself: the `wait` command moves the clock forward and fires the timers
that are due. `wait` also first lets running `fetch` and `runCommand` calls
finish, so tests don't depend on how fast the machine is.
