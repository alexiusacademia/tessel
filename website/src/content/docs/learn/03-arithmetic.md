---
title: "3. Arithmetic"
description: Make Tessel calculate for you with + - * / %, whole and decimal division, conversions, math functions and neat number formatting.
---

Computers were invented to calculate, and they're very good at it. In this
lesson you'll make Tessel do sums for you, from simple additions to square
roots, and you'll see what happens when a calculation goes wrong.

In this lesson you'll learn:

- the five arithmetic operators: `+`, `-`, `*`, `/` and `%`
- in which order a calculation is done, and how parentheses change it
- why `7 / 2` is `3`, and how to get `3.5`
- how to convert between `Int` and `Float`
- shortcuts like `+=` for changing a variable
- math functions such as `sqrt`, `pow`, `abs`, `min`, `max` and `round`
- how to show a number with a fixed number of decimals
- which calculations stop your program with an error

## The five operators

An **operator** is a symbol that does something with values. These five do
arithmetic:

| Operator | Meaning | Example | Result |
|---|---|---|---|
| `+` | add | `7 + 2` | `9` |
| `-` | subtract | `7 - 2` | `5` |
| `*` | multiply | `7 * 2` | `14` |
| `/` | divide | `7 / 2` | `3` |
| `%` | remainder | `7 % 2` | `1` |

```tessel
fn main() {
    print(7 + 2)
    print(7 - 2)
    print(7 * 2)
    print(7 / 2)
    print(7 % 2)
}
```

```text
9
5
14
3
1
```

Computers use `*` for "times" because `×` isn't on most keyboards, and `/`
for "divided by".

Two results may surprise you. `7 / 2` is `3`, not `3.5`; you'll see why in a
moment. And `%` gives the **remainder** of a division: 7 divided by 2 is 3,
with 1 left over, so `7 % 2` is `1`.

Of course, you can calculate with named values too, and store the result:

```tessel
fn main() {
    let width = 8
    let height = 5
    let area = width * height
    print("The room is {area} square meters.")
}
```

```text
The room is 40 square meters.
```

### What is the remainder good for?

More than you might think. Division and remainder together split a number
into parts. For example, to turn minutes into hours and minutes:

```tessel
fn main() {
    let totalMinutes = 135
    let hours = totalMinutes / 60
    let minutes = totalMinutes % 60
    print("{totalMinutes} minutes is {hours} h {minutes} min")
    print(10 % 2)
    print(11 % 2)
}
```

```text
135 minutes is 2 h 15 min
0
1
```

The last two lines show another common use: a number is even when its
remainder after dividing by 2 is `0`, and odd when it's `1`.

## Order of operations

What is `2 + 3 * 4`? Tessel follows the same rule you learned in school:
multiplication and division come before addition and subtraction. So it's
`2 + 12`, which is `14`. To do the addition first, use parentheses:

```tessel
fn main() {
    print(2 + 3 * 4)
    print((2 + 3) * 4)
    print(10 - 4 - 3)
    print(20 / 2 * 5)
}
```

```text
14
20
3
50
```

The rules:

1. What's in parentheses `( )` is worked out first.
2. Then `*`, `/` and `%`.
3. Then `+` and `-`.
4. Operators at the same level go from left to right: `10 - 4 - 3` is
   `6 - 3`, which is `3`.

### Common mistake: forgetting parentheses

To find the average of two numbers, you add them and divide by 2. This looks
right but isn't:

```tessel
fn main() {
    let a = 8
    let b = 6
    print(a + b / 2)
    print((a + b) / 2)
}
```

```text
11
7
```

The first line calculates `8 + 3`, because `/` goes first. Tessel can't warn
you about this: both are valid calculations, just different ones. When you're
not sure about the order, add parentheses. They never hurt, and they make your
intention clear to the reader.

## Whole numbers and decimal numbers

Here's the mystery of `7 / 2` solved. When you divide an `Int` by an `Int`,
the answer is an `Int` too, so the fraction is dropped. When you divide a
`Float` by a `Float`, you get a `Float`, with the fraction:

