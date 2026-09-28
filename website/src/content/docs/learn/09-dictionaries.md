---
title: "9. Dictionaries"
description: Store values under names instead of positions, look them up, update and remove them, and use a dictionary to count things.
---

A list keeps things in order, and you find an item by its position: item 0,
item 1, item 2. But often you want to find something by a *name* instead.
What's Ada's phone number? How much does a coffee cost? How many times does
the word "the" appear in this book?

For that, Tessel has **dictionaries**. Like a real dictionary, where you
look up a word to find its meaning, a Tessel dictionary lets you look up a
**key** to find its **value**.

In this lesson you'll learn:

- how to create a dictionary of key/value pairs
- how to look up a value, and what happens when the key isn't there
- how to add, change and remove entries
- how to get all the keys and values, and loop over them
- how to use a dictionary to count things
- when to use a list, and when to use a dictionary

## Creating a dictionary

Write each entry as `key: value`, separate the entries with commas, and put
them all in square brackets:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2, "cake": 4]
    print(prices.count)
}
```

```text
3
```

Here the keys are strings (`"coffee"`, `"tea"`, `"cake"`) and the values are
`Int`s. The type of this dictionary is written `[String: Int]`: "a dictionary
from `String` to `Int`". `count` is the number of entries.

All the keys must have the same type, and so must all the values. Keys are
usually `String`s or `Int`s. Each key can appear only once: a dictionary
can't have two different prices for `"tea"`. If you write the same key twice
in the brackets, as in `["tea": 2, "tea": 5]`, Tessel stops with
``the key "tea" appears twice`` and points at both.

### Empty dictionaries

An empty dictionary is written `[:]` (to tell it apart from an empty list,
`[]`). As with an empty list, you must write its type:

```tessel
fn main() {
    var phoneBook: [String: String] = [:]
    print(phoneBook.isEmpty)
}
```

```text
true
```

### Common mistake: an empty dictionary without a type

```tessel
fn main() {
    var phoneBook = [:]
}
```

```text
error: can't tell what type this empty dictionary is
 --> main.tsl:2:21
  |
2 |     var phoneBook = [:]
  |                     ^^^
  |
  = help: give it a type, like `var ages: [String: Int] = [:]`

1 error found
```

## Looking up a value

To look up a key, put it in square brackets, like a list index:
`prices["tea"]`. But there's a twist. What if you ask for a key that isn't
there, like `prices["pizza"]`? A list would stop the program. A dictionary
doesn't: asking whether a key is there is a normal question, not a mistake.

So a lookup gives an **optional**: the value if the key is there, or `nil`
(Tessel's word for "nothing") if it isn't. The type of `prices["tea"]` is
`Int?`, "an `Int`, or nothing".

The simplest way to deal with that is `??`, which you've already met. It
gives a default to use when the value is missing:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2, "cake": 4]
    print(prices["tea"] ?? 0)
    print(prices["pizza"] ?? 0)

    let item = "cake"
    print("A {item} costs {prices[item] ?? 0} dollars")
}
```

```text
2
0
A cake costs 4 dollars
```

In [lesson 10](/learn/10-optionals-input/) you'll learn more ways to handle
optionals, like `if let`, which runs code only when the value is there.

If you only want to know whether a key is there, use `contains(key:)`:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2]
    print(prices.contains(key: "tea"))
    print(prices.contains(key: "pizza"))
}
```

```text
true
false
```

### Common mistake: using a lookup as a plain value

Because a lookup might give `nil`, you can't do sums with it directly:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2]
    let twoTeas = prices["tea"] * 2
    print(twoTeas)
}
```

```text
error: can't use `*` on `Int?` and `Int`
 --> main.tsl:3:19
  |
3 |     let twoTeas = prices["tea"] * 2
  |                   ^^^^^^^^^^^^^^^^^
  |
  = help: one side might be `nil`: give it a default with `??` (like `count ?? 0`), or unwrap it with `if let`

1 error found
```

