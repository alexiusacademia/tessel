//! The index of published libraries: a git repository on GitHub with one
//! file per published version, `packages/<name>/<version>.toml`, saying
//! where its code is and what it depends on. Tessel keeps a copy in its
//! folder (`index/`), and downloads it again when it's asked for something
//! the copy doesn't have.
//!
//! `TESSEL_INDEX` names another index: a folder on this computer (for
//! tests and for trying a library out) or the URL of a `.tar.gz` of one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::version::Version;

/// The repository of the index, where `tessel publish` proposes versions.
pub const REPO: &str = "alexiusacademia/tessel-packages";

/// One published version of a library.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    /// Where its code is: `https://github.com/owner/name` (or another git
    /// host).
    pub repository: String,
    /// The git tag it was published from, like `v1.2.0`.
    pub tag: String,
    /// The commit the tag named, so that moving the tag changes nothing.
    pub commit: String,
    /// See `fetch::checksum`.
    pub checksum: String,
    /// The libraries it uses, with the versions it asks for.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

impl Entry {
    pub fn version(&self) -> Version {
        Version::parse(&self.version).unwrap_or(Version { major: 0, minor: 0, patch: 0 })
    }

    /// As it's written in the index.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

/// Where the index's files are: the folder `TESSEL_INDEX` names, or the
/// downloaded copy.
fn folder() -> Result<PathBuf, String> {
    match std::env::var("TESSEL_INDEX") {
        Ok(v) if !v.contains("://") && !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Ok(crate::update::home()?.join("index")),
    }
}

fn source_url() -> String {
    match std::env::var("TESSEL_INDEX") {
        Ok(v) if v.contains("://") => v,
        _ => format!("https://codeload.github.com/{REPO}/tar.gz/refs/heads/main"),
    }
}

/// The index, as known to this `tessel`.
pub struct Index {
    dir: PathBuf,
    /// Downloaded during this command already.
    fresh: bool,
}

impl Index {
    pub fn open() -> Result<Index, String> {
        Ok(Index { dir: folder()?, fresh: false })
    }

    /// Downloads the index again (once per command); a folder given with
    /// `TESSEL_INDEX` is always up to date.
    pub fn refresh(&mut self) -> Result<(), String> {
        if self.fresh {
            return Ok(());
        }
        self.fresh = true;
        if std::env::var("TESSEL_INDEX").is_ok_and(|v| !v.contains("://") && !v.is_empty()) {
            return Ok(());
        }
        let url = source_url();
        eprintln!("Updating the library index");
        let bytes = super::fetch::get(&url)?;
        let parent = self.dir.parent().unwrap_or(Path::new(".")).to_path_buf();
        let staging = parent.join(format!(".index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| format!("can't write {}: {e}", staging.display()))?;
        std::fs::write(staging.join("index.tar.gz"), bytes).map_err(|e| e.to_string())?;
        let ok = Command::new("tar")
            .args(["-xzf", "index.tar.gz"])
            .current_dir(&staging)
            .status()
            .map_err(|e| format!("can't run tar: {e}"))?
            .success();
        let top = std::fs::read_dir(&staging).ok().and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir()));
        let result = match (ok, top) {
            (true, Some(top)) => {
                let old = parent.join(format!(".index-old-{}", std::process::id()));
                let _ = std::fs::rename(&self.dir, &old);
                let moved = std::fs::rename(&top, &self.dir).map_err(|e| format!("can't write {}: {e}", self.dir.display()));
                let _ = std::fs::remove_dir_all(&old);
                moved
            }
            _ => Err(format!("can't unpack the library index from {url}")),
        };
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    /// Whether there's a copy of the index at all.
    pub fn exists(&self) -> bool {
        self.dir.join("packages").is_dir()
    }

    /// Every published version of `name`, newest first.
    pub fn versions(&self, name: &str) -> Result<Vec<Entry>, String> {
        let dir = self.dir.join("packages").join(name);
        let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for e in rd.filter_map(|e| e.ok()) {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "toml") {
                continue;
            }
            out.push(read(&path)?);
        }
        out.sort_by_key(|e| std::cmp::Reverse(e.version()));
        Ok(out)
    }

    /// The names of every library in the index.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.join("packages"))
            .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }
}

pub fn read(path: &Path) -> Result<Entry, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
