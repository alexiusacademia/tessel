//! Linux packages of an app (`tessel build --linux --bundle`, or
//! `--bundle` on Linux): an AppImage, one file that runs on most desktops,
//! and a .deb, which Debian and Ubuntu (and their kin) install, with the
//! app in their menus.
//!
//! Both hold the program, a desktop entry (its name, icon and the files it
//! opens, `<id>.desktop`), its icon at the usual sizes, and the document
//! types it owns (shared-mime-info's XML, with their icons). The desktop
//! knows the app's windows by its ID, which they're named with (see
//! `open` in the runtime's app.rs).

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::icon::Icon;
use crate::manifest::Document;

/// What goes into the packages, as `tessel build` found it.
pub struct App<'a> {
    pub name: &'a str,
    pub id: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub author: &'a str,
    /// The manifest's `category`, as macOS names it (`productivity`).
    pub category: Option<&'a str>,
    pub icon: &'a Icon,
    /// The types it opens, each with its icon if it owns it and has one.
    pub documents: Vec<(Document, Option<Icon>)>,
    /// `x86_64` or `aarch64`.
    pub arch: &'a str,
}

/// Icon sizes the desktop looks for.
const SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];

/// The program's file name in the packages: the app's name, lower case,
/// with dashes (`My Notes` is `my-notes`).
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "app".into() } else { out }
}

/// The MIME type of a document type: its own for one the app owns
/// (`application/x-<app>-<ext>`), else the usual one for its extension.
fn mime_type(app: &App, d: &Document) -> Option<String> {
    let ext = d.extensions.first()?.to_ascii_lowercase();
    if d.owned {
        return Some(format!("application/x-{}-{}", slug(app.id), slug(&ext)));
    }
    let known = match ext.as_str() {
        "txt" | "text" => "text/plain",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "mp3" => "audio/mpeg",
        "wav" => "audio/x-wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "zip" => "application/zip",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "sqlite" | "db" => "application/vnd.sqlite3",
        "svgz" => "image/svg+xml-compressed",
        _ => return None,
    };
    Some(known.to_string())
}

/// The freedesktop menu category for macOS's (`public.app-category.x`).
fn menu_category(category: Option<&str>) -> &'static str {
    let c = category.unwrap_or("").trim_start_matches("public.app-category.");
    match c {
        "developer-tools" => "Development;",
        "education" | "reference" => "Education;",
        "entertainment" | "music" | "video" | "photography" => "AudioVideo;",
        "games" | "action-games" | "board-games" | "puzzle-games" | "strategy-games" => "Game;",
        "graphics-design" => "Graphics;",
        "business" | "finance" | "productivity" => "Office;",
        "medical" | "healthcare-fitness" => "Science;",
        "news" | "social-networking" | "weather" | "travel" => "Network;",
        "utilities" | "lifestyle" => "Utility;",
        _ => "Utility;",
    }
}

fn escape_desktop(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', " ")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The desktop entry. `exec` is how the program is started.
fn desktop_entry(app: &App, exec: &str) -> String {
    let mimes: Vec<String> = app.documents.iter().filter_map(|(d, _)| mime_type(app, d)).collect();
    let mut entry = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={exec}{}\nIcon={}\nTerminal=false\nCategories={}\nStartupWMClass={}\n",
        escape_desktop(app.name),
        if mimes.is_empty() { "" } else { " %F" },
        app.id,
        menu_category(app.category),
        app.id,
    );
    if !app.description.is_empty() {
        entry += &format!("Comment={}\n", escape_desktop(app.description));
    }
    if !mimes.is_empty() {
        entry += &format!("MimeType={};\n", mimes.join(";"));
    }
    entry
}

