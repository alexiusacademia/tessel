---
title: "6. Functions"
description: Give a piece of work a name, so you can run it again and again with different values, and build bigger programs out of small, clear parts.
---

So far, every program you've written has lived inside one `main` function,
running from top to bottom. That works for short programs. But as programs
grow, you'll notice the same few lines showing up again and again, and
`main` gets long and hard to follow.

A **function** fixes both problems. It's a piece of work with a name. You
write the work once, and then you *call* the function by its name whenever
you need it done.

In this lesson you'll learn:

- why functions are useful, and how to declare one
- how to give a function values to work with (parameters), and how labels work
- how a function hands back an answer (return values)
- how to give parameters default values
- where a variable can be used (scope)
- how functions can call other functions, and even themselves (recursion)

## Why functions?

Here's a program that welcomes two people:

```tessel
fn main() {
    print("==========")
    print("Welcome, Ada!")
    print("==========")
    print("==========")
    print("Welcome, Grace!")
    print("==========")
}
```

```text
==========
Welcome, Ada!
==========
==========
Welcome, Grace!
==========
```

The same three lines appear twice, with only the name changed. If you
wanted a wider border, you'd have to change it in four places, and it's easy
to miss one.

With a function, you write those lines once and give them a name:

```tessel
fn welcome(name: String) {
    print("==========")
    print("Welcome, {name}!")
    print("==========")
}

fn main() {
    welcome(name: "Ada")
    welcome(name: "Grace")
}
```

```text
==========
Welcome, Ada!
==========
==========
Welcome, Grace!
==========
```

The output is the same, but the program is shorter, and the border is
written in one place. `main` now reads almost like a to-do list: welcome
Ada, welcome Grace.

That's what functions give you:

- **A name for a piece of work.** `welcome(name: "Ada")` says *what*
  happens. You only need to read the function itself to see *how*.
- **Reuse.** Write the work once, use it as often as you like.
- **One place to fix things.** Change the function, and every call gets the
  change.

## Declaring a function

You've actually been writing a function since lesson 1: `main`. Other
functions look the same. Here's one with no parameters:

```tessel
fn sayHello() {
    print("Hello!")
}

fn main() {
    sayHello()
    sayHello()
}
```

```text
Hello!
Hello!
```

A function declaration has:

- the keyword `fn` (short for "function"),
- a name, like `sayHello`,
- parentheses `()`, which hold the parameters (here there are none, but the
  parentheses are still needed),
- a body in braces `{ }`, with the code that runs when you call it.

To **call** the function, write its name followed by parentheses:
`sayHello()`. Each call runs the whole body, then the program carries on
from where it was.

Functions go at the top level of the file, next to `main`, not inside it.
The order doesn't matter: `main` can call a function that's written below
it. The program always starts at `main`.

## Parameters and labels

A **parameter** is a value the function needs to do its work. You list the
parameters in the parentheses, each written `name: Type`, like a variable
declaration. When you call the function, you give a value for each one.
That value is called an **argument**.

```tessel
fn describePet(name: String, legs: Int) {
    print("{name} has {legs} legs.")
}

fn main() {
    describePet(name: "Rex", legs: 4)
    describePet(name: "Polly", legs: 2)
}
```

```text
Rex has 4 legs.
Polly has 2 legs.
```

Inside the function, `name` and `legs` work like constants holding the
values from the call.

In the call, each argument has a **label**: the parameter's name followed
by a colon, like `name:` and `legs:`. Labels make a call easy to read. Compare
`describePet(name: "Rex", legs: 4)` with a bare `describePet("Rex", 4)`: with
labels, you can tell what the `4` means without looking up the function.

The arguments go in the same order as the parameters.

There's one shortcut: when a function has **just one parameter**, you can
leave the label out, because there's nothing to mix it up with. So with
`fn welcome(name: String)`, both `welcome(name: "Ada")` and `welcome("Ada")`
work.

### Common mistake: forgetting a label

With two or more parameters, the labels are required:

```tessel
fn describePet(name: String, legs: Int) {
    print("{name} has {legs} legs.")
}

fn main() {
    describePet("Rex", 4)
}
```

