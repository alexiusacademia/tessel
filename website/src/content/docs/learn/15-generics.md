---
title: "15. Generics"
description: Write a function or type once and use it with any type, with type parameters, generic structs and enums, and constraints.
---

Lists already work with any type: `[Int]`, `[String]`, `[Book]`. You never
needed a separate "list of books" feature. In this lesson you'll learn to
write your own functions and types that work like that: written once, usable
with any type, and still fully checked by Tessel.

In this lesson you'll learn:

- why you'd want one function to work for many types
- how to write a **generic function** with a **type parameter**, `<T>`
- how Tessel works out the type at each call
- how to write generic structs, like a `Stack<T>`
- how a generic enum, `Result<T>`, gives you a clean way to report errors
- how to limit a type parameter to types with certain abilities, `<T: Shape>`
- when to use generics, and when an interface is the better fit

## The problem: the same function, again and again

Say you want the last item of a list, or a fallback value if the list is
empty. For numbers and for text, you'd write:

```tessel
fn lastOrInt(items: [Int], fallback: Int) -> Int {
    items.last ?? fallback
}

fn lastOrString(items: [String], fallback: String) -> String {
    items.last ?? fallback
}

fn main() {
    print(lastOrInt(items: [1, 2, 3], fallback: 0))
    print(lastOrString(items: [], fallback: "empty"))
}
```

```text
3
empty
```

The two bodies are exactly the same. Only the types differ. For `Float`,
`Bool` or `Book` you'd need yet another copy. And if you found a bug, you'd
have to fix every copy.

## Generic functions

Instead, write the function once, with a **type parameter**: a stand-in
name for "some type", in angle brackets after the function's name:

```tessel
fn lastOr<T>(items: [T], fallback: T) -> T {
    items.last ?? fallback
}

fn main() {
    print(lastOr(items: [1, 2, 3], fallback: 0))
    print(lastOr(items: ["red", "green"], fallback: "none"))
    let noNames: [String] = []
    print(lastOr(items: noNames, fallback: "nobody"))
    print(lastOr(items: [1.5, 2.5], fallback: 0.0))
}
```

```text
3
green
nobody
2.5
```

Read `fn lastOr<T>(items: [T], fallback: T) -> T` as: "for any type `T`,
`lastOr` takes a list of `T`s and a fallback `T`, and gives back a `T`."

A function like this is called **generic**, because it isn't tied to one
type: it works for a whole family of types. `T` is just a name. You could call it `Item`, but
a single capital letter like `T` (for "type") is the usual choice.

### Tessel works out `T` for you

You never write the type at the call. Tessel looks at the arguments: in
`lastOr(items: [1, 2, 3], fallback: 0)` the list holds `Int`s, so `T` is
`Int` for that call. Tessel then checks the call exactly as if you had
written `lastOrInt` by hand.

That means mixing types is still caught:

```tessel
print(lastOr(items: [1, 2, 3], fallback: "none"))
```

```text
error: expected `Int`, found `String`
 --> main.tsl:6:46
  |
6 |     print(lastOr(items: [1, 2, 3], fallback: "none"))
  |                                              ^^^^^^ this is `String`
```

The list made `T` an `Int`, so the fallback must be an `Int` too.

Generic code is also just as fast as code written for one type. When you
compile, Tessel makes a separate copy of `lastOr` for each type you actually
use it with, with the types filled in.

### More generic functions

Type parameters can appear anywhere a type can: in a list type, a result,
even a function type. This function builds a list by repeating an item:

```tessel
fn repeated<T>(item: T, times: Int) -> [T] {
    var out: [T] = []
    for _ in 0..times {
        out.append(item)
    }
    out
}

fn main() {
    print(repeated(item: "ho", times: 3).joined(separator: " "))
    print(repeated(item: 7, times: 4).sum())
}
```

```text
ho ho ho
28
```

And here's the `countWhere` idea from lesson 13, now for any kind of list:

```tessel
fn countMatching<T>(items: [T], test: fn(T) -> Bool) -> Int {
    var count = 0
    for item in items {
        if test(item) {
            count += 1
        }
    }
    count
}

fn main() {
    print(countMatching(items: [4, 15, 8, 23], test: { n in n > 10 }))
    print(countMatching(items: ["apple", "fig", "kiwi"]) { w in w.count < 5 })
}
```

```text
2
2
```

In the second call, `T` is `String`, so the block's `w` is a `String`.

## Generic structs

Types can have type parameters too. A **stack** is a pile of things where
you add to the top and take from the top, like a stack of plates. The last
thing you put on is the first thing you take off. A stack works the same
way whatever it holds, so it's a natural generic struct:

