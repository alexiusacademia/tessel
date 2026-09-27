#!/bin/sh
# Collects the MinGW-w64 files that tessel's built-in linker needs to link
# Windows apps: the C runtime start-up objects, compiler builtins, libunwind,
# the MinGW-w64 C runtime (UCRT) and the Windows import libraries the Tessel
# runtime uses. They come from llvm-mingw (https://github.com/mstorsjo/llvm-mingw),
# whose parts are freely redistributable (see its LICENSE.TXT).
#
# Usage: install/windows-sdk.sh <llvm-mingw folder> <output folder>
set -eu
MINGW="$1"
OUT="$2"
LIB="$MINGW/x86_64-w64-mingw32/lib"
mkdir -p "$OUT"
cp "$LIB/crt2.o" "$LIB/crtbegin.o" "$LIB/crtend.o" "$OUT/"
cp "$(ls "$MINGW"/lib/clang/*/lib/windows/libclang_rt.builtins-x86_64.a | head -1)" "$OUT/"
# Must match WINDOWS_LIBS in crates/tessel-cli/src/main.rs, plus the C runtime.
for name in kernel32 advapi32 bcrypt ntdll userenv ws2_32 synchronization user32 gdi32 ole32 \
    oleaut32 shell32 uuid dwmapi uxtheme imm32 comctl32 winmm dxgi d3d12 d3dcompiler \
    opengl32 propsys runtimeobject secur32 crypt32 ncrypt dbghelp pathcch \
    unwind mingw32 moldname mingwex msvcrt; do
  cp "$LIB/lib$name.a" "$OUT/"
done
cp "$MINGW/LICENSE.TXT" "$OUT/LICENSE-llvm-mingw.txt"
echo "wrote $(ls "$OUT" | wc -l | tr -d ' ') files to $OUT"
