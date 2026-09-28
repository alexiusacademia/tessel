---
title: 17. Algorithms
description: What an algorithm is, how to search and sort, how to measure how fast your code is, and why some ways of solving a problem are much faster than others.
---

You can now write programs that do real work. This lesson is about doing
that work *well*: choosing a way of solving a problem that stays fast when
the data gets big.

In this lesson you'll learn:

- what an algorithm is
- two ways to search a list: linear search and binary search
- how to sort a list yourself, and why you'll usually use `sorted()`
- how to time your code with `now()`, and what happens when the input
  doubles
- how remembering answers (a *memo*) can make a slow recursive function
  instant

## What is an algorithm?

An **algorithm** is a step-by-step recipe for solving a problem. You've
written many already: "go through the list, and keep the biggest number
you've seen so far" is an algorithm for finding the largest number.

Most problems can be solved in more than one way. The ways all give the
same answer, but they can take very different amounts of time. With ten
items, you'll never notice. With a million, one way might take a
millisecond and another an hour. Learning a few classic algorithms, and
how to tell fast from slow, is one of the most useful skills in
programming.

## Searching: linear search

Say you have a list of numbers and want to know where `42` is. The obvious
recipe: look at each item in turn, from the start, until you find it. This
is called **linear search**:

```tessel
fn linearSearch(_ items: [Int], target: Int) -> Int? {
    for i in items.indices {
        if items[i] == target {
            return i
        }
    }
    nil
}

fn main() {
    let numbers = [3, 8, 15, 21, 42, 57, 64, 90]
    print(linearSearch(numbers, target: 42) ?? -1)
    print(linearSearch(numbers, target: 5) ?? -1)
}
```

```text
4
-1
```

The function returns the index where it found the target, or `nil` if it
isn't there. `return i` leaves the function as soon as the item is found;
there's no point looking further.

Linear search always works, but it can be slow. If the item isn't in the
list, it has to look at *every* item to be sure. A list of a million items
means a million comparisons.

## Searching: binary search

Think about how you look up a word in a paper dictionary. You don't start
at page one. You open it in the middle, see whether your word comes before
or after that page, and throw away the half it can't be in. Then you do the
same with the half that's left.

That's **binary search**. It only works on a **sorted** list, but then it's
very fast. Here it is as a recursive function: one that calls itself. `low` and `high` mark the
part of the list that's still worth searching: from index `low` up to, but
not including, `high`. The `print` shows each step:

```tessel
fn binarySearch(_ items: [Int], target: Int, low: Int, high: Int) -> Int? {
    if low >= high {
        return nil    // nothing left to look at
    }
    let middle = (low + high) / 2
    print("looking between {low} and {high - 1}: middle item is {items[middle]}")
    if items[middle] == target {
        middle
    } else if items[middle] < target {
        binarySearch(items, target: target, low: middle + 1, high: high)
    } else {
        binarySearch(items, target: target, low: low, high: middle)
    }
}

fn main() {
    let numbers = [3, 8, 15, 21, 42, 57, 64, 90]
    print(binarySearch(numbers, target: 57, low: 0, high: numbers.count) ?? -1)
    print(binarySearch(numbers, target: 5, low: 0, high: numbers.count) ?? -1)
}
```

```text
looking between 0 and 7: middle item is 42
looking between 5 and 7: middle item is 64
looking between 5 and 5: middle item is 57
5
looking between 0 and 7: middle item is 42
looking between 0 and 3: middle item is 15
looking between 0 and 1: middle item is 8
looking between 0 and 0: middle item is 3
-1
```

Follow the search for `57`: the middle item is `42`, which is too small, so
`57` must be to its right. Now only indexes 5 to 7 are left. The middle of
those is `64`, too big, so only index 5 is left, and there it is.

Every recursive function needs a **base case**, a situation where it stops
calling itself. Here there are two: the item is found, or there's nothing
left to search (`low >= high`). Each call works on a smaller part of the
list, so one of them is always reached.

