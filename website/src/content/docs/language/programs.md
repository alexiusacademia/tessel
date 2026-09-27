---
title: Programs and files
description: How a Tessel program is organized - a single .tsl file or a folder of files sharing one namespace, with an app or fn main as its entry point.
---

A Tessel program is either **one `.tsl` file**, or **a folder of `.tsl`
files**. There are no import statements, packages or modules to set up: all
the files in the folder form one program.

## One file

The simplest program is a single file:

```tessel
// hello.tsl
fn main() {
    print("Hello!")
}
```

```sh
tessel run hello.tsl
```

## A folder of files

As a program grows, split it into several files in one folder:

```text
todo-app/
  main.tsl
  todo.tsl
```

```tessel
// todo.tsl
struct Todo {
    title: String
    done: Bool = false
}

fn describe(todo: Todo) -> String {
    let mark = if todo.done { "x" } else { " " }
    "[{mark}] {todo.title}"
}
```

```tessel
// main.tsl
fn main() {
    let todo = Todo(title: "Write docs")
    print(describe(todo: todo))
}
```

Pass the folder to `tessel`:

```sh
tessel run todo-app
```

```text
[ ] Write docs
```

`main.tsl` uses `Todo` and `describe` from `todo.tsl` without importing
anything. Every file in the folder sees every declaration in every other
file, and the order of the files and of the declarations in them doesn't
matter. The file names are up to you; `main.tsl` is just a convention.

Only files ending in `.tsl` directly inside the folder are part of the
program. Files in subfolders are not included.

:::caution
If you run a single file from a multi-file program, like
`tessel run todo-app/main.tsl`, only that file is compiled, so names from the
other files will be missing. Run the folder instead.
:::

## What goes at the top level

A file contains only declarations:

| Declaration | For |
|---|---|
| `fn` | a function, see [Functions](/language/functions/) |
| `struct` | a struct type, see [Structs and enums](/language/structs-enums/) |
| `enum` | an enum type, see [Structs and enums](/language/structs-enums/) |
| `view` | a piece of user interface, see [Apps and views](/ui/apps-views/) |
| `app` | the entry point of a program with a window |

Statements and variables live inside these. A `let` or `var` at the top
level of a file is an error; for a value the whole program needs, write a
function that returns it.

## Names are shared

Because all files share one namespace, each top-level name must be unique
across the whole program. Declaring the same name twice, even in different
files, is an error that points at both places:

```text
error: `describe` is declared more than once
  = help: names must be unique across all files in the program
```

Built-in names such as `Int`, `String`, `Text`, `Button` and `print` can't
be redeclared either.

Names inside a struct or enum (fields, cases and methods) belong to that
type, so two types can both have a method called `describe` without any
conflict.

## The entry point

Every program has exactly one place where it starts: either an `app` or a
`fn main()`.

- **`fn main()`** is for programs without a window, such as command-line
  tools, scripts and tests. It runs from top to bottom, and the program ends
  when it returns. `main` takes no parameters and returns nothing.
- **`app`** is for programs with a window. It declares the main window and
  what it shows, and the program keeps running until the window is closed:

```tessel
app Hello(title: "Hello", width: 320, height: 200) {
    Text("Hello from Tessel")
        .padding(24)
}
```

A program can't have both. With neither, `tessel check` reports
`this program has no entry point`. Building apps is covered in
[Your first app](/start/first-app/) and [Apps and views](/ui/apps-views/).

## Checking, running and building

The `tessel` command takes a file or a folder:

```sh
tessel check todo-app          # find errors without building
tessel run todo-app            # build and run
tessel build todo-app -o todo  # build a native executable called todo
```

`tessel check` type-checks the whole program and lists every error with its
file, line and column. `tessel run` compiles the program to native code and
starts it. When the program ends normally, `tessel run` exits with status 0;
after a [runtime error](/language/optionals/#runtime-errors), it exits with
status 101.

See [Command line](/start/cli/) for all the options.
