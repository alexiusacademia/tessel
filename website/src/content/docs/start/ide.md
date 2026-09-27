---
title: The Tessel IDE
description: Edit, check, run and package Tessel programs in the Tessel IDE, which comes with the tessel command.
---

Tessel comes with its own small IDE: a file explorer, a tabbed code editor,
buttons to check, run and build your program, and panels for problems and
program output. You don't need a terminal or another editor to write Tessel
apps.

![The Tessel IDE: a toolbar with Save, Check, Run, Build and Package; an explorer on the left; a code editor showing todos.tsl; and a Problems panel below](../../../assets/screens/ide.png)

The IDE is itself a Tessel app. Its source is in the `ide/` folder of the
Tessel repository, and it's a good example of a larger Tessel program.

## Opening the IDE

Run `tessel` with no arguments to open the IDE on the current folder, or
name a folder:

```sh
tessel              # the current folder
tessel ide todos    # the folder `todos`
```

The folder is your project: the program the IDE checks, runs and builds.
The first time you open the IDE, `tessel` compiles it, which takes a moment
(`preparing the IDE (first run only)…`). After that it starts straight away,
until you update `tessel`.

When it opens, the IDE shows the first `.tsl` file of the project and
checks the program.

## The explorer

The left side lists the project's files and folders, folders first. Click a
folder to open or close it, and click a file to open it in the editor.
Files and folders whose names start with `.` are hidden.

The four buttons at the top of the explorer are:

| Button | What it does |
|---|---|
| New file | Asks for a name and creates an empty file. If the name has no extension, `.tsl` is added. |
| New folder | Asks for a name and creates a folder. |
| Rename | Renames the selected file or folder. Open tabs follow the renamed file. |
| Delete | Asks you to confirm, then deletes the selected file. Folders can't be deleted from the IDE. |

New files and folders go into the selected folder, or into the folder of the
selected file. Type the name and press Enter.

## The editor

Each open file gets a tab. The editor has:

- **Syntax colors** for Tessel code, and line numbers.
- **Error markers** on lines with errors. They are updated each time the
  program is checked: when the IDE opens, when you save, and when you press
  Check.
- **Undo and redo** with Cmd+Z and Cmd+Shift+Z.
- The usual text editing keys: select with the mouse or Shift and the arrow
  keys, copy, cut, paste, and select all. Tab inserts four spaces.

A tab whose file has unsaved changes shows a dot (`•`) before its name.
Closing a tab with its **×** button saves your changes first.

On Windows, use Ctrl wherever this page says Cmd.

## The toolbar

| Button | Shortcut | What it does |
|---|---|---|
| Save | Cmd+S | Saves every changed file, then checks the program again. |
| Check | Cmd+K | Checks the program for errors (like `tessel check`) and lists them under Problems. |
| Run / Stop | Cmd+R | Saves, builds and runs the program, showing its output in the Console. While it runs, the button becomes Stop. |
| Build | Cmd+B | Builds an executable into the project folder, named after the folder. |
| Package | | Like Build, but makes an app bundle (`Name.app` on macOS). |

Next to the buttons, a status text says what the IDE is doing ("Building…",
"Running", "2 problem(s)", and so on).

### Restart on save

With **Restart on save** switched on (it is on by default), saving while
your program is running stops it and runs it again, so you see your change
straight away. This restarts the program, so its state starts over; the
IDE doesn't yet keep an app's state across changes.

When Run builds your program, it writes the executable to a hidden file,
`.tessel-run`, in the project folder.

## Problems and console

The bottom of the window has two tabs:

- **Problems** lists the errors from the last check, with the file and line
  of each one. The tab title shows how many there are. Click a problem to
  jump to it: its file opens and the editor goes to that line.
- **Console** shows what the program prints while it runs, the output of
  Build and Package, and finally the exit code. Errors in the console show
  their location as ` --> file:line:column`; click such a line to jump to it.
  This works for compile errors and for runtime errors, such as an index
  out of range.

## Other editors

If you prefer VS Code or another editor, see [Editor support](/start/editors/).
Everything the IDE does is also available from the
[command line](/start/cli/).