```text
error: this argument needs its label `name:`
 --> main.tsl:6:17
  |
6 |     describePet("Rex", 4)
  |                 ^^^^^
  |
  = help: write `name: …`

error: this argument needs its label `legs:`
 --> main.tsl:6:24
  |
6 |     describePet("Rex", 4)
  |                        ^
  |
  = help: write `legs: …`

2 errors found
```

The message tells you exactly what to add. Mixing up the order is also
caught: `describePet(legs: 4, name: "Rex")` gives
``error: `name:` is in the wrong position``.

## Parameters without labels: `_`

Sometimes a label adds nothing. In `print("Hi")`, it's obvious what the
text is for. You can make a parameter unlabeled by writing `_` (an
underscore) and a space before its name:

```tessel
fn shout(_ text: String, times: Int) {
    for i in 0..times {
        print("{text}!")
    }
}

fn main() {
    shout("Hooray", times: 3)
}
```

```text
Hooray!
Hooray!
Hooray!
```

The caller writes just the value, `"Hooray"`, for the first argument.
Inside the function, the parameter is still called `text`.

Use `_` when the function's name already says what the argument is, like
`shout("Hooray")` or `print("Hi")`. When in doubt, keep the label.

## Return values

The functions so far *do* something (they print). Many functions instead
*work something out* and hand the answer back to the caller. That answer is
the function's **return value**.

To return a value, write `->` and the type of the answer after the
parentheses. The **last line of the body** is the answer:

```tessel
fn square(_ n: Int) -> Int {
    n * n
}

fn main() {
    let area = square(5)
    print(area)
    print(square(3) + square(4))
}
```

```text
25
25
```

`square(5)` is replaced by its answer, `25`, so you can store it in a
variable, print it, or use it in a calculation, like any other value.

Why return a value instead of printing it? Because the caller can then
decide what to do with it. A function that prints can only print. A
function that returns can be used anywhere.

### Leaving early with `return`

You can also hand back the answer with `return`. The function stops right
there, and the rest of the body is skipped. It's handy for dealing with
special cases first:

```tessel
fn describe(temperature: Int) -> String {
    if temperature < 0 {
        return "freezing"
    }
    if temperature < 20 {
        return "cool"
    }
    "warm"
}

fn main() {
    print(describe(temperature: -5))
    print(describe(temperature: 12))
    print(describe(temperature: 25))
}
```

```text
freezing
cool
warm
```

As you saw in lesson 4, an `if` can also *give* a value: `if a > b { a }
else { b }` is `a` when `a` is bigger, and `b` otherwise. So the last line
can be an `if` too. Then every branch needs a value, so it must have an `else`:

```tessel
fn biggest(a: Int, b: Int) -> Int {
    if a > b { a } else { b }
}

fn main() {
    print(biggest(a: 3, b: 8))
}
```

```text
8
```

A function without `->` returns nothing. It can still use a plain `return`
(with no value) to stop early.

### Common mistake: no value at the end

If a function promises a value with `->`, its last line must be that
value. Here the last line is a `let`, which stores the result but doesn't
hand it back:

```tessel
fn square(_ n: Int) -> Int {
    let result = n * n
}

fn main() {
    print(square(4))
}
```

```text
error: this block must end with `Int`
 --> main.tsl:3:1
  |
3 | }
  | ^ expected `Int` before this
  |
  = help: put the value on the last line, or use `return …`

1 error found
```

The fix is to put `result` on its own line at the end, or simply write
`n * n` as the last line.

## Default values

A parameter can have a **default value**, written with `=` after its type.
If the caller leaves that argument out, the default is used:

```tessel
fn greet(name: String, greeting: String = "Hello") {
    print("{greeting}, {name}!")
}

fn main() {
    greet(name: "Ada")
    greet(name: "Grace", greeting: "Good morning")
}
```

```text
Hello, Ada!
Good morning, Grace!
```

Defaults are good for settings that are usually the same. The common case
stays short, and the unusual case is still possible.

## Scope: where a variable lives

A variable declared inside a function belongs to that function. It's
created when the function runs, and it's gone when the function ends. The
part of the program where a name can be used is called its **scope**.

