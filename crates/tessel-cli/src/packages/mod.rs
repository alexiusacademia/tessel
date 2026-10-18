//! Libraries other people publish, and the programs that use them.
//!
//! A library is a module (a folder of `.tsl` files) with a `tessel.toml`
//! that has a `[library]` table: its name, version, description and
//! licence. It's published from its git repository: `tessel publish`
//! tags the version and proposes it to the index (see `index`).
//!
//! A program lists the libraries it uses in its `tessel.toml`:
//!
//! ```toml
//! [dependencies]
//! textkit = "1.2"                                               # from the index
//! geokit = { git = "https://github.com/me/geokit", tag = "v0.3.0" }
//! mine = { path = "../mine" }                                   # a folder
//! ```
//!
//! and imports them like any module: `import textkit`. Tessel works out a
//! version of each library (and of the libraries they use) that meets
//! everyone's requirements, writes them to `tessel.lock` so that every
//! build uses the same ones, and downloads their code into its folder
//! (`packages/`). The compiler asks for a library when an `import` names
//! no folder next to the file (see `find`).

pub mod fetch;
pub mod index;
mod publish;
pub mod version;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use index::Index;
use version::{Req, Version};

pub const LOCK: &str = "tessel.lock";

/// What a program (or a library) asks for under one name.
#[derive(Clone, Debug, PartialEq)]
pub enum Dep {
    /// A published version (and its compatible successors).
    Index(Req),
    /// A tag of a git repository.
    Git { repository: String, tag: String },
    /// A folder on this computer (absolute), as written.
    Path(PathBuf, String),
}

impl Dep {
    fn show(&self) -> String {
        match self {
            Dep::Index(r) => format!("\"{r}\""),
            Dep::Git { repository, tag } => format!("tag {tag} of {repository}"),
            Dep::Path(_, p) => format!("the folder {p}"),
        }
    }
}

/// A `[dependencies]` table, read: `dir` is the folder of its
/// `tessel.toml`, which paths are relative to.
pub fn deps_of(dir: &Path, table: &BTreeMap<String, toml::Value>) -> Result<BTreeMap<String, Dep>, String> {
    let mut out = BTreeMap::new();
    for (name, value) in table {
        if !is_library_name(name) {
            return Err(format!(
                "[dependencies]: `{name}` can't be a library's name; names are lowercase letters, digits and `_`, starting with a letter"
            ));
        }
        let dep = match value {
            toml::Value::String(s) => Dep::Index(Req::parse(s).map_err(|e| format!("[dependencies] {name}: {e}"))?),
            toml::Value::Table(t) => {
                let get = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
                if let Some(k) = t.keys().find(|k| !["version", "git", "tag", "path"].contains(&k.as_str())) {
                    return Err(format!("[dependencies] {name}: unknown key `{k}`; use `version`, `git` and `tag`, or `path`"));
                }
                match (get("version"), get("git"), get("tag"), get("path")) {
                    (Some(v), None, None, None) => Dep::Index(Req::parse(&v).map_err(|e| format!("[dependencies] {name}: {e}"))?),
                    (None, Some(git), Some(tag), None) => Dep::Git { repository: git, tag },
                    (None, Some(_), None, None) => {
                        return Err(format!("[dependencies] {name}: say which version of the repository with `tag`, like tag = \"v1.0.0\""))
                    }
                    (None, None, None, Some(p)) => {
                        let full = dir.join(&p);
                        let full = full.canonicalize().map_err(|_| format!("[dependencies] {name}: there's no folder `{p}`"))?;
                        Dep::Path(full, p)
                    }
                    _ => {
                        return Err(format!(
                            "[dependencies] {name}: write a version (\"1.2\"), a git repository and its tag ({{ git = \"…\", tag = \"v1.2.0\" }}) or a folder ({{ path = \"…\" }})"
                        ))
                    }
                }
            }
            _ => return Err(format!("[dependencies] {name}: write a version in quotes, like {name} = \"1.0\"")),
        };
        out.insert(name.clone(), dep);
    }
    Ok(out)
}

