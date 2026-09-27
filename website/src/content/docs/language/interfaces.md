---
title: Interfaces
description: Interfaces in Tessel - declaring the methods several types share, conforming structs and enums to them, and using values of an interface type.
---

An **interface** names a set of methods. Different types can all have those
methods, and then they can be used alike: put a circle, a rectangle and a
triangle in one list of shapes, and ask each for its area.

```tessel
interface Shape {
    fn area() -> Float
    fn describe() -> String
}

struct Circle: Shape {
    radius: Float

    fn area() -> Float {
        pi * radius * radius
    }

    fn describe() -> String {
        "a circle of radius {radius}"
    }
}

struct Rect: Shape {
    width: Float
    height: Float

    fn area() -> Float {
        width * height
    }

    fn describe() -> String {
        "a {width}×{height} rectangle"
    }
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Rect(width: 2.0, height: 3.0)]
    for s in shapes {
        print("{s.describe()}: {s.area().formatted(decimals: 2)}")
    }
}
```

```text
a circle of radius 1.0: 3.14
a 2.0×3.0 rectangle: 6.00
```

## Declaring an interface

An interface lists method signatures, each like a function's first line
without a body:

```tessel
interface Shape {
    fn area() -> Float
    fn scaled(by: Float) -> Float
}
```

The parameters and the result are part of the requirement. Interface methods
can't have default values for their parameters.

## Conforming to an interface

A struct or an enum says which interfaces it has after a colon, and then has
a method for each requirement, with the same name, the same parameters (labels
and types) and the same result:

```tessel
interface Named {
    fn name() -> String
}

enum Planet: Named {
    mercury
    venus
    earth

    fn name() -> String {
        match self {
            .mercury -> "Mercury"
            .venus -> "Venus"
            .earth -> "Earth"
        }
    }
}

fn main() {
    let things: [Named] = [Planet.earth, Planet.venus]
    print(things.map { t in t.name() }.joined(separator: ", "))
}
```

```text
Earth, Venus
```

A type can conform to several interfaces: `struct Circle: Shape, Named`.

If a method is missing or doesn't match, `tessel check` says what the
interface requires:

```text
error: `Circle` says it's a `Shape`, but has no method `describe`
  = help: add it to `Circle`: `fn describe() -> String { … }`
```

## Using interface values

An interface is a type. A value of a conforming type can be used wherever the
interface is expected (it's converted automatically):

- in lists: `let shapes: [Shape] = [Circle(radius: 1.0), Rect(width: 2.0, height: 3.0)]`
- as parameters and results: `fn largest(shapes: [Shape]) -> Shape?`
- in optionals: `var selected: Shape? = nil`
- as struct fields, and in a view's `state` and parameters.

Calling a method on an interface value runs the method of the value's own
type. With an optional, use `?.` as usual: `selected?.area()`.

```tessel
fn largest(shapes: [Shape]) -> Shape? {
    var best: Shape? = nil
    var bestArea = -1.0
    for s in shapes {
        if s.area() > bestArea {
            best = s
            bestArea = s.area()
        }
    }
    best
}
```

## Rules and limits

- **Interface methods can't change the value.** A method that changes its
  struct (say, `count += 1`) can't be the one an interface requires. Return a
  changed copy instead: `fn incremented() -> Counter`. This keeps interface
  values plain values, like everything else in Tessel.
- **Only methods.** An interface can't require fields; require a method that
  returns the value instead, like `fn name() -> String`.
- **No `==`.** Interface values can't be compared, since two different types
  have no common way to be equal.
- **Not saved.** Values of an interface type can't be saved with `toJson` or
  `toBinary`, because loading them back wouldn't know which type to make.
- **Converted one at a time.** A `Circle` converts to a `Shape`, but a list
  `[Circle]` doesn't convert to `[Shape]`; build the list as `[Shape]` from the
  start.
- You can't yet ask an interface value which type it holds.
