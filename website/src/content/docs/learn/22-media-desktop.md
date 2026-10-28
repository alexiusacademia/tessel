---
title: 22. Sound, video, the web and the desktop
description: Play sounds and videos, show web pages, send notifications, live in the menu bar or the taskbar's tray, fill the screen, drag files out of your app, and give your app's documents their own icon.
---

Your apps so far live inside their window. Desktop apps also make sounds,
play videos, show web pages, tell you something with a notification when
you're in another app, keep an icon in the menu bar, and let you drag a
file they made onto the desktop.

In this lesson you'll learn:

- how `playSound` plays a sound, and what you can do with the `Sound` it gives
- how `VideoPlayer` shows a video with its own controls
- how `WebView` shows a web page, or HTML of your own
- how `request` reads a stream from the web, like internet radio, as it
  arrives
- how `showNotification` tells the user something from outside the window
- how `.trayIcon` puts an icon and a menu in the menu bar (macOS) or the
  taskbar's tray (Windows), and keeps the app running there
- `setFullScreen`, and `.dragFile` for files dragged out of the app
- how your app's own kind of file opens with it when double-clicked, with
  its own icon
- how to check all of this without a window

You'll finish with a focus timer that lives in the tray.

## Sounds

`playSound` starts playing a sound file (MP3, M4A, WAV, …) and gives a
**`Sound`**, which can pause it, stop it, and say where it is:

```tessel
app Player(width: 320, height: 200) {
    state sound: Sound? = nil
    state info = ""

    VStack(spacing: 10) {
        HStack {
            Button("Play") { sound = playSound(joinPath(appFolder(), "song.mp3"), volume: 0.8) }
            Button("Pause") { sound?.pause() }
            Button("Resume") { sound?.resume() }
            Button("Stop") {
                sound?.stop()
                sound = nil
            }
        }
        Button("Where is it?") {
            if let s = sound {
                info = "{Int(s.position())} of {Int(s.duration())} seconds"
            }
        }
        Text(info)
    }
}
```

`joinPath(appFolder(), "song.mp3")` finds the file next to the app (or next
to its source under `tessel run`), wherever it's installed. `loop: true`
plays a sound over and over, which suits background music; for a short
"ding", play it and forget the `Sound`.

## Videos

A **`VideoPlayer`** shows a video, with a play button, a bar to move along
it, and its time, all drawn by Tessel:

```tessel
app Movie(width: 640, height: 440) {
    state playing = false
    state at = 0.0

    VStack {
        VideoPlayer(joinPath(appFolder(), "intro.mp4"), playing: playing, position: at)
        HStack {
            Button(if playing { "Pause" } else { "Play" }) { playing = !playing }
            Button("Back to the start") { at = 0.0 }
            Text("{Int(at)} s")
        }
    }
    .padding(12)
}
```

`playing:` and `position:` are bindings: the app can change them (to play,
pause or jump), and the player changes them as it plays, so the text above
shows the time as it goes. When the video comes to its end, `playing`
becomes false by itself. Both are optional; without them the player's own
controls do everything. `controls: false` leaves the controls out, and
`loop: true` plays the video over and over.

The video takes its own size, made smaller to fit; give it a `.frame` to
make it fill a place, as other pictures do.

## Web pages

A **`WebView`** shows a web page with the system's browser engine:

```tessel
app Help(width: 640, height: 480) {
    state page = 0

    VStack {
        Picker(selection: page) {
            Text("Website").tag(0)
            Text("Quick help").tag(1)
        }
        if page == 0 {
            WebView(url: "https://tessel-lang.syncster.dev")
        } else {
            WebView(html: "<h1>Quick help</h1><p>Press <b>Space</b> to start.</p>")
        }
    }
}
```

`url:` loads a page from the web, or a file (`"help/index.html"`, with
the pictures next to it). `html:` shows HTML your program made. The page
fills the place it's given. It's shown by a view of the system's, over
your window, so a popover can't appear over it.

## Streams from the web

