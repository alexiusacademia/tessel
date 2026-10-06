# {{title}}

A to-do list app made with [Tessel](https://tessel-lang.netlify.app): add
tasks, tick them off, and they're still there the next time it opens.

## Running it

```sh
tessel run .          # build and run it
tessel run . --hot    # and take changes to the code as you save
tessel ide            # or work on it in the Tessel IDE
```

## Making the app

```sh
tessel build . --bundle
```

makes `{{title}}.app` on macOS and `{{folder}}.exe` on Windows, which run
on computers without Tessel. (The IDE's Package button does the same.)

## What's where

| File | |
|---|---|
| `main.tsl` | The app: its window, its menus, and what the buttons do. |
| `task.tsl` | The data: a task, the filter, and saving the tasks as JSON. |
| `views.tsl` | `TaskRow`, a view of its own for showing one task. |
| `tessel.toml` | The version of Tessel the project uses, and the app's name, identifier and version. |

Every `.tsl` file in the folder is part of the program: names declared in
one are seen in the others. To give the app an icon, put an `icon.png`
(1024 × 1024) next to `main.tsl`.

See the [Tessel documentation](https://tessel-lang.netlify.app) to learn
more.
