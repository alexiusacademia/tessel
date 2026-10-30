---
title: Control flow
description: Making decisions and repeating work in Tessel with if, if let, for loops and match.
---

Tessel has three control flow constructs: `if` for decisions, `for` for
loops, and `match` for choosing between many cases. They work the same in
functions and in views.

## if and else

```tessel
let temperature = 25
if temperature > 30 {
    print("Hot")
} else if temperature > 15 {
    print("Pleasant")
} else {
    print("Cold")
}
```

```text
Pleasant
```

The condition must be a `Bool`, and needs no parentheses. The braces around
each body are required, even for a single line. `else` can go on the same
line as the closing brace or at the start of the next line.

### if as a value

`if` is an expression: when every branch ends with a value, the whole `if`
produces that value.

```tessel
let score = 85
let grade = if score >= 90 { "A" } else if score >= 80 { "B" } else { "C" }
print(grade)
```

```text
B
```

Every branch must produce the same type, and there must be an `else`, so
that there is always a value. This is Tessel's replacement for the
`condition ? a : b` operator of other languages.

## if let

`if let` checks whether an optional has a value, and if so, gives you the
value under a new name:

```tessel
let input = "42"
if let number = Int(input) {
    print("Twice that is {number * 2}")
} else {
    print("{input} is not a number")
}
```

```text
Twice that is 84
```

Inside the first block, `number` is a plain `Int`. It only exists there. You
can chain `else if let` and `else if` as usual. See
[Optionals](/language/optionals/) for more.

