//! Getting a library's code: from GitHub as a `.tar.gz` of one commit, from
//! another git host with `git`, or (for trying things out, and for tests)
//! from a folder on this computer. And its checksum, which names exactly
//! what was published.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// Downloads `url` into memory.
pub fn get(url: &str) -> Result<Vec<u8>, String> {
    get_as(url, "*/*")
}

fn get_as(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    let mut req = crate::update::agent().get(url).header("User-Agent", "tessel").header("Accept", accept);
    // GitHub's API allows few requests without a login, per network address,
    // and shared machines (like CI's) use them up: a token set for `gh` or
    // GitHub Actions lifts that.
    if url.starts_with("https://api.github.com/")
        && let Some(token) = ["GH_TOKEN", "GITHUB_TOKEN"].iter().find_map(|k| std::env::var(k).ok().filter(|t| !t.is_empty()))
    {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let mut r = req
        .call()
        .map_err(|e| format!("can't download {url}: {e}"))?;
    let mut bytes = Vec::new();
    r.body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("can't download {url}: {e}"))?;
    Ok(bytes)
}

/// `owner/name` of a repository on GitHub.
pub fn github(repository: &str) -> Option<String> {
    let rest = repository.strip_prefix("https://github.com/")?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}"))
}

/// A repository given as a folder on this computer (not a URL).
pub fn local(repository: &str) -> Option<PathBuf> {
    (!repository.contains("://")).then(|| PathBuf::from(repository))
}

/// The commit a tag names in `repository`.
pub fn commit_of(repository: &str, tag: &str) -> Result<String, String> {
    if local(repository).is_some() {
        return Ok("local".into());
    }
    // (GitHub's API needs no `git`; when it refuses, `git` is asked.)
    if let Some(repo) = github(repository)
        && let Ok(sha) = get_as(&format!("https://api.github.com/repos/{repo}/commits/{tag}"), "application/vnd.github.sha")
    {
        let sha = String::from_utf8_lossy(&sha).trim().to_string();
        if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(sha);
        }
    }
    let out = Command::new("git")
        .args(["ls-remote", "--tags", repository])
        .output()
        .map_err(|e| format!("can't run git (needed to look up {repository}): {e}"))?;
    if !out.status.success() {
        return Err(format!("can't read the tags of {repository}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    // An annotated tag is listed twice: `refs/tags/v1^{}` is its commit.
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found = None;
    for line in text.lines() {
        let Some((sha, name)) = line.split_once('\t') else { continue };
        if name == format!("refs/tags/{tag}^{{}}") {
            return Ok(sha.to_string());
        }
        if name == format!("refs/tags/{tag}") {
            found = Some(sha.to_string());
        }
    }
    found.ok_or_else(|| format!("{repository} has no tag `{tag}`"))
}

/// Puts the code of `repository` at `commit` (or tag `tag`, off GitHub)
/// into the new folder `into`.
pub fn fetch(repository: &str, tag: &str, commit: &str, into: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(into);
    if let Some(dir) = local(repository) {
        return copy(&dir, into);
    }
    if let Some(repo) = github(repository) {
        let bytes = get(&format!("https://codeload.github.com/{repo}/tar.gz/{commit}"))?;
        let unpack = into.with_extension("unpack");
        let _ = std::fs::remove_dir_all(&unpack);
        std::fs::create_dir_all(&unpack).map_err(|e| format!("can't write {}: {e}", unpack.display()))?;
        let archive = unpack.join("code.tar.gz");
        std::fs::write(&archive, bytes).map_err(|e| format!("can't write {}: {e}", archive.display()))?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg("code.tar.gz")
            .current_dir(&unpack)
            .status()
            .map_err(|e| format!("can't run tar: {e}"))?;
        let _ = std::fs::remove_file(&archive);
        if !status.success() {
            return Err(format!("can't unpack the code of {repository}"));
        }
        // (The archive has one folder, `name-commit`.)
        let top = std::fs::read_dir(&unpack)
            .ok()
            .and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir()))
            .ok_or_else(|| format!("the code of {repository} came empty"))?;
        let moved = std::fs::rename(&top, into).map_err(|e| format!("can't write {}: {e}", into.display()));
        let _ = std::fs::remove_dir_all(&unpack);
        return moved;
    }
    let status = Command::new("git")
        .args(["-c", "core.autocrlf=false", "-c", "advice.detachedHead=false", "clone", "--quiet", "--depth", "1", "--branch", tag, repository])
        .arg(into)
        .status()
        .map_err(|e| format!("can't run git (needed to get {repository}): {e}"))?;
    if !status.success() {
        return Err(format!("can't get tag `{tag}` of {repository}"));
    }
    let head = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(into).output().map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let _ = std::fs::remove_dir_all(into.join(".git"));
    if head != commit {
        return Err(format!("tag `{tag}` of {repository} is now commit {head}, not {commit} as published"));
    }
    Ok(())
}

/// Copies a folder, leaving out `.git`.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("can't write {}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("can't read {}: {e}", from.display()))?;
    for e in entries.filter_map(|e| e.ok()) {
        let (path, name) = (e.path(), e.file_name());
        if name == ".git" {
            continue;
        }
        let kind = e.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy(&path, &to.join(&name))?;
        } else if kind.is_file() {
            std::fs::copy(&path, to.join(&name)).map_err(|e| format!("can't copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The checksum of a library's code: every file in its folder (but `.git`
/// and links), by path, so the same code has the same checksum wherever
/// it was downloaded.
pub fn checksum(dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort();
    let mut h = Sha256::new();
    for rel in files {
        let bytes = std::fs::read(dir.join(&rel)).map_err(|e| format!("can't read {rel}: {e}"))?;
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(format!("sha256:{}", h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
    for e in entries.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let kind = e.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            walk(&e.path(), &rel, out)?;
        } else if kind.is_file() {
            out.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_repositories() {
        assert_eq!(github("https://github.com/someone/textkit").as_deref(), Some("someone/textkit"));
        assert_eq!(github("https://github.com/someone/textkit.git").as_deref(), Some("someone/textkit"));
        assert_eq!(github("https://gitlab.com/someone/textkit"), None);
        assert_eq!(github("https://github.com/someone"), None);
    }
}
