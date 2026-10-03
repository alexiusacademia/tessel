---
title: Basics
description: Comments, fn main, print, constants and variables, literals, string interpolation, operators and how lines end in Tessel.
---

This page covers the pieces every Tessel program is made of: comments,
printing, constants and variables, literal values, and operators. If you have
written code in Swift, Kotlin, JavaScript or a similar language, most of it
will look familiar.

## A first program

```tessel
// Say hello, then do a little arithmetic.
fn main() {
    let name = "Tessel"
    print("Hello, {name}!")

    var total = 0
    for i in 0..5 {
        total += i
    }
    print("The sum of 0..5 is {total}")
}
```

Run it with `tessel run hello.tsl`, and it prints:

```text
Hello, Tessel!
The sum of 0..5 is 10
```

A program without a window starts at `fn main()`. It takes no parameters and
returns nothing. (A program with a window starts at an `app` instead; see
[Programs and files](/language/programs/).)

`print` writes one value to the terminal, followed by a new line. It accepts
an `Int`, `Float`, `Bool`, `String` or an enum value. To print several
values, put them in one string with `{…}`, as shown below.

## Comments

A comment starts with `//` and runs to the end of the line. It can be on its
own line or after code:

```tessel
// This whole line is a comment.
let width = 20 // and this is a comment after some code
```

There are no block comments (`/* … */`). To comment out several lines, put
`//` in front of each one.

## Constants and variables

Use `let` for a value that never changes, and `var` for one that does:

```tessel
let name = "Ada"   // a constant
var score = 0      // a variable
score = 10
score += 5
print("{name} has {score} points")
```

Every `let` and `var` needs a starting value. Tessel works out the type from
that value, so `score` above is an `Int`. You can also write the type
yourself after a colon:

```tessel
let ratio: Float = 3      // 3 is read as 3.0 here
var names: [String] = []  // an empty list needs its type written out
```

Changing a `let` is an error that tells you to use `var` instead:

<!-- error -->
```tessel
let limit = 10
limit = 20
```

```text
error: can't change `limit`, which is a constant
  = help: declare it with `var` to be able to change it
```

Variables live inside functions (and `state` lives inside views). There are
no global variables: a `let` or `var` at the top level of a file is an
error. If you need a value that the whole program shares, write a function
that returns it:

```tessel
fn maxPlayers() -> Int {
    4
}
```

Names are made of letters, digits and `_`, and can't start with a digit. The
usual style is `lowerCamelCase` for variables and functions, and
`UpperCamelCase` for types and views.

## Literals

| Kind | Examples | Type |
|---|---|---|
| Whole numbers | `0`, `42`, `-7`, `1_000_000`, `0xff`, `0b1010`, `0o17` | `Int` |
| Decimal numbers | `3.14`, `0.5`, `2.0`, `1_000.25`, `1e6`, `2.5e-3` | `Float` |
| Truth values | `true`, `false` | `Bool` |
| Text | `"hello"`, `"Total: {n}"` | `String` |
| No value | `nil` | an optional, see [Optionals](/language/optionals/) |
| Lists | `[1, 2, 3]`, `[]` | `[Int]` |
| Dictionaries | `["a": 1]`, `[:]` | `[String: Int]` |

You can put `_` anywhere in a number to make it easier to read:
`1_000_000` is one million.

A decimal number needs digits on both sides of the point: write `0.5`, not
`.5`, and `2.0`, not `2.`.

A number with an exponent is a `Float`: `1e6` is `1000000.0`, `2.5e-3` is
`0.0025` and `1E+9` is `1000000000.0` (`e` or `E`, then an optional sign).

