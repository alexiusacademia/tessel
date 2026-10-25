---
title: "7. Lists"
description: Keep many values together in order, read and change them by position, loop over them, and use the built-in methods to sort, search and transform them.
---

A variable holds one value. But programs often deal with *many* values of
the same kind: the scores in a game, the names of your friends, the
temperatures for each day of the week. Making a separate variable for each
one (`score1`, `score2`, `score3`, …) gets out of hand quickly, and you
can't loop over separate variables.

A **list** holds any number of values, in order, under one name.

In this lesson you'll learn:

- how to create a list, including an empty one
- how to read, change, add and remove items
- how to loop over a list
- what happens when you ask for an item that isn't there
- the most useful built-in list methods: `contains`, `sorted`, `reversed`,
  `filter` and `map`
- why changing a copy of a list doesn't change the original

## Creating a list

Write the items between square brackets, separated by commas:

```tessel
fn main() {
    let scores = [72, 95, 58]
    let names = ["Ada", "Grace", "Linus"]
    print(scores.count)
    print(names.count)
}
```

```text
3
3
```

`count` tells you how many items a list has.

All the items in a list must have the **same type**. `scores` is a list of
`Int`s, and its type is written `[Int]`. `names` is a `[String]`. Tessel
works this out from the items, just as it works out that `let age = 36` is
an `Int`.

### Empty lists

Often you start with an empty list and fill it as the program runs. An
empty list, `[]`, has no items for Tessel to look at, so you have to write
its type yourself:

```tessel
fn main() {
    var shopping: [String] = []
    print(shopping.count)
    print(shopping.isEmpty)
}
```

```text
0
true
```

`isEmpty` is `true` when the list has no items. It's a shorter way of
writing `shopping.count == 0`.

### Common mistake: an empty list without a type

If you leave the type out, Tessel can't tell what the list will hold:

```tessel
fn main() {
    var shopping = []
    shopping.append("milk")
}
```

```text
error: can't tell what type this empty list is
 --> main.tsl:2:20
  |
2 |     var shopping = []
  |                    ^^
  |
  = help: give it a type, like `var names: [String] = []`

1 error found
```

## Reading items by position

Each item has a position, called its **index**. Indexes start at **0**, not
1. So the first item is at index 0, the second at index 1, and the last at
index `count - 1`.

To read an item, write the list's name and the index in square brackets:

```tessel
fn main() {
    let names = ["Ada", "Grace", "Linus"]
    print(names[0])
    print(names[1])
    print(names[names.count - 1])
}
```

```text
Ada
Grace
Linus
```

Starting at 0 feels strange at first. It helps to think of the index as
"how many items come before this one": zero items come before `"Ada"`.

## Changing a list

To change a list, it must be a `var`. A list declared with `let` can never
change: not its items, and not its length.

### Replacing an item

Assign to an index to replace the item there:

```tessel
fn main() {
    var names = ["Ada", "Grace", "Linus"]
    names[2] = "Margaret"
    print(names[2])
}
```

```text
Margaret
```

### Adding items

`append` adds an item to the end. `insert` puts an item *before* a given
index, and moves the rest along:

```tessel
fn main() {
    var queue = ["Ada", "Grace"]
    queue.append("Linus")
    queue.insert("Alan", at: 0)
    for name in queue {
        print(name)
    }
}
```

```text
Alan
Ada
Grace
Linus
```

A name followed by a dot and a word, like `queue.append(…)`, is a
**method**: a function that belongs to a value and works on it. `count` is
similar, but without parentheses, because it just reads a fact about the
list. That kind is called a **property**.

### Removing items

`remove(at:)` takes out the item at an index, and moves the later items
back to fill the gap. It also gives you the item it removed, in case you
want it:

```tessel
fn main() {
    var queue = ["Alan", "Ada", "Grace", "Linus"]
    let served = queue.remove(at: 0)
    print("Now serving {served}")
    print("{queue.count} still waiting")
    print("Next up: {queue[0]}")
}
```

```text
Now serving Alan
3 still waiting
Next up: Ada
```

### Common mistake: changing a `let` list

```tessel
fn main() {
    let names = ["Ada", "Grace"]
    names.append("Linus")
}
```

```text
error: can't call `append` on `names`, which is a constant, because `append` changes it
 --> main.tsl:3:5
  |
3 |     names.append("Linus")
  |     ^^^^^
 ::: main.tsl:2:9
  |
2 |     let names = ["Ada", "Grace"]
  |         ----- declared here
  |
  = help: declare it with `var` to be able to change it

1 error found
```

