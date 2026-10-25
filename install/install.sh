#!/bin/sh
# Installs Tessel on macOS:
#   curl -fsSL https://github.com/alexiusacademia/tessel/releases/latest/download/install.sh | sh
# Set TESSEL_VERSION=v0.1.0 to install a specific version, and
# TESSEL_NO_STATS=1 to leave the download and install uncounted.
set -eu

# Releases are GitHub's; those up to 0.15.1 are only in the old bucket.
GITHUB="https://github.com/alexiusacademia/tessel/releases"
BUCKET="https://storage.googleapis.com/tessel-releases"
DIR="${TESSEL_HOME:-$HOME/.tessel}"

fail() { echo "tessel install: $*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "this script is for macOS; on Windows, use install.ps1"
case "$(uname -m)" in
  arm64) ARCH=arm64 ;;
  *) fail "Tessel currently supports Apple silicon Macs (arm64) only" ;;
esac

VERSION="${TESSEL_VERSION:-$(curl -fsSL "$GITHUB/latest/download/latest-macos.txt" || curl -fsSL "$BUCKET/latest-macos.txt")}" || fail "can't reach $GITHUB"
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
FILE="tessel-$VERSION-macos-$ARCH.tar.gz"
echo "Installing Tessel $VERSION into $DIR/bin"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fSL --progress-bar "$GITHUB/download/$VERSION/$FILE" -o "$TMP/$FILE" 2>/dev/null ||
  curl -fSL --progress-bar "$BUCKET/$VERSION/$FILE" -o "$TMP/$FILE" || fail "can't download $FILE"
mkdir -p "$DIR/bin"
tar -xzf "$TMP/$FILE" -C "$DIR/bin"

# Counts the download and the install (or the update, over an earlier
# install): the version, the system, and a random id kept in $DIR/stats, and
# nothing else. See "What Tessel reports" in the installation guide. It
# never holds up or fails the install. `tessel` reads and writes the same
# file (its `telemetry` says what's in it), so its other lines are kept.
TOLD=""
count() {
  [ -z "${TESSEL_NO_STATS:-}" ] || return 0
  case "${DO_NOT_TRACK:-0}" in 0) ;; *) return 0 ;; esac
  STATS="$DIR/stats"
  kept() { sed -n "s/^$1=//p" "$STATS" 2>/dev/null | head -1; }
  # (Turned off with `tessel telemetry off`: it stays off.)
  [ "$(kept off)" != 1 ] || return 0
  save() {
    { grep -v -e '^id=' -e '^reported=' -e '^noticed=' "$STATS" 2>/dev/null || true
      printf 'id=%s\nreported=%s\nnoticed=1\n' "$ID" "$1"; } > "$STATS.new" && mv "$STATS.new" "$STATS"
  }
  PING="${TESSEL_STATS_URL:-https://stats.syncster.dev/v1/ping}"
  V="${VERSION#v}"
  TOLD=1
  curl -fsS -m 5 -o /dev/null "$PING?app=tessel&type=download&v=$V&os=macos" || true
  ID="$(kept id)"
  OLD="$(kept reported)"
  [ -n "$ID" ] || ID="$(uuidgen | tr 'A-Z' 'a-z')"
  [ -n "$ID" ] || return 0
  KIND=install
  PREV=""
  if [ -n "$OLD" ]; then
    [ "$OLD" != "$V" ] || { save "$OLD"; return 0; }
    KIND=update
    PREV="&prev=$OLD"
  fi
  save "$OLD" || return 0
  # The version is noted only once the count has taken it, so one that
  # didn't arrive is sent by the next install or `tessel update`.
  if curl -fsS -m 5 -o /dev/null "$PING?app=tessel&type=$KIND&id=$ID&v=$V&os=macos&arch=$ARCH$PREV"; then
    save "$V"
  fi
}
count 2>/dev/null || true

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

echo
"$DIR/bin/tessel" --version
echo "Done. Try: tessel run hello.tsl, or just 'tessel' in a folder to open the IDE."
if [ -n "$TOLD" ]; then
  echo
  echo "Tessel reports that it was installed, and at most once a day that it's in use:"
  echo "its version, your system, and a random id for this installation. Nothing about"
  echo "you, your files or your code. 'tessel telemetry off' turns it off. More:"
  echo "https://tessel-lang.syncster.dev/start/installation/#what-tessel-reports"
fi
