---
title: "11. Structs"
description: Group related values into a type of your own, with fields, default values and methods.
---

So far every value you've used has been one thing: a number, a piece of
text, a list. But the things programs deal with usually have several parts.
A book has a title, an author and a number of pages. A point on a map has an
`x` and a `y`. In this lesson you'll learn to make your own types that keep
those parts together.

In this lesson you'll learn:

- how to declare a **struct**, a type made of named parts called fields
- how to create struct values, and give fields default values
- how to read and change fields
- how to write **methods**, functions that belong to a struct
- why a struct is a value, and what that means when you copy it
- how to keep many structs in a list

## The problem: values that belong together

Say you want to keep track of some books. With what you know so far, you
might write:

```tessel
let title = "The Hobbit"
let author = "J. R. R. Tolkien"
let pages = 310
let title2 = "Dune"
let author2 = "Frank Herbert"
let pages2 = 412
```

This gets messy fast. Nothing says that `title2` and `pages2` belong
together, and passing "a book" to a function means passing three separate
values. What you want is one value that *is* a book.

## Declaring a struct

A **struct** (short for "structure") is a new type that you describe
yourself. You list its parts, called **fields**, each with a name and a
type:

```tessel
struct Book {
    title: String
    author: String
    pages: Int
}

fn main() {
    let book = Book(title: "The Hobbit", author: "J. R. R. Tolkien", pages: 310)
    print(book.title)
    print("{book.title} has {book.pages} pages")
}
```

```text
The Hobbit
The Hobbit has 310 pages
```

A few things to notice:

- The struct is declared outside `main`, at the top level of the file. Now
  `Book` is a type, just like `Int` or `String`.
- Type names start with a capital letter by convention: `Book`, `Point`,
  `Player`. Variables start with a small letter: `book`.
- To create a book, you call `Book(...)` like a function, with a label for
  each field.
- To read a field, write the value, a dot, and the field name: `book.title`.

## Creating values

When you create a struct value, you give the fields **in the order they are
declared**. The labels are there so you can see what each value means, and so
Tessel can catch mixups. Put them in the wrong order and you get an error:

```tessel
let book = Book(author: "Frank Herbert", title: "Dune", pages: 412)
```

```text
error: `title:` is in the wrong position
 --> main.tsl:8:46
  |
8 |     let book = Book(author: "Frank Herbert", title: "Dune", pages: 412)
  |                                              ^^^^^
  |
  = help: arguments go in this order: `title:`, `author:`, `pages:`
```

Leave one out and Tessel tells you which:

```text
error: `Book` is missing `author:`
```

### Default values

Some fields have an obvious starting value. A new book usually hasn't been
read yet. Give a field a **default value** with `=`, and you can leave it out
when you create the value:

```tessel
struct Book {
    title: String
    author: String
    pages: Int
    read: Bool = false
    rating: Int? = nil
}

fn main() {
    let hobbit = Book(title: "The Hobbit", author: "J. R. R. Tolkien", pages: 310)
    let dune = Book(title: "Dune", author: "Frank Herbert", pages: 412, read: true)
    print(hobbit.read)
    print(dune.read)
    print(dune.rating ?? 0)
}
```

```text
false
true
0
```

`rating` is an optional (`Int?`), because not every book has a rating yet.
An optional field still needs a value, so give it the default `nil` if it
should start out empty.

## Changing fields

To change a field, assign to it with `=`, or use `+=` and friends. The value
must be stored in a `var`:

```tessel
struct Point {
    x: Int
    y: Int
}

fn main() {
    var p = Point(x: 2, y: 3)
    p.x = 10
    p.y += 1
    print("({p.x}, {p.y})")
}
```

```text
(10, 4)
```

### Common mistake: changing a field of a `let`

A `let` constant can never change, and that includes its fields. If you
write `let p = Point(x: 2, y: 3)` and then `p.x = 10`, Tessel stops you:

