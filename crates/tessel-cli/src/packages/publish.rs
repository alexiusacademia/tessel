//! `tessel publish`: a library's new version, proposed to the index.
//!
//! In the library's folder (its git repository), it checks the library
//! (its `[library]` table, that its code compiles, that it only uses
//! published libraries), tags the commit `v<version>` and pushes the tag,
//! downloads the tag back from the repository the way programs will, and
//! writes the index's entry for it. The entry goes to the index as a pull
//! request: `tessel publish` opens GitHub's page for proposing the file,
//! where one click sends it. The index's checks run on the request (with
//! `tessel publish --check`), and once it's merged, `tessel add` finds it.
//!
//! With `TESSEL_INDEX` naming a folder, the entry is written there instead,
//! and a `repository` that's a folder is copied from rather than tagged:
//! for trying a library out, and for tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::index::{self, Entry, Index};
use super::version::Version;
use super::{fail, fetch, library_manifest, Dep};

const USAGE: &str = "usage: tessel publish [--dry-run]
       tessel publish --check <entry.toml>

Publishes the library in this folder (its tessel.toml has a [library]
table): checks it, tags its git commit v<version>, pushes the tag and
proposes it to the library index. With --dry-run, it only checks the
library. With --check, it checks an entry of the index.";

pub fn publish(args: &[String]) -> ExitCode {
    // (Anything not understood stops it before it tags or pushes.)
    let dry = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--help" | "-h"] => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        ["--check", file] => {
            return match check_entry(Path::new(file)) {
                Ok(()) => {
                    println!("ok: {file}");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            };
        }
        [] => false,
        ["--dry-run"] => true,
        _ => return fail(format!("`tessel publish {}`: not understood\n{USAGE}", args.join(" "))),
    };
    match run(dry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).current_dir(dir).output().map_err(|e| format!("can't run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("`git {}` failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git@github.com:me/x.git` and `https://github.com/me/x.git` → `https://github.com/me/x`.
fn web_url(remote: &str) -> String {
    let r = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(rest) = r.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("https://{host}/{path}");
        }
    }
    r.to_string()
}