```tessel
fn total(a: Int, b: Int, c: Int) -> Int {
    var sum = 0
    sum += a
    sum += b
    sum += c
    sum
}

fn main() {
    var sum = 100
    print(total(a: 1, b: 2, c: 3))
    print(sum)
}
```

```text
6
100
```

There are two different variables called `sum` here, one in each function.
They don't affect each other. That's a good thing: when you write a
function, you don't have to worry that its variables clash with names used
somewhere else in the program.

The same goes for blocks: a variable declared inside the braces of an `if`
or a `for` only exists inside those braces.

### Common mistake: using another function's variable

Because each function's variables are its own, one function can't reach
into another:

```tessel
fn setUp() {
    let total = 10
}

fn main() {
    setUp()
    print(total)
}
```

```text
error: cannot find `total`
 --> main.tsl:7:11
  |
7 |     print(total)
  |           ^^^^^ not found

1 error found
```

To get a value out of a function, *return* it, and store the result:
`let total = setUp()`, with `fn setUp() -> Int`.

### Common mistake: changing a parameter

Parameters are constants. Trying to change one is an error:

```tessel
fn countDown(from: Int) {
    while from > 0 {
        print(from)
        from -= 1
    }
}

fn main() {
    countDown(from: 3)
}
```

```text
error: can't change `from`, which is a parameter
 --> main.tsl:4:9
  |
4 |         from -= 1
  |         ^^^^
 ::: main.tsl:1:14
  |
1 | fn countDown(from: Int) {
  |              ---- declared here
  |
  = help: parameters are copies and can't be changed; copy it into a `var`, or (in a view) declare it as `bind`

1 error found
```

The function gets its own *copy* of each argument, so there's nothing
useful to change. Do what the help says: copy it into a `var` first.

```tessel
fn countDown(from: Int) {
    var n = from
    while n > 0 {
        print(n)
        n -= 1
    }
}

fn main() {
    countDown(from: 3)
}
```

```text
3
2
1
```

## Functions calling functions

A function can call other functions. This lets you build bigger ideas out
of smaller ones, each easy to understand on its own:

```tessel
fn isLeapYear(_ year: Int) -> Bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn daysInYear(_ year: Int) -> Int {
    if isLeapYear(year) { 366 } else { 365 }
}

fn main() {
    for year in 2023..2027 {
        print("{year} has {daysInYear(year)} days")
    }
}
```

```text
2023 has 365 days
2024 has 366 days
2025 has 365 days
2026 has 365 days
```

(You may recognize the leap-year rule from lesson 4.) `main` calls
`daysInYear`, which calls `isLeapYear`. When `isLeapYear`
finishes, its answer goes back to `daysInYear`, which finishes and hands its
answer back to `main`.

## Recursion: a function that calls itself

A function can even call *itself*. This is called **recursion**. It sounds
odd, but it's a natural fit for problems where the big version is made of a
smaller version of the same problem.

Take a countdown. Counting down from 3 means: say "3", then count down from
2. Counting down from 2 means: say "2", then count down from 1. And so on,
until you reach 0, where you stop.

```tessel
fn countdown(_ n: Int) {
    if n == 0 {
        print("Liftoff!")
        return
    }
    print(n)
    countdown(n - 1)
}

fn main() {
    countdown(3)
}
```

```text
3
2
1
Liftoff!
```

Every recursive function needs two parts:

- a **base case**, where it stops without calling itself (here, `n == 0`),
- a **recursive step**, where it calls itself with a *smaller* problem
  (here, `n - 1`), so it gets closer to the base case each time.

Here's a classic: the factorial of a number, written `5!` in maths, is
`5 × 4 × 3 × 2 × 1`. Notice that `5!` is `5 × 4!`, and `4!` is `4 × 3!`,
and so on down to `1! = 1`:

```tessel
fn factorial(_ n: Int) -> Int {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

fn main() {
    print(factorial(5))
    print(factorial(10))
}
```

```text
120
3628800
```

Factorials grow very fast. `factorial(20)` is 2432902008176640000, the
largest that fits in an `Int`. Ask for `factorial(21)` and the program
stops with `error: integer overflow`.

### Common mistake: forgetting the base case

