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
| `Range` | `0..3`, `1..n` | Half-open range of `Int`s. A normal value: it can be stored, passed, compared with `==` and printed. |
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

## 6b. Interfaces

```tessel
interface Shape {
    fn area() -> Float
}

struct Circle: Shape {
    radius: Float
    fn area() -> Float { pi * radius * radius }
}

let shapes: [Shape] = [Circle(radius: 1.0), Rect(width: 2.0, height: 3.0)]
```

- An interface lists method signatures and properties (`name: String`);
  structs and enums conform with `: A, B` and must have each method with the
  same labels, types and result, and each property as a field (so enums can't
  meet property requirements).
- An interface is a type. A conforming value converts to it where the
  interface is expected (arguments, results, list items, fields, optionals),
  and so do lists and optionals of it (`[Circle]` to `[Shape]`, copied).
- Interface values are values: a copy is independent. Methods that change the
  value (inferred, like struct methods) work on a `var`, copying the box
  first when it's shared (copy-on-write).
- `==` compares the type and the value; `is` and `as?` ask which type a value
  holds. Saving writes `{"type": name, "value": …}`, and loading picks the
  type by name among the conforming types.
- Implementation: an interface value is `{method table, box}`; the box holds
  the concrete value, and the table (one per type and interface) lists that
  type's methods in the interface's order, then one function per property
  giving the field's address. `is`/`as?`, `==` and saving compare the table
  pointer to identify the type.

## 6c. Generics

```tessel
fn lastOr<T>(items: [T], fallback: T) -> T { items.last ?? fallback }
struct Stack<T> { items: [T] = [] }
enum Result<T> { ok(value: T), failure(message: String) }
fn largest<T: Shape>(items: [T]) -> T? { … }
```

- Functions, structs and enums take type parameters, optionally constrained
  to an interface (`T: Shape`), whose methods and properties can then be used.
- Type arguments are inferred from the arguments and the expected type, or
  written out for types (`Stack<Int>()`, `Result<Int>.ok(value: 1)`). In
  expressions, `Name<` starts type arguments only when the name is
  capitalized and the matching `>` is followed by `(` or `.`.
