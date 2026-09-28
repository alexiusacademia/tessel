---
title: "4. Making decisions"
description: Let your program choose what to do with comparisons, Bool values, if, else if and else, and the logic operators &&, || and !.
---

So far, your programs have done the same thing every time: every statement,
top to bottom. Real programs react to what's going on. A game checks whether
you've won; a shop checks whether an item is in stock. In this lesson, your
programs learn to make decisions.

In this lesson you'll learn:

- how to compare values with `==`, `!=`, `<`, `>`, `<=` and `>=`
- what a `Bool` is really for
- how to run code only sometimes with `if`, `else if` and `else`
- how to combine conditions with `&&` (and), `||` (or) and `!` (not)
- how to use `if` to choose a value
- how to put one decision inside another

## Asking questions: comparisons

A **comparison** asks a yes-or-no question about two values. The answer is a
`Bool`: `true` or `false`.

| Operator | Question | Example | Answer |
|---|---|---|---|
| `==` | are they equal? | `5 == 5` | `true` |
| `!=` | are they different? | `5 != 5` | `false` |
| `<` | is the left smaller? | `3 < 5` | `true` |
| `>` | is the left larger? | `3 > 5` | `false` |
| `<=` | smaller or equal? | `5 <= 5` | `true` |
| `>=` | larger or equal? | `4 >= 5` | `false` |

```tessel
fn main() {
    let age = 15
    print(age > 12)
    print(age < 13)
    print(age == 15)
    print(age != 15)
    print(age >= 18)
    print("cat" == "cat")
    print("Cat" == "cat")
}
```

```text
true
false
true
false
false
true
false
```

Strings can be compared too. Two strings are equal only if they match
exactly, capitals included, so `"Cat" == "cat"` is `false`.

Notice that "is equal" is written with **two** equals signs. A single `=`
already has a job: it stores a value in a variable (see
[lesson 2](/learn/02-values-variables/)). `==` asks a question; `=` gives an
order.

Since the answer is a value, you can store it like any other:

```tessel
let isAdult = age >= 18
```

`isAdult` is a `Bool`. Names for `Bool`s often start with `is`, `has` or
`can`, so they read like a question: `isAdult`, `hasTicket`, `canVote`.

## Doing something only sometimes: `if`

An `if` statement runs a block of code only when a condition is `true`:

```tessel
fn main() {
    let temperature = 32
    if temperature > 30 {
        print("It's hot! Drink some water.")
    }
    print("Have a nice day.")
}
```

```text
It's hot! Drink some water.
Have a nice day.
```

Change `temperature` to `18` and run it again:

```text
Have a nice day.
```

The pieces are:

- `if`, then a **condition**: something that is `true` or `false`.
- A **block**: statements between `{` and `}`. They run only when the
  condition is `true`. The braces are required, even for one line.

Whatever the answer, the program then carries on after the closing `}`. That's
why "Have a nice day." is printed both times.

## Either this or that: `else`

Add an `else` block for what to do when the condition is `false`:

```tessel
fn main() {
    let age = 15
    if age >= 18 {
        print("You can vote.")
    } else {
        print("You can vote in {18 - age} years.")
    }
}
```

```text
You can vote in 3 years.
```

Exactly one of the two blocks runs, never both, never neither.

## More than two choices: `else if`

When there are several possibilities, chain them with `else if`:

```tessel
fn main() {
    let temperature = 22
    if temperature > 30 {
        print("Hot")
    } else if temperature > 15 {
        print("Pleasant")
    } else if temperature > 0 {
        print("Chilly")
    } else {
        print("Freezing")
    }
}
```

```text
Pleasant
```

Tessel checks the conditions from top to bottom and runs the **first** block
whose condition is `true`. Then it skips all the rest. The final `else` catches
everything that didn't match. You can leave it out if you don't need it.

Here `22 > 30` is `false`, so Tessel moves on. `22 > 15` is `true`, so it
prints "Pleasant" and is done. It never even looks at `temperature > 0`,
although that's true too.

### Common mistake: the wrong order

Because the first match wins, order matters. This program has the same
conditions as above, but the other way round:

```tessel
fn main() {
    let temperature = 35
    if temperature > 0 {
        print("Chilly")
    } else if temperature > 15 {
        print("Pleasant")
    } else if temperature > 30 {
        print("Hot")
    }
}
```

```text
Chilly
```

At 35 degrees! `35 > 0` is true, so the first block wins, and the other
conditions are never checked. Tessel can't warn you about this: the program is
valid, it just doesn't say what you meant. With a chain of `>` checks, start
with the largest number. With `<`, start with the smallest.

### Common mistake: `=` instead of `==`

```tessel
fn main() {
    let age = 18
    if age = 18 {
        print("Just 18!")
    }
}
```

