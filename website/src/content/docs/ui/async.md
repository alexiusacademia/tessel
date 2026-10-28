---
title: Files, commands and timers
description: Read and write files, let the user choose files with open and save dialogs, run other programs, talk to web servers, run code later or on a timer, and do heavy work in the background. Callbacks run on the UI thread and may change state.
---

Apps need to do more than draw: save files, run tools, load data from the
network, do something a few seconds from now, or crunch numbers without
freezing the window. This page covers the built-in functions for that.

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

Every function on this page that ends with such a block can also be used
with [`await`](/language/async/), which reads top to bottom instead of
nesting:

```tessel
Button("Refresh") {
    report = "Loading…"
    let text = await fetch(url: "https://example.com/weather.txt")
    report = text ?? "Could not load the weather"
}
```

The two forms do the same thing. The examples below use blocks; see
[Waiting with async and await](/language/async/) for the other.

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
| `appendFile(path:text:)` | `Bool` | Adds `text` to the end of the file. |
| `copyFile(from:to:)` | `Bool` | Copies a file. |
| `fileSize(path)` | `Int?` | The file's size in bytes. |
| `modifiedTime(path)` | `Float?` | When the file last changed. |
| `readData(path)` | `Data?` | The file's raw bytes, for files that aren't text. |
| `readData(path, from:count:)` | `Data?` | Up to `count` bytes, starting at byte `from`. Only those bytes are read. |
| `writeData(path:data:)` | `Bool` | Writes raw bytes to the file. |
| `appendData(path:data:)` | `Bool` | Adds raw bytes to the end of the file. |