```text
error: can't change `p`, which is a constant
 --> main.tsl:8:5
  |
8 |     p.x = 10
  |     ^^^
 ::: main.tsl:7:9
  |
7 |     let p = Point(x: 2, y: 3)
  |         - declared here
  |
  = help: declare it with `var` to be able to change it
```

The fix is what the help line says: use `var p`. This is on purpose. When
you see `let`, you know the whole value stays the same, every field of it.

### Printing a struct

You can't print a whole struct directly. `print(p)` gives:

```text
error: `Point` can't be shown as text
```

Tessel doesn't guess how you'd like a point to look. Print the fields you
want instead, or write a method that describes the value, as you'll do next.

## Methods

A **method** is a function that lives inside a struct. You call it on a value
with a dot, the same way you read a field:

```tessel
struct Point {
    x: Int
    y: Int

    fn describe() -> String {
        "({x}, {y})"
    }

    fn distanceFromOrigin() -> Int {
        abs(x) + abs(y)
    }
}

fn main() {
    let p = Point(x: 3, y: -4)
    print(p.describe())
    print(p.distanceFromOrigin())
}
```

```text
(3, -4)
7
```

Inside a method, you can use the fields by name: `x` and `y` mean the `x`
and `y` of the point the method was called on. So `p.describe()` uses `p`'s
fields. Methods can take parameters and return results like any function.

Why put a function inside the struct instead of writing
`fn describe(point: Point)` outside? Because the method *belongs* to points.
Anyone reading `p.describe()` knows where to look for it, and your editor can
suggest it when you type `p.`.

### Methods that change the value

A method can also change the struct's fields:

```tessel
struct Point {
    x: Int
    y: Int

    fn describe() -> String {
        "({x}, {y})"
    }

    fn move(dx: Int, dy: Int) {
        x += dx
        y += dy
    }
}

fn main() {
    var p = Point(x: 0, y: 0)
    p.move(dx: 2, dy: 5)
    p.move(dx: 1, dy: -1)
    print(p.describe())
}
```

```text
(3, 4)
```

You don't have to mark `move` in any special way. Tessel sees that it
assigns to a field and remembers that `move` *changes* the point. That has
one consequence: you can only call it on a value that is allowed to change,
like a `var`. Call it on a `let` and you get:

```text
error: can't call `move` on `p`, which is a constant, because `move` changes it
  --> main.tsl:13:5
   |
13 |     p.move(dx: 2, dy: 5)
   |     ^
  ::: main.tsl:12:9
   |
12 |     let p = Point(x: 0, y: 0)
   |         - declared here
   |
   = help: declare it with `var` to be able to change it
```

### `self`

Inside a method, the whole value is called `self`. You rarely need it,
because fields are available by name. The one time you do is when a
parameter has the same name as a field:

```tessel
struct Player {
    name: String
    score: Int = 0

    fn rename(name: String) {
        self.name = name
    }
}

fn main() {
    var player = Player(name: "ada")
    player.rename(name: "Ada")
    print(player.name)
}
```

```text
Ada
```

Here `name` alone means the parameter, and `self.name` means the field.

## Structs are values

In Tessel, a struct behaves like a number. When you assign it to another
variable, you get a separate **copy**. Changing the copy leaves the original
alone:

```tessel
struct Point {
    x: Int
    y: Int
}

fn main() {
    var a = Point(x: 1, y: 1)
    var b = a
    b.x = 50
    print("a is ({a.x}, {a.y})")
    print("b is ({b.x}, {b.y})")
}
```

```text
a is (1, 1)
b is (50, 1)
```

This is the same thing that happens with `var b = a` when `a` is an `Int`:
changing `b` never changes `a`. It means a value can only change through the
variable that holds it, so you never get surprised by something changing
"behind your back". (If you've used Python or JavaScript, be careful: there,
`b = a` would make two names for the *same* object. Not in Tessel.)

