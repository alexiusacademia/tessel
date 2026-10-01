//! With the `bundled-linker` feature, builds LLVM's linker (lld) into
//! `tessel`, from the LLVM that llvm-sys uses (LLVM_SYS_221_PREFIX). Release
//! builds use it so building apps needs no system linker.

use std::path::PathBuf;
use std::process::Command;

/// Lists the runtime's functions by name (`runtime_symbols.rs`), so the code
/// `tessel repl` compiles can be linked to them inside `tessel`.
fn runtime_symbols() {
    fn scan(dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, names);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for (at, _) in text.match_indices("extern \"C\" fn tsl_") {
                    let name = &text[at + "extern \"C\" fn ".len()..];
                    let end = name.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(name.len());
                    names.insert(name[..end].to_string());
                }
            }
        }
    }
    let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../tessel-runtime/src");
    println!("cargo:rerun-if-changed={}", source.display());
    let mut names = std::collections::BTreeSet::new();
    scan(&source, &mut names);
    let mut out = String::from("unsafe extern \"C\" {\n");
    for name in &names {
        out += &format!("    fn {name}();\n");
    }
    out += "}\n\n/// The runtime's functions: (name, address).\nstatic RUNTIME_SYMBOLS: &[(&str, unsafe extern \"C\" fn())] = &[\n";
    for name in &names {
        out += &format!("    ({name:?}, {name}),\n");
    }
    out += "];\n";
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("runtime_symbols.rs"), out).unwrap();
}

fn main() {
    runtime_symbols();
    println!("cargo:rerun-if-changed=src/lld.cpp");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    if std::env::var_os("CARGO_FEATURE_BUNDLED_LINKER").is_none() {
        return;
    }
    let prefix = PathBuf::from(std::env::var("LLVM_SYS_221_PREFIX").expect("bundled-linker needs LLVM_SYS_221_PREFIX"));
    let config = prefix.join("bin").join(if cfg!(windows) { "llvm-config.exe" } else { "llvm-config" });
    let cxxflags = Command::new(&config).arg("--cxxflags").output().expect("can't run llvm-config").stdout;
    let cxxflags = String::from_utf8(cxxflags).unwrap();

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let mut build = cc::Build::new();
    build.cpp(true).file("src/lld.cpp").warnings(false);
    for flag in cxxflags.split_whitespace() {
        build.flag(flag);
    }
    let libs: &[&str] = if target_os == "macos" {
        build.define("TESSEL_LLD_MACHO", None);
        &["lldMachO", "lldCommon"]
    } else {
        build.define("TESSEL_LLD_COFF", None);
        &["lldCOFF", "lldMinGW", "lldCommon"]
    };
    build.compile("tessel_lld");
    println!("cargo:rustc-link-search=native={}", prefix.join("lib").display());
    for lib in libs {
        println!("cargo:rustc-link-lib=static={lib}");
    }
}
