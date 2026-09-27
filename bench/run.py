#!/usr/bin/env python3
"""Benchmarks Tessel against C, Go, Swift and Python on the same programs.

    python3 bench/run.py [path/to/tessel]

Builds each program (Tessel with `tessel build`, C with `clang -O2`, Go with
`go build`, Swift with `swiftc -O`), checks that all print the same result,
then times each: one warm-up run, then the median of 7 (3 for Python, which
is much slower). Languages whose tools aren't installed are skipped. Python
runs with the newest `python3` found (set PYTHON to choose).
"""
import os, shutil, statistics, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
BENCHES = ["fib", "sieve", "mandel", "nbody", "strings", "dict", "sort"]
tessel = sys.argv[1] if len(sys.argv) > 1 else shutil.which("tessel") or "tessel"
python = os.environ.get("PYTHON") or shutil.which("python3", path="/opt/homebrew/bin:/usr/local/bin") or sys.executable
out = tempfile.mkdtemp(prefix="tessel-bench-")

def build(lang, b):
    exe = os.path.join(out, f"{b}-{lang}")
    src = lambda d, ext: os.path.join(HERE, d, f"{b}.{ext}")
    if lang == "tessel":
        subprocess.run([tessel, "build", src("tsl", "tsl"), "-o", exe], check=True, stdout=subprocess.DEVNULL)
    elif lang == "c":
        subprocess.run(["clang", "-O2", "-o", exe, src("c", "c"), "-lm"], check=True)
    elif lang == "go":
        subprocess.run(["go", "build", "-o", exe, f"./{b}"], cwd=os.path.join(HERE, "go"), check=True)
    elif lang == "swift":
        subprocess.run(["swiftc", "-O", "-o", exe, src("swift", "swift")], check=True, stderr=subprocess.DEVNULL)
    elif lang == "python":
        return [python, src("py", "py")]
    return [exe]

def median_time(cmd, runs):
    subprocess.run(cmd, stdout=subprocess.DEVNULL)
    times = []
    for _ in range(runs):
        t = time.perf_counter()
        subprocess.run(cmd, stdout=subprocess.DEVNULL)
        times.append(time.perf_counter() - t)
    return statistics.median(times)

tools = {"c": "clang", "go": "go", "swift": "swiftc", "tessel": tessel, "python": python}
langs = [l for l in ["c", "go", "swift", "tessel", "python"] if shutil.which(tools[l]) or os.path.exists(tools[l])]
names = {"c": "C", "go": "Go", "swift": "Swift", "tessel": "Tessel", "python": "Python"}
print(f"{'benchmark':<10}" + "".join(f"{names[l]:>11}" for l in langs))
for b in BENCHES:
    cmds = {l: build(l, b) for l in langs}
    outputs = {l: subprocess.run(c, capture_output=True, text=True).stdout for l, c in cmds.items()}
    differ = sorted(l for l in langs if outputs[l] != outputs[langs[-1]])
    t = {l: median_time(c, 3 if l == "python" else 7) for l, c in cmds.items()}
    row = "".join(f"{t[l] * 1000:>9.0f}ms" for l in langs)
    print(f"{b:<10}{row}" + (f"  (different result: {', '.join(names[l] for l in differ)})" if differ else ""), flush=True)
