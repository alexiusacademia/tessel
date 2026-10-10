//! A project's `tessel.toml`: the version of Tessel it uses (`tessel`, see
//! `pin`) and what it says about its app (`[app]`):
//!
//! ```toml
//! tessel = "0.1.35"
//!
//! [app]
//! name = "Tally"
//! id = "com.example.tally"
//! version = "1.2.0"
//!
//! [[app.documents]]
//! name = "Tally Workbook"
//! extensions = ["tly"]
//! ```
//!
//! Builds use the app's part: the bundle's name, identifier, version and
//! document types on macOS, the details of the .exe on Windows, and what
//! `appInfo()` and the About panel tell in the app.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Read by `pin` (leniently, so a file for a newer Tessel still pins).
    #[allow(dead_code)]
    pub tessel: Option<String>,
    #[serde(default)]
    pub app: App,
    #[serde(default)]
    pub android: Android,
}

/// What an Android package needs besides `[app]` (`[android]`).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Android {
    /// The keystore the package is signed with, relative to the manifest
    /// (without one, Android's debug key signs it, which is for trying the
    /// app, not for Google Play).
    pub keystore: Option<String>,
    /// The name (alias) of the key in the keystore.
    pub key: Option<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub name: Option<String>,
    pub id: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub copyright: Option<String>,
    /// A PNG, relative to the manifest (`icon.png` next to the program
    /// otherwise).
    pub icon: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub documents: Vec<Document>,
}

/// Files the app opens (`[[app.documents]]`).
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub name: Option<String>,
    pub extensions: Vec<String>,
    /// "editor" (the default) or "viewer".
    pub role: Option<String>,
}

/// A manifest, and the file it was read from.
pub struct Found {
    pub file: PathBuf,
    pub manifest: Manifest,
}

/// The App Store's categories (`LSApplicationCategoryType`), by the names
/// a manifest uses.
const CATEGORIES: &[&str] = &[
    "business", "developer-tools", "education", "entertainment", "finance", "games", "graphics-design",
    "healthcare-fitness", "lifestyle", "medical", "music", "news", "photography", "productivity", "reference",
    "social-networking", "sports", "travel", "utilities", "video", "weather",
];

/// The nearest `tessel.toml` for a program at `path`, read and checked.
pub fn find(path: &Path) -> Result<Option<Found>, String> {
    let Some(file) = crate::pin::find(path) else { return Ok(None) };
    // (Shown from the current folder, when it's in it.)
    let cwd = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());
    let shown = cwd.and_then(|d| file.strip_prefix(d).ok().map(Path::to_path_buf)).unwrap_or_else(|| file.clone());
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", shown.display()))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", shown.display()))?;
    let problems = check(&manifest.app);
    if !problems.is_empty() {
        return Err(format!("{}: {}", shown.display(), problems.join("; ")));
    }
    Ok(Some(Found { file, manifest }))
}

/// What's wrong with an `[app]` table's values (the format itself is
/// checked as it's read).
fn check(app: &App) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(id) = &app.id {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !ok {
            out.push(format!("`id = \"{id}\"`: write it with letters, digits, `.` and `-`, like \"com.example.tally\""));
        }
    }
    if let Some(name) = &app.name {
        if name.trim().is_empty() || name.contains(['/', '\\', ':']) {
            out.push(format!("`name = \"{name}\"`: it's the app's file name too, so it can't be empty or have `/`, `\\` or `:`"));
        }
    }
    if let Some(c) = &app.category {
        let short = c.strip_prefix("public.app-category.").unwrap_or(c);
        if !CATEGORIES.contains(&short) {
            out.push(format!("`category = \"{c}\"` isn't one of {}", CATEGORIES.join(", ")));
        }
    }
    for d in &app.documents {
        if d.extensions.is_empty() {
            out.push("each `[[app.documents]]` needs `extensions`, like `extensions = [\"tly\"]`".into());
        }
        for e in &d.extensions {
            if e.is_empty() || e.starts_with('.') || e.contains(['/', ' ']) {
                out.push(format!("`\"{e}\"` isn't a file extension: write it without the dot, like \"tly\""));
            }
        }
        if let Some(r) = &d.role {
            if r != "editor" && r != "viewer" {
                out.push(format!("`role = \"{r}\"`: it's \"editor\" (the default) or \"viewer\""));
            }
        }
    }
    out
}

