---
title: Tessel's design
description: The goals behind Tessel and the ten design decisions that shape its syntax and semantics, with the reasons for each.
---

Tessel is meant to be a small language that is pleasant for building
desktop apps. This page explains the goals it was designed around and the
main decisions that follow from them. Knowing the reasons makes the rules
easier to remember.

## Goals

- **Declarative UI.** A view is a description of what the screen shows for
  the current state, in the style of SwiftUI. You don't create widgets by
  hand or write code to keep the screen in sync with your data.
- **Little syntax.** Tessel has 18 keywords, one kind of block, no
  semicolons, no imports and no special symbols in names.
- **Easy to learn.** Familiar C-style operators, types that are inferred
  almost everywhere, and error messages that point at your code and say how
  to fix it.
- **Fast.** Programs are compiled ahead of time to native code with LLVM.
  There's no garbage collector: memory is reference counted, so there are no
  collection pauses.
- **Cross-platform.** The same source builds a native app for macOS and
  Windows. Tessel draws every control itself, so apps look the same on both.

Some things are deliberately left out, at least for now: mobile and web
targets, native operating-system widgets, pointers and references, threads,
inheritance, macros and operator overloading.

The 18 keywords are:

```
app  view  struct  enum  fn
let  var  state  bind
if  else  for  in  match  return
true  false  nil
```

## The decisions

### 1. One kind of block

A block in braces after a call can be an action to run later, the child
views of a container, or a closure. All three look the same:

```tessel
Button("Add") { add() }                 // an action
VStack { Text("Hi") }                   // child views
todos.filter { t in !t.done }           // a closure with a parameter
```

What a block means is decided by the type of the parameter it's passed to,
not by different syntax. There's one thing to learn instead of three.

### 2. `&&`, `||` and `!` for logic

```tessel
if age >= 18 && member {
    "full member"
} else if age < 18 || !member {
    "guest"
}
```

Tessel uses the symbols most languages use, rather than words like `and`
and `or`. They're familiar to anyone who has seen C, Java, JavaScript or
Swift, and they keep the keyword list short.

### 3. `{expr}` for string interpolation

```tessel
Text("Count: {count}")
print("Use \{name} to show a value")
```

Anything inside `{ }` in a string is evaluated and inserted. Since text in
apps constantly shows values, the shortest possible syntax won. To write a
literal brace, escape it: `\{`.

### 4. No marker for bindings at the call site

```tessel
TextField("Name", text: draft)          // text: is a `bind String`
TodoRow(todo: todo)                     // todo: is a `bind Todo`
```

When a view declares a parameter as `bind`, the caller passes a variable
it owns, and the view can read and change it. The call looks like any other
call: there's no `&` or `$`. The parameter's declaration already says it's a
binding, and passing something that can't be changed is a compile error that
explains why, so a marker would only add noise. See
[State and bindings](/ui/state/).

### 5. Labelled arguments

```tessel
area(width: 2.0, height: 3.0)
```

Arguments are passed by name, so a call reads clearly without looking up
the function, and arguments can't be mixed up. The label may be left out
for the first argument when the function has only one parameter, or when
the parameter is declared with `_`:

```tessel
fn greet(_ name: String) {
    print("Hello, {name}!")
}

greet("Ada")
```

That's why built-ins read naturally: `Text("Hello")`, `Button("Add") { … }`.
See [Functions](/language/functions/).

### 6. Value types only

```tessel
let a = [1, 2, 3]
var b = a
b.append(4)
print("a has {a.count} items, b has {b.count}")
```

```
a has 3 items, b has 4
```

Every value, including lists, dictionaries, strings and structs, behaves
like a number: assigning it or passing it gives the receiver its own copy.
Nothing can be changed "from somewhere else", which makes programs easier to
follow. Copies are cheap because the actual copying only happens when one
of the copies is changed (copy-on-write).

This also keeps memory management simple. Without shared references,
values can't form cycles, so plain reference counting frees everything,
without a garbage collector. When a view needs to change its caller's data,
it uses a `bind` parameter instead of a reference. See
[Values and copying](/language/values/).

### 7. No imports

A program is a folder. Every `.tsl` file in it shares one namespace, so a
struct declared in `models.tsl` can be used in `views.tsl` without any
`import` line. For small and medium apps, which is what Tessel is for, this
removes a whole category of busywork. Names must be unique across the
folder. See [Programs and files](/language/programs/).

### 8. Optionals for missing values

```tessel
let city = user?.address.city ?? "Unknown"

if let value = Int("42") {
    print("got {value}")
}
```

A value that may be missing has an optional type, like `User?`, and is
either a value or `nil`. You have to check it before using it, with
`if let`, `?.` or `??`. There's no force-unwrap operator, so a missing value
can't crash your program. See [Optionals and errors](/language/optionals/).

### 9. Errors are optionals

```tessel
if let text = readFile(path: "notes.txt") {
    print(text)
} else {
    print("Could not load the file")
}
```

A function that can fail returns an optional, and `nil` means it failed.
This reuses the optional machinery instead of adding exceptions or a
separate error type. The downside is that `nil` says *that* something
failed, not *why*; a richer error type may come later.

Mistakes in the program itself, such as an index out of range, integer
overflow or division by zero, are different: they stop the program with a
message and the place in your code.

### 10. Callbacks for slow work

```tessel
Button("Refresh") {
    fetch(url: "https://example.com/weather") { text in
        weather = text ?? "Could not load"
    }
}
```

Slow work, such as network requests, timers and running other programs,
is done by built-in functions that take a completion block. The block always
runs on the UI thread, so it can change `state` directly, and the view
updates. There are no threads, locks or `async` keywords to learn.

Heavy computations use the same shape: `background(work: { … }) { result in … }`
runs the work on another thread and hands the result to a block on the UI
thread. The work gets *copies* of the values it uses, the same way values
are saved with `toBinary`, so the two threads never share anything and
nothing needs a lock. The compiler checks that the work doesn't touch
`state` or the app's own functions.
See [Files, commands and the network](/ui/async/).

## Still open

A few questions are not settled yet:

- **Error reasons.** If optionals turn out to be too limiting, a
  `Result`-style type that carries a reason may be added.
- **Styling and themes.** Whether themes should be a value passed down the
  view tree, or stay modifiers only.

See the [roadmap](/about/roadmap/) for what's being worked on.
