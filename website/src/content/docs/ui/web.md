---
title: Web pages
description: Show a web page, a local HTML file or HTML of your own in an app with WebView, using the system's browser engine.
---

```text
WebView(url: String = "", html: String = "")
```

A `WebView` shows a web page with the system's browser engine: WebKit on
macOS (as Safari), and WebView2 on Windows (as Edge, which comes with
Windows 11 and nearly every Windows 10).

```tessel
app Docs(width: 800, height: 600) {
    state address = "https://tessel-lang.syncster.dev"
    state typed = "https://tessel-lang.syncster.dev"

    VStack(spacing: 0) {
        TextField("Address", text: typed)
            .onSubmit { address = typed }
            .padding(8)
        WebView(url: address)
    }
}
```

- **`url:`** is a web address (`https://…`), or the path of an HTML file
  (`"help/index.html"`), which can use the pictures and styles in its
  folder.
- **`html:`** is HTML your program made: a report, a preview, help text.
  With both, `url:` is shown.
- When `url` or `html` changes, the page loads again. The user can follow
  links in it.
- It takes all the space it's offered.

## Things to know

- The page is shown by a view of the system's, put over the window where
  the `WebView` is laid out. So it's always on top: popovers, sheets and
  menus drawn by Tessel don't show over it. Hide it while one is open, or
  open them beside it.
- In a scroll view, it isn't cut off at the scroll view's edges, so give it
  a place of its own rather than putting it in one.
- On Windows, the pages it visits keep their cookies and storage in a
  folder of the app's own, in the user's `AppData\Local`.
- In a [headless run](/ui/testing/) it shows nothing; `tree` says what it
  would load: `WebView "https://…"`.