```tessel
struct Stack<T> {
    items: [T] = []

    fn push(item: T) {
        items.append(item)
    }

    fn pop() -> T? {
        if items.isEmpty {
            return nil
        }
        items.removeLast()
    }

    fn peek() -> T? {
        items.last
    }

    fn size() -> Int {
        items.count
    }
}

fn main() {
    var numbers = Stack<Int>()
    numbers.push(item: 1)
    numbers.push(item: 2)
    numbers.push(item: 3)
    print(numbers.pop() ?? 0)
    print(numbers.peek() ?? 0)
    print(numbers.size())

    var plates: Stack<String> = Stack()
    plates.push(item: "blue plate")
    plates.push(item: "red plate")
    print(plates.pop() ?? "no plates")
    print(plates.pop() ?? "no plates")
    print(plates.pop() ?? "no plates")
}
```

```text
3
2
2
red plate
blue plate
no plates
```

- `Stack<Int>` is a stack of `Int`s, `Stack<String>` a stack of `String`s.
  Inside the struct, every `T` becomes that type.
- `pop` returns an optional, `T?`, because the stack might be empty.
- The struct's methods use its `T`. (Methods can't have type parameters of
  their own; they share the struct's.)

Once it's a `Stack<Int>`, it only takes `Int`s:
`numbers.push(item: "four")` gives
``error: expected `Int`, found `String` ``.

A struct can have several type parameters, separated by commas:

```tessel
struct Pair<A, B> {
    first: A
    second: B
}

fn main() {
    let entry = Pair(first: "apples", second: 3)
    print("{entry.first}: {entry.second}")
    let pairs = [Pair(first: "x", second: 1.5), Pair(first: "y", second: 2.0)]
    print(pairs.map { p in p.second }.sum())
}
```

```text
apples: 3
3.5
```

Here Tessel worked out the types from the values: `entry` is a
`Pair<String, Int>`.

### Common mistake: not saying what `T` is

Tessel needs to know what `T` is. When you create an empty stack with
nothing to go on, it can't tell:

```tessel
var things = Stack()
```

```text
error: can't tell what `T` is in this use of `Stack`
 --> main.tsl:6:18
  |
6 |     var things = Stack()
  |                  ^^^^^^^
  |
  = help: write the type, like `Stack<Int>(…)`, or give the variable a type: `var x: Stack<Int> = Stack(…)`
```

Either write it out, `Stack<Int>()`, or give the variable a type,
`var things: Stack<Int> = Stack()`. In the same way, where you write a type
(for a parameter or a field), you must always say what's in it:
`fn count(stack: Stack<Int>)`, not `fn count(stack: Stack)`.

## Generic enums: a clean way to report errors

In lesson 10 you used optionals for "this might not have worked": `Int("ten")`
gives `nil`. But `nil` doesn't say *what* went wrong. Was the text empty? Not
a number? A number that's too big?

A generic enum solves this neatly. A `Result<T>` is either `ok` with a value
of type `T`, or a `failure` with a message explaining the problem:

```tessel
enum Result<T> {
    ok(value: T)
    failure(message: String)
}

fn parseAge(text: String) -> Result<Int> {
    let trimmed = text.trim()
    if let n = Int(trimmed) {
        if n < 0 {
            return .failure(message: "an age can't be negative")
        }
        if n > 150 {
            return .failure(message: "{n} is too old to be true")
        }
        return .ok(value: n)
    }
    Result.failure(message: "\"{trimmed}\" is not a number")
}

fn main() {
    for input in ["42", " 7 ", "-3", "200", "ten"] {
        match parseAge(text: input) {
            .ok(age) -> print("Age: {age}")
            .failure(message) -> print("Problem: {message}")
        }
    }
}
```

```text
Age: 42
Age: 7
Problem: an age can't be negative
Problem: 200 is too old to be true
Problem: "ten" is not a number
```

Why this is nice:

- The function's type, `-> Result<Int>`, tells anyone calling it: "this can
  fail, and you'll get a reason if it does."
- The caller **must** handle both cases. `match` won't let you forget the
  failure.
- The same `Result` works for any type: a `Result<Float>`, a
  `Result<Book>`. You write the enum once.

Notice the last line of `parseAge` starts with `Result.failure`, not
`.failure`. As in lesson 12, a line starting with `.` would continue the line
before it. After `return` on the same line, `.failure(…)` is fine.

### Common mistake: using a Result as if it were the value

A `Result<Int>` isn't an `Int`. It *might contain* one. So this doesn't work:

```tessel
let answer = safeDivide(a: 10, b: 2)
print(answer + 1)
```

```text
error: can't use `+` on `Result<Int>` and `Int`
  --> main.tsl:15:11
   |
15 |     print(answer + 1)
   |           ^^^^^^^^^^
   |
   = help: arithmetic works on two numbers of the same type
```

