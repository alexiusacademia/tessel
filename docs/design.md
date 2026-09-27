# Tessel — Language Design (v0.1 draft)

Tessel is a compiled, statically typed language for building desktop GUI apps
on macOS and Windows. You describe *what* the UI looks like for the current
state; the runtime works out what changed and redraws it.

Status: **draft for review.** Nothing here is final until the parser exists.

## 1. Goals

| Goal | What it means in practice |
|---|---|
| **Declarative UI** | Views are expressions of state, in the style of SwiftUI. No manual widget creation, no callbacks to keep the screen in sync. |
| **Few syntax** | 18 keywords. One block syntax `{ }` for everything. No semicolons, no imports, no sigils. |
| **Easy to learn** | Familiar C-family operators. Types are inferred almost everywhere. Every error message points at the code and says how to fix it. |
| **Fast** | Compiled ahead of time to native code via LLVM. No garbage collector: memory is reference counted with copy-on-write, so there are no pauses during animation. |
| **Cross-platform** | One source tree builds a native `.app` on macOS and `.exe` on Windows. The runtime draws every widget itself, so apps look identical on both. |

**Non-goals for v1:** mobile and web targets, native OS widgets, user-visible
pointers or references, threads, inheritance, macros, operator overloading.

## 2. A first look

```tessel
app Counter(width: 320, height: 200) {
    state count = 0

    VStack(spacing: 12) {
        Text("Count: {count}")
            .font(size: 32, weight: .bold)

        HStack(spacing: 8) {
            Button("-") { count -= 1 }
            Button("+") { count += 1 }
        }
    }
    .padding(24)
}
```

`state count` is part of the view. Pressing a button changes `count`, and
anything that reads `count` is redrawn. That is the whole reactivity model.

A larger example using structs, enums, loops and bindings is in
[`examples/todos/todos.tsl`](../examples/todos/todos.tsl).

## 3. Lexical structure

- **Source files** are UTF-8 and end in `.tsl`.
- **Comments:** `// to end of line`. There are no block comments.
- **Statements end at a newline.** A line that starts with `.` continues the
  previous expression, which is how modifier chains span lines. A line also
  continues while a `(` or `[` is open.
- **Identifiers:** letters, digits and `_`, not starting with a digit.
  Convention: `Types` and `Views` in UpperCamelCase, everything else in
  lowerCamelCase.
- **Keywords (18):**

  ```
  app  view  struct  enum  fn
  let  var  state  bind
  if  else  for  in  match  return
  true  false  nil
  ```

- **Literals:** `42`, `1_000`, `3.14`, `true`, `"text"`.
- **Strings** interpolate with `{expr}`: `"Hello, {user.name}!"`. Escapes are
  `\n \r \t \" \\ \{`.

## 4. Types

| Type | Examples | Notes |
|---|---|---|
| `Int` | `0`, `-7`, `1_000` | 64-bit signed. Overflow is a runtime error. |
| `Float` | `0.5`, `2.0` | 64-bit. No implicit Int↔Float conversion: write `Float(n)` or `Int(x)`. |
| `Bool` | `true`, `false` | |
| `String` | `"hi {name}"` | Immutable UTF-8 text. |
| `[T]` | `[1, 2, 3]`, `[]` | Growable list. |
| `[K: V]` | `["a": 1]`, `[:]` | Dictionary. |
| `T?` | `nil`, `5` | Optional: a value or `nil`. See §7.1. |
| `fn(A, B) -> R` | `{ a, b in a + b }` | Function / closure. `fn()` takes nothing and returns nothing. |
| `View` | `Text("x")` | Anything that can be drawn. |
| `Color`, `Font`, … | `.red`, `.bold` | Built-in UI value types. |
| structs, enums | `Todo(title: "x")`, `.active` | User-defined, see §6. |

**All values are value types.** Assigning or passing a struct or list gives
the receiver its own copy; there is no shared mutable data. Copies are cheap
because lists, dictionaries and strings are copy-on-write. This rules out
reference cycles, which is what lets plain reference counting manage all
memory without a garbage collector.

**Number literals** take their type from context: `12` is a `Float` where a
`Float` is expected, and `1 + 2.5` is a `Float`. Variables never convert
implicitly: if `count` is an `Int`, `count + 2.5` is an error.

**Converting text to numbers** can fail, so `Int("42")` and `Float("1.5")`
return optionals (`Int?`, `Float?`).

**Inference:** variable types are inferred from their initial value. Function
parameters and non-`View` return types are written out. An empty literal
needs a type: `state todos: [Todo] = []`.

**Optional chaining** short-circuits the whole chain, as in Swift: in
`user?.address.city`, if `user` is `nil` the result is `nil`, and `.city`
needs no second `?`.

