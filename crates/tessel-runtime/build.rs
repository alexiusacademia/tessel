//! Embeds the built-in icons (`icons/*.svg`, listed in `icons/names.txt`).

use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons");
    println!("cargo:rerun-if-changed={}", dir.display());
    let names = std::fs::read_to_string(dir.join("names.txt")).expect("icons/names.txt");
    let mut out = String::from("/// Built-in icons, in the order of the `IconName` enum: (name, SVG).\npub const ICONS: &[(&str, &str)] = &[\n");
    for name in names.lines().map(str::trim).filter(|n| !n.is_empty()) {
        let path = dir.join(format!("{name}.svg"));
        assert!(path.exists(), "missing icon {}", path.display());
        writeln!(out, "    ({name:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    out.push_str("];\n");
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icons.rs"), out).unwrap();
}
