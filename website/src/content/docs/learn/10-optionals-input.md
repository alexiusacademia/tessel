---
title: "10. Missing values and input"
description: Handle values that might be missing with optionals, if let, ?? and ?., and write interactive programs that read what the user types.
---

In the last few lessons you've met some values that might be missing: the
`first` item of a list that could be empty, a dictionary lookup for a key
that might not be there, a number read from text that might not be a
number. Each time, you used `?? default` to get past them.

This lesson explains what's really going on, and gives you the full set of
tools for missing values. Then you'll use them for something new: programs
that ask the user questions and read their answers.

In this lesson you'll learn:

- what `nil` means, and what an optional type like `Int?` is
- why Tessel makes you deal with missing values
- how to unwrap an optional with `if let`, `??` and `?.`
- how to read what the user types with `readLine()`
- how to turn typed text into numbers safely
- how to read all of the input at once, and how to report errors

## What `nil` means

`nil` is Tessel's word for "no value here". It's not zero, and it's not an
empty string. Zero is a number, and `""` is a piece of text with no
characters. `nil` means there's no number and no text at all.

A variable that might hold `nil` has an **optional type**, written with a
`?` after the type. An `Int?` (say "optional Int") is either an `Int`, or
`nil`:

```tessel
fn main() {
    var middleName: String? = nil
    print(middleName == nil)
    middleName = "Augusta"
    print(middleName == nil)
}
```

```text
true
false
```

Optionals are for values that are sometimes, legitimately, not there. Not
everyone has a middle name. A shop might not have a price for an item. A
user might type "hello" when you asked for a number.

You've already seen where optionals come from:

| Expression | Type | It's `nil` when… |
|---|---|---|
| `list.first`, `list.last` | `Int?` for an `[Int]` | the list is empty |
| `dict[key]` | `Int?` for a `[String: Int]` | the key isn't in the dictionary |
| `Int(text)`, `Float(text)` | `Int?`, `Float?` | the text isn't a number |
| `readLine()` | `String?` | there's no more input (you'll see this below) |

### Common mistake: `nil` in a plain type

Only optional types can hold `nil`. A plain `Int` always has a number in it:

```tessel
fn main() {
    var score: Int = nil
}
```

```text
error: `nil` can't be used as `Int`
 --> main.tsl:2:22
  |
2 |     var score: Int = nil
  |                      ^^^ expected `Int`
  |
  = help: only optional values can be `nil`; make the type optional: `Int?`

1 error found
```

## Why Tessel makes you handle missing values

In many programming languages, *any* value can secretly be "nothing". If
your program then tries to use it (to count its letters, say), it crashes
while running. These crashes are among the most common bugs in software.

Tessel avoids this. A plain `String` is always a real string. Only a
`String?` can be `nil`, and Tessel won't let you use a `String?` as if it
were a `String`, until you've said what should happen when it's `nil`:

```tessel
fn main() {
    let nickname: String? = "Ada"
    print(nickname.count)
}
```

```text
error: this might be `nil`
 --> main.tsl:3:11
  |
3 |     print(nickname.count)
  |           ^^^^^^^^ optional value
  |
  = help: use `?.count` to use it only when there is a value, or unwrap it with `if let`

1 error found
```

This can feel fussy at first. But it means the mistake is found when you
check your program, not by a user when it crashes. Getting the value out of
an optional is called **unwrapping** it, as if the value were inside a box
that might be empty. There are three ways to do it.

## `if let`: run code only when there's a value

