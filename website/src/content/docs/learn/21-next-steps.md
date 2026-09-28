---
title: 21. Where to go next
description: A look back at what you've learned, ideas for projects to build next, how to use the reference, and advice for growing as a programmer.
---

You made it. You started this course not knowing what a variable was, and
you've now written command-line tools, classic algorithms, and apps with
windows that save their data. That's real programming.

This last lesson looks back at what you've learned, suggests what to build
next, and shows you how to keep learning on your own.

## What you've learned

### Foundations (lessons 1–5)

You wrote your first programs, and met the basic material every program is
made of: **values** (numbers, text, true and false), **variables** that hold
them, **arithmetic**, **decisions** with `if` and `else`, and **loops** with
`for` and `while` that repeat work. You also learned to read Tessel's error
messages, which you'll do for as long as you program.

### Organizing code and data (lessons 6–10)

You learned to name a piece of work as a **function**, with labeled
parameters and a result, and even to make functions call themselves. You
kept many values in **lists** and **dictionaries**, worked with **text**,
and handled missing values safely with **optionals**: `if let`, `??` and
`?.`. With `readLine()`, your programs started talking to their users.

### Modeling your world (lessons 11–15)

You designed your own types: **structs** that group values together, and
**enums** that list the possible cases of something, taken apart with
`match`. You passed **functions as values** (closures) to `map`, `filter`
and `sorted`. **Interfaces** let different types share abilities, and
**generics** let one function or type work with any type.

### Building real programs (lessons 16–20)

You made programs remember things with **files** and **JSON**, and split
them into several files. You looked at **algorithms**: searching, sorting,
recursion and memos, and how to tell a fast approach from a slow one by
timing it. You built two complete projects, a **to-do list in the terminal**
and a **flashcards app**, and in between learned how **apps** work: views,
`state`, bindings, and describing what the window shows rather than
drawing it step by step.

Most importantly, you practiced the real skill underneath all of this:
breaking a problem into small steps, and turning each step into code.

## Ideas for projects

The best way to keep learning is to build things you care about. Here are
some ideas, from gentle to challenging. Pick one that sounds fun, not the
one that sounds impressive.

### Warm-ups (a few hours, in the terminal)

- **Times-table quiz.** Ask ten random multiplication questions with
  `random(…)` and `readLine()`, and give a score at the end.
- **Word counter.** Read a text file and print how many lines, words and
  characters it has, and the ten most common words (a dictionary from word
  to count, then `sorted(by:)`).
- **Unit converter.** Convert between kilometers and miles, Celsius and
  Fahrenheit, kilograms and pounds, chosen with commands like in lesson 18.
- **Hangman.** Pick a secret word, show it as `_ _ _ _`, and let the player
  guess letters until they win or run out of tries.

### Next steps (a weekend)

