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

A dictionary can't be looped over directly. Loop over its `keys` or its
`values` instead. Both come in the order the keys were first added:

```tessel
let stock = ["apples": 3, "pears": 0]
for name in stock.keys {
    let count = stock[name] ?? 0
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

### No while, break or continue

Tessel doesn't have `while` loops, `break` or `continue` yet. Some ways to
get the same effect:

- To skip an item, wrap the body in an `if`.
- To stop at the first match, put the loop in a function and `return` from
  it, or use a list method like `first(where:)`.
- To repeat something a fixed number of times, loop over a range.

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

Patterns can't be nested or combined, and there are no ranges or conditions
in patterns. Use `if` for those.

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