`if let` checks whether an optional has a value. If it does, it gives the
value a name, and runs the block. Inside the block, the name is a plain,
non-optional value:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2]
    if let price = prices["tea"] {
        print("Tea costs {price}. Two cost {price * 2}.")
    } else {
        print("We don't sell tea.")
    }
}
```

```text
Tea costs 2. Two cost 4.
```

Read `if let price = prices["tea"]` as "if there is a price for tea, call it
`price`, and…". Inside the braces, `price` is an `Int`, so `price * 2` works.
The `else` part runs when the value is `nil`. You can leave it out if
there's nothing to do in that case.

It's common to reuse the same name, which reads naturally:

```tessel
fn main() {
    let nickname: String? = "Ada"
    if let nickname = nickname {
        print("{nickname} has {nickname.count} letters")
    }
}
```

```text
Ada has 3 letters
```

## `??`: use a default

You know this one already. `a ?? b` gives the value inside `a` if there is
one, and `b` if not. The result is a plain value:

```tessel
fn main() {
    let settings = ["volume": 7]
    let volume = settings["volume"] ?? 5
    let brightness = settings["brightness"] ?? 10
    print("volume {volume}, brightness {brightness}")
}
```

```text
volume 7, brightness 10
```

Use `??` when there's a sensible default. Use `if let` when you need to do
something different in the missing case, like showing a message.

## `?.`: use it only if it's there

`?.` calls a method or reads a property only when there's a value. If
there's `nil`, the whole thing is `nil`. So the result is always optional,
and you'll often finish with `??`:

```tessel
fn main() {
    let words = ["tessel", "is", "fun"]
    let empty: [String] = []
    print(words.first?.uppercase() ?? "nothing")
    print(empty.first?.uppercase() ?? "nothing")
}
```

```text
TESSEL
nothing
```

`words.first` is `"tessel"`, so `?.uppercase()` runs. `empty.first` is `nil`,
so `?.uppercase()` is skipped, the result is `nil`, and `??` gives
`"nothing"`.

## Your own functions can return optionals

When a function might not have an answer, give it an optional return type.
Return `nil` for "no answer":

```tessel
fn indexOfFirstNegative(_ numbers: [Int]) -> Int? {
    for i in 0..numbers.count {
        if numbers[i] < 0 {
            return i
        }
    }
    nil
}

fn main() {
    if let i = indexOfFirstNegative([4, 9, -2, 7]) {
        print("The first negative number is at index {i}")
    }
    print(indexOfFirstNegative([1, 2, 3]) ?? -1)
}
```

```text
The first negative number is at index 2
-1
```

Whoever calls the function can't forget that there might be no answer:
Tessel makes them unwrap it.

## Reading input with `readLine()`

Until now, your programs have always done the same thing each time. Now
let's make them ask questions. `readLine()` waits for the user to type a
line and press Enter, and gives you what they typed, without the Enter.

<!-- nocompare -->
```tessel
fn main() {
    print("What's your name? ", terminator: "")
    let name = readLine() ?? "stranger"
    print("Hello, {name}!")
}
```

Run it with `tessel run hello.tsl`, and the program waits for you to type.
Here's what it looks like in the terminal, after typing `Ada` and pressing
Enter:

```text
What's your name? Ada
Hello, Ada!
```

Two new things here:

- `print(…, terminator: "")` prints without going to a new line at the end.
  (The `terminator` is what `print` puts after the text, and it's normally a
  line break.) That's how you make a **prompt**: the cursor waits on the same
  line, right after the question.
- `readLine()` gives a `String?`, not a `String`. Why could it be `nil`?
  Because the input can *end*. When there's nothing more to read, you get
  `nil`. In a terminal you can end the input by pressing Ctrl+D.

### Giving a program its input in advance

Typing the answers every time you test a program gets tedious. Instead, you
can send text into the program with a **pipe**, the `|` character in the
terminal. The text on the left becomes the program's input:

<!-- stdin: Grace\n -->
<!-- expect: 1 -->
```tessel
fn main() {
    print("What's your name? ", terminator: "")
    let name = readLine() ?? "stranger"
    print("Hello, {name}!")
}
```

```sh
printf 'Grace\n' | tessel run hello.tsl
```

```text
What's your name? Hello, Grace!
```

`printf` prints its text, and `\n` in it is a line break, so this sends the
line `Grace` to the program. Notice that "Grace" doesn't appear after the
prompt this time: when you type, the terminal shows your typing, but piped
input isn't shown.

If there's no input at all, `readLine()` gives `nil`, and `??` steps in:

```sh
printf '' | tessel run hello.tsl
```

```text
What's your name? Hello, stranger!
```

### Reading every line with `while let`

`while let` is like `if let`, but it repeats: it keeps running the loop as
long as there's a value, and stops at the first `nil`. With `readLine()`,
that means "read lines until the input ends":

<!-- stdin: apples\npears\nplums\n -->
<!-- expect: 1 -->
```tessel
fn main() {
    var count = 0
    while let line = readLine() {
        count += 1
        print("{count}: {line.uppercase()}")
    }
    print("That was {count} lines.")
}
```

```sh
printf 'apples\npears\nplums\n' | tessel run shout.tsl
```

```text
1: APPLES
2: PEARS
3: PLUMS
That was 3 lines.
```

## Turning input into numbers

What the user types is always text. To do sums with it, convert it with
`Int(…)`. Since the user might type something that isn't a number, that
gives an optional too, and `if let` is the natural way to handle both
cases:

<!-- stdin: 1990\n -->
<!-- nocompare -->
```tessel
fn main() {
    print("What year were you born? ", terminator: "")
    let text = readLine() ?? ""
    if let year = Int(text.trim()) {
        print("In 2030 you'll be {2030 - year}.")
    } else {
        print("\"{text}\" isn't a year I understand.")
    }
}
```

In the terminal, typing `1990` and then, in a second run, `nineteen`:

```text
What year were you born? 1990
In 2030 you'll be 40.
```

```text
What year were you born? nineteen
"nineteen" isn't a year I understand.
```

`trim()` (from lesson 8) removes any spaces the user typed around the
number.

### Common mistake: using the input as a number directly

```tessel
fn main() {
    let text = readLine() ?? ""
    let year: Int = Int(text)
    print(2030 - year)
}
```

```text
error: expected `Int`, found `Int?`
 --> main.tsl:3:21
  |
