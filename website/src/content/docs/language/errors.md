---
title: Errors
description: Reporting and handling failures with a reason in Tessel - the built-in Result and Error types, try, and the standard functions that say why they failed.
---

Some things can fail: a file might not exist, text might not be a number,
JSON might not have the shape you expect. Tessel has two ways to say so:

- An **optional** (`T?`, see [Optionals](/language/optionals/)) says only
  *whether* there's a value: `readFile` gives `nil` if it can't read the file.
- A **`Result<T>`** also says *why* not: `readText` gives the text, or an
  `Error` like "can't read notes.txt: there's no such file or folder".

Use optionals when missing is normal and the reason doesn't matter, and
`Result` when the reason is worth showing or passing on.

## Result and Error

Every program has these two types built in:

```tessel
struct Error {
    message: String
}

enum Result<T> {
    ok(value: T)
    failure(error: Error)
}
```

`Error("…")` creates an error (the `message:` label is optional). A function
that can fail returns a `Result`, and says why when it does:

```tessel
fn parseAge(text: String) -> Result<Int> {
    if let n = Int(text) {
        if n < 0 {
            return .failure(error: Error("an age can't be negative"))
        }
        return .ok(value: n)
    }
    Result.failure(error: Error("\"{text}\" isn't a number"))
}
```

The caller handles both cases with `match`:

```tessel
match parseAge(text: input) {
    .ok(age) -> print("Age: {age}")
    .failure(e) -> print("Problem: {e.message}")
}
```

## Passing a failure on with `try`

`try` unwraps a `Result`: it gives the value if there is one, and otherwise
**returns the failure** from the function it's in. So a function that just
passes failures on to its caller reads like one that can't fail:

```tessel
struct Settings {
    name: String
    volume: Int = 5
}

fn loadSettings(path: String) -> Result<Settings> {
    let text = try readText(path)
    let settings: Settings = try decodeJson(text)
    if settings.volume > 10 {
        return .failure(error: Error("volume {settings.volume} is too loud"))
    }
    Result.ok(value: settings)
}

fn main() {
    writeFile(path: "loud.json", text: "\{\"name\": \"Max\", \"volume\": 11}")
    for path in ["loud.json", "missing.json"] {
        match loadSettings(path: path) {
            .ok(s) -> print("Hello, {s.name}")
            .failure(e) -> print("Couldn't load {path}: {e.message}")
        }
    }
}
```

```text
Couldn't load loud.json: volume 11 is too loud
Couldn't load missing.json: can't read missing.json: there's no such file or folder
```

`try` applies to the value right after it: `try parseInt(text) * 2` doubles
the number. The function using `try` must return a `Result` (of any type) or
an `Error?`; elsewhere, `tessel check` says so.

### In `fn main`

In `fn main`, there's no caller to return the failure to. So `try` prints the
error and stops the program with exit status 1, which suits small scripts:

```tessel
fn main() {
    let text = try readText("settings.txt")
    print(text)
}
```

```text
error: can't read settings.txt: there's no such file or folder
```

## Operations that give no value: `Error?`

Writing a file has no value to give back, only success or a reason. Such
functions return an `Error?`: `nil` if it worked. `try` works on those too,
and your own functions can return `Error?` the same way:

```tessel
fn saveAll(lines: [String], path: String) -> Error? {
    try writeText(path: path, text: "")
    for line in lines {
        try appendText(path: path, text: line + "\n")
    }
    nil
}

fn main() {
    if let e = saveAll(lines: ["a", "b"], path: "no/such/folder/out.txt") {
        print(e.message)
    }
}
```

```text
can't write no/such/folder/out.txt: there's no such file or folder
```

## Getting the value out

Besides `match` and `try`, a `Result` has helper methods:

| Method | Gives |
|---|---|
| `value()` | the value, or `nil` if it failed |
| `error()` | the `Error`, or `nil` if it worked |
| `isOk()` | whether it worked |
| `valueOr(fallback)` | the value, or `fallback` if it failed |

```tessel
fn main() {
    let r = parseInt("42")
    print(r.isOk())
    print(r.value() ?? 0)
    print(parseInt("x").valueOr(-1))
    if let e = parseInt("x").error() {
        print(e.message)
    }
}
```

```text
true
42
-1
"x" isn't a whole number
```

## Functions that say why they failed

| Function | Returns | Instead of |
|---|---|---|
| `readText(path)` | `Result<String>` | `readFile` |
| `writeText(path:text:)` | `Error?` | `writeFile` |
| `appendText(path:text:)` | `Error?` | `appendFile` |
| `readBytes(path)` | `Result<Data>` | `readData` |
| `writeBytes(path:data:)` | `Error?` | `writeData` |
| `parseInt(text)` | `Result<Int>` | `Int(text)` |
| `parseFloat(text)` | `Result<Float>` | `Float(text)` |
| `decodeJson(text)` | `Result<T>` | `fromJson` |
| `decodeBinary(data)` | `Result<T>` | `fromBinary` |

The reasons are written for people: "can't read notes.txt: there's no such
file or folder", "no permission", "\"x\" isn't a whole number", "this isn't
valid JSON: EOF while parsing an object at line 1 column 15". The functions
in the last column still work as before, returning optionals, for when the
reason doesn't matter. See the [standard library](/reference/stdlib/#reasons-for-failures)
for details.

## Your own Result or Error

These types, and the functions above, come from Tessel's **prelude**: a
small set of declarations every program includes. A program can declare its
own `Result` or `Error` instead (the ones it declares replace the built-in
ones, and the functions above aren't included), or its own function with one
of these names. `try` works only with the built-in `Result` and `Error`.
