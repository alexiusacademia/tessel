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

**Status:** programs with a `fn main()` compile to native executables through
LLVM and run. UI apps (`app`, views) come next, in milestone M3. See
[docs/design.md](docs/design.md) for the language design and the milestone plan.

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
| `examples/` | Sample programs (`.tsl`) |
| `tests/run/` | Programs with expected output, run by `cargo test` |
| `docs/` | Language design |

## Building

Requires Rust and LLVM 22 (`brew install llvm@22`; the path is set in
`.cargo/config.toml`).

```sh
cargo build
./target/debug/tessel check examples/todos.tsl   # a file, or a folder of .tsl files
./target/debug/tessel run tests/run/basics.tsl    # compile and run
./target/debug/tessel build app.tsl -o app        # compile to a native executable
cargo test    # unit tests, error-message fixtures, and programs in tests/run
              # (checked for memory leaks on macOS)
```
