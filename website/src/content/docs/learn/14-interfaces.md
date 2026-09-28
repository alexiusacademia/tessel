---
title: "14. Interfaces"
description: Let different types share the same abilities, so you can keep them in one list and use them alike.
---

A circle and a square are different things. A circle has a radius, a square
has a side. But they have something in common: both have an area. In this
lesson you'll learn how to tell Tessel "these different types can all do the
same things", so you can treat them alike.

In this lesson you'll learn:

- why a list can't simply mix different struct types
- how to declare an **interface**, a list of abilities that types can share
- how a type **conforms** to an interface
- how to make lists and functions that work with any type that conforms
- how an interface can require properties, not just methods
- how to check which type a value really is, with `is` and `as?`

## The problem: different types, same abilities

Here are two shapes, each with an `area()` method:

```tessel
struct Circle {
    radius: Float
    fn area() -> Float { 3.14159 * radius * radius }
}

struct Square {
    side: Float
    fn area() -> Float { side * side }
}

fn main() {
    let shapes = [Circle(radius: 1.0), Square(side: 2.0)]
}
```

It would be nice to keep all your shapes in one list and add up their
areas. But this doesn't work:

```text
error: expected `Circle`, found `Square`
  --> main.tsl:12:40
   |
12 |     let shapes = [Circle(radius: 1.0), Square(side: 2.0)]
   |                                        ^^^^^^^^^^^^^^^^^ this is `Square`
```

A list holds items of **one** type. The first item made this a list of
`Circle`s, so a `Square` can't go in. And Tessel is right to worry: if you
had a list with circles, squares and, say, numbers, what would
`shapes[1].area()` even mean?

What you want to say is: "this is a list of *things that have an area*".
That's what an interface is for. (Notice that the one-line methods here are
written on a single line. That's fine for short methods.)

## Declaring an interface

An **interface** is a name for a set of abilities. It lists methods, without
their bodies:

```tessel
interface Shape {
    fn area() -> Float
    fn name() -> String
}
```

This says: "a `Shape` is anything that has an `area()` method returning a
`Float` and a `name()` method returning a `String`." It doesn't say *how*
those work. Each type decides that for itself.

## Conforming to an interface

A type **conforms** to an interface when it promises to have those methods.
Write the interface's name after a colon, and then write the methods:

```tessel
interface Shape {
    fn area() -> Float
    fn name() -> String
}

struct Circle: Shape {
    radius: Float

    fn area() -> Float {
        3.14159 * radius * radius
    }

    fn name() -> String {
        "circle"
    }
}

struct Square: Shape {
    side: Float

    fn area() -> Float {
        side * side
    }

    fn name() -> String {
        "square"
    }
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Square(side: 2.0), Circle(radius: 0.5)]
    for shape in shapes {
        print("{shape.name()}: {shape.area().formatted(decimals: 2)}")
    }
}
```

```text
circle: 3.14
square: 4.00
circle: 0.79
```

`Shape` is now a type you can use like any other. `[Shape]` is a list of
shapes, and it can hold circles and squares side by side.

When you call `shape.area()`, Tessel runs the `area` of whatever the shape
really is: the circle formula for a circle, the square formula for a square.
The loop doesn't need to know or care which kind it has.

### Common mistake: a missing or different method

`struct Square: Shape` is a promise, and Tessel checks that you keep it. If
`Square` has no `name` method:

```text
error: `Square` says it's a `Shape`, but has no method `name`
 --> main.tsl:6:16
  |
6 | struct Square: Shape {
  |                ^^^^^
  |
  = help: add it to `Square`: `fn name() -> String { … }`
```

The method must also match exactly: the same name, the same parameters and
the same result type. If `Square`'s `area` returned an `Int`:

```text
error: `area` doesn't match what `Shape` requires
 --> main.tsl:8:8
  |
8 |     fn area() -> Int {
  |        ^^^^ this is `fn area() -> Int`
  |
  = help: `Shape` requires `fn area() -> Float`
```

These checks are what make interfaces safe. If a type says it's a `Shape`,
you can be sure every `Shape` method is there.

A type can have more methods and fields than the interface asks for, and it
can conform to several interfaces at once: `struct Circle: Shape, Named`.

## Using interface values

You can use an interface anywhere you'd use a type: for a variable, a
parameter, a result, or an optional. A function that takes `[Shape]` works
for any mix of shapes:

```tessel
interface Shape {
    fn area() -> Float
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { 3.14159 * radius * radius }
}

struct Square: Shape {
    side: Float
    fn area() -> Float { side * side }
}

fn totalArea(shapes: [Shape]) -> Float {
    var total = 0.0
    for shape in shapes {
        total += shape.area()
    }
    total
}

fn biggest(shapes: [Shape]) -> Shape? {
    var best: Shape? = nil
    for shape in shapes {
        if shape.area() > (best?.area() ?? 0.0) {
            best = shape
        }
    }
    best
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Square(side: 2.0), Square(side: 1.5)]
    print(totalArea(shapes: shapes).formatted(decimals: 2))
    print(biggest(shapes: shapes)?.area() ?? 0.0)
    let circles = [Circle(radius: 1.0), Circle(radius: 2.0)]
    print(totalArea(shapes: circles).formatted(decimals: 2))
}
```

```text
9.39
4.0
15.71
```

The last call passes a `[Circle]` where a `[Shape]` is expected. That's
fine: every circle is a shape, so Tessel converts the list for you.

The best part: if you add a `Triangle: Shape` next week, `totalArea` and
`biggest` work with triangles without changing a single line.

### Common mistake: using what only one type has

Through a `Shape`, you can only use what *every* shape has. A square has no
radius, so this is an error, even though the item happens to be a circle:

```tessel
let shapes: [Shape] = [Circle(radius: 1.0)]
print(shapes[0].radius)
```

```text
error: `Shape` has no property `radius`
  --> main.tsl:12:21
   |
12 |     print(shapes[0].radius)
   |                     ^^^^^^ unknown
```

If you really need the radius, first find out whether the shape is a
circle. You'll see how in [Which type is it?](#which-type-is-it) below.

## Required properties

An interface can also require **properties** (fields), written like the
fields of a struct. Every struct that conforms must have a field with that
name and type:

```tessel
interface Named {
    name: String
}

struct Person: Named {
    name: String
    age: Int
}

struct City: Named {
    name: String
    population: Int
}

fn main() {
    var things: [Named] = [Person(name: "Ada", age: 36), City(name: "Lisbon", population: 545000)]
    things[0].name = "Ada L."
    for thing in things {
        print(thing.name)
    }
}
```

```text
Ada L.
Lisbon
```

You can read the property through the interface, and change it too, as long
as the value is in a `var`. A struct that forgets the field gets an error
like this:

```text
error: `City` says it's a `Named`, but has no field `name`
 --> main.tsl:5:14
  |
5 | struct City: Named {
  |              ^^^^^
  |
  = help: add it to `City`: `name: String`
```

Only structs have fields, so only structs can conform to an interface with
properties. Enums can conform to interfaces that only list methods.

## Which type is it?

Sometimes you do need to know what an interface value really is. Two
keywords help:

- `value is Circle` is `true` if the value is a `Circle`.
- `value as? Circle` gives you the value as a `Circle`, or `nil` if it isn't
  one. It's an optional, so you use it with `if let`.

```tessel
interface Shape {
    fn area() -> Float
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { 3.14159 * radius * radius }
}

struct Square: Shape {
    side: Float
    fn area() -> Float { side * side }
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Square(side: 2.0), Circle(radius: 3.0)]
    for shape in shapes {
        if let circle = shape as? Circle {
            print("a circle with radius {circle.radius}")
        } else if shape is Square {
            print("a square")
        }
    }
    let circleCount = shapes.count(where: { s in s is Circle })
    print("{circleCount} circles")
}
```

```text
a circle with radius 1.0
a square
a circle with radius 3.0
2 circles
```

Inside the `if let`, `circle` is a real `Circle`, so `circle.radius` works.

Use these sparingly. If you find yourself writing "if it's a circle do this,
if it's a square do that" all over your program, that's usually a sign the
job should be a method in the interface instead. Then each type does its own
part.

Tessel also stops you asking a question whose answer is always "no". If
`Dog` isn't a `Shape`:

```text
error: a `Shape` is never a `Dog`
  --> main.tsl:13:16
   |
13 |     print(s is Dog)
   |                ^^^
   |
   = help: `Dog` doesn't conform to `Shape`
```

## Comparing and saving

Two quick things you'll find useful later:

- `==` works on interface values. Two values are equal when they're the
  same type *and* equal as that type. A circle never equals a square. So
  list methods like `contains` work on a `[Shape]` too.