/// A library's name is written in code (`import textkit`), and is the
/// same everywhere, so it's kept simple.
pub fn is_library_name(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// One library in `tessel.lock`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Locked {
    pub name: String,
    pub version: String,
    /// "index", "git" or "path".
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
    /// For a folder: as `tessel.toml` writes it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// What it asks for (a folder's are read from it instead).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct LockFile {
    /// How the file is written (1 for now).
    format: u32,
    #[serde(default, rename = "package")]
    packages: Vec<Locked>,
}

const LOCK_HEADER: &str = "# The exact versions of the libraries this project uses, worked out by\n# Tessel from tessel.toml. Commit it, so every build uses the same ones.\n\n";

fn read_lock(path: &Path) -> Result<Option<Vec<Locked>>, String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(None) };
    let lock: LockFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(lock.packages))
}

fn write_lock(path: &Path, packages: Vec<Locked>) -> Result<(), String> {
    if packages.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    let text = format!("{LOCK_HEADER}{}", toml::to_string(&LockFile { format: 1, packages }).map_err(|e| e.to_string())?);
    if std::fs::read_to_string(path).is_ok_and(|t| t == text) {
        return Ok(());
    }
    std::fs::write(path, text).map_err(|e| format!("can't write {}: {e}", path.display()))
}

/// A library chosen for a program.
#[derive(Clone, Debug)]
struct Pick {
    locked: Locked,
    /// Where its code is (or will be, once downloaded).
    dir: PathBuf,
    deps: BTreeMap<String, Dep>,
}

impl Pick {
    fn version(&self) -> Version {
        Version::parse(&self.locked.version).unwrap_or(Version { major: 0, minor: 0, patch: 0 })
    }

    /// Whether it's what `dep` asks for.
    fn meets(&self, dep: &Dep) -> bool {
        match dep {
            Dep::Index(req) => req.matches(self.version()),
            Dep::Git { repository, tag } => {
                self.locked.source == "git" && &self.locked.repository == repository && &self.locked.tag == tag
            }
            Dep::Path(dir, _) => self.locked.source == "path" && &self.dir == dir,
        }
    }
}

/// Where downloaded libraries are kept: one folder per name, version and
/// checksum, shared by every project.
fn store() -> Result<PathBuf, String> {
    Ok(crate::update::home()?.join("packages"))
}

fn stored(name: &str, version: &str, checksum: &str) -> Result<PathBuf, String> {
    let short: String = checksum.trim_start_matches("sha256:").chars().take(12).collect();
    Ok(store()?.join(format!("{name}-{version}-{short}")))
}

/// What a library's own `tessel.toml` says: its `[library]` and what it uses.
pub struct LibraryManifest {
    pub library: crate::manifest::Library,
    pub deps: BTreeMap<String, Dep>,
}

pub fn library_manifest(dir: &Path, expected: Option<&str>) -> Result<LibraryManifest, String> {
    let file = dir.join(crate::pin::FILE);
    let text = std::fs::read_to_string(&file).map_err(|_| format!("{} has no tessel.toml, so it isn't a library", dir.display()))?;
    let m = crate::manifest::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let library = m.library.ok_or_else(|| format!("{}: there's no [library] table, so it isn't a library", file.display()))?;
    if let Some(name) = expected.filter(|n| *n != library.name) {
        return Err(format!("{} is the library `{}`, not `{name}`", dir.display(), library.name));
    }
    Version::parse(&library.version).map_err(|e| format!("{}: [library] version: {e}", file.display()))?;
    let deps = deps_of(dir, &m.dependencies)?;
    Ok(LibraryManifest { library, deps })
}

/// Who asks for what: the program, and the libraries it reaches through
/// `picks`. `None` means "this project".
fn wants(root: &BTreeMap<String, Dep>, picks: &BTreeMap<String, Pick>) -> BTreeMap<String, Vec<(Option<String>, Dep)>> {
    let mut out: BTreeMap<String, Vec<(Option<String>, Dep)>> = BTreeMap::new();
    let mut queue: Vec<String> = Vec::new();
    for (n, d) in root {
        out.entry(n.clone()).or_default().push((None, d.clone()));
        queue.push(n.clone());
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(n) = queue.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        let Some(p) = picks.get(&n) else { continue };
        let who = format!("{} {}", p.locked.name, p.locked.version);
        for (dn, d) in &p.deps {
            out.entry(dn.clone()).or_default().push((Some(who.clone()), d.clone()));
            queue.push(dn.clone());
        }
    }
    out
}

