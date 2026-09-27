# Tessel

A small, declarative, compiled language for building desktop GUI apps on
macOS and Windows.

```tessel
app Counter(width: 320, height: 200) {
    state count = 0

    VStack(spacing: 12) {
        Text("Count: {count}")
        Button("+") { count += 1 }
    }
}
```

**Status:** apps compile to native macOS executables through LLVM and run in
their own window, drawn by Tessel's own GPU renderer (`wgpu` + `vello`).
`examples/counter` and `examples/todos` both work. Windows support is
milestone M5. See [docs/design.md](docs/design.md) for the design and plan.

```tessel
fn main() {
    let names = ["Ada", "Grace", "Linus"]
    for name in names.sorted() {
        print("Hello, {name}!")
    }
}
```

## Layout

| Path | |
|---|---|
| `crates/tessel-compiler` | Lexer, parser, type checker, LLVM code generation |
| `crates/tessel-runtime` | Windows, layout, GPU rendering, event loop |
| `crates/tessel-cli` | The `tessel` command |
| `examples/` | Sample programs, one folder each (a folder is a program) |
| `tests/run/` | Programs with expected output, run by `cargo test` |
| `docs/` | Language design |

## Building

Requires Rust and LLVM 22 (`brew install llvm@22`; the path is set in
`.cargo/config.toml`).

```sh
cargo build
./target/debug/tessel check examples/todos       # a folder of .tsl files, or one file
./target/debug/tessel run examples/todos         # compile and run (opens a window)
./target/debug/tessel build app.tsl -o app        # compile to a native executable
cargo test    # unit tests, error-message fixtures, and programs in tests/run
              # (checked for memory leaks on macOS)
```

On Windows, build with `cargo build --target x86_64-pc-windows-msvc` (see
`.cargo/config.toml`) and set `LLVM_SYS_221_PREFIX` to an LLVM 22 install; the
CI workflow shows the exact steps.

Use `cargo build --release` for much smaller app binaries: `tessel build`
links against the runtime library built next to the `tessel` executable.

### Testing apps without a window

Set `TESSEL_SCRIPT` to drive an app headlessly — click, type, wait for
timers, print the view tree, or save a screenshot:

```sh
TESSEL_SCRIPT='click "+"; click "+"; tree; snapshot counter.png' ./counter
```

See `crates/tessel-runtime/src/ui/app.rs` for all commands. `tests/run/app_*`
use this.
