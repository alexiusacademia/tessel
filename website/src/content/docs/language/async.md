---
title: Waiting with async and await
description: Writing code that waits in Tessel - async functions, await, how await works with any function that ends with a completion block, errors, background work, and the rules.
---

Some work takes time: a download, a timer, a heavy computation. Tessel's
built-in functions for such work take a *completion block*, which runs when
the work is done:

```tessel
fetch(url: "https://example.com/data.txt") { text in
    print(text ?? "failed")
}
```

That's fine for one step. With several steps in a row, each block ends up
inside the one before it. **`await`** lets you write the steps one after
another instead:

```tessel
let text = await fetch(url: "https://example.com/data.txt")
print(text ?? "failed")
```

`await` starts the work, and the code after it goes on when the work is
done. Meanwhile the rest of the program keeps running: an app's window stays
responsive, and timers and other blocks run as usual.

## Async functions

A function that uses `await` is declared with **`async fn`**:

```tessel
async fn countdown(from: Int) {
    var n = from
    while n > 0 {
        print(n)
        await after(seconds: 1.0)
        n -= 1
    }
    print("Go!")
}

async fn main() {
    await countdown(from: 3)
    print("Done")
}
```

```text
3
2
1
Go!
Done
```

`await after(seconds: 1.0)` waits a second without stopping anything else.
`await countdown(from: 3)` waits until `countdown` has finished.

An async function returns a value like any other function, and `await`
gives that value to the caller:

```tessel
async fn wordCount(path: String) -> Int {
    let text = await fetch(url: "file://{path}")
    return (text ?? "").split(" ").count
}

async fn main() {
    let n = await wordCount(path: "{currentFolder()}/notes.txt")
    print("{n} words")
}
```

`await` works anywhere an expression does: in a `let`, in an `if`, in a
loop, as an argument. Put it in parentheses to use the result directly:
`(await load()).count`.

A program's `main` can be `async`. The program ends when `main` and
everything it started have finished.

## Where `await` can be used

In two places:

- In the body of an **`async fn`**.
- In a **block that produces no value**, such as a button's action, the
  block of `after`, or `.onAppear`. The block starts, runs up to its first
  `await`, and goes on later.

```tessel
app Weather(width: 360, height: 200) {
    state report = "Press Refresh"

    Text(report)
    Button("Refresh") {
        report = "Loading…"
        let text = await fetch(url: "https://example.com/weather.txt")
        report = text ?? "Could not load the weather"
    }
}
```

An ordinary function can't `await`, because its caller expects its result
right away:

```text
error: `await` can only be used in an `async` function, or in a block that doesn't produce a value
  = help: mark the function: `async fn …`
```

For the same reason a block that must produce a value, like the one given
to `map`, can't `await`.

## What can be awaited

Two kinds of call:

- A call to an **`async fn`**.
- A call to any function whose **last parameter is a completion block**: a
  block that takes the result (or nothing) and returns nothing. You leave
  the block out, and `await` gives you what the block would have received.

So the built-in functions work with `await` as they are:

| Call | `await` gives |
|------|---------------|
| `after(seconds:)` | nothing; it just waits |
| `fetch(url:)` | `String?`, the text |
| `request(url:method:headers:body:)` | the `HttpResponse` |
| `download(url:to:)` | `Bool`, whether it worked |
| `runCommand(path:args:onOutput:)` | `Int`, the exit code |
| `background(work:)` | the work's result |

And so do your own functions that take a completion block:

```tessel
fn ask(question: String, done: fn(String)) {
    after(seconds: 0.5) {
        done("yes to \"{question}\"")
    }
}

async fn main() {
    // With a block:
    ask(question: "tea?") { answer in
        print(answer)
    }
    // With await:
    let answer = await ask(question: "coffee?")
    print(answer)
}
```

```text
yes to "tea?"
yes to "coffee?"
```

It works the other way round too. An `async fn` can be called *without*
`await`, with a block that gets its result. The caller then goes on at once:

```tessel
async fn square(n: Int) -> Int {
    await after(seconds: 0.1)
    return n * n
}

async fn main() {
    square(4) { result in
        print("the block got {result}")
    }
    print("main goes on")
    await after(seconds: 0.2)
}
```