How much faster is it? Each step throws away half of what's left. This
program counts how many times you can halve a list before nothing is left:

```tessel
fn main() {
    for size in [10, 1000, 1000000, 1000000000] {
        var left = size
        var steps = 0
        while left > 0 {
            left = left / 2
            steps += 1
        }
        print("{size} items: linear search up to {size} steps, binary search up to {steps}")
    }
}
```

```text
10 items: linear search up to 10 steps, binary search up to 4
1000 items: linear search up to 1000 steps, binary search up to 10
1000000 items: linear search up to 1000000 steps, binary search up to 20
1000000000 items: linear search up to 1000000000 steps, binary search up to 30
```

A billion items, found in 30 steps. Making the list a thousand times bigger
only adds 10 steps.

## Sorting

Binary search needs a sorted list, so how do you sort one? There are many
sorting algorithms. One of the simplest to understand is **selection
sort**:

1. Find the smallest item, and swap it into the first place.
2. Find the smallest of the *rest*, and swap it into the second place.
3. Keep going until you reach the end.

```tessel
fn selectionSort(_ items: [Int]) -> [Int] {
    var list = items
    for i in list.indices {
        // Find the smallest item from position i to the end.
        var smallest = i
        for j in i + 1..list.count {
            if list[j] < list[smallest] {
                smallest = j
            }
        }
        // Swap it into position i.
        let temp = list[i]
        list[i] = list[smallest]
        list[smallest] = temp
    }
    list
}

fn show(_ list: [Int]) -> String {
    list.map { n in "{n}" }.joined(separator: " ")
}

fn main() {
    let numbers = [42, 7, 19, 3, 88, 21]
    print(show(selectionSort(numbers)))
    print(show(numbers.sorted()))
    print(show(numbers))
}
```

```text
3 7 19 21 42 88
3 7 19 21 42 88
42 7 19 3 88 21
```

Some things to notice:

- Swapping two items needs a temporary variable. If you wrote
  `list[i] = list[smallest]` first, the old `list[i]` would be lost.
- The function gets a copy of the list (parameters are always copies), so
  it makes its own `var list` to change, and returns it. The original
  `numbers` is unchanged, as the last line shows.
- `show` is a small helper, because `print` can't print a whole list
  directly.

Tessel's own `sorted()` gives the same result. So why write your own? To
understand how sorting works, and to see why the built-in one is worth
using. Let's measure.

## Measuring time

`now()` gives the current time, in seconds, as a `Float`. Call it before
and after some work, and the difference is how long the work took.

This program sorts lists of 2,000, 4,000, 8,000, 16,000 and 32,000 numbers
both ways, and times each. The `jumbled` function makes a list of numbers
in a mixed-up order, with a formula that gives the same "random-looking"
numbers every run:

```tessel
fn selectionSort(_ items: [Int]) -> [Int] {
    var list = items
    for i in list.indices {
        var smallest = i
        for j in i + 1..list.count {
            if list[j] < list[smallest] {
                smallest = j
            }
        }
        let temp = list[i]
        list[i] = list[smallest]
        list[smallest] = temp
    }
    list
}

// A list of `count` numbers in a jumbled order.
fn jumbled(_ count: Int) -> [Int] {
    var x = 42
    var items: [Int] = []
    for _ in 0..count {
        x = (x * 1103515245 + 12345) % 2147483648
        items.append(x % 1000000)
    }
    items
}

fn millisSince(_ start: Float) -> String {
    ((now() - start) * 1000.0).formatted(decimals: 1)
}

fn main() {
    var size = 2000
    while size <= 32000 {
        let items = jumbled(size)

        var start = now()
        let a = selectionSort(items)
        let slow = millisSince(start)

        start = now()
        let b = items.sorted()
        let fast = millisSince(start)

        print("{size} items: selection sort {slow} ms, sorted() {fast} ms, same: {a == b}")
        size *= 2
    }
}
```

