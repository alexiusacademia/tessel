---
title: "2. Values and variables"
description: Learn the four basic kinds of values in Tessel, how to give them names with let and var, and how to put them into text.
---

In the last lesson, your program printed text that you wrote directly into
the code. Real programs work with information that changes: a player's
score, the price of an item, the name someone typed in. In this lesson you'll
learn how a program remembers things.

In this lesson you'll learn:

- what values and types are
- how to give a value a name with `let`
- how to make a name whose value can change with `var`
- how to choose good names
- how to write down a type yourself
- how to put values into text with `{…}`
- why Tessel cares so much about types

## Values and types

A **value** is a single piece of information, such as the number `42` or the
text `"Tessel"`. Every value has a **type**, which says what kind of value it
is. Tessel has four basic types:

| Type | What it holds | Examples |
|---|---|---|
| `Int` | a whole number (short for *integer*) | `42`, `-7`, `0`, `1_000_000` |
| `Float` | a number with a decimal point | `3.14`, `2.0`, `-0.5` |
| `String` | text | `"Tessel"`, `"Hello, world!"`, `""` |
| `Bool` | true or false (short for *Boolean*) | `true`, `false` |

You can print all of them:

```tessel
fn main() {
    print(42)
    print(-7)
    print(3.14)
    print(2.0)
    print("Tessel")
    print(true)
}
```

```text
42
-7
3.14
2.0
Tessel
true
```

A few things to notice:

- `2.0` prints as `2.0`, not `2`. The `.0` shows that it's a `Float`.
- The quotes are not printed. They only tell Tessel where the text starts and
  ends.
- `1_000_000` is the same as `1000000`. The `_` is only there to make big
  numbers easier to read.

Why have both `Int` and `Float`? Some things can only be whole: you can't
have 2.5 people in a room or be on step 3.7 of a recipe. Others need
fractions, like a price or a temperature. Keeping them apart helps Tessel
catch mistakes, as you'll see at the end of this lesson.

Note that `5`, `5.0` and `"5"` are three different values. The first is an
`Int`, the second a `Float`, and the third is a `String`: the text made of the
single character `5`. They print the same way, but they are not the same
thing.

## Naming a value with `let`

You can give a value a name, and then use the name wherever you need the
value:

```tessel
fn main() {
    let name = "Ada"
    print("Hello!")
    print(name)
    print("Goodbye!")
    print(name)
}
```

```text
Hello!
Ada
Goodbye!
Ada
```

`let name = "Ada"` means "from now on, `name` stands for `"Ada"`". This is
called a **constant**: once it has a value, the value never changes.

Look at the difference between `print(name)` and `print("name")`. Without
quotes, `name` is a name, and Tessel prints the value it stands for: `Ada`.
With quotes, `"name"` is just the text `name`.

Why bother naming values? Two reasons:

- **To avoid repeating yourself.** If Ada changes her name, you change one
  line, not every place it's printed.
- **To make code readable.** `let hoursPerDay = 24` tells the reader what the
  number means. A bare `24` doesn't.

## Values that change: `var`

Sometimes a value needs to change while the program runs, like a score in a
game. For that, use `var` instead of `let`. A `var` is a **variable**:

```tessel
fn main() {
    var score = 0
    print(score)
    score = 10
    print(score)
    score = score + 5
    print(score)
}
```

```text
0
10
15
```

`score = 10` puts a new value into `score`. The old value is gone.

`score = score + 5` may look strange if you know maths, where it would make no
sense. In programming, `=` doesn't mean "is equal to". It means "store this
value". Tessel first works out the right side (`score + 5`, which is
`10 + 5`, so `15`), and then stores the result in `score`.

Think of a variable as a labeled box. The label is the name; the thing inside
is the value. You can take out the old thing and put in a new one, but the
box keeps its label.

### Common mistake: changing a `let`

If you try to change a constant, Tessel stops you:

```tessel
fn main() {
    let name = "Ada"
    name = "Grace"
    print(name)
}
```

```text
error: can't change `name`, which is a constant
 --> values.tsl:3:5
  |
3 |     name = "Grace"
  |     ^^^^
 ::: values.tsl:2:9
  |
2 |     let name = "Ada"
  |         ---- declared here
  |
  = help: declare it with `var` to be able to change it

1 error found
```

The message points at both places: the line that tries to change `name`, and
the line where `name` was declared (created) with `let`. The fix is in the
help line: write `var name = "Ada"`.

### `let` or `var`?

Use `let` unless you know the value needs to change. It may seem simpler to
use `var` everywhere, but `let` is a promise: "this value stays the same".
When you read a long program, knowing that something can't change makes it
much easier to follow. And if you change it by accident, Tessel tells you.

## Choosing names