Tessel is making you decide what should happen if there's no tea. Give a
default, with parentheses so the `??` happens first: `(prices["tea"] ?? 0) *
2`.

## Adding and changing entries

To add an entry, assign to a key. If the key is already there, its value is
replaced. The dictionary must be a `var`:

```tessel
fn main() {
    var stock = ["apples": 10, "pears": 4]
    stock["plums"] = 7
    stock["apples"] = 8
    print(stock.count)
    print(stock["apples"] ?? 0)
}
```

```text
3
8
```

`stock["plums"] = 7` added a new entry. `stock["apples"] = 8` replaced the
old value, 10. There's still only one `"apples"` entry.

To change a value based on the old one, read it with a default, work out the
new value, and store it:

```tessel
fn main() {
    var stock = ["apples": 10, "pears": 4]
    stock["apples"] = (stock["apples"] ?? 0) - 3
    stock["kiwis"] = (stock["kiwis"] ?? 0) + 5
    print(stock["apples"] ?? 0)
    print(stock["kiwis"] ?? 0)
}
```

```text
7
5
```

There were no kiwis yet, so `stock["kiwis"] ?? 0` gave `0`, and the new
entry became `5`. This "read with a default, then store" pattern is the
heart of counting things, as you'll see below.

## Removing entries

To remove an entry, set it to `nil`:

```tessel
fn main() {
    var stock = ["apples": 10, "pears": 4, "plums": 7]
    stock["pears"] = nil
    print(stock.count)
    print(stock.contains(key: "pears"))
}
```

```text
2
false
```

There's also `removeValue(forKey:)`, which removes the entry and gives you
back the value it had (as an optional, since the key might not have been
there):

```tessel
fn main() {
    var stock = ["apples": 10, "pears": 4]
    let removed = stock.removeValue(forKey: "apples") ?? 0
    print("Removed {removed} apples, {stock.count} kind left")
}
```

```text
Removed 10 apples, 1 kind left
```

## Keys, values and looping

`keys` is a list of all the keys, and `values` is a list of all the values.
They're in the order the entries were first added:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2, "cake": 4]
    print(prices.keys.joined(separator: ", "))
    print(prices.values.sum())
}
```

```text
coffee, tea, cake
9
```

(`sum()` adds up a list of numbers. It's one of the list methods from lesson
7.)

You can't loop over a dictionary directly, but you can loop over its keys,
and look up each value:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2, "cake": 4]
    for item in prices.keys {
        print("{item}: {prices[item] ?? 0} dollars")
    }
}
```

```text
coffee: 3 dollars
tea: 2 dollars
cake: 4 dollars
```

Inside the loop, `item` is certainly a key, so the `?? 0` is never actually
used. But Tessel can't know that, so you still need it.

To go through the keys in alphabetical order, sort them first:
`for item in prices.keys.sorted()`.

### Common mistake: printing a dictionary

Like a list, a whole dictionary can't be printed at once:

```tessel
fn main() {
    let prices = ["coffee": 3, "tea": 2]
    print(prices)
}
```

```text
error: a dictionary can't be shown as text directly
 --> main.tsl:3:11
  |
3 |     print(prices)
  |           ^^^^^^
  |
  = help: show its size with `.count`, or loop over its `keys` and show each value

1 error found
```

Loop over the keys and print each entry, as above.

## Counting things

One of the most common uses of a dictionary is counting. Say you want to
know how many times each word appears in a text. The words are the keys, and
the counts are the values. For each word, add one to its count:

```tessel
fn main() {
    let text = "the cat and the dog and the bird"
    var counts: [String: Int] = [:]
    for word in text.words() {
        counts[word] = (counts[word] ?? 0) + 1
    }
    for word in counts.keys {
        print("{word}: {counts[word] ?? 0}")
    }
}
```

```text
the: 3
cat: 1
and: 2
dog: 1
bird: 1
```

The first time a word is seen, it isn't in the dictionary yet, so
`counts[word] ?? 0` gives `0`, and the count becomes `1`. After that, each
time the word comes up, its count goes up by one.

