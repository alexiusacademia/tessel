---
title: Status and roadmap
description: What works in Tessel today, its known limitations, and what is planned next.
---

Tessel is at version 0.1. It's young: the core of the language and its UI
toolkit work, and they're enough to write real apps (the
[Tessel IDE](/start/ide/) is one), but plenty is missing, and details of the
language may still change. This page is an honest list of where things stand.

## What works today

- **The language:** structs and enums (with methods), functions and
  closures, lists and dictionaries, optionals, `if`, `for` and `match`, and
  value semantics with copy-on-write. Programs made of several files.
- **The compiler:** type checking with clear error messages, and native code
  through LLVM. Built apps are single executables that need nothing else
  installed.
- **Platforms:** macOS (11 and later) and Windows. Both are built and tested
  on every change. There's no Linux support yet.
- **UI:** windows, HiDPI screens, `state` and `bind`, stacks, `Scroll`,
  `Text`, `Button`, `TextField`, `Toggle`, `Picker`, `TabView`,
  `CodeEditor` with syntax colors, `Divider`, about 60 built-in icons, SVG,
  keyboard shortcuts, hover and pressed states, and the layout and styling
  modifiers.
- **Text editing:** selecting with the mouse and keyboard, word and line
  movement, copy, cut, paste, and undo and redo.
- **Background work:** timers (`after` and `every`), HTTP requests,
  downloads and WebSockets, running other programs, and heavy computations on
  another thread (`background`), all with completion blocks that run on the
  UI thread. They work in programs with only `fn main` too.
- **Standard library:** files and folders, JSON and binary encoding of your
  own types, `Data` for raw bytes, dates and times, random numbers, the
  clipboard, and many methods for strings, numbers and lists.
- **Tools:** the `tessel` command, the Tessel IDE, a VS Code extension, a
  language server for other editors, app bundles on macOS, and a headless
  mode for testing apps and taking screenshots.

## Known limitations

:::note
These are the main gaps you may run into today.
:::

- **Images:** `Image("file.svg")` draws SVG files, but PNG and JPEG images
  aren't loaded yet: they show as an empty placeholder box. Use SVG, `Svg`
  or the built-in [icons](/ui/icons/) for now.
- **Hot reload:** the IDE's "Restart on save" restarts your app when you
  save, so the app's state starts over. Reloading code while keeping the
  state isn't there yet.
- **Debugging:** there is no debugger with breakpoints or stepping yet.
  Runtime errors show a message and the place in your code, which you can
  click in the IDE's console, and `print` output appears there too.
- **Keyboard and input:** Tab doesn't move focus between text fields, and
  input methods (IME) for languages such as Chinese and Japanese aren't
  supported.
- **Appearance:** no dark mode, no animations, and no accessibility support
  (screen readers) yet.
- **Errors:** a function that fails returns `nil`, which says that it failed
  but not why.
- **Network:** WebSocket messages are text only (a binary message arrives
  as text), and there's no way to cancel a request or download once
  started.
- **Packaging:** `tessel build --bundle` makes a `.app` on macOS, but apps
  have no custom icon and aren't code signed.
- **Editor support:** the language server reports errors and shows types on
  hover, but doesn't offer completion or go-to-definition.
- **Installation:** packages exist for Apple silicon Macs and 64-bit
  Windows only (no Intel Macs yet). The macOS package is signed and
  notarized; the Windows one isn't signed yet, so Windows SmartScreen may
  warn about a downloaded `tessel.exe`.

## Planned

These are the next areas of work. None of them has a date.

| Area | Plans |
|---|---|
| Hot reload | Keep the running app's state when its code changes. |
| App polish | Dark mode, animations, loading PNG and JPEG images, Tab between fields, IME input, accessibility. |
| Standard library | Filling gaps as apps need them, such as time zones and more date handling. |
| Language | Errors that carry a reason, and generics for your own types. |
| Packaging | App icons, code signing and installers. |
| Debugging | Stepping through code with breakpoints, building on the clickable errors and console the IDE has today. |

Tessel's goals and the reasons for its design are on
[Tessel's design](/about/design/).
