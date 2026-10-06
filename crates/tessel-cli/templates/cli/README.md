# {{title}}

A command-line program made with [Tessel](https://tessel-lang.netlify.app).

## Running it

```sh
tessel run . Ana Ben    # build it and run it, with two arguments
```

## Building it

```sh
tessel build .
```

makes `{{folder}}` (`{{folder}}.exe` on Windows), which runs on computers
without Tessel:

```sh
./{{folder}} Ana Ben
```

## What's where

| File | |
|---|---|
| `main.tsl` | The program: it starts at `fn main`. Add more `.tsl` files to the folder as it grows: they're all part of the program. |
| `tessel.toml` | The version of Tessel the project uses. |

See the [Tessel documentation](https://tessel-lang.netlify.app) to learn
more.
