#!/bin/sh
# What Linux programs are linked with: the runtime library and, in sdk/,
# glibc's and GCC's files a program is linked against. `tessel build
# --linux` (on a Mac or Windows) downloads them; Tessel's Linux package
# has them next to `tessel`. Runs on the oldest system Linux programs are
# for, glibc 2.28 (AlmaLinux 8, with gcc-toolset and Rust), so programs
# use nothing newer:
#
#   install/linux-parts.sh <out-dir>
#
# makes <out-dir>/libtessel_runtime.a and <out-dir>/sdk/ (with the AppImage
# runtime, downloaded and checked).
set -eu
out="$1"
here="$(cd "$(dirname "$0")/.." && pwd)"
arch="$(uname -m)"
case "$arch" in
  x86_64) loader=/lib64/ld-linux-x86-64.so.2 ;;
  aarch64) loader=/lib/ld-linux-aarch64.so.1 ;;
  *) echo "linux-parts.sh: no Linux parts for $arch" >&2; exit 1 ;;
esac
glibc="$(ldd --version | head -1 | awk '{print $NF}')"
if [ "$glibc" != "2.28" ]; then
  echo "linux-parts.sh: this system has glibc $glibc; the parts are made on glibc 2.28" >&2
  exit 1
fi

(cd "$here" && cargo build -p tessel-runtime --release)
target="${CARGO_TARGET_DIR:-$here/target}"

rm -rf "$out"
mkdir -p "$out/sdk"
cp "$target/release/libtessel_runtime.a" "$out/"
# (Its debug information, and the LLVM bitcode Rust keeps for link-time
# optimization, which programs aren't linked with, would only make every
# download bigger: they're most of it.)
strip --strip-debug -R .llvmbc -R .llvmcmd "$out/libtessel_runtime.a"

# The C library: start-up code, the shared libraries (linked against, not
# copied into programs: the system's are used when they run) and the
# static part of libc.
for f in Scrt1.o crti.o crtn.o libc_nonshared.a libpthread_nonshared.a; do
  cp -L "/usr/lib64/$f" "$out/sdk/"
done
for f in libc.so.6 libm.so.6 libpthread.so.0 libdl.so.2 librt.so.1 libutil.so.1 libgcc_s.so.1; do
  cp -L "/lib64/$f" "$out/sdk/"
done
cp -L "$loader" "$out/sdk/$(basename "$loader")"
# GCC's: constructors and destructors, and helpers compiled code calls.
gcc="$(dirname "$(gcc -print-libgcc-file-name)")"
for f in crtbeginS.o crtendS.o libgcc.a; do
  cp -L "$gcc/$f" "$out/sdk/"
done
# The AppImage runtime (AppImage's type2-runtime, MIT): the program at the
# start of an AppImage, which mounts the app inside it and runs it.
runtime_release=20251108
case "$arch" in
  x86_64) runtime_sum=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
  aarch64) runtime_sum=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
esac
curl -sSfL --retry 5 --retry-all-errors --retry-delay 10 -o "$out/sdk/appimage-runtime" \
  "https://github.com/AppImage/type2-runtime/releases/download/$runtime_release/runtime-$arch"
echo "$runtime_sum  $out/sdk/appimage-runtime" | sha256sum -c - > /dev/null

cat > "$out/sdk/README.txt" <<EOF
Files Linux programs are linked against, from AlmaLinux 8 ($arch):
glibc $glibc (LGPL 2.1; programs use the system's own when they run, and
take in only libc_nonshared.a and the crt files, which the LGPL's
exceptions cover) and GCC's runtime (crtbeginS.o, crtendS.o, libgcc.a and
libgcc_s.so.1, under the GCC Runtime Library Exception); and
appimage-runtime, AppImage's type2-runtime $runtime_release (MIT), which
starts every AppImage.
EOF
echo "made $out"
