---
title: "13. Functions as values"
description: Store functions in variables, pass blocks of code to map, filter and sorted, and write functions that make new functions.
---

You've been passing values like numbers, text and lists to functions. In
this lesson you'll pass something new: a piece of **code**. That sounds
strange at first, but it's one of the most useful ideas in programming. It
lets one function do many different jobs, depending on the code you hand it.

In this lesson you'll learn:

- how to write a **block** of code, `{ x in … }`, that you can pass around
- how to use blocks with `map`, `filter`, `sorted(by:)` and `first(where:)`
- how to store a function in a variable, and what a **function type** like
  `fn(Int) -> Int` means
- how to write your own functions that take functions
- how a function can make and return a new function
- how blocks can use and change the variables around them
- how to write a block after the parentheses, as a **trailing block**

## The problem: almost the same function twice

Here are two functions that count numbers in a list: one counts numbers
bigger than 10, the other counts even numbers.

```tessel
fn countBig(numbers: [Int]) -> Int {
    var count = 0
    for n in numbers {
        if n > 10 {
            count += 1
        }
    }
    count
}

fn countEven(numbers: [Int]) -> Int {
    var count = 0
    for n in numbers {
        if n % 2 == 0 {
            count += 1
        }
    }
    count
}

fn main() {
    let numbers = [4, 15, 8, 23, 42]
    print(countBig(numbers: numbers))
    print(countEven(numbers: numbers))
}
```

```text
3
3
```

They're identical except for one line: the test inside the `if`. If you
wanted to count odd numbers or negative numbers, you'd copy the whole thing
again. What you really want is to write the loop **once**, and hand it the
test.

## Blocks

In Tessel you can do exactly that. A **block** is a piece of code in braces
that you can pass around like a value:

```tessel
fn countWhere(numbers: [Int], test: fn(Int) -> Bool) -> Int {
    var count = 0
    for n in numbers {
        if test(n) {
            count += 1
        }
    }
    count
}

fn main() {
    let numbers = [4, 15, 8, 23, 42]
    print(countWhere(numbers: numbers, test: { n in n > 10 }))
    print(countWhere(numbers: numbers, test: { n in n % 2 == 0 }))
}
```

```text
3
3
```

Read `{ n in n > 10 }` as "given `n`, the answer is `n > 10`":

- The names before `in` are the block's **parameters**, here just `n`.
- After `in` comes the body. Like a function body, its **last line is the
  result**.
- `countWhere` runs the block with `test(n)`, once for each number.

The parameter `test` has the type `fn(Int) -> Bool`: "a function that takes
an `Int` and gives back a `Bool`". You'll see more of these types below.

Blocks are also called **closures**. You'll see both words in the Tessel
docs; they mean the same thing.

## map, filter and friends

You'll rarely need to write `countWhere` yourself, because lists come with
methods that take blocks. The two you'll use most are `map` and `filter`.

**`map`** makes a new list by running the block on every item:

```tessel
fn main() {
    let prices = [12, 5, 30, 8]

    let labels = prices.map { p in "${p}" }
    print(labels.joined(separator: ", "))

    let doubled = prices.map { p in p * 2 }
    print(doubled.sum())
}
```

```text
$12, $5, $30, $8
110
```

The new list can have a different type from the old one: `labels` is a
`[String]` made from an `[Int]`. That's handy for printing, since a list of
numbers can't be printed directly but a list of strings can be joined.

**`filter`** keeps only the items for which the block says `true`:

```tessel
fn main() {
    let prices = [12, 5, 30, 8]
    let cheap = prices.filter { p in p < 10 }
    print(cheap.count)
    print(cheap.map { p in "${p}" }.joined(separator: ", "))
}
```

```text
2
$5, $8
```

Neither method changes the original list. They give you a new one.

