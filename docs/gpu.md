<!-- Author: Alexius Academia -->
<!-- Date: 2026-11-09 -->

# GPU compute: design note

Tessel programs run their own number-crunching on the GPU: kernels written
in Tessel, data kept in GPU buffers between runs, and a CPU fallback that
gives the same answers where there's no usable GPU. Chosen with the user
on 2026-11-09 (each the recommended option).

## The language

```tessel
kernel fn smooth(_ at: (Int, Int, Int), p: GPUBuffer<Float32>, out: GPUBuffer<Float32>, nx: Int, ny: Int) {
    let (i, j, k) = at
    let c = i + nx * (j + ny * k)
    out[c] = 0.5 * p[c] + 0.25 * (p[c - 1] + p[c + 1])
}
```

- **`kernel fn`**: a function run once for each index of a range. Its first
  parameter is the index: `Int`, `(Int, Int)` or `(Int, Int, Int)`. The
  others are `GPUBuffer<T>`s, `Float32`, `Int` and `Bool` values. It
  returns nothing.
- **The subset**: `let`/`var` of `Int`, `Float32`, `Bool`; arithmetic and
  comparisons; `if`/`else`; `for i in a..b`, `while`, `break`,
  `continue`, `return`; reading and writing buffer elements; the math
  functions (`sqrt`, `abs`, `min`, `max`, `floor`, `ceil`, `round`, `exp`,
  `log`, `pow`, `sin`, `cos`, `tan`, `atan2`, `clamp`…); conversions
  `Float32(i)`, `Int(f)`. Anything else (strings, lists, structs, calls to
  your own functions, closures, `Float`) is a compile error at its line.
- **`Float32`**: a new type in the whole language (32-bit, ~7 digits):
  arithmetic, comparisons, `Float32(x)` / `Float(x)` / `Int(x)`, printing
  (shortest form that reads back), `[Float32]`. A literal takes it from
  context; inside a kernel, a bare `0.5` is a `Float32`.
- **`Int` in a kernel is 32-bit** (GPUs have no 64-bit integers): it wraps
  past ±2³¹. Indices of buffers up to 2³¹ elements fit.

## Buffers and running

```tessel
let p = GPUBuffer<Float32>(from: start)     // [Float32] → GPU
let q = GPUBuffer<Float32>(count: n)        // zeros
gpu.run(smooth, over: (nx, ny, nz), p, q, nx, ny)
let r = gpu.dot(q, q)                       // waits
let field = q.read()                        // [Float32]
```

- `GPUBuffer<Float32>` and `GPUBuffer<Int>` (32-bit on the GPU; `read()`
  gives `[Int]`). `count`, `write(_:)`, `read()`, `fill(_:)`,
  `copy(from:)`.
- `gpu.run(kernel, over: size, args…)` queues the kernel over `0..<size`
  (each axis) and returns at once. Runs happen in order. Only `read()`, a
  reduction or `gpu.wait()` waits for the GPU.
- Reductions: `gpu.sum(b)`, `gpu.min(b)`, `gpu.max(b)`, `gpu.dot(a, b)`,
  each a `Float` (64-bit): blocks of 256 are added in 32-bit as a tree on
  the GPU, and the blocks' results in 64-bit with compensation (Kahan) on
  the CPU. That's what a conjugate-gradient solve to 1e-6 needs.
- `gpu.device` names the GPU (or `"CPU"`), `gpu.isCPU` says which.
- Out-of-range indices don't crash: an index is clamped as
  `min(u32(i), count - 1)`, so it reads or writes the last element (a
  negative one too), the same on the CPU.

## Under it

- **GPU**: each kernel compiles to WGSL, run by wgpu (Metal on macOS, DX12
  on Windows, Vulkan or OpenGL on Linux), which the runtime already has.
  With `TESSEL_GPU_CHECK=1` the CPU fallback validates each kernel's WGSL
  with naga; the run tests set it, so CI machines without a GPU check it. Scalars go in
  a uniform block; buffers are storage buffers (the device's own limit,
  ≥ 8 per kernel). Work groups of 256 (1D), 16×16 (2D), 8×8×4 (3D).
  Kernel runs are recorded into one command encoder, submitted when
  something waits (or every 64 runs).
- **CPU fallback**: the same kernel is also compiled by LLVM to native
  code, with the same 32-bit floats, and run over slabs of the range on all
  cores. Used when there's no usable GPU (a VM, CI, no driver), and always
  with `TESSEL_GPU=cpu` (tests). Answers match the GPU's within rounding
  (the order of a reduction's additions differs).
- **Precision, plainly**: kernel values are 32-bit (Metal has no 64-bit
  floats at all, so there's one precision everywhere). Reductions return a
  64-bit `Float` with the error of 32-bit sums of 256 values. The docs say
  so on the page and in the reference.
- **App Store**: Metal compute needs no entitlement; nothing else changes.

## Proof

`examples/gpu-flow`: one time step of a Riptide-like solver on an 885k-cell
grid: advection of u, v, w; a pressure Poisson solve by conjugate
gradients with a Jacobi (and multigrid) preconditioner to 1e-6 relative
residual; a VOF update. Measures GPU vs CPU fallback per step on this Mac,
how far the solve converges in 32-bit, and the water volume's drift over
1000 steps. Target: 5× the multi-core CPU path.
