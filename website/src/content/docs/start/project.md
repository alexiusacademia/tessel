---
title: The project file
description: "tessel.toml: the version of Tessel a project uses, and what the app says about itself: its name, identifier, version, author, icon, and the files it opens, with their icons."
---

A project can have a `tessel.toml` next to its code. It says which version
of Tessel the project uses, and what the app says about itself: its name,
version and author, its icon, and the kinds of files it opens. Tessel puts
that wherever an app shows it: in the app bundle on macOS, in the `.exe` on
Windows, in the About panel, and in `appInfo()` for the app's own use.

```toml
tessel = "0.1.35"

[app]
name = "Tally"
id = "com.example.tally"
version = "1.2.0"
build = "42"
description = "Spreadsheets for surveyors"
author = "Example Surveys"
copyright = "© 2026 Example Surveys"
icon = "assets/icon.png"
category = "productivity"

[[app.documents]]
name = "Tally Workbook"
extensions = ["tly"]
owned = true
icon = "assets/workbook.png"

[[app.documents]]
name = "Comma-separated values"
extensions = ["csv"]
role = "viewer"
```

Every part is optional; a project without a `tessel.toml` works as before.
[`tessel new`](/start/cli/#tessel-new) starts a project with one.
Tessel looks for the file next to the program, then in the folders above
it, so a project's subfolders share it. Commit it with the code.

## The version of Tessel

`tessel = "0.1.35"` makes the project use that version of Tessel, whatever
version is installed: commands and editors hand the project's work to it,
installing it the first time. [`tessel pin`](/start/cli/#tessel-pin) writes
this line for you, keeping the rest of the file.

## The app

The `[app]` table says what the app says about itself:

| Key | What it is | Default |
|---|---|---|
| `name` | The app's name, as people see it: the app bundle's and its menu's name on macOS, the product name on Windows, the window's title. It may have spaces. | The `app`'s name in the code |
| `id` | A name no other app has, written like a reversed web address: `com.example.tally`. macOS tells apps apart by it, so keep it the same from one version to the next. | `dev.tessel.` and the name |
| `version` | The version people see, like `1.2.0`. | `1.0` |
| `build` | A build number, for telling builds of one version apart. | The version |
| `description` | What the app is, in a few words. Windows shows it in the file's Details. | — |
| `author` | Who makes the app: a person or a company. | — |
| `copyright` | The copyright notice, like `© 2026 Example Surveys`. | — |
| `icon` | A square PNG, relative to `tessel.toml`. See [the app's icon](/start/cli/#the-apps-icon). | `icon.png` next to the code |
| `category` | Where the App Store and Finder file the app: `business`, `developer-tools`, `education`, `entertainment`, `finance`, `games`, `graphics-design`, `healthcare-fitness`, `lifestyle`, `medical`, `music`, `news`, `photography`, `productivity`, `reference`, `social-networking`, `sports`, `travel`, `utilities`, `video` or `weather`. | — |
| `license` | A text file of the app's license terms, relative to `tessel.toml`. The [Windows installer](/start/cli/#the-installer-of-a-windows-app) shows them first and installs only once they're accepted. | — |

On macOS, [`tessel build --bundle`](/start/cli/#tessel-build) writes these
into the bundle's `Info.plist` (and names the bundle after the app). On
Windows, `tessel build` puts them in the `.exe`, where its Properties show
them, and the [installer](/start/cli/#the-installer-of-a-windows-app)
(`--bundle`) lists the app under its name, version and `author` in
Settings, by its `id`. The About panel (from the app's menu on macOS) shows the name,
version and copyright.

## Android

An `[android]` table names the key that signs the app's Android packages:

```toml
[android]
keystore = "upload.jks"
key = "upload"
```

`keystore` is the keystore's path, relative to `tessel.toml`, and `key` is
the name (alias) of the key in it. Its password comes from the environment,
never from this file. See
[Publishing on Google Play](/guides/android/#publishing-on-google-play).

## Libraries

A `[dependencies]` table lists the [libraries](/guides/libraries/) the
project uses, and a `[library]` table makes the project a library others
can use:

```toml
[library]
name = "textkit"
version = "1.2.0"
description = "Text helpers"
license = "MIT"

[dependencies]
geokit = "0.3"
mine = { path = "../mine" }
```

`tessel add` and `tessel remove` edit `[dependencies]`; the versions
they come to are kept in `tessel.lock`, next to `tessel.toml`. Commit both.

## Documents

Each `[[app.documents]]` is a kind of file the app opens:

| Key | |
|---|---|
| `extensions` | The files' extensions, without the dot. |
| `name` | What the kind of file is called (Finder's "Kind", Explorer's "Type"). |
| `role` | `"editor"` (the default) for files the app changes, `"viewer"` for ones it only shows. |
| `owned` | `true` for the app's own kind of file, like a Tally workbook. |
| `icon` | For an owned kind: the files' icon, a PNG (1024 × 1024 is best), relative to `tessel.toml`. |

Files the app opens come to its [`.onOpenFile`](/ui/menus-windows/#opening-documents).
Extensions listed in the code's `app Name(documentTypes: […])` are added to
these, as kinds the app doesn't own.

### The app's own documents

A kind with `owned = true` is the app's: the app declares it to the system
(on macOS, in its bundle; on Windows, when [its installer](/start/cli/#the-installer-of-a-windows-app)
installs it). Then:

- double-clicking one of its files opens it with the app;
- its files show `icon`, or the app's icon when there's no `icon`;
- Finder and Explorer call them by `name`.

Mark only the app's own formats as owned. For kinds other apps make too,
like `csv` or `png`, leave `owned` out: the app is then offered for those
files (in **Open With** on macOS, **Open with** on Windows), and they keep
the app and the icon the user already has for them. `icon` without
`owned = true` is a mistake, which the build reports.

Owned types and icons are part of a built app: `tessel build --bundle` on
macOS, and the installer on Windows. Under `tessel run`, the app isn't
installed, so nothing is declared.

## In the app

[`appInfo()`](/reference/stdlib/#appinfo) gives the app's name, identifier,
version, build, description, author and copyright, with the same defaults,
for an About box of its own, or for telling a server which version is
asking:

```tessel
app Tally {
    state about = false

    VStack {
        Button("About") { about = true }
        if about {
            let info = appInfo()
            Text("{info.name} {info.version}")
            Text(info.copyright)
        }
    }
}
```

## Mistakes

A key Tessel doesn't know, or a value it can't use, is an error in every
command, with where it is:

```text
tessel: tessel.toml: TOML parse error at line 3, column 1
  |
3 | verison = "1.0"
  | ^^^^^^^
unknown field `verison`, expected one of `name`, `id`, `version`, `build`, …
```
