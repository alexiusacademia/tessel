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

**Status:** early design. Nothing runs yet. See [docs/design.md](docs/design.md)
for the language design and the milestone plan.

## Layout

| Path | |
|---|---|
| `crates/tessel-compiler` | Lexer, parser, type checker, LLVM code generation |
| `crates/tessel-runtime` | Windows, layout, GPU rendering, event loop |
| `crates/tessel-cli` | The `tessel` command |
| `examples/` | Sample programs (`.tsl`) |
| `docs/` | Language design |

## Building

```sh
cargo build
./target/debug/tessel version
```
