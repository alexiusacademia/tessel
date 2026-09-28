---
title: Collections and strings
description: Working with lists, dictionaries and strings in Tessel - literals, indexing, and every built-in property and method.
---

Tessel has two collection types, **lists** (`[T]`) and **dictionaries**
(`[K: V]`), and a set of methods for working with them and with strings.

Like all Tessel values, collections are copied when you assign them or pass
them to a function, so changing one copy never changes another. See
[Values and copying](/language/values/).

## Lists

A list holds items of one type, in order.

```tessel
var fruits = ["apple", "banana"]
fruits.append("cherry")
print(fruits.count)       // 3
print(fruits[0])          // apple
fruits[1] = "blueberry"
print(fruits.joined(separator: ", "))
```

```text
3
apple
apple, blueberry, cherry
```

An empty list needs its type written out: `var scores: [Int] = []`.

### Indexing

`list[i]` reads the item at position `i`, counting from `0`. `list[i] = v`
replaces it (the list must be in a `var`). An index outside `0` up to
`count - 1` stops the program with a
[runtime error](/language/optionals/#runtime-errors), so check the index
first, or use `first` and `last`, which return optionals.

Lists can contain lists, and you can index several levels deep:

```tessel
var grid: [[Int]] = [[1, 2], [3, 4]]
grid[1][0] = 30
print(grid[1][0] + grid[0][1])   // 32
```

### Joining lists

`+` joins two lists into a new one, and `+=` adds to a list in place:

```tessel
var queue = [1, 2] + [3]
queue += [4, 5]
print(queue.count)   // 5
```

### List properties

| Property | Type | Value |
|---|---|---|
| `count` | `Int` | the number of items |
| `isEmpty` | `Bool` | `true` if there are no items |
| `first` | `T?` | the first item, or `nil` if the list is empty |
| `last` | `T?` | the last item, or `nil` if the list is empty |

### Methods that change the list

These can only be called on a list that can change: a `var`, `state`,
`bind` parameter, or a field or item of one.

| Method | What it does |
|---|---|
| `append(item)` | adds `item` to the end |
| `insert(item, at: i)` | inserts `item` before position `i` (`i` can be `count`, to add at the end) |
| `remove(at: i)` | removes the item at `i` and returns it |
| `removeAll(where: { x in … })` | removes every item for which the block returns `true` |

```tessel
var numbers = [5, 3, 8]
numbers.append(1)
numbers.insert(10, at: 0)
let removed = numbers.remove(at: 1)
numbers.removeAll(where: { n in n > 7 })
print("removed {removed}, left: {numbers.count}")
```

```text
removed 5, left: 2
```

### Methods that make a new value

These leave the list alone and return a result.

| Method | Returns |
|---|---|
| `filter { x in … }` | a new list of the items for which the block returns `true` |
| `map { x in … }` | a new list of what the block returns for each item |
| `sorted()` | a new list in ascending order (for `[Int]`, `[Float]` and `[String]`) |
| `sorted(by: { a, b in … })` | a new list, where the block returns `true` if `a` should come before `b` |
| `reversed()` | a new list in the opposite order |
| `contains(item)` | `true` if an item is equal to `item` |
| `first(where: { x in … })` | the first item for which the block returns `true`, or `nil` |
| `last(where: { x in … })` | the last such item, or `nil` |
| `joined(separator: s)` | for `[String]` only: the items joined into one string, with `s` between them (the separator is `""` if left out) |

```tessel
let words = ["pear", "fig", "banana", "kiwi"]
print(words.filter { w in w.count > 3 }.joined(separator: " "))
print(words.map { w in w.count }.contains(6))
print(words.sorted().joined(separator: " "))
print(words.sorted(by: { a, b in a.count < b.count })[0])
print(words.first(where: { w in w.hasPrefix("b") }) ?? "none")
print(words.reversed().joined(separator: " "))
```

```text
pear banana kiwi
true
banana fig kiwi pear
fig
banana
kiwi banana fig pear
```

`map` can return a different type from the one it starts with, so
`words.map { w in w.count }` is an `[Int]`. The block must produce a value.

A list of structs or enums has no natural order, so `sorted()` needs a
`by:` rule:

```tessel
struct Player {
    name: String
    score: Int
}

fn main() {
    let players = [Player(name: "Ada", score: 7), Player(name: "Grace", score: 9)]
    let ranked = players.sorted(by: { a, b in a.score > b.score })
    print(ranked.map { p in p.name }.joined(separator: ", "))
}
```

```text
Grace, Ada
```

Strings sort by their character codes, so all uppercase letters come before
lowercase ones: `["b", "Z", "a"].sorted()` is `["Z", "a", "b"]`.

There are more: `index(of:)`, `count(where:)`, `any(where:)` and
`all(where:)`, `sum()`, `min()` and `max()`, `prefix`, `suffix` and `slice`,
`unique()`, `shuffled()`, `removeFirst()`, `appendAll` and others. See
[List](/reference/stdlib/#list) in the standard library reference for all of
them.

The blocks you pass to these methods are
[closures](/language/closures/). Most are given as a trailing block
(`filter { … }`); for the ones with a label, like `sorted(by:)` and
`first(where:)`, write the label and put the block in parentheses.

## Dictionaries

A dictionary stores values under keys. Each key appears at most once.

```tessel
var ages = ["ada": 36, "alan": 41]
ages["grace"] = 85        // add a new key
ages["ada"] = 37          // replace a value
print(ages["ada"] ?? 0)   // 37
print(ages.count)         // 3
```

An empty dictionary is written `[:]` and needs its type:
`var cache: [String: Int] = [:]`.

Keys can be `Int`, `String`, `Bool`, or an enum whose cases carry no
values. Values can be any type.

### Reading and writing

`dict[key]` returns an **optional**, because the key might not be there.
Use `??`, `if let` or `?.` to get the value; see
[Optionals](/language/optionals/).

`dict[key] = value` adds or replaces a value, and `dict[key] = nil` removes
the key. To update a value based on the old one, give a default for the
missing case:

```tessel
var visits: [String: Int] = [:]
visits["home"] = (visits["home"] ?? 0) + 1
visits["home"] = (visits["home"] ?? 0) + 1
print(visits["home"] ?? 0)   // 2
```

To change a value in place, such as appending to a list stored in a
dictionary, use `?.`. It changes the value if the key is there, and does
nothing if it isn't:

```tessel
var groups = ["fruit": ["apple"]]
groups["fruit"]?.append("pear")
groups["veg"]?.append("leek")      // no "veg" key: nothing happens
print(groups["fruit"]?.count ?? 0) // 2
```

### Dictionary properties and methods

| Member | Type | What it does |
|---|---|---|
| `count` | `Int` | the number of keys |
| `isEmpty` | `Bool` | `true` if there are no keys |
| `keys` | `[K]` | all the keys, in the order they were first added |
| `values` | `[V]` | all the values, in the same order |
| `contains(key: k)` | `Bool` | `true` if `k` is a key |
| `removeValue(forKey: k)` | `V?` | removes `k` and returns its value, or `nil` if it wasn't there |

`removeValue` changes the dictionary, so it needs a `var`.

### Looping over a dictionary

Loop over `keys` or `values`:

```tessel
let prices = ["tea": 3, "cake": 5]
for item in prices.keys {
    print("{item} costs {prices[item] ?? 0}")
}
```

```text
tea costs 3
cake costs 5
```

## Strings

Strings aren't collections you can index or loop over, but they have
properties and methods for common text work. None of them change the
string; they return a new value. So a call whose result isn't used, like
`name.uppercase()` on a line of its own, does nothing, and Tessel reports it
as an error: store the result instead, `name = name.uppercase()`. (The same
goes for list methods that return a new list, like `sorted()`.)

| Member | Type | What it does |
|---|---|---|
| `count` | `Int` | the number of characters |
| `isEmpty` | `Bool` | `true` for `""` |
| `contains(s)` | `Bool` | `true` if `s` appears anywhere in the text |
| `hasPrefix(s)` | `Bool` | `true` if the text starts with `s` |
| `hasSuffix(s)` | `Bool` | `true` if the text ends with `s` |
| `find(s)` | `Int?` | the character position where `s` first appears, or `nil` |
| `split(sep)` | `[String]` | the pieces between each `sep`; with `""`, one string per character |
| `lines()` | `[String]` | the lines, without their `\n` or `\r\n` endings |
| `trim()` | `String` | the text without spaces, tabs and new lines at either end |
| `uppercase()` | `String` | the text in capitals |
| `lowercase()` | `String` | the text in small letters |
| `replace(s, with: t)` | `String` | the text with every `s` replaced by `t` |
| `substring(from: a, to: b)` | `String` | the characters from position `a` up to, but not including, `b` |

```tessel
let line = "  Hello, Tessel!  "
let text = line.trim()
print(text.count)
print(text.lowercase())
print(text.find("Tessel") ?? -1)
print(text.substring(from: 7, to: 13))
print(text.replace("Hello", with: "Goodbye"))
print("a,b,c".split(",").count)
```

```text
14
hello, tessel!
7
Tessel
Goodbye, Tessel!
3
```

Positions count characters from `0`, the same way `count` does, so they
work with accented letters and other non-English text. `substring` never
stops the program: positions past the end are clamped to the end of the
text.

To go through a string character by character, loop over its `characters`:

```tessel
for letter in "abc".characters {
    print(letter)
}
```

```text
a
b
c
```

Strings have more methods, such as `words()`, `padStart`, `capitalized()`
and `urlEncoded`; see [String](/reference/stdlib/#string) in the standard
library reference.

`+` joins strings, and `+=` adds to a string variable. Strings compare with
`==` and, in dictionary order, with `<` and `>`.
