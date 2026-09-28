---
title: "1. Your first program"
description: Write, run and fix your very first Tessel program, and learn what a program and a compiler are.
---

Welcome! This course teaches you to program from the very beginning. You
don't need to know anything about programming to start. You only need a
computer and a little patience.

In this lesson you'll learn:

- what programming is, and what a program is
- how to install Tessel
- how to write a program that says "Hello, world!" and run it
- what "compiling" means
- how to leave notes in your code with comments
- how to read an error message and fix your mistake

## What is programming?

A computer is very fast, but it doesn't think for itself. It does exactly what
it is told, step by step. **Programming** is writing down those steps.

A **program** is a list of instructions for the computer, written in a
language the computer can understand (with a bit of help). The instructions
are written as plain text, called **code**. The language you'll use in this
course is **Tessel**.

Every app you have ever used, from a calculator to a game, is a program
someone wrote this way. The programs in this course start small: they print
a few lines of text. Step by step, they'll grow into real tools and, later,
apps with windows and buttons.

## Installing Tessel

To run Tessel programs, you need the `tessel` command on your computer. Follow
the steps on the [Installation](/start/installation/) page. It's one command
to copy and paste into a terminal.

A **terminal** is a window where you type commands instead of clicking
buttons. On macOS it's the app called Terminal; on Windows, use PowerShell.

When Tessel is installed, check that it works by typing this in a new
terminal window and pressing Enter:

```sh
tessel --version
```

It prints the version number, something like:

```text
tessel 0.1.5
```

If you'd rather not use a terminal at all, skip ahead to
[The Tessel IDE](#the-tessel-ide) below.

## Hello, world!

It's a tradition to make your first program say "Hello, world!". Let's keep
the tradition.

Open a text editor (TextEdit on macOS in plain text mode, Notepad on Windows,
or any code editor you like). Type this, exactly as shown:

```tessel
fn main() {
    print("Hello, world!")
}
```

Save it as a file called `hello.tsl`. Tessel files always end in `.tsl`.

Now, in your terminal, go to the folder where you saved the file and type:

```sh
tessel run hello.tsl
```

You should see:

```text
Hello, world!
```

Congratulations: you just wrote and ran a program!

Let's look at what each part means:

- `fn main() { … }` is the **main function**: the place where your program
  starts. `fn` is short for "function", which is a named group of
  instructions. Everything between the braces `{` and `}` belongs to `main`.
  When you run the program, Tessel runs the instructions inside `main`.
- `print(…)` is an instruction that shows something in the terminal. What to
  show goes between the parentheses.
- `"Hello, world!"` is a piece of text. Text in a program goes between
  double quotes, so the computer knows where it begins and ends. Programmers
  call a piece of text a **string**.

The instructions inside `main` are indented by four spaces. Tessel doesn't
require that, but it makes it easy to see what belongs inside the braces.
Every program in this course is written that way.

## What does "compiling" mean?

Your computer doesn't read Tessel directly. Deep down, it only understands
very simple instructions written as numbers, called **machine code**. Writing
machine code by hand would be slow and painful.

So Tessel comes with a **compiler**: a program that translates your Tessel
code into machine code. That translation is called **compiling**.

When you type `tessel run hello.tsl`, two things happen:

1. Tessel compiles `hello.tsl` into machine code.
2. Then it runs that machine code, and you see the result.

Before translating, the compiler checks your code carefully. If something
doesn't make sense, it stops and tells you what's wrong, instead of running a
broken program. You'll see that in action at the end of this lesson.

If you only want the check, without running anything, use `tessel check`:

```sh
tessel check hello.tsl
```

```text
ok: 1 file checked, no errors
```

## One instruction after another

A program can have many instructions. Each one goes on its own line, and they
run **from top to bottom**, one at a time:

```tessel
fn main() {
    print("Good morning!")
    print("Time to learn Tessel.")
    print("See you soon.")
}
```

```text
Good morning!
Time to learn Tessel.
See you soon.
```

Each instruction is called a **statement**. Each `print` shows its text on a
new line. If you swap two lines in your code, the two lines of output swap
too. The computer never skips ahead or goes back unless you tell it to (you'll
learn how in later lessons).

Tessel wants one statement per line. Putting two on the same line is an
error:

```tessel
fn main() {
    print("Hello") print("World")
}
```

```text
error: expected end of line, found `print`
 --> hello.tsl:2:20
  |
2 |     print("Hello") print("World")
  |                    ^^^^^
  |
  = help: put each statement on its own line

1 error found
```

## Comments: notes for humans

Sometimes you want to write a note in your code, for yourself or for someone
else who reads it. A **comment** starts with `//`. Tessel ignores everything
from `//` to the end of the line.

```tessel
// This program prints a short poem.
// Lines that start with // are comments. Tessel skips them.
fn main() {
    print("Roses are red,")
    print("Violets are blue,")
    // print("This line is switched off.")
    print("Tessel is new.") // a comment can go after code too
}
```

```text
Roses are red,
Violets are blue,
Tessel is new.
```

Comments are handy in two ways:

- **To explain why** your code does something. Code says *what* happens; a
  good comment says *why*.
- **To switch a line off** for a moment, without deleting it. This is called
  "commenting out" a line. Remove the `//` to switch it back on.

## The Tessel IDE

An **IDE** (integrated development environment) is an app for writing
programs. It puts an editor, a Run button and the program's output together in
one window. Tessel comes with its own IDE, so you don't need a terminal or a
separate editor.

To open it, type `tessel` in a terminal, in the folder you want to work in.
Tessel treats that folder as one program, so give each program its own
folder: for example, a folder `hello` with a file `main.tsl` inside it.
(Two files in the same folder that both have a `fn main()` are an error.)

Type your code, then press **Run** (Cmd+R on macOS, Ctrl+R on Windows). The
output appears in the **Console** at the bottom. Any errors show up under
**Problems**, and clicking one jumps to the line.

See [The Tessel IDE](/start/ide/) for a full tour. Everything in this course
works in the IDE and in the terminal alike; the lessons show terminal
commands because they're short to write down.

## Reading an error message

Everyone makes mistakes when programming, including people who have done it
for decades. What matters is knowing how to read the error message and fix
the problem. Let's make a mistake on purpose.

Change `print` to `prnt` in `hello.tsl`:

```tessel
fn main() {
    prnt("Hello, world!")
}
```

Run it again with `tessel run hello.tsl`. Tessel refuses, and prints:

```text
error: cannot find `prnt`
 --> hello.tsl:2:5
  |
2 |     prnt("Hello, world!")
  |     ^^^^ not found
  |
  = help: did you mean `print`?

1 error found
```

Here's how to read it, line by line:

- ``error: cannot find `prnt` `` says **what** is wrong. Tessel doesn't know
  anything called `prnt`.
- `--> hello.tsl:2:5` says **where**: in the file `hello.tsl`, on line 2,
  column 5 (the 5th character of the line).
- The next lines show that line of your code, with `^^^^` under the exact
  spot.
- `help:` is a suggestion for fixing it. Here Tessel guessed right: you meant
  `print`.

Fix the typo, run it again, and "Hello, world!" is back.

Notice that the program didn't run at all. The compiler found the problem
first. That's a good thing: it's much easier to fix a mistake the compiler
points at than to hunt down a program that runs and does the wrong thing.

### Common mistake: a missing quote

Text must start *and* end with a double quote. Forget the closing one:

```tessel
fn main() {
    print("Hello, world!)
}
```

```text
error: this string is missing its closing `"`
 --> hello.tsl:2:11
  |
2 |     print("Hello, world!)
  |           ^^^^^^^^^^^^^^^ string starts here
  |
  = help: strings must end on the same line they start

error: expected `,` or `)`, found `}`
 --> hello.tsl:3:1
  |
3 | }
  | ^

2 errors found
```

There are two errors, but only one mistake. Because the string never ended,
Tessel also got confused about the `)` (it thinks the `)` is part of the
text). This happens often: one mistake can cause more errors further down.

**Tip:** always fix the *first* error first, then run the program again.
Often the other errors disappear by themselves.

### Common mistake: capital letters

Tessel cares about capital letters. `print` and `Print` are two different
names, and only `print` exists:

```text
error: cannot find `Print`
 --> hello.tsl:2:5
  |
