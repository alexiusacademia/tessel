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

Functions are declared at the top level of a file, directly inside a
`view` (see [Apps and views](/ui/apps-views/)), or
[inside another function](#functions-inside-functions).

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

## Changing the caller's value with `bind`

To change the caller's variable instead of a copy, declare the parameter
`bind`. Inside the function it can be changed, and the change happens to
what the caller passed:

```tessel
fn addBonus(scores: bind [Int]) {
    scores.append(100)
}

fn main() {
    var scores = [10, 20]
    addBonus(scores: scores)
    print(scores)
}
```

```text
[10, 20, 100]
```

The argument must be something that can change: a `var`, a `state`, a
field or item of one, or another `bind` parameter. Methods can have `bind`
parameters too. Functions declared inside a function or a view can't, and a
`bind` parameter can't have a default value. See
[Changing the caller's value](/language/values/#changing-the-callers-value)
for more.

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

## Functions inside functions

A function can be declared inside another function, a method, or a
function of a view or app (and in a block, like a button's action). It's
known from its declaration to the end of the block it's in, so it doesn't
clutter the rest of the file. It can call itself, and it can use the
functions declared before it in the same place:

```tessel
fn report(scores: [Int]) {
    fn grade(_ score: Int) -> String {
        if score >= 90 { "A" } else if score >= 75 { "B" } else { "C" }
    }
    fn line(_ score: Int) -> String {
        "{score}: {grade(score)}"
    }
    for s in scores {
        print(line(s))
    }
}

fn main() {
    report(scores: [95, 80, 60])
}
```

```text
95: A
80: B
60: C
```

Like a [closure](/language/closures/#capturing-variables), it can use the
variables, constants and parameters around it. It uses the variables
themselves, not copies: it sees their values as they are when it's called,
and it can change a `var`. (Constants and parameters can't change, so it
simply has their values.)

```tessel
fn main() {
    var total = 0
    fn add(_ n: Int, times: Int = 1) {
        total += n * times
    }
    add(5)
    add(2, times: 3)
    print(total)
}
```

```text
11
```

It's called like any function, with labels and default values. It can also
be used as a value, as in `numbers.map(grade)`.

A function inside a function can't be generic (it can use the type
parameters of the function around it), and it isn't visible before its
declaration, so two of them can't call each other.

## Functions that wait

A function that waits for something slow (a download, a timer, work in the
background) is declared with `async fn`, and called with `await`:

```tessel
async fn loadTitle(url: String) -> String {
    let text = await fetch(url: url)
    return (text ?? "").lines().first ?? "untitled"
}
```

See [Waiting with async and await](/language/async/).
