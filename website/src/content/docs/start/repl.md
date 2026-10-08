---
title: The REPL
description: Try Tessel code a line at a time with tessel repl, where variables, functions and types stay as you go.
---

`tessel repl` runs code as you type it. It's the quickest way to try
something out: what a method returns, whether a function does what you meant,
how a piece of the standard library behaves. There's no file to make and no
`fn main()` to write.

```sh
tessel repl
```

```text
Tessel 0.1.41 (:help for help, :quit to leave)
>
```

Type a line at the `>` prompt and press Enter. (REPL stands for
read–eval–print loop: it reads what you type, runs it, prints the result, and
waits for more.)

## Expressions

An expression shows its value:

```text
> 1 + 2
3
> "Hello, {"Ada"}!"
Hello, Ada!
> [3, 1, 2].sorted()
[1, 2, 3]
> 10 / 4
2
> 10.0 / 4
2.5
```

Statements that have no value, like `print(…)`, a `let`, or an assignment,
just run.

## Variables

Variables stay for the lines that follow:

```text
> let name = "Ada"
> name.uppercase()
ADA
> var total = 0
> total += 5
> total
5
```

A `let` can't be changed afterwards, as in a program, but you can declare the
name again, even with another type. The new one takes its place:

```text
> let total = "five"
> total
five
```

`:vars` lists what you have so far:

```text
> :vars
let name: String
let total: String
```

## Functions and types

Functions, structs, enums and interfaces are declared as in a file, and stay
too:

```text
> fn double(n: Int) -> Int { n * 2 }
> double(21)
42
```

Declare one again to replace it. Everything that uses it, uses the new one:

```text
> fn double(n: Int) -> Int { n + n + 1 }
> double(21)
43
```

When a struct or enum is declared again, values of the old one no longer fit
it, so the variables holding them are removed. The REPL tells you:

```text
> struct Point { x: Int }
> let p = Point(x: 1)
> struct Point { x: Int, y: Int }
note: `p` is gone, because its type (`Point`) was declared again
```

## Modules

Start the REPL in your project's folder to try out its
[modules](/language/programs/#modules). An `import` stays for the rest of the
session:

```text
> import geometry
> let c = geometry.Circle(radius: 2)
> c.area()
12.56636
> :vars
let c: geometry.Circle
```

As in a file, `import geometry` looks for the folder `geometry` next to
where you are, `import "../shared/geometry"` takes a path, and only what the
module marks `public` can be used. The module's files are read again with
every entry, so a change you save there is picked up by the next line you
type. When that happens, variables holding values of types that aren't built
in are removed (as when you declare a type again), since the module's types
may no longer be what they were.

## Several lines

A line that opens a bracket (`{`, `(` or `[`) goes on until the bracket is
closed. The prompt is a `.` meanwhile:

```text
> for i in 0..3 {
.     print(double(i))
. }
1
3
5
> struct Point {
.     x: Int
.     y: Int
. }
```

Everything up to the closing bracket is one entry: it's checked as a whole,
and runs only if all of it is right.

## How values are shown

| The value is… | It's shown… | Example |
|---|---|---|
| a number, text, a `Bool`, a range | as `print` shows it | `3`, `Hello`, `true` |
| a list, dictionary, set or tuple of those | as `print` shows it | `[1, 2, 3]`, `(1, "one")` |
| an enum | by its case's name | `red` |
| a struct | as JSON | `{"x":1,"y":2}` |
| an optional | as JSON: the value, or `null` | `1`, `null` |
| anything else, such as a function | by its type, in parentheses | `(fn(Int) -> Int)` |

```text
> let p = Point(x: 1, y: 2)
> p
{"x":1,"y":2}
> [1, 2, 3].first
1
> let add: fn(Int) -> Int = { n in n + 1 }
> add
(fn(Int) -> Int)
```

To see a value another way, print what you want of it: `print(p.x)`.

## When something is wrong

Mistakes in the code are reported as [`tessel check`](/start/cli/#tessel-check)
reports them, and nothing of that entry runs:

```text
> let xs = [1, 2, 3]
> let n = xs.cuont
error: `[Int]` has no property `cuont`
 --> repl.2:1:12
  |
1 | let n = xs.cuont
  |            ^^^^^ unknown
  |
  = help: did you mean `count`?
```

The place is given as `repl.2:1:12`: the second entry of the session, its
first line, column 12.

A mistake that only shows when the code runs, like an index out of range or
a function that calls itself forever, stops that entry with its message.
The session goes on:

```text
> xs[5]
error: index 5 is out of range for a list of 3 items
 --> repl.3:1:1
> xs.count
3
```

After such an error, variables that the entry declared are left out, but
whatever it had already changed stays changed.

## Timers and background work

`after`, `every`, `fetch`, `request`, `runCommand` and `background` work as
in a program. The REPL waits for them to finish before it shows the next
prompt, so their output appears under the entry that started them:

```text
> after(seconds: 0.5) { print("later") }
later
>
```

(So an `every` timer that's never stopped keeps the prompt from coming back.
Stop it from its own block with [`stopTimer`](/reference/stdlib/#stoptimer).)

## Commands and keys

Commands start with a colon:

| Command | What it does |
|---|---|
| `:vars` | Lists the variables so far, with their types. |
| `:keywords` | Lists Tessel's keywords: the words you can't use as names. |
| `:help` | Shows a short summary. |
| `:quit` | Leaves. |

| Key | What it does |
|---|---|
| Up, Down | Bring back earlier entries. |
| Left, Right, Home, End | Move in the line being typed. |
| Ctrl+D | Leaves, like `:quit`. |
| Ctrl+C | At the prompt, clears the line. While code is running, ends the REPL. |

## Running lines from a file

When its input isn't the keyboard, the REPL runs the lines it's given
without showing prompts:

```sh
tessel repl < lines.txt
echo 'print(2 + 2)' | tessel repl
```

Brackets still decide where an entry ends, so a function or loop can span
lines in the file. Output goes to standard output and errors to standard
error, and the REPL carries on after an error, so this is a simple way to
script a few checks.

## What's different from a program

- **No window.** `view` and `app` aren't accepted. Put them in a file and use
  [`tessel run`](/start/cli/#tessel-run), with
  [`--hot`](/start/cli/#hot-reload) to see changes as you save.
- **No `return` or `try` at the prompt.** They need a function to return
  from, so use them inside functions. A function that returns a `Result` can
  be called at the prompt like any other.
- **Statements can be anywhere.** In a file, code that runs has to be inside
  a function; at the prompt, you just type it.
- **One file at a time isn't loaded.** There's no command to read a `.tsl`
  file into the session; put shared code in a module and `import` it.
- **Nothing is saved.** The session is gone when you leave. Copy what's worth
  keeping into a `.tsl` file.

## How it works

The REPL doesn't interpret your code. Each entry is checked and compiled to
machine code, exactly as [`tessel build`](/start/cli/#tessel-build) would,
then loaded into the running `tessel` and run there. Code runs at the same
speed as in a built program.

Your declarations are kept as source and compiled again along with each new
entry, which is why declaring a function again changes it everywhere. Your
variables live on in memory between entries.