## Looping over a list

You already know `for` loops from lesson 5. A `for` loop over a list runs
its body once for each item, in order:

```tessel
fn main() {
    let temperatures = [18, 21, 25, 19]
    var total = 0
    for t in temperatures {
        total += t
    }
    print("Total: {total}")
}
```

```text
Total: 83
```

When you need the index as well as the item, loop over the indexes with
`0..list.count` (from 0 up to, but not including, `count`), and read each
item with `list[i]`:

```tessel
fn main() {
    let days = ["Mon", "Tue", "Wed"]
    let temperatures = [18, 21, 25]
    for i in 0..days.count {
        print("{i + 1}. {days[i]}: {temperatures[i]} degrees")
    }
}
```

```text
1. Mon: 18 degrees
2. Tue: 21 degrees
3. Wed: 25 degrees
```

When you need each item's position but only one list, `enumerated()` gives
both at once:

```tessel
fn main() {
    let days = ["Mon", "Tue", "Wed"]
    for (i, day) in days.enumerated() {
        print("{i + 1}. {day}")
    }
}
```

```text
1. Mon
2. Tue
3. Wed
```

`(i, day)` takes each position and item apart into two names; you'll see
how this works (it's a *tuple*) in lesson 11.

## Going out of range

What happens if you ask for an index that doesn't exist? Tessel can't know
ahead of time how long a list will be when the program runs, so this mistake
is caught *while the program runs*. The program stops straight away with a
**runtime error**:

```tessel
fn main() {
    let names = ["Ada", "Grace", "Linus"]
    print(names[0])
    print(names[3])
    print("done")
}
```

```text
Ada
error: index 3 is out of range for a list of 3 items
 --> main.tsl:4:11
```

The message tells you which index you asked for, how long the list was,
and the line and column where it happened (line 4, column 11). Notice that
`"done"` was never printed: the program stopped at the error.

A list of 3 items has the indexes 0, 1 and 2. Asking for index 3, one past
the end, is the most common way to hit this error. If you're not sure an
index is valid, check it first:

```tessel
fn main() {
    let names = ["Ada", "Grace", "Linus"]
    let wanted = 3
    if wanted < names.count {
        print(names[wanted])
    } else {
        print("There's no item {wanted}")
    }
}
```

```text
There's no item 3
```

The same error happens with `remove(at:)` and `insert(_:at:)` when the index
is outside the list.

## `first` and `last`

`first` and `last` give you the first and last item. But an empty list has
neither, so these don't give a plain value. They give an **optional**: a
value that might be missing. You'll learn all about optionals in
[lesson 10](/learn/10-optionals-input/). For now, the one thing you need is
`??`, which says "or, if it's missing, use this instead":

```tessel
fn main() {
    let scores = [72, 95, 58]
    let empty: [Int] = []
    print(scores.first ?? 0)
    print(scores.last ?? 0)
    print(empty.first ?? 0)
}
```

```text
72
58
0
```

This is safer than `scores[0]`, which would stop the program if the list
were empty.

## Useful list methods

Lists come with many built-in methods. Here are the ones you'll reach for
most.

### `contains`

`contains` tells you whether an item is in the list:

```tessel
fn main() {
    let guests = ["Ada", "Grace", "Linus"]
    print(guests.contains("Grace"))
    print(guests.contains("Bob"))
}
```

```text
true
false
```

### `sorted` and `reversed`

`sorted()` gives you a **new** list in order, smallest first (or
alphabetical, for text). `reversed()` gives you a new list back to front.
Neither changes the original list:

```tessel
fn main() {
    let scores = [72, 95, 58, 84]
    let ordered = scores.sorted()
    let highestFirst = scores.sorted().reversed()
    print(ordered[0])
    print(highestFirst[0])
    print(scores[0])

    let names = ["Linus", "Ada", "Grace"]
    print(names.sorted().joined(separator: ", "))
}
```

```text
58
95
72
Ada, Grace, Linus
```

`joined(separator:)` glues a list of strings into one string, with the
separator between the items. It's the easiest way to show a whole `[String]`
at once.

### `filter` and `map`

`filter` keeps only the items you want. `map` turns each item into
something else. Both take a small piece of code in braces, called a
**block**, which is run for each item:

```tessel
fn main() {
    let scores = [72, 95, 58, 84]
    let passed = scores.filter { s in s >= 60 }
    let doubled = scores.map { s in s * 2 }
    print(passed.count)
    print(doubled[0])
}
```

```text
3
144
```

Read `{ s in s >= 60 }` as "for each item, call it `s`, and keep it if
`s >= 60`". Read `{ s in s * 2 }` as "for each item `s`, give back `s * 2`".
The name before `in` is up to you.

Blocks are a big topic of their own, and [lesson 13](/learn/13-closures/)
explains them fully. For now, you can use `filter` and `map` with this simple
pattern. Anything they do, you could also do with a `for` loop, just with
more lines.

There are many more methods, such as `sum()`, `min()`, `max()`,
`index(of:)` and `shuffled()`. The [List reference](/reference/stdlib/#list)
lists them all.

### Printing a whole list

`print` shows a whole list the way you'd write it in code, and so does
`{…}` in text:

```tessel
fn main() {
    let scores = [72, 95, 58]
    print(scores)
    let names = ["Ada", "Alan"]
    print("Players: {names}")
}
```

```text
[72, 95, 58]
Players: ["Ada", "Alan"]
```

Text inside a list is shown in quotes, so you can tell `["a b"]` (one item)
from `["a", "b"]` (two). To show the items your own way, loop over them, or
join a list of strings with `joined(separator: ", ")`.

## Functions that take any number of values

Some functions take as many values as you give them. `print` is one:
`print("Total:", total, "kg")` prints them with spaces between. Your own
functions can do the same: write `...` after a parameter's type, and inside
the function, that parameter is a list:

```tessel
fn average(_ values: Float...) -> Float {
    if values.isEmpty {
        return 0.0
    }
    var total = 0.0
    for v in values {
        total += v
    }
    total / Float(values.count)
}

fn main() {
    print(average(2.0, 4.0, 9.0))
    print(average(7.5))
    let scores = [6.0, 8.0]
    print(average(scores...))
}
```

```text
5.0
7.5
7.0
```

`values` is a `[Float]`. With no values, it's an empty list. To pass a list
you already have, write `...` after it: `average(scores...)`. `min` and
`max` take any number too: `max(a, b, c)`.

## Lists are values: copies are separate

When you assign a list to another variable, you get a **copy**. Changing
the copy doesn't change the original:

```tessel
fn main() {
    let original = ["Ada", "Grace"]
    var copy = original
    copy.append("Linus")
    print(original.count)
    print(copy.count)
}
```

```text
2
3
```

The same happens when you pass a list to a function: the function gets its
own copy. This makes programs easier to reason about. When you hand a list
to a function, you know your list is still exactly as you left it. If you
want a changed version, have the function *return* the new list:

```tessel
fn withBonus(_ scores: [Int]) -> [Int] {
    var result = scores
    for i in 0..result.count {
        result[i] += 5
    }
    result
}

fn main() {
    let scores = [72, 95, 58]
    let boosted = withBonus(scores)
    print("{scores[0]} became {boosted[0]}")
}
```

```text
72 became 77
```

To learn more about how Tessel copies values, see
[Values and copying](/language/values/).

## Worked example: grades

Let's put it together. Given a list of test grades, we want to know the
average, the highest grade, and which grades are above average.

We'll write one function for each question. Each one takes the list as a
parameter.

```tessel
fn average(_ grades: [Int]) -> Float {
    var total = 0
    for g in grades {
        total += g
    }
    Float(total) / Float(grades.count)
}

fn highest(_ grades: [Int]) -> Int {
    var best = grades[0]
    for g in grades {
        if g > best {
            best = g
        }
    }
    best
}

fn main() {
    let grades = [72, 95, 58, 84, 90, 67]
    let avg = average(grades)
    print("Grades: {grades.count}")
    print("Average: {avg.formatted(decimals: 1)}")
    print("Highest: {highest(grades)}")

    let above = grades.filter { g in Float(g) > avg }
    print("Above average: {above.count}")
    for g in above {
        print("  {g}")
    }
}
```

```text
Grades: 6
Average: 77.7
Highest: 95
Above average: 3
  95
  84
  90
```

A few things to notice:

- In `average`, `total` and `grades.count` are `Int`s. Dividing two `Int`s
  drops the fraction (lesson 3), so we turn both into `Float`s with
  `Float(…)` first.
- `avg.formatted(decimals: 1)` shows a `Float` with one digit after the
  point, so `77.66666666666667` is shown as `77.7`.
- `highest` starts with the first grade as the best so far, then checks
  every grade against it. It's the "find the largest" pattern from lesson 5:
  keep the best so far, and update it when you find better.
- To compare an `Int` grade with the `Float` average, we turn the grade
  into a `Float` too.

There's a problem hiding here, though. What if the list is empty?
`highest` reads `grades[0]`, which would stop the program with an
out-of-range error. A careful program checks first:

```tessel
fn main() {
    let grades: [Int] = []
    if grades.isEmpty {
        print("No grades yet")
    } else {
        print("Highest: {grades.max() ?? 0}")
    }
}
```

```text
No grades yet
```

This version also uses the built-in `max()`, which finds the largest item
for you. Like `first`, it gives an optional, because an empty list has no
largest item.

## Exercises

**1. Shopping list.** Start with an empty `[String]`. Append `"eggs"`,
`"milk"` and `"bread"`, then insert `"apples"` at the front. Print how many
items there are, then each item on its own line.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    var shopping: [String] = []
    shopping.append("eggs")
    shopping.append("milk")
    shopping.append("bread")
    shopping.insert("apples", at: 0)
    print("{shopping.count} items:")
    for item in shopping {
        print(item)
    }
}
```

```text
4 items:
apples
eggs
milk
bread
```

</details>

**2. Count the evens.** Given `let numbers = [3, 8, 12, 7, 6, 1]`, count
how many are even, first with a `for` loop, then with `filter`.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let numbers = [3, 8, 12, 7, 6, 1]

    var evens = 0
    for n in numbers {
        if n % 2 == 0 {
            evens += 1
        }
    }
    print(evens)

    print(numbers.filter { n in n % 2 == 0 }.count)
}
```

```text
3
3
```

</details>

**3. Smallest, by hand.** Write `fn smallest(_ numbers: [Int]) -> Int` that
finds the smallest number with a loop (don't use `min()`). You may assume
the list isn't empty. Test it with `[42, 7, 19, 3, 25]`.

<details>
<summary>Solution</summary>

```tessel
fn smallest(_ numbers: [Int]) -> Int {
    var result = numbers[0]
    for n in numbers {
        if n < result {
            result = n
        }
    }
    result
}

fn main() {
    print(smallest([42, 7, 19, 3, 25]))
}
```

```text
3
```

</details>

**4. Remove the negatives.** Write `fn positives(_ numbers: [Int]) -> [Int]`
that returns a new list with only the numbers greater than zero. Build the
new list with a loop and `append`. Check that the original list is
unchanged afterwards.

<details>
<summary>Solution</summary>

```tessel
fn positives(_ numbers: [Int]) -> [Int] {
    var result: [Int] = []
    for n in numbers {
        if n > 0 {
            result.append(n)
        }
    }
    result
}

fn main() {
    let readings = [5, -2, 8, 0, -7, 3]
    let good = positives(readings)
    print("{good.count} of {readings.count} readings are positive")
    for n in good {
        print(n)
    }
}
```

```text
3 of 6 readings are positive
5
8
3
```

`readings.filter { n in n > 0 }` does the same in one line.

</details>

**5. Running totals.** Write `fn runningTotals(_ numbers: [Int]) -> [Int]`.
Each item of the result is the sum of all the numbers up to and including
that position. For `[3, 1, 4, 1, 5]` the result is `[3, 4, 8, 9, 14]`.
Print the result as `3, 4, 8, 9, 14`. (Hint: to join numbers, `map` them
to strings with `{ n in "{n}" }` first.)

<details>
<summary>Solution</summary>

```tessel
fn runningTotals(_ numbers: [Int]) -> [Int] {
    var result: [Int] = []
    var sum = 0
    for n in numbers {
        sum += n
        result.append(sum)
    }
    result
}

fn main() {
    let totals = runningTotals([3, 1, 4, 1, 5])
    let texts = totals.map { n in "{n}" }
    print(texts.joined(separator: ", "))
}
```

```text
3, 4, 8, 9, 14
```

</details>

## Summary

- A **list** holds many values of one type, in order: `[72, 95, 58]` is an
  `[Int]`. An empty list needs its type: `var names: [String] = []`.
- Items are numbered from **0**. Read one with `list[i]`, replace it with
  `list[i] = value`.
- `count` is the number of items; `isEmpty` tells you if there are none.
- `append`, `insert(_:at:)` and `remove(at:)` change the list, so it must be
  a `var`.
- An index outside the list stops the program with a runtime error.
  Check indexes against `count`, or use `first` and `last`, which are
  optional.
- `contains`, `sorted()`, `reversed()`, `filter { … }` and `map { … }` answer
  questions or make new lists, leaving the original alone.
- Lists are values: assigning or passing a list makes a copy.
- A parameter written `values: Float...` takes any number of values, as a
  list; `f(list...)` passes a list you have.

**Next:** [8. Working with text](/learn/08-text/)
