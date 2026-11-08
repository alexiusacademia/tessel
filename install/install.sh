#!/bin/sh
# Installs Tessel on macOS or Linux:
#   curl -fsSL https://github.com/alexiusacademia/tessel/releases/latest/download/install.sh | sh
# Set TESSEL_VERSION=v0.1.0 to install a specific version, and
# TESSEL_NO_STATS=1 to leave the download and install uncounted.
set -eu

# Releases are GitHub's; those up to 0.15.1 are only in the old bucket.
GITHUB="https://github.com/alexiusacademia/tessel/releases"
BUCKET="https://storage.googleapis.com/tessel-releases"
DIR="${TESSEL_HOME:-$HOME/.tessel}"

fail() { echo "tessel install: $*" >&2; exit 1; }

case "$(uname -s)" in
  Darwin)
    OS=macos
    case "$(uname -m)" in
      arm64) ARCH=arm64 ;;
      *) fail "on macOS, Tessel supports Apple silicon Macs (arm64) only" ;;
    esac
    ;;
  Linux)
    OS=linux
    case "$(uname -m)" in
      x86_64 | amd64) ARCH=x64 ;;
      aarch64 | arm64) ARCH=arm64 ;;
      *) fail "on Linux, Tessel supports x86-64 and ARM64 computers" ;;
    esac
    # Tessel itself needs glibc 2.34 (Ubuntu 22.04, Debian 12, Fedora 35,
    # RHEL 9 or newer). (The programs it builds need only 2.28.)
    GLIBC="$(ldd --version 2>/dev/null | head -1 | sed 's/.* //')"
    case "$GLIBC" in
      2.[0-9] | 2.[12][0-9] | 2.3[0-3]) fail "Tessel needs glibc 2.34 or newer (Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9); this system has $GLIBC" ;;
      "") fail "Tessel needs glibc (2.34 or newer); this system has another C library (Alpine's musl?)" ;;
    esac
    ;;
  *) fail "this script is for macOS and Linux; on Windows, use install.ps1" ;;
esac

VERSION="${TESSEL_VERSION:-$(curl -fsSL "$GITHUB/latest/download/latest-$OS.txt" || { [ "$OS" = macos ] && curl -fsSL "$BUCKET/latest-macos.txt"; })}" || fail "can't reach $GITHUB"
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
FILE="tessel-$VERSION-$OS-$ARCH.tar.gz"
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
  curl -fsS -m 5 -o /dev/null "$PING?app=tessel&type=download&v=$V&os=$OS" || true
  ID="$(kept id)"
  OLD="$(kept reported)"
  [ -n "$ID" ] || ID="$( (uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null) | tr 'A-Z' 'a-z')"
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
  if curl -fsS -m 5 -o /dev/null "$PING?app=tessel&type=$KIND&id=$ID&v=$V&os=$OS&arch=$ARCH$PREV"; then
    save "$V"
  fi
}
count 2>/dev/null || true

# Put ~/.tessel/bin on PATH for new terminals.
case ":$PATH:" in
  *":$DIR/bin:"*) ;;
  *)
    # The shell's startup file: zsh's; bash's (.bash_profile on macOS,
    # .bashrc on Linux); otherwise .profile.
    case "$(basename "${SHELL:-sh}")" in
      zsh) PROFILE="$HOME/.zshrc" ;;
      bash) if [ "$OS" = macos ]; then PROFILE="$HOME/.bash_profile"; else PROFILE="$HOME/.bashrc"; fi ;;
      *) if [ "$OS" = macos ]; then PROFILE="$HOME/.zshrc"; else PROFILE="$HOME/.profile"; fi ;;
    esac
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