- **Expense tracker.** A command-line program that records what you spend,
  with a category and a date (see [Dates and time](/reference/stdlib/#dates-and-time)),
  saves it as JSON, and prints totals per category and per month.
- **Text adventure.** Rooms as structs, directions as an enum, and a
  dictionary of rooms. Type `go north`, `take lamp`, `look`.
- **Game of Life.** Conway's classic simulation in the terminal: a grid of
  cells as a list of lists, updated step by step.
- **Pomodoro timer app.** A window that counts down 25 minutes of work and
  5 of rest, using `every` (see [Timers](/ui/async/#repeating-with-every)).

### Bigger projects (a week or more)

- **Habit tracker app.** A list of habits, a tick for each day, a streak
  counter, everything saved between runs.
- **Recipe book app.** Recipes with ingredients and steps, a search field,
  and a way to scale a recipe for more people.
- **Notes app with documents.** Open and save notes wherever the user
  wants, with [file dialogs](/ui/async/#documents-and-custom-file-types),
  and several notes open in [tabs](/ui/tabs/).
- **Tic-tac-toe against the computer.** A window with a 3 by 3 grid of
  buttons, and a computer player that looks ahead at every possible move
  with recursion (an algorithm called *minimax*). It can't be beaten.
- **A weather app.** Download a forecast from a free weather service with
  [`fetch`](/ui/async/#downloading-text-with-fetch) and read the JSON
  answer with `fromJson`.

Whatever you choose, start with the smallest version that does something,
get it working, and then add one feature at a time, exactly as you did in
the project lessons.

## Using the reference

The lessons showed you the most useful parts of Tessel, but not everything.
When you want to know what else is there, or the exact details of
something, the reference pages on this site are the place to look:

- **[Standard library](/reference/stdlib/)**: every built-in function
  (`print`, `readFile`, `now`, `random` …) and every property and method of
  `Int`, `Float`, `String`, lists and dictionaries. Wondering whether a
  list can do something? Look at the [List](/reference/stdlib/#list)
  section first.
- **The language**: one page per topic, from [the basics](/language/basics/)
  to [generics](/language/generics/). They're shorter and denser than the
  lessons, and cover the corner cases the lessons skipped.
- **Apps**: [Apps and views](/ui/apps-views/),
  [State and bindings](/ui/state/), [Layout](/ui/layout/),
  [Controls](/ui/controls/) and more, with the full lists in
  [Views](/reference/views/) and [Modifiers](/reference/modifiers/).
- **[Saving data as JSON](/guides/json/)** goes deeper into what you did in
  lessons 16, 18 and 20.

Reference pages describe each function with a **signature**, like this:

```text
writeFile(path: String, text: String) -> Bool
fileSize(_ path: String) -> Int?
```

You already know how to read these. Each parameter has a name and a type.
`_` means you pass that argument without a label, so the second one is
called as `fileSize("notes.txt")`. After `->` comes the type of the result,
and `?` tells you it can be `nil`, so you'll need `if let` or `??`. A
parameter with `= value` has a default and can be left out.

Don't read the reference from top to bottom. Search it when you need
something, try the examples, and change them to see what happens.

## The Tessel IDE

You can write Tessel in any text editor, but Tessel also comes with its
own [IDE](/start/ide/) (Integrated Development Environment): an editor, a
file explorer, and buttons to check, run and build, in one window. Open it
on a folder with:

```sh
tessel ide flashcards
```

It marks errors in your code as you save, suggests names as you type, and
shows your program's output in a console. It's written in Tessel itself.
If you prefer VS Code, there's an [extension](/start/editors/) with the
same error checking and completion.

## Testing your apps

In lessons 19 and 20 you ran apps headless, with `TESSEL_SCRIPT`, and read
the view tree they printed. That's more than a trick for checking lessons:
it's a way to write **tests**. Keep a script and the output you expect,
and compare them after every change:

```sh
TESSEL_SCRIPT="$(cat flashcards.script)" tessel run flashcards > actual.txt
diff expected.txt actual.txt
```

If `diff` prints nothing, everything still works. For command-line programs,
the same idea works with a file of input, as in lesson 18. Tests like these
let you change a program with confidence: if you break something, you find
out in seconds, not weeks later. [Testing your UI](/ui/testing/) has every
script command, including `snapshot` for screenshots.

## A word about speed

Tessel compiles your programs to native machine code, the same kind of
code a C or Swift compiler makes. That means Tessel programs are fast.

The Tessel repository has a `bench/` folder with the same seven small
programs written in Tessel, C, Go, Swift and Python: recursive function
calls, a prime-number sieve, floating-point math, structs, text,
dictionaries and sorting. On one Mac, the times were:

| Program | C | Go | Swift | Tessel | Python |
|---|---|---|---|---|---|
| fib (function calls) | 60 ms | 93 ms | 96 ms | 96 ms | 2304 ms |
| sieve (list items) | 41 ms | 49 ms | 82 ms | 87 ms | 4952 ms |
| mandel (math) | 42 ms | 46 ms | 49 ms | 48 ms | 5569 ms |
| nbody (structs) | 32 ms | 34 ms | 110 ms | 57 ms | 5737 ms |
| strings (text) | 53 ms | 68 ms | 181 ms | 70 ms | 135 ms |
| dict (dictionaries) | 79 ms | 78 ms | 77 ms | 65 ms | 196 ms |
| sort (sorting) | 69 ms | 62 ms | 55 ms | 58 ms | 311 ms |

Lower is faster. As always with timings, the exact numbers depend on the
computer and will change as the languages improve. The pattern is what
matters: Tessel is in the same league as Go and Swift. Python is
about 25 to more than 100 times slower on the programs that do lots of small steps
(function calls, loops, math), and closer on text, dictionaries and
sorting, where most of its work happens inside fast built-in code. You can
run the comparison yourself with `python3 bench/run.py`.

But remember lesson 17: a better algorithm matters far more than a faster
language. A good algorithm in Python beats a bad one in C. Write clear code
first, measure when something feels slow, and fix the part that's actually
slow.

## Advice for the road

A few habits make the difference between struggling and making progress.
They're the same for beginners and for people who have programmed for
decades.

- **Read the error message.** All of it, slowly. Tessel's errors say where
  the problem is and usually how to fix it. When there are several, fix the
  first one first: the others are often caused by it.
- **Take small steps.** Write a few lines, run them, check they do what you
  expect, then write a few more. When something breaks, it's in the few
  lines you just wrote.
- **Print things.** When a program doesn't do what you expect, add `print`
  calls to see what the values really are. Your guess about what's
  happening is often wrong; the printout never is.
- **Try it out.** Wondering what `"a,,b".split(",")` gives? Don't guess:
  write a three-line program and see. Experiments are cheap.
- **Get stuck, and then get unstuck.** Being stuck is normal, not a sign
  that you're bad at this. Take a break, explain the problem out loud (to a
  person, or a rubber duck), or make the problem smaller.
- **Read other people's code.** The examples in the reference, the
  `examples/` folder in the Tessel repository, and the IDE's own source in
  `ide/` show how others solve problems.
- **Practice.** Programming is learned by doing, like a language or an
  instrument. A little every day beats a lot once a month.
- **Build what you want to exist.** The projects you care about are the
  ones you'll finish, and finishing things is how you get good.

What you've learned carries over to other languages too. Swift, Kotlin,
Rust, TypeScript, Python and Go all have variables, functions, lists,
structs, optionals or something like them. The second language is much
easier than the first.

Happy programming!