Notice there are no parentheses around the block in `prices.map { … }`.
When a block is the only argument, you can leave them out. More on that in
[Trailing blocks](#trailing-blocks) below.

### sorted(by:)

`sorted()` puts numbers and strings in their natural order. With `by:`, you
decide the order. The block gets two items, `a` and `b`, and returns `true`
when `a` should come before `b`:

```tessel
fn main() {
    let names = ["Grace", "Al", "Linus", "Ada"]
    print(names.sorted().joined(separator: ", "))
    print(names.sorted(by: { a, b in a.count < b.count }).joined(separator: ", "))
    print(names.sorted(by: { a, b in a > b }).joined(separator: ", "))
}
```

```text
Ada, Al, Grace, Linus
Al, Ada, Grace, Linus
Linus, Grace, Al, Ada
```

The second line sorts shortest first. "Al" and "Ada" both come before the
longer names; the third line sorts backwards.

A block with two parameters lists both names before `in`: `{ a, b in … }`.

### first(where:)

`first(where:)` finds the first item that passes the test. There might not
be one, so it gives back an optional:

```tessel
fn main() {
    let names = ["Grace", "Al", "Linus", "Ada"]
    let long = names.first(where: { n in n.count > 4 })
    print(long ?? "none")
    let veryLong = names.first(where: { n in n.count > 10 })
    print(veryLong ?? "none")
}
```

```text
Grace
none
```

Other list methods that take blocks include `count(where:)`,
`any(where:)`, `all(where:)`, `firstIndex(where:)` and `removeAll(where:)`.
See the [List reference](/reference/stdlib/#list).

### Blocks with structs

Blocks really shine with lists of structs. You can sort books by pages, or
pick out just their titles, in one line each:

```tessel
struct Book {
    title: String
    pages: Int
}

fn main() {
    let books = [Book(title: "Dune", pages: 412), Book(title: "Matilda", pages: 240), Book(title: "The Hobbit", pages: 310)]
    let shortestFirst = books.sorted(by: { a, b in a.pages < b.pages })
    for book in shortestFirst {
        print("{book.pages} {book.title}")
    }
    let titles = books.filter { b in b.pages > 300 }.map { b in b.title }
    print(titles.joined(separator: " and "))
}
```

```text
240 Matilda
310 The Hobbit
412 Dune
Dune and The Hobbit
```

The last lines **chain** two methods: `filter` makes a list of long books,
and `map` turns that into a list of titles.

A struct has no natural order, so a list of structs always needs a `by:`
block. Forget it, and Tessel tells you:

```text
error: `Book` values have no natural order, so `sorted()` needs a rule
 --> main.tsl:8:18
  |
8 |     let sorted = books.sorted()
  |                  ^^^^^^^^^^^^^^
  |
  = help: pass one, like `sorted(by: { a, b in a.title < b.title })`
```

### Longer blocks

A block can have several lines. As in a function, the last line is the
result:

```tessel
fn main() {
    let numbers = [3, 1, 2]
    let bigger = numbers.map { n in
        let doubled = n * 2
        doubled + 1
    }
    print(bigger.sum())
}
```

```text
15
```

## Storing a function in a variable

A block is a value, so you can store it in a variable and call it later,
like a function:

```tessel
fn main() {
    let double: fn(Int) -> Int = { n in n * 2 }
    let add: fn(Int, Int) -> Int = { a, b in a + b }
    let greet: fn(String) = { name in print("Hello, {name}!") }
    let sayBye = { print("Bye!") }

    print(double(21))
    print(add(2, 3))
    greet("Sam")
    sayBye()
}
```

```text
42
5
Hello, Sam!
Bye!
```

### Function types

The type of a function value is written `fn(…) -> …`: the types it takes in
parentheses, then an arrow and the type it gives back.

| Type | Means | Example block |
|---|---|---|
| `fn(Int) -> Int` | takes an `Int`, gives an `Int` | `{ n in n * 2 }` |
| `fn(Int, Int) -> Int` | takes two `Int`s, gives an `Int` | `{ a, b in a + b }` |
| `fn(String)` | takes a `String`, gives nothing back | `{ s in print(s) }` |
| `fn()` | takes nothing, gives nothing back | `{ print("Bye!") }` |

A block with no parameters has no `in` at all.

### Common mistake: a block with no type

Look again at `double` above. Why write out `fn(Int) -> Int`? Try leaving
it off:

```tessel
fn main() {
    let increment = { n in n + 1 }
}
```

```text
error: can't tell the types of this block's parameters
 --> main.tsl:2:21
  |
2 |     let increment = { n in n + 1 }
  |                     ^^^^^^^^^^^^^^
  |
  = help: pass it where a function is expected, or give it a type: `let f: fn(Int) -> Int = { x in … }`
```

Is `n` a number? Some text? Tessel can't tell. When you pass a block to
`map` or `filter`, Tessel knows the type from the list, so you never write
it there. When you store a block in a variable, give the variable a type.
(A block with no parameters, like `sayBye`, has nothing to guess, so it
doesn't need one.)

### Named functions are values too

A function you declared with `fn` can be used as a value by writing its name
without parentheses:

```tessel
fn isEven(n: Int) -> Bool {
    n % 2 == 0
}

fn main() {
    let check: fn(Int) -> Bool = isEven
    print(check(4))
    print([1, 2, 3, 4, 6].filter(isEven).count)
}
```

```text
true
3
```

`isEven` (no parentheses) is the function itself; `isEven(n: 4)` would call
it.

One difference: a function value is called **without labels**. Write
`check(4)`, not `check(n: 4)`, or you get:

```text
error: function values take their arguments without labels
 --> main.tsl:7:17
  |
7 |     print(check(n: 4))
  |                 ^
  |
  = help: remove `n:`
```

### Changing which function a variable holds

A `var` can hold different functions over time, and a list can hold several:

```tessel
fn main() {
    var operation: fn(Int, Int) -> Int = { a, b in a + b }
    print(operation(6, 3))
    operation = { a, b in a * b }
    print(operation(6, 3))

    let steps: [fn(Int) -> Int] = [{ n in n + 1 }, { n in n * 10 }, { n in n - 3 }]
    var value = 4
    for step in steps {
        value = step(value)
    }
    print(value)
}
```

```text
9
18
47
```

The list runs each step in turn: 4 + 1 is 5, times 10 is 50, minus 3 is 47.

## Functions that return functions

A function can also **make** a function and give it back. Here
`makeAdder` returns a block that adds a fixed amount:

```tessel
fn makeAdder(amount: Int) -> fn(Int) -> Int {
    { n in n + amount }
}

fn main() {
    let addTen = makeAdder(amount: 10)
    let addOne = makeAdder(amount: 1)
    print(addTen(5))
    print(addOne(5))
}
```

```text
15
6
```

Read the return type `fn(Int) -> Int` as "a function from `Int` to `Int`".
The body is a single block, which is the last expression, so it's the
result.

Notice that the block uses `amount`, a parameter of `makeAdder`. Even after
`makeAdder` has finished, `addTen` still remembers that its `amount` was 10.
That's called **capturing**.

## Capturing variables

A block can use the variables around it. It doesn't take a copy: it uses
the variable itself. So a block can even change a `var` from outside:

```tessel
fn main() {
    var total = 0
    let addFive = { total += 5 }
    addFive()
    addFive()
    print(total)
}
```

```text
10
```

Put that together with returning a function, and you can make a counter
that keeps its own private count:

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
    let other = makeCounter()
    print(other())
}
```

```text
1
2
3
1
```

Each call to `makeCounter` creates a fresh `count`, and the block it returns
keeps that `count` alive. Nothing else in the program can reach it: the only
way to change it is to call `next()`. And `other` has its own `count`, so it
starts again at 1.

This is the one place where Tessel shares instead of copying. In the last
lesson you learned that structs are values: copying one gives you a separate
copy. A block's captured variables are different: if you copy `next` with
`let again = next`, both names advance the *same* count.

## Trailing blocks

When a block is the **last** argument of a call, you can write it after the
closing parenthesis. If it's the only argument, you can drop the
parentheses altogether. That's why you've been writing `prices.map { … }`.

This works for your own functions too:

```tessel
fn repeatTimes(times: Int, action: fn()) {
    for _ in 0..times {
        action()
    }
}

fn measure(label: String, work: fn() -> Int) {
    print("{label}: {work()}")
}

fn main() {
    repeatTimes(times: 2, action: { print("hip hip") })
    repeatTimes(times: 2) {
        print("hooray!")
    }
    measure(label: "answer") {
        6 * 7
    }
}
```

```text
hip hip
hip hip
hooray!
hooray!
answer: 42
```

The first two calls do the same thing. The second, with the trailing block,
reads more like a built-in part of the language, especially when the block
has several lines. (The label, `action:`, is left out when the block
trails.)

### Common mistake: a trailing block in an `if`

In the first line of an `if`, `for` or `match`, a `{` means "here comes the
body". So a trailing block there confuses things:

```tessel
fn main() {
    let numbers = [1, 2, 3]
    if numbers.filter { n in n > 5 }.isEmpty {
        print("none")
    }
}
```

```text
error: a block can't follow `filter` here
 --> main.tsl:3:23
  |
3 |     if numbers.filter { n in n > 5 }.isEmpty {
  |                       ^ this looks like a block for `filter`
  |
  = help: in `if`, `for` and `match` headers, pass it in parentheses: `filter({ … })`
```

Do what the help line says: `if numbers.filter({ n in n > 5 }).isEmpty { … }`.

## When to use blocks

- **To say "how" to a function that does the "what".** `sorted` knows how
  to sort; you tell it what order you want. `filter` knows how to build a
  new list; you tell it what to keep.
- **To avoid copying almost-identical code.** If two functions differ only
  in one small step, pass the step as a block, as `countWhere` did.
- **To say what should happen later.** When you build apps, a button takes a
  block that runs when it's clicked: `Button("Save") { save() }`.

A plain `for` loop is still fine, and sometimes clearer, especially when you
do several things per item. Use a block when it makes your code shorter
*and* easier to read.

## Worked example: a scoreboard

Here's a small scoreboard. `printRanking` knows how to print a numbered
list; the block passed to it decides the order.

```tessel
struct Player {
    name: String
    score: Int
    level: Int
}

fn printRanking(players: [Player], title: String, rank: fn(Player, Player) -> Bool) {
    print(title)
    let ordered = players.sorted(by: rank)
    for i in ordered.indices {
        print("  {i + 1}. {ordered[i].name}")
    }
}

fn main() {
    let players = [
        Player(name: "Ada", score: 420, level: 3),
        Player(name: "Ben", score: 980, level: 5),
        Player(name: "Cleo", score: 610, level: 5),
        Player(name: "Dev", score: 150, level: 1),
    ]

    printRanking(players: players, title: "By score:") { a, b in
        a.score > b.score
    }
    printRanking(players: players, title: "By name:") { a, b in
        a.name < b.name
    }

    let experts = players.filter { p in p.level >= 5 }.map { p in p.name }
    print("Experts: {experts.joined(separator: ", ")}")

    let total = players.map { p in p.score }.sum()
    print("Total score: {total}")

    if let first = players.first(where: { p in p.score > 500 }) {
        print("First over 500: {first.name}")
    }
}
```

```text
By score:
  1. Ben
  2. Cleo
  3. Ada
  4. Dev
By name:
  1. Ada
  2. Ben
  3. Cleo
  4. Dev
Experts: Ben, Cleo
Total score: 2160
First over 500: Ben
```

- `printRanking` passes its `rank` block straight on to `sorted(by:)`.
- `ordered.indices` gives the positions `0`, `1`, `2`, `3`, so the loop can
  print a number before each name.
- `players.map { p in p.score }` turns the players into a list of scores, so
  `sum()` can add them up.

## Exercises

**1. Words.** Start with `let words = ["apple", "fig", "banana", "kiwi", "cherry"]`.
Use `map` to make every word uppercase (`w.uppercase()`), and `filter` to
keep only the words with at most 4 letters. Print both lists joined by
spaces.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let words = ["apple", "fig", "banana", "kiwi", "cherry"]
    let shout = words.map { w in w.uppercase() }
    print(shout.joined(separator: " "))
    let short = words.filter { w in w.count <= 4 }
    print(short.joined(separator: " "))
}
```

```text
APPLE FIG BANANA KIWI CHERRY
fig kiwi
```

</details>

**2. Hot days.** Given a week of temperatures,
`[18, 25, 31, 22, 29, 35, 19]`, print how many days were 28 or more, which
day was the first of those (counting from day 1), and the three highest
temperatures, highest first.

Hint: `firstIndex(where:)` gives the index of the first match, and
`prefix(3)` keeps the first three items of a list.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let temperatures = [18, 25, 31, 22, 29, 35, 19]
    let hotDays = temperatures.filter { t in t >= 28 }
    print("Hot days: {hotDays.count}")
    let firstHot = temperatures.firstIndex(where: { t in t >= 28 }) ?? -1
    print("First hot day: day {firstHot + 1}")
    let warmest = temperatures.sorted(by: { a, b in a > b }).prefix(3)
    print(warmest.map { t in "{t}" }.joined(separator: ", "))
}
```

```text
Hot days: 3
First hot day: day 3
35, 31, 29
```

</details>

**3. Twice.** Write a function `applyTwice(value: Int, f: fn(Int) -> Int) -> Int`
that runs `f` on the value, then runs `f` again on the result. Call it with
a trailing block that adds 4, and one that squares.

<details>
<summary>Solution</summary>

```tessel
fn applyTwice(value: Int, f: fn(Int) -> Int) -> Int {
    f(f(value))
}

fn main() {
    print(applyTwice(value: 3) { n in n + 4 })
    print(applyTwice(value: 3) { n in n * n })
}
```

```text
11
81
```

3 + 4 + 4 is 11, and (3 × 3) × (3 × 3) is 81.

</details>

**4. Multipliers.** Write `makeMultiplier(factor: Int) -> fn(Int) -> Int`,
which returns a function that multiplies by `factor`. Make `double` and
`triple`, and use `triple` with `map` on `[1, 2, 3]`.

<details>
<summary>Solution</summary>

```tessel
fn makeMultiplier(factor: Int) -> fn(Int) -> Int {
    { n in n * factor }
}

fn main() {
    let double = makeMultiplier(factor: 2)
    let triple = makeMultiplier(factor: 3)
    print(double(10))
    print(triple(10))
    print([1, 2, 3].map(triple).map { n in "{n}" }.joined(separator: " "))
}
```

```text
20
30
3 6 9
```

`map(triple)` passes the function value itself, with no block around it.

</details>

**5. Ticket machines.** At a market, each counter hands out numbered
tickets: the bakery gives "B-1", "B-2", …, the deli "D-1", "D-2", …. Write
`makeTicketMachine(prefix: String) -> fn() -> String` so that each machine
keeps its own count.

<details>
<summary>Solution</summary>

```tessel
fn makeTicketMachine(prefix: String) -> fn() -> String {
    var number = 0
    {
        number += 1
        "{prefix}-{number}"
    }
}

fn main() {
    let bakery = makeTicketMachine(prefix: "B")
    let deli = makeTicketMachine(prefix: "D")
    print(bakery())
    print(bakery())
    print(deli())
    print(bakery())
}
```

```text
B-1
B-2
D-1
B-3
```

Each call to `makeTicketMachine` makes a new `number` that only its own block
can see.

</details>

## Summary

- A **block** (also called a closure) is code in braces that you can pass
  around: `{ n in n * 2 }`. Parameters come before `in`, and the last line is
  the result.
- `map`, `filter`, `sorted(by:)` and `first(where:)` take blocks, so you
  describe *what* you want and the list does the looping.
- A **function type** like `fn(Int) -> Bool` describes what a function takes
  and gives back. Give a variable that type when you store a block in it.
- Named functions are values too: `filter(isEven)`. Function values are
  called without labels.
- A function can return a new function. Blocks **capture** the variables
  they use, and keep them alive, which is how `makeCounter` works.
- When a block is the last argument, write it after the parentheses as a
  **trailing block**, except in the first line of an `if`, `for` or `match`.

For more, see [Closures](/language/closures/) and
[Functions](/language/functions/#functions-that-take-functions).

**Next:** [14. Interfaces](/learn/14-interfaces/)