2 |     Print("Hello, world!")
  |     ^^^^^ not found
  |
  = help: did you mean `print`?

1 error found
```

## Exercises

Try each exercise yourself before you open the solution. There is often more
than one right answer.

**1. Say hello to yourself.** Write a program that prints two lines: a hello
with your own name, and then "Nice to meet you."

<details>
<summary>Solution</summary>

```tessel
fn main() {
    print("Hello, Sam!")
    print("Nice to meet you.")
}
```

```text
Hello, Sam!
Nice to meet you.
```

</details>

**2. Draw a tree.** Use several `print` statements to draw a small tree out
of `*` characters. Spaces inside the quotes count, so you can use them to move
the stars to the right.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    print("    *")
    print("   ***")
    print("  *****")
    print(" *******")
    print("    |")
}
```

```text
    *
   ***
  *****
 *******
    |
```

</details>

**3. Predict the output.** Without running it, write down what this program
prints. Then run it to check.

```tessel
fn main() {
    print("one")
    // print("two")
    print("three") // print("four")
    print("five")
}
```

<details>
<summary>Solution</summary>

```text
one
three
five
```

The line with `"two"` is commented out. On the `"three"` line, everything
after `//` is a comment, so `print("four")` never runs.

</details>

**4. Fix the program.** This program has two mistakes. Run it, read the error,
fix it, and repeat until it works.

```tessel
fn main() {
    print("Tessel is fun"
    Print("Let's learn more.")
}
```

<details>
<summary>Solution</summary>

The first run says:

```text
error: expected `,` or `)`, found `Print`
 --> fix.tsl:3:5
  |
3 |     Print("Let's learn more.")
  |     ^^^^^

1 error found
```

The `print` on line 2 never got its closing `)`, so Tessel was still waiting
for it when it reached line 3. Add the `)` and run again. Now Tessel reports
the second mistake, ``cannot find `Print` ``, with the hint
``did you mean `print`?``. Fix that too:

```tessel
fn main() {
    print("Tessel is fun")
    print("Let's learn more.")
}
```

```text
Tessel is fun
Let's learn more.
```

Notice that Tessel pointed at line 3, even though the mistake was at the end
of line 2. When an error doesn't seem to make sense, look at the line just
before it too.

</details>

## Summary

- A **program** is a list of instructions for the computer, written as text
  (code). Tessel files end in `.tsl`.
- A Tessel program starts at `fn main() { … }`. The statements inside run from
  top to bottom, one per line.
- `print("…")` shows text in the terminal. Text goes between double quotes.
- The **compiler** checks your code and translates it into machine code.
  `tessel run file.tsl` compiles and runs; `tessel check file.tsl` only checks.
- `//` starts a comment, which Tessel ignores.
- An error message tells you what is wrong, where (file, line, column), and
  often how to fix it. Fix the first error first.

**Next:** [2. Values and variables](/learn/02-values-variables/)
