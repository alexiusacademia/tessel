---
title: "8. Working with text"
description: Measure, search, cut up, clean and build text, and convert between text and numbers.
---

A lot of what programs do is work with text: checking what someone typed,
reading a file, formatting a report, searching for a word. In Tessel, text
is a `String`, and you've been using strings since lesson 1. In this lesson
you'll learn the tools for taking text apart and putting it together.

In this lesson you'll learn:

- why string methods give you a *new* string instead of changing the old one
- how to measure text, change its case, and search in it
- how to split text into pieces and join pieces back together
- how to clean up text with `trim` and `replace`
- how to go through text one character, or one line, at a time
- how to build a string bit by bit in a loop
- how to turn text into a number, and a number into text

## Strings are values

A string is a value, like a number. Just as `5 + 1` gives you a new number
and leaves `5` alone, the string methods in this lesson give you a **new**
string and leave the original alone:

```tessel
fn main() {
    let name = "Ada Lovelace"
    let loud = name.uppercase()
    print(loud)
    print(name)
}
```

```text
ADA LOVELACE
Ada Lovelace
```

`uppercase()` is a **method**: a function that belongs to a value, called
with a dot. (You met methods on lists in lesson 7.) It worked out a
capitalized version of `name`, which we stored in `loud`. `name` itself
didn't change.

To change what a variable holds, assign the new string to it. The variable
must be a `var`:

```tessel
fn main() {
    var title = "hello"
    title = title.uppercase()
    title += "!"
    print(title)
}
```

```text
HELLO!
```

`+=` adds text to the end of a string variable, just as it adds to a number.

### Common mistake: throwing the result away

This program looks like it should print `HELLO`:

```tessel
fn main() {
    var greeting = "hello"
    greeting.uppercase()
    print(greeting)
}
```

```text
error: the result of `uppercase()` isn't used
 --> main.tsl:3:5
  |
3 |     greeting.uppercase()
  |     ^^^^^^^^^^^^^^^^^^^^ this makes a new value, and then throws it away
  |
  = help: text doesn't change by itself; store the result: `greeting = greeting.uppercase()`

1 error found
```

`greeting.uppercase()` makes a new, capitalized string, but it doesn't change
`greeting`, and nothing on that line keeps the new string. It would be thrown
away, and the program would print `hello`. Because that's never what you
want, Tessel stops you. Store the result, as the help shows, or in a new
constant: `let loud = greeting.uppercase()`. Or use it directly:
`print(greeting.uppercase())`.

The same goes for properties like `count` and for list methods like
`sorted()`: a line that works something out and then doesn't use it is an
error.

## Measuring text: `count` and `isEmpty`

`count` is the number of characters in a string. Spaces and punctuation
count too. `isEmpty` is `true` for the empty string `""`:

```tessel
fn main() {
    let word = "Tessel"
    print(word.count)
    print("Hello, world!".count)
    print(word.isEmpty)
    print("".isEmpty)
}
```

```text
6
13
false
true
```

`count` and `isEmpty` have no parentheses, because they're **properties**:
facts about the string, not actions.

## Changing case

`uppercase()` and `lowercase()` give the text in capital or small letters.
They're very useful for comparing text when you don't care about case:

```tessel
fn main() {
    let answer = "YES"
    print(answer == "yes")
    print(answer.lowercase() == "yes")
}
```

```text
false
true
```

To a computer, `"Y"` and `"y"` are different characters, so `"YES"` and
`"yes"` aren't equal. Turning both sides into lowercase first makes the
comparison ignore case.

## Searching: `contains`, `hasPrefix` and `hasSuffix`

These three answer yes-or-no questions about text:

- `contains(…)`: does this text appear anywhere?
- `hasPrefix(…)`: does it *start* with this text?
- `hasSuffix(…)`: does it *end* with this text?

```tessel
fn main() {
    let file = "holiday-photo.jpg"
    print(file.contains("photo"))
    print(file.hasPrefix("holiday"))
    print(file.hasSuffix(".png"))
    print(file.contains("Photo"))
}
```

```text
true
true
false
false
```

The last line is `false` because searching cares about case: `"Photo"` with a
capital P isn't in the text. Use `lowercase()` first if you want to ignore
case.

## Splitting and joining

`split` cuts a string into a list of pieces, wherever a separator appears.
`joined(separator:)` does the opposite: it glues a list of strings together,
with the separator between each piece.