- Interface values can be saved as text with `toJson`, and loaded back with
  `fromJson`. Tessel writes down each value's type, so loading gives you
  back circles and squares, not just "shapes":

```tessel
fn main() {
    let a: Shape = Circle(radius: 1.0)
    let b: Shape = Circle(radius: 1.0)
    let c: Shape = Square(side: 1.0)
    print(a == b)
    print(a == c)
    let shapes: [Shape] = [a, c]
    print(shapes.contains(Square(side: 1.0)))

    let text = toJson(shapes)
    print(text)
    let loaded: [Shape] = fromJson(text) ?? []
    print(loaded.count)
    print(loaded == shapes)
}
```

```text
true
false
true
[{"type":"Circle","value":{"radius":1.0}},{"type":"Square","value":{"side":1.0}}]
2
true
```

(This uses the `Shape`, `Circle` and `Square` from the previous example.)
You'll learn more about saving data in lesson 16.

## Worked example: pets

Let's model a house full of pets. Every pet has a name, makes a sound and
has some number of legs. Each kind of pet also has things only it has: a dog
knows tricks, a parrot knows words.

```tessel
interface Pet {
    name: String
    fn sound() -> String
    fn legs() -> Int
}

struct Dog: Pet {
    name: String
    tricks: [String] = []

    fn sound() -> String { "Woof" }
    fn legs() -> Int { 4 }
}

struct Cat: Pet {
    name: String
    indoor: Bool = true

    fn sound() -> String { "Meow" }
    fn legs() -> Int { 4 }
}

struct Parrot: Pet {
    name: String
    words: [String]

    fn sound() -> String {
        words.first ?? "Squawk"
    }
    fn legs() -> Int { 2 }
}

fn chorus(pets: [Pet]) {
    for pet in pets {
        print("{pet.name} says {pet.sound()}!")
    }
}

fn main() {
    var pets: [Pet] = [
        Dog(name: "Rex", tricks: ["sit", "roll over"]),
        Cat(name: "Misty"),
        Parrot(name: "Polly", words: ["Hello", "Cracker"]),
        Dog(name: "Bella"),
    ]

    chorus(pets: pets)

    let legs = pets.map { p in p.legs() }.sum()
    print("Legs in the house: {legs}")

    pets.append(Cat(name: "Tom", indoor: false))
    print("Pets: {pets.count}")

    for pet in pets {
        if let dog = pet as? Dog {
            if !dog.tricks.isEmpty {
                print("{dog.name} can {dog.tricks.joined(separator: " and ")}")
            }
        }
    }

    let cats = pets.filter { p in p is Cat }.map { p in p.name }
    print("Cats: {cats.joined(separator: ", ")}")
}
```

```text
Rex says Woof!
Misty says Meow!
Polly says Hello!
Bella says Woof!
Legs in the house: 14
Pets: 5
Rex can sit and roll over
Cats: Misty, Tom
```

Things to notice:

- `chorus` only knows about `Pet`. It works for dogs, cats, parrots, and any
  pet type you add later.
- A parrot's sound depends on its words. Each type decides for itself *how*
  to do what the interface asks.
- `pets.append(Cat(…))` adds a cat to a `[Pet]` list; the cat is converted
  to a `Pet` automatically.
- `as? Dog` gets at the dog-only `tricks`, and `is Cat` picks out the cats.

## Exercises

**1. A triangle.** Add a `Triangle` (with a `base` and a `height`) to the
`Shape` example with `area()` and `name()`. Its area is base times height
divided by two. Print the name and area of a circle, a square and a
triangle.

<details>
<summary>Solution</summary>

```tessel
interface Shape {
    fn area() -> Float
    fn name() -> String
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { 3.14159 * radius * radius }
    fn name() -> String { "circle" }
}

struct Square: Shape {
    side: Float
    fn area() -> Float { side * side }
    fn name() -> String { "square" }
}

struct Triangle: Shape {
    base: Float
    height: Float
    fn area() -> Float { base * height / 2.0 }
    fn name() -> String { "triangle" }
}

fn main() {
    let shapes: [Shape] = [Circle(radius: 1.0), Square(side: 2.0), Triangle(base: 3.0, height: 4.0)]
    for shape in shapes {
        print("{shape.name()}: {shape.area().formatted(decimals: 2)}")
    }
}
```

```text
circle: 3.14
square: 4.00
triangle: 6.00
```

</details>

**2. Favorites.** Make an interface `Describable` with one method,
`describe() -> String`. Make a `Book` (title, author) and a `Movie` (title,
minutes) that conform. Put one of each in a list and print
"I like …" for each.

