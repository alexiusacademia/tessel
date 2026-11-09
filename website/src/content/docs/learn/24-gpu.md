---
title: 24. Number-crunching on the GPU
description: Write kernels that run on the graphics card, keep numbers there in GPU buffers, run a kernel over a grid, add up results with gpu.sum, and see why 32-bit numbers are enough for most of it.
---

Your computer has two kinds of processor. The **CPU** runs your program:
it does one thing after another, very quickly, and has a handful of cores
that can each do that. The **GPU**, the graphics card, has thousands of
small cores that all do the same thing at once, on different numbers.
It was made to work out the color of every pixel on the screen, but
anything shaped like that ("do this to each of a million numbers") runs
on it too: simulations, image filters, solving equations on a grid.

In this lesson you'll learn:

- what a **kernel** is, and how `kernel fn` writes one
- how a `GPUBuffer` keeps numbers on the GPU, and how they get there and
  back
- how `gpu.run` runs a kernel over a list or a grid
- how `gpu.sum`, `gpu.min`, `gpu.max` and `gpu.dot` add up results
- what `Float32` is, and why GPUs use it

At the end you'll run the Game of Life, the classic grid of living and
dead cells, on four million cells.

## A kernel

A **kernel** is a function the GPU runs many times at once, once for each
**index**. You write it like any function, with `kernel` in front, and its
first parameter is the index:

```tessel
kernel fn square(_ i: Int, xs: GPUBuffer<Float32>) {
    xs[i] = xs[i] * xs[i]
}

fn main() {
    let xs = GPUBuffer(from: [Float32(1), 2, 3, 4, 5])
    gpu.run(square, over: xs.count, xs)
    print(xs.read())
}
```

```text
[1.0, 4.0, 9.0, 16.0, 25.0]
```

Read `main` a line at a time:

1. `GPUBuffer(from: …)` copies a list to the GPU. A **GPU buffer** is a
   list of numbers that lives in the graphics card's own memory, where the
   GPU can reach it quickly.
2. `gpu.run(square, over: xs.count, xs)` runs `square` five times, with
   `i` from 0 to 4, and `xs` for its other parameter. There's no loop:
   the five runs happen side by side.
3. `xs.read()` copies the numbers back into an ordinary list.

Compare it with a `for` loop that does the same. The loop's body is the
kernel; `over:` says how many times; the loop's variable is the index. The
one thing a kernel can't count on is order: index 3 may run before index
1, or at the same moment. So each index should only change its own
element.

`Float32` is a decimal number like `Float`, but in 32 bits instead of 64.
You'll see why [below](#why-float32).

## Numbers stay on the GPU

Copying numbers between the CPU's memory and the GPU's takes time, so a
buffer **stays** on the GPU: run kernel after kernel on it, and copy it
back only when you need to look. Make one with zeros, or from a list:

```tessel
let a = GPUBuffer<Float32>(count: 1000)     // 1000 zeros
let b = GPUBuffer(from: [Float32(1), 2, 3]) // a copy of a list
```