fn run(dry: bool) -> Result<(), String> {
    let toml = crate::pin::find(Path::new(".")).ok_or("there's no tessel.toml here; a library's folder has one with a [library] table")?;
    let dir = toml.parent().unwrap_or(Path::new(".")).to_path_buf();
    let m = library_manifest(&dir, None)?;
    let lib = &m.library;
    let file = toml.display();
    if !super::is_library_name(&lib.name) {
        return Err(format!("{file}: [library] name `{}`: names are lowercase letters, digits and `_`, starting with a letter", lib.name));
    }
    let description = lib.description.clone().filter(|d| !d.trim().is_empty());
    let license = lib.license.clone().filter(|d| !d.trim().is_empty());
    let (Some(description), Some(license)) = (description, license) else {
        return Err(format!("{file}: [library] needs a `description` (what it's for, in a sentence) and a `license` (like \"MIT\")"));
    };
    let version = Version::parse(&lib.version).map_err(|e| format!("{file}: [library] version: {e}"))?;
    let mut dependencies = BTreeMap::new();
    for (n, d) in &m.deps {
        match d {
            Dep::Index(r) => dependencies.insert(n.clone(), r.to_string()),
            _ => return Err(format!("{file}: `{n}` is {}; a published library can only use published libraries", d.show())),
        };
    }

    let repository = match &lib.repository {
        Some(r) if fetch::local(r).is_some() => dir.join(r).canonicalize().map_err(|e| format!("{r}: {e}"))?.display().to_string(),
        Some(r) => web_url(r),
        None => web_url(&git(&dir, &["remote", "get-url", "origin"]).map_err(|_| {
            format!("{file}: say where the library's code is with [library] repository = \"https://github.com/…\", or publish it from a git repository with an `origin`")
        })?),
    };

    let mut index = Index::open()?;
    index.refresh()?;
    let published = index.versions(&lib.name)?;
    if let Some(e) = published.first().filter(|e| e.repository != repository) {
        return Err(format!("the name `{}` is taken: it's the library at {}", lib.name, e.repository));
    }
    if published.iter().any(|e| e.version() == version) {
        return Err(format!("{} {version} is published already; give this one a new version in {file}", lib.name));
    }
    if let Some(newest) = published.first().filter(|e| e.version() > version) {
        eprintln!("Note: {} {} is newer than this {version}", lib.name, newest.version);
    }

    let tag = format!("v{version}");
    let local = fetch::local(&repository).is_some();
    // Every check runs the same with --dry-run, before anything is tagged
    // or pushed, so a dry run that's ready means the publish goes through.
    // (The repository's first: checking the code may write the library's
    // tessel.lock.)
    let head = if local {
        None
    } else {
        if !git(&dir, &["status", "--porcelain"])?.is_empty() {
            return Err("the repository has changes that aren't committed; commit them first, so what's published is in git".into());
        }
        let head = git(&dir, &["rev-parse", "HEAD"])?;
        let tagged = git(&dir, &["rev-parse", &format!("refs/tags/{tag}^{{commit}}")]).ok();
        if let Some(c) = tagged.as_ref().filter(|c| **c != head) {
            return Err(format!("tag {tag} is on another commit ({c}); a version's tag can't move"));
        }
        Some((head, tagged.is_some()))
    };
    eprintln!("Checking {} {version}", lib.name);
    check_code(&dir)?;
    if dry {
        println!("{} {version} is ready to publish from {repository} (tag {tag})", lib.name);
        return Ok(());
    }
    let commit = match head {
        None => "local".to_string(),
        Some((head, tagged)) => {
            if !tagged {
                git(&dir, &["tag", &tag])?;
                eprintln!("Tagged {tag}");
            }
            eprintln!("Pushing {tag} to origin");
            git(&dir, &["push", "origin", &format!("refs/tags/{tag}")])?;
            head
        }
    };

    // What programs will download, and its checksum.
    let tmp = crate::update::home()?.join(format!(".publish-{}", std::process::id()));
    let fetched = fetch::fetch(&repository, &tag, &commit, &tmp).and_then(|()| fetch::checksum(&tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    let checksum = fetched?;

    let entry = Entry {
        name: lib.name.clone(),
        version: version.to_string(),
        description,
        license,
        repository,
        tag,
        commit,
        checksum,
        dependencies,
    };
    let path = format!("packages/{}/{version}.toml", lib.name);
    match std::env::var("TESSEL_INDEX") {
        Ok(folder) if !folder.contains("://") && !folder.is_empty() => {
            let to = PathBuf::from(folder).join(&path);
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&to, entry.to_toml()).map_err(|e| format!("can't write {}: {e}", to.display()))?;
            println!("Published {} {version} to {}", lib.name, to.display());
        }
        _ => {
            let url = format!(
                "https://github.com/{}/new/main?filename={}&value={}",
                index::REPO,
                encode(&path),
                encode(&entry.to_toml())
            );
            println!("{} {version} is tagged and ready. To publish it, propose its entry to the index:\n\n{url}\n", lib.name);
            println!("That page (opening now) adds {path} to {} as a pull request: press \"Propose new file\", then \"Create pull request\". Once it's merged, `tessel add {}` finds it.", index::REPO, lib.name);
            open(&url);
        }
    }
    Ok(())
}

/// Compiles the library's code (its `.tsl` files, with the libraries it
/// uses), as `tessel check` would.
fn check_code(dir: &Path) -> Result<(), String> {
    let files = crate::program_files(dir)?;
    let mut sources = tessel_compiler::source::SourceMap::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("can't read {}: {e}", f.display()))?;
        sources.add(f.display().to_string(), text);
    }
    let analysis = tessel_compiler::analyze(&mut sources, false);
    if !analysis.diags.is_empty() {
        crate::report(&analysis.diags.list, &sources);
        return Err("the library has errors; fix them before publishing".into());
    }
    if analysis.program.items.iter().any(|i| matches!(i, tessel_compiler::ast::Item::App(_))) {
        return Err("a library can't have an `app`; that belongs to the programs that use it".into());
    }
    if analysis.program.public.is_empty() {
        return Err("nothing in the library is `public`, so a program that imports it can use none of it".into());
    }
    Ok(())
}

