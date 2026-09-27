---
title: Building from source
description: Build the tessel command yourself, for working on Tessel's compiler and runtime.
---

Most people should [install a release](/start/installation/). Build Tessel
from source if you want to work on the compiler, the runtime or the IDE, or
try changes that aren't released yet.

Tessel's compiler is written in Rust and uses LLVM, so building it needs
**Rust** and **LLVM 22**. (Programs written *in* Tessel never need either.)

## macOS

1. **Install the command line developer tools**, if you haven't already.
   Tessel uses the system's `cc` to link your apps.

   ```sh
   xcode-select --install
   ```

2. **Install Rust** from [rustup.rs](https://rustup.rs). Tessel uses the Rust
   2024 edition, so an up-to-date stable Rust is needed.

3. **Install LLVM 22** with [Homebrew](https://brew.sh):

   ```sh
   brew install llvm@22
   ```

   The repository's `.cargo/config.toml` points the build at
   `/opt/homebrew/opt/llvm@22`, which is where Homebrew puts it on Apple
   silicon Macs. If yours is somewhere else (for example on an Intel Mac),
   set `LLVM_SYS_221_PREFIX` before building:

   ```sh
   export LLVM_SYS_221_PREFIX="$(brew --prefix llvm@22)"
   ```

4. **Get the source and build it:**

   ```sh
   git clone https://github.com/alexiusacademia/tessel
   cd tessel
   cargo build --release
   ```

   This builds two things in `target/release/`: the `tessel` command, and
   `libtessel_runtime.a`, the runtime library that every Tessel app is
   linked with.

5. **Put `tessel` on your `PATH`.** Add the `target/release` folder itself
   to your `PATH`, for example in `~/.zshrc`:

   ```sh
   export PATH="$HOME/path/to/tessel/target/release:$PATH"
   ```

:::caution
`tessel build` looks for the runtime library in the same folder as the
`tessel` executable. A symbolic link to `tessel` (for example in
`/usr/local/bin`) works, but if you copy `tessel` somewhere else, copy
`libtessel_runtime.a` along with it.

The `tessel` command also loads LLVM when it runs, so keep LLVM installed.
The apps you build don't need LLVM or anything else from this setup.
:::

## Windows

Building on Windows follows the same steps as Tessel's CI, which builds and
tests every change on Windows.

1. **Install Visual Studio Build Tools** with the "Desktop development with
   C++" workload. Rust needs it, and Tessel uses its linker (`link.exe`) to
   link your apps.

2. **Install Rust** from [rustup.rs](https://rustup.rs).

3. **Install LLVM 22.** Download the prebuilt archive
   `clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz` from the
   [LLVM releases page](https://github.com/llvm/llvm-project/releases) and
   unpack it, for example into `C:\llvm`. This archive is what Tessel's CI
   uses; it includes the libraries and `llvm-config` that building against
   LLVM needs, which the regular LLVM Windows installer leaves out.

4. **Tell the build where LLVM is** by setting `LLVM_SYS_221_PREFIX` to the
   unpacked folder, in PowerShell:

   ```sh
   $env:LLVM_SYS_221_PREFIX = "C:\llvm\clang+llvm-22.1.8-x86_64-pc-windows-msvc"
   ```

5. **Build for the `x86_64-pc-windows-msvc` target:**

   ```sh
   git clone https://github.com/alexiusacademia/tessel
   cd tessel
   cargo build --release --target x86_64-pc-windows-msvc
   ```

   Naming the target explicitly matters. LLVM's Windows build uses the
   static C runtime, and `.cargo/config.toml` makes Tessel use it too, but
   only when a target is given. A side benefit: Tessel apps need no extra C
   runtime DLLs.

   The result is in `target\x86_64-pc-windows-msvc\release\`: `tessel.exe`
   and `tessel_runtime.lib`. Add that folder to your `PATH`.

:::note
Some LLVM release archives list a library called `xml2s.lib` that they don't
include, and linking then fails with a missing `xml2s.lib`. Tessel never uses
the part of LLVM that needs it, so an empty stand-in library is enough. The
CI workflow (`.github/workflows/ci.yml`) shows how it creates one with
`clang-cl` and `llvm-lib` from the same LLVM folder.
:::

## Check your build

```sh
tessel check examples/counter
tessel run examples/counter
```

## Release or debug build?

You can build Tessel with a plain `cargo build` too; the result goes to
`target/debug/`. It works the same, but use the release build for real use:
`tessel build` links your app with the runtime library that sits next to the
`tessel` executable, so a release `tessel` makes much smaller and faster
apps. On macOS, the todo example is about 12 MB when built with a release
`tessel`, and about 55 MB with a debug one.

## How releases are built

Release packages are built by `.github/workflows/release.yml` when a version
tag is pushed. It links LLVM into `tessel` statically (from LLVM's official
static build), so the packaged `tessel` needs no LLVM, Rust or Homebrew,
and uploads the packages to Google Cloud Storage.
