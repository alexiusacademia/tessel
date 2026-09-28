---
title: "12. Enums and match"
description: Make a type whose value is one of a fixed set of choices, and handle every choice safely with match.
---

A traffic light is red, yellow or green. Never purple, never "grean". A
compass direction is north, east, south or west. Lots of things in programs
are like this: a value that is **one of a few choices**. In this lesson
you'll learn the type made exactly for that.

In this lesson you'll learn:

- how to declare an **enum**, a type with a fixed list of cases
- how to use `match` to do something different for each case
- why Tessel insists that a `match` handles every case
- how cases can carry extra values, like the radius of a circle
- how to give an enum methods
- how to use enums to keep track of the state of something, like an order or
  a vending machine

## The problem with strings

You could store a traffic light's color as a `String`. But look at this:

```tessel
fn main() {
    let light = "gren"
    if light == "green" {
        print("Go")
    } else {
        print("Stop")
    }
}
```

```text
Stop
```

There's a typo, `"gren"`, and nothing tells you. Tessel can't know which
strings are sensible colors, so any string is allowed. The program quietly
does the wrong thing.

## Declaring an enum

An **enum** (short for "enumeration", a list of possibilities) is a type
whose value is always exactly one of the **cases** you list:

```tessel
enum Direction {
    north
    east
    south
    west
}

fn main() {
    var heading = Direction.north
    print(heading)
    heading = .west
    print("Now heading {heading}")
    if heading == .west {
        print("The sun sets this way")
    }
}
```

```text
north
Now heading west
The sun sets this way
```

- To name a case, write the enum's name, a dot and the case:
  `Direction.north`.
- When Tessel already knows a `Direction` is expected, the name can be left
  out: `heading = .west`. `heading` is a `Direction`, so `.west` can only
  mean `Direction.west`.
- Printing a case gives its name.
- Cases can also go on one line, separated by commas:
  `enum Direction { north, east, south, west }`.

Now a typo is caught before the program even runs:

```text
error: `Direction` has no case `.up`
 --> main.tsl:5:15
  |
5 |     heading = .up
  |               ^^^ unknown case
  |
  = help: its cases are `.north`, `.east`, `.south`, `.west`
```

That's the main reason to use an enum: Tessel knows every possible value,
so it can check your work.

## match

You'll often want to do something different for each case. You could write a
chain of `if … else if …`, but there's a better tool, `match`:

```tessel
enum TrafficLight { red, yellow, green }

fn instruction(light: TrafficLight) -> String {
    match light {
        .red -> "Stop"
        .yellow -> "Slow down"
        .green -> "Go"
    }
}

fn main() {
    print(instruction(light: .red))
    print(instruction(light: .green))
}
```

```text
Stop
Go
```

`match` looks at a value and picks the **arm** that fits. Each arm is a
pattern, an arrow `->`, and what to do. Like `if`, a `match` gives back a
value: the value of the arm that ran. Here it's the last expression of the
function, so it's the function's result.

### Common mistake: forgetting a case

A `match` must handle **every** case. Leave one out, and Tessel refuses to
run the program:

```tessel
fn instruction(light: TrafficLight) -> String {
    match light {
        .red -> "Stop"
        .green -> "Go"
    }
}
```

```text
error: this `match` doesn't handle `.yellow`
 --> main.tsl:4:11
  |
4 |     match light {
  |           ^^^^^
  |
  = help: add arms for them, or `_ -> …` to handle the rest
```

This might feel strict, but it's one of the most useful checks Tessel does.
Imagine you later add a fourth case, `flashing`, to `TrafficLight`. Every
`match` in your program that forgot about it becomes an error, and Tessel
shows you each place that needs updating. Nothing slips through.

### The `_` arm

Sometimes you only care about one or two cases. An arm with `_` matches
anything that the arms above it didn't:

```tessel
enum TrafficLight { red, yellow, green }

fn canDrive(light: TrafficLight) -> Bool {
    match light {
        .green -> true
        _ -> false
    }
}

fn main() {
    print(canDrive(light: .yellow))
    print(canDrive(light: .green))
}
```

```text
false
true
```

Use `_` with care: it also swallows any case you add later, so you lose the
reminder described above.

`match` works on other values too, like numbers and strings. There are too
many possible numbers to list, so there you always need a `_` arm:

```text
error: this `match` must handle every possible `Int`
  = help: add a `_ -> …` arm for all other values
```

### Arms with several lines

To run more than one line in an arm, wrap the arm in braces:

```tessel
enum Direction { north, east, south, west }

fn main() {
    let heading = Direction.east
    match heading {
        .north -> print("Up")
        .south -> print("Down")
        _ -> {
            print("Sideways")
            print("({heading})")
        }
    }
}
```

```text
Sideways
(east)
```

### Common mistake: changing a variable in an arm

An arm without braces must be a value. Changing a variable isn't a value, so
this doesn't work:

```tessel
match light {
    .red -> speed = 0
    .green -> { speed = 50 }
}
```

```text
error: expected end of line after the match arm, found `=`
 --> main.tsl:6:23
  |
6 |         .red -> speed = 0
  |                       ^
  |
  = help: to change something in an arm, put it in braces: `.case -> { x = 1 }`
```

The second arm is written the right way: `.red -> { speed = 0 }`.

## Cases with values

Some choices come with extra information. A shape might be a circle or a
rectangle, but a circle also has a radius, and a rectangle has a width and a
height. Cases can carry values, written like function parameters:

```tessel
enum Shape {
    circle(radius: Float)
    rect(width: Float, height: Float)
}

fn area(shape: Shape) -> Float {
    match shape {
        .circle(r) -> 3.14159 * r * r
        .rect(w, h) -> w * h
    }
}

fn main() {
    let shapes = [Shape.circle(radius: 1.0), Shape.rect(width: 2.0, height: 3.0)]
    for shape in shapes {
        print(area(shape: shape))
    }
}
```

```text
3.14159
6.0
```

- Create one by calling the case with labels:
  `Shape.circle(radius: 1.0)`.
- In a `match` arm, `.circle(r)` matches any circle and gives its radius the
  name `r` for that arm. `.rect(w, h)` names the width `w` and the height
  `h`, in the order they were declared. You pick the names.
- Use `_` for a value you don't need, as in `.rect(w, _)`, or leave off the
  parentheses to match the case whatever its values: `.circle -> "round"`.

Why not a struct with a `radius`, a `width` and a `height`? Because then
every shape would have all three, and a circle's `width` would be
meaningless. With an enum, a circle has *only* a radius. You can't even ask
a shape for its width without first finding out, with `match`, that it's a
rectangle:

```text
error: `Shape` has no property `width`
```

### Common mistake: a line starting with a dot

A line that starts with `.` continues the line before it. (That's what lets
you split a long chain of method calls over several lines, and you'll use it
a lot when you build apps later.) So this function goes wrong:

```tessel
fn unitCircle() -> Shape {
    let size = 1.0
    .circle(radius: size)
}
```

Tessel reads the last two lines as one, `let size = 1.0.circle(radius: size)`,
and complains:

```text
error: `Float` has no method `circle`
 --> main.tsl:8:6
  |
8 |     .circle(radius: size)
  |      ^^^^^^ unknown

error: this block must end with `Shape`
 --> main.tsl:9:1
  |
9 | }
  | ^ expected `Shape` before this
  |
  = help: put the value on the last line, or use `return …`
```

The fix: at the start of a line, write the enum's name,
`Shape.circle(radius: size)`. That's why the lists in this lesson use
`Shape.circle(…)` for every item on its own line.

## Methods on enums

Like structs, enums can have methods. Inside one, `self` is the value the
method was called on, and you usually `match` on it:

```tessel
enum Shape {
    circle(radius: Float)
    rect(width: Float, height: Float)

    fn area() -> Float {
        match self {
            .circle(r) -> 3.14159 * r * r
            .rect(w, h) -> w * h
        }
    }

    fn name() -> String {
        match self {
            .circle -> "circle"
            .rect(w, h) -> if w == h { "square" } else { "rectangle" }
        }
    }
}

fn main() {
    let shapes = [
        Shape.circle(radius: 2.0),
        Shape.rect(width: 3.0, height: 3.0),
        Shape.rect(width: 2.0, height: 5.0),
    ]
    for shape in shapes {
        print("a {shape.name()} with area {shape.area().formatted(decimals: 2)}")
    }
}
```

```text
a circle with area 12.57
a square with area 9.00
a rectangle with area 10.00
```

A method can even replace the whole value by assigning to `self`. Like a
struct method that changes a field, it can then only be called on a `var`:

```tessel
enum Light {
    red, yellow, green

    fn next() -> Light {
        match self {
            .red -> .green
            .green -> .yellow
            .yellow -> .red
        }
    }

    fn advance() {
        self = next()
    }
}

fn main() {
    var light = Light.red
    light.advance()
    print(light)
    light.advance()
    print(light)
}
```

```text
green
yellow
```