3 |     let year: Int = Int(text)
  |                     ^^^^^^^^^ this is `Int?`
  |
  = help: this value might be `nil`; unwrap it with `if let`, or give a default with `?? …`

1 error found
```

## Reading all the input at once

Sometimes you want all of the input in one go, for example to count the
words in a whole file. `readInput()` reads everything until the input ends,
and gives it to you as one `String`. (It never gives `nil`: with no input,
it's just `""`.)

<!-- stdin: the quick brown fox\njumps over\nthe lazy dog\n -->
<!-- expect: 1 -->
```tessel
fn main() {
    let text = readInput()
    let lines = text.lines()
    let words = text.words()
    print("{lines.count} lines, {words.count} words, {text.count} characters")
}
```

You can pipe a file into it with `cat`, or some text with `printf`:

```sh
printf 'the quick brown fox\njumps over\nthe lazy dog\n' | tessel run count.tsl
```

```text
3 lines, 9 words, 44 characters
```

## Reporting errors: `printError` and `exit`

When a program can't do its job, it should say so clearly, and stop.
Two built-in functions help:

- `printError(…)` works like `print`, but writes to the **standard error**
  stream. That's a separate channel for error messages, so they don't get
  mixed up with the program's real output when that's piped into a file or
  another program. In the terminal, both appear on the screen as usual.
- `exit(code)` stops the program right away. The **exit code** is a number
  that tells whoever ran the program how it went: `0` means success,
  anything else means something went wrong.

<!-- stdin: 12\nabc\n5\n -->
<!-- expect: 1 -->
```tessel
fn main() {
    var total = 0
    while let line = readLine() {
        if let n = Int(line) {
            total += n
        } else {
            printError("error: \"{line}\" is not a number")
            exit(1)
        }
    }
    print("Total: {total}")
}
```

```sh
printf '12\nabc\n5\n' | tessel run total.tsl
```

```text
error: "abc" is not a number
```

The program stopped at the bad line, so "Total" was never printed. With
good input, it runs to the end:

```sh
printf '12\n30\n5\n' | tessel run total.tsl
```

```text
Total: 47
```

## Worked example: an adding calculator

Let's write a calculator that asks for two numbers and adds them. It should
be friendly when the user makes a typo: instead of stopping, it asks again.
And if the input runs out, it should report an error.

The work of asking for a number is the same both times, so it goes in a
function. It returns an `Int?`: the number, or `nil` if the input ran out
before the user gave a valid number.

<!-- stdin: 5\n7\n -->
<!-- file: add.tsl -->
<!-- expect: 2 -->
```tessel
fn askNumber(_ prompt: String) -> Int? {
    print(prompt, terminator: "")
    while let line = readLine() {
        if let n = Int(line) {
            return n
        }
        print("\"{line}\" isn't a whole number. Try again: ", terminator: "")
    }
    nil
}