Names can contain letters, digits and `_`, but they can't start with a digit
and can't contain spaces. When a name is made of several words, the usual
style in Tessel is to start each word after the first with a capital letter:
`highScore`, `firstName`, `hoursPerDay`. This is called *camel case*,
because the capitals stick up like the humps of a camel.

| Name | OK? |
|---|---|
| `score`, `highScore`, `player2` | yes |
| `2ndPlace` | no: starts with a digit |
| `first name` | no: contains a space |
| `let`, `var`, `fn`, `if` | no: these are **keywords**, words that already mean something in Tessel |

Good names say what the value *means*. `price` is better than `p`, and
`secondsLeft` is better than `number`.

Capital letters matter. `userName` and `username` are two different names:

```tessel
fn main() {
    let userName = "Ada"
    print(username)
}
```

```text
error: cannot find `username`
 --> values.tsl:3:11
  |
3 |     print(username)
  |           ^^^^^^^^ not found
  |
  = help: did you mean `userName`?

1 error found
```

### Common mistake: using a name before creating it

A name has to be created with `let` or `var` before you can use it. If you
forget, Tessel doesn't know what you mean:

```tessel
fn main() {
    score = 10
    print(score)
}
```

```text
error: can't set `score`: it hasn't been created
 --> values.tsl:2:5
  |
2 |     score = 10
  |     ^^^^^ not created yet
  |
  = help: create it first with `var`, like `var score = …`

1 error found
```

`score = 10` on its own means "change `score` to 10", but there is no
`score` to change yet. The fix is what the help says: create the variable the
first time with `var score = 10`. After that, `score = …` changes it.

## Writing down the type

You may have noticed that you never told Tessel what type `score` or `name`
is. Tessel works it out from the starting value: `0` is an `Int`, so `score`
is an `Int`. This is called **type inference**.

You can also write the type yourself, after the name and a colon. This is a
**type annotation**:

```tessel
fn main() {
    let price: Float = 5
    let count: Int = 3
    let title: String = "Coffee"
    let inStock: Bool = true
    print(price)
    print("{count} x {title}, in stock: {inStock}")
}
```

```text
5.0
3 x Coffee, in stock: true
```

Most of the time you don't need annotations. They're useful when the value
alone doesn't say what you mean. Above, `5` on its own would be an `Int`, but
`price: Float` says you want a decimal number, so `price` is `5.0`.

Every `let` and `var` needs a starting value, even when you write the type.
If you write just `var total: Int`, Tessel says ``error: `total` needs a
starting value`` and suggests `var total: Int = 0`.

## Putting values into text

You've already seen this line:

```tessel
print("{count} x {title}, in stock: {inStock}")
```

Inside a string, `{` and `}` mark a spot where a value goes. Tessel replaces
`{count}` with the value of `count`. This is called **string
interpolation**. You can put any small calculation between the braces, too:

```tessel
fn main() {
    let name = "Ada"
    let age = 36
    print("{name} is {age} years old.")
    print("Next year, {name} will be {age + 1}.")
    print("Is {name} an adult? {age >= 18}")
}
```

```text
Ada is 36 years old.
Next year, Ada will be 37.
Is Ada an adult? true
```

(`age >= 18` asks "is `age` at least 18?". The answer is a `Bool`. You'll
learn more about questions like this in [lesson 4](/learn/04-decisions/).)

Because `{` has this special meaning, you need a trick to print a real brace:
put a backslash in front of it, `\{`. The same trick works for a double
quote inside a string, `\"`:

```tessel
fn main() {
    print("Use \{name\} to insert")
    print("She said \"hi\"")
}
```

```text
Use {name} to insert
She said "hi"
```

If you forget the closing `}`, Tessel tells you:

```text
error: this `{` in a string is never closed
 --> values.tsl:3:19
  |
3 |     print("Hello, {name!")
  |                   ^
  |
  = help: close it with `}`, or write \{ for a literal brace

1 error found
```

You can also join two strings with `+`: `"Ada" + " " + "Lovelace"` is
`"Ada Lovelace"`. But interpolation is usually shorter and easier to read.

## Why types matter

Tessel is a **statically typed** language. That means every name has one
type, decided before the program runs, and the type never changes. If
`age` starts as an `Int`, it's an `Int` forever.

This lets the compiler catch mistakes early. Try to put text into a number
variable:

```tessel
fn main() {
    var age = 36
    age = "thirty-seven"
}
```

```text
error: expected `Int`, found `String`
 --> values.tsl:3:11
  |
3 |     age = "thirty-seven"
  |           ^^^^^^^^^^^^^^ this is `String`

1 error found
```

`age` is an `Int`, so Tessel expected an `Int`, but it found a `String`.

Why is this helpful? Imagine that later in the program you calculate
`age + 1`. With a number, that's easy. With the text `"thirty-seven"`, it
makes no sense. Some languages would only notice when the program is already
running, maybe in front of a user. Tessel refuses to compile the program, so
the mistake never reaches anyone.