**Leading-dot shorthand:** when the expected type is known, `.name` refers to
a case or constant of that type: `.bold`, `.red`, `Filter.all` → `.all`.

## 5. Variables and functions

```tessel
let name = "Tessel"      // constant
var total = 0            // mutable local
total += 5

fn area(width: Float, height: Float) -> Float {
    width * height        // the last expression is the result
}

fn greet(name: String) {  // no `->` means it returns nothing
    print("Hello, {name}")
}

area(width: 2.0, height: 3.0)   // arguments are always labelled
```

- **Labelled arguments.** Arguments are passed by label. The first argument
  of a call may drop its label when the function has only one parameter, or
  when the parameter is declared `_ name: Type`. Built-ins like `Text("x")`
  and `Button("Go") { … }` use this.
- **`return`** exits early. It is only needed before the last line.
- **Closures** are `{ params in body }`; a closure with no parameters is just
  `{ body }`: `todos.filter { t in !t.done }`.
- **Trailing block:** if the last parameter is a function or view content, the
  block may follow the call: `Button("Add") { add() }`, `VStack { … }`.
  Whether the block is an *action* or *children* is decided by the parameter's
  type, not by different syntax.

### Operators (lowest to highest precedence)

| Operators | |
|---|---|
| `=  +=  -=  *=  /=` | assignment (statement only, not an expression) |
| `\|\|` | logical or |
| `&&` | logical and |
| `==  !=  <  <=  >  >=` | non-associative |
| `??` | default for a missing optional: `name ?? "Guest"` |
| `+  -` | `+` also joins strings and lists |
| `*  /  %` | |
| `!  -` | prefix not, prefix negation |
| `.  ?.  ()  []` | member, optional member, call, index |

## 6. Structs and enums

```tessel
struct Todo {
    title: String
    done: Bool = false          // fields may have defaults
}

let t = Todo(title: "Write spec")
t.done                          // false

enum Filter { all, active, done }

enum Shape {
    circle(radius: Float)
    rect(width: Float, height: Float)
}
```

Structs and enums may declare methods with `fn` inside their body. Inside a
method, fields are available by name and `self` is the whole value (so an enum
method can `match self { … }`). `self` is not a keyword, just a name that
exists inside methods. Methods that change fields may only be called on a
`var`, `state` or `bind`.

## 7. Control flow

`if`, `match` and `for` work the same inside functions and inside views.

```tessel
if score > 90 { "A" } else if score > 80 { "B" } else { "C" }

match shape {
    .circle(r) -> 3.14159 * r * r
    .rect(w, h) -> w * h
}

for item in items { print(item) }
for i in 0..10 { … }           // half-open range
```

- `if` and `match` are expressions: they produce a value when every branch
  does. `match` must cover every case.
- Inside a view, `if` shows or hides views, and `for` produces one view per
  item.

### 7.1 Optionals

A value of type `T?` is either a `T` or `nil`. It must be unwrapped before
use, so a missing value can never crash the program.

```tessel
state selected: Todo? = nil

if let todo = selected {
    Text(todo.title)          // todo is a plain Todo here
} else {
    Text("Nothing selected")
}

let name = user?.name ?? "Guest"   // ?. gives nil if user is nil
```

There is no force-unwrap operator.

### 7.2 Errors

Functions that can fail return an optional: `nil` means it failed.

```tessel
fn load(path: String) -> String? { … }

if let text = load(path: "notes.txt") {
    show(text)
} else {
    print("Could not load notes")
}
```

Programmer mistakes (out-of-range index, overflow, division by zero) are not
optionals; they stop the program with a message (§9).

## 8. Views and state

### 8.1 Declaring views

```tessel
view TodoRow(todo: bind Todo) {
    HStack(spacing: 8) {
        Toggle(isOn: todo.done)
        Text(todo.title)
        Spacer()
    }
}
```

A `view` is a function from its parameters and state to a tree of views. Its
body is a list of view expressions plus any number of `state` declarations
and `fn` helpers (which may read and change state).

`app Name(width:, height:, title:) { … }` declares the program's entry point:
a view that also owns the main window.

A program without a UI (a command-line tool, or a test) uses `fn main() { … }`
instead. A program has exactly one entry point: an `app` or a `fn main()`.

### 8.2 State

`state x = value` is storage that belongs to one instance of a view. It lives
as long as the view stays on screen. Writing to it schedules a redraw of that
view; the runtime rebuilds the view's tree, diffs it against the previous
one and updates only what changed.

### 8.3 Bindings

A parameter declared `bind T` receives a *place* the view can read and write,
not a copy. The caller passes any assignable place — a `state`, a `var`, a
field of one, or a list element — **without extra syntax**:

