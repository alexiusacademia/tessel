#!/bin/sh
# Installs Tessel on macOS:
#   curl -fsSL https://storage.googleapis.com/tessel-releases/install.sh | sh
# Set TESSEL_VERSION=v0.1.0 to install a specific version.
set -eu

BASE="https://storage.googleapis.com/tessel-releases"
DIR="${TESSEL_HOME:-$HOME/.tessel}"

fail() { echo "tessel install: $*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "this script is for macOS; on Windows, use install.ps1"
case "$(uname -m)" in
  arm64) ARCH=arm64 ;;
  *) fail "Tessel currently supports Apple silicon Macs (arm64) only" ;;
esac

VERSION="${TESSEL_VERSION:-$(curl -fsSL "$BASE/latest.txt")}" || fail "can't reach $BASE"
FILE="tessel-$VERSION-macos-$ARCH.tar.gz"
echo "Installing Tessel $VERSION into $DIR/bin"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fSL --progress-bar "$BASE/$VERSION/$FILE" -o "$TMP/$FILE" || fail "can't download $FILE"
mkdir -p "$DIR/bin"
tar -xzf "$TMP/$FILE" -C "$DIR/bin"

# Put ~/.tessel/bin on PATH for new terminals.
case ":$PATH:" in
  *":$DIR/bin:"*) ;;
  *)
    PROFILE="$HOME/.zshrc"
    [ -n "${SHELL:-}" ] && [ "$(basename "$SHELL")" = "bash" ] && PROFILE="$HOME/.bash_profile"
    LINE="export PATH=\"$DIR/bin:\$PATH\""
    if ! grep -qsF "$LINE" "$PROFILE"; then
      printf '\n# Tessel\n%s\n' "$LINE" >> "$PROFILE"
      echo "Added $DIR/bin to PATH in $PROFILE (open a new terminal to use it)"
    fi
    ;;
esac

# Building apps uses the system linker from Apple's Command Line Tools.
if ! xcode-select -p >/dev/null 2>&1; then
  echo
  echo "Note: building apps needs Apple's Command Line Tools. Install them with:"
  echo "  xcode-select --install"
fi

echo
"$DIR/bin/tessel" --version
echo "Done. Try: tessel run hello.tsl, or just 'tessel' in a folder to open the IDE."
