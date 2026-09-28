---
title: "5. Loops"
description: Repeat work with for and while loops, add things up, stop early with break and continue, and put loops inside loops.
---

Computers never get bored. They'll happily do the same thing a million times,
and that's where much of their power comes from. In this lesson you'll learn
to write **loops**: code that runs again and again.

In this lesson you'll learn:

- how to repeat code with `for` and a range like `0..5`
- how to add things up and count things in a loop
- how to repeat "as long as something is true" with `while`
- how to leave a loop early with `break`, or skip a round with `continue`
- how to put a loop inside another loop
- common loop patterns: finding the first match, and finding the largest
- how to recognize and stop a loop that never ends

## Repeating with `for`

Say you want to print five lines. You could write `print` five times. But what
about a hundred lines? Or a number of lines that depends on what the user
types? A `for` loop does the repeating for you:

```tessel
fn main() {
    for i in 0..5 {
        print("Hello number {i}")
    }
    print("Done!")
}
```

```text
Hello number 0
Hello number 1
Hello number 2
Hello number 3
Hello number 4
Done!
```

Here's what happens:

- `0..5` is a **range**: the whole numbers from 0 up to 5, *but not including
  5*. So: 0, 1, 2, 3, 4.
- The block between `{` and `}` is the **body** of the loop. It runs once for
  each number in the range. Each run is called a round, or an **iteration**.
- In each round, `i` holds the current number. `i` is the **loop variable**;
  you can give it any name you like.
- When the range is used up, the program carries on after the loop, and
  prints "Done!".

### Why doesn't `0..5` include 5?

It looks odd at first, but it's handy: `0..5` runs exactly 5 times, and
`0..n` runs exactly `n` times. Programmers very often count from 0, as you'll
see when you meet lists.

To count from 1 instead, start at 1, and end one past the last number you
want:

```tessel
fn main() {
    for day in 1..8 {
        print("Day {day}")
    }
}
```

```text
Day 1
Day 2
Day 3
Day 4
Day 5
Day 6
Day 7
```

The ends of a range can be calculations, too. To go from 1 up to and
including `n`, write `1..(n + 1)`:

```tessel
fn main() {
    let n = 4
    for i in 1..(n + 1) {
        print("{i} squared is {i * i}")
    }
}
```

```text
1 squared is 1
2 squared is 4
3 squared is 9
4 squared is 16
```

Some languages write "up to and including" as `1..=4` or `1...4`. Tessel
doesn't: if you try, it says ``ranges in Tessel are written `a..b`, and leave
out `b` `` and suggests `1..(n + 1)`.

If the end isn't larger than the start, as in `5..0`, the range is empty and
the body doesn't run at all. A `for` loop can't count down by itself. To count
down, calculate the number from the loop variable: see exercise 1.

When you don't need the number at all, name the loop variable `_`:

```tessel
fn main() {
    for _ in 0..3 {
        print("Hip hip hooray!")
    }
}
```

```text
Hip hip hooray!
Hip hip hooray!
Hip hip hooray!
```