impl App {
    /// The App Store category, as `Info.plist` has it.
    pub fn category_type(&self) -> Option<String> {
        let c = self.category.as_deref()?;
        Some(if c.starts_with("public.app-category.") { c.to_string() } else { format!("public.app-category.{c}") })
    }
}

/// What the compiler builds into the program (`appInfo()`, the About
/// panel): the manifest's `[app]`, if there is one, and the program's name.
pub fn app_info(found: Option<&Found>, program: &Path) -> tessel_compiler::AppInfo {
    let app = found.map(|f| f.manifest.app.clone()).unwrap_or_default();
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
    // A folder's program is named after the folder; a file's, after the file.
    let program = if program.is_dir() { program.canonicalize().ok().and_then(|p| stem(&p)) } else { stem(program) };
    tessel_compiler::AppInfo {
        name: app.name,
        id: app.id,
        version: app.version,
        build: app.build,
        description: app.description,
        author: app.author,
        copyright: app.copyright,
        program,
    }
}

/// Where the app's icon is: the manifest's `icon`, relative to it.
/// The keystore `[android]` names, and the key in it.
pub fn keystore(found: Option<&Found>) -> Option<(PathBuf, Option<String>)> {
    let found = found?;
    let android = &found.manifest.android;
    let file = found.file.parent().unwrap_or(Path::new(".")).join(android.keystore.as_ref()?);
    Some((file, android.key.clone()))
}

pub fn icon_path(found: Option<&Found>) -> Option<PathBuf> {
    let found = found?;
    let icon = found.manifest.app.icon.as_ref()?;
    Some(found.file.parent().unwrap_or(Path::new(".")).join(icon))
}

/// Reads a manifest's text as a build does, for tests.
#[cfg(test)]
pub fn parse_for_test(text: &str) -> Result<Manifest, String> {
    let m: Manifest = toml::from_str(text).map_err(|e| e.to_string())?;
    let problems = check(&m.app);
    if problems.is_empty() { Ok(m) } else { Err(problems.join("; ")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Manifest, String> {
        parse_for_test(text)
    }

    #[test]
    fn reads_the_app() {
        let m = read(
            "tessel = \"0.1.35\"\n[app]\nname = \"Tally\"\nid = \"com.example.tally\"\nversion = \"1.2.0\"\ncategory = \"productivity\"\n\
             [[app.documents]]\nname = \"Workbook\"\nextensions = [\"tly\", \"tlx\"]\n",
        )
        .unwrap();
        assert_eq!(m.app.name.as_deref(), Some("Tally"));
        assert_eq!(m.app.documents[0].extensions, ["tly", "tlx"]);
        assert_eq!(m.app.category_type().as_deref(), Some("public.app-category.productivity"));
        assert!(read("tessel = \"0.1.35\"\n").unwrap().app.name.is_none());
    }

    #[test]
    fn reports_mistakes() {
        assert!(read("[app]\nnmae = \"x\"\n").unwrap_err().contains("nmae"));
        assert!(read("[app]\nid = \"com example\"\n").unwrap_err().contains("letters, digits"));
        assert!(read("[app]\ncategory = \"games2\"\n").unwrap_err().contains("isn't one of"));
        assert!(read("[[app.documents]]\nextensions = [\".tly\"]\n").unwrap_err().contains("without the dot"));
        assert!(read("[app]\nversion = 2\n").is_err());
    }
}
