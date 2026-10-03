#!/usr/bin/env python3
"""Generates the macOS link stubs that ship with Tessel.

`tessel build` links apps with its built-in copy of LLVM's linker (lld), so
it doesn't need Apple's Command Line Tools. A linker still needs to know which
system library provides each symbol an app uses. Instead of shipping Apple's
SDK, we describe only what Tessel apps can use: every symbol the runtime
library imports from macOS, found by linking the whole runtime with Apple's
linker, plus the C library functions compiled Tessel code may call. The
result is a small "SDK" of text stubs (.tbd files) with the same install
names and versions as the real libraries.

Usage (on a Mac with Xcode or the Command Line Tools):
    install/macos-stubs.py <libtessel_runtime.a> <output folder>
"""

import os
import re
import subprocess
import sys
import tempfile

# Must match platform_link_args() in crates/tessel-cli/src/main.rs.
FRAMEWORKS = [
    "AppKit", "ApplicationServices", "Carbon", "CoreFoundation", "CoreGraphics", "CoreText", "CoreVideo",
    "Foundation", "ImageIO", "Metal", "QuartzCore",
]
LIBS = ["objc", "iconv"]
MIN_MACOS = "11.0"

# C library functions LLVM may call from compiled Tessel code (floating-point
# remainder, memory copies, ...), whether or not the runtime uses them. LLVM
# also turns some calls into others: pow(10, x) into __exp10(x), and sin and
# cos of one value into __sincos_stret (tests/run/math_calls.tsl has them).
LIBSYSTEM_EXTRA = """
_abort _bcmp _bzero _calloc _ceil _cos _exp _exp2 _floor _fmod _fmodf _free _log _log10 _log2
_malloc _memcmp _memcpy _memmove _memset _pow _realloc _round _sin _sqrt _tan _trunc
_acos _asin _atan _atan2 ___exp10 ___exp10f ___sincos_stret ___sincosf_stret
___bzero ___stack_chk_fail ___stack_chk_guard __tlv_bootstrap _dyld_stub_binder
""".split()


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def main():
    runtime, out = sys.argv[1], sys.argv[2]
    with tempfile.TemporaryDirectory() as tmp:
        # A program containing the whole runtime, so every symbol it could use is imported.
        main_c = os.path.join(tmp, "main.c")
        with open(main_c, "w") as f:
            f.write("int main(void) { return 0; }\n")
        exe = os.path.join(tmp, "all")
        args = ["cc", main_c, "-Wl,-all_load", runtime, f"-mmacosx-version-min={MIN_MACOS}", "-o", exe]
        for fw in FRAMEWORKS:
            args += ["-framework", fw]
        args += [f"-l{lib}" for lib in LIBS]
        subprocess.run(args, check=True)

        # Install names and versions of every library the program loads.
        libs = {}  # short name -> (install name, compatibility version, current version)
        for line in run("otool", "-L", exe).splitlines()[1:]:
            m = re.match(r"\s+(\S+) \(compatibility version ([\d.]+), current version ([\d.]+)\)", line)
            if m:
                path = m.group(1)
                libs[short_name(path)] = (path, m.group(2), m.group(3))

        # Each imported symbol and the library it comes from.
        imports = {name: set() for name in libs}
        for line in run("dyld_info", "-imports", exe).splitlines():
            m = re.match(r"\s+(\S+)\s+\(from (\S+)\)", line)
            if m:
                imports.setdefault(m.group(2), set()).add(m.group(1))
        imports["libSystem"].update(LIBSYSTEM_EXTRA)

    sdk_version = run("xcrun", "--show-sdk-version").strip()
    for name, (path, compat, current) in libs.items():
        if path.startswith("/System/Library/Frameworks/"):
            # Frameworks are found as <Name>.framework/<Name>.tbd.
            stub = os.path.join(out, "System/Library/Frameworks", f"{name}.framework", f"{name}.tbd")
        else:
            stub = os.path.join(out, "usr/lib", f"lib{name[3:] if name.startswith('lib') else name}.tbd")
        os.makedirs(os.path.dirname(stub), exist_ok=True)
        with open(stub, "w") as f:
            f.write(tbd(path, compat, current, imports.get(name, set())))
    with open(os.path.join(out, "SDKVersion"), "w") as f:
        f.write(sdk_version + "\n")
    total = sum(len(s) for s in imports.values())
    print(f"wrote stubs for {len(libs)} libraries ({total} symbols), SDK {sdk_version}, to {out}")


def short_name(path):
    """/System/.../AppKit.framework/Versions/C/AppKit -> AppKit; /usr/lib/libobjc.A.dylib -> libobjc."""
    base = os.path.basename(path)
    return base.split(".")[0]


def tbd(path, compat, current, symbols):
    classes, eh_types, ivars, plain = set(), set(), set(), set()
    for s in symbols:
        if s.startswith(("_OBJC_CLASS_$_", "_OBJC_METACLASS_$_")):
            classes.add(s.split("$_", 1)[1])
        elif s.startswith("_OBJC_EHTYPE_$_"):
            eh_types.add(s.split("$_", 1)[1])
        elif s.startswith("_OBJC_IVAR_$_"):
            ivars.add(s.split("$_", 1)[1])
        else:
            plain.add(s)
    lines = [
        "--- !tapi-tbd",
        "tbd-version:     4",
        "targets:         [ arm64-macos ]",
        f"install-name:    '{path}'",
        f"current-version: {current}",
        f"compatibility-version: {compat}",
    ]
    if symbols:
        lines += ["exports:", "  - targets:         [ arm64-macos ]"]
        for key, items in [("symbols", plain), ("objc-classes", classes), ("objc-eh-types", eh_types),
                           ("objc-ivars", ivars)]:
            if items:
                quoted = ", ".join(f"'{s}'" for s in sorted(items))
                lines.append(f"    {key}: [ {quoted} ]")
    lines.append("...")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