```text
error: expected `{` after the `if` condition
 --> decide.tsl:3:12
  |
3 |     if age = 18 {
  |            ^
  |
  = help: the body of an `if` goes in braces: `if ready { … }`

1 error found
```

The message doesn't say "use `==`", but look where it points: at the `=`.
Tessel read `if age` as the condition, and then expected the `{`. When an
`if` error points at an `=`, you almost always meant `==`.

### Common mistake: a condition that isn't a `Bool`

The condition must be a `Bool`. Some languages let you write `if apples` to
mean "if apples isn't zero", but Tessel doesn't guess:

```tessel
fn main() {
    let apples = 3
    if apples {
        print("We have apples.")
    }
}
```

```text
error: expected `Bool`, found `Int`
 --> decide.tsl:3:8
  |
3 |     if apples {
  |        ^^^^^^ this is `Int`

1 error found
```

Say what you mean: `if apples > 0`.

## Combining conditions: `&&`, `||` and `!`

Often one question isn't enough. Three **logic operators** combine `Bool`s:

| Operator | Name | `true` when… |
|---|---|---|
| `a && b` | and | both `a` and `b` are true |
| <code>a &#124;&#124; b</code> | or | at least one of them is true |
| `!a` | not | `a` is false (it flips `true` and `false`) |

```tessel
fn main() {
    let hasTicket = true
    let age = 15
    let withAdult = false
    if hasTicket && age >= 12 {
        print("Enjoy the film!")
    }
    if age >= 16 || withAdult {
        print("You may watch the late show.")
    } else {
        print("The late show is not for you, sorry.")
    }
    if !hasTicket {
        print("Please buy a ticket first.")
    }
}
```

```text
Enjoy the film!
The late show is not for you, sorry.
```

`||` is two vertical bars. On most keyboards, `|` is above the Enter key or
next to the left Shift key.

`&&` is applied before `||`, just like `*` before `+`. When you mix them, add
parentheses so it's clear which parts belong together:
`(a && b) || c`.

### Common mistake: checking a range in one go

In maths you might write 13 ≤ age ≤ 19. Tessel doesn't allow that:

```tessel
fn main() {
    let age = 15
    if 13 <= age <= 19 {
        print("Teenager")
    }
}
```

```text
error: comparisons can't be chained
 --> decide.tsl:3:18
  |
3 |     if 13 <= age <= 19 {
  |                  ^^
  |
  = help: combine them with `&&`, like `a < b && b < c`

1 error found
```

Ask two questions and join them: `if age >= 13 && age <= 19`.

## `if` as a value

Sometimes you use `if` just to pick a value. You could do it with a variable:

```tessel
fn main() {
    let hour = 14
    var greeting = "Good evening"
    if hour < 12 {
        greeting = "Good morning"
    } else if hour < 18 {
        greeting = "Good afternoon"
    }
    print(greeting)
}
```

```text
Good afternoon
```

But there's a shorter way. In Tessel, `if` can **produce a value**: the last
value in the block that runs. So you can write:

```tessel
fn main() {
    let score = 85
    let grade = if score >= 90 { "A" } else if score >= 80 { "B" } else { "C" }
    print("Your grade: {grade}")
}
```

```text
Your grade: B
```

Now `grade` can be a `let`, because it gets its value once and never changes.

When you use `if` for a value, two rules apply:

- Every block must give a value of the **same type**. A grade can't be a
  `String` in one block and an `Int` in another.
- There must be an `else`, so there's always a value:

```text
error: this `if` has no `else`, so it doesn't always produce `String`
 --> decide.tsl:3:60
  |
3 |     let grade = if score >= 90 { "A" } else if score >= 80 { "B" }
  |                                                            ^^^^^^^
  |
  = help: add an `else { … }` branch

1 error found
```

What would `grade` be for a score of 50? Tessel spots that there's no answer
and asks you for one.

## Decisions inside decisions

A block can contain any statements, including another `if`. This is called
**nesting**:

```tessel
fn main() {
    let isWeekend = true
    let isRaining = false
    if isWeekend {
        if isRaining {
            print("Read a book at home.")
        } else {
            print("Go for a walk in the park.")
        }
    } else {
        print("Time for school.")
    }
}
```

```text
Go for a walk in the park.
```

The indentation shows which `else` belongs to which `if`. Keep it tidy, and
avoid nesting too deep: after two or three levels, code gets hard to follow.
Often `&&` or `else if` can flatten it.

A name you create inside a block only exists inside that block. If you need
it afterwards, create it before the `if` (as `greeting` above), or use `if` as
a value.

## Worked example: leap years

A year has 366 days if it's a **leap year**, and 365 otherwise. The rule for
leap years is:

- A year divisible by 4 is a leap year,
- except when it's divisible by 100,
- except when it's divisible by 400 (then it *is* a leap year after all).

So 2024 is a leap year, 2023 isn't, 1900 isn't (divisible by 100), and 2000
is (divisible by 400).

"Divisible by 4" means "the remainder after dividing by 4 is 0", so it's
`year % 4 == 0` (see [lesson 3](/learn/03-arithmetic/)). A good first step is
to give each question a name:

```tessel
fn main() {
    let year = 1900

    let divisibleBy4 = year % 4 == 0
    let divisibleBy100 = year % 100 == 0
    let divisibleBy400 = year % 400 == 0

    if divisibleBy400 {
        print("{year} is a leap year.")
    } else if divisibleBy100 {
        print("{year} is not a leap year.")
    } else if divisibleBy4 {
        print("{year} is a leap year.")
    } else {
        print("{year} is not a leap year.")
    }
}
```

```text
1900 is not a leap year.
```

Notice the order: the most specific rule (400) comes first, as you learned in
"the wrong order" above. Try 2024, 2000 and 2023 as well.

Once you're comfortable, the rule fits in one line with `&&` and `||`, and
`if` as a value picks the number of days:

```tessel
fn main() {
    let year = 2024
    let isLeap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    let days = if isLeap { 366 } else { 365 }
    print("{year} has {days} days.")
}
```

```text
2024 has 366 days.
```

Read the condition out loud: "divisible by 4 and not by 100, or divisible by
400". Both versions are correct. The first one is longer but easier to check
against the rule, which is worth a lot.

## Exercises

**1. Even or odd.** Print whether a number is even or odd. Try it with 17 and
with 42.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let number = 17
    if number % 2 == 0 {
        print("{number} is even.")
    } else {
        print("{number} is odd.")
    }
}
```

```text
17 is odd.
```

</details>

**2. Password check.** `password.count` gives the number of characters in a
`String`. Print "Too short" if a password has fewer than 8 characters, "Too
long" if it has more than 64, and "Password length OK." otherwise.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let password = "tessel2026"
    let length = password.count
    if length < 8 {
        print("Too short: use at least 8 characters.")
    } else if length > 64 {
        print("Too long: use at most 64 characters.")
    } else {
        print("Password length OK.")
    }
}
```