### Common mistake: adding text and a number

Here's a mistake almost everyone makes at first:

```tessel
fn main() {
    let age = 36
    print("Age: " + age)
}
```

```text
error: can't use `+` on `String` and `Int`
 --> values.tsl:3:11
  |
3 |     print("Age: " + age)
  |           ^^^^^^^^^^^^^
  |
  = help: to put a value into text, use "{…}", like "Total: {total}"

1 error found
```

`+` can add two numbers, or join two strings, but not one of each. The help
line shows the fix: `print("Age: {age}")`.

## Exercises

**1. About you.** Make three constants: your name, your city and your favorite
number. Then print three sentences that use them, like "My name is Sam.".

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let name = "Sam"
    let city = "Lisbon"
    let favoriteNumber = 7
    print("My name is {name}.")
    print("I live in {city}.")
    print("My favorite number is {favoriteNumber}.")
}
```

```text
My name is Sam.
I live in Lisbon.
My favorite number is 7.
```

</details>

**2. Fix the game.** This program should show a player losing a life, but it
doesn't compile. Read the error and fix it.

```tessel
fn main() {
    let lives = 3
    print("Lives: {lives}")
    lives = lives - 1
    print("Ouch! Lives: {lives}")
}
```

<details>
<summary>Solution</summary>

Tessel says ``can't change `lives`, which is a constant``. The number of
lives has to change, so it must be a `var`:

```tessel
fn main() {
    var lives = 3
    print("Lives: {lives}")
    lives = lives - 1
    print("Ouch! Lives: {lives}")
}
```

```text
Lives: 3
Ouch! Lives: 2
```

</details>

**3. One of each.** Write a program about a book with four constants, one of
each basic type: the title (`String`), the number of pages (`Int`), a rating
like 4.5 (`Float`) and whether you've finished it (`Bool`). Write each type
as an annotation. Print everything in one line.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let title: String = "The Hobbit"
    let pages: Int = 310
    let rating: Float = 4.5
    let finished: Bool = false
    print("{title}: {pages} pages, rated {rating}, finished: {finished}")
}
```

```text
The Hobbit: 310 pages, rated 4.5, finished: false
```

</details>

**4. Shopping cart.** A shop sells a T-shirt for 20 coins. Make a constant for
the price and a variable for how many are in the cart, starting at 1. Print a
line with the number of shirts and the total price. Then add 2 more shirts to
the cart and print the line again. Use `{…}` to do the multiplication inside
the text (`*` means "times").

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let product = "Tessel T-shirt"
    let price = 20
    var inCart = 1
    print("{inCart} x {product}, total {inCart * price} coins")
    inCart = inCart + 2
    print("{inCart} x {product}, total {inCart * price} coins")
}
```

```text
1 x Tessel T-shirt, total 20 coins
3 x Tessel T-shirt, total 60 coins
```

The price never changes, so it's a `let`. The number in the cart does, so
it's a `var`.

</details>

**5. Swap.** You have two variables, `left` holding `"apple"` and `right`
holding `"banana"`. Swap them, so that `left` holds `"banana"` and `right`
holds `"apple"`. You may not type `"apple"` or `"banana"` again: use only the
variables. Hint: if you do `left = right` first, the apple is lost. Where
could you keep it for a moment?

<details>
<summary>Solution</summary>

Keep a copy of one value in a third name before overwriting it:

```tessel
fn main() {
    var left = "apple"
    var right = "banana"
    print("Before: left is {left}, right is {right}")
    let temporary = left
    left = right
    right = temporary
    print("After: left is {left}, right is {right}")
}
```

```text
Before: left is apple, right is banana
After: left is banana, right is apple
```

It's like swapping the drinks in two full glasses: you need a third, empty
glass.

</details>

## Summary

- A **value** is a piece of information. Its **type** says what kind: `Int`
  (whole number), `Float` (decimal number), `String` (text) or `Bool`
  (`true`/`false`).
- `let name = value` makes a **constant**: a name for a value that never
  changes.
- `var name = value` makes a **variable**, which you can change later with
  `name = newValue`. Prefer `let` unless the value must change.
- `=` means "store", not "is equal to".
- Names use letters, digits and `_`, don't start with a digit, and are usually
  written in camel case, like `highScore`. Capitals matter.
- Tessel works out types by itself, or you can write them: `let price: Float = 5`.
- `"{name}"` puts a value into text. Write `\{` for a real brace.
- Every name has one type forever. Tessel checks this before running, which
  catches many mistakes early.

See also: [Basics](/language/basics/) and [Types](/language/types/) in the
language guide.

**Next:** [3. Arithmetic](/learn/03-arithmetic/)