Relative paths are relative to `currentFolder()`, the folder the program was
started from. That's fine for tools, but an app started from the Finder or
the Start menu can't count on it: keep its files in
[`appDataFolder("YourApp")`](/reference/stdlib/#appdatafolder) or
[`documentsFolder()`](/reference/stdlib/#folders-and-paths), and build paths
with [`joinPath`](/reference/stdlib/#joinpath).

To save structs, lists and other values rather than plain text, turn them
into JSON with `toJson` and read them back with `fromJson`; see
[Saving data as JSON](/guides/json/).

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

### Reading part of a big file

`readData(path, from:count:)` reads only part of a file: up to `count` bytes,
starting at byte `from`. Only those bytes go into memory, so a program can
work through a file bigger than memory, such as a video or a disk image, a
piece at a time. Near the end you get fewer bytes, and at the end you get
none. This counts the zero bytes in a file, a megabyte at a time:

```tessel
fn main() {
    let path = "disk.img"
    let size = fileSize(path) ?? 0
    let piece = 1048576
    var zeros = 0
    var from = 0
    while from < size {
        if let data = readData(path, from: from, count: piece) {
            zeros += data.bytes.filter { b in b == 0 }.count
        }
        from += piece
    }
    print("{zeros} of {size} bytes are zero")
}
```

In an app, do this in [background work](#heavy-work-in-the-background), which
can call `readData` like any other code.

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
`fetch` is the simple way to load text: it makes a GET request and gives you
the text, or `nil` for any failure, including an error status from the
server. To send data, set headers, or see the status code, use
[`request`](#talking-to-a-web-server-with-request).
:::

## Talking to a web server with `request`

`request(url:method:headers:body:done:)` sends any HTTP request and gives
its block the whole response, an
[`HttpResponse`](/reference/stdlib/#httpresponse):

```tessel
struct Todo {
    title: String
    done: Bool = false
}

app Todos(width: 360, height: 300) {
    state todos: [Todo] = []
    state status = ""

    VStack(spacing: 8, alignment: .leading) {
        Button("Load") { load() }
        Button("Add") { add("Buy milk") }
        Text(status).color(.secondary)
        for todo in todos {
            Text(todo.title)
        }
    }
    .padding(16)

    fn load() {
        request(url: "https://example.com/api/todos") { response in
            if !response.ok {
                status = "Couldn't load (status {response.status})"
            } else if let loaded: [Todo] = fromJson(response.text) {
                todos = loaded
            } else {
                status = "The server sent something unexpected"
            }
        }
    }

    fn add(_ title: String) {
        request(url: "https://example.com/api/todos", method: "POST",
                headers: ["Content-Type": "application/json"],
                body: toJson(Todo(title: title))) { response in
            status = if response.ok { "Added" } else { "Couldn't add: {response.status} {response.error}" }
        }
    }
}
```

Everything but `url` and the block is optional: `method` is `"GET"` unless
you say otherwise, and there are no extra headers and no body. The response
has:

- `status`: the HTTP status code, like `200` or `404`, and `ok`: whether it's
  a success (200 to 299). An error status is still a response, so its body
  is there too, for the server's explanation.
- `text` and `data`: the body as text, and as bytes.
- `headers`: the response headers, with lower-case names.
- `error`: if the server couldn't be reached at all, why. Then `status` is
  `0`.

`request` also reads `file://` URLs. Like a web server, it honours a
`Range` header: `headers: ["Range": "bytes=1000-1999"]` gives status 206 and
only those bytes, and only they are read from the disk.

[`toJson`](/reference/stdlib/#tojson) and [`fromJson`](/reference/stdlib/#fromjson)
turn your structs into the JSON that most web APIs use, and back.

## Downloading files

`download(url:to:progress:done:)` saves a URL straight to a file, and can
report how far it has got:

```tessel
app Downloader(width: 360, height: 160) {
    state progress = 0.0
    state status = "Ready"

    VStack(spacing: 8) {
        Text(status)
        Text("{Int(progress * 100.0)}%").font(size: 24)
        Button("Download") {
            status = "Downloading…"
            let file = joinPath(documentsFolder(), "tessel.zip")
            download(url: "https://example.com/tessel.zip", to: file, progress: { fraction in
                progress = fraction
            }) { ok in
                status = if ok { "Saved to {file}" } else { "Download failed" }
            }
        }
    }
    .padding(16)
}
```

`progress` is optional. It gets a fraction from `0.0` to `1.0` as the file
arrives (when the server says how big the file is). `done` gets whether it
worked. A failed download leaves no partial file behind.

Both `request` and `download` return an id. To give one up while it's under
way (the user pressed Cancel, or left the page that needed it), pass the id to
[`cancelRequest`](/reference/stdlib/#cancelrequest). Its `done` block then
never runs, and a cancelled download leaves no file.

## WebSockets

A WebSocket is a connection that stays open, so the server can send messages
at any time: for chat, live prices, or notifications.
`openWebSocket(url:onMessage:onClose:)` connects and returns an id;
`sendWebSocket(id, text:)` sends a message, and `closeWebSocket(id)` hangs
up:

```tessel
app Chat(width: 360, height: 400) {
    state socket: Int? = nil
    state messages: [String] = []
    state draft = ""

    VStack(spacing: 8, alignment: .leading) {
        HStack {
            Button("Connect") { connect() }.disabled(socket != nil)
            Button("Disconnect") { disconnect() }.disabled(socket == nil)
        }
        for message in messages {
            Text(message)
        }
        TextField("Message", text: draft).onSubmit {
            if let id = socket {
                sendWebSocket(id, text: draft)
                draft = ""
            }
        }
    }
    .padding(16)

    fn connect() {
        socket = openWebSocket(url: "wss://example.com/chat", onMessage: { text in
            messages.append(text)
        }) { reason in
            messages.append("Disconnected: {reason}")
            socket = nil
        }
    }

    fn disconnect() {
        if let id = socket {
            closeWebSocket(id)
        }
    }
}
```

`onMessage` runs for each message from the server. `onClose` runs once when
the connection ends, for whatever reason: you closed it, the server did, or
it couldn't connect in the first place. Messages are text; send JSON with
`toJson` to exchange structured data.

For servers that speak in bytes rather than text, give `openWebSocket` an
`onData:` block, which gets each binary message as `Data`, and send with
[`sendWebSocketData`](/reference/stdlib/#sendwebsocketdata).

## Running code later with `after`

`after(seconds:action:)` runs a block once, after a delay:

```tessel
Button("Save") {
    save()
    showSaved = true
    after(seconds: 2) { showSaved = false }
}
```

The delay is a `Float`, so `after(seconds: 0.5) { … }` works. A timer made
with `after` can't be cancelled once started. If it might no longer be wanted
when it fires, have the block check a `state` value first.

## Repeating with `every`

`every(seconds:action:)` runs a block again and again, and returns an id
that `stopTimer` takes to stop it. A stopwatch:

```tessel
app Stopwatch(width: 300, height: 180) {
    state tenths = 0
    state timer: Int? = nil

    VStack(spacing: 12) {
        Text("{tenths / 10}.{tenths % 10}").font(size: 40, weight: .bold)
        HStack {
            Button("Start") { start() }.disabled(timer != nil)
            Button("Stop") { stop() }.disabled(timer == nil)
            Button("Reset") { tenths = 0 }
        }
    }
    .padding(16)

    fn start() {
        timer = every(seconds: 0.1) {
            tenths += 1
        }
    }

    fn stop() {
        if let id = timer {
            stopTimer(id)
            timer = nil
        }
    }
}
```

The first run is one interval after `every` is called. The block can also
stop its own timer, once it has done its job.

## Heavy work in the background

Everything in an app's blocks runs on the UI thread, the one that draws the
window. Something that takes a long time there, like a big calculation,
freezes the window until it's done. `background(work:done:)` runs it on
another thread instead, and hands the result to a block back on the UI
thread:

```tessel
fn primesBelow(limit: Int) -> [Int] {
    var sieve: [Bool] = []
    for _ in 0..limit {
        sieve.append(true)
    }
    var primes: [Int] = []
    for i in 2..limit {
        if sieve[i] {
            primes.append(i)
            for k in i..(limit - 1) / i + 1 {
                sieve[i * k] = false
            }
        }
    }
    primes
}

app Primes(width: 360, height: 200) {
    state limit = 200000
    state status = "Idle"
    state busy = false

    VStack(spacing: 12) {
        Text(status)
        Button("Count") {
            busy = true
            status = "Counting…"
            let n = limit
            background(work: { primesBelow(limit: n) }) { primes in
                status = "{primes.count} primes below {n}"
                busy = false
            }
        }
        .disabled(busy)
    }
    .padding(24)
}
```

While the primes are counted, the window stays responsive. Then the block
after `background` gets the list, and like every callback, it can change
`state`: the window shows `17984 primes below 200000`.

The `work:` block runs at the same time as the rest of the app, so it
mustn't touch anything the app might be changing. Tessel makes this safe by
**copying**: `work` gets its own copies of the values it uses, and the block
after it gets a copy of the result. For that to work:

- `work` can't use `state`, bindings, fields of the app or view, or its
  functions (`fn`s declared inside the `app` or `view`). Top-level functions
  like `primesBelow` are fine, and so are local values.
- The values it uses, and its result, must be plain data that
  [`toBinary`](/reference/stdlib/#tobinary) could save: numbers, strings,
  lists, dictionaries, your structs and enums, and so on, but not functions.

That's why the example copies `limit` into `let n` first. Using `limit`
directly is an error:

```text
error: background work can't use `limit`: `limit` is `state`, which the app keeps changing
  --> main.tsl:28:30
   |
28 |             background(work: { primesBelow(limit: limit) }) { primes in
   |                              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: copy what it needs first, like `let input = limit`, and use that
```

The copy is made when `background` is called, so changing `limit` afterwards
doesn't affect work that has already started.

The `work:` block ends with its result, like a function. It can be several
lines long, and `return` works in it too:

```tessel
let points = shape.points
background(work: {
    if points.isEmpty {
        return 0.0
    }
    var total = 0.0
    for p in points {
        total += p.x * p.y
    }
    total
}) { area in
    result = area
}
```

### Progress, and cancelling

Work that takes a while should say how far it is, and let the user stop it.
Give the `work:` block a name for its job, `work: { job in … }`; then:

- `job.progress(fraction, note: "…")` reports how far it is (usually from 0
  to 1, with an optional note), to a `progress:` block that runs on the UI
  thread and can change `state`;
- `job.isCancelled()` says whether the job was cancelled, so the work can
  stop early;
- `background` returns the job (a [`BackgroundJob`](/reference/stdlib/#backgroundjob)),
  whose `cancel()` cancels it.

```tessel
fn countPrimes(limit: Int, job: BackgroundJob) -> Int {
    var count = 0
    for n in 2..limit {
        if n % 1000 == 0 {
            if job.isCancelled() {
                return count
            }
            job.progress(Float(n) / Float(limit), note: "{count} so far")
        }
        var prime = true
        var d = 2
        while d * d <= n {
            if n % d == 0 {
                prime = false
                break
            }
            d += 1
        }
        if prime {
            count += 1
        }
    }
    count
}

app Primes(width: 360, height: 200) {
    state status = "Idle"
    state fraction = 0.0
    state job: BackgroundJob? = nil

    VStack(spacing: 12) {
        Text(status)
        Text("{(fraction * 100).formatted(decimals: 0)}%")
        HStack {
            Button("Count") {
                status = "Counting…"
                job = background(work: { job in countPrimes(limit: 2000000, job: job) }, progress: { f, note in
                    fraction = f
                    status = note
                }) { count in
                    status = "{count} primes"
                    job = nil
                }
            }
            .disabled(job != nil)
            Button("Cancel") {
                job?.cancel()
                job = nil
                status = "Cancelled"
            }
            .disabled(job == nil)
        }
    }
    .padding(24)
}
```

Reports can come much faster than the window can show them, so the
`progress:` block gets the latest one each time the app gets round to it,
skipping those in between; the last report always arrives before the result.
After `cancel()`, neither block runs again: the work sees `isCancelled()`,
stops, and what it returns is dropped. The job can be passed on to the
functions doing the work, like `countPrimes` here, and kept in `state` to
cancel it later.

## Callbacks in a program without an app

`fetch`, `request`, `download`, WebSockets, `runCommand`, `after`, `every`
and `background` also work in a program with `fn main()` and no `app`. When
`main` returns, the program doesn't end right away: it keeps running until no
timers, requests, downloads, open WebSockets, commands or background work
are left, running their callbacks as they come in, and then exits.

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

A timer started with `every` keeps the program running until it's stopped
with `stopTimer`, and an open WebSocket until it's closed. To end the program
right away, call [`exit`](/reference/stdlib/#exit).

## Testing code that waits

When you test an app with a [headless script](/ui/testing/), time doesn't pass
by itself: the `wait` command moves the clock forward and fires the timers
that are due, including `every` timers. `wait` also first lets running
`fetch`, `request`, `download`, `runCommand` and `background` calls finish,
so tests don't depend on how fast the machine is.
