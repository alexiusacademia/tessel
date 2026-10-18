# {{lib}}

A library for [Tessel](https://tessel-lang.netlify.app) programs.

## Using it

```sh
tessel add {{lib}}
```

then, in the program:

```tessel
import {{lib}}

fn main() {
    print({{lib}}.greet("Ann"))
}
```

## Publishing it

The library is published from its git repository, on GitHub or another
git host. Commit the code, then:

```sh
tessel publish --dry-run   # check it first
tessel publish             # tag the version, push the tag, propose it to the index
```

Before the next version, raise `version` in `tessel.toml`.

## What's where

| File | |
|---|---|
| `{{lib}}.tsl` | The library's code. Add more `.tsl` files to the folder as it grows. |
| `tessel.toml` | The library's name, version, description and licence, and the libraries it uses. |
