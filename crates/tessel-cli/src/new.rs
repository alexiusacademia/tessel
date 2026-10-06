//! `tessel new <folder> [--template app|blank|cli]`: a project to start
//! from, with its `tessel.toml` (this version of Tessel, and for apps their
//! name and identifier), a README and a `.gitignore`.
//!
//! The templates are in `templates/`, with `{{…}}` for what depends on the
//! project: `{{Name}}` (the `app`'s name in the code), `{{title}}` (its name
//! for people), `{{folder}}`, `{{id}}`, `{{tessel}}`, `{{year}}`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tessel_compiler::source::SourceMap;

/// A template: what it's for, its files (name, text), and whether it's an
/// app (which gets an `[app]` table).
struct Template {
    name: &'static str,
    about: &'static str,
    files: &'static [(&'static str, &'static str)],
    app: Option<&'static str>,
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "app",
        about: "a to-do list app, in a few files: a window, a menu, a view of its own, and saving",
        files: &[
            ("main.tsl", include_str!("../templates/app/main.tsl")),
            ("task.tsl", include_str!("../templates/app/task.tsl")),
            ("views.tsl", include_str!("../templates/app/views.tsl")),
            ("README.md", include_str!("../templates/app/README.md")),
        ],
        app: Some("productivity"),
    },
    Template {
        name: "blank",
        about: "an app with one window and a button",
        files: &[
            ("main.tsl", include_str!("../templates/blank/main.tsl")),
            ("README.md", include_str!("../templates/blank/README.md")),
        ],
        app: Some("utilities"),
    },
    Template {
        name: "cli",
        about: "a command-line program",
        files: &[
            ("main.tsl", include_str!("../templates/cli/main.tsl")),
            ("README.md", include_str!("../templates/cli/README.md")),
        ],
        app: None,
    },
];

const TESSEL_TOML: &str = include_str!("../templates/common/tessel.toml");
const APP_TOML: &str = include_str!("../templates/common/app.toml");
const GITIGNORE: &str = include_str!("../templates/common/gitignore");

/// The templates, for `tessel help` and the IDE: `name  about` lines.
pub fn list() -> String {
    TEMPLATES.iter().map(|t| format!("  {:<7}{}", t.name, t.about)).collect::<Vec<_>>().join("\n")
}

/// The names a project's folder gives it.
struct Names {
    /// In the code: `MyTasks`.
    code: String,
    /// For people: `My Tasks`.
    title: String,
    /// The app's identifier: `com.example.my-tasks`.
    id: String,
    folder: String,
}

fn names(folder: &str) -> Names {
    let words: Vec<String> = folder
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect();
    let mut code: String = words.concat().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if !code.starts_with(|c: char| c.is_ascii_alphabetic()) {
        code = format!("App{code}");
    }
    let title = if words.is_empty() { code.clone() } else { words.join(" ") };
    let mut id: String = folder
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    id = id.trim_matches('-').to_string();
    if id.is_empty() {
        id = code.to_lowercase();
    }
    Names { code, title, id: format!("com.example.{id}"), folder: folder.to_string() }
}

fn fill(text: &str, n: &Names, code: &str, category: &str) -> String {
    let year = chrono_year();
    text.replace("{{Name}}", code)
        .replace("{{title}}", &n.title)
        .replace("{{folder}}", &n.folder)
        .replace("{{id}}", &n.id)
        .replace("{{tessel}}", tessel_compiler::VERSION)
        .replace("{{year}}", &year.to_string())
        .replace("{{category}}", category)
}