/// Whether `picks` are exactly what `root` needs.
fn settled(root: &BTreeMap<String, Dep>, picks: &BTreeMap<String, Pick>) -> bool {
    let w = wants(root, picks);
    w.len() == picks.len() && w.iter().all(|(n, asks)| picks.get(n).is_some_and(|p| asks.iter().all(|(_, d)| p.meets(d))))
}

fn who(w: &Option<String>) -> String {
    w.clone().unwrap_or_else(|| "this project".into())
}

/// Works out a version of every library the program needs. `prefer` (the
/// lock) is kept where it still fits, except for the names in `unlock`
/// (every name, when it's `None`).
fn resolve(
    root: &BTreeMap<String, Dep>,
    prefer: &[Locked],
    unlock: Option<&BTreeSet<String>>,
    index: &mut Index,
) -> Result<BTreeMap<String, Pick>, String> {
    let locked = |n: &str| unlock.is_some_and(|u| !u.contains(n)).then(|| prefer.iter().find(|l| l.name == n)).flatten();
    let mut picks: BTreeMap<String, Pick> = BTreeMap::new();
    for _ in 0..500 {
        let w = wants(root, &picks);
        picks.retain(|n, _| w.contains_key(n));
        let Some((name, asks)) = w.iter().find(|(n, asks)| !picks.get(*n).is_some_and(|p| asks.iter().all(|(_, d)| p.meets(d)))) else {
            return Ok(picks);
        };
        let pick = choose(name, asks, locked(name), index)?;
        picks.insert(name.clone(), pick);
    }
    Err("can't settle on versions of the libraries: their requirements keep changing each other".into())
}

/// A version of `name` that meets everything in `asks`.
fn choose(name: &str, asks: &[(Option<String>, Dep)], locked: Option<&Locked>, index: &mut Index) -> Result<Pick, String> {
    let fixed: Vec<&(Option<String>, Dep)> = asks.iter().filter(|(_, d)| !matches!(d, Dep::Index(_))).collect();
    let pick = if let Some((first_who, first)) = fixed.first() {
        if let Some((other_who, other)) = fixed.iter().skip(1).find(|(_, d)| d != first) {
            return Err(format!(
                "`{name}` is asked for as {} by {} and as {} by {}; a program can have one `{name}`",
                first.show(),
                who(first_who),
                other.show(),
                who(other_who)
            ));
        }
        match first {
            Dep::Path(dir, written) => {
                let m = library_manifest(dir, Some(name))?;
                Pick {
                    locked: Locked {
                        name: name.into(),
                        version: m.library.version.clone(),
                        source: "path".into(),
                        repository: String::new(),
                        tag: String::new(),
                        commit: String::new(),
                        checksum: String::new(),
                        path: written.replace('\\', "/"),
                        dependencies: BTreeMap::new(),
                    },
                    dir: dir.clone(),
                    deps: m.deps,
                }
            }
            Dep::Git { repository, tag } => {
                let reuse = locked.filter(|l| l.source == "git" && &l.repository == repository && &l.tag == tag);
                match reuse {
                    Some(l) => from_locked(l, Path::new("."))?,
                    None => git_pick(name, repository, tag)?,
                }
            }
            Dep::Index(_) => unreachable!(),
        }
    } else {
        index_pick(name, asks, locked, index)?
    };
    if let Some((w, d)) = asks.iter().find(|(_, d)| !pick.meets(d)) {
        return Err(format!("{} asks for `{name}` {}, but `{name}` is {} {}", who(w), d.show(), pick.locked.version, describe_source(&pick.locked)));
    }
    Ok(pick)
}

fn describe_source(l: &Locked) -> String {
    match l.source.as_str() {
        "git" => format!("(tag {} of {})", l.tag, l.repository),
        "path" => format!("(the folder {})", l.path),
        _ => String::new(),
    }
}