```tessel
fn main() {
    let csv = "apples,pears,plums"
    let fruits = csv.split(",")
    print(fruits.count)
    print(fruits[1])
    print(fruits.joined(separator: " and "))
}
```

```text
3
pears
apples and pears and plums
```

The separator itself isn't kept in the pieces.

`split` is exact: two separators in a row give an empty piece between them.
To split a sentence into words, where there might be several spaces, use
`words()`, which skips the gaps:

```tessel
fn main() {
    let sentence = "the  quick brown   fox"
    print(sentence.split(" ").count)
    print(sentence.words().count)
}
```

```text
7
4
```

## Cleaning up: `trim` and `replace`

Text that comes from people or files often has extra spaces or line breaks
around it. `trim()` removes spaces, tabs and line breaks from both ends, but
not from the middle:

```tessel
fn main() {
    let typed = "   Ada Lovelace  "
    print("[{typed}]")
    print("[{typed.trim()}]")
}
```

```text
[   Ada Lovelace  ]
[Ada Lovelace]
```

(The square brackets are just there so you can see where the spaces are.)

`replace(_:with:)` swaps every appearance of one piece of text for another:

```tessel
fn main() {
    let date = "2026-09-28"
    print(date.replace("-", with: "/"))
    print("I like cats. Cats are great.".replace("cats", with: "dogs"))
}
```

```text
2026/09/28
I like dogs. Cats are great.
```

Like searching, `replace` cares about case, so `"Cats"` with a capital C
stayed as it was.

## One character at a time

`characters` gives you a list with each character of the text as its own
one-letter string. Loop over it to look at each character in turn:

```tessel
fn main() {
    var vowels = 0
    for letter in "programming".characters {
        if "aeiou".contains(letter) {
            vowels += 1
        }
    }
    print("{vowels} vowels")
}
```

```text
3 vowels
```

Notice the trick: `"aeiou".contains(letter)` asks "is this letter one of the
vowels?".

To take out part of a string by position, use `substring(from:to:)`.
Positions count from 0, like list indexes. The character at `from` is
included, the one at `to` isn't:

```tessel
fn main() {
    let word = "Tessel"
    print(word.substring(from: 0, to: 1))
    print(word.substring(from: 1, to: 4))
}
```

```text
T
ess
```

### Common mistake: indexing a string like a list

You can't use square brackets on a string:

```tessel
fn main() {
    let word = "hello"
    print(word[0])
}
```

```text
error: text can't be indexed by number
 --> main.tsl:3:11
  |
3 |     print(word[0])
  |           ^^^^^^^
  |
  = help: get one character with `text.character(at: 0)`, or all of them as a list with `text.characters`

1 error found
```

To get one character, use `word.characters[0]` (which works like any list,
including stopping the program if the index is too big), or
`word.substring(from: 0, to: 1)`. The help also mentions
`word.character(at: 0)`: that one gives back a value that might be missing,
which you'll learn to handle in [lesson 10](/learn/10-optionals-input/).

## One line at a time

Text can span several lines. Inside a string, `\n` stands for a line
break. `lines()` splits text into a list of its lines:

```tessel
fn main() {
    let poem = "Roses are red\nViolets are blue\nTessel is small\nAnd friendly too"
    let poemLines = poem.lines()
    print("{poemLines.count} lines")
    for i in 0..poemLines.count {
        print("{i + 1}: {poemLines[i]}")
    }
}
```

```text
4 lines
1: Roses are red
2: Violets are blue
3: Tessel is small
4: And friendly too
```

You'll use `lines()` a lot when reading files, in
[lesson 16](/learn/16-files-data/).

## Building a string in a loop

A common pattern is to start with an empty string and add to it in a loop:

```tessel
fn main() {
    var row = ""
    for i in 1..6 {
        row += "{i} "
    }
    print(row)

    var backwards = ""
    for letter in "stressed".characters {
        backwards = letter + backwards
    }
    print(backwards)
}
```

```text
1 2 3 4 5 
desserts
```

