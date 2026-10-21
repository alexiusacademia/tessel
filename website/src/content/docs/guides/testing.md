---
title: Testing your code
description: Write tests next to your code with test blocks and expect, and run them with tessel test, each in its own process.
---

A **test** is a block of code that checks that something works. Write it
next to the code it checks, with `test`, a name that says what it checks,
and `expect` for each thing that must be true:

```tessel
struct Cart {
    prices: [Float] = []

    fn total() -> Float {
        var sum = 0.0
        for p in prices {
            sum += p
        }
        sum
    }
}

test "an empty cart costs nothing" {
    expect(Cart().total() == 0.0)
}

test "the total adds the prices" {
    let cart = Cart(prices: [1.5, 2.5])
    expect(cart.total() == 4.0)
}
```

Then run them:

```sh
tessel test .
```

```text
ok    an empty cart costs nothing  (6 ms)
ok    the total adds the prices  (6 ms)

2 passed, 0 failed, in 0.01 s
```

Tests are part of the program's files, so they can use everything in them,
private declarations too. Builds and `tessel run` leave them out of the
app; `tessel check` and editors check them like the rest of the code.

## When a test fails

When an `expect` is false, the test fails, and `tessel test` says where and
what it was. For a comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`) it shows
both sides too:

```tessel
test "the total adds the prices" {
    let cart = Cart(prices: [1.5, 2.5])
    expect(cart.total() == 5.0)
}
```

```text
FAIL  the total adds the prices  (main.tsl:17, 6 ms)
      main.tsl:19: expect(cart.total() == 5.0)
        left:  4.0
        right: 5.0
```

Each side is worked out once, so `expect(stack.removeLast() == 3)` takes
one item off. Values are shown as [JSON](/guides/json/), which shows structs,
enums, lists and optionals too: `{"x":1,"y":2}`.

A test goes on after a failed `expect`, so one run shows everything that's
wrong in it, and fails at the end. To stop it, `return` (or let it fail).

- **`expect(condition, "why")`** adds a message to the report.
- **`fail("why")`** fails the test without a condition, for a place it
  shouldn't reach.
- An `expect` in a function the test calls says where it is too, so
  helpers can check things for many tests.
- What a failing test printed is shown under it. A passing test's output
  isn't shown.

## Each test on its own

`tessel test` builds the program once, then runs each test in a process of
its own, several at a time:

- A test that **crashes** (an index out of range, say) fails, with the
  error, and the others still run.
- A test that **doesn't finish** is stopped after 60 seconds and fails.
  `--timeout 5` changes that.
- Tests can't change each other's data, so they can run in any order.

## Running some of them

```sh
tessel test .                     # every test in the program
tessel test . --filter total      # those whose name has "total" in it
tessel test math.tsl              # a program that's one file
```

`tessel test` ends with exit code 1 when a test fails, so a script or CI
can stop there.

## Tests of a library

A [library](/guides/libraries/)'s tests run with `tessel test .` in its
folder. The programs that use the library check its tests but never run
them. Put tests that need data files, or that are long, in a program of
their own in a subfolder that imports the library:

```tessel
// tests/main.tsl
import ".." as numeric

test "brent finds the square root of 2" {
    let r = numeric.brent(low: 0.0, high: 2.0) { x in x * x - 2.0 }
    expect(r.converged)
    expect(abs(r.value - sqrt(2.0)) < 1e-12)
}
```

```sh
tessel test tests
```

## Apps

An app's tests run without its window: `tessel test` sets the `app` aside
and runs the tests alone. To check what the app shows and how it reacts to
clicks and keys, see [Testing your UI](/ui/testing/).