Use `match` to get the value out, and decide what to do if there isn't one.
That's the whole point: you can't accidentally ignore the error.

## Constraints

Inside a generic function, `T` could be *any* type. So you can't call a
method on it. What if `T` is `Bool`, which has no `area()`?

```tessel
fn biggest<T>(items: [T]) -> T? {
    var best: T? = nil
    for item in items {
        if item.area() > 0.0 {
            best = item
        }
    }
    best
}
```

```text
error: `T` could be any type, so it has no method `area`
 --> main.tsl:4:17
  |
4 |         if item.area() > 0.0 {
  |                 ^^^^
  |
  = help: require an interface that has it, like `<T: Shape>`
```

The help line shows the fix: a **constraint**. Writing `<T: Scored>` means
"`T` can be any type, as long as it conforms to the `Scored` interface".
Then you can use `Scored`'s methods on it:

```tessel
interface Scored {
    fn score() -> Int
}

struct Player: Scored {
    name: String
    points: Int
    fn score() -> Int { points }
}

struct Team: Scored {
    city: String
    wins: Int
    fn score() -> Int { wins * 3 }
}

fn best<T: Scored>(items: [T]) -> T? {
    var top: T? = nil
    for item in items {
        if item.score() > (top?.score() ?? -1) {
            top = item
        }
    }
    top
}

fn main() {
    let players = [Player(name: "Ada", points: 12), Player(name: "Ben", points: 30)]
    let teams = [Team(city: "Oslo", wins: 5), Team(city: "Lima", wins: 8)]

    if let p = best(items: players) {
        print("Best player: {p.name}")
    }
    if let t = best(items: teams) {
        print("Best team: {t.city}")
    }
}
```

```text
Best player: Ben
Best team: Lima
```

Look closely at `p.name`. `best(items: players)` returns a `Player?`, not
just "something scored", so you can use the player's `name` right away. For
teams, it returns a `Team?`, so `t.city` works.

## Interfaces or generics?

You've now seen two ways to write code that works with many types. They
solve different problems:

- **An interface type** like `[Shape]` means "a mix of different types that
  can all do the same things". Use it when one list really holds different
  kinds of things: circles *and* squares, dogs *and* cats.
- **A generic** like `<T>` or `<T: Scored>` means "one type, whichever the
  caller chooses". Every item in a `[T]` is the same type, and you get that
  exact type back, with all its fields, not just the interface's methods.

So `best(items: [T])` couldn't take a list of players and teams mixed
together. With `[Fruit(…), Ticket(…)]` Tessel says
``error: expected `Fruit`, found `Ticket` ``. For a mix, declare the list as
an interface type (`let mixed: [Priced] = …`). You can still pass that to a
generic function: `T` is then the interface itself.

A good rule of thumb: start with plain types. When you catch yourself
writing the same code for two types, reach for a generic. When you need to
keep different types side by side, reach for an interface.

## Worked example: undo

Almost every editor has an "undo" button. A stack is exactly the right tool
for it: before each change, push the old text; to undo, pop the most recent
old text and put it back. Here the generic `Stack` from above holds
`String`s:

```tessel
struct Stack<T> {
    items: [T] = []

    fn push(item: T) {
        items.append(item)
    }

    fn pop() -> T? {
        if items.isEmpty {
            return nil
        }
        items.removeLast()
    }
}

struct Editor {
    text: String = ""
    history: Stack<String> = Stack()

    fn write(_ more: String) {
        history.push(item: text)
        text += more
    }

    fn deleteAll() {
        history.push(item: text)
        text = ""
    }

    fn undo() {
        if let previous = history.pop() {
            text = previous
        } else {
            print("(nothing to undo)")
        }
    }
}

fn main() {
    var editor = Editor()
    editor.write("Hello")
    editor.write(", world")
    print(editor.text)

    editor.deleteAll()
    print("[{editor.text}]")

    editor.undo()
    print(editor.text)
    editor.undo()
    print(editor.text)
    editor.undo()
    print("[{editor.text}]")
    editor.undo()
}
```

```text
Hello, world
[]
Hello, world
Hello
[]
(nothing to undo)
```

- `history: Stack<String> = Stack()` starts each editor with an empty stack.
  The field's type says it holds `String`s, so `Stack()` needs nothing more.
- `write` and `deleteAll` save the old text *before* changing it.
- `undo` pops the most recent saved text. When the stack is empty, `pop`
  returns `nil` and there's nothing to undo.
- The `Stack` doesn't know anything about editors. The same struct could
  hold moves in a game (`Stack<Move>`) or pages in a browser's history
  (`Stack<String>` again). That's the payoff of writing it generically.

## Exercises

**1. Repeat anything.** Use the `repeated<T>(item:times:)` function from
this lesson with three different types: a `String`, an `Int` and a `Bool`.