### Common mistake: forgetting the parentheses

This line looks almost the same as the one above, but without the
parentheses:

```tessel
fn main() {
    let text = "the cat and the dog and the bird"
    var counts: [String: Int] = [:]
    for word in text.words() {
        counts[word] = counts[word] ?? 0 + 1
    }
    print(counts["the"] ?? 0)
}
```

```text
error: `??` with `+` after it is ambiguous
 --> main.tsl:5:40
  |
5 |         counts[word] = counts[word] ?? 0 + 1
  |                                        ^^^^^ this is all the default, so `+` only applies to it
  |
  = help: add parentheses: `(value ?? 0) + 1` to do it to the result, or `value ?? (0 + 1)`

1 error found
```

`??` is done *after* `+`, just as `+` is done after `*`. So without
parentheses, the line would mean `counts[word] ?? (0 + 1)`: "the old count,
or if there's none, 1". The old count would never go up, and every word
would end up with a count of 1. That's almost never what you mean, so Tessel
asks you to say which one you want. To add one to the count, write
`(counts[word] ?? 0) + 1`.

## Worked example: word frequency

Let's build a small word-frequency tool. Given some text, it should:

1. count every word, ignoring case and punctuation,
2. list the words alphabetically with their counts,
3. report the most common word.

```tessel
fn cleanWords(_ text: String) -> [String] {
    var cleaned = text.lowercase()
    for mark in [".", ",", "!", "?"] {
        cleaned = cleaned.replace(mark, with: "")
    }
    cleaned.words()
}

fn countWords(_ words: [String]) -> [String: Int] {
    var counts: [String: Int] = [:]
    for word in words {
        counts[word] = (counts[word] ?? 0) + 1
    }
    counts
}

fn main() {
    let text = "One fish, two fish. Red fish, blue fish! One, two, three?"
    let counts = countWords(cleanWords(text))

    for word in counts.keys.sorted() {
        print("{word}: {counts[word] ?? 0}")
    }

    var best = ""
    var bestCount = 0
    for word in counts.keys {
        let n = counts[word] ?? 0
        if n > bestCount {
            best = word
            bestCount = n
        }
    }
    print("Most common: \"{best}\", {bestCount} times")
}
```

```text
blue: 1
fish: 4
one: 2
red: 1
three: 1
two: 2
Most common: "fish", 4 times
```

How it works:

- `cleanWords` lowercases the text (so "One" and "one" count as the same
  word), removes punctuation with `replace`, and splits it into words. You
  learned all of these in lesson 8.
- `countWords` is the counting pattern from above, wrapped in a function that
  returns the finished dictionary.
- `counts.keys.sorted()` gives the keys in alphabetical order.
- Finding the most common word uses the "best so far" pattern from lesson 7:
  look at every entry, and remember the one with the highest count.

## Lists or dictionaries?

Both hold many values. Which should you use?

Use a **list** when:

- the order matters (steps in a recipe, a queue of people, the lines of a
  file),
- you find things by position ("the first", "the third", "the last"),
- the same value can appear more than once and each one matters (all the
  grades in a class).

Use a **dictionary** when:

- you look things up by a name or ID (a phone book, prices, settings),
- each key should appear only once,
- you're counting or totaling things per key (word counts, sales per
  month).

A quick test: if you find yourself looping through a list to find the item
with a particular name, a dictionary with that name as the key is probably
a better fit.

## Exercises