On one computer, this printed:

```text
2000 items: selection sort 4.6 ms, sorted() 0.1 ms, same: true
4000 items: selection sort 17.9 ms, sorted() 0.1 ms, same: true
8000 items: selection sort 68.1 ms, sorted() 0.3 ms, same: true
16000 items: selection sort 240.4 ms, sorted() 0.5 ms, same: true
32000 items: selection sort 792.7 ms, sorted() 1.1 ms, same: true
```

Your numbers will be different: they depend on your computer, and on what
else it's doing. Run it twice and even your own numbers change a little.
What matters is the *pattern*:

- Each time the list **doubles**, selection sort takes about **four
  times** as long (4.6, 17.9, 68.1, 240.4 …).
- `sorted()` takes only about **twice** as long.

Why four times? Selection sort compares every item with the items after
it. Double the items, and each of the twice-as-many passes is also twice as
long: 2 × 2 = 4. Programmers call this *quadratic* growth, or write it
**O(n²)** ("order n squared"): the time grows with the square of the size
`n`.

`sorted()` uses a cleverer algorithm that keeps splitting the list in
halves, a bit like binary search. Its time grows only a little faster than
the size itself. That's written **O(n log n)**. At 32,000 items it is
already hundreds of times faster, and the gap keeps growing: at a million
items, selection sort would take many minutes; `sorted()` takes a blink.

Here are the common growth patterns, in plain words:

| Name | When the input doubles, the time… | Example |
|---|---|---|
| O(1), constant | stays the same | reading `list[5]`, looking up a dictionary key |
| O(log n) | grows by one small step | binary search |
| O(n), linear | doubles | linear search, adding up a list |
| O(n log n) | a bit more than doubles | `sorted()` |
| O(n²), quadratic | grows four times | selection sort, comparing every pair |

You don't need the math. The habit that matters is asking: *what happens to
my program if the data gets ten times bigger?*

## Recursion again: remembering answers

The Fibonacci numbers start with 0 and 1, and each next one is the sum of
the two before it: 0, 1, 1, 2, 3, 5, 8, 13, 21 … That definition turns
straight into a recursive function:

```tessel
fn fib(_ n: Int) -> Int {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}
```

It's correct, and it's beautifully short. Let's time it:

```tessel
fn main() {
    for n in [30, 35, 40, 45] {
        let start = now()
        let result = fib(n)
        let seconds = (now() - start).formatted(decimals: 3)
        print("fib({n}) = {result} in {seconds} s")
    }
}
```

```text
fib(30) = 832040 in 0.002 s
fib(35) = 9227465 in 0.021 s
fib(40) = 102334155 in 0.242 s
fib(45) = 1134903170 in 2.707 s
```

Each time `n` grows by 5, the time grows about **eleven times**. `fib(50)`
would take half a minute, and `fib(60)` about an hour.

The problem is that it does the same work again and again. `fib(45)` calls
`fib(44)` and `fib(43)`. But `fib(44)` *also* calls `fib(43)`, and each of
those calls `fib(42)`, and so on. The small cases are recomputed billions of
times.

The fix: remember each answer the first time you compute it, and look it up
next time. A dictionary from `n` to `fib(n)` is perfect for this. It's
called a **memo** (as in a note to yourself), and the technique
**memoization**.

The memo has to survive between calls. A function's parameters are copies,
so a function can't change a dictionary passed to it. A struct can: a
method that changes a field keeps the change (you saw this in
[lesson 11](/learn/11-structs/)). So the memo goes in a struct:

```tessel
struct Fibonacci {
    memo: [Int: Int] = [:]

    fn fib(_ n: Int) -> Int {
        if n < 2 {
            return n
        }
        if let known = memo[n] {
            return known
        }
        let result = fib(n - 1) + fib(n - 2)
        memo[n] = result
        result
    }
}

fn main() {
    var f = Fibonacci()
    let start = now()
    let big = f.fib(90)
    let seconds = (now() - start).formatted(decimals: 3)
    print("with a memo: fib(90) = {big} in {seconds} s")
}
```