fn main() {
    if let a = askNumber("First number: ") {
        if let b = askNumber("Second number: ") {
            print("{a} + {b} = {a + b}")
            return
        }
    }
    printError("error: I need two numbers")
    exit(1)
}
```

Here's a run in the terminal, with a typo on the first try:

```text
First number: five
"five" isn't a whole number. Try again: 5
Second number: 7
5 + 7 = 12
```

And with piped input:

```sh
printf '5\n7\n' | tessel run add.tsl
```

```text
First number: Second number: 5 + 7 = 12
```

With only one number piped in, the input runs out, `askNumber` returns
`nil`, and the program reports the problem:

```sh
printf '5\n' | tessel run add.tsl
```

```text
First number: Second number: error: I need two numbers
```

How it works:

- `askNumber` prints the prompt, then reads lines until one is a whole
  number, and returns it. The `while let` loop only ends if the input runs
  out, and then the function's last line, `nil`, is the result.
- In `main`, two `if let`s, one inside the other, unwrap both numbers. Only
  if *both* are there do we print the sum. The plain `return` then ends
  `main`, so the error lines below are skipped.
- If either is `nil`, we fall through to `printError` and `exit(1)`.

## Exercises

**1. Default greeting.** Declare `let name: String? = nil`. Print
`Hello, <name>!`, using `"friend"` when the name is missing. Then change
`nil` to `"Ada"` and run it again.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let name: String? = nil
    print("Hello, {name ?? "friend"}!")
}
```

```text
Hello, friend!
```

With `let name: String? = "Ada"`, it prints `Hello, Ada!`.

</details>

**2. Safe division.** Write `fn divide(_ a: Int, by: Int) -> Int?` that
returns `nil` when `by` is `0` (instead of stopping the program with a
division-by-zero error), and `a / by` otherwise. Use `if let` to print
the result of `divide(10, by: 2)`, and `??` to print `divide(1, by: 0)`
with a default of `0`.

<details>
<summary>Solution</summary>

```tessel
fn divide(_ a: Int, by: Int) -> Int? {
    if by == 0 {
        return nil
    }
    a / by
}

fn main() {
    if let result = divide(10, by: 2) {
        print("10 / 2 = {result}")
    }
    print(divide(1, by: 0) ?? 0)
}
```

```text
10 / 2 = 5
0
```

</details>

**3. Longest line.** Write a program that reads lines until the input ends,
and then prints the longest line and its length. Test it with
`printf 'short\nthe longest line\nmedium one\n' | tessel run longest.tsl`.

<details>
<summary>Solution</summary>

<!-- stdin: short\nthe longest line\nmedium one\n -->
```tessel
fn main() {
    var longest = ""
    while let line = readLine() {
        if line.count > longest.count {
            longest = line
        }
    }
    print("Longest: \"{longest}\" ({longest.count} characters)")
}
```

```text
Longest: "the longest line" (16 characters)
```

</details>