The same goes for functions. A function gets its own copy of each argument,
and it can't change its parameters at all:

```tessel
fn moveRight(point: Point) {
    point.x += 1
}
```

```text
error: can't change `point`, which is a parameter
 --> main.tsl:7:5
  |
7 |     point.x += 1
  |     ^^^^^^^
 ::: main.tsl:6:14
  |
6 | fn moveRight(point: Point) {
  |              ----- declared here
  |
  = help: parameters are copies and can't be changed; copy it into a `var`, or (in a view) declare it as `bind`
```

Instead, have the function make a changed copy and return it. (Or use a
method that changes the value, like `move` above.)

```tessel
struct Point {
    x: Int
    y: Int
}

fn movedRight(point: Point) -> Point {
    var moved = point
    moved.x += 1
    moved
}

fn main() {
    let start = Point(x: 1, y: 1)
    let next = movedRight(point: start)
    print("start: {start.x}, next: {next.x}")
    print(start == next)
    print(start == Point(x: 1, y: 1))
}
```

```text
start: 1, next: 2
false
true
```

The last two lines show that `==` works on structs: two values are equal
when all their fields are equal.

## Lists of structs

A struct is a type, so you can have a list of them, `[Todo]`, just like a
list of `Int`s:

```tessel
struct Todo {
    title: String
    done: Bool = false
}

fn main() {
    var todos = [Todo(title: "Buy milk"), Todo(title: "Call Sam"), Todo(title: "Water plants")]
    todos[1].done = true
    todos.append(Todo(title: "Read a book"))

    for todo in todos {
        let mark = if todo.done { "x" } else { " " }
        print("[{mark}] {todo.title}")
    }
}
```

```text
[ ] Buy milk
[x] Call Sam
[ ] Water plants
[ ] Read a book
```

`todos[1].done = true` changes the field of the item *inside* the list. That
works because the whole path, `todos[1].done`, starts at a `var`.

### Common mistake: changing a copy

Because structs are values, taking an item out of a list gives you a copy:

```tessel
struct Todo {
    title: String
    done: Bool = false
}

fn main() {
    var todos = [Todo(title: "Buy milk"), Todo(title: "Call Sam")]
    var first = todos[0]
    first.done = true
    print(todos[0].done)
    todos[0].done = true
    print(todos[0].done)
}
```

```text
false
true
```

Setting `first.done` only changed the copy in `first`. To change the item in
the list, change it through the list: `todos[0].done = true`.

There's one handy exception. When you loop over a list stored in a `var`,
the loop variable *is* the item in the list, not a copy. So inside
`for todo in todos { … }` you can write `todo.done = true` and the list
changes. You'll use this in the next example.

## Tuples: a quick group without a name

Sometimes a few values belong together only for a moment, like the two
answers of a division. Declaring a struct for that is more than you need. A
**tuple** groups values without a declaration: write them in parentheses,
`(3, 2)`, and write its type the same way, `(Int, Int)`. Its items are
numbered from 0: `.0`, `.1`, and so on.

```tessel
fn divide(_ a: Int, by: Int) -> (Int, Int) {
    (a / by, a % by)
}

fn main() {
    let result = divide(17, by: 5)
    print("17 / 5 is {result.0}, remainder {result.1}")

    let (whole, rest) = divide(20, by: 6)
    print("{whole} and {rest} left over")
}
```

```text
17 / 5 is 3, remainder 2
3 and 2 left over
```

`let (whole, rest) = …` takes the tuple apart into two names. A `for` loop
can do the same with each item of a list of tuples:

```tessel
fn main() {
    let moves: [(String, Int)] = [("up", 3), ("right", 2), ("up", 1)]
    for (direction, steps) in moves {
        print("{direction} {steps}")
    }
}
```

```text
up 3
right 2
up 1
```