```tessel
fn main() {
    let slices = 7
    let people = 2
    print(slices / people)
    print(7.0 / 2.0)
}
```

```text
3
3.5
```

Both are useful. If 7 pizza slices are shared by 2 people who only take whole
slices, each gets 3 (and 1 is left: `7 % 2`). If you're sharing 7 liters of
juice, each person gets 3.5.

Note that the fraction is simply cut off, not rounded: `19 / 10` is `1`, not
`2`.

### Common mistake: mixing `Int` and `Float`

Tessel never mixes an `Int` and a `Float` in a calculation:

```tessel
fn main() {
    let price = 2.5
    let count = 4
    let total = price * count
    print(total)
}
```

```text
error: can't use `*` on `Float` and `Int`
 --> math.tsl:4:17
  |
4 |     let total = price * count
  |                 ^^^^^^^^^^^^^
  |
  = help: convert one side with `Float(…)` or `Int(…)`

1 error found
```

This may feel strict. The reason is that mixing them silently is a classic
source of bugs: should the answer be whole or not? Should the fraction of
`price` be dropped? Tessel makes you decide.

## Converting between `Int` and `Float`

To decide, **convert** one of the values. `Float(x)` turns a number into a
`Float`, and `Int(x)` turns it into an `Int`:

```tessel
fn main() {
    let price = 2.5
    let count = 4
    let total = price * Float(count)
    print(total)
    print(Int(total))
}
```

```text
10.0
10
```

This fixes the pizza problem too:

```tessel
fn main() {
    let slices = 7
    let people = 2
    let each = Float(slices) / Float(people)
    print("Everyone gets {each} slices.")
    print(Int(3.99))
    print(Int(-3.99))
}
```

```text
Everyone gets 3.5 slices.
3
-3
```

`Int(…)` drops the fraction, just like Int division: `Int(3.99)` is `3`, not
`4`. To round to the nearest whole number instead, see `round` below.

A number you type directly into the code (a **literal**) is more flexible
than a variable. Where a `Float` is needed, `4` counts as `4.0`, so
`price * 4` works without any conversion. It's only variables (and other
calculated values) that never change type by themselves.

## Changing a variable: `+=` and friends

Changing a variable based on its own value is so common that there's a short
way to write it:

| Short | Means |
|---|---|
| `x += 5` | `x = x + 5` |
| `x -= 5` | `x = x - 5` |
| `x *= 5` | `x = x * 5` |
| `x /= 5` | `x = x / 5` |

```tessel
fn main() {
    var coins = 10
    coins += 5
    print(coins)
    coins -= 3
    print(coins)
    coins *= 2
    print(coins)
    coins /= 4
    print(coins)
}
```

```text
15
12
24
6
```

There's no `%=` in Tessel. Write `x = x % 5` instead.

## Math functions

For more than the basic operators, Tessel has built-in **functions**. A
function is a named piece of work: you give it values between parentheses,
and it gives you back a result. You've already used one: `print`.

| Function | Gives back | Example | Result |
|---|---|---|---|
| `sqrt(x)` | the square root of `x` | `sqrt(16.0)` | `4.0` |
| `pow(x, y)` | `x` to the power `y` | `pow(2.0, 10.0)` | `1024.0` |
| `abs(x)` | `x` without its minus sign | `abs(-7)` | `7` |
| `min(a, b)` | the smaller of the two | `min(3, 8)` | `3` |
| `max(a, b)` | the larger of the two | `max(3, 8)` | `8` |
| `round(x)` | `x` rounded to the nearest whole number | `round(2.5)` | `3.0` |

```tessel
fn main() {
    let a = 3.0
    let b = 4.0
    let c = sqrt(a * a + b * b)
    print("The long side is {c}.")
    let radius = 2.0
    let area = pi * pow(radius, 2.0)
    print("The circle's area is {area}.")
}
```

```text
The long side is 5.0.
The circle's area is 12.566370614359172.
```