```text
with a memo: fib(90) = 2880067194370816120 in 0.000 s
```

`fib(90)` in less than a thousandth of a second, where the plain version
would take longer than you'll live. Each `fib(n)` is now computed only once,
so the work grows linearly with `n`: O(n) instead of the plain version's
explosive growth.

This is a pattern you'll use again and again: **trade a little memory for a
lot of time.**

## Worked example: finding a repeat

Here's a practical problem: does a list contain any number twice? Let's
solve it two ways and compare.

**Approach 1: compare every pair.** Take each item, and compare it with
every item after it. Simple, and it uses no extra memory.

**Approach 2: remember what you've seen.** Walk through the list once,
keeping a dictionary of the numbers seen so far. For each item, check the
dictionary first: if it's already there, that's a repeat. Looking up a key
in a dictionary takes about the same time however big it is, so this is one
pass over the list.

```tessel
// Approach 1: compare every item with every item after it.
fn hasRepeatSlow(_ items: [Int]) -> Bool {
    for i in items.indices {
        for j in i + 1..items.count {
            if items[i] == items[j] {
                return true
            }
        }
    }
    false
}

// Approach 2: remember what we've seen in a dictionary.
fn hasRepeatFast(_ items: [Int]) -> Bool {
    var seen: [Int: Bool] = [:]
    for item in items {
        if seen.contains(key: item) {
            return true
        }
        seen[item] = true
    }
    false
}
```

First, check that both give the right answers on small examples. A fast
answer is worthless if it's wrong:

```tessel
fn main() {
    print(hasRepeatSlow([4, 8, 15, 8]))
    print(hasRepeatFast([4, 8, 15, 8]))
    print(hasRepeatFast([4, 8, 15, 16]))
}
```

```text
true
true
false
```

Now time them. To test the worst case, where there's no repeat and both
functions have to look at everything, `mixedNumbers` makes the numbers from
0 to `count - 1` in a mixed-up order:

```tessel
// The numbers 0 to count - 1, mixed up, with no repeats.
fn mixedNumbers(_ count: Int) -> [Int] {
    var items: [Int] = []
    for i in 0..count {
        items.append((i * 7919) % count)
    }
    items
}

fn millisSince(_ start: Float) -> String {
    ((now() - start) * 1000.0).formatted(decimals: 1)
}

fn main() {
    var size = 5000
    while size <= 80000 {
        let items = mixedNumbers(size)

        var start = now()
        let a = hasRepeatSlow(items)
        let slow = millisSince(start)

        start = now()
        let b = hasRepeatFast(items)
        let fast = millisSince(start)

        print("{size}: compare all pairs {slow} ms, dictionary {fast} ms ({a}, {b})")
        size *= 2
    }
}
```

On one computer:

```text
5000: compare all pairs 5.9 ms, dictionary 0.2 ms (false, false)
10000: compare all pairs 23.5 ms, dictionary 0.3 ms (false, false)
20000: compare all pairs 94.2 ms, dictionary 0.5 ms (false, false)
40000: compare all pairs 380.6 ms, dictionary 1.0 ms (false, false)
80000: compare all pairs 1511.0 ms, dictionary 2.1 ms (false, false)
```

Again, your numbers will differ, but the pattern won't. Comparing all pairs
is O(n²): four times slower each time the list doubles. The dictionary
version is O(n): twice as slow. At 80,000 items it's over 700 times faster,
and with a million items the first approach would take about four minutes,
the second a fraction of a second.

Which should you use? For a list of 20 items, either: both are instant, and
the simplest code wins. Speed only matters when the data is big, or the
code runs very often. But when it matters, it matters a lot, and a better
algorithm beats any amount of tuning.

