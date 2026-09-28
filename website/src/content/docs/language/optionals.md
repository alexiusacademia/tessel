---
title: Optionals and errors
description: How Tessel represents missing values with optionals (T?, nil, if let, ??, ?.), how functions report failure, and what happens on a runtime error.
---

Sometimes a value might be missing: a dictionary may not have the key you
ask for, text may not be a number, a file may not exist. Tessel represents
"a value or nothing" with **optionals**, and makes you deal with the nothing
case before you use the value. There is no `null` that can crash your
program by surprise.

## Optional types

Add `?` to a type to make it optional. An `Int?` is either an `Int` or
`nil`:

```tessel
var age: Int? = nil
age = 36
```

A plain value can always be stored in an optional, as `age = 36` shows. The
other way doesn't work: you can't use an `Int?` where an `Int` is needed,
because it might be `nil`:

<!-- error -->
```tessel
let age: Int? = 36
let nextYear: Int = age
```

```text
error: expected `Int`, found `Int?`
  = help: this value might be `nil`; unwrap it with `if let`, or give a default with `?? …`
```

There are three ways to get the value out: `if let`, `??` and `?.`.

## if let

`if let` runs a block only when there is a value, and names the unwrapped
value:

```tessel
let ages = ["ada": 36, "alan": 41]
if let age = ages["ada"] {
    print("Ada is {age}")
} else {
    print("No age for Ada")
}
```

```text
Ada is 36
```

Inside the braces, `age` is a plain `Int`. The name can be the same as the
optional's name, which is a common way to write it:

```tessel
let nickname: String? = "Countess"
if let nickname = nickname {
    print(nickname.uppercase())
}
```

The new name can have a type, written after a colon: `if let age: Int =
ages["ada"]`. It must be the type inside the optional. This is how you tell
[`fromJson`](/reference/stdlib/#fromjson) what to read:

```tessel
if let tags: [String] = fromJson("[\"red\", \"green\"]") {
    print(tags.count)
}
```

## ?? for a default

`a ?? b` is the value inside `a` if there is one, and `b` otherwise:

```tessel
let input = "twelve"
let count = Int(input) ?? 0
print(count)
```

```text
0
```

The result is a plain value, so this is often the shortest way to deal with
an optional. You can chain several: `first ?? second ?? 0` tries each in
turn.

`??` binds more loosely than `+` and `*`, so write
`(count ?? 0) + 1`, with parentheses, to add to the result.

## ?. for optional chaining

`a?.b` reads a field or calls a method only when `a` has a value. If `a` is
`nil`, the result is `nil`. Either way, the result is optional:

```tessel
struct User {
    name: String
    email: String? = nil
}

fn main() {
    var user: User? = User(name: "Ada", email: "ada@example.com")
    print(user?.name ?? "nobody")
    print(user?.email?.uppercase() ?? "no email")
    print(user?.name.count ?? 0)
    user = nil
    print(user?.name ?? "nobody")
}
```

```text
Ada
ADA@EXAMPLE.COM
3
nobody
```

Once one `?.` finds `nil`, the rest of the chain is skipped: in
`user?.name.count`, if `user` is `nil`, `.count` isn't evaluated, so it
doesn't need a second `?`. You need `?.` again only where a member is itself
optional, like `email` above.

### Changing a value through `?.`

A method that changes its value (like `append`, or a
[changing method](/language/structs-enums/#methods-that-change-the-value) of your own) also
works through `?.`. It changes the value inside the optional, and does nothing
if there is none:

```tessel
struct Counter {
    count: Int = 0

    fn add() {
        count += 1
    }
}

fn main() {
    var box: Counter? = Counter()
    box?.add()
    box?.add()
    print(box?.count ?? 0)

    var groups = ["fruit": ["apple"]]
    groups["fruit"]?.append("pear")    // changes the list in the dictionary
    groups["veg"]?.append("leek")      // no "veg" key, so nothing happens
    print(groups["fruit"]?.count ?? 0)
    print(groups.count)
}
```

```text
2
2
1
```

If the method returns a value, the result of the call is optional, as with
any `?.`.

## Comparing with nil

You can test an optional with `== nil` and `!= nil`, and compare it with a
plain value:

```tessel
var name: String? = nil
print(name == nil)
name = "Ada"
print(name != nil)
print(name == "Ada")
```

```text
true
true
true
```

Prefer `if let` when you want to use the value, since it unwraps it for you.

This works for optionals of any type, including optional functions:
`onDone == nil` checks whether a `fn()?` was given.

## Where optionals come from

Many built-in operations return an optional when they might not have an
answer:

| Expression | Type | `nil` when |
|---|---|---|
| `dict[key]` | `V?` | the key isn't in the dictionary |
| `list.first`, `list.last` | `T?` | the list is empty |
| `list.first(where: …)`, `list.last(where: …)` | `T?` | no item matches |
| `dict.removeValue(forKey: …)` | `V?` | the key wasn't there |
| `text.find("…")` | `Int?` | the text isn't found |
| `Int("…")`, `Float("…")` | `Int?`, `Float?` | the text isn't a number |
| `readFile(path: …)` | `String?` | the file can't be read |
| `listFolder(path: …)` | `[String]?` | the folder can't be read |
| `environment(name: …)` | `String?` | the variable isn't set |

Reading a list item with `list[i]` is **not** optional: an index that
doesn't exist is a mistake in the program, and stops it (see below).

## Reporting failure from your own functions

Tessel has no exceptions. A function that can fail returns an optional, and
returns `nil` to say it failed:

```tessel
fn parsePercent(text: String) -> Int? {
    if let n = Int(text) {
        if n >= 0 && n <= 100 {
            return n
        }
    }
    nil
}

fn main() {
    print(parsePercent(text: "42") ?? -1)
    print(parsePercent(text: "420") ?? -1)
    print(parsePercent(text: "lots") ?? -1)
}
```

```text
42
-1
-1
```

The caller can't forget to check: the result has to be unwrapped before it
can be used.

The built-in file functions work the same way. `readFile` returns `nil` if
the file can't be read, and functions like `writeFile` and `deleteFile`
return a `Bool` that says whether they worked:

```tessel
if let text = readFile("notes.txt") {
    print("The notes have {text.lines().count} lines")
} else {
    print("Could not read notes.txt")
}
```

:::note
An optional says *that* something failed, not *why*. Error values with a
reason are planned for a later version.
:::

## Runtime errors

Some mistakes can't be found by `tessel check`, because they depend on the
values the program works with. When one of these happens, the program stops
straight away and prints what went wrong and where:

```tessel
fn main() {
    let items = [1, 2, 3]
    print("before")
    print(items[5])
    print("after")
}
```

```text
before
error: index 5 is out of range for a list of 3 items
 --> main.tsl:4:11
```

The location is the file, line and column of the code that failed. Output
printed before the error still appears. The program exits with status code
101.

These are the runtime errors:

| Error | Caused by |
|---|---|
| `index 5 is out of range for a list of 3 items` | Reading, writing, inserting or removing a list item at an index that doesn't exist (including negative indexes) |
| `integer overflow` | An `Int` result too large or too small to fit, from `+`, `-`, `*`, `/` or `%` |
| `division by zero` | Dividing an `Int` by zero with `/` or `%` |
| `stack overflow` | Too many function calls inside each other, usually a function that calls itself without a case that stops it |

`Float` arithmetic never stops the program: dividing a `Float` by zero gives
infinity.

To avoid these errors, check before you act: compare an index with
`list.count`, or use `list.first` and `list.last`, which return optionals.
