# {{title}}

An app made with [Tessel](https://tessel-lang.netlify.app).

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
| `main.tsl` | The app. Add more `.tsl` files to the folder as it grows: they're all part of the program. |
| `tessel.toml` | The version of Tessel the project uses, and the app's name, identifier and version. |

To give the app an icon, put an `icon.png` (1024 × 1024) next to
`main.tsl`. See the [Tessel documentation](https://tessel-lang.netlify.app)
to learn more.
