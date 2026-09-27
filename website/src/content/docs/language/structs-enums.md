---
title: Structs and enums
description: Declaring your own types in Tessel - structs with fields, defaults and methods, and enums with cases, associated values and methods.
---

Tessel has two ways to declare your own types:

- A **struct** groups several values together, like a todo item with a title
  and a done flag.
- An **enum** is a value that is exactly one of a fixed set of cases, like a
  direction that is north, east, south or west.

Both can have methods.

## Structs

```tessel
struct Todo {
    title: String
    done: Bool = false
    tags: [String] = []
}
```

Each field has a name and a type, and may have a default value after `=`.
Fields go on separate lines (or are separated by commas).

### Creating a value

A struct is created by calling its name with a labelled argument for each
field, in the order the fields are declared. Fields with a default can be
left out:

```tessel
let first = Todo(title: "Write docs")
let second = Todo(title: "Ship it", done: true)
let third = Todo(title: "Relax", tags: ["weekend"])
print(first.done)    // false
```

Fields without a default are required. That includes optional fields: write
`note: String? = nil` if a field should start out empty.

### Reading and changing fields

Use `.` to read a field. To change one, the value must be in a `var`:

```tessel
var todo = Todo(title: "Write docs")
todo.done = true
todo.tags.append("writing")
print("{todo.title}: {todo.done}, {todo.tags.count} tag")
```

```text
Write docs: true, 1 tag
```

Doing this to a `let` constant is an error. Fields of structs inside lists
can be changed in place too: `todos[0].done = true`. See
[Values and copying](/language/values/) for how this works.

### Methods

Functions declared inside a struct are its methods:

```tessel
struct Rectangle {
    width: Float
    height: Float

    fn area() -> Float {
        width * height
    }

    fn describe() -> String {
        "{width} by {height}, area {area()}"
    }
}

fn main() {
    let r = Rectangle(width: 3, height: 2)
    print(r.area())
    print(r.describe())
}
```

```text
6.0
3.0 by 2.0, area 6.0
```

Inside a method, the fields are available by name (`width`), and other
methods can be called by name (`area()`). The whole value is available as
`self`. You need `self` when a parameter has the same name as a field:

```tessel
struct Player {
    name: String
    score: Int = 0

    fn rename(name: String) {
        self.name = name
    }
}
```

### Methods that change the value

A method can change the struct's fields. There is no special keyword for
this: Tessel sees that the method assigns to a field (or calls a changing
method like `append` on one) and treats it as a changing method.

```tessel
struct Counter {
    count: Int = 0
    step: Int = 1

    fn increment() {
        count += step
    }

    fn reset() {
        count = 0
    }
}

fn main() {
    var counter = Counter(step: 5)
    counter.increment()
    counter.increment()
    print(counter.count)
    counter.reset()
    print(counter.count)
}
```

```text
10
0
```

A changing method can only be called on something that can change: a `var`,
a `state`, a `bind` parameter, or a field or item of one. Calling it on a
`let` is an error:

```text
error: can't call `increment` on `counter`, which is a constant, because `increment` changes it
  = help: declare it with `var` to be able to change it
```

### Comparing structs

Two struct values are equal with `==` when all their fields are equal:

```tessel
struct Point {
    x: Int
    y: Int = 0
}

fn main() {
    print(Point(x: 1) == Point(x: 1, y: 0))
    print(Point(x: 1) == Point(x: 2))
}
```

```text
true
false
```

Structs can't be compared with `<` or `>`, and they can't be printed or put
into a string directly. Print their fields instead, or write a `describe()`
method like the one above.

### Rules for fields

- A struct can't contain itself, directly or through another struct or an
  optional, because its values would be infinitely large. A list of itself is
  fine: `children: [Node]`.
- Fields can't hold views. Store the data a view needs, and build the view in
  a `view`.
- A field default can only use literals and top-level functions, not other
  fields.

## Enums

An enum lists the possible cases of a value:

```tessel
enum Direction { north, east, south, west }
```

Cases can be separated by commas or put on separate lines.

To refer to a case, write the enum name, a dot and the case name:
`Direction.north`. When Tessel already knows which enum is expected, you can
leave the name out and write just `.north`:

```tessel
var heading = Direction.north   // the type isn't known yet: write it out
heading = .west                 // heading is a Direction, so .west is enough
if heading == .west {
    print("Going {heading}")
}
```

```text
Going west
```

The shorthand works anywhere the type is known: assigning to a variable of
that type, passing an argument, comparing with `==`, returning from a
function, and in list and dictionary literals whose type is known. It is the
same shorthand you use for `.bold` or `.red` in views.

Printing an enum value, or putting it in a string, gives the case name.

### Cases with values

A case can carry values, written like parameters:

```tessel
enum Shape {
    circle(radius: Float)
    rectangle(width: Float, height: Float)
    empty
}
```

Create one by calling the case with labelled values:

```tessel
let round = Shape.circle(radius: 2)
let box: Shape = .rectangle(width: 3, height: 4)
let shapes = [Shape.circle(radius: 1), .rectangle(width: 2, height: 5), .empty]
```

To get the values back out, use `match`. Each arm names the case, and gives
names to its values in order:

```tessel
fn area(shape: Shape) -> Float {
    match shape {
        .circle(r) -> 3.14159 * r * r
        .rectangle(w, h) -> w * h
        .empty -> 0
    }
}
```

The names in the pattern (`r`, `w`, `h`) are your choice; they don't have to
match the labels in the declaration. Use `_` for a value you don't need,
like `.rectangle(w, _)`, or leave the parentheses off to match the case
whatever its values are: `.circle -> "round"`.

`match` must handle every case. See [Control flow](/language/control-flow/#match)
for everything `match` can do.

Enum values can be compared with `==`, including their values:
`Shape.circle(radius: 1) == .circle(radius: 1)` is `true`.

### Methods on enums

Enums can have methods too. Inside one, `self` is the current value, which
you usually `match` on:

```tessel
enum Light {
    red, yellow, green

    fn next() -> Light {
        match self {
            .red -> .green
            .yellow -> .red
            .green -> .yellow
        }
    }

    fn advance() {
        self = next()
    }
}

fn main() {
    var light = Light.red
    print(light.next())
    light.advance()
    light.advance()
    print(light)
}
```

```text
green
yellow
```

`advance` assigns to `self`, so like a changing struct method it can only be
called on a variable.

### Enums as dictionary keys

An enum whose cases have no values can be a dictionary key:

```tessel
enum Priority { low, medium, high }

fn main() {
    let labels: [Priority: String] = [.low: "later", .high: "now"]
    print(labels[.high] ?? "?")
}
```

```text
now
```
