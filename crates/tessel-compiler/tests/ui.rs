//! UI tests: each `tests/ui/*.tsl` file is checked, and every diagnostic must
//! match a `// error: <text>` comment on the line where it is reported (and
//! every such comment must match a diagnostic).

use std::path::Path;

use tessel_compiler::source::SourceMap;

#[test]
fn ui() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let mut failures = Vec::new();
    for path in &paths {
        // A program is a file, or a folder of files (with modules in it).
        let mut sources = SourceMap::new();
        if path.is_dir() {
            let mut files: Vec<_> = std::fs::read_dir(path).unwrap().map(|e| e.unwrap().path()).collect();
            files.retain(|p| p.extension().is_some_and(|e| e == "tsl"));
            files.sort();
            for f in files {
                let name = f.strip_prefix(&dir).unwrap().display().to_string();
                sources.add(name, std::fs::read_to_string(&f).unwrap());
            }
        } else if path.extension().is_some_and(|e| e == "tsl") {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            sources.add(name, std::fs::read_to_string(path).unwrap());
        } else {
            continue;
        }
        // (Run from the tests folder, so imports find the modules.)
        let analysis = with_dir(&dir, || tessel_compiler::analyze(&mut sources, false));

        // `// error:` comments in every file, the modules' included.
        let mut expected: Vec<(u32, usize, String)> = Vec::new();
        for (id, f) in sources.files() {
            for (i, line) in f.text.lines().enumerate() {
                if let Some((_, msg)) = line.split_once("// error: ") {
                    expected.push((id, i + 1, msg.trim().to_string()));
                }
            }
        }
        for d in &analysis.diags.list {
            let file = d.span().file & tessel_compiler::source::FILE_MASK;
            let name = sources.file(file).name.clone();
            let line = sources.file(file).line_col(d.span().lo as usize).0;
            let mut text = d.message.clone();
            text += &format!(" {} {}", d.primary.text, d.help.clone().unwrap_or_default());
            match expected.iter().position(|(f, l, msg)| *f == file && *l == line && text.contains(msg.as_str())) {
                Some(i) => {
                    expected.remove(i);
                }
                None => failures.push(format!(
                    "{name}:{line}: unexpected error: {}\n{}",
                    d.message,
                    tessel_compiler::diag::render(d, &sources, false)
                )),
            }
        }
        for (file, line, msg) in expected {
            let name = &sources.file(file).name;
            failures.push(format!("{name}:{line}: expected an error containing `{msg}`, got none"));
        }
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

/// Runs `f` with `dir` as the current folder.
fn with_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let saved = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let r = f();
    std::env::set_current_dir(saved).unwrap();
    r
}
