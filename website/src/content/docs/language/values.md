---
title: Values and copying
description: Tessel's value semantics - why assigning or passing a value gives an independent copy, how to change values in place, and how copy-on-write keeps it fast.
---

In Tessel, every value behaves like a number. When you assign it to another
variable, pass it to a function, or put it in a list, the receiver gets its
**own copy**. Changing one copy never changes another. This is true for
structs, enums, strings, lists and dictionaries alike.

```tessel
struct Point {
    x: Int
    y: Int
}

fn main() {
    var a = Point(x: 1, y: 2)
    var b = a        // b is a copy of a
    b.x = 99
    print("{a.x} {b.x}")
}
```

```text
1 99
```

If you have used Swift, this is how Swift structs and arrays behave. If you
come from JavaScript, Python, Java or C#, it's different: in those languages
`b = a` makes two names for the *same* object, so changing `b.x` would change
`a.x` too. In Tessel it never does.

## Why values?

With values, a piece of data can only change through the variable that holds
it. No other part of the program can change it behind your back. That makes
programs easier to follow, and it rules out a whole class of bugs where two
parts of a program accidentally share and modify the same data.

It also keeps memory management simple. Tessel has no garbage collector and
no pauses to clean up memory: Tessel counts how many places use each piece of
storage (reference counting), and frees it as soon as nothing uses it any
more.

## Copies are cheap

Copying a big list every time you pass it somewhere would be slow, so Tessel
doesn't actually do that. Strings, lists and dictionaries are
**copy-on-write**: a copy at first shares the same storage as the original,
and the storage is only duplicated at the moment one of them is changed.
Passing a large list to a function that only reads it copies nothing.

You never see this sharing in your program. It only affects speed, not
behavior.

## Changing values in place

Because a copy is independent, changing it doesn't change the original. To
change a value that lives inside another value, change it through the
original's path, all in one expression:

```tessel
struct Todo {
    title: String
    done: Bool = false
    tags: [String] = []
}

fn main() {
    var todos = [Todo(title: "Write"), Todo(title: "Test")]

    // Taking an item out gives you a copy:
    var first = todos[0]
    first.done = true
    print(todos[0].done)

    // Changing it through the list changes the list:
    todos[0].done = true
    todos[1].tags.append("urgent")
    print("{todos[0].done} {todos[1].tags.count}")
}
```

```text
false
true 1
```

Any path made of variables, fields and indexes works:
`todos[1].tags[0] = "later"`, `game.players[i].score += 1`, and so on. The
path has to start at something that can change: a `var`, a `state`, a
`bind` parameter, or a loop variable over one of those. Through a `let`,
nothing inside can be changed either, not even a field of an item of a list.

### Loop variables

When you loop over a list you can change (for example, a `var`), the loop
variable is the item in the list, not a copy. Changing it changes the list:

```tessel
struct Todo {
    title: String
    done: Bool = false
}

fn main() {
    var todos = [Todo(title: "Write"), Todo(title: "Test")]
    for todo in todos {
        todo.done = true
    }
    print(todos[1].done)
}
```

```text
true
```

When the list can't change (a `let`, a parameter, or the result of a call),
the loop variable is a constant.

### Snapshots

Since a copy never changes, you can keep one as a snapshot of the past:

```tessel
var scores = [10, 20]
let before = scores
scores.append(30)
scores[0] = 0
print("before: {before.count} items, first {before[0]}")
print("now: {scores.count} items, first {scores[0]}")
```

```text
before: 2 items, first 10
now: 3 items, first 0
```

## Functions and copies

A function receives copies of its arguments, and its parameters can't be
changed. A function that "changes" a value takes it and returns a new one:

```tessel
struct Account {
    owner: String
    balance: Int = 0
}

fn deposit(account: Account, amount: Int) -> Account {
    var updated = account
    updated.balance += amount
    updated
}

fn main() {
    var account = Account(owner: "Ada")
    account = deposit(account: account, amount: 50)
    print(account.balance)
}
```

```text
50
```

Often a method is the more natural way to change a value. A method can
change the value it is called on, as long as that value is in a variable. See
[Structs and enums](/language/structs-enums/#methods-that-change-the-value):

```tessel
struct Account {
    owner: String
    balance: Int = 0

    fn deposit(amount: Int) {
        balance += amount
    }
}

fn main() {
    var account = Account(owner: "Ada")
    account.deposit(amount: 50)
    print(account.balance)
}
```

```text
50
```

A function can also change the caller's variable directly, when it declares
the parameter `bind`. See [Changing the caller's value](#changing-the-callers-value).

## Changing the caller's value

A parameter declared `bind` isn't a copy: it is the caller's variable
itself, so the function reads and changes the caller's value. The argument
must be something that can change, such as a `var`, a field of one, or an
item of a list:

```tessel
struct Account {
    owner: String
    balance: Int = 0
}

fn deposit(_ account: bind Account, amount: Int) {
    account.balance += amount
}

fn main() {
    var accounts = [Account(owner: "Ada"), Account(owner: "Grace")]
    deposit(accounts[1], amount: 50)
    for account in accounts {
        deposit(account, amount: 10)
    }
    print("{accounts[0].balance} {accounts[1].balance}")
}
```

```text
10 60
```

The call looks like any other. The `bind` in the function's declaration
says it can change what it's given, so passing a constant (a `let`), a plain
parameter or a value like `3` is an error.

A `bind` parameter works on top-level functions and on methods. A function
with one can't be used as a value (`let f = deposit`), because a function
type like `fn(Account, Int)` can't say which parameters it changes. Call it
inside a block instead: `{ deposit(account, amount: 10) }`.

Prefer returning a new value or using a method when either reads naturally.
Reach for `bind` when a function changes something that lives in a list or
in another value, or changes more than one thing, like a `swap`:

```tessel
fn swap<T>(_ a: bind T, _ b: bind T) {
    let t = a
    a = b
    b = t
}

fn main() {
    var words = ["left", "middle", "right"]
    swap(words[0], words[2])
    print(words)
}
```

```text
["right", "middle", "left"]
```

## The exception: closures

A [closure](/language/closures/#capturing-variables) uses the variables it
captures directly, not copies of them. Copies of a closure also share those
variables. This is what lets a closure update a counter or a view's `state`.

## Sharing on purpose in views

Sometimes two parts of an app really should work on the same data: a
`TextField` needs to change the text stored in its parent view, for example.
For that, views have `bind` parameters, as functions do, which receive a
*place* to read and write rather than a copy. See
[State and bindings](/ui/state/).