```tessel
TextField("Name", text: draft)      // TextField's `text` is `bind String`
TodoRow(todo: todo)                 // todo is an element of a state list
```

When a `for` loop iterates over a place (a `state`, `var` or `bind` list), its
loop variable is also a place, so it can be passed on to `bind` parameters.
Passing a non-place (such as `"x"` or a function result) to a `bind`
parameter is a compile error that says why.

### 8.4 Modifiers

Modifiers are methods on `View` that return a new view:

```tessel
Text("Hi")
    .font(size: 18, weight: .semibold)
    .color(.secondary)
    .padding(8)
    .background(.blue, radius: 6)
```

### 8.5 Built-in views (v1)

| View | Signature (simplified) |
|---|---|
| `Text` | `Text(_ text: String)` |
| `Button` | `Button(_ label: String, action: fn())` |
| `TextField` | `TextField(_ placeholder: String, text: bind String)` |
| `Toggle` | `Toggle(isOn: bind Bool)` |
| `Picker` | `Picker(selection: bind T, content: View)` with `.tag(value)` on options |
| `VStack` / `HStack` / `ZStack` | `(spacing: Float = 8, content: View)` |
| `Spacer` | `Spacer()` — takes the remaining space in a stack |
| `Scroll` | `Scroll(content: View)` |
| `Image` | `Image(_ path: String)` |

Common modifiers: `padding`, `frame(width:height:)`, `font`, `color`,
`background`, `border`, `opacity`, `hidden`, `disabled`, `onTap`, `id`.

### 8.6 Identity

When a `for` loop's list changes, views are matched to items by position. Use
`.id(item.id)` to match by a stable key instead, so state inside rows (for
example a text field being edited) follows its item when the list reorders.

## 9. Programs, files and the standard library

- A **program** is a folder. Every `.tsl` file in it shares one namespace, so
  there are no imports. Names must be unique across the folder.
- **Standard library (v1):** `print`, `String` methods (`count`, `contains`,
  `split`, `trim`, `uppercase`, …), `List` methods (`append`, `remove(at:)`,
  `count`, `filter`, `map`, `sorted`, `contains`, …), `Dictionary` methods,
  and `Int`/`Float` math.
- **Runtime errors** (out-of-range index, integer overflow, division by zero)
  stop the program with the source location and a message.
- **Slow work** (network, timers, large files) runs in the background through
  built-ins that take a completion block. The block always runs on the UI
  thread, so it may write to `state` directly:

  ```tessel
  Button("Refresh") {
      fetch(url: "https://example.com/weather") { text in
          weather = text ?? "Could not load"
      }
  }
  after(seconds: 2) { showHint = true }
  ```

## 10. Implementation architecture

```
 .tsl files ─▶ lexer ─▶ parser ─▶ AST ─▶ type checker ─▶ typed IR ─▶ LLVM IR ─▶ object file ─┐
                                                                                          ├─ link ─▶ .app / .exe
                               tessel-runtime (static library, Rust) ─────────────────────┘
```

| Crate | Role |
|---|---|
| `tessel-compiler` | Lexer, parser, type checker, lowering and LLVM code generation (via `inkwell`). |
| `tessel-runtime` | Windows and input (`winit`), GPU (`wgpu`: Metal on macOS, DirectX 12 on Windows), 2D drawing (`vello`), text (`parley`), layout, the view tree and diffing, reference counting, strings and lists. Exposes a C ABI. |
| `tessel-cli` | The `tessel` command: `check`, `build`, `run`. |

### 10.1 How a view compiles

Each `view` becomes:

1. a **state record** holding its `state` fields;
2. a **build function** that, given the parameters and the state record,
   calls the runtime to describe the current tree (`tsl_begin(VSTACK)`,
   `tsl_text(str)`, `tsl_end()`, …);
3. one **closure** per action block, which may write to state.

When an action writes to state, the runtime marks that view instance dirty.
Before the next frame it re-runs the dirty build functions, diffs the new
tree against the old one, re-runs layout for the changed subtrees and repaints
only the damaged region.

### 10.2 Memory

Strings, lists, dictionaries, closures and state records are heap objects
with a reference count. Because every value is a value type (§4), cycles
cannot be created, so reference counting alone frees everything. Mutation of
a shared object copies it first (copy-on-write).

Every expression produces an owned value; storing it moves it, and each
variable is released when its scope ends (including on early `return`).
`cargo test` runs every program in `tests/run/` under the macOS `leaks` tool.

Known gaps (M2): an early `return` from inside a larger expression (for
example `f(a(), if c { return 1 } else { 2 })`) can leak the already-evaluated
`a()`. Iterating a changeable list that is itself nested in another list's
element (`for x in outer[i].items`) while appending to `outer` in the loop
body is not safe yet.