See [for loops](/language/control-flow/#for-loops) and
[Range](/language/types/#range) in the language guide for more.

## Adding things up

One of the most common things to do with a loop is to **accumulate** a
result: keep a running total in a variable, and add to it in each round.
Here's the sum of all numbers from 1 to 100:

```tessel
fn main() {
    var total = 0
    for i in 1..101 {
        total += i
    }
    print("1 + 2 + ... + 100 = {total}")
}
```

```text
1 + 2 + ... + 100 = 5050
```

The pattern has three steps:

1. **Before** the loop, create a variable with a starting value (`0` for a
   sum).
2. **Inside** the loop, update it (`total += i`).
3. **After** the loop, use the result.

**Counting** is the same pattern: add 1 each time something is true. How many
numbers from 1 to 100 are divisible by 7?

```tessel
fn main() {
    var count = 0
    for n in 1..101 {
        if n % 7 == 0 {
            count += 1
        }
    }
    print("{count} numbers from 1 to 100 are divisible by 7.")
}
```

```text
14 numbers from 1 to 100 are divisible by 7.
```

The `if` from [lesson 4](/learn/04-decisions/) sits inside the loop body.
You can put anything in a loop body, including decisions and other loops.

### Common mistake: creating the total inside the loop

What's wrong here?

```tessel
fn main() {
    for i in 1..4 {
        var total = 0
        total += i
        print("total is {total}")
    }
}
```

```text
total is 1
total is 2
total is 3
```

The total should grow to 1, 3, 6, but it doesn't. Because `var total = 0` is
*inside* the body, it runs in every round, so `total` starts again from 0 each
time. The running total must be created **before** the loop.

### Common mistake: changing the loop variable

The loop variable belongs to the loop. You can read it, but you can't change
it:

```tessel
fn main() {
    for i in 0..5 {
        i = i + 1
        print(i)
    }
}
```

```text
error: can't change `i`, which is a loop variable
 --> loops.tsl:3:9
  |
3 |         i = i + 1
  |         ^
 ::: loops.tsl:2:9
  |
2 |     for i in 0..5 {
  |         - declared here
  |
  = help: the loop sets it for each round; to change the number, copy it into a `var` first: `var x = i`
```

Each round, the loop hands you a fresh copy of the next number, so changing
it wouldn't affect the loop anyway. If you want to print `i + 1`, just print
it: `print(i + 1)`. Or change the range.

## Repeating while something is true: `while`

A `for` loop is great when you know how many rounds you need. Sometimes you
don't. A `while` loop keeps going **as long as a condition is true**:

```tessel
fn main() {
    var n = 3
    while n > 0 {
        print(n)
        n -= 1
    }
    print("Liftoff!")
}
```

```text
3
2
1
Liftoff!
```

Before each round, Tessel checks the condition. If it's `true`, the body runs,
and then it checks again. When it's `false`, the loop ends. If the condition
is `false` from the start (say, `n` starts at `0`), the body never runs.

Here's a question that really needs `while`: you put 1000 coins in a savings
account that pays 5% interest a year. How many years until you have 2000?

```tessel
fn main() {
    var savings = 1000.0
    var years = 0
    while savings < 2000.0 {
        savings *= 1.05
        years += 1
    }
    print("After {years} years you have {savings.formatted(decimals: 2)} coins.")
}
```

```text
After 15 years you have 2078.93 coins.
```

You couldn't write this with `for 0..something`, because you don't know the
number of years in advance. That's the point of the program.

A simple rule: use `for` when you're counting through a range, and `while`
when you're waiting for something to happen.

### Warning: loops that never end

Something inside a `while` loop must eventually make the condition `false`.
If you forget, the loop runs forever. This countdown forgot `n -= 1`:

```tessel
fn main() {
    var n = 3
    while n > 0 {
        print(n)
    }
    print("Liftoff!")
}
```

```text
3
3
3
3
```

…and so on, forever. `n` is always 3, so `n > 0` is always true. Tessel can't
catch this for you: the program is valid, it just never finishes.

This is called an **infinite loop**, and it happens to every programmer. Don't
panic. To stop the program, press **Ctrl+C** in the terminal (on macOS too:
Control, not Cmd). In the Tessel IDE, press **Stop**. Then look at your loop
and ask: "what changes in each round, and does it bring the condition closer
to `false`?"

## Leaving early: `break` and `continue`

Two statements change how a loop runs:

- `break` leaves the loop straight away. The program continues after the loop.
- `continue` skips the rest of this round and goes on with the next one.

`break` is useful when you're searching and have found what you need. What's
the first number that's divisible by both 5 and 7?

```tessel
fn main() {
    for n in 1..1000 {
        if n % 7 == 0 && n % 5 == 0 {
            print("The first number divisible by 5 and 7 is {n}.")
            break
        }
    }
}
```

```text
The first number divisible by 5 and 7 is 35.
```

Without `break`, the loop would go on and print 70, 105 and so on as well.

`continue` is useful to skip the rounds you're not interested in. Here are
the numbers from 1 to 10, without the multiples of 3:

```tessel
fn main() {
    for n in 1..11 {
        if n % 3 == 0 {
            continue
        }
        print(n)
    }
}
```

```text
1
2
4
5
7
8
10
```

With `break`, you can also write a loop that decides *in the middle* when to
stop. `while true` would run forever on its own; the `break` ends it:

```tessel
fn main() {
    var guess = 1
    while true {
        if guess * guess > 50 {
            break
        }
        guess += 1
    }
    print("{guess} is the first number whose square is above 50.")
}
```

```text
8 is the first number whose square is above 50.
```

## Loops inside loops

A loop body can contain another loop. The inner loop then runs completely,
from start to end, in every round of the outer loop:

```tessel
fn main() {
    for row in 1..4 {
        for col in 1..4 {
            print("{row} x {col} = {row * col}")
        }
    }
}
```

```text
1 x 1 = 1
1 x 2 = 2
1 x 3 = 3
2 x 1 = 2
2 x 2 = 4
2 x 3 = 6
3 x 1 = 3
3 x 2 = 6
3 x 3 = 9
```

The outer loop runs 3 times, and each time the inner loop runs 3 times, so
the inner body runs 3 × 3 = 9 times.

### A multiplication table

To print a real table, each row should be one line. So instead of printing
inside the inner loop, build up the line as a `String` and print it once the
row is complete. (`+=` works on strings too: it adds text to the end.)

```tessel
fn main() {
    for row in 1..6 {
        var line = ""
        for col in 1..6 {
            line += "{row * col}".padStart(4)
        }
        print(line)
    }
}
```

```text
   1   2   3   4   5
   2   4   6   8  10
   3   6   9  12  15
   4   8  12  16  20
   5  10  15  20  25
```

`"{row * col}".padStart(4)` turns the number into text and adds spaces in
front until it's 4 characters wide, so the columns line up. See
[padStart](/reference/stdlib/#padstart-and-padend) for details.

This is the accumulate pattern again, with a `String` instead of a number.
Notice where `var line = ""` is: inside the outer loop, so each row starts
empty, but outside the inner loop, so the row can grow.

## Loop patterns

A handful of patterns cover most loops you'll ever write. You've already seen
**sum** and **count**. Here are two more.

### Find the first

Go through the candidates and stop at the first match. Keep the match in a
variable, so you can use it after the loop:

```tessel
fn main() {
    var found = -1
    for n in 1..100 {
        if n * n > 500 {
            found = n
            break
        }
    }
    if found == -1 {
        print("No such number below 100.")
    } else {
        print("{found} is the first number whose square is above 500.")
    }
}
```

```text
23 is the first number whose square is above 500.
```

`-1` is a placeholder that means "nothing found yet". It works because `-1`
can never be a real answer here. (Later you'll meet *optionals*, a cleaner way
to say "maybe no value".)

### Find the largest

Keep the best value so far, and let each new candidate challenge it. You did
this for three numbers in lesson 4; a loop does it for any number. A ball
thrown up in the air is `20 * t - t * t` meters high after `t` seconds. When
is it highest?

```tessel
fn main() {
    var bestTime = 0
    var bestHeight = 0
    for t in 0..21 {
        let height = 20 * t - t * t
        if height > bestHeight {
            bestHeight = height
            bestTime = t
        }
    }
    print("Highest point: {bestHeight} m after {bestTime} s.")
}
```

```text
Highest point: 100 m after 10 s.
```

### Common mistake: a total that grows too big

Loops can produce very large numbers very quickly. The **factorial** of a
number is all the numbers up to it multiplied together: 5! is
1 × 2 × 3 × 4 × 5 = 120. Let's print a few:

```tessel
fn main() {
    var product = 1
    for n in 1..30 {
        product *= n
        print("{n}! = {product}")
    }
}
```

```text
1! = 1
2! = 2
3! = 6
4! = 24
5! = 120
6! = 720
7! = 5040
8! = 40320
9! = 362880
10! = 3628800
11! = 39916800
12! = 479001600
13! = 6227020800
14! = 87178291200
15! = 1307674368000
16! = 20922789888000
17! = 355687428096000
18! = 6402373705728000
19! = 121645100408832000
20! = 2432902008176640000
error: integer overflow
 --> loops.tsl:4:9
```

21! doesn't fit in an `Int`, so the program stops with a runtime error, as you
saw in [lesson 3](/learn/03-arithmetic/#when-arithmetic-goes-wrong). Notice
also that `product` starts at `1`, not `0`: for a product, 1 is the "nothing
yet" value, because multiplying by 0 would give 0 forever.

## Worked example: FizzBuzz

FizzBuzz is a counting game, and a classic programming puzzle. Count from 1
to 15, but:

- for multiples of 3, say "Fizz" instead of the number,
- for multiples of 5, say "Buzz",
- for multiples of both 3 and 5, say "FizzBuzz".

Break it into steps. First, a loop that counts from 1 to 15: `for n in 1..16`.
Then, inside it, a decision with four outcomes: an `if` / `else if` chain.
Which check comes first? A multiple of both, like 15, is also a multiple of 3.
If "multiple of 3" came first, 15 would say "Fizz" and never reach
"FizzBuzz". So the most specific check goes first, just like the leap year
rule in lesson 4. A multiple of both 3 and 5 is a multiple of 15:

```tessel
fn main() {
    for n in 1..16 {
        if n % 15 == 0 {
            print("FizzBuzz")
        } else if n % 3 == 0 {
            print("Fizz")
        } else if n % 5 == 0 {
            print("Buzz")
        } else {
            print(n)
        }
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

Try moving the `n % 15` check to the end, and see the last line turn into
"Fizz".

## Exercises

**1. Countdown.** Use a `for` loop over `0..10` to print the numbers from 10
down to 1, then "Liftoff!". Hint: if `i` goes up, what goes down?

<details>
<summary>Solution</summary>

```tessel
fn main() {
    for i in 0..10 {
        print(10 - i)
    }
    print("Liftoff!")
}
```

```text
10
9
8
7
6
5
4
3
2
1
Liftoff!
```

When `i` is 0, `10 - i` is 10; when `i` is 9 (the last round), it's 1.

</details>

**2. Even sum.** Add up all the even numbers from 1 to 100.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    var total = 0
    for n in 1..101 {
        if n % 2 == 0 {
            total += n
        }
    }
    print("The even numbers from 1 to 100 add up to {total}.")
}
```

```text
The even numbers from 1 to 100 add up to 2550.
```

</details>

**3. A triangle of stars.** Print this triangle with two nested loops. Row 1
has one star, row 2 has two, and so on:

```text
*
**
***
****
*****
```

<details>
<summary>Solution</summary>

```tessel
fn main() {
    for row in 1..6 {
        var line = ""
        for _ in 0..row {
            line += "*"
        }
        print(line)
    }
}
```

The inner range `0..row` runs `row` times, so it adds as many stars as the row
number.

</details>

**4. Is it prime?** A **prime number** is a whole number of at least 2 that can
only be divided evenly by 1 and itself: 2, 3, 5, 7, 11, 13… Check whether a
number is prime by trying every divisor from 2 up to (but not including) the
number itself. If you find a divisor, print it, and stop searching. Try 91, 97
and 1.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let number = 91
    var isPrime = number >= 2
    for divisor in 2..number {
        if number % divisor == 0 {
            print("{number} = {divisor} x {number / divisor}")
            isPrime = false
            break
        }
    }
    if isPrime {
        print("{number} is prime.")
    } else {
        print("{number} is not prime.")
    }
}
```

```text
91 = 7 x 13
91 is not prime.
```

With 97 it prints `97 is prime.`, and with 1, `1 is not prime.`. `isPrime`
starts as `number >= 2`, because 0 and 1 are not prime; for them, the range
`2..number` is empty and the loop doesn't run at all.

</details>

**5. The 3n + 1 game.** Start with a number. If it's even, halve it; if it's
odd, multiply by 3 and add 1. Repeat until you reach 1. Nobody has ever found
a starting number that doesn't reach 1, but nobody has proved that all of
them do, either! Starting from 27, count the steps it takes to reach 1, and
find the highest number on the way.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let start = 27
    var n = start
    var steps = 0
    var highest = n
    while n != 1 {
        if n % 2 == 0 {
            n = n / 2
        } else {
            n = 3 * n + 1
        }
        steps += 1
        highest = max(highest, n)
    }
    print("{start} reaches 1 after {steps} steps.")
    print("On the way, it climbs as high as {highest}.")
}
```

```text
27 reaches 1 after 111 steps.
On the way, it climbs as high as 9232.
```

This is a job for `while`: you can't know the number of steps in advance.
That's exactly what the program finds out.

</details>

## Summary

- `for i in a..b { … }` runs the body once for each whole number from `a` up
  to, but not including, `b`. `0..n` runs `n` times. Use `_` when you don't
  need the number.
- To accumulate (sum, count, build a string), create the variable **before**
  the loop, update it **inside**, and use it **after**.
- `while condition { … }` repeats as long as the condition is true. Use it
  when you don't know the number of rounds in advance.
- Make sure something in a `while` loop moves it toward the end. Stop a
  runaway program with Ctrl+C, or Stop in the IDE.
- `break` leaves a loop; `continue` skips to the next round.
- A loop inside a loop runs completely in every round of the outer loop.
- Useful patterns: sum, count, find the first (with `break`), and find the
  largest (keep the best so far).

See also: [Control flow](/language/control-flow/) in the language guide.

**Next:** [6. Functions](/learn/06-functions/)