Whole numbers can also be written in hexadecimal (`0x`), binary (`0b`) or
octal (`0o`), which is handy for bytes, colors and
[bit operations](#bit-operations). The prefix is lowercase; the digits can be
either case:

```tessel
print(0xff)          // 255
print(0xFF_FF)       // 65535
print(0b1010)        // 10
print(0o17)          // 15
print(1e6)           // 1000000.0
print(2.5e-3)        // 0.0025
```

A whole-number literal takes its type from where it is used. Where a `Float`
is expected, `3` means `3.0`, so you rarely have to write `.0`:

```tessel
let width: Float = 3
print(width)     // 3.0
print(1 + 2.5)   // 3.5
```

This only applies to literals. A variable never changes type by itself; see
[Types](/language/types/#converting-between-types) for converting between
`Int` and `Float`.

## Strings

A string is text between double quotes. It must end on the same line it
starts on.

### Interpolation

Put any expression inside `{` and `}` to insert its value into the text:

```tessel
let apples = 3
let price = 2
print("{apples} apples cost {apples * price} coins")
```

```text
3 apples cost 6 coins
```

You can interpolate an `Int`, `Float`, `Bool`, `String`, `Range` or enum
value.
Lists, structs and optionals can't go in directly; for example, use
`{list.count}`, `{names.joined(separator: ", ")}` or `{nickname ?? "none"}`
instead.

### Escapes

A backslash starts an escape sequence:

| Escape | Means |
|---|---|
| `\n` | new line |
| `\t` | tab |
| `\r` | carriage return |
| `\"` | a double quote |
| `\\` | a backslash |
| `\{` and `\}` | a literal brace, not an interpolation |

```tessel
print("Line one\nLine two")
print("She said \"hi\"")
print("Path: C:\\temp")
print("Use \{name\} to insert a value")
```

```text
Line one
Line two
She said "hi"
Path: C:\temp
Use {name} to insert a value
```

Any other character after a backslash is an error.

## Operators

### Arithmetic

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add | `7 + 2` | `9` |
| `-` | subtract | `7 - 2` | `5` |
| `*` | multiply | `7 * 2` | `14` |
| `/` | divide | `7 / 2` | `3` |
| `%` | remainder | `7 % 2` | `1` |
| `-x` | negate | `-7` | `-7` |

Both sides must have the same type. Dividing two `Int`s gives an `Int` and
drops the fraction (it rounds toward zero, so `-7 / 2` is `-3`). For a
decimal result, convert first: `Float(7) / 2` is `3.5`. `%` works on
`Float`s too: it's what is left after taking away as many whole multiples
of the right side as fit, with the sign of the left side (like Swift's
`truncatingRemainder` and `%` in Rust or C):

```tessel
print(7.5 % 2.0)      // 1.5
print(-7.5 % 2.0)     // -1.5
print(370.0 % 360.0)  // 10.0
```

`+` also joins two strings or two lists:

```tessel
let greeting = "Hello, " + "world"
let numbers = [1, 2] + [3]
```

`Int` arithmetic is checked. If a result doesn't fit in an `Int`, or you
divide an `Int` by zero, the program stops with an error; see
[Runtime errors](/language/optionals/#runtime-errors).

### Comparison

| Operator | Meaning |
|---|---|
| `==` | equal |
| `!=` | not equal |
| `<` `<=` | less than, less than or equal |
| `>` `>=` | greater than, greater than or equal |

`==` and `!=` work on almost every type, including strings, lists, structs,
enums and optionals. `<`, `<=`, `>` and `>=` work on numbers and strings
(strings compare in dictionary order, so `"apple" < "banana"`).

Comparisons can't be chained: write `a < b && b < c`, not `a < b < c`.

### Logic

| Operator | Meaning |
|---|---|
| `&&` | and: true if both sides are true |
| <code>&#124;&#124;</code> | or: true if either side is true |
| `!` | not: flips `true` and `false` |

```tessel
let age = 30
let member = false
print(age >= 18 && !member)
```

The operands must be `Bool`s. Tessel doesn't treat `0` or `""` as false.

### Bit operations

These work on the bits of `Int`s (in two's complement, as in most
languages):

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `&` | and: bits set in both | `0b1100 & 0b1010` | `8` (`0b1000`) |
| <code>&#124;</code> | or: bits set in either | <code>0b1100 &#124; 0b1010</code> | `14` (`0b1110`) |
| `^` | exclusive or: bits set in one but not both | `0b1100 ^ 0b1010` | `6` (`0b0110`) |
| `~x` | not: every bit flipped | `~5` | `-6` |
| `<<` | shift left | `1 << 10` | `1024` |
| `>>` | shift right, keeping the sign | `-16 >> 2` | `-4` |

```tessel
let (r, g, b) = (0x12, 0x34, 0x56)
let color = r << 16 | g << 8 | b
print(color)                        // 1193046
print(color >> 8 & 0xff)            // 52

let bold = 1 << 0
let italic = 1 << 1
var style = bold | italic
style &= ~bold
print(style & italic != 0)          // true
print(style & bold != 0)            // false
```

- Shifting by 64 or more gives `0` (or, for `>>` of a negative number,
  `-1`). Shifting by a negative amount stops the program with an error.
- `<<` doesn't check for overflow: bits shifted past the top are lost, so
  `1 << 63` is the smallest `Int`.
- On `Bool`s, use `&&`, `||` and `!=` instead: `a & b` on two `Bool`s is an
  error that suggests `&&`.

### Assignment

| Operator | Same as |
|---|---|
| `x = v` | set `x` to `v` |
| `x += v` | `x = x + v` (numbers, strings, lists and `Data`) |
| `x -= v` | `x = x - v` |
| `x *= v` | `x = x * v` |
| `x /= v` | `x = x / v` |
| `x %= v` | `x = x % v` (`Int` and `Float`) |
| `x &= v`, <code>x &#124;= v</code>, `x ^= v` | `x = x & v`, … (`Int`) |
| `x <<= v`, `x >>= v` | `x = x << v`, … (`Int`) |

Assignment is a statement, not an expression: you can't write
`a = b = 0` or use `x += 1` as a value. The left side can be a variable, a
field (`player.score += 1`) or an item (`scores[0] = 10`).

### Ranges

`a..b` is the range of whole numbers from `a` up to, but not including, `b`.
You use it with `for`:

```tessel
for i in 0..3 {
    print(i)   // prints 0, 1, 2
}
```

If `b` is not greater than `a`, the range is empty and the loop body doesn't
run. There is no inclusive range operator; write `1..(n + 1)` to include
`n`.

A range is a value like any other, of type `Range`. You can store it in a
variable, pass it to a function, compare two ranges with `==`, and print it:

```tessel
let lines = 2..5
for i in lines {
    print(i)          // prints 2, 3, 4
}
print(lines)          // 2..5
print(lines == 2..5)  // true
```

To repeat something without using the number, name the loop variable `_`:
`for _ in 0..3 { … }`.

### Other operators

These are covered on their own pages:

- `??` gives a default for a missing optional value, and `?.` reads a member
  only if there is a value. See [Optionals](/language/optionals/).
- `.` reads a field or calls a method, `()` calls a function, and `[]` reads
  an item from a list or dictionary.

### Precedence

From lowest to highest. Operators on the same row are applied left to right,
except `??`, which groups to the right.

| Operators |
|---|
| <code>&#124;&#124;</code> |
| `&&` |
| `==` `!=` `<` `<=` `>` `>=` |
| `..` |
| `??` |
| `+` `-` <code>&#124;</code> `^` |
| `*` `/` `%` `&` |
| `<<` `>>` |
| `!` `-` `~` (prefix) |
| `.` `?.` `()` `[]` |

The bit operators sit where Swift puts them: shifts bind tighter than `*`,
`&` is like `*`, and `|` and `^` are like `+`. So `1 + 2 << 3` is
`1 + (2 << 3)`, and `flags & mask == 0` is `(flags & mask) == 0`. Use
parentheses when you mix `|` or `^` with `+` or `-`, which group left to
right with them: `1 | 2 + 4` is `(1 | 2) + 4`.

Because `??` is lower than `+`, `count ?? 0 + 1` would mean
`count ?? (0 + 1)`, which is rarely what's meant. So Tessel asks for
parentheses when `??` is followed by arithmetic: write `(count ?? 0) + 1`, or
`count ?? (0 + 1)` if that's what you mean.

## Lines and semicolons

A statement ends at the end of its line. Tessel doesn't use semicolons; a
`;` is an error. Put each statement on its own line.

A statement can still span several lines:

- while a `(` or `[` is open,
- after an operator such as `+`, `&&` or `=`,
- when the next line starts with `.`, which continues a chain of calls.

```tessel
let total = 1 +
    2 +
    3
let names = [
    "Ada",
    "Grace",
]
let shout = names
    .joined(separator: " and ")
    .uppercase()
print(shout)
```

A trailing comma after the last item in a list, like `"Grace",` above, is
allowed.
