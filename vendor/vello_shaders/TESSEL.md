<!-- Author: Alexius Academia -->
<!-- Date: 2026-10-22 -->

# vello_shaders 0.10.0, patched for ARM Mali GPUs

This is the `vello_shaders` crate from crates.io (0.10.0), used in place of
it through `[patch.crates-io]` in the workspace's `Cargo.toml`. Only six
lines of its WGSL differ (`diff -r` against the published crate shows them).

## Why

On an ARM Mali GPU (tried: a Pixel 10a's Mali-G715), the first frame lost
the device (`VK_ERROR_DEVICE_LOST`), and every app stopped. See Tessel
issue #22.

Several shaders read a workgroup array at `x - 1u` inside a `select` that
only uses the value when `x > 0u`:

```wgsl
let seq_ix = ix - select(0u, sh_tile_count[el_ix - 1u], el_ix > 0u);
```

`select` evaluates both values, so for `x == 0` the index is `0u - 1u`
(4294967295). vello creates its shaders without runtime bounds checks
(`ShaderRuntimeChecks::unchecked()`), so the read really happens. Desktop
GPUs read something harmless; the Mali faults. Bisected on the phone:
`coarse` was the first step to fail, inside its binary search over
`sh_part_count`.

## The change

The index is clamped, which reads a valid element that `select` then
ignores:

```wgsl
let seq_ix = ix - select(0u, sh_tile_count[max(el_ix, 1u) - 1u], el_ix > 0u);
```

In `backdrop_dyn.wgsl`, `coarse.wgsl` (two), `fine.wgsl` (two) and
`tile_alloc.wgsl`. The other `x - 1u` reads are inside `if x > 0u`.

Measured on the Pixel 10a, scrolling a list of 300 rows: 7 ms a frame on
the GPU, against 60 ms with the earlier workaround (vello's steps before
`fine` on the processor).

Upstream: vello issue #1816 reports the same bug (Pixel 9 Pro), and pull
request #1817 makes the same change; as of 2026-10-23 it isn't merged
(a review asked for an `if` instead of `select`), and vello 0.11.0
doesn't have it. Our confirmation from the Pixel 10a is on #1816:
https://github.com/linebender/vello/issues/1816#issuecomment-6360706095

When updating vello, check whether the published shaders still read out
of bounds this way, and drop this copy if they don't.
