//! With the `bundled-linker` feature, builds LLVM's linker (lld) into
//! `tessel`, from the LLVM that llvm-sys uses (LLVM_SYS_221_PREFIX). Release
//! builds use it so building apps needs no system linker.

use std::path::PathBuf;
use std::process::Command;

fn main() {
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