Enum values can be compared with `==`, values included:
`Shape.circle(radius: 1.0) == Shape.circle(radius: 1.0)` is `true`.

## Modeling states

Enums are perfect for keeping track of what **state** something is in. Take
an online order. It's placed, then shipped (with a tracking number), then
delivered (on some day), or it might be cancelled (for some reason).

You could use a struct with fields like `isShipped: Bool`,
`trackingNumber: String?`, `isCancelled: Bool` and so on. But then nothing
stops an order from being both cancelled and delivered, or shipped without a
tracking number. With an enum, the order is in exactly one state, and each
state carries exactly the information that goes with it:

```tessel
enum OrderStatus {
    placed
    shipped(tracking: String)
    delivered(day: String)
    cancelled(reason: String)

    fn describe() -> String {
        match self {
            .placed -> "We got your order"
            .shipped(code) -> "On its way, tracking number {code}"
            .delivered(day) -> "Delivered on {day}"
            .cancelled(why) -> "Cancelled: {why}"
        }
    }

    fn canCancel() -> Bool {
        match self {
            .placed -> true
            _ -> false
        }
    }
}

struct Order {
    item: String
    status: OrderStatus = .placed
}

fn main() {
    var order = Order(item: "Headphones")
    print(order.status.describe())
    print("Can cancel: {order.status.canCancel()}")

    order.status = .shipped(tracking: "ZX-4410")
    print(order.status.describe())
    print("Can cancel: {order.status.canCancel()}")

    order.status = .delivered(day: "Friday")
    print(order.status.describe())
}
```

```text
We got your order
Can cancel: true
On its way, tracking number ZX-4410
Can cancel: false
Delivered on Friday
```

Notice how structs and enums work together: the `Order` struct has a field
whose type is an enum.

## Worked example: a vending machine

A **state machine** is something that is in one state at a time, and moves
to another state when an event happens. Vending machines, traffic lights and
game characters are all state machines, and enums describe them well.

Our machine is either `waiting` for money, or has been `paid` some cents.
Three things can happen to it: a coin goes in, someone chooses an item, or
someone presses "refund". Both the states and the events are enums:

```tessel
enum Event {
    coin(cents: Int)
    choose(item: String, price: Int)
    refund
}

enum Machine {
    waiting
    paid(cents: Int)

    fn credit() -> Int {
        match self {
            .waiting -> 0
            .paid(cents) -> cents
        }
    }

    fn handle(event: Event) -> String {
        match event {
            .coin(cents) -> {
                self = .paid(cents: credit() + cents)
                "Credit: {credit()} cents"
            }
            .choose(item, price) -> {
                if credit() < price {
                    return "{item} costs {price} cents, you have {credit()}"
                }
                let change = credit() - price
                self = .waiting
                "Here is your {item}, change: {change} cents"
            }
            .refund -> {
                let back = credit()
                self = .waiting
                "Returned {back} cents"
            }
        }
    }
}

fn main() {
    var machine = Machine.waiting
    let events = [
        Event.choose(item: "juice", price: 150),
        Event.coin(cents: 100),
        Event.choose(item: "juice", price: 150),
        Event.coin(cents: 100),
        Event.choose(item: "juice", price: 150),
        Event.coin(cents: 20),
        Event.refund,
    ]
    for event in events {
        print(machine.handle(event: event))
    }
}
```

```text
juice costs 150 cents, you have 0
Credit: 100 cents
juice costs 150 cents, you have 100
Credit: 200 cents
Here is your juice, change: 50 cents
Credit: 20 cents
Returned 20 cents
```

How it works:

- `credit()` answers "how much money is in the machine?" for either state.
- `handle` matches on the **event**, and each arm decides the machine's next
  state by assigning to `self`. Each arm also returns a message: the last
  line of an arm in braces is its value.
- In `.choose`, `return` leaves the method early when there isn't enough
  money, and the state stays as it was.
- Because `handle` assigns to `self`, `machine` must be a `var`.

If you add a new kind of event later, say `.restock`, Tessel points at the
`match` in `handle` until you decide what the machine should do with it.

## Exercises

**1. Seasons.** Make an enum `Season` with the four seasons and a method
`next()` that returns the season after it. Starting from autumn, print five
seasons in a row.

<details>
<summary>Solution</summary>

```tessel
enum Season {
    spring, summer, autumn, winter

    fn next() -> Season {
        match self {
            .spring -> .summer
            .summer -> .autumn
            .autumn -> .winter
            .winter -> .spring
        }
    }
}

fn main() {
    var season = Season.autumn
    for _ in 0..5 {
        print(season)
        season = season.next()
    }
}
```

