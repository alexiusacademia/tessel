---
title: Types
description: Tessel's built-in types (Int, Float, Bool, String, lists, dictionaries, optionals and functions), type inference, and converting between types.
---

Every value in Tessel has a type, and the type of every variable is known
before the program runs. `tessel check` uses this to catch mistakes, like
adding a number to a piece of text, before you ever run your code.

You rarely have to write types yourself: Tessel infers them from the values
you use. You do write them for function parameters, return values, struct
fields, and empty lists or dictionaries.

## Overview

| Type | Example values | What it holds |
|---|---|---|
| `Int` | `0`, `42`, `-7`, `1_000` | A whole number (64-bit) |
| `Float` | `0.5`, `3.14`, `2.0` | A decimal number (64-bit) |
| `Bool` | `true`, `false` | A truth value |
| `String` | `"hi"`, `"Hello, {name}"` | Text |
| `[T]` | `[1, 2, 3]` | A list of values of type `T` |
| `[K: V]` | `["ada": 36]` | A dictionary from keys `K` to values `V` |
| `T?` | `nil`, `5` | An optional: a `T`, or `nil` |
| `fn(A, B) -> R` | `{ a, b in a + b }` | A function |
| structs and enums | `Point(x: 1, y: 2)`, `.red` | Types you declare yourself |

For building user interfaces there are also `View` and a few built-in enums
such as `Color`, `Weight`, `Alignment` and `IconName`. Those are covered in
[Building apps](/ui/apps-views/).

## Int

An `Int` is a whole number from −9,223,372,036,854,775,808 to
9,223,372,036,854,775,807 (a 64-bit signed integer).

```tessel
let year = 2026
let big = 1_000_000
let negative = -40
```