```text
main goes on
the block got 16
```

Calling an async function with neither is an error, since its result would
be lost:

```text
error: `square` is `async`: wait for it with `await`
```

Some of the built-in functions also return a number (`request` and
`download` return one for cancelling). With `await` you get the result
instead, and that number isn't available. Call them with a block when you
need it.

## Errors

`await` combines with [`try`](/language/errors/). An async function that can
fail returns a `Result`, and `try await` passes a failure on:

```tessel
async fn parsed(text: String) -> Result<Int> {
    await after(seconds: 0.1)
    let n = try parseInt(text)
    return .ok(value: n)
}

async fn total(texts: [String]) -> Result<Int> {
    var sum = 0
    for text in texts {
        sum += try await parsed(text: text)
    }
    return .ok(value: sum)
}

async fn main() {
    match await total(texts: ["1", "2", "3"]) {
        .ok(value) -> print("total {value}")
        .failure(error) -> print("failed: {error.message}")
    }
    match await total(texts: ["1", "x"]) {
        .ok(value) -> print("total {value}")
        .failure(error) -> print("failed: {error.message}")
    }
}
```

```text
total 6
failed: "x" isn't a whole number
```

## Heavy work

`await` doesn't make code run on another thread. Everything still runs on
the UI thread, one piece at a time, so a long loop in an async function
freezes the window just as it would anywhere else.

To compute something heavy, give it to
[`background`](/ui/async/#heavy-work-in-the-background) and await that. The
work runs on another thread, and the result comes back when it's ready:

```tessel
fn slowSum(to: Int) -> Int {
    var total = 0
    for i in 0..to + 1 {
        total += i
    }
    total
}

async fn main() {
    let n = 1000000
    let total = await background { slowSum(to: n) }
    print(total)
}
```

```text
500000500000
```

## In apps

An app's own functions can be `async` too, and change its `state` as they
go on:

```tessel
app Steps(width: 360, height: 240) {
    state status = "Idle"
    state log: [String] = []

    VStack(spacing: 8, alignment: .leading) {
        Text(status)
        Button("Run") {
            status = "Running"
            await run()
            status = "Done"
        }
        for line in log {
            Text(line)
        }
    }
    .padding(16)

    async fn run() {
        for i in 1..4 {
            await after(seconds: 0.5)
            log.append("step {i}")
        }
    }
}
```

The window updates each time the function goes on. Nothing stops the user
from pressing **Run** again while it's running, which would start a second
run alongside the first. Disable the button while the work is going on if
that's not wanted: keep a `state busy = false`, set it around the work, and
add `.disabled(busy)`.

## Methods and interfaces

Methods can be `async`, and an interface can require one:

```tessel
interface Store {
    async fn load(key: String) -> String?
}

struct MemoryStore: Store {
    items: [String: String]
    async fn load(key: String) -> String? {
        await after(seconds: 0.1)
        return items[key]
    }
}

async fn show(store: Store, key: String) {
    print(await store.load(key: key) ?? "nothing for {key}")
}

async fn main() {
    let store = MemoryStore(items: ["a": "apple"])
    await show(store: store, key: "a")
    await show(store: store, key: "b")
}
```

```text
apple
nothing for b
```

An async method works on a **copy** of the value it's called on, because the
caller's variable may be gone by the time the method goes on. So it can read
`self` and its fields, but not change them. Return the new value instead and
let the caller store it.

## When the work never finishes

If the thing being awaited is dropped without finishing (a request that's
cancelled, a completion block that's never called), the code after the
`await` never runs. The function stops there, and everything it holds is
released. The same then happens to whatever was awaiting *that* function.

## Rules

- `await` goes right before a call.
- It can be used in an `async fn`, and in a block that produces no value.
- An `async fn` can't have `bind` parameters, and an async method can't
  change the value it's called on. A block that waits, inside a method, can't
  change that value either.
- `await` can't be combined with `?.` (`await item?.load()`): unwrap the
  value first with `if let`.
- A [computed property](/language/structs-enums/#computed-properties) and a
  [constant](/language/basics/#constants) can't wait.
- `async` and `await` are only keywords where they're used this way, so
  existing code with variables of those names keeps working.
