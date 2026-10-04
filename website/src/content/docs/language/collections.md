---
title: Collections and strings
description: Working with lists, dictionaries, sets, tuples and strings in Tessel - literals, indexing, and every built-in property and method.
---

Tessel has three collection types, **lists** (`[T]`), **dictionaries**
(`[K: V]`) and **sets** (`Set<T>`), plus **tuples** (`(Int, String)`) for a
few values grouped together, and methods for working with them and with
strings.

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
| `enumerated()` | a list of `(index, item)` [tuples](#tuples): `for (i, x) in list.enumerated()` |
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

Keys can be `Int`, `String`, `Bool`, an enum whose cases carry no values,
or a [tuple](#tuples) or struct made of those. Values can be any type.

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

A `for` loop gives each entry as a `(key, value)` [tuple](#tuples), in the
order the keys were first added:

```tessel
let prices = ["tea": 3, "cake": 5]
for (item, price) in prices {
    print("{item} costs {price}")
}
```

```text
tea costs 3
cake costs 5
```

## Sets

A set holds values without order and without repeats. It answers "is this
in there?" quickly, however many items it has. Write one like a list,
where a set is expected:

```tessel
var tags: Set<String> = ["red", "blue", "red"]
print(tags.count)                 // 2
print(tags.contains("blue"))      // true
tags.insert("green")
tags.remove("red")
print(tags.sorted().joined(separator: ", "))   // blue, green
```

Inserting a value that's already there does nothing, and so does removing
one that isn't. `Set(list)` makes a set from a list, and `Set<Int>()` an
empty one:

```tessel
let words = "the cat saw the other cat".split(" ")
let unique = Set(words)
print(unique.count)    // 4
var empty = Set<Int>()
empty.insert(7)
print(empty.isEmpty)   // false
```

The items of a set can be the same types as dictionary keys: `Int`,
`String`, `Bool`, an enum whose cases carry no values, or a tuple or struct
made of those.

### Combining sets

```tessel
let mine: Set<String> = ["tea", "cake", "jam"]
let yours: Set<String> = ["cake", "scones"]
print(mine.union(yours).count)                  // 4
print(mine.intersection(yours).sorted()[0])     // cake
print(mine.subtracting(yours).sorted().joined(separator: ", "))  // jam, tea
print(yours.isSubset(of: mine))                 // false
print(mine.isDisjoint(with: ["bread"]))         // true
```

### Set properties and methods

| Member | Type | What it does |
|---|---|---|
| `count` | `Int` | the number of items |
| `isEmpty` | `Bool` | `true` if there are no items |
| `values` | `[T]` | the items as a list, in the order they were first added |
| `contains(x)` | `Bool` | `true` if `x` is in the set |
| `insert(x)` | | adds `x` |
| `remove(x)` | | removes `x` |
| `union(other)` | `Set<T>` | the items in either set |
| `intersection(other)` | `Set<T>` | the items in both sets |
| `subtracting(other)` | `Set<T>` | the items not in `other` |
| `isSubset(of: other)` | `Bool` | every item is also in `other` |
| `isSuperset(of: other)` | `Bool` | every item of `other` is in this set |
| `isDisjoint(with: other)` | `Bool` | no item is in both |
| `filter { x in … }` | `Set<T>` | the items the block keeps |
| `sorted()` | `[T]` | the items as a sorted list (`sorted(by:)` for your own order) |

`insert` and `remove` change the set, so it needs a `var`. Two sets are
`==` when they have the same items, in any order.

### Looping over a set

A `for` loop goes through the items in the order they were first added:

```tessel
let primes: Set<Int> = [2, 3, 5, 7]
var total = 0
for p in primes {
    total += p
}
print(total)   // 17
```

## Tuples

A tuple groups a few values, of any types, without declaring a struct. Its
type lists the types in parentheses, and its items are numbered from 0:
`.0`, `.1`, and so on. A function can return several values this way:

```tessel
fn minMax(_ numbers: [Int]) -> (Int, Int) {
    var lo = numbers[0]
    var hi = numbers[0]
    for n in numbers {
        lo = min(lo, n)
        hi = max(hi, n)
    }
    (lo, hi)
}

fn main() {
    let range = minMax([4, 9, 1, 7])
    print("from {range.0} to {range.1}")   // from 1 to 9
    let (lo, hi) = minMax([10, 20])
    print(hi - lo)                         // 10
}
```

`let (lo, hi) = …` takes a tuple apart into names (`var (a, b) = …` makes
variables). A `for` loop can take each item apart too, and `_` skips an
item:

```tessel
let scores: [(String, Int)] = [("Ada", 90), ("Alan", 85)]
for (name, score) in scores {
    print("{name}: {score}")
}
var point = (3, 4)
point.0 += 1
print(point == (4, 4))     // true
let (_, y) = point
print(y)                   // 4
```

`zip(a, b)` pairs up two lists item by item, as long as the shorter one:

```tessel
let names = ["Ada", "Alan"]
let ages = [36, 41, 29]
print(zip(names, ages))        // [("Ada", 36), ("Alan", 41)]
```

Tuples are compared with `==` item by item. A tuple of key types can be a
dictionary key or a set item, which is handy for grid positions. JSON
writes a tuple as an array:

```tessel
var walls: Set<(Int, Int)> = [(0, 1), (2, 2)]
walls.insert((5, 3))
print(walls.contains((2, 2)))    // true
var names: [(Int, Int): String] = [:]
names[(0, 0)] = "start"
print(names[(0, 0)] ?? "")       // start
print(toJson([(1, "one"), (2, "two")]))   // [[1,"one"],[2,"two"]]
```

A tuple can have 2 to 8 items. When the items deserve names, or the value
is used in many places, a [struct](/language/structs-enums/) is clearer.

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

`+` joins strings, and `+=` adds to a string variable. Like `+=` on a
list, it adds to the string where it is, so building a long text a piece at
a time (a CSV file of 100 000 rows, say) takes time in proportion to its
length; a copy of the string made earlier doesn't change. Strings compare
with `==` and, in dictionary order, with `<` and `>`.