[`request`](/ui/async/#talking-to-a-web-server-with-request) usually waits
for the whole answer. An internet radio station never finishes its answer.
Give `request` an `onData:` block, and it gets the sound in pieces as it
arrives. `onHeaders:` comes first, with the station's headers:

```tessel
app Tuner(width: 300, height: 120) {
    state station = "Not playing"
    state kilobytes = 0
    state playing: Int? = nil

    VStack(spacing: 8) {
        Text(station)
        Text("{kilobytes} KB received")
        Button(if playing == nil { "Listen" } else { "Stop" }) {
            if let id = playing {
                cancelRequest(id)
                playing = nil
                station = "Not playing"
            } else {
                var bytes = 0
                playing = request(url: "https://radio.example.com/jazz", onHeaders: { r in
                    station = r.headers["icy-name"] ?? "Radio"
                }, onData: { piece in
                    bytes += piece.count
                    kilobytes = bytes / 1024
                }) { r in
                    station = "The station stopped"
                    playing = nil
                }
            }
        }
    }
    .padding(16)
}
```

The pieces come in order. The body isn't kept, so the last block's `text`
and `data` are empty and memory doesn't fill up. `cancelRequest` stops the
stream. If the station redirects you elsewhere, the response's `finalUrl`
says where you ended up.

## Notifications

`showNotification` tells the user something even when your app isn't in
front, the way a calendar says a meeting is starting:

```tessel
Button("Remind me in a minute") {
    after(seconds: 60) {
        showNotification("Stretch", message: "Time to stand up for a minute.")
    }
}
```

The first time, the system asks the user whether your app may send
notifications. A built app (`tessel build --bundle`) shows them as itself;
under `tessel run` on macOS, they come from the system's scripting.

## Living in the tray

`.trayIcon` puts an icon in the menu bar (macOS) or the taskbar's tray
(Windows), with a menu of your app's commands. With `keepRunning: true`,
closing the window doesn't end the app: it goes on in the tray, and
`showWindow()` brings the window back.

```tessel
app Clipper(width: 320, height: 200) {
    state count = 0

    Text("Copied {count} times")
        .trayIcon(.clipboard, keepRunning: true) {
            MenuItem("Copy the date") {
                copyToClipboard(formatDate(now(), format: "d MMMM yyyy"))
                count += 1
            }
            MenuItem("Show window") { showWindow() }
            MenuDivider()
            MenuItem("Quit") { quit() }
        }
}
```

The icon is one of the [built-in icons](/ui/icons/), drawn to suit the menu
bar and the tray. `title:` puts a few words beside it on macOS (and is the
icon's tooltip on Windows): a timer's minutes, a count.

## The whole screen, and files dragged out

`setFullScreen(true)` makes the window fill the screen, and
`setFullScreen(false)` puts it back; `isFullScreen()` says which it is. A
presentation, a game, or a focus mode want it.

`.dragFile(path)` on a view lets the user drag that file out of the app:
onto the desktop, into a folder, or into an email. The app makes the file
first (a report, an export), then the view stands for it:

```tessel
app Export(width: 320, height: 200) {
    let path = joinPath(tempFolder(), "report.txt")

    VStack {
        Button("Make the report") {
            writeText(path: path, text: "Sales: 42")
        }
        HStack {
            Icon(.fileText)
            Text("report.txt")
        }
        .padding(8)
        .background(.lightGray, radius: 6)
        .dragFile(path)
    }
}
```

## Your app's own documents

An app that saves its work in a format of its own, like a `.note` for a
notes app, wants those files to open with it when they're double-clicked,
and to look like its documents. Say so in the project's `tessel.toml`:

```toml
[app]
name = "Notes"

[[app.documents]]
name = "Note"
extensions = ["note"]
owned = true
icon = "note-icon.png"
```

- `owned = true` says `.note` is the app's own kind of file.
- `icon` is a PNG for the files (1024 × 1024 is best). Without it, they show
  the app's icon.
- The app gets the file in `.onOpenFile`, as in [Opening
  documents](/ui/menus-windows/#opening-documents):

```tessel
app Notes(width: 420, height: 280) {
    state path = ""
    state text = ""

    TextField("Note", text: text, lines: 10)
        .onOpenFile { file in
            path = file
            text = readFile(path: file) ?? ""
        }
}
```

It takes effect in the built app: `tessel build --bundle` on a Mac, and the
installer on Windows. Leave `owned` out for kinds of files other apps make
too (`csv`, `png`): your app is then offered in **Open With** for them,
and they keep the app and icon they have.

## Worked example: a focus timer

A timer for working in 25-minute blocks: it counts down in the tray, says
so with a notification and a sound when the block is over, and can fill
the screen while you work.

```tessel
app Focus(width: 340, height: 260) {
    state left = 25 * 60
    state timer = 0
    state running = false

    let seconds = left % 60
    let minutes = "{left / 60}:{if seconds < 10 { "0" } else { "" }}{seconds}"

    fn start() {
        running = true
        timer = every(seconds: 1) {
            left -= 1
            if left <= 0 {
                stop()
                left = 25 * 60
                showNotification("Time for a break", message: "You focused for 25 minutes.")
                playSound(joinPath(appFolder(), "bell.wav"))
            }
        }
    }

    fn stop() {
        stopTimer(timer)
        running = false
        setFullScreen(false)
    }

    VStack(spacing: 14) {
        Text(minutes).font(size: 48, weight: .bold)
        HStack {
            Button(if running { "Stop" } else { "Start" }) {
                if running { stop() } else { start() }
            }
            Button("Focus mode") { setFullScreen(!isFullScreen()) }
        }
    }
    .trayIcon(.clock, title: minutes, keepRunning: true) {
        MenuItem(if running { "Stop" } else { "Start" }) {
            if running { stop() } else { start() }
        }
        MenuItem("Show window") { showWindow() }
        MenuDivider()
        MenuItem("Quit") { quit() }
    }
}
```

Things to notice:

- The tray's title is `minutes`, worked out in the body, so the menu bar
  shows the time left as it changes.
- The tray's menu has the same command as the window's button. Both call
  the view's functions `start()` and `stop()`.
- Closing the window leaves the timer running in the tray.

## Checking it without a window

A headless run says what these would do, instead of doing it:

```sh
TESSEL_SCRIPT='tray; tray "Start"; wait 3; tray; click "Focus mode"; tree' tessel run focus.tsl
```

- `tray` prints the tray icon and its menu; `tray "Start"` chooses an item.
- A notification prints `(notification "Title" "message")`.
- Its players play nothing, say every file is a minute long, and keep time
  on the script's clock, so `wait 3` moves them three seconds on.
- `tree` shows `Root (hidden)` while the window is closed into the tray, a
  `VideoPlayer` with where it is, and a `WebView` with what it shows.
- `dragto "report.txt" "…"` prints `(drag file … out)`.

## Common mistakes

### Paths that only work in your folder

`playSound("bell.wav")` looks for the file in the folder the app was
started from, which is yours under `tessel run` but not on someone else's
computer. Use `joinPath(appFolder(), "bell.wav")` for files that come with
the app.

### Expecting a popover over a web page

A `WebView` is the system's own view, put over your window, so it covers
popovers and sheets that open over it. Open those beside it, or hide the
page while one is open.

### Claiming files that aren't yours

`owned = true` on `csv` or `txt` would take those files from the apps
people use for them. Mark only your app's own format as owned; for others,
the app is offered in Open With.

### A tray app with no way back

With `keepRunning: true`, closing the window hides it. Give the tray's menu
a "Show window" item (`showWindow()`) and a "Quit" item (`quit()`), or
the user can't get the window back or end the app.

## Exercises

**1. A metronome.** Make an app with a slider for beats per minute (40 to
200), and Start and Stop buttons. While it runs, it plays a short sound
every beat (`every(seconds: 60.0 / bpm)`).

<details>
<summary>Solution</summary>

```tessel
app Metronome(width: 320, height: 200) {
    state bpm = 100.0
    state timer = 0
    state running = false

    VStack(spacing: 12) {
        Text("{Int(bpm)} beats per minute")
        Slider(value: bpm, from: 40, to: 200, step: 1)
        Button(if running { "Stop" } else { "Start" }) {
            if running {
                stopTimer(timer)
                running = false
            } else {
                timer = every(seconds: 60.0 / bpm) {
                    playSound(joinPath(appFolder(), "tick.wav"))
                }
                running = true
            }
        }
    }
    .padding(20)
}
```

Changing the slider while it runs only counts once it's started again,
since the timer's interval was set when it started.

</details>

**2. A video with chapters.** Show a video, and under it three buttons,
"Start", "Middle" and "End", that jump to 0 seconds, 30 seconds and 60
seconds, and play.

<details>
<summary>Solution</summary>

```tessel
app Chapters(width: 640, height: 440) {
    state playing = false
    state at = 0.0

    VStack {
        VideoPlayer(joinPath(appFolder(), "talk.mp4"), playing: playing, position: at)
        HStack {
            for (title, seconds) in [("Start", 0.0), ("Middle", 30.0), ("End", 60.0)] {
                Button(title) {
                    at = seconds
                    playing = true
                }
            }
        }
    }
    .padding(12)
}
```

</details>

**3. A tray counter.** Make an app that lives in the tray with a counter
as its title, a menu with "Add one" and "Reset", and a notification when
the count reaches 10.

<details>
<summary>Solution</summary>

```tessel
app Tally(width: 260, height: 160) {
    state count = 0

    Text("Count: {count}")
        .trayIcon(.plus, title: "{count}", keepRunning: true) {
            MenuItem("Add one") {
                count += 1
                if count == 10 {
                    showNotification("Ten!", message: "You've counted to ten.")
                }
            }
            MenuItem("Reset") { count = 0 }
            MenuItem("Show window") { showWindow() }
            MenuDivider()
            MenuItem("Quit") { quit() }
        }
}
```

</details>

## Summary

- **`playSound(path, volume:loop:)`** plays a sound and gives a **`Sound`**:
  `pause()`, `resume()`, `stop()`, `seek(to:)`, `setVolume(…)`,
  `isPlaying()`, `position()`, `duration()`.
- **`VideoPlayer(path, playing:position:)`** shows a video with controls
  drawn by Tessel; its bindings play, pause and jump.
- **`WebView(url:)`** and **`WebView(html:)`** show web pages with the
  system's browser engine.
- **`request(url:onHeaders:onData:)`** reads a stream as it arrives;
  **`cancelRequest`** stops it.
- **`showNotification(title, message:)`** tells the user something from
  outside the window.
- **`.trayIcon(icon, title:keepRunning:) { … }`** gives the app an icon and a
  menu in the menu bar or tray; `showWindow()` brings its window back.
- **`setFullScreen`**, **`isFullScreen`**, and **`.dragFile(path)`** for files
  dragged out of the app.
- **`owned = true`** and **`icon`** in a `[[app.documents]]` of `tessel.toml`
  make a kind of file the app's own: it opens with the app, with its icon.
- For more, see [Sound and video](/ui/media/), [Web pages](/ui/web/),
  [The desktop](/ui/desktop/) and [The project file](/start/project/#documents).

**Next:** [23. Where to go next](/learn/21-next-steps/)