## Common mistakes

### Binary search on an unsorted list

Binary search assumes the list is
sorted. On an unsorted list, it throws away the wrong half, and silently
gives a wrong answer. There's no error message, which makes this mistake
sneaky:

```tessel
fn main() {
    let numbers = [42, 7, 19, 3, 88, 21]
    print(binarySearch(numbers, target: 7, low: 0, high: numbers.count) ?? -1)
    let sorted = numbers.sorted()
    print(binarySearch(sorted, target: 7, low: 0, high: sorted.count) ?? -1)
}
```

```text
-1
1
```

`7` *is* in the list, but the first search says it isn't. Sort first.

### Going one step too far

Loops over indexes are easy to get off by one.
This function tries to find the largest number, but its range goes one past
the end:

```tessel
fn largest(_ items: [Int]) -> Int {
    var best = items[0]
    for i in 1..items.count + 1 {
        if items[i] > best {
            best = items[i]
        }
    }
    best
}

fn main() {
    print(largest([4, 8, 15, 16]))
}
```

```text
error: index 4 is out of range for a list of 4 items
 --> main.tsl:4:12
```

The indexes of a list with 4 items are 0 to 3, and `1..items.count` already
stops *before* `items.count`. Remove the `+ 1`.

### Numbers that are too big

Memoization makes `fib` fast enough to
compute huge values, but an `Int` can only hold numbers up to about 9
quintillion (a 9 with 18 digits after it). `fib(93)` is bigger than that:

```tessel
fn main() {
    var f = Fibonacci()
    print(f.fib(100))
}
```

```text
error: integer overflow
 --> main.tsl:11:22
```

Line 11 is `let result = fib(n - 1) + fib(n - 2)`, where the addition got
too big. Tessel stops the program instead of giving a wrong answer.

## Exercises