fn index_pick(name: &str, asks: &[(Option<String>, Dep)], locked: Option<&Locked>, index: &mut Index) -> Result<Pick, String> {
    let reqs: Vec<(&Option<String>, &Req)> = asks
        .iter()
        .filter_map(|(w, d)| match d {
            Dep::Index(r) => Some((w, r)),
            _ => None,
        })
        .collect();
    let fits = |v: Version| reqs.iter().all(|(_, r)| r.matches(v));
    if let Some(l) = locked.filter(|l| l.source == "index" && Version::parse(&l.version).is_ok_and(fits)) {
        return from_locked(l, Path::new("."));
    }
    let mut versions = if index.exists() { index.versions(name)? } else { Vec::new() };
    if !versions.iter().any(|e| fits(e.version())) {
        index.refresh()?;
        versions = index.versions(name)?;
    }
    if versions.is_empty() {
        return Err(format!("there's no library `{name}` in the index (`tessel search` lists them)"));
    }
    let Some(e) = versions.iter().find(|e| fits(e.version())) else {
        let asked: Vec<String> = reqs.iter().map(|(w, r)| format!("{} asks for \"{r}\"", who(w))).collect();
        let published: Vec<&str> = versions.iter().map(|e| e.version.as_str()).collect();
        return Err(format!(
            "no version of `{name}` is what everyone asks for: {}; the published versions are {}",
            asked.join(", "),
            published.join(", ")
        ));
    };
    let mut deps = BTreeMap::new();
    for (dn, r) in &e.dependencies {
        deps.insert(dn.clone(), Dep::Index(Req::parse(r).map_err(|err| format!("{name} {}: {err}", e.version))?));
    }
    Ok(Pick {
        dir: stored(name, &e.version, &e.checksum)?,
        locked: Locked {
            name: name.into(),
            version: e.version.clone(),
            source: "index".into(),
            repository: e.repository.clone(),
            tag: e.tag.clone(),
            commit: e.commit.clone(),
            checksum: e.checksum.clone(),
            path: String::new(),
            dependencies: e.dependencies.clone(),
        },
        deps,
    })
}