/// The document types the app owns, for shared-mime-info (`None` if none).
fn mime_xml(app: &App) -> Option<String> {
    let mut types = String::new();
    for (d, _) in app.documents.iter().filter(|(d, _)| d.owned) {
        let Some(mime) = mime_type(app, d) else { continue };
        let comment = d.name.clone().unwrap_or_else(|| format!("{} document", app.name));
        types += &format!("  <mime-type type=\"{mime}\">\n    <comment>{}</comment>\n", escape_xml(&comment));
        for ext in &d.extensions {
            types += &format!("    <glob pattern=\"*.{}\"/>\n", escape_xml(&ext.to_ascii_lowercase()));
        }
        types += &format!("    <icon name=\"{}\"/>\n  </mime-type>\n", mime.replace('/', "-"));
    }
    (!types.is_empty())
        .then(|| format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n{types}</mime-info>\n"))
}

/// A file in a package: where, how it may be used (its mode), and what.
struct Entry {
    path: String,
    mode: u32,
    data: Data,
}

enum Data {
    Bytes(Vec<u8>),
    Dir,
    Link(String),
}

/// What both packages hold, under `usr/` (the program in `usr/bin`).
fn usr_files(app: &App, exe: &[u8], exe_name: &str, exec: &str) -> Vec<Entry> {
    let file = |path: String, mode: u32, bytes: Vec<u8>| Entry { path, mode, data: Data::Bytes(bytes) };
    let mut files = vec![
        file(format!("usr/bin/{exe_name}"), 0o755, exe.to_vec()),
        file(format!("usr/share/applications/{}.desktop", app.id), 0o644, desktop_entry(app, exec).into_bytes()),
    ];
    for &size in SIZES {
        files.push(file(format!("usr/share/icons/hicolor/{size}x{size}/apps/{}.png", app.id), 0o644, app.icon.png_at(size)));
    }
    if let Some(xml) = mime_xml(app) {
        files.push(file(format!("usr/share/mime/packages/{}.xml", app.id), 0o644, xml.into_bytes()));
    }
    for (d, icon) in &app.documents {
        let (Some(icon), Some(mime)) = (icon, mime_type(app, d)) else { continue };
        for &size in &[48, 128, 256] {
            files.push(file(format!("usr/share/icons/hicolor/{size}x{size}/mimetypes/{}.png", mime.replace('/', "-")), 0o644, icon.png_at(size)));
        }
    }
    files
}

/// Every folder the files are in, before them, each once.
fn with_folders(files: Vec<Entry>) -> Vec<Entry> {
    let mut folders: Vec<String> = Vec::new();
    for f in &files {
        let mut at = Path::new(&f.path).parent();
        let mut chain = Vec::new();
        while let Some(p) = at.filter(|p| !p.as_os_str().is_empty()) {
            chain.push(p.to_string_lossy().into_owned());
            at = p.parent();
        }
        for p in chain.into_iter().rev() {
            if !folders.contains(&p) {
                folders.push(p);
            }
        }
    }
    let mut out: Vec<Entry> = folders.into_iter().map(|path| Entry { path, mode: 0o755, data: Data::Dir }).collect();
    out.extend(files);
    out
}

/// Makes the AppImage and the .deb of the program at `exe`, next to it.
/// `runtime` is the AppImage runtime (the program at the start of an
/// AppImage, which mounts the rest and runs the app). Returns their paths.
pub fn make(exe: &Path, app: &App, runtime: &Path) -> Result<Vec<PathBuf>, String> {
    let program = std::fs::read(exe).map_err(|e| format!("can't read `{}`: {e}", exe.display()))?;
    let exe_name = slug(app.name);
    let dir = exe.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let appimage = dir.join(format!("{}-{}.AppImage", app.name.replace(' ', "_"), app.arch));
    write_appimage(&appimage, app, &program, &exe_name, runtime)?;
    let deb_arch = if app.arch == "aarch64" { "arm64" } else { "amd64" };
    let deb = dir.join(format!("{exe_name}_{}_{deb_arch}.deb", deb_version(app.version)));
    write_deb(&deb, app, &program, &exe_name, deb_arch)?;
    Ok(vec![appimage, deb])
}

/// A version as Debian's allow (digits, letters and `.+~-`; starting with a digit).
fn deb_version(v: &str) -> String {
    let v: String = v.chars().filter(|c| c.is_ascii_alphanumeric() || ".+~-".contains(*c)).collect();
    if v.starts_with(|c: char| c.is_ascii_digit()) { v } else { format!("0{v}") }
}

// ===== AppImage =====

fn write_appimage(out: &Path, app: &App, program: &[u8], exe_name: &str, runtime: &Path) -> Result<(), String> {
    use backhand::compression::Compressor;
    use backhand::{FilesystemCompressor, FilesystemWriter, NodeHeader};

    let runtime = std::fs::read(runtime).map_err(|e| format!("can't read the AppImage runtime `{}`: {e}", runtime.display()))?;
    // AppRun starts the program; the desktop entry and icon at the top are
    // what AppImage tools show.
    let mut files = usr_files(app, program, exe_name, exe_name);
    files.push(Entry { path: "AppRun".into(), mode: 0o777, data: Data::Link(format!("usr/bin/{exe_name}")) });
    files.push(Entry { path: format!("{}.desktop", app.id), mode: 0o644, data: Data::Bytes(desktop_entry(app, exe_name).into_bytes()) });
    files.push(Entry { path: format!("{}.png", app.id), mode: 0o644, data: Data::Bytes(app.icon.png_at(256)) });
    files.push(Entry { path: ".DirIcon".into(), mode: 0o777, data: Data::Link(format!("{}.png", app.id)) });
    let files = with_folders(files);

    let mut fs = FilesystemWriter::default();
    fs.set_compressor(FilesystemCompressor::new(Compressor::Gzip, None).map_err(|e| e.to_string())?);
    fs.set_root_mode(0o755);
    fs.set_only_root_id();
    let header = |mode: u32| NodeHeader { permissions: mode as u16, uid: 0, gid: 0, mtime: 0 };
    for f in &files {
        let path = format!("/{}", f.path);
        let made = match &f.data {
            Data::Dir => fs.push_dir(&path, header(f.mode)),
            Data::Bytes(b) => fs.push_file(std::io::Cursor::new(b.clone()), &path, header(f.mode)),
            Data::Link(to) => fs.push_symlink(to.as_str(), &path, header(f.mode)),
        };
        made.map_err(|e| format!("can't put `{}` in the AppImage: {e}", f.path))?;
    }
    let mut image = std::io::Cursor::new(Vec::new());
    fs.write(&mut image).map_err(|e| format!("can't make the AppImage's image: {e}"))?;
    let mut bytes = runtime;
    bytes.extend_from_slice(&image.into_inner());
    write_executable(out, &bytes)
}

fn write_executable(out: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(out, bytes).map_err(|e| format!("can't write `{}`: {e}", out.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

// ===== .deb =====

fn write_deb(out: &Path, app: &App, program: &[u8], exe_name: &str, arch: &str) -> Result<(), String> {
    let files = with_folders(usr_files(app, program, exe_name, &format!("/usr/bin/{exe_name}")));
    let installed_kb = files.iter().map(|f| if let Data::Bytes(b) = &f.data { b.len() } else { 0 }).sum::<usize>().div_ceil(1024);
    let data = gzip(&tar(&files))?;
    let maintainer = if app.author.is_empty() { app.name.to_string() } else { app.author.to_string() };
    let maintainer = if maintainer.contains('<') { maintainer } else { format!("{maintainer} <nobody@localhost>") };
    let summary = if app.description.is_empty() { app.name.to_string() } else { app.description.lines().next().unwrap_or("").to_string() };
    let control = format!(
        "Package: {exe_name}\nVersion: {}\nArchitecture: {arch}\nMaintainer: {maintainer}\nInstalled-Size: {installed_kb}\nDepends: libc6 (>= 2.28), libgcc-s1 | libgcc1\nSection: misc\nPriority: optional\nDescription: {summary}\n {} is a Tessel app.\n",
        deb_version(app.version),
        app.name,
    );
    let control_tar = gzip(&tar(&[
        Entry { path: ".".into(), mode: 0o755, data: Data::Dir },
        Entry { path: "control".into(), mode: 0o644, data: Data::Bytes(control.into_bytes()) },
    ]))?;
    // An `ar` archive of the three, in this order.
    let mut deb = b"!<arch>\n".to_vec();
    for (name, body) in [("debian-binary", &b"2.0\n"[..]), ("control.tar.gz", &control_tar[..]), ("data.tar.gz", &data[..])] {
        deb.extend_from_slice(format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, "100644", body.len()).as_bytes());
        deb.extend_from_slice(body);
        if body.len() % 2 == 1 {
            deb.push(b'\n');
        }
    }
    std::fs::write(out, deb).map_err(|e| format!("can't write `{}`: {e}", out.display()))
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(bytes).map_err(|e| e.to_string())?;
    z.finish().map_err(|e| e.to_string())
}

/// A tar archive (ustar) of `files`, owned by root, paths starting `./`.
fn tar(files: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    for f in files {
        let path = if f.path == "." { "./".to_string() } else { format!("./{}", f.path) };
        let (kind, size, link) = match &f.data {
            Data::Bytes(b) => (b'0', b.len(), String::new()),
            Data::Dir => (b'5', 0, String::new()),
            Data::Link(to) => (b'2', 0, to.clone()),
        };
        let path = if kind == b'5' && !path.ends_with('/') { format!("{path}/") } else { path };
        let mut h = [0u8; 512];
        // Long paths: the part up to a `/` goes in `prefix`.
        let (prefix, name) = if path.len() <= 100 {
            (String::new(), path.clone())
        } else {
            let cut = path[..path.len().min(155)].rfind('/').unwrap_or(0);
            (path[..cut].to_string(), path[cut + 1..].to_string())
        };
        let put = |h: &mut [u8; 512], at: usize, len: usize, s: &[u8]| h[at..at + s.len().min(len)].copy_from_slice(&s[..s.len().min(len)]);
        put(&mut h, 0, 100, name.as_bytes());
        put(&mut h, 100, 8, format!("{:07o}\0", f.mode).as_bytes());
        put(&mut h, 108, 8, b"0000000\0");
        put(&mut h, 116, 8, b"0000000\0");
        put(&mut h, 124, 12, format!("{size:011o}\0").as_bytes());
        put(&mut h, 136, 12, b"00000000000\0");
        h[156] = kind;
        put(&mut h, 157, 100, link.as_bytes());
        put(&mut h, 257, 6, b"ustar\0");
        put(&mut h, 263, 2, b"00");
        put(&mut h, 265, 32, b"root");
        put(&mut h, 297, 32, b"root");
        put(&mut h, 345, 155, prefix.as_bytes());
        // The checksum counts its own field as spaces.
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        put(&mut h, 148, 8, format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        if let Data::Bytes(b) = &f.data {
            out.extend_from_slice(b);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
    }
    out.resize(out.len() + 1024, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_programs_and_versions_as_debian_takes_them() {
        assert_eq!(slug("My Notes 2"), "my-notes-2");
        assert_eq!(slug("  Todos!  "), "todos");
        assert_eq!(deb_version("1.2.3"), "1.2.3");
        assert_eq!(deb_version("beta 2"), "0beta2");
    }

    #[test]
    fn tar_headers_have_good_checksums() {
        let t = tar(&[Entry { path: "usr/bin/x".into(), mode: 0o755, data: Data::Bytes(b"hi".to_vec()) }]);
        assert_eq!(t.len(), 512 + 512 + 1024);
        let mut h = t[..512].to_vec();
        let stored = u32::from_str_radix(std::str::from_utf8(&h[148..154]).unwrap(), 8).unwrap();
        h[148..156].copy_from_slice(b"        ");
        assert_eq!(stored, h.iter().map(|&b| b as u32).sum::<u32>());
        assert_eq!(&t[..11], b"./usr/bin/x");
    }
}