```text
Password length OK.
```

</details>

**3. Cinema tickets.** A ticket costs 10 coins. Children under 4 go free,
children under 12 pay 5, and people aged 65 or over pay 6. Use `if` as a value
to set the price, then print it. Test with the ages 2, 8, 30 and 70.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let age = 70
    let price = if age < 4 {
        0
    } else if age < 12 {
        5
    } else if age >= 65 {
        6
    } else {
        10
    }
    print("A ticket costs {price} coins.")
}
```

```text
A ticket costs 6 coins.
```

With the ages 2, 8 and 30, it prints 0, 5 and 10 coins. `age < 4` must come
before `age < 12`, or a 2-year-old would pay 5.

</details>

**4. The largest of three.** Given three numbers `a`, `b` and `c`, print the
largest, without using `max`. Hint: start with a variable `largest` that holds
`a`, and then let `b` and `c` challenge it.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let a = 12
    let b = 31
    let c = 7
    var largest = a
    if b > largest {
        largest = b
    }
    if c > largest {
        largest = c
    }
    print("The largest is {largest}.")
}
```

```text
The largest is 31.
```

These are two separate `if`s, not an `else if` chain: both challenges must
happen. This "keep the best so far" idea will come back in the
[next lesson](/learn/05-loops/), with loops.

</details>

**5. Rock, paper, scissors.** Store the player's and the computer's choice as
the strings `"rock"`, `"paper"` or `"scissors"`. Print whether it's a tie,
whether the player wins, or whether the player loses. (Rock beats scissors,
paper beats rock, scissors beat paper.)

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let player = "rock"
    let computer = "scissors"

    let rockWins = player == "rock" && computer == "scissors"
    let paperWins = player == "paper" && computer == "rock"
    let scissorsWins = player == "scissors" && computer == "paper"

    if player == computer {
        print("It's a tie!")
    } else if rockWins || paperWins || scissorsWins {
        print("You win: {player} beats {computer}.")
    } else {
        print("You lose: {computer} beats {player}.")
    }
}
```

```text
You win: rock beats scissors.
```

With `player` set to `"paper"`, it prints
`You lose: scissors beats paper.` Naming the three winning situations keeps
the `if` short and readable.

</details>

## Summary

- Comparisons (`==`, `!=`, `<`, `>`, `<=`, `>=`) give a `Bool`: `true` or
  `false`. `==` compares; `=` stores.
- `if condition { … }` runs the block only when the condition is `true`.
  The condition must be a `Bool`, and the braces are required.
- `else { … }` runs when it's `false`. `else if` adds more choices; the first
  one that's true wins, so order matters.
- `&&` (and), `||` (or) and `!` (not) combine conditions. Use parentheses when
  you mix `&&` and `||`.
- `if` can produce a value: `let grade = if score >= 90 { "A" } else { "B" }`.
  Then every block needs a value of the same type, and there must be an
  `else`.
- An `if` can go inside another `if`. A name created inside a block only
  exists there.

See also: [Control flow](/language/control-flow/) in the language guide.

**Next:** [5. Loops](/learn/05-loops/)
