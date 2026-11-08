#!/bin/sh
# Builds Tessel's Linux package for this computer's kind (x86-64 or ARM64),
# in containers, and checks it on a clean Ubuntu 22.04:
#
#   install/release-linux.sh <out-dir> [<cache-dir>]
#
# makes <out-dir>/tessel-<version>-linux-<arch>.tar.gz (tessel, the runtime
# library and sdk/, as the install script and `tessel update` unpack it)
# and tessel-<version>-linux-<arch>-parts.tar.gz (the runtime and sdk/,
# which `tessel build --linux` downloads on a Mac or Windows). Needs Docker.
# <cache-dir> keeps LLVM and Cargo's downloads between runs.
#
# The runtime and sdk/ are built on AlmaLinux 8 (glibc 2.28), so programs
# run on any Linux as new; `tessel` on AlmaLinux 9 (glibc 2.34), as LLVM's
# own Linux build needs.
set -eu
out="$(mkdir -p "$1" && cd "$1" && pwd)"
cache="$(mkdir -p "${2:-$HOME/.cache/tessel-release}" && cd "${2:-$HOME/.cache/tessel-release}" && pwd)"
repo="$(cd "$(dirname "$0")/.." && pwd)"
version="v$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)"
llvm_version=22.1.8
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 llvm_arch=X64 ;;
  aarch64 | arm64) arch=arm64 llvm_arch=ARM64 ;;
  *) echo "release-linux.sh: no Linux package for $(uname -m)" >&2; exit 1 ;;
esac
echo "Tessel $version for linux-$arch"

# What both build containers start with: GCC 13 and Rust.
tools='dnf -y -q install gcc-toolset-13-gcc gcc-toolset-13-gcc-c++ gcc-toolset-13-binutils git make perl-core xz tar findutils which >/dev/null &&
  curl -sSf --retry 5 --retry-all-errors --retry-delay 10 https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable >/dev/null &&
  export PATH=/opt/rh/gcc-toolset-13/root/usr/bin:${CARGO_HOME:-/root/.cargo}/bin:$PATH'

# ---- The runtime and sdk/, on glibc 2.28 ----
docker run --rm -v "$repo:/src" -v "$cache:/cache" -v "$out:/out" -w /src \
  -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target-runtime almalinux:8 sh -c "
  set -e; $tools
  sh install/linux-parts.sh /out/parts"

# ---- tessel, on glibc 2.34, with LLVM's static libraries ----
# LLVM's llvm-config: its shared library isn't there (only static ones are),
# and it names the build machine's libzstd.a by its path.
docker run --rm -v "$repo:/src" -v "$cache:/cache" -v "$out:/out" -w /src \
  -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target-tessel almalinux:9 sh -c "
  set -e; $tools
  dnf -y -q install zlib-devel libzstd-devel libxml2-devel libffi-devel >/dev/null
  llvm=/cache/llvm-$llvm_version-$llvm_arch
  if [ ! -f \$llvm/lib/libPolly.a ]; then
    rm -rf \$llvm
    name=LLVM-$llvm_version-Linux-$llvm_arch
    curl -sSfL --retry 5 --retry-all-errors --retry-delay 10 https://github.com/llvm/llvm-project/releases/download/llvmorg-$llvm_version/\$name.tar.xz | tar -xJ -C /cache \
      \$name/bin/llvm-config \$name/include/llvm \$name/include/llvm-c \$name/include/lld \
      --wildcards \"\$name/lib/libLLVM*.a\" \"\$name/lib/libPolly*.a\" \"\$name/lib/liblld*.a\"
    mv /cache/\$name \$llvm
  fi
  mkdir -p /tmp/llvm/bin && ln -s \$llvm/lib /tmp/llvm/lib && ln -s \$llvm/include /tmp/llvm/include
  cat > /tmp/llvm/bin/llvm-config <<EOF