Here's `factorial` without its base case:

```tessel
fn factorial(_ n: Int) -> Int {
    n * factorial(n - 1)
}

fn main() {
    print(factorial(5))
}
```

```text
error: stack overflow: too many function calls inside each other
 = help: a function that calls itself needs a case where it stops calling itself, like `if n == 0 { return 1 }`
```

`factorial(5)` calls `factorial(4)`, which calls `factorial(3)`, then 2, 1,
0, -1, -2… Nothing ever says "stop", and every call is still waiting for the
one inside it to finish. The computer keeps a list of the calls that are
waiting, called the **stack**, and it has room for only so many. When it runs
out, the program stops with this runtime error.

Sometimes the program doesn't stop at all. If calling itself is the very last
thing the function does, as in `countdown`, a forgotten base case can make it
run forever, like the infinite loops in
[lesson 5](/learn/05-loops/#warning-loops-that-never-end). Stop it with
**Ctrl+C** (or **Stop** in the IDE). Either way, the fix is the same. Always
check: when does it stop?

Anything you can do with recursion, you can also do with a loop. Use
whichever makes the code clearer. For a countdown a `while` loop is just as
good, but for some problems (like exploring folders inside folders),
recursion is much simpler.

## Worked example: tidying up a receipt

Here's a program that prints a shop receipt. It works, but notice how much
is repeated:

```tessel
fn main() {
    let applePrice = 2
    let appleCount = 5
    print("apples x{appleCount}: ${applePrice * appleCount}")

    let breadPrice = 4
    let breadCount = 2
    print("bread x{breadCount}: ${breadPrice * breadCount}")

    let cheesePrice = 9
    let cheeseCount = 1
    print("cheese x{cheeseCount}: ${cheesePrice * cheeseCount}")

    let total = applePrice * appleCount + breadPrice * breadCount + cheesePrice * cheeseCount
    if total >= 20 {
        print("total: ${total - total / 10} (10% off)")
    } else {
        print("total: ${total}")
    }
}
```

```text
apples x5: $10
bread x2: $8
cheese x1: $9
total: $25 (10% off)
```

Each line does the same job: multiply a price by a count, and print it. The
price-times-count sum is written twice for every item. Let's pull the
pieces out into functions, one job each:

1. Working out the cost of one line: `lineCost`.
2. Printing one line, and handing back its cost so it can be added to the
   total: `printLine`.
3. Working out the discount: `discount`.

```tessel
fn lineCost(price: Int, count: Int) -> Int {
    price * count
}

fn printLine(item: String, price: Int, count: Int) -> Int {
    let cost = lineCost(price: price, count: count)
    print("{item} x{count}: ${cost}")
    cost
}

fn discount(total: Int) -> Int {
    if total >= 20 { total / 10 } else { 0 }
}

fn main() {
    var total = 0
    total += printLine(item: "apples", price: 2, count: 5)
    total += printLine(item: "bread", price: 4, count: 2)
    total += printLine(item: "cheese", price: 9, count: 1)

    let off = discount(total: total)
    if off > 0 {
        print("total: ${total - off} (10% off)")
    } else {
        print("total: ${total}")
    }
}
```

```text
apples x5: $10
bread x2: $8
cheese x1: $9
total: $25 (10% off)
```

Same output, but now:

- Adding an item is one line.
- The receipt's format is written once, in `printLine`. To change how a
  line looks, you change it in one place.
- The discount rule has a name and lives on its own. When the shop changes
  its offer, you know exactly where to look.

Programmers call this kind of change **refactoring**: making code clearer
without changing what it does. When you see the same lines two or three
times, that's usually a sign they want to become a function.

## Exercises

**1. Cube.** Write a function `cube` that takes an `Int` without a label and
returns it multiplied by itself three times. `print(cube(3))` should
print `27`.

<details>
<summary>Solution</summary>

```tessel
fn cube(_ n: Int) -> Int {
    n * n * n
}

fn main() {
    print(cube(3))
    print(cube(10))
}
```

```text
27
1000
```

</details>

**2. Between.** Write `fn isBetween(value: Int, low: Int, high: Int) -> Bool`
that returns `true` when `value` is at least `low` and at most `high`.
Test it with a few values.

<details>
<summary>Solution</summary>

```tessel
fn isBetween(value: Int, low: Int, high: Int) -> Bool {
    value >= low && value <= high
}

fn main() {
    print(isBetween(value: 5, low: 1, high: 10))
    print(isBetween(value: 10, low: 1, high: 10))
    print(isBetween(value: 11, low: 1, high: 10))
}
```

```text
true
true
false
```

</details>

**3. A line of stars.** Write a function `line` that returns a string of
stars. It takes a `length` (default `10`) and a `symbol` (default `"*"`).
`line()` gives `**********`, `line(length: 3)` gives `***`, and
`line(length: 5, symbol: "-")` gives `-----`.

<details>
<summary>Solution</summary>

```tessel
fn line(length: Int = 10, symbol: String = "*") -> String {
    var text = ""
    for i in 0..length {
        text += symbol
    }
    text
}

fn main() {
    print(line())
    print(line(length: 3))
    print(line(length: 5, symbol: "-"))
}
```

```text
**********
***
-----
```

The loop variable `i` isn't used, which is fine: the loop just needs to
run `length` times.

</details>

**4. FizzBuzz as a function.** You wrote FizzBuzz in lesson 5. This time,
put the decision in a function. Write `fn fizzBuzz(_ n: Int) -> String` that
returns `"FizzBuzz"` if `n` divides by both 3 and 5, `"Fizz"` if it
divides by 3, `"Buzz"` if it divides by 5, and the number itself as text
otherwise. Then use it in a loop to print the results for 1 to 15.

<details>
<summary>Solution</summary>

```tessel
fn fizzBuzz(_ n: Int) -> String {
    if n % 15 == 0 {
        return "FizzBuzz"
    }
    if n % 3 == 0 {
        return "Fizz"
    }
    if n % 5 == 0 {
        return "Buzz"
    }
    "{n}"
}

fn main() {
    for n in 1..16 {
        print(fizzBuzz(n))
    }
}
```

```text
1
2
Fizz
4
Buzz
Fizz
7
8
Fizz
Buzz
11
Fizz
13
14
FizzBuzz
```

`"{n}"` turns the number into text using interpolation. Notice that the
`FizzBuzz` check comes first: if it came after the `Fizz` check, 15 would
return `"Fizz"` and never get there.

</details>

**5. Power, recursively.** Write `fn power(base: Int, exponent: Int) -> Int`
using recursion, not a loop. Hint: any number to the power 0 is 1, and
`base` to the power `exponent` is `base` times `base` to the power
`exponent - 1`. `power(base: 2, exponent: 10)` should be `1024`.

<details>
<summary>Solution</summary>

```tessel
fn power(base: Int, exponent: Int) -> Int {
    if exponent == 0 {
        1
    } else {
        base * power(base: base, exponent: exponent - 1)
    }
}

fn main() {
    print(power(base: 2, exponent: 10))
    print(power(base: 3, exponent: 4))
    print(power(base: 7, exponent: 0))
}
```

```text
1024
81
1
```

The base case is `exponent == 0`. Each recursive call makes `exponent`
one smaller, so it always gets there (as long as you don't pass a
negative exponent).

</details>

## Summary

- A **function** is a named piece of work. Declare it with `fn`, a name,
  parameters in `()` and a body in `{}`. Call it by name: `sayHello()`.
- **Parameters** are the values a function needs, written `name: Type`.
  Callers pass arguments with **labels**: `describePet(name: "Rex", legs: 4)`.
- A function with one parameter can be called without the label. Write
  `_` before a parameter to make it unlabeled always.
- Write `-> Type` to **return** a value. The last line of the body is the
  answer; `return` hands it back early.
- A parameter can have a **default value**: `greeting: String = "Hello"`.
- Variables inside a function belong to it (their **scope**). Parameters are
  constants; copy one into a `var` to change it.
- Functions can call other functions, and themselves (**recursion**). A
  recursive function needs a base case where it stops.
- When you see the same lines repeated, turn them into a function.

For all the details, see [Functions](/language/functions/) in the language
guide.

**Next:** [7. Lists](/learn/07-lists/)
