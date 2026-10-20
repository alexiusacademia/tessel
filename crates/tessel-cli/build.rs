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

/// The documentation that comes with `tessel` (for the IDE's Docs panel):
/// the website's pages as plain Markdown, their pictures, and the contents
/// (`index.txt`). Lists them in `docs.rs` as (path, bytes).
fn docs() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let website = root.join("website");
    let pages = website.join("src/content/docs");
    let assets = website.join("src/assets");
    let config = website.join("astro.config.mjs");
    let icons = root.join("crates/tessel-runtime/icons/names.txt");
    for watched in [&pages, &assets, &config, &icons] {
        println!("cargo:rerun-if-changed={}", watched.display());
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("docs");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    // (path in the docs folder, the file with its bytes)
    let mut files: Vec<(String, PathBuf)> = Vec::new();

    // The contents, from the website's sidebar: groups and their pages.
    let quoted = |line: &str, key: &str| -> Option<String> {
        let rest = &line[line.find(key)? + key.len()..];
        let rest = &rest[rest.find('\'')? + 1..];
        Some(rest[..rest.find('\'')?].to_string())
    };
    let sidebar = std::fs::read_to_string(&config).unwrap();
    let sidebar = &sidebar[sidebar.find("sidebar:").expect("the sidebar in astro.config.mjs")..];
    let outer = sidebar.lines().filter(|l| l.contains("label:")).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    let mut index = String::new();
    let icon_names = std::fs::read_to_string(&icons).unwrap();
    for line in sidebar.lines() {
        let Some(label) = quoted(line, "label:") else { continue };
        match quoted(line, "slug:") {
            Some(slug) => {
                let source = [format!("{slug}.md"), format!("{slug}.mdx"), format!("{slug}/index.md"), format!("{slug}/index.mdx")]
                    .iter()
                    .map(|name| pages.join(name))
                    .find(|p| p.is_file())
                    .unwrap_or_else(|| panic!("no page for `{slug}`"));
                let text = page(&std::fs::read_to_string(&source).unwrap(), &icon_names);
                let file = out.join(format!("{slug}.md"));
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, text).unwrap();
                files.push((format!("{slug}.md"), file));
                index += &format!("page\t{slug}\t{label}\n");
            }
            // A group; one inside another is a level deeper.
            None => {
                let level = if line.len() - line.trim_start().len() > outer { 2 } else { 1 };
                index += &format!("group\t{level}\t{label}\n");
            }
        }
    }
    std::fs::write(out.join("index.txt"), index).unwrap();
    files.push(("index.txt".into(), out.join("index.txt")));

    fn pictures(dir: &std::path::Path, under: &str, files: &mut Vec<(String, PathBuf)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.is_dir() {
                pictures(&path, &format!("{under}/{name}"), files);
            } else if name.ends_with(".png") || name.ends_with(".jpg") {
                files.push((format!("{under}/{name}"), path));
            }
        }
    }
    pictures(&assets, "images", &mut files);

    let mut list = String::from("/// The documentation's files: (path, bytes).\nstatic DOCS: &[(&str, &[u8])] = &[\n");
    for (name, file) in &files {
        list += &format!("    ({name:?}, include_bytes!({:?})),\n", file.canonicalize().unwrap().display().to_string());
    }
    list += "];\n";
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("docs.rs"), list).unwrap();
}

/// A page of the website as plain Markdown: its title as a heading, its
/// notes and its hidden solutions as quotes, its pictures by their place in
/// the docs folder, and without what only the website can show.
fn page(source: &str, icon_names: &str) -> String {
    let mut out = String::new();
    let mut lines = source.lines().peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
            if let Some(title) = line.strip_prefix("title:") {
                out += &format!("# {}\n\n", title.trim().trim_matches(['"', '\'']));
            }
        }
    }
    // Inside a note or a solution: each line is quoted.
    let mut quoted = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            continue;
        }
        // The icon gallery: the icons' names.
        if trimmed.starts_with("<IconGrid") {
            let names: Vec<String> = icon_names.lines().map(str::trim).filter(|n| !n.is_empty()).map(|n| format!("`{n}`")).collect();
            out += &names.join(" ");
            out.push('\n');
            continue;
        }
        if let Some(note) = trimmed.strip_prefix(":::").filter(|n| !n.is_empty()) {
            let (kind, title) = match note.split_once('[') {
                Some((kind, title)) => (kind, title.trim_end_matches(']').to_string()),
                None => (note, String::new()),
            };
            let title = if title.is_empty() { format!("{}{}", kind[..1].to_uppercase(), &kind[1..]) } else { title };
            out += &format!("> **{title}**\n>\n");
            quoted = true;
            continue;
        }
        if trimmed == ":::" || trimmed == "</details>" {
            quoted = false;
            out.push('\n');
            continue;
        }
        if trimmed == "<details>" {
            quoted = true;
            continue;
        }
        if let Some(summary) = trimmed.strip_prefix("<summary>") {
            out += &format!("> **{}**\n>\n", summary.trim_end_matches("</summary>"));
            continue;
        }
        // `![…](../../../assets/screens/x.png)` → `![…](images/screens/x.png)`
        let mut line = line.to_string();
        while let Some(at) = line.find("](../") {
            let rest = &line[at + 2..];
            let ups = rest.len() - rest.trim_start_matches("../").len();
            if !rest[ups..].starts_with("assets/") {
                break;
            }
            line.replace_range(at + 2..at + 2 + ups + "assets/".len(), "images/");
        }
        if quoted {
            out += "> ";
        }
        out += &line;
        out.push('\n');
    }
    out
}

fn main() {
    docs();
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
        // (And Windows's, for `tessel build --windows`.)
        build.define("TESSEL_LLD_MACHO", None);
        build.define("TESSEL_LLD_COFF", None);
        &["lldMachO", "lldCOFF", "lldMinGW", "lldCommon"]
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