- Implementation: specialization. Each set of type arguments gets a copy of
  the declaration (`generics.rs`: new expression ids, the parameters replaced,
  names like `Stack<Int>`), which is checked and compiled as ordinary code, so
  codegen knows nothing about generics. The declaration is also checked once
  with its parameters as `Ty::Generic`, which is accepted like an error type;
  errors a particular type causes are reported in its copy, with a note at
  the use that needed it. Declared names in a copy get spans tagged in the
  file number's high bits, since local types are recorded by span.

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
while x != 1 { … }
while let next = queue.first { … }
```

- `break` leaves the innermost `for` or `while`; `continue` goes on with its
  next round. Neither crosses a closure (a block is a function of its own).
  View bodies use only `if`, `match` and `for`: they describe what to show.

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

`.padding` takes one amount for every edge (`.padding()` is 8), or
separate edges; the most specific value wins, and edges not named get 0:

```tessel
.padding(16)
.padding(horizontal: 16, vertical: 8)
.padding(top: 4, leading: 12)
.padding(8, bottom: 20)      // 8 everywhere except the bottom
```

### 8.5 Built-in views (v1)

| View | Signature (simplified) |
|---|---|
| `Text` | `Text(_ text: String)` |
| `Button` | `Button(_ label: String, action: fn())` |
| `TextField` | `TextField(_ placeholder: String, text: bind String)` |
| `Toggle` | `Toggle(isOn: bind Bool)` |
| `Picker` | `Picker(selection: bind T, content: View)` with `.tag(value)` on options |
| `VStack` / `HStack` | `(spacing: Float = 8, alignment: Alignment = .center, content: View)` |
| `ZStack` | `ZStack(alignment: Alignment = .center, content: View)` |
| `Spacer` | `Spacer()` — takes the remaining space in a stack |
| `Scroll` | `Scroll(content: View)` |
| `Image` | `Image(_ path: String)` — an SVG, PNG or JPEG file |
| `Icon` | `Icon(_ name: IconName)` — a built-in icon, e.g. `Icon(.folder)` |
| `Svg` | `Svg(_ source: String)` — SVG markup written inline |
| `CodeEditor` | `CodeEditor(text: bind String, errorLines: [Int] = [], line: bind Int = …)` (`line:` is optional) |
| `TabView` | `TabView(selection: bind T, onClose: fn(T)?, content: View)` with `.tabItem(title)` and `.tag(value)` on pages |
| `Divider` | `Divider()` — a thin separator line |

**Icons.** `IconName` is a built-in enum of commonly used icons (from the
Lucide set, ISC license): `file`, `fileText`, `fileCode`, `filePlus`,
`folder`, `folderOpen`, `folderPlus`, `plus`, `minus`, `close`, `check`,
`trash`, `pencil`, `play`, `stop`, `save`, `search`, `chevronRight`,
`chevronDown`, `chevronLeft`, `chevronUp`, `settings`, `refresh`, `warning`,
`info`, `circleCheck`, `circleX`, `copy`, `home`, `user`, `heart`, `star`,
`arrowLeft`, `arrowRight`, `arrowUp`, `arrowDown`, `menu`, `more`, `sun`,
`moon`, `download`, `upload`, `link`, `externalLink`, `calendar`, `clock`,
`bell`, `mail`, `lock`, `eye`, `eyeOff`, `terminal`, `code`, `bug`,
`package`, `hammer`, `undo`, `redo`, `image`. An icon takes its size from
`.font(size:)` and its color from `.color(…)`, so it lines up with text:

```tessel
HStack(spacing: 4) {
    Icon(.warning).color(.orange)
    Text("3 problems")
}
```

In `Svg` and SVG files, `currentColor` is drawn in the view's `.color`.

Common modifiers: `padding`, `frame(width:height:minWidth:maxWidth:minHeight:maxHeight:alignment:)`, `font`, `color`,
`textAlignment`, `background`, `border`, `opacity`, `hidden`, `disabled`, `onTap`, `id`.

### 8.6 Alignment

`Alignment` has nine cases: `leading`, `center`, `trailing`, `top`, `bottom`,
`topLeading`, `topTrailing`, `bottomLeading`, `bottomTrailing`.

- A stack's `alignment:` lines its children up **across** the stack: a
  `VStack` uses left/center/right (`.leading`, `.center`, `.trailing`), an
  `HStack` top/center/bottom (`.top`, `.center`, `.bottom`).
- Along the stack, children are packed in the middle. Push them to one end
  with a `Spacer()` before or after them.
- `.frame(width:height:alignment:)` places the view's content inside a
  larger frame. Text and SVGs are centered unless told otherwise; a stack
  with a frame spreads over the whole frame unless an alignment is given.
- `.frame(maxWidth: .infinity)` makes a view take all the width it's
  offered (a finite value caps it), so it can line up with the edges of its
  container. `.infinity` is the one shorthand for a `Float`:

```tessel
Text("Subheading").frame(maxWidth: .infinity, alignment: .trailing)
Button("Continue") { next() }.frame(maxWidth: .infinity)
```

- `.frame(minWidth:minHeight:)` keeps a view from getting smaller than a
  size (it still grows with its content), e.g. buttons of equal width:
  `Button("OK") { … }.frame(minWidth: 100)`.
- `.textAlignment(.leading | .center | .trailing)` lines up the lines of
  multi-line text within the text's box. Like `.font`, it applies to every
  text inside a stack it's set on. (Where the box itself goes is up to the
  stack's `alignment:` or the frame's.)

```tessel
Text("Welcome to Tessel.\nLet's build something.").textAlignment(.center)
```

- `ZStack(alignment:)` places each layer, e.g. a badge in a corner:

```tessel
ZStack(alignment: .topTrailing) {
    Icon(.bell).font(size: 24)
    Text("3").font(size: 10).color(.white).padding(2).background(.red, radius: 6)
}
```

### 8.7 Identity

When a `for` loop's list changes, views are matched to items by position. Use
`.id(item.id)` to match by a stable key instead, so state inside rows (for
example a text field being edited) follows its item when the list reorders.

## 9. Programs, files and the standard library

- A **program** is a folder. Every `.tsl` file in it shares one namespace, so
  there are no imports. Names must be unique across the folder.
- **Standard library:** `print`, `String` methods (`count`, `contains`,
  `split`, `trim`, `uppercase`, `padStart`, `words`, `urlEncoded`, path parts,
  …), `List` methods (`append`, `remove(at:)`, `filter`, `map`, `sorted`,
  `index(of:)`, `sum`, `min`/`max`, `slice`, `unique`, `shuffled`, …),
  `Dictionary` methods, `Int`/`Float` math and random numbers, files and
  folders (`readFile`, `appendFile`, `readData`, `appDataFolder`, `joinPath`,
  …), dates (`now`, `date`, `formatDate`, `parseDate`), the system
  (`arguments`, `exit`, `platform`, `openURL`, the clipboard), and file
  dialogs. The full list is on the website's standard library reference.
- **Encoding:** `toJson(value, pretty:)` / `fromJson(text)` and
  `toBinary(value)` / `fromBinary(data)` (MessagePack) work on any struct,
  enum, list, dictionary, optional, range, `Data` or basic value; the
  compiler generates the encoder and decoder for each type. The decoders take
  their type from where the result goes (`let p: Project? = fromJson(text)`,
  or `if let p: Project = fromJson(text)`) and give `nil` if it doesn't fit.
  Structs are objects by field name: missing fields take their defaults,
  unknown ones are ignored. `Data` holds raw bytes.
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

  The built-ins: `fetch` (text), `request` (any HTTP method, headers and
  body; the block gets an `HttpResponse` with `status`, `ok`, `text`,
  `data`, `headers` and `error`), `download` (to a file, with progress),
  WebSockets (`openWebSocket`, `sendWebSocket`, `closeWebSocket`), timers
  (`after`, `every`/`stopTimer`) and `runCommand`. In a program with only
  `fn main`, the program keeps running after `main` returns until all of
  them are done.
- **Heavy work** runs on another thread with
  `background(work: { … }) { result in … }`. Nothing is shared between
  threads: the work block gets encoded *copies* of the values it uses (the
  same encoding as `toBinary`), and its result is decoded again for the
  trailing block, which runs on the UI thread and may change `state`. So the
  checker rejects `state`, bindings, `self` and its fields, and the app's or
  view's own functions inside the work block (top-level `fn`s are fine), as
  well as values or results that can't be encoded; the fix is to copy what
  the work needs into a `let` first. `fn main` programs and the headless
  test `wait` command wait for background work like they do for timers.

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

An early `return`, `break` or `continue` in the middle of an expression
(`f(a(), if c { return 1 } else { 2 })`) releases the values already computed
for it: while the rest of an expression is evaluated, codegen keeps them on a
"held" list, and early exits release what's on it. A `for` loop over a list
reached through an index (`for x in outer[i].items`) finds the list again each
round through a locator (so the outer list may grow in the loop), and the
variable at its root is boxed so the locator shares it.

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
| M5 | Windows | Both examples run on Windows from the same source. ✅ (built and tested headlessly in CI; not yet tried on a Windows desktop) |

### 12.1 Roadmap after M5

| Area | Status | What it means |
|---|---|---|
| Editor support | ✅ first version | VS Code extension: highlighting; `tessel lsp` for live errors and types on hover |
| Packaging | ✅ first version | `tessel build --bundle` makes `Name.app` on macOS; Windows apps have no console window. An `icon.png` next to the program is the app's icon (Tessel's by default), in the bundle on macOS and in the `.exe` on Windows; the IDE has Tessel's. Still to do: code signing, installers |
| **Tessel IDE** | ✅ (`ide/`, opened by `tessel`): explorer with a folder tree, new / rename / delete, tabbed editor, undo, problems and console panels, restart on save | Launching Tessel opens its own IDE. It edits `.tsl` files and runs the `tessel` commands for you: check (errors as you type, like `tessel lsp`), run, debug, build and package. No terminal needed. |
| Hot reload | partly | The IDE restarts the running app when you save ("Restart on save"). True hot reload, keeping the app's state, is still planned |
| App polish | ✅ first version | Light and dark appearance (following the system, or `app Name(appearance: .dark)`; `isDarkMode()`), colors of your own (`.rgb`, `.rgba`), `.animation()` for views that move, resize and fade, PNG and JPEG in `Image`, Tab between fields, input methods (IME). Still to do: accessibility, keyboard use of buttons and toggles, a view fading out as it disappears |
| Standard library | ✅ first version | Files and folders, JSON and binary encoding (`toJson`/`fromJson`, `toBinary`/`fromBinary`), `Data`, dates, random numbers, the clipboard, many `String`/`List`/number methods; networking with `request`, `download` and WebSockets; repeating timers; `background` work on another thread. Still to do: time zones, cancelling requests, binary WebSocket messages |
| Language gaps | ✅ | Error reasons (`Result`, `Error`, `try`), generics for user types |

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
(SVG, PNG and JPEG), all modifiers, `.id` (view state follows its item when a
list is reordered), `after`, `fetch` (`http(s)://` and `file://`), hover and
pressed states, and a headless script mode for tests.

`TextField` editing: typing, click to place the caret, drag / shift-click /
double-click / triple-click to select, arrow keys (with Shift to select,
Option for words, Cmd for line start and end), Home/End, Backspace/Delete
(also by word or line), select all, copy, cut and paste. The field scrolls
to keep the caret visible.

Also: Tab and Shift+Tab between fields, input methods (IME, with the text
being composed shown underlined), light and dark appearance, and
`.animation()`.

Not yet: accessibility; keyboard focus for buttons, toggles and pickers.