```text
autumn
winter
spring
summer
autumn
```

</details>

**2. Counting coins.** Make an enum `Coin` with the cases `penny`, `nickel`,
`dime` and `quarter`, and a method `cents()` that gives 1, 5, 10 and 25. Add
up the value of a list of coins.

<details>
<summary>Solution</summary>

```tessel
enum Coin {
    penny, nickel, dime, quarter

    fn cents() -> Int {
        match self {
            .penny -> 1
            .nickel -> 5
            .dime -> 10
            .quarter -> 25
        }
    }
}

fn main() {
    let purse = [Coin.quarter, Coin.dime, Coin.dime, Coin.penny, Coin.nickel]
    var total = 0
    for coin in purse {
        total += coin.cents()
    }
    print("{purse.count} coins, {total} cents")
}
```

```text
5 coins, 51 cents
```

</details>

**3. Temperatures.** Make an enum `Temperature` with two cases that carry a
value: `celsius(degrees: Float)` and `fahrenheit(degrees: Float)`. Give it a
method `inCelsius() -> Float`. (To convert, subtract 32 and multiply by
5/9.) Print a few readings in Celsius with one decimal.

<details>
<summary>Solution</summary>

```tessel
enum Temperature {
    celsius(degrees: Float)
    fahrenheit(degrees: Float)

    fn inCelsius() -> Float {
        match self {
            .celsius(d) -> d
            .fahrenheit(d) -> (d - 32.0) * 5.0 / 9.0
        }
    }
}

fn main() {
    let readings = [Temperature.celsius(degrees: 21.0), Temperature.fahrenheit(degrees: 98.6), Temperature.fahrenheit(degrees: 32.0)]
    for t in readings {
        print(t.inCelsius().formatted(decimals: 1))
    }
}
```

```text
21.0
37.0
0.0
```

</details>

**4. A robot.** A robot starts at `(0, 0)` facing north. It understands three
commands: `forward(steps: Int)`, `left` and `right` (turning a quarter turn).
Make a `Direction` enum with methods `turnedLeft()` and `turnedRight()`, a
`Command` enum, and a `Robot` struct with a method `run(command:)`. Run the
commands forward 3, right, forward 2, right, forward 5, and print where the
robot ends up. (North adds to `y`, east adds to `x`.)

<details>
<summary>Solution</summary>

```tessel
enum Direction {
    north, east, south, west

    fn turnedRight() -> Direction {
        match self {
            .north -> .east
            .east -> .south
            .south -> .west
            .west -> .north
        }
    }

    fn turnedLeft() -> Direction {
        match self {
            .north -> .west
            .west -> .south
            .south -> .east
            .east -> .north
        }
    }
}

enum Command {
    forward(steps: Int)
    left
    right
}

struct Robot {
    x: Int = 0
    y: Int = 0
    facing: Direction = .north

    fn run(command: Command) {
        match command {
            .left -> { facing = facing.turnedLeft() }
            .right -> { facing = facing.turnedRight() }
            .forward(steps) -> {
                match facing {
                    .north -> { y += steps }
                    .south -> { y -= steps }
                    .east -> { x += steps }
                    .west -> { x -= steps }
                }
            }
        }
    }
}

fn main() {
    var robot = Robot()
    let commands = [Command.forward(steps: 3), Command.right, Command.forward(steps: 2), Command.right, Command.forward(steps: 5)]
    for command in commands {
        robot.run(command: command)
    }
    print("The robot is at ({robot.x}, {robot.y}), facing {robot.facing}")
}
```

```text
The robot is at (2, -2), facing south
```

The arms that change a field are in braces, because changing a variable
isn't a value.

</details>

## Summary

- An **enum** is a type whose value is exactly one of a fixed set of
  **cases**: `enum Direction { north, east, south, west }`.
- Write a case as `Direction.north`, or just `.north` where the type is
  already known. At the start of a line, always write the enum's name.
- `match` picks the arm that fits a value. It must handle **every** case,
  which Tessel checks for you. `_` matches everything else.
- Cases can carry values, `circle(radius: Float)`, which a `match` arm gets
  back out with `.circle(r)`.
- Enums can have methods. A method that assigns to `self` changes the value,
  so it needs a `var`.
- Enums are a great way to model **states**: something that is in exactly one
  situation at a time, with just the information that goes with it.

For the full details, see [Structs and enums](/language/structs-enums/#enums)
and [match](/language/control-flow/#match).

**Next:** [13. Functions as values](/learn/13-closures/)