/// A library from a tag of a git repository: downloaded now, to read its
/// version and what it uses.
fn git_pick(name: &str, repository: &str, tag: &str) -> Result<Pick, String> {
    let commit = fetch::commit_of(repository, tag)?;
    let tmp = store()?.join(format!(".get-{name}-{}", std::process::id()));
    eprintln!("Downloading {name} ({tag} of {repository})");
    fetch::fetch(repository, tag, &commit, &tmp)?;
    let done = (|| {
        let checksum = fetch::checksum(&tmp)?;
        let m = library_manifest(&tmp, Some(name))?;
        let mut dependencies = BTreeMap::new();
        for (dn, d) in &m.deps {
            match d {
                Dep::Index(r) => dependencies.insert(dn.clone(), r.to_string()),
                _ => return Err(format!("`{name}` uses `{dn}` from {}; a library from git can only use published libraries", d.show())),
            };
        }
        let dir = stored(name, &m.library.version, &checksum)?;
        if !dir.is_dir() {
            std::fs::rename(&tmp, &dir).map_err(|e| format!("can't write {}: {e}", dir.display()))?;
        }
        Ok(Pick {
            locked: Locked {
                name: name.into(),
                version: m.library.version.clone(),
                source: "git".into(),
                repository: repository.into(),
                tag: tag.into(),
                commit,
                checksum,
                path: String::new(),
                dependencies,
            },
            dir,
            deps: m.deps,
        })
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    done
}

/// A library as the lock has it (`root` is the project's folder, which a
/// folder's path is relative to).
fn from_locked(l: &Locked, root: &Path) -> Result<Pick, String> {
    if l.source == "path" {
        let dir = root.join(&l.path).canonicalize().map_err(|_| format!("there's no folder `{}` for `{}`", l.path, l.name))?;
        let m = library_manifest(&dir, Some(&l.name))?;
        let mut locked = l.clone();
        locked.version = m.library.version.clone();
        return Ok(Pick { locked, dir, deps: m.deps });
    }
    let mut deps = BTreeMap::new();
    for (dn, r) in &l.dependencies {
        deps.insert(dn.clone(), Dep::Index(Req::parse(r)?));
    }
    Ok(Pick { dir: stored(&l.name, &l.version, &l.checksum)?, locked: l.clone(), deps })
}

/// Downloads the libraries that aren't in Tessel's folder yet.
fn download(picks: &BTreeMap<String, Pick>) -> Result<(), String> {
    for p in picks.values() {
        if p.locked.source == "path" || p.dir.is_dir() {
            continue;
        }
        let l = &p.locked;
        eprintln!("Downloading {} {}", l.name, l.version);
        let tmp = store()?.join(format!(".get-{}-{}", l.name, std::process::id()));
        let done = fetch::fetch(&l.repository, &l.tag, &l.commit, &tmp).and_then(|()| {
            let got = fetch::checksum(&tmp)?;
            if got != l.checksum {
                return Err(format!(
                    "the code of {} {} isn't what was published (its checksum is {got}, not {}); it was changed after publishing",
                    l.name, l.version, l.checksum
                ));
            }
            // (Another `tessel` may have stored it meanwhile: as good.)
            if std::fs::rename(&tmp, &p.dir).is_err() && !p.dir.is_dir() {
                return Err(format!("can't write {}", p.dir.display()));
            }
            Ok(())
        });
        let _ = std::fs::remove_dir_all(&tmp);
        done?;
    }
    Ok(())
}

/// The libraries a project has, found and downloaded.
#[derive(Clone, Default)]
pub struct Resolved {
    /// What the project itself depends on.
    root: BTreeSet<String>,
    /// Each library's folder, and the names it may import.
    packages: BTreeMap<String, (PathBuf, BTreeSet<String>)>,
    picks: BTreeMap<String, Pick>,
}

/// The project's `tessel.toml`, read for its dependencies.
fn project_deps(toml: &Path) -> Result<BTreeMap<String, Dep>, String> {
    let text = std::fs::read_to_string(toml).map_err(|e| format!("can't read {}: {e}", toml.display()))?;
    let m = crate::manifest::parse(&text).map_err(|e| format!("{}: {e}", toml.display()))?;
    deps_of(toml.parent().unwrap_or(Path::new(".")), &m.dependencies)
}

/// Makes sure the project of `toml` has its libraries: the lock is used as
/// it is when it still fits `tessel.toml`, worked out again when it
/// doesn't, and what's missing is downloaded.
pub fn ensure(toml: &Path) -> Result<Resolved, String> {
    let root_dir = toml.parent().unwrap_or(Path::new(".")).to_path_buf();
    let root = project_deps(toml)?;
    let lock_path = root_dir.join(LOCK);
    let lock = read_lock(&lock_path)?;
    if root.is_empty() && lock.is_none() {
        return Ok(Resolved::default());
    }
    let from_lock = lock.as_ref().and_then(|l| {
        l.iter().map(|x| from_locked(x, &root_dir).map(|p| (x.name.clone(), p))).collect::<Result<BTreeMap<_, _>, _>>().ok()
    });
    let picks = match from_lock.filter(|p| settled(&root, p)) {
        Some(p) => p,
        None => {
            let mut index = Index::open()?;
            let none = BTreeSet::new();
            let p = resolve(&root, lock.as_deref().unwrap_or(&[]), Some(&none), &mut index)?;
            write_lock(&lock_path, p.values().map(|x| x.locked.clone()).collect())?;
            p
        }
    };
    download(&picks)?;
    Ok(resolved(&root, picks))
}

fn resolved(root: &BTreeMap<String, Dep>, picks: BTreeMap<String, Pick>) -> Resolved {
    let packages = picks
        .iter()
        .map(|(n, p)| (n.clone(), (p.dir.canonicalize().unwrap_or(p.dir.clone()), p.deps.keys().cloned().collect())))
        .collect();
    Resolved { root: root.keys().cloned().collect(), packages, picks }
}

/// What `ensure` found for each project, while its files stay the same.
type Stamp = Vec<Option<std::time::SystemTime>>;
static FOUND: Mutex<Option<HashMap<PathBuf, (Stamp, Resolved)>>> = Mutex::new(None);

fn stamp(toml: &Path) -> Stamp {
    let dir = toml.parent().unwrap_or(Path::new("."));
    [toml.to_path_buf(), dir.join(LOCK)].iter().map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).collect()
}

/// For the compiler: the folder of the library `name`, imported from a file
/// in `from`, in the program in `program`. `None` when the project has no
/// such library.
pub fn find(program: &Path, from: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let Some(toml) = crate::pin::find(program) else { return Ok(None) };
    let now = stamp(&toml);
    let cached = FOUND.lock().unwrap().as_ref().and_then(|m| m.get(&toml).filter(|(s, _)| *s == now).map(|(_, r)| r.clone()));
    let r = match cached {
        Some(r) => r,
        None => {
            let r = ensure(&toml)?;
            FOUND.lock().unwrap().get_or_insert_with(HashMap::new).insert(toml.clone(), (stamp(&toml), r.clone()));
            r
        }
    };
    let Some((dir, _)) = r.packages.get(name) else { return Ok(None) };
    let from = from.canonicalize().unwrap_or(from.to_path_buf());
    let owner = r.packages.iter().find(|(_, (d, _))| from.starts_with(d));
    let allowed = owner.map(|(_, (_, deps))| deps).unwrap_or(&r.root);
    if !allowed.contains(name) {
        return Err(match owner {
            Some((o, _)) => format!("library `{o}` imports `{name}` without listing it in the [dependencies] of its tessel.toml"),
            None => format!("`{name}` is a library that another library uses, not one this project depends on; to use it here, run `tessel add {name}`"),
        });
    }
    Ok(Some(dir.clone()))
}

// The commands.

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("tessel: {msg}");
    ExitCode::FAILURE
}