### 10.3 Runtime interface

The runtime exposes a small C ABI that generated code calls. It is internal
and versioned together with the compiler, so it can change freely.

## 11. Decisions log

| # | Question | Decision |
|---|---|---|
| 1 | Action blocks vs. child-view blocks | Same `{ }` syntax; the parameter type decides. |
| 2 | Logic operators | Symbols: `&&`, `\|\|`, `!`. |
| 3 | String interpolation | `{expr}`, escape a brace with `\{`. |
| 4 | Bindings at the call site | No marker; the `bind` parameter type decides. |
| 5 | Argument labels | Required, except the first argument of a one-parameter function or a `_` parameter. |
| 6 | Memory model | Value types only in v1; reference counting with copy-on-write. |
| 7 | Multiple files | No imports; a folder is one program with one namespace. |
| 8 | Missing values | Optionals `T?`, `nil`, `if let`, `?.`, `??`. No force-unwrap. |
| 9 | Error handling | Failing functions return `T?`. |
| 10 | Slow work | Built-ins take a completion block that runs on the UI thread. |

Still open, not blocking v1:

- **Error reasons.** Optionals say *that* something failed, not *why*. If that
  proves too limiting, add a `Result`-style enum later.
- **Styling and themes.** A theme value passed down the tree (like SwiftUI's
  environment) vs. modifiers only.
- **LLVM version.** `inkwell` must support the installed LLVM (Homebrew
  LLVM 22). If not, pin an older LLVM or call the LLVM C API directly.

## 12. Milestones

| # | Milestone | Done when |
|---|---|---|
| M0 | This design doc | Syntax approved (see §11). ✅ |
| M1 | Front end | `tessel check` parses and type-checks both examples, with clear errors for bad programs. ✅ |
| M2 | Native code, no UI | `tessel run` compiles functions, structs, lists and strings through LLVM and prints output. ✅ |
| M3 | First window | `examples/counter` runs on macOS: window, text, stacks, buttons, state. ✅ |
| M4 | Real app | `examples/todos` runs on macOS: text fields, toggles, pickers, loops, bindings. ✅ |
| M5 | Windows | Both examples run on Windows from the same source. |

### 12.1 Roadmap after M5

| Area | Status | What it means |
|---|---|---|
| Editor support | ✅ first version | VS Code extension: highlighting; `tessel lsp` for live errors and types on hover |
| Packaging | ✅ first version | `tessel build --bundle` makes `Name.app` on macOS; Windows apps have no console window. Still to do: app icons, code signing, installers |
| **Tessel IDE** | ✅ first version (`ide/`, opened by `tessel`) | Launching Tessel opens its own IDE. It edits `.tsl` files and runs the `tessel` commands for you: check (errors as you type, like `tessel lsp`), run, debug, build and package. No terminal needed. |
| Hot reload | planned | Save a file and the running app updates, keeping its state |
| App polish | planned | Dark mode, animations, real images, Tab between fields, IME input, accessibility |
| Standard library | planned | Files, JSON, dates, math, richer `fetch` (POST, headers, status codes) |
| Language gaps | planned | Error reasons (not just `nil`), generics for user types, more collection methods |

**Tessel IDE — first thoughts.** Ideally the IDE is itself a Tessel app, built
with the language it edits. That needs a few things Tessel doesn't have yet:
a multi-line code editor view with syntax colors, reading and writing files,
and starting other programs (`tessel run`) and showing their output. Those are
useful for any Tessel app, so building the IDE drives the standard library
forward too. "Debugging" first means clickable runtime errors and `print`
output in a console panel; stepping through code with breakpoints is a larger
project on top of that.

### 12.2 UI status after M4

Working: windows, HiDPI, the build → layout → draw cycle, `state` that
survives rebuilds, actions, `bind` through any number of views (including
list elements), `Text`, `Button`, `Toggle`, `Picker` (segmented),
`VStack`/`HStack`/`ZStack`, `Spacer`, `Scroll` (mouse wheel), `Image`
(placeholder box), all modifiers, `.id` (view state follows its item when a
list is reordered), `after`, `fetch` (`http(s)://` and `file://`), hover and
pressed states, and a headless script mode for tests.

`TextField` editing: typing, click to place the caret, drag / shift-click /
double-click / triple-click to select, arrow keys (with Shift to select,
Option for words, Cmd for line start and end), Home/End, Backspace/Delete
(also by word or line), select all, copy, cut and paste. The field scrolls
to keep the caret visible.

Not yet: IME input (e.g. Chinese, Japanese); undo; keyboard focus with Tab;
loading real images; accessibility; dark mode; animations.