`pi` is a built-in constant: 3.141592653589793.

A few rules to remember:

- `sqrt`, `pow` and `round` work with `Float`s and give back a `Float`.
  `round(2.6)` is `3.0`. To get an `Int`, convert it: `Int(round(2.6))` is `3`.
- `abs`, `min` and `max` work with either type, and give back the same type.
  Both values of `min` and `max` must have the same type.
- To use an `Int` variable with `sqrt` or `pow`, convert it first:

```tessel
fn main() {
    let n = 16
    print(sqrt(n))
}
```

```text
error: expected `Float`, found `Int`
 --> math.tsl:3:16
  |
3 |     print(sqrt(n))
  |                ^ this is `Int`
  |
  = help: convert it with `Float(…)`

1 error found
```

Write `sqrt(Float(n))` and it prints `4.0`.

There are more, such as `floor`, `ceil`, `sin` and `cos`. See
[Math](/reference/stdlib/#math) in the reference for the full list.

## Showing a neat number

A `Float` is printed with all its digits, which is often more than you want:

```tessel
fn main() {
    let third = 1.0 / 3.0
    print(third)
    print(third.formatted(decimals: 2))
    print(third.formatted(decimals: 5))
    let price = 4.5
    print("Price: {price.formatted(decimals: 2)} coins")
    let votes = 7
    print(Float(votes).formatted(decimals: 1))
}
```

```text
0.3333333333333333
0.33
0.33333
Price: 4.50 coins
7.0
```

`third.formatted(decimals: 2)` turns the number into text with exactly 2
digits after the point, rounded. The dot after `third` means "use something
that belongs to this value": `formatted` is a function that every `Float`
has. `decimals:` is a **label** that says what the `2` is for. You'll see
labels like this often.

`formatted` belongs to `Float` only. For an `Int`, convert first, as in the
last line. See [formatted](/reference/stdlib/#formatted) for details.

Why so many digits in the first place? Computers store most decimal numbers
very slightly inaccurately, because they work in binary. For example,
`print(0.1 + 0.2)` shows `0.30000000000000004`. It's not a Tessel bug; nearly
every programming language does this. `formatted` hides that noise when you
show a number to a person.

## When arithmetic goes wrong

Some calculations have no sensible answer. When a program meets one while it
is running, it stops with a **runtime error**: an error that happens while
the program runs, not when it's compiled. Tessel can't catch these in
advance, because they depend on the values at that moment.

### Dividing by zero

What is 12 cookies shared among 0 friends? There's no answer, so a program
that tries stops straight away:

```tessel
fn main() {
    let cookies = 12
    let friends = 0
    print("Sharing {cookies} cookies...")
    print(cookies / friends)
    print("Done!")
}
```

```text
Sharing 12 cookies...
error: division by zero
 --> math.tsl:5:11
```

The first line was printed, because it ran before the problem. Then the
program stopped at line 5, column 11, and `"Done!"` was never printed. The same
happens with `%` and zero.

With `Float`s, the rules are different: `12.0 / 0.0` gives `inf` (infinity)
and the program carries on.

### Numbers that are too big

An `Int` can hold numbers up to 9,223,372,036,854,775,807 (about nine
quintillion). That's big, but not infinite. A result that doesn't fit is
called an **overflow**:

```tessel
fn main() {
    var big = 9_000_000_000_000_000_000
    print(big)
    big = big * 2
    print(big)
}
```

```text
9000000000000000000
error: integer overflow
 --> math.tsl:4:11
```

Some languages quietly give you a wrong answer in this situation (often a
large negative number). Tessel stops instead, because a wrong answer that
looks right is much more dangerous than a program that clearly stops.

In a later lesson, you'll learn how to check a value before using it, so you
can avoid these errors: for example, only divide when the number of friends
isn't zero.

## Exercises

**1. Seconds.** Using constants for the seconds in a minute, the minutes in
an hour and the hours in a day, print how many seconds there are in a day and
in a week.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let secondsPerMinute = 60
    let minutesPerHour = 60
    let hoursPerDay = 24
    let secondsPerDay = secondsPerMinute * minutesPerHour * hoursPerDay
    print("A day has {secondsPerDay} seconds.")
    print("A week has {secondsPerDay * 7} seconds.")
}
```

```text
A day has 86400 seconds.
A week has 604800 seconds.
```

</details>

**2. Temperature.** To turn degrees Celsius into Fahrenheit, multiply by 9,
divide by 5 and add 32. Convert 21.5 °C.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let celsius = 21.5
    let fahrenheit = celsius * 9 / 5 + 32
    print("{celsius} °C is {fahrenheit} °F")
}
```

```text
21.5 °C is 70.7 °F
```

`celsius` is a `Float`, and the literals `9`, `5` and `32` count as `Float`s
next to it. If `celsius` were the `Int` `21`, the division would drop the
fraction, and 21 °C would come out as 69 °F instead of 69.8 °F.

</details>

**3. Clock time.** Turn 4000 seconds into hours, minutes and seconds, and
print `4000 seconds is 1 h 6 min 40 s`. Use `/` and `%`.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let totalSeconds = 4000
    let hours = totalSeconds / 3600
    let minutes = totalSeconds % 3600 / 60
    let seconds = totalSeconds % 60
    print("{totalSeconds} seconds is {hours} h {minutes} min {seconds} s")
}
```

```text
4000 seconds is 1 h 6 min 40 s
```

An hour has 3600 seconds. `totalSeconds % 3600` is what's left after the
whole hours (400 seconds), and dividing that by 60 gives the whole minutes.
`%` and `/` are at the same level, so they go left to right.

</details>

**4. Test average.** You scored 82, 91 and 77 on three tests (store them as
`Int`s). Print the total, the average with one decimal, and the average
rounded to a whole number.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let first = 82
    let second = 91
    let third = 77
    let total = first + second + third
    let average = Float(total) / 3
    print("Total: {total}")
    print("Average: {average.formatted(decimals: 1)}")
    print("Rounded: {Int(round(average))}")
}
```