`read()` copies back, `write(list)` copies in, `fill(value)` sets every
number, and `copy(from:)` copies one buffer into another without leaving
the GPU. Outside a kernel you can't use `a[0]`: the number is on the GPU,
and your program would have to wait for it. Read the buffer, or let the
GPU add it up for you (see [Adding it up](#adding-it-up)).

## A grid

A kernel's index can have two parts, for a grid, or three, for a block of
cells. The cells are stored one row after another in a buffer, so cell
`(x, y)` of a grid `width` cells wide is element `x + width * y`.

The **Game of Life** is a grid of cells, each alive or dead. Each
generation, every cell counts its eight neighbours: a live cell with two
or three live neighbours lives on, a dead cell with exactly three comes
alive, and every other cell is dead in the next generation. Simple rules,
the same for every cell: a perfect kernel.

```tessel
kernel fn generation(_ at: (Int, Int), now: GPUBuffer<Float32>, next: GPUBuffer<Float32>, width: Int, height: Int) {
    let (x, y) = at
    var neighbours: Float32 = 0
    for dy in -1..2 {
        for dx in -1..2 {
            let nx = x + dx
            let ny = y + dy
            let inside = nx >= 0 && nx < width && ny >= 0 && ny < height
            if inside && !(dx == 0 && dy == 0) {
                neighbours += now[nx + width * ny]
            }
        }
    }
    let alive = now[x + width * y] == 1
    if neighbours == 3 || (alive && neighbours == 2) {
        next[x + width * y] = 1
    } else {
        next[x + width * y] = 0
    }
}
```

A live cell is `1` and a dead one `0`. The kernel reads the generation
`now` and writes the next one into a **different** buffer, `next`: if it
wrote into `now`, a neighbour running at the same moment might count a
cell that has already changed.

Inside a kernel you have numbers, `Bool`s, `if`, `for` over a range,
`while`, and the math functions. Text, lists, and calls to your own
functions aren't there; the GPU can't run them, and the compiler tells you
at the line:

```text
error: a kernel can't call `show`
```

Now run it on a small grid, with a **glider**: five cells that crawl
across the grid as the generations go by.

```tessel
fn show(_ cells: [Float32], width: Int, height: Int) {
    for y in 0..height {
        var line = ""
        for x in 0..width {
            line += if cells[x + width * y] == 1 { "#" } else { "." }
        }
        print(line)
    }
}

fn main() {
    let width = 12
    let height = 8
    var start: [Float32] = []
    for _ in 0..width * height {
        start.append(0)
    }
    // A glider: five cells that crawl down and to the right.
    for (x, y) in [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)] {
        start[x + width * y] = 1
    }
    let a = GPUBuffer(from: start)
    let b = GPUBuffer<Float32>(count: width * height)
    for _ in 0..4 {
        gpu.run(generation, over: (width, height), a, b, width, height)
        gpu.run(generation, over: (width, height), b, a, width, height)
    }
    show(a.read(), width: width, height: height)
    print("{gpu.sum(a)} cells alive")
}
```

```text
............
............
...#........
....#.......
..###.......
............
............
............
5.0 cells alive
```

`over: (width, height)` runs the kernel once for each cell. Each time
round the loop, one run goes from `a` to `b` and the next from `b` back to
`a`: eight generations, and the glider has moved two cells down and two to
the right.

`gpu.run` doesn't wait for the GPU. It puts the run in a **queue** and
returns at once; the GPU works through the queue in order while your
program goes on. Only `read()`, a sum, or `gpu.wait()` wait for the queue
to finish. That's why the loop above costs almost nothing: it just fills
the queue.

## Adding it up

`gpu.sum(a)` added up the whole grid on the GPU, and since live cells are
1, that's the number alive. There are four of these **reductions**, which
turn a buffer into one number:

| | |
|---|---|
| `gpu.sum(b)` | All the numbers added up |
| `gpu.min(b)` | The smallest |
| `gpu.max(b)` | The largest |
| `gpu.dot(a, b)` | `a[0]*b[0] + a[1]*b[1] + …` |

Each gives a `Float`, and waits for the runs queued before it.

## Why Float32

A `Float` has about 16 significant digits; a `Float32` has about 7. GPUs
are built for 32-bit numbers (Apple's have no 64-bit ones at all), so
kernels compute with `Float32`, and a decimal number in a kernel, like
`0.25`, is a `Float32`.

Seven digits is plenty for a pixel, a temperature or a cell that's 0 or 1.
Where it shows is in long sums, where small errors add up. So the
reductions add 256 numbers at a time on the GPU, then add those sums in
64-bit on the CPU, carefully. A sum of four million cells comes out
exact.

You'll see the 32 bits sometimes. Blur an image whose pixels add up to
500000, and the blurred image's sum may print as `500000.0001678467`: each
pixel is an average of nine, rounded to 7 digits. That's the size of
error to expect, about one part in ten million.

`Float32` works outside kernels too: `Float32(x)` makes one, `Float(y)`
turns one back, and they print the way you wrote them, so
`Float32(0.1)` prints `0.1`.

## Worked example: four million cells

How much faster is it? Here's the Game of Life on a 2048 × 2048 grid, with
a second kernel that fills it with a scrambled pattern, and a stopwatch
from `now()`:

```tessel
kernel fn seed(_ at: (Int, Int), cells: GPUBuffer<Float32>, width: Int) {
    let (x, y) = at
    // Scrambled bits of x and y (Ints in a kernel wrap around at 32 bits),
    // so about a quarter of the cells start alive.
    let h = (x * 374761393 + y * 668265263) ^ (x * y * 1274126177)
    let alive = (h >> 13) & 3 == 0
    cells[x + width * y] = if alive { 1.0 } else { 0.0 }
}

// (kernel fn generation, as above)

fn main() {
    let width = 2048
    let height = 2048
    let a = GPUBuffer<Float32>(count: width * height)
    let b = GPUBuffer<Float32>(count: width * height)
    gpu.run(seed, over: (width, height), a, width)
    let started = now()
    for _ in 0..100 {
        gpu.run(generation, over: (width, height), a, b, width, height)
        gpu.run(generation, over: (width, height), b, a, width, height)
    }
    let alive = gpu.sum(a)
    let seconds = now() - started
    print("{alive} cells alive after 200 generations")
    print("{(seconds * 1000 / 200).formatted(decimals: 2)} ms a generation on {gpu.device}")
}
```

On a MacBook with an M5 Pro:

```text
311982.0 cells alive after 200 generations
0.58 ms a generation on Apple M5 Pro
```

The same `generation`, written as an ordinary function over a list, took
16.4 ms a generation: the GPU is about 28 times faster here.

`Int`s in a kernel are 32-bit too, and instead of stopping the program
when they overflow, they **wrap around**, as GPUs do. `seed` uses that on
purpose to scramble its bits; in your own kernels, keep numbers below
about two billion.

## Without a GPU

Not every computer has a GPU Tessel can use: a virtual machine, or a
server, may not. Then each kernel runs on the CPU instead, spread over all
its cores, with the same 32-bit numbers. The program doesn't change, and
`gpu.device` says `CPU`. You can ask for that yourself, to compare:

```sh
TESSEL_GPU=cpu ./biglife
```

```text
311982.0 cells alive after 200 generations
1.79 ms a generation on CPU
```

The same cells, so the same answer, three times slower than the GPU, and
still nine times faster than the plain loop, because it uses every core.

## Common mistakes

### Reading and writing the same buffer

If a kernel reads its neighbours and writes into the same buffer, some
neighbours have already changed and some haven't, and the result changes
from run to run. Read one buffer and write another, then swap.

### Waiting inside the loop

`read()` and the reductions wait for the GPU. One of them every time
round a loop of short runs keeps the GPU idle half the time. Queue many
runs, then look once.

### Too little work

Starting a run costs a few microseconds. A kernel over a hundred numbers is
slower than a plain loop; over a million, it's much faster.

### Comparing to the last digit

The GPU and the CPU fallback can round differently in the last digit
(`sqrt`, `sin` and sums, mostly). Compare decimals with a tolerance:
`abs(a - b) < 1e-5`, not `a == b`.

## Exercises

**1. A world without edges.** Change `generation` so the grid wraps
around: a cell on the right edge has neighbours on the left edge, and the
bottom row's are on the top row. Run the glider for 24 generations and
watch it come back in at the top.

<details>
<summary>Solution</summary>

Replace the neighbour loop's inside:

```tessel
            let nx = (x + dx + width) % width
            let ny = (y + dy + height) % height
            if !(dx == 0 && dy == 0) {
                neighbours += now[nx + width * ny]
            }
```

and run `for _ in 0..12`:

```text
......###...
............
............
............
............
............
.......#....
........#...
5.0 cells alive
```

</details>

**2. How much changed.** Write a kernel `difference` that stores
`abs(a[i] - b[i])` for each cell into a third buffer, and use it with
`gpu.sum` to print how many cells changed in each of the glider's first
four generations.

<details>
<summary>Solution</summary>

```tessel
kernel fn difference(_ i: Int, a: GPUBuffer<Float32>, b: GPUBuffer<Float32>, out: GPUBuffer<Float32>) {
    out[i] = abs(a[i] - b[i])
}
```

and in `main`, instead of the loop:

```tessel
    let changed = GPUBuffer<Float32>(count: width * height)
    for g in 1..5 {
        gpu.run(generation, over: (width, height), a, b, width, height)
        gpu.run(difference, over: width * height, a, b, changed)
        print("generation {g}: {gpu.sum(changed)} cells changed")
        a.copy(from: b)
    }
```

```text
generation 1: 4.0 cells changed
generation 2: 4.0 cells changed
generation 3: 4.0 cells changed
generation 4: 4.0 cells changed
```

</details>

**3. A blur.** Make a 1000 × 1000 image of black and white squares, 50
pixels each (`1.0` for white), and a kernel that sets each pixel of a
second buffer to the average of its 3 × 3 neighbourhood (only the
neighbours inside the image). Print the sums before and after, and the
brightest and darkest pixels.

<details>
<summary>Solution</summary>

```tessel
kernel fn blur(_ at: (Int, Int), image: GPUBuffer<Float32>, out: GPUBuffer<Float32>, width: Int, height: Int) {
    let (x, y) = at
    var total: Float32 = 0
    var count: Float32 = 0
    for dy in -1..2 {
        for dx in -1..2 {
            let nx = x + dx
            let ny = y + dy
            if nx >= 0 && nx < width && ny >= 0 && ny < height {
                total += image[nx + width * ny]
                count += 1
            }
        }
    }
    out[x + width * y] = total / count
}

fn main() {
    let width = 1000
    let height = 1000
    var pixels: [Float32] = []
    for y in 0..height {
        for x in 0..width {
            pixels.append(if (x / 50 + y / 50) % 2 == 0 { 1.0 } else { 0.0 })
        }
    }
    let image = GPUBuffer(from: pixels)
    let out = GPUBuffer<Float32>(count: width * height)
    gpu.run(blur, over: (width, height), image, out, width, height)
    print("before: {gpu.sum(image)}, after: {gpu.sum(out)}")
    print("brightest: {gpu.max(out)}, darkest: {gpu.min(out)}")
}
```

```text
before: 500000.0, after: 500000.0001678467
brightest: 1.0, darkest: 0.0
```

The sum barely moves, as a blur only spreads the light around; the
`0.0001678467` is 32-bit rounding.

</details>

## Summary

- A **kernel** (`kernel fn`) runs once for each index, all at the same
  time, on the GPU. Its first parameter is the index: an `Int`, or
  `(Int, Int)` or `(Int, Int, Int)` for a grid or a block.
- A **`GPUBuffer`** keeps numbers on the GPU between runs. Make one with
  `count:` or `from:`; `read()`, `write`, `fill` and `copy(from:)` move
  numbers.
- **`gpu.run(kernel, over: size, values…)`** queues a run and returns at
  once; runs happen in order.
- **`gpu.sum`, `gpu.min`, `gpu.max`, `gpu.dot`** turn a buffer into one
  number, and wait for the queue.
- Kernels compute with **`Float32`** (about 7 digits) and 32-bit `Int`s
  that wrap around; sums are added carefully in 64-bit.
- Without a usable GPU, kernels run on the CPU's cores, with the same
  numbers; `TESSEL_GPU=cpu` asks for that.
- For more, see [Computing on the GPU](/guides/gpu/) and the
  [reference](/reference/stdlib/#gpu-compute).

**Next:** [25. Where to go next](/learn/21-next-steps/)