`Int` arithmetic never silently wraps around. If a result is too large or too
small to fit, the program stops with an `integer overflow` error. Dividing by
zero (with `/` or `%`) also stops the program. See
[Runtime errors](/language/optionals/#runtime-errors).

Dividing two `Int`s gives an `Int` and drops the fraction: `7 / 2` is `3`.

## Float

A `Float` is a 64-bit floating-point number (a "double" in many languages).
A number literal with a decimal point is a `Float`.

```tessel
let pi = 3.14159
let half = 0.5
print(10.0 / 4.0)   // 2.5
print(2.0 * 3.0)    // 6.0
```

When a `Float` is printed or put into a string, a whole value keeps its
`.0` (so `6.0`, not `6`), which makes it easy to tell from an `Int`.

`Float` arithmetic follows the usual floating-point rules: it never stops the
program. Dividing by zero gives infinity, which prints as `inf`. And, as in
most languages, some decimal fractions can't be stored exactly:
`0.1 + 0.2` prints `0.30000000000000004`.

## Bool

A `Bool` is `true` or `false`. Comparisons produce `Bool`s, and `if`
conditions need one.

```tessel
let score = 72
let passed = score >= 50
if passed && score < 80 {
    print("Passed")
}
```

## String

A `String` is a piece of Unicode text. Strings support interpolation with
`{…}` and escapes such as `\n`; see [Basics](/language/basics/#strings).

```tessel
let name = "Ada"
let greeting = "Hello, {name}!"
print(greeting.count)      // 11
print(greeting.uppercase()) // HELLO, ADA!
```

`count` counts characters, not bytes, so `"héllo".count` is `5`. The full
list of string properties and methods is on the
[Collections](/language/collections/#strings) page.

Strings can't be indexed with `[…]`. Use methods like `substring`, `split`,
`contains` or `hasPrefix` instead.

## Lists

`[T]` is an ordered list whose items all have type `T`:

```tessel
let primes = [2, 3, 5, 7]       // [Int]
var names: [String] = []        // an empty [String]
names.append("Grace")
print(primes[0])                // 2
```

## Dictionaries

`[K: V]` maps keys of type `K` to values of type `V`:

```tessel
var ages = ["ada": 36, "alan": 41]   // [String: Int]
ages["grace"] = 85
print(ages["ada"] ?? 0)              // 36
```

Keys can be `Int`, `String`, `Bool`, or an enum whose cases carry no
values. Looking up a key gives an optional, because the key might be
missing.

Lists and dictionaries have many methods; see
[Collections](/language/collections/).

## Optionals

`T?` is "a `T`, or nothing". The nothing value is written `nil`:

```tessel
var nickname: String? = nil
nickname = "Countess"
print(nickname ?? "no nickname")
```

You can't use an optional as if it were a plain value. You unwrap it first
with `if let`, `??` or `?.`. See [Optionals and errors](/language/optionals/).

## Function types

Functions are values too. Their type is written with `fn`:

| Type | A function that… |
|---|---|
| `fn()` | takes nothing and returns nothing |
| `fn(Int) -> Bool` | takes an `Int` and returns a `Bool` |
| `fn(String, Int) -> String` | takes a `String` and an `Int`, returns a `String` |
| `fn() -> Int` | takes nothing and returns an `Int` |

```tessel
let isEven: fn(Int) -> Bool = { n in n % 2 == 0 }
print(isEven(4))   // true
```

To make a function type optional, the `?` goes at the end: `fn()?` is an
optional function. See [Closures](/language/closures/).

## Structs and enums

You can declare your own types:

```tessel
struct Point {
    x: Float
    y: Float
}

enum Suit { hearts, diamonds, clubs, spades }
```

See [Structs and enums](/language/structs-enums/).

## Type inference

Tessel works out a variable's type from its starting value:

```tessel
let count = 3              // Int
let price = 9.99           // Float
let names = ["a", "b"]     // [String]
let lookup = ["a": 1]      // [String: Int]
let isOn = count > 2       // Bool
```

Write the type after a colon when you want a different one, or when Tessel
can't tell:

```tessel
let ratio: Float = 1          // a Float, not an Int
var items: [String] = []      // an empty list has no items to look at
var cache: [String: Int] = [:]
var selected: Int? = nil      // nil alone doesn't say what's missing
```

Without the type, an empty list is an error:

<!-- error -->
```tessel
var items = []
```

```text
error: can't tell what type this empty list is
  = help: give it a type, like `var names: [String] = []`
```

Function parameters and return types are always written out. See
[Functions](/language/functions/).

## Converting between types

Tessel never converts between types on its own. A variable of type `Int` is
never used as a `Float`, or turned into text, unless you ask for it. You
convert with `Int(…)`, `Float(…)` and `String(…)`:

| Conversion | From | Result | Notes |
|---|---|---|---|
| `Float(x)` | `Int` or `Float` | `Float` | `Float(7)` is `7.0` |
| `Int(x)` | `Int` or `Float` | `Int` | Drops the fraction: `Int(3.99)` is `3`, `Int(-2.99)` is `-2` |
| `Int(text)` | `String` | `Int?` | `nil` if the text isn't a whole number |
| `Float(text)` | `String` | `Float?` | `nil` if the text isn't a number |
| `String(x)` | `Int`, `Float`, `Bool`, `String` or an enum | `String` | Same text as `"{x}"` |

```tessel
let count = 7
print(Float(count) / 2)          // 3.5
print(Int(3.99))                 // 3
print(Int("42") ?? 0)            // 42
print(Int("forty-two") ?? 0)     // 0
print(Float("2.5") ?? 0)         // 2.5
print(String(12) + " items")     // 12 items
```

Parsing text can fail, so `Int("…")` and `Float("…")` return optionals.
Spaces around the number are ignored, so `Int(" 12 ")` is `12`. `Int("12.5")`
is `nil`, because `12.5` is not a whole number.

`Int(x)` on a `Float` that is too large for an `Int` gives the largest (or
smallest) `Int` instead of stopping the program.

Mixing types by mistake gives an error that suggests the conversion:

<!-- error -->
```tessel
let count = 7
let half = count / 2.0
```

```text
error: can't use `/` on `Int` and `Float`
  = help: convert one side with `Float(…)` or `Int(…)`
```