/// The index's check of a proposed entry (run on its pull requests): it's
/// well formed, new, from the repository its name belongs to, its tag names
/// its commit, its code has its checksum and compiles, and what it uses is
/// published.
fn check_entry(file: &Path) -> Result<(), String> {
    let e = index::read(file)?;
    let shown = file.display();
    if !super::is_library_name(&e.name) {
        return Err(format!("{shown}: `{}` can't be a library's name", e.name));
    }
    let version = Version::parse(&e.version)?;
    let expected = format!("packages/{}/{version}.toml", e.name);
    if !file.to_string_lossy().replace('\\', "/").ends_with(&expected) {
        return Err(format!("{shown}: the entry for {} {version} belongs in {expected}", e.name));
    }
    if fetch::github(&e.repository).is_none() && !e.repository.starts_with("https://") {
        return Err(format!("{shown}: repository `{}` must be an https:// address", e.repository));
    }
    if e.tag != format!("v{version}") {
        return Err(format!("{shown}: the tag of {version} is v{version}, not {}", e.tag));
    }
    // The other versions of the name, in the index this entry joins.
    let packages = file.parent().ok_or("no folder")?;
    for other in std::fs::read_dir(packages).map_err(|e| e.to_string())?.filter_map(|x| x.ok()) {
        if other.file_name() == file.file_name().unwrap_or_default() {
            continue;
        }
        let o = index::read(&other.path())?;
        if o.repository != e.repository {
            return Err(format!("{shown}: the name `{}` belongs to {}, not {}", e.name, o.repository, e.repository));
        }
    }
    let commit = fetch::commit_of(&e.repository, &e.tag)?;
    if commit != e.commit {
        return Err(format!("{shown}: tag {} of {} is commit {commit}, not {}", e.tag, e.repository, e.commit));
    }
    let tmp = std::env::temp_dir().join(format!("tessel-check-{}-{}", e.name, std::process::id()));
    let result = (|| {
        fetch::fetch(&e.repository, &e.tag, &e.commit, &tmp)?;
        let got = fetch::checksum(&tmp)?;
        if got != e.checksum {
            return Err(format!("{shown}: the code's checksum is {got}, not {}", e.checksum));
        }
        let m = library_manifest(&tmp, Some(&e.name))?;
        if m.library.version != e.version {
            return Err(format!("{shown}: the code at {} says it's version {}, not {}", e.tag, m.library.version, e.version));
        }
        let deps: BTreeMap<String, String> = m
            .deps
            .iter()
            .map(|(n, d)| match d {
                Dep::Index(r) => Ok((n.clone(), r.to_string())),
                _ => Err(format!("{shown}: it uses `{n}` from {}; a published library can only use published libraries", d.show())),
            })
            .collect::<Result<_, _>>()?;
        if deps != e.dependencies {
            return Err(format!("{shown}: `dependencies` must be what its tessel.toml lists: {deps:?}"));
        }
        check_code(&tmp)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Opens `url` in the browser, where there is one.
fn open(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(windows) {
        Command::new("cmd").args(["/c", "start", "", url]).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_become_web_addresses() {
        assert_eq!(web_url("git@github.com:me/textkit.git"), "https://github.com/me/textkit");
        assert_eq!(web_url("https://github.com/me/textkit.git\n"), "https://github.com/me/textkit");
        assert_eq!(encode("a b/c=\n"), "a%20b%2Fc%3D%0A");
    }
}