**4. Average of the input.** Read numbers, one per line, until the input
ends, and print their average with two decimals. Skip lines that aren't
numbers, but print a warning about each one with `printError`. If there
were no numbers at all, print an error and exit with code 1. Test it with
`printf '4\n8\noops\n9\n' | tessel run average.tsl`.

<details>
<summary>Solution</summary>

<!-- stdin: 4\n8\noops\n9\n -->
```tessel
fn main() {
    var numbers: [Int] = []
    while let line = readLine() {
        if let n = Int(line) {
            numbers.append(n)
        } else {
            printError("skipping \"{line}\"")
        }
    }
    if numbers.isEmpty {
        printError("error: no numbers given")
        exit(1)
    }
    let average = Float(numbers.sum()) / Float(numbers.count)
    print("Average of {numbers.count} numbers: {average.formatted(decimals: 2)}")
}
```

```text
skipping "oops"
Average of 3 numbers: 7.00
```

</details>

**5. Guess the number.** Write a game: the program picks a secret number from
1 to 100 with `random(1..101)` (which gives a random `Int` from 1 up to,
but not including, 101). The player guesses, and after each guess the
program says "Too low" or "Too high", until the guess is right. Then it
says how many guesses it took. If the input ends first, reveal the
number. If the player types something that isn't a number, ask again
without counting it as a guess.

<details>
<summary>Solution</summary>

<!-- stdin: 50\n -->
<!-- nocompare -->
```tessel
fn main() {
    let secret = random(1..101)
    var guesses = 0
    print("I'm thinking of a number from 1 to 100.")
    print("Your guess: ", terminator: "")
    while let line = readLine() {
        if let guess = Int(line.trim()) {
            guesses += 1
            if guess < secret {
                print("Too low. Guess again: ", terminator: "")
            } else if guess > secret {
                print("Too high. Guess again: ", terminator: "")
            } else {
                print("Yes! It was {secret}. You got it in {guesses} guesses.")
                return
            }
        } else {
            print("Please type a whole number: ", terminator: "")
        }
    }
    print("")
    print("The number was {secret}. Bye!")
}
```

A game in the terminal (your secret number will be different):

```text
I'm thinking of a number from 1 to 100.
Your guess: 50
Too low. Guess again: 75
Too low. Guess again: 88
Too low. Guess again: 94
Too high. Guess again: 91
Too high. Guess again: 89
Yes! It was 89. You got it in 6 guesses.
```

The smart strategy is to always guess halfway between what you know is
too low and what you know is too high. With it, you never need more than
7 guesses. This idea, called *binary search*, comes back in
[lesson 17](/learn/17-algorithms/).

</details>

## Summary

- `nil` means "no value". Only an **optional** type, like `Int?` or
  `String?`, can hold `nil`.
- Tessel won't let you use an optional as a plain value until you've
  handled the `nil` case. This catches a whole family of crashes before your
  program runs.
- `if let x = optional { … } else { … }` runs code only when there's a
  value, and names it.
- `optional ?? default` gives the value, or the default.
- `optional?.method()` calls the method only when there's a value; the
  result is optional.
- Your own functions can return `T?` and `nil` for "no answer".
- `readLine()` reads one line of input as a `String?`; it's `nil` when the
  input ends. `while let line = readLine() { … }` reads every line.
- `print(…, terminator: "")` prints a prompt without a line break.
- `Int(text)` turns input into a number, as an `Int?`.
- `readInput()` reads all the input at once.
- `printError(…)` reports errors, and `exit(code)` stops the program; a code
  other than `0` means failure.
- Test programs that read input by piping text in:
  `printf '5\n7\n' | tessel run add.tsl`.

For more, see [Optionals and errors](/language/optionals/) in the language
guide, and [readLine](/reference/stdlib/#readline),
[readInput](/reference/stdlib/#readinput) and
[printError](/reference/stdlib/#printerror) in the standard library
reference.

**Next:** [11. Structs](/learn/11-structs/)