Tuples are handy for returning two or three values, or for pairs in a list.
When the values deserve names (`result.0` doesn't say what it is), or you
pass them around a lot, a struct is clearer: `book.title` beats `book.0`.

## Worked example: a small library

Let's put it together. A `Library` is a struct too: it holds a list of
books, and its methods do the work of adding, searching and counting.

```tessel
struct Book {
    title: String
    author: String
    pages: Int
    read: Bool = false

    fn describe() -> String {
        "{title} by {author} ({pages} pages)"
    }
}

struct Library {
    books: [Book] = []

    fn add(book: Book) {
        books.append(book)
    }

    fn byAuthor(author: String) -> [Book] {
        var found: [Book] = []
        for book in books {
            if book.author == author {
                found.append(book)
            }
        }
        found
    }

    fn totalPages() -> Int {
        var total = 0
        for book in books {
            total += book.pages
        }
        total
    }

    fn markRead(title: String) {
        for book in books {
            if book.title == title {
                book.read = true
            }
        }
    }

    fn unreadCount() -> Int {
        var count = 0
        for book in books {
            if !book.read {
                count += 1
            }
        }
        count
    }
}

fn main() {
    var library = Library()
    library.add(book: Book(title: "The Hobbit", author: "J. R. R. Tolkien", pages: 310))
    library.add(book: Book(title: "Dune", author: "Frank Herbert", pages: 412))
    library.add(book: Book(title: "The Silmarillion", author: "J. R. R. Tolkien", pages: 365))

    print("Books by Tolkien:")
    for book in library.byAuthor(author: "J. R. R. Tolkien") {
        print("- {book.describe()}")
    }

    print("Total pages: {library.totalPages()}")

    library.markRead(title: "Dune")
    print("Still to read: {library.unreadCount()} of {library.books.count}")
}
```

```text
Books by Tolkien:
- The Hobbit by J. R. R. Tolkien (310 pages)
- The Silmarillion by J. R. R. Tolkien (365 pages)
Total pages: 1087
Still to read: 2 of 3
```

Some things worth noticing:

- `Library()` needs no arguments, because its only field has a default (an
  empty list).
- `add` and `markRead` change the library, so `library` must be a `var`.
  `byAuthor`, `totalPages` and `unreadCount` only read, so they would work on
  a `let` too.
- In `markRead`, `book.read = true` changes the book inside `books`, because
  the loop runs over a field of a value that can change.
- `main` stays short and readable. The details of *how* to find books by an
  author live in one place, inside `Library`.

## Exercises

**1. Rectangle.** Make a struct `Rectangle` with `width` and `height`
(both `Int`). Give it two methods: `area()` and `perimeter()` (the length
all the way around, `2 * (width + height)`). Create a 3 by 4 rectangle and
print both.

<details>
<summary>Solution</summary>

```tessel
struct Rectangle {
    width: Int
    height: Int

    fn area() -> Int {
        width * height
    }

    fn perimeter() -> Int {
        2 * (width + height)
    }
}

fn main() {
    let r = Rectangle(width: 3, height: 4)
    print("Area: {r.area()}")
    print("Perimeter: {r.perimeter()}")
}
```

```text
Area: 12
Perimeter: 14
```

</details>

**2. Bank account.** Make a struct `Account` with an `owner` and a `balance`
that starts at `0`. Add a method `deposit(amount:)` that adds money, and
`withdraw(amount:)` that takes money out and returns `true`, or returns
`false` and changes nothing if there isn't enough. Deposit 100, withdraw 30,
then try to withdraw 500.

<details>
<summary>Solution</summary>

```tessel
struct Account {
    owner: String
    balance: Int = 0

    fn deposit(amount: Int) {
        balance += amount
    }

    fn withdraw(amount: Int) -> Bool {
        if amount > balance {
            return false
        }
        balance -= amount
        true
    }
}

fn main() {
    var account = Account(owner: "Sam")
    account.deposit(amount: 100)
    print(account.withdraw(amount: 30))
    print(account.withdraw(amount: 500))
    print("{account.owner} has {account.balance}")
}
```

```text
true
false
Sam has 70
```

`withdraw` changes `balance`, so it's a changing method, even though it also
returns a value.

</details>

**3. Best student.** Make a struct `Student` with a `name` and a list of
`scores: [Int]`. Give it a method `average() -> Float`. Make a list of three
students, print each one's average with one decimal, and then print the name
of the student with the best average.

Hint: `Float(n)` turns an `Int` into a `Float`, and
`x.formatted(decimals: 1)` turns a `Float` into text with one decimal.

<details>
<summary>Solution</summary>

```tessel
struct Student {
    name: String
    scores: [Int]

    fn average() -> Float {
        if scores.isEmpty {
            return 0.0
        }
        Float(scores.sum()) / Float(scores.count)
    }
}

fn main() {
    let students = [
        Student(name: "Ana", scores: [80, 92, 75]),
        Student(name: "Ben", scores: [95, 88, 90]),
        Student(name: "Cleo", scores: [70, 85, 99]),
    ]

    var best = students[0]
    for student in students {
        print("{student.name}: {student.average().formatted(decimals: 1)}")
        if student.average() > best.average() {
            best = student
        }
    }
    print("Best: {best.name}")
}
```

```text
Ana: 82.3
Ben: 91.0
Cleo: 84.7
Best: Ben
```

The check for an empty list matters: without it, a student with no scores
would divide by zero.

</details>

**4. The longest book.** Add a method `longest() -> Book?` to the `Library`
from the worked example. It returns the book with the most pages, or `nil`
if the library is empty. Test it on an empty library and on a full one.

<details>
<summary>Solution</summary>

```tessel
struct Book {
    title: String
    author: String
    pages: Int
}

struct Library {
    books: [Book] = []

    fn add(book: Book) {
        books.append(book)
    }

    fn longest() -> Book? {
        var best: Book? = nil
        for book in books {
            if book.pages > (best?.pages ?? 0) {
                best = book
            }
        }
        best
    }
}

fn main() {
    var library = Library()
    if let book = library.longest() {
        print(book.title)
    } else {
        print("The library is empty")
    }

    library.add(book: Book(title: "The Hobbit", author: "J. R. R. Tolkien", pages: 310))
    library.add(book: Book(title: "Dune", author: "Frank Herbert", pages: 412))
    library.add(book: Book(title: "Matilda", author: "Roald Dahl", pages: 240))

    if let book = library.longest() {
        print("Longest: {book.title} ({book.pages} pages)")
    } else {
        print("The library is empty")
    }
}
```

```text
The library is empty
Longest: Dune (412 pages)
```

`best?.pages` is the pages of `best`, or `nil` if there is no best book yet;
`?? 0` turns that `nil` into `0`, so the first book always wins the first
comparison.

</details>

## Summary

- A **struct** groups related values into one new type:
  `struct Book { title: String … }`.
- Create a value by calling the type with a label for each field, in order:
  `Book(title: "Dune", author: "Frank Herbert", pages: 412)`.
- Fields can have **default values** (`read: Bool = false`), which you can
  leave out when creating a value.
- Read a field with a dot, `book.title`. To change one, the value must be in
  a `var`.
- **Methods** are functions inside a struct. They use the fields by name.
  A method that changes fields can only be called on a `var`.
- Structs are **values**: assigning or passing one makes a copy, and `==`
  compares all the fields.
- Lists of structs work like any list. Change an item in place through the
  list, `todos[0].done = true`, not through a copy.
- A **tuple** groups a few values without a declaration: `(17, "Ada")` is
  an `(Int, String)`. Read its items with `.0`, `.1`, or take it apart with
  `let (a, b) = pair`.

For the full details, see [Structs and enums](/language/structs-enums/) and
[Values and copying](/language/values/).

**Next:** [12. Enums and match](/learn/12-enums/)
