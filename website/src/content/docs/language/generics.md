---
title: Generics
description: Generic functions, structs and enums in Tessel - writing code once for any type, with type parameters, inferred type arguments and interface constraints.
---

**Generics** let you write a function or a type once and use it with any
type, while every use is still checked exactly. `[T]`, `[K: V]` and `T?` are
generic already: a list can hold `Int`s or `Note`s. Generics let your own
code do the same.

## Generic functions

A function lists its **type parameters** in angle brackets after its name,
and uses them like types:

```tessel
fn lastOr<T>(items: [T], fallback: T) -> T {
    items.last ?? fallback
}

fn main() {
    print(lastOr(items: [1, 2, 3], fallback: 0))
    let none: [String] = []
    print(lastOr(items: none, fallback: "empty"))
}
```

```text
3
empty
```

`T` stands for whatever type a call uses. You don't write it at the call:
Tessel works it out from the arguments (`[1, 2, 3]` makes `T` an `Int`), and
then checks the call as if the function had been written for `Int`. So
`lastOr(items: [1, 2], fallback: "x")` is an error: `T` can't be both an
`Int` and a `String`.

A function can have several type parameters, and they can appear inside
other types, including function types:

```tessel
fn mapAll<T, U>(items: [T], f: fn(T) -> U) -> [U] {
    var out: [U] = []
    for x in items {
        out.append(f(x))
    }
    out
}

fn main() {
    let lengths = mapAll(items: ["a", "bbb"], f: { s in s.count })
    print(lengths[1])
}
```

```text
3
```

## Generic structs

A struct with type parameters is a template for a family of types:
`Stack<Int>`, `Stack<String>`, `Stack<Note>`:

```tessel
struct Stack<T> {
    items: [T] = []

    fn push(item: T) {
        items.append(item)
    }

    fn pop() -> T? {
        let top = items.last
        if items.count > 0 {
            items.removeLast()
        }
        top
    }
}

fn main() {
    var numbers = Stack<Int>()
    numbers.push(item: 1)
    numbers.push(item: 2)
    print(numbers.pop() ?? 0)

    var words: Stack<String> = Stack()
    words.push(item: "hello")
    print(words.pop() ?? "")
}
```

```text
2
hello
```

Wherever a type is written, a generic struct needs its type arguments:
`var s: Stack<Int>`, `[Stack<String>]`. When creating a value, they can be
worked out instead:

- from the values: `Stack(items: [1.5, 2.5])` is a `Stack<Float>`;
- from the type that's expected: `var words: Stack<String> = Stack()`;
- or written out: `Stack<Int>()`.

Inside its own declaration, a struct's bare name means the same type with
the same arguments, and it can refer to other versions of itself:

```tessel
struct Pair<A, B> {
    first: A
    second: B

    fn swapped() -> Pair<B, A> {
        Pair<B, A>(first: second, second: first)
    }
}
```

## Generic enums

Enums can be generic too. A common one holds either a result or a reason it
failed:

```tessel
enum Result<T> {
    ok(value: T)
    failure(message: String)
}

fn parse(text: String) -> Result<Int> {
    if let n = Int(text) {
        return .ok(value: n)
    }
    Result.failure(message: "not a number: {text}")
}

fn main() {
    for t in ["12", "x"] {
        match parse(text: t) {
            .ok(v) -> print("parsed {v}")
            .failure(m) -> print(m)
        }
    }
}
```

```text
parsed 12
not a number: x
```

Where the enum's type is known (a return type, a declared variable), `.ok(…)`
works on its own. (At the start of a line, write the enum's name as in
`Result.failure(…)`: a line starting with `.` continues the line before, as
[modifiers](/ui/styling/) do.) Otherwise write the type arguments, like
`Result<Int>.ok(value: 1)`, or let them be worked out from the case's
values: `Result.ok(value: 1)`.

## Constraints

A type parameter can be limited to types that conform to an
[interface](/language/interfaces/): `<T: Shape>`. Then the function can use
the interface's methods and properties on its values:

```tessel
interface Shape {
    fn area() -> Float
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { pi * radius * radius }
}

fn largest<T: Shape>(items: [T]) -> T? {
    var best: T? = nil
    for s in items {
        if s.area() > (best?.area() ?? -1.0) {
            best = s
        }
    }
    best
}

fn main() {
    let big = largest(items: [Circle(radius: 1.0), Circle(radius: 3.0)])
    print(big?.radius ?? 0.0)
}
```

```text
3.0
```

`largest` returns a `Circle?` here, not a `Shape?`, so `.radius` works
without `as?`. That's the difference from an interface on its own: a
`[Shape]` can mix circles and squares, while `T` is one type for each call.

Without a constraint, `T` could be any type, so calling a method on it is an
error that suggests adding one.

## How it works, and what to know

Tessel compiles generic code by **specialization**: for each set of type
arguments a program uses, it makes a copy of the declaration with the types
filled in (`Stack<Int>`, `lastOr<String>`), and checks and compiles the copy
like code you'd have written by hand. So:

- Generic code runs exactly as fast as the same code written for one type.
- Each type used adds its own copy to the program, as with hand-written
  versions.
- A generic declaration is checked on its own, so most mistakes are reported
  where you wrote them. A mistake that only one type causes (say, adding
  values of a type that can't be added) is reported for that copy, with a
  note pointing to where that type was used:

```text
error: expected `Int`, found `String` (in `asInt<String>`)
 --> main.tsl:2:5
  |
2 |     x
  |     ^ this is `String`
 ::: main.tsl:6:11
  |
6 |     print(asInt(x: "five"))
  |           ----- `asInt<String>` is needed here
```

- A generic function can't be used as a value without calling it (write a
  block that calls it instead). Methods can't have type parameters of their
  own; a generic struct's methods use the struct's.
