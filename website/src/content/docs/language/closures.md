---
title: Closures
description: Writing blocks of code as values in Tessel - closure syntax, parameters, function types, capturing variables and trailing blocks.
---

A **closure** is a piece of code written in braces that you can store in a
variable, pass to a function, and run later. You have already seen them in
calls like `list.filter { n in n > 0 }` and `Button("Save") { save() }`.

```tessel
let double: fn(Int) -> Int = { n in n * 2 }
print(double(21))
```

```text
42
```

## Syntax

A closure is written in braces. If it takes parameters, their names come
first, followed by `in`:

```tessel
let add: fn(Int, Int) -> Int = { a, b in a + b }
let sayHi: fn() = { print("hi") }
```

- `{ a, b in … }` takes two parameters, `a` and `b`.
- `{ … }` with no `in` takes no parameters.
- Use `_` for a parameter you don't need: `{ _ in "same" }`.

The body works like a function body: it can have several lines, and its
**last line is the result**. `return` inside a closure returns from the
closure, not from the function around it.

```tessel
let describe: fn(Int) -> String = { n in
    if n < 0 {
        return "negative"
    }
    let size = if n > 100 { "big" } else { "small" }
    "a {size} number"
}
print(describe(7))
```

```text
a small number
```

## Types come from context

Parameter types aren't written inside the closure. Tessel takes them from
where the closure is used: the function type of the variable it's stored in,
or the parameter it's passed to.

```tessel
let numbers = [1, 2, 3]
let squares = numbers.map { n in n * n }   // n is an Int, because numbers is [Int]
```

On its own, a closure with parameters has no types to go on, so this is an
error:

<!-- error -->
```tessel
let increment = { n in n + 1 }
```

```text
error: can't tell the types of this block's parameters
  = help: pass it where a function is expected, or give it a type: `let f: fn(Int) -> Int = { x in … }`
```

Add a type to the variable, `let increment: fn(Int) -> Int = { n in n + 1 }`,
and it works. A closure without parameters can be stored without a type.

## Function types

A function type is written `fn(ParameterTypes) -> ReturnType`. Leave off
`-> …` for a function that returns nothing.

| Type | Example closure |
|---|---|
| `fn()` | `{ print("done") }` |
| `fn(String)` | `{ text in print(text) }` |
| `fn(Int) -> Bool` | `{ n in n > 0 }` |
| `fn(Int, Int) -> Int` | `{ a, b in a + b }` |

Closures and named functions both have function types, so a function name
can be used wherever a closure can:

```tessel
fn isEven(n: Int) -> Bool {
    n % 2 == 0
}

fn main() {
    let check: fn(Int) -> Bool = isEven
    print(check(4))
    print([1, 2, 3, 4].filter(isEven).count)
}
```

```text
true
2
```

Calling a function value never uses labels: `check(4)`, not
`check(n: 4)`.

Function values can go in lists, be returned from functions, and be
optional (`fn()?`). An optional function is unwrapped with `if let`:

```tessel
let onFinish: fn()? = nil
if let finish = onFinish {
    finish()
} else {
    print("nothing to do")
}
```

## Passing closures to functions

A parameter with a function type accepts a closure:

```tessel
fn measure(label: String, work: fn() -> Int) {
    print("{label}: {work()}")
}

fn main() {
    measure(label: "answer", work: { 6 * 7 })
}
```

```text
answer: 42
```

### Trailing blocks

When the closure is the **last** argument, it can go after the parentheses.
If there are no other arguments, the parentheses can be left out too:

```tessel
fn measure(label: String, work: fn() -> Int) {
    print("{label}: {work()}")
}

fn twice(action: fn()) {
    action()
    action()
}

fn main() {
    measure(label: "answer") {
        6 * 7
    }
    twice {
        print("hello")
    }
}
```

```text
answer: 42
hello
hello
```

Built-in methods that take a single closure, like `filter { … }` and
`map { … }`, are usually called this way. Views use the same rule, which is why
`Button("Add") { … }` and `VStack { … }` look the way they do.

:::note
In the header of an `if`, `for` or `match`, the `{` begins the body, so pass
the closure in parentheses there: `if list.filter({ x in x > 0 }).isEmpty { … }`.
:::

## Capturing variables

A closure can use the variables around it. It doesn't take a copy of them: it
uses the variables themselves. If a variable changes after the closure is
made, the closure sees the new value, and a closure can change a `var` it
uses:

```tessel
var total = 0
let addFive = { total += 5 }
addFive()
addFive()
print(total)
```

```text
10
```

A captured variable stays alive as long as a closure uses it, even after the
function that declared it has returned. This lets a function hand back a
closure with its own private state:

```tessel
fn makeCounter() -> fn() -> Int {
    var count = 0
    {
        count += 1
        count
    }
}

fn main() {
    let next = makeCounter()
    print(next())
    print(next())
    print(next())
}
```

```text
1
2
3
```

Each call to `makeCounter` makes a new `count`, so two counters don't affect
each other.

:::caution
Copying a closure value doesn't copy the variables it captured. After
`let other = next`, calling `other()` and `next()` both advance the same
`count`. This is the one place where a Tessel value shares state instead of
being copied; see [Values and copying](/language/values/).
:::

## Closures in views

In an app, closures are how you say what should happen when something
occurs: `Button("+") { count += 1 }`, `.onTap { … }`, or
`after(seconds: 2) { … }`. They capture the view's `state`, so changing it in
the closure updates the screen. See [State and bindings](/ui/state/).
