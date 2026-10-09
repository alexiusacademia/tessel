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

## Properties

An interface can also require properties, written like struct fields. Every
conforming struct has a field with that name and type:

```tessel
interface Named {
    name: String
}

struct Person: Named {
    name: String
    age: Int
}

struct Pet: Named {
    name: String
    species: String
}

fn main() {
    var all: [Named] = [Person(name: "Ada", age: 36), Pet(name: "Rex", species: "dog")]
    all[1].name = "Max"
    print(all.map { n in n.name }.joined(separator: ", "))
}
```

```text
Ada, Max
```

Properties can be read and, on a `var`, changed through the interface. Only
structs have fields, so an enum can't conform to an interface that requires a
property. For the same reason a
[computed property](/language/structs-enums/#computed-properties) doesn't
satisfy the requirement: it must be a stored field. To let each type work
the value out its own way, require a method instead (`fn name() -> String`).

## Methods that change the value

A conforming type's methods may change its value, and calling them through
the interface changes the interface value, like calling a changing method on
the struct itself. The value must be a `var` (or `state`, or a list item you
can change):

```tessel
interface Counter {
    fn increment()
    fn value() -> Int
}

struct Clicks: Counter {
    count: Int = 0

    fn increment() {
        count += 1
    }

    fn value() -> Int {
        count
    }
}

fn main() {
    var a: Counter = Clicks()
    a.increment()
    let copy = a
    a.increment()
    print("{a.value()} {copy.value()}")
}
```

```text
2 1
```

Interface values are values, like everything in Tessel: `copy` keeps the
value it had when it was copied, and changing `a` afterwards doesn't change
it.

## Comparing

`==` and `!=` work on interface values: two values are equal when they hold
the same type and equal values. A `Circle` never equals a `Rect`, and list
methods like `contains` work on lists of interface values. (A type that
can't be compared, because it holds functions, is only equal to the very same
copy.)

## Which type is it?

`is` checks which type an interface value holds, and `as?` gives you the value
as that type, or `nil`:

```tessel
interface Shape {
    fn area() -> Float
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { pi * radius * radius }
}

struct Square: Shape {
    side: Float
    fn area() -> Float { side * side }
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Square(side: 2.0)]
    for s in shapes {
        if let c = s as? Circle {
            print("a circle, radius {c.radius}")
        } else if s is Square {
            print("a square")
        }
    }
}
```

```text
a circle, radius 1.0
a square
```

The type after `is` or `as?` must conform to the interface; asking whether a
`Shape` is a type that isn't a `Shape` is an error, since the answer is
always no.

## Converting lists and optionals

A value converts to an interface automatically, and so do lists and
optionals of it: a `[Circle]` can be passed where a `[Shape]` is expected,
and a `Circle?` where a `Shape?` is. The list is converted into a new
`[Shape]` list; the original `[Circle]` list stays as it was.

## Saving

Interface values can be saved with `toJson` and `toBinary`, and loaded back
with `fromJson` and `fromBinary`, as long as every type that conforms can be
saved. Each value is written with the name of its type, so loading makes the
right type again:

```json
{"type": "Circle", "value": {"radius": 1.0}}
```

A saved value whose type name isn't one of the conforming types makes the
load return `nil`. If you rename a type, older files still name the old type.

The same goes for [background work](/ui/async/#heavy-work-in-the-background):
interface values can be passed to it and returned from it.

## Rules

- A conforming method must match the requirement exactly: the same name,
  the same parameter labels and types, and the same result.
- Interface methods can't have default values for their parameters.
- A requirement can be `async` (`async fn load(key: String) -> String`); the
  conforming method must be `async` too. See
  [Waiting with async and await](/language/async/#methods-and-interfaces).
- An interface value is created by converting a value of a conforming type;
  the interface itself can't be called to create one.