<details>
<summary>Solution</summary>

```tessel
interface Describable {
    fn describe() -> String
}

struct Book: Describable {
    title: String
    author: String
    fn describe() -> String { "the book {title} by {author}" }
}

struct Movie: Describable {
    title: String
    minutes: Int
    fn describe() -> String { "the movie {title} ({minutes} min)" }
}

fn main() {
    let favorites: [Describable] = [Book(title: "Dune", author: "Frank Herbert"), Movie(title: "Up", minutes: 96)]
    for item in favorites {
        print("I like {item.describe()}")
    }
}
```

```text
I like the book Dune by Frank Herbert
I like the movie Up (96 min)
```

</details>

**3. A wallet.** Make an interface `Valuable` with `cents() -> Int`. Make
an **enum** `Coin` with `dime` and `quarter` that conforms, and a struct
`Banknote` with a number of `dollars` that conforms too. Put a mix of both
in a `[Valuable]` wallet and print how much is in it, as dollars and cents.

<details>
<summary>Solution</summary>

```tessel
interface Valuable {
    fn cents() -> Int
}

enum Coin: Valuable {
    dime, quarter

    fn cents() -> Int {
        match self {
            .dime -> 10
            .quarter -> 25
        }
    }
}

struct Banknote: Valuable {
    dollars: Int

    fn cents() -> Int {
        dollars * 100
    }
}

fn main() {
    let wallet: [Valuable] = [Banknote(dollars: 5), Coin.quarter, Coin.quarter, Coin.dime, Banknote(dollars: 1)]
    let total = wallet.map { v in v.cents() }.sum()
    print("{total / 100} dollars and {total % 100} cents")
}
```

```text
6 dollars and 60 cents
```

</details>

**4. Payroll.** Make an interface `Worker` with a `name: String` property and
a `weeklyPay() -> Int` method. `Salaried` workers have a yearly salary (their
weekly pay is a 52nd of it). `Hourly` workers have a rate and a number of
hours; every hour over 40 is paid one and a half times. Print each worker's
pay, the total, how many are paid by the hour, and who is best paid.

<details>
<summary>Solution</summary>

```tessel
interface Worker {
    name: String
    fn weeklyPay() -> Int
}

struct Salaried: Worker {
    name: String
    yearlySalary: Int

    fn weeklyPay() -> Int {
        yearlySalary / 52
    }
}

struct Hourly: Worker {
    name: String
    rate: Int
    hours: Int

    fn weeklyPay() -> Int {
        let overtime = if hours > 40 { hours - 40 } else { 0 }
        rate * hours + rate * overtime / 2
    }
}

fn main() {
    let staff: [Worker] = [
        Salaried(name: "Ada", yearlySalary: 62400),
        Hourly(name: "Ben", rate: 20, hours: 30),
        Hourly(name: "Cleo", rate: 25, hours: 44),
    ]

    var total = 0
    for worker in staff {
        print("{worker.name}: {worker.weeklyPay()}")
        total += worker.weeklyPay()
    }
    print("Total: {total}")

    let hourlyCount = staff.count(where: { w in w is Hourly })
    print("Paid by the hour: {hourlyCount}")

    if let top = staff.sorted(by: { a, b in a.weeklyPay() > b.weeklyPay() }).first {
        print("Best paid this week: {top.name}")
    }
}
```

```text
Ada: 1200
Ben: 600
Cleo: 1150
Total: 2950
Paid by the hour: 2
Best paid this week: Ada
```

Cleo's 44 hours pay 25 × 44 = 1100, plus half again for the 4 extra hours
(25 × 4 / 2 = 50).

</details>

## Summary

- An **interface** names a set of methods (and properties) that different
  types can share: `interface Shape { fn area() -> Float }`.
- A type **conforms** by naming the interface after a colon and having every
  method it requires: `struct Circle: Shape { … }`. Tessel checks that the
  methods match exactly.
- An interface is a type. A `[Shape]` can hold circles and squares together,
  and a function that takes `[Shape]` works for all of them.
- Calling a method through an interface runs the method of the value's real
  type.
- Through an interface you can only use what the interface lists. Use
  `is` and `as?` when you really need to know the real type.
- Interface values can be compared with `==` and saved with `toJson`.

For the full details, see [Interfaces](/language/interfaces/).

**Next:** [15. Generics](/learn/15-generics/)