The second loop puts each new letter at the *front*, so the word comes out
reversed. (There's also a built-in `reversed()` for strings, but it's good
to see how you'd do it yourself.)

Another way to build a string is to collect pieces in a list and `join`
them at the end. That's handy when you want something between the pieces
but not after the last one:

```tessel
fn main() {
    var parts: [String] = []
    for i in 1..6 {
        parts.append("{i * i}")
    }
    print(parts.joined(separator: ", "))
}
```

```text
1, 4, 9, 16, 25
```

## Converting between numbers and text

### Numbers to text

You already know the easiest way: put the number in a string with
interpolation, `"{n}"`. You can also write `String(n)`:

```tessel
fn main() {
    let n = 2026
    let text = "{n}"
    print(text.count)
    print(String(3.5))
}
```

```text
4
3.5
```

### Text to numbers

Going the other way is more interesting. `Int("42")` turns the text `"42"`
into the number `42`. But what should `Int("hello")` give? There's no
sensible number. So `Int(…)` doesn't give a plain `Int`: it gives an
**optional** `Int`, written `Int?`. That's a value that might be missing.

You'll learn everything about optionals in
[lesson 10](/learn/10-optionals-input/). For now, use `??` to say what to
use when the text isn't a number:

```tessel
fn main() {
    let good = Int("42") ?? 0
    let bad = Int("hello") ?? 0
    print(good + 1)
    print(bad + 1)
    print(Float("2.5") ?? 0.0)
}
```

```text
43
1
2.5
```

`Float(…)` works the same way for decimal numbers.

### Common mistake: using the result of `Int(…)` directly

```tessel
fn main() {
    let total = Int("42") + 1
    print(total)
}
```

```text
error: can't use `+` on `Int?` and `Int`
 --> main.tsl:2:17
  |
2 |     let total = Int("42") + 1
  |                 ^^^^^^^^^^^^^
  |
  = help: one side might be `nil`: give it a default with `??` (like `count ?? 0`), or unwrap it with `if let`

1 error found
```

`nil` is Tessel's word for "no value". Tessel won't let you add a number that
might not be there. Give it a default with `?? 0`, as the help says, or wait
for `if let` in lesson 10.

## Worked example: a password checker

Let's write a function that checks whether a password is strong enough, and
explains what's wrong if it isn't. Our rules:

1. At least 8 characters long.
2. Contains at least one digit.
3. Contains at least one capital letter.
4. Doesn't contain the word "password", in any case.

The first and last rules use methods you've seen. For the other two, we'll
go through the characters one by one. How do you tell if a character is a
capital letter? A neat trick: a capital letter changes when you lowercase
it, while a small letter, digit or symbol doesn't.

```tessel
fn problems(_ password: String) -> [String] {
    var found: [String] = []
    if password.count < 8 {
        found.append("it's shorter than 8 characters")
    }

    var hasDigit = false
    var hasCapital = false
    for c in password.characters {
        if "0123456789".contains(c) {
            hasDigit = true
        }
        if c.lowercase() != c {
            hasCapital = true
        }
    }
    if !hasDigit {
        found.append("it has no digit")
    }
    if !hasCapital {
        found.append("it has no capital letter")
    }

    if password.lowercase().contains("password") {
        found.append("it contains the word \"password\"")
    }
    found
}

fn check(_ password: String) {
    let found = problems(password)
    if found.isEmpty {
        print("{password}: strong")
    } else {
        print("{password}: weak, because {found.joined(separator: " and ")}")
    }
}

fn main() {
    check("cat")
    check("MyPassword1")
    check("sunflower")
    check("Sunflower42")
}
```

```text
cat: weak, because it's shorter than 8 characters and it has no digit and it has no capital letter
MyPassword1: weak, because it contains the word "password"
sunflower: weak, because it has no digit and it has no capital letter
Sunflower42: strong
```

How it's built:

- `problems` does the checking, and returns a list of everything that's
  wrong. An empty list means all is well. Keeping the checking separate from
  the printing means we could reuse `problems` elsewhere, for example in an
  app that shows the problems in red.
- `check` does the printing. It uses `joined` to turn the list of problems
  into one readable sentence.
- `password.lowercase().contains("password")` catches `Password`,
  `PASSWORD` and every other mix of cases.
- Inside a string, `\"` is a double quote (lesson 2), so the message can
  quote the word "password".

## Exercises

**1. Shout.** Write `fn shout(_ text: String) -> String` that returns the
text in capitals with `"!"` on the end, after removing any spaces around
it. `shout("  hello there ")` should give `HELLO THERE!`.

<details>
<summary>Solution</summary>

```tessel
fn shout(_ text: String) -> String {
    text.trim().uppercase() + "!"
}

fn main() {
    print(shout("  hello there "))
}
```

```text
HELLO THERE!
```

You can call one method on the result of another: `text.trim()` gives a
new string, and `.uppercase()` is called on that.

</details>

**2. Initials.** Write `fn initials(_ name: String) -> String` that returns
the first letter of each word, in capitals. `initials("ada lovelace")`
should give `AL`, and `initials("grace brewster murray hopper")` should
give `GBMH`.

<details>
<summary>Solution</summary>

```tessel
fn initials(_ name: String) -> String {
    var result = ""
    for word in name.words() {
        result += word.substring(from: 0, to: 1)
    }
    result.uppercase()
}

fn main() {
    print(initials("ada lovelace"))
    print(initials("grace brewster murray hopper"))
}
```

```text
AL
GBMH
```

</details>

**3. Count a letter.** Write `fn countLetter(_ text: String, letter: String) -> Int`,
which counts how often `letter` appears in `text`, ignoring case.
`countLetter("Mississippi", letter: "s")` is `4`, and
`countLetter("Anna", letter: "a")` is `2`.

<details>
<summary>Solution</summary>

```tessel
fn countLetter(_ text: String, letter: String) -> Int {
    var count = 0
    for c in text.lowercase().characters {
        if c == letter.lowercase() {
            count += 1
        }
    }
    count
}

fn main() {
    print(countLetter("Mississippi", letter: "s"))
    print(countLetter("Anna", letter: "a"))
}
```

```text
4
2
```

</details>

**4. Palindromes.** A palindrome reads the same forwards and backwards,
like "level" or "Never odd or even" (ignoring spaces and case). Write
`fn isPalindrome(_ text: String) -> Bool`. Hint: lowercase the text,
remove the spaces with `replace`, and build the reversed version with a
loop.

<details>
<summary>Solution</summary>

```tessel
fn isPalindrome(_ text: String) -> Bool {
    let cleaned = text.lowercase().replace(" ", with: "")
    var backwards = ""
    for c in cleaned.characters {
        backwards = c + backwards
    }
    cleaned == backwards
}

fn main() {
    print(isPalindrome("level"))
    print(isPalindrome("Never odd or even"))
    print(isPalindrome("Tessel"))
}
```

```text
true
true
false
```

</details>

**5. Word statistics.** Write a program that takes this text:
`"the cat sat on the windowsill and the dog slept on the rug"`, and prints how
many words it has, the longest word, and how many times `"the"`
appears.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let text = "the cat sat on the windowsill and the dog slept on the rug"
    let words = text.words()

    var longest = ""
    var theCount = 0
    for word in words {
        if word.count > longest.count {
            longest = word
        }
        if word == "the" {
            theCount += 1
        }
    }

    print("words: {words.count}")
    print("longest: {longest}")
    print("\"the\" appears {theCount} times")
}
```

```text
words: 13
longest: windowsill
"the" appears 4 times
```

`longest` starts empty, so the first word replaces it. After that, a
word only replaces it if it's longer than the longest one so far: the
same "best so far" pattern as in the grades example in lesson 7.

</details>

## Summary

- Strings are values. Methods like `uppercase()` give a **new** string; to
  keep it, store it.
- `count` and `isEmpty` measure text.
- `uppercase()` and `lowercase()` change case; lowercase both sides to
  compare text without caring about case.
- `contains`, `hasPrefix` and `hasSuffix` search, and care about case.
- `split(…)` cuts text into a list; `words()` splits into words;
  `joined(separator:)` glues a list of strings back together.
- `trim()` removes spaces at the ends; `replace(_:with:)` swaps text.
- `characters` and `lines()` let you loop over characters or lines;
  `substring(from:to:)` cuts out part of the text. Strings can't be indexed
  with `[…]`.
- Build text in a loop with `+=`, or collect a list and `join` it.
- `"{n}"` turns a number into text. `Int(text)` and `Float(text)` turn text
  into a number, but give an optional: use `?? default` for now.

The [String reference](/reference/stdlib/#string) lists every string method,
including more you haven't seen here, like `find`, `padStart` and
`capitalized`.

**Next:** [9. Dictionaries](/learn/09-dictionaries/)
