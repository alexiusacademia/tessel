---
title: Computing on the GPU
description: "Run your own number-crunching on the GPU: `kernel fn`, `GPUBuffer`, `gpu.run` over 1, 2 or 3 axes, sums and dot products, the CPU fallback, and what 32-bit precision means for your results."
---

A GPU does the same small piece of work on millions of numbers at once.
When a program spends its time in loops over big grids or arrays (a
simulation, an image filter, a solver), moving those loops to the GPU can
make it several times faster. In Tessel you write the loop's body as a
**kernel**, in Tessel, and run it on the GPU's numbers with `gpu.run`.

```tessel
kernel fn scale(_ i: Int, xs: GPUBuffer<Float32>, by: Float32) {
    xs[i] = xs[i] * by
}

fn main() {
    let xs = GPUBuffer(from: [Float32(1), 2, 3, 4])
    gpu.run(scale, over: xs.count, xs, 10)
    print(xs.read())
    print(gpu.sum(xs))
    print(gpu.device)
}
```

```text
[10.0, 20.0, 30.0, 40.0]
100.0
Apple M5 Pro
```

- A `kernel fn` runs once for each index: here `scale` runs four times, with
  `i` from 0 to 3, all at the same time.
- A [`GPUBuffer`](/reference/stdlib/#gpubuffer) is a block of numbers that
  lives on the GPU. `GPUBuffer(from:)` copies a list there, and `read()`
  copies it back.
- `gpu.run(scale, over: xs.count, xs, 10)` runs `scale` over `0..<4`, with
  `xs` and `10` for the kernel's other parameters.
- `gpu.sum` adds up a buffer on the GPU. `gpu.device` names the GPU (or
  `CPU`, see [Without a GPU](#without-a-gpu)).

The same program works on macOS (Metal), Windows (DirectX 12) and Linux
(Vulkan), and on a computer with no usable GPU it runs the kernel on the
CPU's cores instead.

## Kernels

A kernel is a function marked `kernel`. Its first parameter is its
**index**: an `Int` for a list, `(Int, Int)` for a grid, or
`(Int, Int, Int)` for a block of cells. Its other parameters are
`GPUBuffer<Float32>`s, `GPUBuffer<Int>`s and plain `Float32`, `Int` and
`Bool` values. It returns nothing: it writes its results into buffers.

```tessel
kernel fn spread(_ at: (Int, Int), t: GPUBuffer<Float32>, next: GPUBuffer<Float32>, n: Int) {
    let (x, y) = at
    let c = x + n * y
    if x == 0 {
        next[c] = 100
    } else if x == n - 1 || y == 0 || y == n - 1 {
        next[c] = 0
    } else {
        next[c] = 0.25 * (t[c - 1] + t[c + 1] + t[c - n] + t[c + n])
    }
}
```

A grid is stored in a buffer row after row, so cell `(x, y)` is at
`x + n * y`, and a block's cell `(i, j, k)` at `i + nx * (j + ny * k)`.

Inside a kernel you can use a part of Tessel that a GPU can run:

- `let` and `var` of `Int`, `Float32` and `Bool`, and tuples to take an
  index apart
- arithmetic, comparisons, `&&`, `||`, `!`, and the bit operations on `Int`
- `if`/`else` (also as a value: `let s: Float32 = if a { 1.0 } else { -1.0 }`),
  `for i in a..b`, `while`, `break`, `continue` and `return`
- reading and writing a buffer's elements: `t[c]`, `next[c] = …`, `+=`
- `sqrt`, `exp`, `log`, `log10`, `pow`, `sin`, `cos`, `tan`, `asin`, `acos`,
  `atan`, `atan2`, `floor`, `ceil`, `round`, `abs`, `min` and `max`
- `Float32(i)` and `Int(f)`

Anything else is a compile error at its line, such as text, lists,
`match`, closures, `Float`, or calling a function of your own:

```tessel
kernel fn heat(_ i: Int, t: GPUBuffer<Float32>, k: Float) {
    t[i] = helper(x: t[i])
}
```

```text
error: a kernel computes with 32-bit numbers: use `Float32`, not `Float`
 --> bad.tsl:5:52
  |
5 | kernel fn heat(_ i: Int, t: GPUBuffer<Float32>, k: Float) {
  |                                                    ^^^^^
  |
  = help: GPUs (Apple's have no 64-bit floats at all) compute in 32 bits; pass `Float32(x)`

error: a kernel can't call `helper`
 --> bad.tsl:6:12
  |
6 |     t[i] = helper(x: t[i])
  |            ^^^^^^
  |
  = help: it can use the math functions (`sqrt`, `exp`, `min`, `max`, `abs`, `floor`…) and `Float32(…)` and `Int(…)`
```

A decimal number inside a kernel, like `0.25`, is a `Float32`.

Two things work differently in a kernel, because that's how GPUs work:

- **`Int` is 32-bit** (from −2,147,483,648 to 2,147,483,647) and wraps
  around instead of stopping the program. Dividing by zero gives the number
  divided (`a / 0` is `a`), and `a % 0` is 0. Buffers of up to two billion
  elements fit.
- **An index outside a buffer doesn't crash.** It reads or writes the
  buffer's last element instead (a negative index too). Keep your indices
  inside, as `spread` above does; this rule only keeps a mistake from
  crashing the computer's graphics.

## GPU buffers

```tessel
let a = GPUBuffer<Float32>(count: n * n)     // n × n zeros
let b = GPUBuffer(from: [Float32(1), 2, 3])  // a copy of a list
let ids = GPUBuffer<Int>(count: 100)         // whole numbers (32-bit)
```

A buffer stays on the GPU between runs, so a simulation can run thousands
of steps without its numbers going back and forth. You copy them yourself,
when you need them:

| | |
|---|---|
| `count` | How many numbers it holds. |
| `read()` | Copies them back as a list (`[Float32]` or `[Int]`). Waits for the runs before it. |
| `write(_ values)` | Copies a list into it, from the start. |
| `fill(_ value)` | Sets every number to `value`. |
| `copy(from: other)` | Copies another buffer's numbers, on the GPU. |

A buffer's numbers can't be used one by one outside a kernel (`a[0]` in
`main` is an error): read them, or add them up with a
[reduction](#sums-and-dot-products).

## Running kernels

`gpu.run(kernel, over: size, values…)` runs `kernel` once for each index
from 0 up to `size`: an `Int`, or `(nx, ny)` or `(nx, ny, nz)` for a kernel
whose index has two or three parts. The values after `over:` go to the
kernel's other parameters, in order.

```tessel
fn main() {
    let n = 512
    let a = GPUBuffer<Float32>(count: n * n)
    let b = GPUBuffer<Float32>(count: n * n)
    for _ in 0..1000 {
        gpu.run(spread, over: (n, n), a, b, n)
        gpu.run(spread, over: (n, n), b, a, n)
    }
    let t = a.read()
    print("10 cells in: {t[10 + n * (n / 2)]}°")
    print("heat on the plate: {gpu.sum(a)}")
}
```

```text
10 cells in: 75.18041°
heat on the plate: 1251268.9649658203
```

`gpu.run` returns at once: the run is **queued**, and the GPU works through
the queue in order while your program goes on. The 2000 runs above are
queued in a moment. Only `read()`, a reduction and `gpu.wait()` wait for
the GPU to catch up, so leave those out of the inner loop when you can.

Each run here reads one buffer and writes the other, and the next run swaps
them. A kernel shouldn't read a cell that another index of the same run
writes: indices run at the same time, in no particular order.

## Sums and dot products

| | |
|---|---|
| `gpu.sum(b)` | The sum of a `GPUBuffer<Float32>`'s numbers. |
| `gpu.min(b)`, `gpu.max(b)` | The smallest and largest. |
| `gpu.dot(a, b)` | `a[0]*b[0] + a[1]*b[1] + …` over two buffers of the same count. |
| `gpu.wait()` | Waits until every queued run is done. |

Each returns a `Float` (64-bit) and waits for the runs before it. Here they
drive a conjugate-gradient solve of `-u'' = 1`, the core of many solvers:

```tessel
kernel fn apply(_ i: Int, p: GPUBuffer<Float32>, ap: GPUBuffer<Float32>, n: Int) {
    let left: Float32 = if i > 0 { p[i - 1] } else { 0.0 }
    let right: Float32 = if i < n - 1 { p[i + 1] } else { 0.0 }
    ap[i] = 2.0 * p[i] - left - right
}

kernel fn update(_ i: Int, x: GPUBuffer<Float32>, r: GPUBuffer<Float32>, p: GPUBuffer<Float32>, ap: GPUBuffer<Float32>, alpha: Float32) {
    x[i] = x[i] + alpha * p[i]
    r[i] = r[i] - alpha * ap[i]
}

kernel fn direction(_ i: Int, p: GPUBuffer<Float32>, r: GPUBuffer<Float32>, beta: Float32) {
    p[i] = r[i] + beta * p[i]
}

fn main() {
    let n = 199
    let h = 1.0 / Float(n + 1)
    let x = GPUBuffer<Float32>(count: n)
    let r = GPUBuffer<Float32>(count: n)
    r.fill(Float32(h * h))
    let p = GPUBuffer<Float32>(count: n)
    p.copy(from: r)
    let ap = GPUBuffer<Float32>(count: n)
    var rr = gpu.dot(r, r)
    let start = sqrt(rr)
    var rounds = 0
    while sqrt(rr) > 1e-6 * start && rounds < 2000 {
        gpu.run(apply, over: n, p, ap, n)
        let alpha = rr / gpu.dot(p, ap)
        gpu.run(update, over: n, x, r, p, ap, Float32(alpha))
        let next = gpu.dot(r, r)
        gpu.run(direction, over: n, p, r, Float32(next / rr))
        rr = next
        rounds += 1
    }
    let u = x.read()
    print("{rounds} rounds")
    print("u(0.5) = {u[n / 2]} (exact 0.125)")
}
```

```text
100 rounds
u(0.5) = 0.12499991 (exact 0.125)
```

On the CPU fallback the same program printed `168 rounds` and
`0.12500001`: rounding in a different order changes the path a solver
takes, though not where it ends up. See [Precision](#precision).

## Precision

Kernels compute with **`Float32`**: 32-bit floating-point numbers, about 7
significant digits (a `Float` has about 16). That's what GPUs are fast at;
Apple's GPUs have no 64-bit numbers at all, so Tessel uses one precision
everywhere. In practice:

- **Each value is good to about 1 part in 10 million.** A grid of
  temperatures, velocities or pixels rarely needs more.
- **Sums are better than that.** `gpu.sum` and `gpu.dot` add 256 numbers
  at a time in 32-bit, as a tree (so each number passes through only 8
  additions), then add those blocks' sums in 64-bit with Kahan's
  compensation. The result is a `Float`. A sum of a million `0.5`s is
  exactly `500000.0`.
- **A solver's own measure of its error can go lower than its answer is
  good to.** A conjugate-gradient solve can drive its residual to 1e-6,
  but the answer is stored in 32-bit, so the residual computed afresh
  from it sits near 1e-5 to 1e-4 on a large grid. If you need a solve to
  1e-8, you need 64-bit numbers, and so the CPU.
- **The GPU and the CPU fallback agree to rounding, not to the last
  digit.** Whole-number work and `+`, `-`, `*` give the same answers;
  `sqrt`, `exp`, `sin` and the like can differ in the last digit, and a
  sum's order differs. Compare results with a tolerance.
- **Fast math.** GPUs may combine `a * b + c` into one rounding, and treat
  values near zero as zero; don't count on infinities or `nan` coming out
  the same on every computer.

Outside kernels, `Float32` is an ordinary type: it has arithmetic,
comparisons, `Float32(x)` and `Float(x)` conversions, the math functions,
lists of it, and it prints with the fewest digits that read back
(`Float32(0.1)` prints `0.1`). See [Float32](/language/types/#float32).

## Without a GPU

When there's no GPU Tessel can use (a virtual machine, a server, a
computer without a graphics driver), each kernel runs on the CPU instead,
compiled to native code and spread over all of the cores. It's the same
program and the same 32-bit numbers, only slower. `gpu.isCPU` tells which
you have, and `gpu.device` is `"CPU"`.

Two settings of the `TESSEL_GPU` environment variable choose for you:

- `TESSEL_GPU=cpu` always uses the CPU, for tests whose output must match
  on every computer, or to compare the two.
- `TESSEL_GPU=gpu` uses a GPU even when the only one is a software one
  (like Linux's llvmpipe), which Tessel otherwise skips because the CPU
  fallback is faster than it.

## How much faster

On an Apple M5 Pro, `examples/gpu-flow` in the Tessel repository runs a
water simulation's time step on a 96 × 96 × 96 grid (884,736 cells):
moving the velocities, a pressure solve by conjugate gradients with a
multigrid preconditioner to 1e-6, and moving the water's surface. Over 1000
steps:

| | Per step |
|---|---|
| GPU (Metal) | 9.4 ms |
| CPU fallback, all cores | 40.5 ms |

The GPU's memory is about three times as fast as the CPU's, and a step like
this one is mostly reading and writing memory, so that's where most of the
gain comes from. Work that does more arithmetic per number gains more.

A kernel is worth it when there's a lot of work per run: thousands of
indices or more. Each run costs a few microseconds and each wait a
fraction of a millisecond, so moving a loop of a hundred numbers to the GPU
makes it slower.

## In apps

GPU compute works in apps as well as in command-line programs, and in apps
for the Mac App Store's sandbox: it needs no extra permission.
