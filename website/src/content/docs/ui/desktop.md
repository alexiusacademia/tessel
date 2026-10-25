---
title: The desktop
description: A tray or menu-bar icon with a menu that keeps the app running, the system's notifications, full screen, and files dragged out of the app.
---

Besides its windows, a desktop app can have an icon in the menu bar
(macOS) or the taskbar's notification area (Windows), send notifications,
fill the screen, and hand the user files to drag onto the desktop or into
other apps. On other systems (and phones), these do nothing.

## A tray icon

`.trayIcon` gives the app an icon with a menu, in the menu bar's right
side on macOS and the taskbar's tray on Windows:

```tessel
app Uploader(width: 360, height: 240) {
    state uploads = 0

    Text("{uploads} files sent")
        .trayIcon(.upload, title: "{uploads}", keepRunning: true) {
            MenuItem("Send a file…") {
                if let path = openFile(title: "Send") {
                    print("sending {path}")
                    uploads += 1
                }
            }
            MenuItem("Show window") { showWindow() }
            MenuDivider()
            MenuItem("Quit") { quit() }
        }
}
```

```text
.trayIcon(_ icon: IconName, title: String = "", keepRunning: Bool = false, items: View)
```

- The icon is one of the [built-in icons](/ui/icons/), drawn in the menu
  bar's color on macOS and to suit the taskbar on Windows.
- `title:` is text beside the icon on macOS (a count, a timer's minutes),
  and the icon's tooltip on Windows. It follows your state like any text.
- The items are [`MenuItem`s, `MenuDivider`s and `Menu`s](/ui/menus-windows/),
  as in a context menu. On macOS a click on the icon opens the menu; on
  Windows a right click does, and a left click shows the window.
- Put it on a view of the app's main window (any view: it's the app's).
  An app has one tray icon.

### Keeping the app running

With `keepRunning: true`, closing the main window doesn't end the app: the
window is hidden, and the app goes on in its icon (a timer goes on
counting, a server goes on serving). **`showWindow()`** brings the window
back, in front. **`quit()`** ends the app.

Give such an app a menu item for each, or the user has no way back to the
window and no way to end the app.

## Notifications

```text
showNotification(_ title: String, message: String = "")
```

Shows a notification from the system, which the user sees even when the
app isn't in front:

```tessel
Button("Back up now") {
    backUp()
    showNotification("Backup done", message: "128 files were saved.")
}
```

- The first time, the system asks the user whether the app may send
  notifications (on macOS). If they say no, nothing shows.
- On macOS, an app built as a bundle (`tessel build --bundle`) sends them
  as itself; one run without a bundle (`tessel run`) sends them through
  the system's scripting, which shows them as Script Editor's.
- On Windows, they come from the app's icon in the tray (a hidden one when
  the app has no `.trayIcon`). Clicking one shows the main window.

## Full screen

```text
setFullScreen(_ on: Bool)
isFullScreen() -> Bool
```

`setFullScreen(true)` makes the main window fill the screen, without its
title bar; `setFullScreen(false)` puts it back. `isFullScreen()` says which
it is (also after the user changed it with the window's own button).

```tessel
Button(if isFullScreen() { "Leave full screen" } else { "Full screen" }) {
    setFullScreen(!isFullScreen())
}
```

## Dragging files out

```text
.dragFile(_ path: String)
```

A view with `.dragFile` can be dragged out of the app, as the file at
`path`: onto the desktop or into a folder (which copies it), or into
another app (an email, a chat, an editor), which gets the file as if it
were dragged from Finder or Explorer.

```tessel
HStack {
    Icon(.fileText)
    Text("summary.csv")
}
.padding(8)
.background(.lightGray, radius: 6)
.dragFile(joinPath(tempFolder(), "summary.csv"))
```

- Write the file before it's dragged (when the data changes, or when the
  view appears). A path to no file drags nothing.
- The drag starts once the mouse has moved a few points with its button
  down; a click is still a click.
- Dropped back on the app, it comes to [`.onDrop`](/reference/modifiers/#ondrop)
  like any file.

## In tests

A [headless run](/ui/testing/) does none of these; it says what it would do:

| | |
|---|---|
| `tray` | Prints the tray icon, its title and its menu. |
| `tray "Show window"` | Chooses an item of its menu. |
| A notification | Prints `(notification "Title" "message")`. |
| A window closed into the tray | `tree` shows `Root (hidden)`. |
| `dragto "summary.csv" "…"` | Prints `(drag file "…" out)`. |
