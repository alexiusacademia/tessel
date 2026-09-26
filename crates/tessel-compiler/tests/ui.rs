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
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "tsl")) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(path).unwrap();
        let mut expected: Vec<(usize, String)> = text
            .lines()
            .enumerate()
            .filter_map(|(i, line)| line.split_once("// error: ").map(|(_, msg)| (i + 1, msg.trim().to_string())))
            .collect();

        let mut sources = SourceMap::new();
        let file = sources.add(name.clone(), text);
        let analysis = tessel_compiler::analyze(&sources, false);

        for d in &analysis.diags.list {
            let line = sources.file(file).line_col(d.span().lo as usize).0;
            let mut text = d.message.clone();
            text += &format!(" {} {}", d.primary.text, d.help.clone().unwrap_or_default());
            match expected.iter().position(|(l, msg)| *l == line && text.contains(msg.as_str())) {
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
        for (line, msg) in expected {
            failures.push(format!("{name}:{line}: expected an error containing `{msg}`, got none"));
        }
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}