/// The `tessel.toml` of the project in the current folder.
fn project_toml(create: bool) -> Result<PathBuf, String> {
    if let Some(f) = crate::pin::find(Path::new(".")) {
        return Ok(f);
    }
    if !create {
        return Err("there's no tessel.toml here or in a folder above; `tessel add` starts one".into());
    }
    let f = std::env::current_dir().map_err(|e| e.to_string())?.join(crate::pin::FILE);
    std::fs::write(&f, "").map_err(|e| format!("can't write {}: {e}", f.display()))?;
    Ok(f)
}

/// `tessel add <name>[@<version>]`, `--git <url> --tag <tag>` or `--path <folder>`.
pub fn add(args: &[String]) -> ExitCode {
    let opt = |k: &str| args.windows(2).find(|w| w[0] == k).map(|w| w[1].clone());
    let Some(spec) = args.first().filter(|a| !a.starts_with("--")) else {
        return fail("usage: tessel add <library>[@<version>] [--git <url> --tag <tag> | --path <folder>]");
    };
    let (name, wanted) = match spec.split_once('@') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (spec.clone(), None),
    };
    if !is_library_name(&name) {
        return fail(format!("`{name}` can't be a library's name; names are lowercase letters, digits and `_`, starting with a letter"));
    }
    let existed = crate::pin::find(Path::new(".")).is_some();
    let toml = match project_toml(true) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let undo = |before: &str| {
        if existed {
            let _ = std::fs::write(&toml, before);
        } else {
            let _ = std::fs::remove_file(&toml);
        }
    };
    let value = if let Some(path) = opt("--path") {
        format!("{{ path = {} }}", quote(&path.replace('\\', "/")))
    } else if let Some(git) = opt("--git") {
        let Some(tag) = opt("--tag") else { return fail("--git needs --tag, the tag of the version to use, like --tag v1.0.0") };
        format!("{{ git = {}, tag = {} }}", quote(&git), quote(&tag))
    } else {
        let req = match wanted {
            Some(w) => match Req::parse(&w) {
                Ok(r) => r,
                Err(e) => return fail(e),
            },
            None => {
                let mut index = match Index::open() {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                if let Err(e) = index.refresh() {
                    return fail(e);
                }
                match index.versions(&name) {
                    Ok(v) if !v.is_empty() => Req::compatible(v[0].version()),
                    Ok(_) => {
                        undo("");
                        return fail(format!("there's no library `{name}` in the index (`tessel search` lists them)"));
                    }
                    Err(e) => {
                        undo("");
                        return fail(e);
                    }
                }
            }
        };
        quote(&req.to_string())
    };
    let before = std::fs::read_to_string(&toml).unwrap_or_default();
    let after = set_dependency(&before, &name, Some(&value));
    if let Err(e) = std::fs::write(&toml, &after) {
        return fail(format!("can't write {}: {e}", toml.display()));
    }
    match ensure(&toml) {
        Ok(r) => {
            let p = &r.picks[&name];
            eprintln!("Added {name} {} to {}", p.locked.version, shown(&toml));
            ExitCode::SUCCESS
        }
        Err(e) => {
            undo(&before);
            fail(e)
        }
    }
}

