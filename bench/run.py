#!/usr/bin/env python3
"""Benchmarks Tessel against C and Go on the same programs.

    python3 bench/run.py [path/to/tessel]

Builds each program (Tessel with `tessel build`, C with `clang -O2`, Go with
`go build`), checks that the three print the same result, then times each:
one warm-up run, then the median of 7. Needs clang and go on PATH.
"""
import os, shutil, statistics, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
BENCHES = ["fib", "sieve", "mandel", "nbody", "strings", "dict", "sort"]
tessel = sys.argv[1] if len(sys.argv) > 1 else shutil.which("tessel") or "tessel"
out = tempfile.mkdtemp(prefix="tessel-bench-")

def build(b):
    exes = {}
    exes["tessel"] = os.path.join(out, f"{b}-tessel")
    subprocess.run([tessel, "build", os.path.join(HERE, "tsl", f"{b}.tsl"), "-o", exes["tessel"]], check=True, stdout=subprocess.DEVNULL)
    exes["c"] = os.path.join(out, f"{b}-c")
    subprocess.run(["clang", "-O2", "-o", exes["c"], os.path.join(HERE, "c", f"{b}.c"), "-lm"], check=True)
    exes["go"] = os.path.join(out, f"{b}-go")
    subprocess.run(["go", "build", "-o", exes["go"], f"./{b}"], cwd=os.path.join(HERE, "go"), check=True)
    return exes

def median_time(exe):
    subprocess.run([exe], stdout=subprocess.DEVNULL)
    times = []
    for _ in range(7):
        t = time.perf_counter()
        subprocess.run([exe], stdout=subprocess.DEVNULL)
        times.append(time.perf_counter() - t)
    return statistics.median(times)

print(f"{'benchmark':<10}{'C':>10}{'Go':>10}{'Tessel':>10}{'vs C':>8}{'vs Go':>8}")
for b in BENCHES:
    exes = build(b)
    outputs = {l: subprocess.run([e], capture_output=True, text=True).stdout for l, e in exes.items()}
    note = "" if len(set(outputs.values())) == 1 else "  (outputs differ)"
    t = {l: median_time(e) for l, e in exes.items()}
    print(f"{b:<10}{t['c']*1000:>8.1f}ms{t['go']*1000:>8.1f}ms{t['tessel']*1000:>8.1f}ms"
          f"{t['tessel']/t['c']:>7.1f}x{t['tessel']/t['go']:>7.1f}x{note}")