#!/bin/sh
case \" \\\$* \" in *\" --link-shared \"*) exit 1;; esac
\$llvm/bin/llvm-config \"\\\$@\" | sed 's#[^ ]*/libzstd\\.a#-lzstd#g'
EOF
  chmod +x /tmp/llvm/bin/llvm-config
  # LLVM lists libxml2, which only its Windows-manifest code uses (Tessel
  # never does): stand-ins, so tessel doesn't need libxml2 to start.
  mkdir -p /tmp/stub && gcc -O2 -c install/xml2-stub.c -o /tmp/stub/xml2-stub.o && ar rcs /tmp/stub/libxml2.a /tmp/stub/xml2-stub.o
  LLVM_SYS_221_PREFIX=/tmp/llvm RUSTFLAGS='-L native=/tmp/stub' cargo build --release -p tessel-cli --features bundled-linker
  cp /cache/target-tessel/release/tessel /out/tessel
  strip /out/tessel"

# ---- The packages ----
rm -rf "$out/pkg"
mkdir -p "$out/pkg"
mv "$out/tessel" "$out/pkg/"
cp -R "$out/parts/libtessel_runtime.a" "$out/parts/sdk" "$out/pkg/"
cp "$repo/install/PACKAGE-README.txt" "$out/pkg/README.txt"
# (No extended attributes: macOS's tar would add its own.)
tar --no-xattrs -czf "$out/tessel-$version-linux-$arch.tar.gz" -C "$out/pkg" .
tar --no-xattrs -czf "$out/tessel-$version-linux-$arch-parts.tar.gz" -C "$out/parts" libtessel_runtime.a sdk

# ---- A check on a clean Ubuntu 22.04 (glibc 2.35): no compiler, no LLVM ----
docker run --rm -v "$repo:/src:ro" -v "$out:/out" -w /tmp ubuntu:22.04 sh -c "
  set -e
  apt-get update -qq >/dev/null && apt-get install -y -qq --no-install-recommends ca-certificates libzstd1 zlib1g >/dev/null
  mkdir -p /opt/tessel && tar -xzf /out/tessel-$version-linux-$arch.tar.gz -C /opt/tessel
  t=/opt/tessel/tessel
  \$t --version
  # Nothing beyond what every supported system has.
  ldd \$t
  if ldd \$t | grep -v -E 'linux-vdso|ld-linux|lib(c|m|dl|rt|pthread|gcc_s|stdc\+\+|z|zstd)\.so'; then echo 'tessel needs more libraries'; exit 1; fi
  \$t check /src/examples/todos
  \$t run /src/tests/run/hello.tsl
  [ \"\$(\$t run /src/tests/run/math_calls.tsl)\" = \"\$(cat /src/tests/run/math_calls.out)\" ]
  [ \"\$(\$t run /src/tests/run/data_append.tsl)\" = \"\$(cat /src/tests/run/data_append.out)\" ]
  [ \"\$(echo '20 + 22' | \$t repl)\" = 42 ]
  \$t build /src/examples/todos -o /tmp/todos
  TESSEL_SCRIPT='click field 1; type \"hello\"; click \"Add\"; tree' /tmp/todos | grep -q 'hello'
  # Its AppImage and .deb; the .deb installs, with the app in the menus.
  mkdir -p /tmp/bundle && cp -R /src/examples/todos /tmp/bundle/todos
  \$t build /tmp/bundle/todos --bundle -o /tmp/bundle/out/todos
  ls -la /tmp/bundle/out
  dpkg -i /tmp/bundle/out/*.deb >/dev/null
  ls /usr/share/applications/*.desktop
  TESSEL_SCRIPT='tree' todos | head -3
  APPIMAGE_EXTRACT_AND_RUN=1 TESSEL_SCRIPT='tree' /tmp/bundle/out/*.AppImage | head -3
  echo 'linux package checked'"
ls -la "$out"/tessel-*