**1. Capitals.** Make a dictionary from countries to their capital cities,
with at least three entries. Look up one country that's in it and one
that isn't, printing `"unknown"` for the missing one.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let capitals = ["France": "Paris", "Japan": "Tokyo", "Kenya": "Nairobi"]
    print(capitals["Japan"] ?? "unknown")
    print(capitals["Atlantis"] ?? "unknown")
}
```

```text
Tokyo
unknown
```

</details>

**2. The shop's stock.** Start with
`var stock = ["apples": 5, "pears": 2, "plums": 0]`. Sell 2 apples, get a
delivery of 6 bananas (a new item), and remove every item whose count is
0. Then print each item and its count in alphabetical order.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    var stock = ["apples": 5, "pears": 2, "plums": 0]
    stock["apples"] = (stock["apples"] ?? 0) - 2
    stock["bananas"] = (stock["bananas"] ?? 0) + 6

    for item in stock.keys {
        if (stock[item] ?? 0) == 0 {
            stock[item] = nil
        }
    }

    for item in stock.keys.sorted() {
        print("{item}: {stock[item] ?? 0}")
    }
}
```

```text
apples: 3
bananas: 6
pears: 2
```

It's safe to remove entries inside the loop: `stock.keys` made a list of
the keys before the loop started, and that list doesn't change.

</details>

**3. Letter counts.** Count how many times each letter appears in
`"mississippi"`, and print the counts in alphabetical order.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    var counts: [String: Int] = [:]
    for letter in "mississippi".characters {
        counts[letter] = (counts[letter] ?? 0) + 1
    }
    for letter in counts.keys.sorted() {
        print("{letter}: {counts[letter] ?? 0}")
    }
}
```

```text
i: 4
m: 1
p: 2
s: 4
```

</details>

**4. Translate back.** Given an English-to-French dictionary, write
`fn reversedDictionary(_ dict: [String: String]) -> [String: String]`
that returns a French-to-English one. Use it to translate `"chat"`.

<details>
<summary>Solution</summary>

```tessel
fn reversedDictionary(_ dict: [String: String]) -> [String: String] {
    var result: [String: String] = [:]
    for key in dict.keys {
        let value = dict[key] ?? ""
        result[value] = key
    }
    result
}

fn main() {
    let englishToFrench = ["cat": "chat", "dog": "chien", "bird": "oiseau"]
    let frenchToEnglish = reversedDictionary(englishToFrench)
    print(frenchToEnglish["chat"] ?? "?")
    print(frenchToEnglish.count)
}
```

```text
cat
3
```

</details>

**5. Group by length.** Given a list of words, build a dictionary of type
`[Int: [String]]` that groups the words by their length: the key is the
length, and the value is a list of the words with that length. Print the
groups from shortest to longest. Hint: to add a word to a group, get the
group's list (or an empty list), append to it, and store it back.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    let words = ["sun", "moon", "star", "sky", "comet", "orbit", "planet"]
    var groups: [Int: [String]] = [:]
    for word in words {
        var group = groups[word.count] ?? []
        group.append(word)
        groups[word.count] = group
    }
    for length in groups.keys.sorted() {
        let group = groups[length] ?? []
        print("{length}: {group.joined(separator: ", ")}")
    }
}
```

```text
3: sun, sky
4: moon, star
5: comet, orbit
6: planet
```

The values of a dictionary can be anything, even lists. Remember that
lists are values: `group` is a *copy* of the list in the dictionary, so
after changing it, you must store it back.

</details>

## Summary

- A **dictionary** stores values under keys: `["tea": 2, "cake": 4]` is a
  `[String: Int]`. An empty one needs its type: `var d: [String: Int] = [:]`.
- Each key appears at most once.
- `dict[key]` looks up a value. It gives an **optional**, because the key
  might be missing: use `?? default` to get a plain value.
- `dict[key] = value` adds or replaces an entry. `dict[key] = nil` or
  `removeValue(forKey:)` removes one. The dictionary must be a `var`.
- `count`, `isEmpty`, `contains(key:)`, `keys` and `values` tell you what's
  in it. Loop over `keys` (or `keys.sorted()`) to go through every entry.
- To count things: `counts[key] = (counts[key] ?? 0) + 1`, with the
  parentheses.
- Use a list for things in order, a dictionary for things you look up by
  name.

For every dictionary method, see [Dictionary](/reference/stdlib/#dictionary)
in the standard library reference.

**Next:** [10. Missing values and input](/learn/10-optionals-input/)
