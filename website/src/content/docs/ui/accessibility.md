---
title: Accessibility
description: How Tessel apps work with screen readers (VoiceOver, Narrator) and the keyboard, what you get without doing anything, and the few places where your app has to say more.
---

Some of the people using your app can't see its window, or can't use a
mouse. They use a **screen reader**, which says what's on screen and lets
them act on it from the keyboard: VoiceOver on macOS, Narrator on Windows.

Tessel draws every control itself, so to the system a window would be one
picture. Tessel therefore tells the system what's in it: every control and
text, what it's called, what it holds, and what can be done with it. Your
app does this without any code of yours.

## What you get

| View | What a screen reader is told |
|---|---|
| `Text` | Its words. |
| `Button` | A button, called by its title. It can be pressed. |
| `TextField` | A text field, called by its placeholder, with its text. A `secure` one is a password field, and its text isn't said. |
| `Toggle` | A switch, called by its label, on or off. |
| `Picker` | A group of options, and which one is chosen. |
| `TabView` | Its tabs, which one is chosen, and the page that shows. |
| `Slider`, `Stepper` | Its number, with the least and the most. It can be moved up and down. |
| `DisclosureGroup` | A button called by its title, open or closed. |
| `CodeEditor` | A text area with its text. |
| `Icon` | A picture called by its name: `Icon(.folderOpen)` is "folder open". |
| `Image`, `Canvas` | A picture or a drawing, called what you say (see below). |
| `Markdown` | A document with its text. |
| A view with `.onTap` | A button, called by the text in it (or its icon's name). |

Stacks and other views that only arrange things aren't mentioned: what's in
them is.

A control that's `.disabled` is said to be. A view that's `.hidden` isn't
there.

## Where your app has to say more

A screen reader can only say what a view is called if something says it.
Three kinds of view need you:

- **An icon that's pressed, whose name doesn't say what it does.**
  `Icon(.trash)` is called "trash", which is close; `Icon(.more)` isn't.
- **A slider or stepper**, which has no title of its own.
- **A picture or a drawing** that matters: a chart, a photo.

Say it with [`.accessibilityLabel`](/reference/modifiers/#accessibilitylabel):

```tessel
Icon(.more).onTap { showMenu() }.accessibilityLabel("More actions")
Slider(value: volume, from: 0, to: 1).accessibilityLabel("Volume")
Canvas { g in drawChart(g) }.accessibilityLabel("Sales by month: highest in June")
```

A view with a [`.tooltip`](/reference/modifiers/#tooltip) and no label is
called by its tooltip, so an icon button with a tooltip is already fine.

## The keyboard

Everything works without a mouse: Tab goes from control to control, Space
or Enter presses a button, and the arrows choose in a picker, a tab view, a
slider or a stepper. See
[The keyboard and controls](/ui/controls/#the-keyboard-and-controls).

For that, use a `Button` for what's pressed. A view with `.onTap` is a
button to a screen reader, but Tab doesn't stop on it.

## Checking your app

Turn the screen reader on and try the app with your eyes closed: Cmd+F5
starts VoiceOver on a Mac, and Ctrl+Win+Enter starts Narrator on Windows.

To see what's said without one, the
[`accessibility`](/ui/testing/#script-commands) script command prints it:

```sh
TESSEL_SCRIPT='accessibility' tessel run settings.tsl
```

```text
Window
  Text = "Settings"
  TextField "Name" = "Ada"
  Switch "Loud" (on)
  Options
    Option "Small" (chosen)
    Option "Large"
  Slider "Volume" = 0.5 (0 to 1)
  Button "More actions"
```

A `Button` or `Slider` with nothing after it has no name: give it a label.
The command works in tests too, so a test can keep the names there.

## What isn't there yet

- **Text, piece by piece.** A screen reader gets a field's whole text, but
  can't yet move through it by character or word, or type into it through
  its own commands (typing with the keyboard works as always).
- **Menus drawn in the window** (context menus, and the menu bar on
  Windows) aren't described. The menu bar on macOS is the system's own, and
  is.
- **Android.** Apps there don't tell TalkBack what's on screen yet.
- This was checked on macOS, through the interface VoiceOver uses. It's
  built for Windows too, and hasn't been tried with Narrator.
