---
title: Functions
description: Declaring and calling functions in Tessel, with labelled arguments, unlabelled parameters, default values, return values and trailing blocks.
---

A function is a named piece of code that you can call from other places.

```tessel
fn greet(name: String) {
    print("Hello, {name}!")
}

fn area(width: Int, height: Int) -> Int {
    width * height
}

fn main() {
    greet(name: "Ada")
    print(area(width: 3, height: 4))
}
```

```text
Hello, Ada!
12
```

## Declaring a function

A function declaration has:

- the keyword `fn`,
- a name,
- a list of parameters in parentheses, each written `name: Type`,
- optionally `->` and the type of value it returns,
- a body in braces.

The parentheses are required even when there are no parameters:
`fn reset() { … }`.

Functions are declared at the top level of a file (or directly inside a
`view`, see [Apps and views](/ui/apps-views/)). You can't declare a function
inside another function; use a [closure](/language/closures/) for that.

A function can be used before it is declared, and from any file in the same
program. Each function needs a name that is unique in the whole program:
there is no overloading by parameter types.

## Returning a value

If a function has a return type, the **last line of its body is the result**:

```tessel
fn square(n: Int) -> Int {
    n * n
}
```

Use `return` to leave the function early:

```tessel
fn describe(n: Int) -> String {
    if n < 0 {
        return "negative"
    }
    if n == 0 { "zero" } else { "positive" }
}
```

Since `if` and `match` are expressions, the last line can be one. Every
branch then needs a value: an `if` without an `else` can't be the result of
a function that returns something.

A function without `->` returns nothing. It can still use a bare `return` to
stop early:

```tessel
fn warn(count: Int) {
    if count < 10 {
        return
    }
    print("That's a lot!")
}
```

## Labelled arguments

When you call a function, you write each argument with its parameter's
name as a label:

```tessel
fn move(x: Int, y: Int) {
    print("moving to {x}, {y}")
}

fn main() {
    move(x: 10, y: 20)
}
```

Labels make calls easy to read, and they catch mistakes such as swapping two
arguments of the same type. Arguments must be given in the same order as the
parameters are declared. `move(y: 20, x: 10)` is an error:

```text
error: `x:` is in the wrong position
  = help: arguments go in this order: `x:`, `y:`
```

### When you can leave the label out

There are two cases where the label is not written:

1. **A function with just one parameter.** Its argument may drop the label:
   `square(4)` and `square(n: 4)` both work.
2. **A parameter declared with `_` in front.** Its argument never has a
   label:

```tessel
fn double(_ value: Int) -> Int {
    value * 2
}

fn repeatText(_ text: String, times: Int) -> String {
    var out = ""
    for i in 0..times {
        out += text
    }
    out
}

fn main() {
    print(double(21))
    print(repeatText("ab", times: 3))
}
```

```text
42
ababab
```

The name after `_` (`value`, `text`) is used inside the function; callers
never see it. This is how built-ins such as `print("…")` and `Text("…")`
work. Any parameter can be unlabelled, not only the first, but it reads best
to reserve `_` for a first parameter whose meaning is obvious from the
function's name.

The label is always the parameter's own name. Unlike Swift, a parameter
can't have a separate outside label (`fn move(to point: Point)` is not
valid).

## Default values

A parameter can have a default value. Callers can then leave that argument
out:

```tessel
fn greeting(name: String, greeting: String = "Hello", punctuation: String = "!") -> String {
    "{greeting}, {name}{punctuation}"
}

fn main() {
    print(greeting(name: "Ada"))
    print(greeting(name: "Ada", punctuation: "?"))
    print(greeting(name: "Ada", greeting: "Welcome"))
}
```

```text
Hello, Ada!
Hello, Ada?
Welcome, Ada!
```

You can skip any argument that has a default, and give later ones by label,
as long as the ones you give stay in declaration order.

A default value is worked out each time the function is called, at the place
of the call. It can use literals and other top-level functions, but not the
function's other parameters: `fn area(width: Int, height: Int = width)` is
an error.

## Parameters are copies

A function receives its own copy of each argument. Parameters are constants
inside the function: assigning to one is an error. To change a value, copy
it into a `var` and return the result:

```tessel
fn addBonus(scores: [Int]) -> [Int] {
    var result = scores
    result.append(100)
    result
}

fn main() {
    let original = [10, 20]
    let updated = addBonus(scores: original)
    print("{original.count} {updated.count}")
}
```

```text
2 3
```

The caller's `original` is unchanged. See
[Values and copying](/language/values/) for why.

## Functions that take functions

A parameter can have a function type, such as `fn()` or `fn(Int) -> Int`.
You can pass a named function, or write a [closure](/language/closures/) in
braces:

```tessel
fn applyTwice(value: Int, change: fn(Int) -> Int) -> Int {
    change(change(value))
}

fn addTen(n: Int) -> Int {
    n + 10
}

fn main() {
    print(applyTwice(value: 1, change: addTen))
    print(applyTwice(value: 3, change: { n in n * n }))
}
```

```text
21
81
```

Calling a value of function type, like `change(value)` above, uses no labels.

### Trailing blocks

When the **last** parameter is a function, you can write the closure after
the closing parenthesis instead of inside it. This is called a trailing
block:

```tessel
fn repeatAction(times: Int, action: fn()) {
    for i in 0..times {
        action()
    }
}

fn main() {
    repeatAction(times: 2) {
        print("hip hip hooray")
    }
}
```

```text
hip hip hooray
hip hip hooray
```

If the function takes only that one parameter, the parentheses can go too:
`run { print("hi") }`. This is the same syntax that
`Button("Save") { save() }` and `VStack { … }` use in views.

Trailing blocks with parameters work the same way:

```tessel
let evens = [1, 2, 3, 4].filter { n in n % 2 == 0 }
```

:::note
In the header of an `if`, `for` or `match`, a `{` starts the body, so a
trailing block can't be used there. Put the closure in parentheses instead:
`for n in numbers.filter({ n in n > 0 }) { … }`.
:::

## Recursion

A function can call itself:

```tessel
fn fib(n: Int) -> Int {
    if n < 2 { n } else { fib(n: n - 1) + fib(n: n - 2) }
}

fn main() {
    print(fib(20))
}
```

```text
6765
```
