# Benchmarks

The same seven programs in Tessel (`tsl/`), C (`c/`) and Go (`go/`):

| Program | Measures |
|---|---|
| fib | function calls: recursive `fib(38)` |
| sieve | reading and writing list items: primes below 20 million |
| mandel | floating-point math: a 1200×1200 Mandelbrot set |
| nbody | structs in a list: 5 bodies, 1 million steps |
| strings | building, joining and splitting text: 1 million strings |
| dict | dictionary inserts and lookups: 1 million keys |
| sort | sorting 1 million integers |

```sh
python3 bench/run.py              # the tessel on PATH
python3 bench/run.py target/release/tessel
```

Each program prints a result, and the runner checks that all three agree.
(`mandel` differs by 2 points out of 1.44 million: clang and Go fuse `x*y + z`
into one multiply-add instruction, which rounds differently; Tessel rounds
each operation separately, like C with `-ffp-contract=off`.)