**1. The smallest number.** Write `smallest(_ items: [Int]) -> Int` with a
loop (don't use `min()`). Check that it gives the same answer as
`numbers.min()` for `[42, 7, 19, 3, 88, 21]`.

<details>
<summary>Solution</summary>

```tessel
fn smallest(_ items: [Int]) -> Int {
    var best = items[0]
    for item in items {
        if item < best {
            best = item
        }
    }
    best
}

fn main() {
    let numbers = [42, 7, 19, 3, 88, 21]
    print(smallest(numbers))
    print(numbers.min() ?? 0)
}
```

```text
3
3
```

This is linear: it looks at each item once. (It stops with an error for an
empty list, because `items[0]` doesn't exist; `min()` returns `nil`
instead.)

</details>

**2. Adding up, recursively.** Write a recursive function
`total(_ items: [Int]) -> Int` that adds up a list without a loop. Think of
it like this: the total of a list is its first item plus the total of the
rest (`items.dropFirst()`). What's the base case?

<details>
<summary>Solution</summary>

```tessel
fn total(_ items: [Int]) -> Int {
    if let first = items.first {
        first + total(items.dropFirst())
    } else {
        0
    }
}

fn main() {
    print(total([4, 8, 15, 16, 23, 42]))
    print(total([7]))
    print(total([]))
}
```

```text
108
7
0
```

The base case is the empty list, whose total is 0. `items.first` is `nil`
exactly then, so `if let` handles both cases. Each call works on a list one
item shorter, so the base case is always reached. (It works, but a loop or
`sum()` is simpler here: recursion shines on problems that split into
smaller copies of themselves, like binary search.)

</details>

**3. Insertion sort.** Another simple sort works the way many people sort a
hand of cards: take each item in turn, and slide it left past every bigger
item until it's in its place. Write `insertionSort(_ items: [Int]) -> [Int]`
and check it against `sorted()`.

<details>
<summary>Solution</summary>

```tessel
fn insertionSort(_ items: [Int]) -> [Int] {
    var list = items
    for i in 1..list.count {
        let item = list[i]
        var j = i - 1
        // Move bigger items one place to the right.
        while j >= 0 && list[j] > item {
            list[j + 1] = list[j]
            j -= 1
        }
        list[j + 1] = item
    }
    list
}

fn show(_ list: [Int]) -> String {
    list.map { n in "{n}" }.joined(separator: " ")
}

fn main() {
    let numbers = [42, 7, 19, 3, 88, 21]
    print(show(insertionSort(numbers)))
    print(insertionSort(numbers) == numbers.sorted())
}
```

```text
3 7 19 21 42 88
true
```

Insertion sort is also O(n²) in general, but it's very fast on a list
that's already almost sorted, because each item only moves a little.

</details>

**4. Binary search with a loop.** Rewrite binary search without recursion:
keep `low` and `high` in variables and use a `while` loop.

<details>
<summary>Solution</summary>

```tessel
fn binarySearch(_ items: [Int], target: Int) -> Int? {
    var low = 0
    var high = items.count
    while low < high {
        let middle = (low + high) / 2
        if items[middle] == target {
            return middle
        } else if items[middle] < target {
            low = middle + 1
        } else {
            high = middle
        }
    }
    nil
}

fn main() {
    let numbers = [3, 8, 15, 21, 42, 57, 64, 90]
    print(binarySearch(numbers, target: 57) ?? -1)
    print(binarySearch(numbers, target: 3) ?? -1)
    print(binarySearch(numbers, target: 5) ?? -1)
}
```

```text
5
0
-1
```

Every recursive function can be written with a loop, and the other way
round. Use whichever is clearer. (The loop version is also nicer to call:
no `low:` and `high:` to pass.)

</details>

**5. Race the searches.** Make a sorted list of a million even numbers
(0, 2, 4, …). Then time 1,000 searches with linear search, and the same
1,000 searches with binary search from exercise 4. Search for `k * 1999`
for `k` from 0 to 999, so about half of them are found.

<details>
<summary>Solution</summary>

```tessel
fn linearSearch(_ items: [Int], target: Int) -> Int? {
    for i in items.indices {
        if items[i] == target {
            return i
        }
    }
    nil
}

// binarySearch from exercise 4 goes here.

fn millisSince(_ start: Float) -> String {
    ((now() - start) * 1000.0).formatted(decimals: 1)
}

fn main() {
    var numbers: [Int] = []
    for i in 0..1000000 {
        numbers.append(i * 2)
    }

    var start = now()
    var found = 0
    for k in 0..1000 {
        if linearSearch(numbers, target: k * 1999) != nil {
            found += 1
        }
    }
    print("linear search: found {found} in {millisSince(start)} ms")

    start = now()
    found = 0
    for k in 0..1000 {
        if binarySearch(numbers, target: k * 1999) != nil {
            found += 1
        }
    }
    print("binary search: found {found} in {millisSince(start)} ms")
}
```

On one computer:

```text
linear search: found 500 in 259.2 ms
binary search: found 500 in 0.1 ms
```

Thousands of times faster, for the same answers.

</details>

## Summary

- An **algorithm** is a recipe for solving a problem. Different recipes can
  give the same answer at very different speeds.
- **Linear search** checks items one by one: it works on any list.
  **Binary search** halves the search each step, but needs a sorted list.
- A **recursive** function calls itself on a smaller problem, and needs a
  base case where it stops.
- Selection sort and insertion sort are easy to write; the built-in
  `sorted()` is much faster on big lists.
- Time code with `now()` before and after. Numbers vary between computers;
  look at how the time grows when the input doubles.
- O(n) doubles when the input doubles; O(n²) grows four times. Ask what
  happens when your data gets ten times bigger.
- A **memo** (a dictionary of answers already computed) can turn a
  hopelessly slow recursive function into an instant one.

**Next:** [18. Project: a to-do list in the terminal](/learn/18-project-cli/)