<details>
<summary>Solution</summary>

```tessel
fn repeated<T>(item: T, times: Int) -> [T] {
    var out: [T] = []
    for _ in 0..times {
        out.append(item)
    }
    out
}

fn main() {
    print(repeated(item: "ho", times: 3).joined(separator: " "))
    print(repeated(item: 7, times: 4).sum())
    print(repeated(item: true, times: 2).count)
}
```

```text
ho ho ho
28
2
```

</details>

**2. A queue.** A **queue** is like a line at a shop: the first one in is the
first one out. Write a generic `Queue<T>` with `join(item:)`, which adds to
the back, and `next() -> T?`, which takes from the front (or gives `nil` if
the queue is empty). Hint: `removeFirst()` removes and returns a list's first
item.

<details>
<summary>Solution</summary>

```tessel
struct Queue<T> {
    items: [T] = []

    fn join(item: T) {
        items.append(item)
    }

    fn next() -> T? {
        if items.isEmpty {
            return nil
        }
        items.removeFirst()
    }
}

fn main() {
    var line = Queue<String>()
    line.join(item: "Ada")
    line.join(item: "Ben")
    line.join(item: "Cleo")
    print(line.next() ?? "nobody")
    print(line.next() ?? "nobody")
    line.join(item: "Dev")
    print(line.next() ?? "nobody")
    print(line.next() ?? "nobody")
    print(line.next() ?? "nobody")
}
```

```text
Ada
Ben
Cleo
Dev
nobody
```

</details>

**3. A safe index.** Reading `list[5]` from a list of 3 items stops the
program. Write `itemAt<T>(items: [T], index: Int) -> Result<T>` that returns
the item, or a failure explaining the problem. Test it on a list of strings
and a list of numbers.

<details>
<summary>Solution</summary>

```tessel
enum Result<T> {
    ok(value: T)
    failure(message: String)
}

fn itemAt<T>(items: [T], index: Int) -> Result<T> {
    if index < 0 || index >= items.count {
        return .failure(message: "there is no item {index}, the list has {items.count}")
    }
    Result.ok(value: items[index])
}

fn main() {
    let colors = ["red", "green", "blue"]
    for i in [1, 5] {
        match itemAt(items: colors, index: i) {
            .ok(color) -> print("Item {i} is {color}")
            .failure(message) -> print("Oops: {message}")
        }
    }
    match itemAt(items: [10, 20], index: 0) {
        .ok(n) -> print(n * 2)
        .failure(message) -> print(message)
    }
}
```

```text
Item 1 is green
Oops: there is no item 5, the list has 3
20
```

In the last `match`, `T` is `Int`, so `n` is an `Int` and `n * 2` works.

</details>

**4. Adding up prices.** Make an interface `Priced` with `price() -> Int`,
and two structs that conform: `Fruit` (price per kilo times kilos) and
`Ticket` (25 per seat). Write one generic function
`totalPrice<T: Priced>(items: [T]) -> Int` and use it on a basket of fruit
and on a list of tickets.

<details>
<summary>Solution</summary>

```tessel
interface Priced {
    fn price() -> Int
}

struct Fruit: Priced {
    name: String
    pricePerKilo: Int
    kilos: Int
    fn price() -> Int { pricePerKilo * kilos }
}

struct Ticket: Priced {
    event: String
    seats: Int
    fn price() -> Int { seats * 25 }
}

fn totalPrice<T: Priced>(items: [T]) -> Int {
    var total = 0
    for item in items {
        total += item.price()
    }
    total
}

fn main() {
    let basket = [Fruit(name: "apples", pricePerKilo: 3, kilos: 2), Fruit(name: "cherries", pricePerKilo: 12, kilos: 1)]
    let tickets = [Ticket(event: "concert", seats: 2)]
    print(totalPrice(items: basket))
    print(totalPrice(items: tickets))
}
```

```text
18
50
```

</details>

## Summary

- A **generic function** has **type parameters** in angle brackets,
  `fn lastOr<T>(items: [T], fallback: T) -> T`, and works for any type.
- Tessel works out `T` from the arguments at each call, and checks the call
  as if it had been written for that type.
- **Generic structs** like `Stack<T>` and `Pair<A, B>` hold values of any
  type. Write `Stack<Int>()`, or give the variable a type.
- A generic enum like `Result<T>` (`ok` or `failure`) reports errors with a
  reason, and `match` makes sure the caller handles them.
- A **constraint**, `<T: Scored>`, allows only types that conform to an
  interface, so you can use that interface's methods on `T`.
- Use an **interface type** to mix different types in one list; use a
  **generic** when it's one type chosen by the caller.

For the full details, see [Generics](/language/generics/).

**Next:** [16. Files and saving data](/learn/16-files-data/)