/// This year, from the clock (for the copyright line).
fn chrono_year() -> i64 {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Civil year from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

/// The project's files, with `code` as the app's name in the code.
fn files(t: &Template, n: &Names, code: &str) -> Vec<(String, String)> {
    let category = t.app.unwrap_or("");
    let app = t.app.map_or(String::new(), |_| fill(APP_TOML, n, code, category));
    let toml = fill(TESSEL_TOML, n, code, category).replace("{{app}}\n", &app);
    let mut out: Vec<(String, String)> = t.files.iter().map(|(f, text)| (f.to_string(), fill(text, n, code, category))).collect();
    out.push(("tessel.toml".into(), toml));
    out.push((".gitignore".into(), fill(GITIGNORE, n, code, category)));
    out
}

/// Whether the program in `files` checks without errors (its app's name
/// could be one Tessel or the template already uses, like `Text` or `Task`).
fn checks(files: &[(String, String)]) -> bool {
    let mut sources = SourceMap::new();
    for (name, text) in files.iter().filter(|(f, _)| f.ends_with(".tsl")) {
        sources.add(name.clone(), text.clone());
    }
    tessel_compiler::analyze(&mut sources, true).diags.is_empty()
}

pub fn run(args: &[String]) -> ExitCode {
    let fail = |msg: String| {
        eprintln!("tessel new: {msg}");
        ExitCode::FAILURE
    };
    let mut folder: Option<&str> = None;
    let mut template = "app";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--template" | "-t" => {
                let Some(t) = args.get(i + 1) else { return fail("`--template` needs a name: app, blank or cli".into()) };
                template = t;
                i += 1;
            }
            a if a.starts_with('-') => return fail(format!("unknown option `{a}`")),
            a if folder.is_none() => folder = Some(a),
            a => return fail(format!("one folder at a time (`{a}` too)")),
        }
        i += 1;
    }
    let Some(t) = TEMPLATES.iter().find(|t| t.name == template) else {
        return fail(format!("there's no template `{template}`; there are:\n{}", list()));
    };
    let Some(folder) = folder else {
        return fail(format!("name the project's folder, like `tessel new my-app`\n\ntemplates (`--template <name>`, `app` unless given):\n{}", list()));
    };
    let dir = PathBuf::from(folder);
    if dir.is_file() {
        return fail(format!("{} is a file", dir.display()));
    }
    if let Err(why) = usable(&dir) {
        return fail(why);
    }
    // The folder's own name (`.` is the current folder's).
    let full = std::path::absolute(&dir).unwrap_or_else(|_| dir.clone());
    let full = full.components().collect::<PathBuf>();
    let base = full.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "app".into());
    let n = names(&base);

    let mut made = files(t, &n, &n.code);
    if !checks(&made) {
        let other = format!("{}App", n.code);
        made = files(t, &n, &other);
        if !checks(&made) {
            return fail(format!("the {} template doesn't build: this is a bug in Tessel", t.name));
        }
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(format!("can't create {}: {e}", dir.display()));
    }
    let mut skipped = Vec::new();
    for (name, text) in &made {
        let path = dir.join(name);
        if path.exists() {
            skipped.push(name.clone());
            continue;
        }
        if let Err(e) = std::fs::write(&path, text) {
            return fail(format!("can't write {}: {e}", path.display()));
        }
    }
    let shown = if folder == "." { "this folder".to_string() } else { dir.display().to_string() };
    println!("Created {} in {shown}, from the {} template.", n.title, t.name);
    for name in &skipped {
        println!("  kept the {name} that was there");
    }
    let go = if folder == "." { String::new() } else { format!("cd {}\n  ", quote(folder)) };
    let run = if t.app.is_none() { "tessel run . Ana" } else { "tessel run ." };
    println!("\nNext:\n  {go}{run}\n\nor open it in the IDE with `tessel ide {}`.", quote(folder));
    ExitCode::SUCCESS
}

/// A new project goes in a new folder, or one without a program in it.
fn usable(dir: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "tsl") || p.file_name().is_some_and(|f| f == "tessel.toml") {
            return Err(format!(
                "{} already has a program ({}); give a new folder's name",
                dir.display(),
                p.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    Ok(())
}

fn quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)) { s.to_string() } else { format!("\"{s}\"") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_folders() {
        let n = names("my-tasks");
        assert_eq!((n.code.as_str(), n.title.as_str(), n.id.as_str()), ("MyTasks", "My Tasks", "com.example.my-tasks"));
        let n = names("2048 game");
        assert_eq!((n.code.as_str(), n.title.as_str()), ("App2048Game", "2048 Game"));
        assert_eq!(names("Tally").id, "com.example.tally");
    }

    #[test]
    fn every_template_builds() {
        for t in TEMPLATES {
            for folder in ["my-project", "text", "task", "Filter"] {
                let n = names(folder);
                let made = files(t, &n, &n.code);
                let ok = checks(&made) || checks(&files(t, &n, &format!("{}App", n.code)));
                assert!(ok, "{} in {folder}", t.name);
                let toml = &made.iter().find(|(f, _)| f == "tessel.toml").unwrap().1;
                assert!(!toml.contains("{{"), "{toml}");
                assert!(crate::manifest::parse_for_test(toml).is_ok(), "{toml}");
            }
        }
    }
}
