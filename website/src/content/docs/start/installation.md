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
curl -fsSL https://github.com/alexiusacademia/tessel/releases/latest/download/install.sh | sh
```

This installs Tessel into `~/.tessel/bin` and adds that folder to your
`PATH`. Open a new terminal window afterwards.

Tessel runs on Macs with Apple silicon (M1 or later), macOS 11 or newer.

## Windows

In PowerShell:

```powershell
irm https://github.com/alexiusacademia/tessel/releases/latest/download/install.ps1 | iex
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

The installers download from the repository's
[GitHub releases](https://github.com/alexiusacademia/tessel/releases). To
install by hand, download the package for your system from the newest
release:

| System | Package |
|---|---|
| macOS (Apple silicon) | `tessel-<version>-macos-arm64.tar.gz` |
| Windows (64-bit) | `tessel-<version>-windows-x64.zip` |

Unpack it into a folder and add the folder to your `PATH`. Each release
also has a `SHA256SUMS` file to check the download against. (Releases up to
0.15.1 are in `https://storage.googleapis.com/tessel-releases/<version>/`;
the installers and `tessel update` find them there too.)

On macOS, `tessel` is signed with a Developer ID and notarized by Apple, so
it runs even when downloaded with a web browser.

Keep `libtessel_runtime.a` (`tessel_runtime.lib` on Windows) in the same
folder as `tessel`: every app you build is linked with it.

## Updating

```sh
tessel update
```

Downloads the latest release and replaces your installation with it. The
download is checked against the release's checksums before anything is
changed, so an interrupted or damaged download leaves your installation as it
was. Apps you've already built aren't affected; rebuild them to use the new
version.

- `tessel update --check` only says whether a newer version exists.
- `tessel update v0.1.2` installs that version instead, newer or older, for
  example to go back after an update.

The [Tessel IDE](/start/ide/) checks for a newer version when it opens, and
shows an **Update** button in its toolbar when there is one.

`tessel update` came with Tessel 0.1.3. To get it from an older version, run
the install command above once more.

## What Tessel reports

So that installations can be counted, Tessel sends a small message to
`stats.syncster.dev`:

- when it's **installed**, and when it changes **version** (the install
  scripts and `tessel update` send this);
- **at most once a day, when it's used**: the first `tessel` command of the
  day says that this installation is in use.

Each message holds:

- a random id made on your computer the first time (kept in the `stats` file
  in Tessel's folder), which names this installation and not you;
- the version of Tessel, and the version before it on an update;
- the system (macOS or Windows) and the processor's kind (arm64 or x86_64).

Nothing else is sent: not your name or your computer's, not your files or
your code, not which commands you run or what your programs do. Your
address reaches the server with the message, as with any request on the
internet; the count doesn't record it. The programs and apps you build with Tessel send
nothing.

You're told about this once: by the install script, or by the first
`tessel` command in a terminal after an update brings it. Nothing is
reported about use before that.

To turn it off for good:

```sh
tessel telemetry off
```

`tessel telemetry` says what's reported and whether it's on, and
`tessel telemetry on` turns it back on. Setting `TESSEL_NO_STATS=1` (or
`DO_NOT_TRACK=1`) in the environment turns it off for the commands that see
it, the install command included:

```sh
curl -fsSL https://github.com/alexiusacademia/tessel/releases/latest/download/install.sh | TESSEL_NO_STATS=1 sh
```

Automated builds (where `CI` is set) report nothing.

## Installing a specific version

Set `TESSEL_VERSION` before running the installer:

```sh
curl -fsSL https://github.com/alexiusacademia/tessel/releases/latest/download/install.sh | TESSEL_VERSION=v0.1.0 sh
```

## Uninstalling

Delete `~/.tessel` (macOS) or `%LOCALAPPDATA%\Tessel` (Windows), and remove
the `PATH` line the installer added to `~/.zshrc` (macOS) or your user
`Path` variable (Windows).