You can also write the type of the new name, after a colon. Usually that's
not needed, but it is for [`fromJson`](/reference/stdlib/#fromjson) and
[`fromBinary`](/reference/stdlib/#frombinary), which find out what to read
from the type they're given:

```tessel
if let scores: [Int] = fromJson("[90, 75, 82]") {
    print("Best score: {scores.max() ?? 0}")
}
```

```text
Best score: 90
```

The type is the unwrapped type (`[Int]`, not `[Int]?`).

### Taking a tuple apart

When the optional holds a [tuple](/language/collections/#tuples), `if let`
can name its items directly, as `let (a, b) = pair` does. `_` skips an item:

```tessel
fn setting(_ line: String) -> (String, Int)? {
    let parts = line.split("=")
    if parts.count != 2 {
        return nil
    }
    if let value = Int(parts[1]) {
        return (parts[0], value)
    }
    return nil
}

fn main() {
    if let (name, value) = setting("width=40") {
        print("{name} is {value}")
    }
    if let (name, _) = setting("height=tall") {
        print("{name} is set")
    } else {
        print("not a number")
    }
}
```

```text
width is 40
not a number
```

The names can't have types (`if let (a, b): (Int, Int) = …`); give the
function a return type instead.

## for loops

`for` runs its body once for each item of a list, or each number in a range:

```tessel
for i in 0..3 {
    print("Round {i}")
}

let fruits = ["apple", "banana", "cherry"]
for fruit in fruits {
    print(fruit.uppercase())
}
```

```text
Round 0
Round 1
Round 2
APPLE
BANANA
CHERRY
```

`a..b` counts from `a` up to, but not including, `b`. If `b` is not larger
than `a`, the loop doesn't run at all.

If you don't need the loop variable, name it `_`:

```tessel
for _ in 0..3 {
    print("Hip hip hooray!")
}
```

A range is a value of type `Range`, so you can also keep one in a variable or
pass it to a function, and loop over it later:

```tessel
fn countdown(_ steps: Range) {
    for i in steps {
        print(10 - i)
    }
}

countdown(7..10)   // prints 3, 2, 1
```

To loop over a list together with the position of each item, loop over a
range of indexes:

```tessel
let names = ["Ada", "Grace"]
for i in 0..names.count {
    print("{i + 1}. {names[i]}")
}
```

```text
1. Ada
2. Grace
```

### Looping over a dictionary

A loop over a dictionary gives each entry's key and value, in the order the
keys were first added:

```tessel
let stock = ["apples": 3, "pears": 0]
for (name, count) in stock {
    print("{name}: {count}")
}
```

```text
apples: 3
pears: 0
```

### Changing items while looping

When you loop over a list stored in a `var` (or a `state`, or a `bind`
parameter), the loop variable stands for the item in the list, so changing it
changes the list:

```tessel
var prices = [10, 20, 30]
for price in prices {
    price *= 2
}
print(prices[0])
```

```text
20
```

When the list can't change (a `let`, a parameter, or the result of a call
like `filter`), the loop variable is a constant.

### Adding and removing while looping

A loop over a list in a `var` goes over the list as it is at each round,
not as it was when the loop began: it stops when it has come to the
list's current end. So items added in the loop are looped over too, and
removing an item before the one the loop is at makes it skip one:

```tessel
var xs = [1, 2, 3]
for x in xs {
    if x < 3 {
        xs.append(x + 10)
    }
}
print(xs)
```

```text
[1, 2, 3, 11, 12]
```

The same goes for loops inside loops. A loop that always adds goes on
forever. To go over the list as it was, loop over a copy:

```tessel
let before = xs
for x in before {
    xs.append(x * 2)     // before doesn't change
}
```

## while loops

`while` repeats its body as long as a condition is true. It checks the
condition before each round, so the body may not run at all:

```tessel
fn collatzSteps(n: Int) -> Int {
    var steps = 0
    var x = n
    while x != 1 {
        x = if x % 2 == 0 { x / 2 } else { 3 * x + 1 }
        steps += 1
    }
    steps
}

fn main() {
    print(collatzSteps(n: 27))
}
```

```text
111
```

Use `for` when you're going through a list or a range, and `while` when you
don't know in advance how many rounds it takes.

### while let

`while let` runs the body as long as an optional has a value, with the value
named inside, like [`if let`](#if-let) in a loop:

```tessel
fn main() {
    var queue = ["one", "two", "three"]
    while let next = queue.first {
        queue.removeFirst()
        print("took {next}")
    }
}
```

```text
took one
took two
took three
```

Like `if let`, `while let` can take an optional tuple apart:

```tessel
fn main() {
    var queue = [(1, "one"), (2, "two")]
    while let (n, word) = queue.first {
        queue.removeFirst()
        print("{n} is {word}")
    }
}
```

```text
1 is one
2 is two
```

## break and continue

In any loop, `for` or `while`:

- `continue` skips the rest of the body and goes on with the next round.
- `break` leaves the loop straight away.

```tessel
fn main() {
    for n in 1..100 {
        if n % 2 == 0 {
            continue       // skip even numbers
        }
        if n > 7 {
            break          // done
        }
        print(n)
    }
}
```

```text
1
3
5
7
```

In nested loops, they apply to the innermost one. `while true { … }` with a
`break` inside is a loop that decides when to stop in the middle:

```tessel
fn main() {
    var guess = 1
    while true {
        if guess * guess > 50 {
            break
        }
        guess += 1
    }
    print("{guess} squared is the first square above 50")
}
```

```text
8 squared is the first square above 50
```

A block (`{ x in … }`) is a small function of its own, so `break` and
`continue` inside it can't leave a loop outside it; use `return` to leave the
block. And `return` still leaves the whole function, from inside any loop:

```tessel
fn firstNegative(numbers: [Int]) -> Int? {
    for n in numbers {
        if n < 0 {
            return n
        }
    }
    nil
}

fn main() {
    print(firstNegative(numbers: [4, -2, -7]) ?? 0)
}
```

```text
-2
```

A view body can't use `while`, `break` or `continue`: it describes what to
show, so it uses `for` to show views for each item, and `if` to choose. Loops
with `while`, `break` and `continue` belong in functions and actions.

## match

`match` compares a value against a list of patterns, and runs the arm of the
first one that fits. Each arm is `pattern -> result`:

```tessel
enum Weather { sunny, rainy, snowy }

fn advice(weather: Weather) -> String {
    match weather {
        .sunny -> "Wear sunglasses"
        .rainy -> "Take an umbrella"
        .snowy -> "Wear boots"
    }
}

fn main() {
    print(advice(weather: .rainy))
}
```

```text
Take an umbrella
```

### Patterns

| Pattern | Matches |
|---|---|
| `.name` | the enum case `name` (whatever values it carries) |
| `.name(a, b)` | the enum case `name`, and names its values `a` and `b` |
| `.name(a, _)` | the same, ignoring the second value |
| `42`, `-1`, `2.5`, `"text"`, `true` | a value equal to this literal |
| `_` | anything |
| `.a, .b`, `1, 2, 3` | any of the patterns (see [below](#several-patterns-in-one-arm)) |

```tessel
fn describe(n: Int) -> String {
    match n {
        0 -> "zero"
        1 -> "one"
        -1 -> "minus one"
        _ -> "many"
    }
}
```

### Several patterns in one arm

An arm can list several patterns, separated by commas; it runs when any of
them fits. The list may continue on the next line after a comma:

```tessel
enum Direction { north, south, east, west }

fn isVertical(_ d: Direction) -> Bool {
    match d {
        .north, .south -> true
        .east, .west -> false
    }
}

fn kind(_ letter: String) -> String {
    match letter {
        "a", "e", "i",
        "o", "u" -> "vowel"
        _ -> "consonant"
    }
}

fn main() {
    print(isVertical(.south))
    print(kind("e"))
}
```

```text
true
vowel
```

Each pattern counts towards handling every value, so the two arms of
`isVertical` cover all four cases. An arm with several patterns can't name
an enum case's values, since it couldn't tell which case they came from:
write `.circle(_), .square(_) -> …`, or give each case its own arm.

Patterns can't be nested, and there are no ranges or conditions in patterns.
Use `if` for those.

### Every value must be handled

A `match` must cover every possible value. For an enum, that means an arm
for every case; for a `Bool`, arms for `true` and `false`. For other types,
like `Int` and `String`, there are too many values to list, so you need a
`_` arm. If something is missing, `tessel check` tells you what:

```text
error: this `match` doesn't handle `.snowy`
  = help: add arms for them, or `_ -> …` to handle the rest
```

`_` must be the last arm, because any arms after it could never run.

### Arms with several lines

To run more than one line in an arm, put the arm's body in braces:

```tessel
let count = 2
match count {
    0 -> print("none")
    _ -> {
        let doubled = count * 2
        print("{count}, doubled is {doubled}")
    }
}
```

```text
2, doubled is 4
```

Braces are also needed when an arm changes a variable, because an
assignment isn't a value:

```tessel
match mode {
    .light -> { brightness = 1.0 }
    .dark -> { brightness = 0.2 }
}
```

Without the braces, `.light -> brightness = 1.0` is an error, and
`tessel check` suggests adding them.

### match as a value

Like `if`, `match` is an expression. Its value is the value of the arm that
ran:

```tessel
let answer = "yes"
let accepted = match answer {
    "yes" -> true
    "y" -> true
    _ -> false
}
print(accepted)
```

```text
true
```

## In views

`if`, `for` and `match` also build user interfaces: inside a view, they
decide which views to show, and `for` creates one view per item. See
[Apps and views](/ui/apps-views/) and [Lists and identity](/ui/lists/).
