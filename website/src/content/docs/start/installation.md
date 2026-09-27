---
title: Installation
description: Install Tessel on macOS or Windows with one command.
---

Tessel is a single download: the `tessel` command (which includes the IDE
and its own linker) and the runtime library that apps are built with. You
don't need anything else: no Xcode, no Visual Studio, no Rust or LLVM.

## macOS

In Terminal:

```sh
curl -fsSL https://storage.googleapis.com/tessel-releases/install.sh | sh
```

This installs Tessel into `~/.tessel/bin` and adds that folder to your
`PATH`. Open a new terminal window afterwards.

Tessel runs on Macs with Apple silicon (M1 or later), macOS 11 or newer.

## Windows

In PowerShell:

```powershell
irm https://storage.googleapis.com/tessel-releases/install.ps1 | iex
```

This installs Tessel into `%LOCALAPPDATA%\Tessel\bin` and adds it to your
`PATH`. Open a new terminal afterwards.

Tessel runs on 64-bit Windows 10 and 11.

## Check that it works

```sh
tessel --version
```

Then make a file called `hello.tsl`:

```tessel
fn main() {
    print("Hello, Tessel!")
}
```

and run it:

```sh
tessel run hello.tsl
```

```
Hello, Tessel!
```

Next, [build your first app](/start/first-app/), or type `tessel` in a
folder to open [the Tessel IDE](/start/ide/).

## Manual download

The installers download from
`https://storage.googleapis.com/tessel-releases/`. To install by hand,
download the package for your system from the folder of the newest version
(the version number is in
[`latest.txt`](https://storage.googleapis.com/tessel-releases/latest.txt)):

| System | Package |
|---|---|
| macOS (Apple silicon) | `tessel-<version>-macos-arm64.tar.gz` |
| Windows (64-bit) | `tessel-<version>-windows-x64.zip` |

Unpack it into a folder and add the folder to your `PATH`. Each version's
folder also has a `SHA256SUMS` file to check the download against.

:::note
On macOS, a package downloaded with a web browser is marked as coming from
the internet, and macOS may refuse to run `tessel` because it isn't signed
yet. The install script doesn't have this problem. If it happens, remove the
mark with `xattr -dr com.apple.quarantine <folder>`.
:::

Keep `libtessel_runtime.a` (`tessel_runtime.lib` on Windows) in the same
folder as `tessel`: every app you build is linked with it.

## Installing a specific version

Set `TESSEL_VERSION` before running the installer:

```sh
curl -fsSL https://storage.googleapis.com/tessel-releases/install.sh | TESSEL_VERSION=v0.1.0 sh
```

## Uninstalling

Delete `~/.tessel` (macOS) or `%LOCALAPPDATA%\Tessel` (Windows), and remove
the `PATH` line the installer added to `~/.zshrc` (macOS) or your user
`Path` variable (Windows).

## Building from source

To work on Tessel itself, see [Building from source](/about/source/).