```text
Total: 250
Average: 83.3
Rounded: 83
```

Without `Float(…)`, `total / 3` would be the `Int` `83`, and the `.3` would
be lost.

</details>

**5. Splitting the bill.** Four friends have dinner. The bill is 87.40, and
they want to add a 15% tip (store the percentage as the `Int` `15`). Print the
tip, the total and how much each person pays, all with two decimals.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let bill = 87.40
    let tipPercent = 15
    let people = 4
    let tip = bill * Float(tipPercent) / 100
    let total = bill + tip
    let each = total / Float(people)
    print("Tip: {tip.formatted(decimals: 2)}")
    print("Total: {total.formatted(decimals: 2)}")
    print("Each of {people} people pays {each.formatted(decimals: 2)}")
}
```

```text
Tip: 13.11
Total: 100.51
Each of 4 people pays 25.13
```

</details>

## Summary

- `+`, `-`, `*`, `/` and `%` add, subtract, multiply, divide and give the
  remainder.
- `*`, `/` and `%` go before `+` and `-`. Use parentheses to change the order,
  or just to make it clear.
- `Int / Int` gives an `Int` and drops the fraction. For a decimal answer,
  divide `Float`s.
- Tessel never mixes `Int` and `Float`. Convert with `Float(x)` or `Int(x)`
  (`Int` drops the fraction).
- `x += 5` is short for `x = x + 5`; there are also `-=`, `*=` and `/=`.
- `sqrt`, `pow`, `abs`, `min`, `max` and `round` are built-in math functions.
- `x.formatted(decimals: 2)` shows a `Float` with exactly two decimals.
- Dividing an `Int` by zero, or an `Int` result that's too big, stops the
  program with a runtime error.

See also: [Operators](/language/basics/#operators) and
[Converting between types](/language/types/#converting-between-types).

**Next:** [4. Making decisions](/learn/04-decisions/)