/// `tessel remove <name>`.
pub fn remove(args: &[String]) -> ExitCode {
    let Some(name) = args.first() else { return fail("usage: tessel remove <library>") };
    let toml = match project_toml(false) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let before = std::fs::read_to_string(&toml).unwrap_or_default();
    let after = set_dependency(&before, name, None);
    if after == before {
        return fail(format!("{} has no dependency `{name}`", shown(&toml)));
    }
    if let Err(e) = std::fs::write(&toml, &after) {
        return fail(format!("can't write {}: {e}", toml.display()));
    }
    match ensure(&toml) {
        Ok(_) => {
            eprintln!("Removed {name} from {}", shown(&toml));
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

/// `tessel deps`: the libraries the project uses; `tessel deps update
/// [names]`: newer versions of them, where `tessel.toml` allows.
pub fn deps(args: &[String]) -> ExitCode {
    let toml = match project_toml(false) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    if args.first().is_some_and(|a| a == "update") {
        return update(&toml, &args[1..]);
    }
    if let Some(a) = args.first() {
        return fail(format!("`tessel deps {a}`? Use `tessel deps` to list the libraries, or `tessel deps update [library…]`"));
    }
    let r = match ensure(&toml) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    if r.root.is_empty() {
        println!("This project uses no libraries; `tessel add <library>` adds one.");
        return ExitCode::SUCCESS;
    }
    let mut out = String::new();
    let mut seen = BTreeSet::new();
    tree(&r, &r.root, "", &mut seen, &mut out);
    print!("{out}");
    ExitCode::SUCCESS
}

fn tree(r: &Resolved, names: &BTreeSet<String>, indent: &str, seen: &mut BTreeSet<String>, out: &mut String) {
    let names: Vec<&String> = names.iter().collect();
    for (i, n) in names.iter().enumerate() {
        let Some(p) = r.picks.get(*n) else { continue };
        // (The project's own libraries at the left, what they use below them.)
        let last = i + 1 == names.len();
        let (branch, more) = match (indent.is_empty(), last) {
            (true, _) => ("", "  "),
            (false, true) => ("└─ ", "   "),
            (false, false) => ("├─ ", "│  "),
        };
        let src = describe_source(&p.locked);
        let src = if src.is_empty() { src } else { format!(" {src}") };
        let again = !seen.insert((*n).clone());
        let note = if again && !p.deps.is_empty() { " (as above)" } else { "" };
        out.push_str(&format!("{indent}{branch}{n} {}{src}{note}\n", p.locked.version));
        if !again {
            let deps: BTreeSet<String> = p.deps.keys().cloned().collect();
            tree(r, &deps, &format!("{indent}{more}"), seen, out);
        }
    }
}

fn update(toml: &Path, names: &[String]) -> ExitCode {
    let root = match project_deps(toml) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let lock_path = toml.parent().unwrap_or(Path::new(".")).join(LOCK);
    let before = match read_lock(&lock_path) {
        Ok(l) => l.unwrap_or_default(),
        Err(e) => return fail(e),
    };
    let mut index = match Index::open() {
        Ok(i) => i,
        Err(e) => return fail(e),
    };
    if let Err(e) = index.refresh() {
        return fail(e);
    }
    let unlock: Option<BTreeSet<String>> = (!names.is_empty()).then(|| names.iter().cloned().collect());
    let picks = match resolve(&root, &before, unlock.as_ref(), &mut index) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    if let Err(e) = write_lock(&lock_path, picks.values().map(|p| p.locked.clone()).collect()) {
        return fail(e);
    }
    if let Err(e) = download(&picks) {
        return fail(e);
    }
    let mut changed = false;
    for p in picks.values() {
        match before.iter().find(|l| l.name == p.locked.name) {
            Some(l) if l.version != p.locked.version => println!("{} {} → {}", l.name, l.version, p.locked.version),
            None => println!("{} {} (new)", p.locked.name, p.locked.version),
            _ => continue,
        }
        changed = true;
    }
    for l in &before {
        if !picks.contains_key(&l.name) {
            println!("{} {} (no longer used)", l.name, l.version);
            changed = true;
        }
    }
    if !changed {
        println!("The libraries are up to date.");
    }
    ExitCode::SUCCESS
}

/// `tessel search [words]`: the published libraries.
pub fn search(args: &[String]) -> ExitCode {
    let mut index = match Index::open() {
        Ok(i) => i,
        Err(e) => return fail(e),
    };
    if let Err(e) = index.refresh() {
        return fail(e);
    }
    let words: Vec<String> = args.iter().map(|a| a.to_lowercase()).collect();
    let mut found = 0;
    for name in index.names() {
        let Ok(versions) = index.versions(&name) else { continue };
        let Some(latest) = versions.first() else { continue };
        let text = format!("{name} {}", latest.description).to_lowercase();
        if words.iter().all(|w| text.contains(w.as_str())) {
            println!("{name} {}  {}", latest.version, latest.description);
            found += 1;
        }
    }
    if found == 0 {
        println!("{}", if words.is_empty() { "No libraries are published yet.".to_string() } else { format!("No library matches `{}`.", args.join(" ")) });
    }
    ExitCode::SUCCESS
}

pub use publish::publish;

fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn shown(p: &Path) -> String {
    let cwd = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());
    cwd.and_then(|d| p.strip_prefix(d).ok().map(|r| r.display().to_string())).unwrap_or_else(|| p.display().to_string())
}

/// `text` (a `tessel.toml`) with `name = value` in its `[dependencies]`
/// (added, replaced, or with `None` removed), and the rest as it was.
pub fn set_dependency(text: &str, name: &str, value: Option<&str>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let header = lines.iter().position(|l| l.trim() == "[dependencies]");
    let key_of = |l: &str| l.split_once('=').map(|(k, _)| k.trim().trim_matches('"').to_string());
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match header {
        Some(h) => {
            let end = (h + 1..lines.len()).find(|&i| lines[i].trim_start().starts_with('[')).unwrap_or(lines.len());
            let at = (h + 1..end).find(|&i| key_of(lines[i]).as_deref() == Some(name));
            match (at, value) {
                (Some(i), Some(v)) => out[i] = format!("{name} = {v}"),
                (Some(i), None) => {
                    out.remove(i);
                }
                (None, Some(v)) => {
                    let last = (h..end).rev().find(|&i| !lines[i].trim().is_empty()).unwrap_or(h);
                    out.insert(last + 1, format!("{name} = {v}"));
                }
                (None, None) => {}
            }
        }
        None => {
            if let Some(v) = value {
                while out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.pop();
                }
                if !out.is_empty() {
                    out.push(String::new());
                }
                out.push("[dependencies]".into());
                out.push(format!("{name} = {v}"));
            }
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependencies_are_edited_in_place() {
        let t = "tessel = \"0.8.0\"\n\n[app]\nname = \"X\"\n";
        let a = set_dependency(t, "textkit", Some("\"1.2\""));
        assert_eq!(a, "tessel = \"0.8.0\"\n\n[app]\nname = \"X\"\n\n[dependencies]\ntextkit = \"1.2\"\n");
        let b = set_dependency(&a, "geokit", Some("\"0.3\""));
        assert!(b.ends_with("[dependencies]\ntextkit = \"1.2\"\ngeokit = \"0.3\"\n"));
        let c = set_dependency(&b, "textkit", Some("\"1.4\""));
        assert!(c.contains("textkit = \"1.4\"") && !c.contains("\"1.2\""));
        let d = set_dependency(&c, "textkit", None);
        assert!(!d.contains("textkit") && d.contains("geokit"));
        let e = set_dependency("[dependencies]\na = \"1\"\n\n[app]\nname = \"X\"\n", "b", Some("\"2\""));
        assert_eq!(e, "[dependencies]\na = \"1\"\nb = \"2\"\n\n[app]\nname = \"X\"\n");
        assert_eq!(set_dependency("", "a", Some("\"1\"")), "[dependencies]\na = \"1\"\n");
    }

    #[test]
    fn library_names() {
        assert!(is_library_name("textkit") && is_library_name("geo_2d"));
        assert!(!is_library_name("TextKit") && !is_library_name("2d") && !is_library_name("text-kit") && !is_library_name(""));
    }
}
