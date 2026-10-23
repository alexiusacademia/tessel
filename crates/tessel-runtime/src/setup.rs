//! The installer of Windows apps. `tessel build --bundle` for Windows links
//! this, instead of a program, into `<Name> Setup.exe`, with the app's
//! `.exe` inside it as a resource (see `tessel-cli/src/setup.rs` for the
//! format). Run, it installs the app for the user who runs it, with no
//! administrator rights:
//!
//! - the app in `%LOCALAPPDATA%\Programs\<Name>`, with a copy of the
//!   installer next to it as `uninstall.exe`,
//! - a shortcut in the Start menu (and on the desktop, if asked),
//! - the app in Settings' list of installed apps, which removes it with
//!   `uninstall.exe /uninstall`,
//! - the app's document types, so Explorer opens those files with it.
//!
//! `/S` installs (or with `/uninstall` removes) without asking anything,
//! and the exit code tells how it went.
//!
//! Author: Alexius Academia
//! Date: 2026-10-24

use std::io::Read;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::LibraryLoader::{FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::Controls::{
    LoadIconWithScaleDown, TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG,
    TD_ERROR_ICON, TDCBF_CANCEL_BUTTON, TDCBF_CLOSE_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_USE_HICON_MAIN,
    TDM_NAVIGATE_PAGE, TDN_BUTTON_CLICKED, TDN_VERIFICATION_CLICKED, TaskDialogIndirect,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_UserProgramFiles, IShellLinkW, KF_FLAG_CREATE, SHCNE_ASSOCCHANGED,
    SHCNF_IDLIST, SHChangeNotify, SHGetKnownFolderPath, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{HICON, SendMessageW};
use windows::core::{GUID, HRESULT, HSTRING, Interface, PCWSTR};

/// What the installer's resource starts with.
const MAGIC: &[u8; 8] = b"TSLSETUP";
/// The resource: RCDATA number 1.
const RT_RCDATA: u16 = 10;
/// The installer's copy of itself in the app's folder.
const UNINSTALLER: &str = "uninstall.exe";
/// Settings' list of installed apps (in HKEY_CURRENT_USER).
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
/// The buttons the dialogs add (besides Cancel and Close).
const GO: i32 = 100;
const OPEN: i32 = 101;

/// What the installer installs: the app's details and its `.exe`.
struct Package {
    name: String,
    id: String,
    version: String,
    publisher: String,
    /// The app's file name, like `Forebay.exe`.
    exe: String,
    /// Each document type: its name and its extensions (without dots).
    documents: Vec<(String, Vec<String>)>,
    /// The `.exe`, compressed (zlib).
    compressed: &'static [u8],
}

/// Runs the installer (or with `/uninstall`, the uninstaller). Returns the
/// exit code: 0 when it's done, 1 when it failed, 2 when it was cancelled.
#[unsafe(no_mangle)]
pub extern "C" fn tessel_setup_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).map(|a| a.to_ascii_lowercase()).collect();
    let silent = args.iter().any(|a| a == "/s" || a == "--silent");
    let uninstall = args.iter().any(|a| a == "/uninstall" || a == "--uninstall");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let package = match package() {
        Ok(p) => p,
        Err(e) => {
            if !silent {
                Ui::message("Setup", &Page::error("This installer is damaged", &e));
            }
            eprintln!("setup: {e}");
            return 1;
        }
    };
    let mut ui = Ui { package, uninstall, desktop: false, done: false, pages: Vec::new(), remove_folder: None };
    let code = if silent {
        match ui.work() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("setup: {e}");
                1
            }
        }
    } else {
        let first = ui.first_page();
        match ui.run(first) {
            OPEN => {
                let _ = std::process::Command::new(ui.package.install_dir().join(&ui.package.exe)).spawn();
                0
            }
            _ if ui.done => 0,
            _ => 2,
        }
    };
    if let Some(folder) = ui.remove_folder.take() {
        remove_later(&folder);
    }
    code
}

/// The package inside this `.exe`.
fn package() -> Result<Package, String> {
    let bytes: &'static [u8] = unsafe {
        let module = GetModuleHandleW(None).map_err(|e| e.message())?;
        let found = FindResourceW(Some(module), PCWSTR(1 as _), PCWSTR(RT_RCDATA as _));
        if found.is_invalid() {
            return Err("It has nothing in it to install.".into());
        }
        let size = SizeofResource(Some(module), found) as usize;
        let data = LoadResource(Some(module), found).map_err(|e| e.message())?;
        std::slice::from_raw_parts(LockResource(data) as *const u8, size)
    };
    let bad = || "Its contents can't be read.".to_string();
    let rest = bytes.strip_prefix(MAGIC.as_slice()).ok_or_else(bad)?;
    let len = u32::from_le_bytes(rest.get(..4).ok_or_else(bad)?.try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(rest.get(4..4 + len).ok_or_else(bad)?).map_err(|_| bad())?;
    let text = |key: &str| header[key].as_str().unwrap_or_default().to_string();
    let documents = header["documents"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|d| {
                    let extensions = d["extensions"].as_array().into_iter().flatten();
                    let extensions = extensions.filter_map(|e| e.as_str().map(str::to_string)).collect();
                    (d["name"].as_str().unwrap_or_default().to_string(), extensions)
                })
                .collect()
        })
        .unwrap_or_default();
    let package = Package {
        name: text("name"),
        id: text("id"),
        version: text("version"),
        publisher: text("publisher"),
        exe: text("exe"),
        documents,
        compressed: &rest[4 + len..],
    };
    if package.name.is_empty() || package.id.is_empty() || package.exe.is_empty() {
        return Err(bad());
    }
    Ok(package)
}

impl Package {
    /// The app's key in Settings' list of installed apps.
    fn uninstall_key(&self) -> String {
        format!(r"{UNINSTALL_KEY}\{}", self.id)
    }

    /// Where the app is installed: where it was before, or else
    /// `%LOCALAPPDATA%\Programs\<Name>`.
    fn install_dir(&self) -> PathBuf {
        if let Some(dir) = get_string(&self.uninstall_key(), "InstallLocation").filter(|d| Path::new(d).is_dir()) {
            return PathBuf::from(dir);
        }
        let programs = known_folder(&FOLDERID_UserProgramFiles).unwrap_or_else(|_| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs")
        });
        programs.join(file_name(&self.name))
    }

    /// The version already installed, if the app is.
    fn installed_version(&self) -> Option<String> {
        get_string(&self.uninstall_key(), "DisplayVersion")
    }

    fn shortcuts(&self) -> Vec<PathBuf> {
        let name = format!("{}.lnk", file_name(&self.name));
        [&FOLDERID_Programs, &FOLDERID_Desktop].iter().filter_map(|f| known_folder(f).ok()).map(|d| d.join(&name)).collect()
    }

    /// The name a document type is registered under (its "ProgID").
    fn prog_id(&self, extensions: &[String]) -> String {
        format!("{}.{}", self.id, extensions.first().map_or("document", String::as_str))
    }

    fn install(&self, desktop: bool) -> Result<(), String> {
        let dir = self.install_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create the folder {}: {e}", dir.display()))?;
        remove_old_copies(&dir);
        let mut exe = Vec::new();
        flate2::read::ZlibDecoder::new(self.compressed)
            .read_to_end(&mut exe)
            .map_err(|_| "The app inside the installer is damaged.".to_string())?;
        let app = dir.join(&self.exe);
        put(&app, &exe)?;
        // The installer, which removes the app too (unless it's that copy).
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let uninstaller = dir.join(UNINSTALLER);
        let size = exe.len() as u64 + me.metadata().map_or(0, |m| m.len());
        if !same_file(&me, &uninstaller) {
            let bytes = std::fs::read(&me).map_err(|e| format!("Can't read the installer: {e}"))?;
            put(&uninstaller, &bytes)?;
        }

        // The Start menu (and the desktop).
        let shortcuts = self.shortcuts();
        for (i, link) in shortcuts.iter().enumerate() {
            if i == 0 || desktop {
                shortcut(link, &app, &self.name).map_err(|e| format!("Can't add the shortcut {}: {}", link.display(), e.message()))?;
            }
        }

        // Settings' list of installed apps.
        let key = create_key(&self.uninstall_key())?;
        let quoted = |p: &Path| format!("\"{}\"", p.display());
        set_string(&key, "DisplayName", &self.name)?;
        set_string(&key, "DisplayVersion", &self.version)?;
        set_string(&key, "DisplayIcon", &format!("{},0", app.display()))?;
        set_string(&key, "InstallLocation", &dir.display().to_string())?;
        set_string(&key, "UninstallString", &format!("{} /uninstall", quoted(&uninstaller)))?;
        set_string(&key, "QuietUninstallString", &format!("{} /uninstall /S", quoted(&uninstaller)))?;
        set_string(&key, "InstallDate", &chrono::Local::now().format("%Y%m%d").to_string())?;
        if !self.publisher.is_empty() {
            set_string(&key, "Publisher", &self.publisher)?;
        }
        set_dword(&key, "EstimatedSize", (size / 1024) as u32)?;
        set_dword(&key, "NoModify", 1)?;
        set_dword(&key, "NoRepair", 1)?;

        // The app's documents open with it.
        for (name, extensions) in &self.documents {
            let prog_id = self.prog_id(extensions);
            let classes = format!(r"Software\Classes\{prog_id}");
            set_string(&create_key(&classes)?, "", name)?;
            set_string(&create_key(&format!(r"{classes}\DefaultIcon"))?, "", &format!("{},0", app.display()))?;
            set_string(&create_key(&format!(r"{classes}\shell\open\command"))?, "", &format!("{} \"%1\"", quoted(&app)))?;
            for ext in extensions {
                let key = format!(r"Software\Classes\.{ext}");
                // (A type another app already opens stays with it; the app is
                // offered in Open with.)
                let current = get_string(&key, "").unwrap_or_default();
                let ext_key = create_key(&key)?;
                if current.is_empty() || current == prog_id {
                    set_string(&ext_key, "", &prog_id)?;
                }
                set_string(&create_key(&format!(r"{key}\OpenWithProgids"))?, &prog_id, "")?;
            }
        }
        if !self.documents.is_empty() {
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        }
        Ok(())
    }

    /// Removes the app. Its folder (with the uninstaller) is removed after
    /// the uninstaller quits, by `remove_later`: returns the folder.
    fn uninstall(&self) -> Result<PathBuf, String> {
        let dir = self.install_dir();
        let app = dir.join(&self.exe);
        if app.exists() && std::fs::remove_file(&app).is_err() {
            return Err(format!("{} is open. Quit it, then try again.", self.name));
        }
        remove_old_copies(&dir);
        for link in self.shortcuts() {
            let _ = std::fs::remove_file(link);
        }
        for (_, extensions) in &self.documents {
            let prog_id = self.prog_id(extensions);
            delete_tree(&format!(r"Software\Classes\{prog_id}"));
            for ext in extensions {
                let key = format!(r"Software\Classes\.{ext}");
                delete_value(&format!(r"{key}\OpenWithProgids"), &prog_id);
                if get_string(&key, "").as_deref() == Some(prog_id.as_str()) {
                    delete_value(&key, "");
                }
            }
        }
        if !self.documents.is_empty() {
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        }
        delete_tree(&self.uninstall_key());
        Ok(dir)
    }
}

/// The installer's dialogs, and what they've done.
struct Ui {
    package: Package,
    uninstall: bool,
    /// Whether "Add a shortcut to the desktop" is ticked.
    desktop: bool,
    /// Whether the work is done.
    done: bool,
    /// The pages shown, kept until the dialog closes (it reads their text,
    /// which stays where it is when a page moves).
    pages: Vec<Page>,
    /// The folder to remove once the uninstaller has quit.
    remove_folder: Option<PathBuf>,
}

impl Ui {
    /// Installs or uninstalls.
    fn work(&mut self) -> Result<(), String> {
        if self.uninstall {
            self.remove_folder = Some(self.package.uninstall()?);
        } else {
            self.package.install(self.desktop)?;
        }
        self.done = true;
        Ok(())
    }

    fn first_page(&self) -> Page {
        let p = &self.package;
        let title = format!("{} Setup", p.name);
        if self.uninstall {
            let mut page = Page::new(&title, &format!("Remove {}?", p.name), &format!("{} will be removed from this computer. The documents you made with it stay where they are.", p.name));
            page.buttons = vec![(GO, "Remove".into())];
            page.common = TDCBF_CANCEL_BUTTON;
            return page;
        }
        let instruction = match p.installed_version() {
            Some(v) if v != p.version => format!("Update {} from {v} to {}?", p.name, p.version),
            Some(_) => format!("Install {} {} again?", p.name, p.version),
            None => format!("Install {} {}?", p.name, p.version),
        };
        let content = format!("It's installed for you only, in {}.", p.install_dir().display());
        let mut page = Page::new(&title, &instruction, &content);
        page.buttons = vec![(GO, "Install".into())];
        page.common = TDCBF_CANCEL_BUTTON;
        page.verification = Some("Add a shortcut to the desktop".into());
        page
    }

    fn done_page(&self) -> Page {
        let p = &self.package;
        let title = format!("{} Setup", p.name);
        if self.uninstall {
            let mut page = Page::new(&title, &format!("{} is removed", p.name), "");
            page.common = TDCBF_CLOSE_BUTTON;
            return page;
        }
        let mut page = Page::new(&title, &format!("{} is installed", p.name), "You'll find it in the Start menu.");
        page.buttons = vec![(OPEN, format!("Open {}", p.name))];
        page.common = TDCBF_CLOSE_BUTTON;
        page
    }

    /// Shows `page` (and the pages after it). Returns the button that closed it.
    fn run(&mut self, mut page: Page) -> i32 {
        let config = page.config(self as *mut Ui as isize);
        self.pages.push(page);
        let mut button = 0;
        unsafe {
            let _ = TaskDialogIndirect(&config, Some(&mut button), None, None);
        }
        button
    }

    /// A dialog with just a message.
    fn message(title: &str, page: &Page) {
        let mut page = Page { title: title.into(), ..page.clone() };
        let config = page.config(0);
        unsafe {
            let _ = TaskDialogIndirect(&config, None, None, None);
        }
    }

    /// Replaces the dialog's page with `page`.
    fn navigate(&mut self, hwnd: HWND, mut page: Page) {
        let config = page.config(self as *mut Ui as isize);
        self.pages.push(page);
        unsafe {
            SendMessageW(hwnd, TDM_NAVIGATE_PAGE.0 as u32, None, Some(LPARAM(&config as *const _ as isize)));
        }
    }
}

unsafe extern "system" fn callback(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, wparam: WPARAM, _: LPARAM, data: isize) -> HRESULT {
    if data == 0 {
        return HRESULT(0);
    }
    let ui = unsafe { &mut *(data as *mut Ui) };
    if msg == TDN_VERIFICATION_CLICKED {
        ui.desktop = wparam.0 != 0;
    } else if msg == TDN_BUTTON_CLICKED && wparam.0 as i32 == GO {
        let next = match ui.work() {
            Ok(()) => ui.done_page(),
            Err(e) => {
                let what = if ui.uninstall { "removed" } else { "installed" };
                Page::error(&format!("{} couldn't be {what}", ui.package.name), &e)
            }
        };
        let next = Page { title: format!("{} Setup", ui.package.name), ..next };
        ui.navigate(hwnd, next);
        // (The dialog stays open, on the new page.)
        return HRESULT(1);
    }
    HRESULT(0)
}

/// One page of a dialog.
#[derive(Clone)]
struct Page {
    title: String,
    instruction: String,
    content: String,
    verification: Option<String>,
    buttons: Vec<(i32, String)>,
    common: TASKDIALOG_COMMON_BUTTON_FLAGS,
    error: bool,
    // The text as Windows reads it, kept for as long as the page is shown.
    wide: Vec<Vec<u16>>,
    raw_buttons: Vec<TASKDIALOG_BUTTON>,
}

impl Page {
    fn new(title: &str, instruction: &str, content: &str) -> Page {
        Page {
            title: title.into(),
            instruction: instruction.into(),
            content: content.into(),
            verification: None,
            buttons: Vec::new(),
            common: TDCBF_CLOSE_BUTTON,
            error: false,
            wide: Vec::new(),
            raw_buttons: Vec::new(),
        }
    }

    fn error(instruction: &str, content: &str) -> Page {
        Page { error: true, ..Page::new("Setup", instruction, content) }
    }

    /// The dialog's settings for this page. They point into the page.
    fn config(&mut self, data: isize) -> TASKDIALOGCONFIG {
        let mut text = |s: &str| -> PCWSTR {
            if s.is_empty() {
                return PCWSTR::null();
            }
            self.wide.push(s.encode_utf16().chain([0]).collect());
            PCWSTR(self.wide.last().unwrap().as_ptr())
        };
        let title = text(&self.title.clone());
        let instruction = text(&self.instruction.clone());
        let content = text(&self.content.clone());
        let verification = text(&self.verification.clone().unwrap_or_default());
        let buttons: Vec<(i32, PCWSTR)> = self.buttons.clone().iter().map(|(id, label)| (*id, text(label))).collect();
        self.raw_buttons = buttons.iter().map(|&(id, label)| TASKDIALOG_BUTTON { nButtonID: id, pszButtonText: label }).collect();
        let mut config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
            dwCommonButtons: self.common,
            pszWindowTitle: title,
            pszMainInstruction: instruction,
            pszContent: content,
            pszVerificationText: verification,
            cButtons: self.raw_buttons.len() as u32,
            pButtons: self.raw_buttons.as_ptr(),
            pfCallback: Some(callback),
            lpCallbackData: data,
            ..Default::default()
        };
        if self.error {
            config.Anonymous1.pszMainIcon = TD_ERROR_ICON;
        } else if let Some(icon) = app_icon() {
            config.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_USE_HICON_MAIN;
            config.Anonymous1.hMainIcon = icon;
        }
        if let Some(module) = unsafe { GetModuleHandleW(None).ok() } {
            config.hInstance = module.into();
        }
        config
    }
}

/// The app's icon (the installer's own), at the size dialogs show it.
fn app_icon() -> Option<HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let size = (32 * GetDpiForSystem().max(96) / 96) as i32;
        LoadIconWithScaleDown(Some(module.into()), PCWSTR(1 as _), size, size).ok()
    }
}

/// A name with only what file names can have.
fn file_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !r#"<>:"/\|?*"#.contains(*c) && !c.is_control()).collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() { "App".into() } else { clean }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Writes `bytes` to `path`. A running `.exe` can't be replaced, but it can
/// be renamed: it's moved out of the way (and removed the next time).
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() && std::fs::remove_file(path).is_err() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let moved = (1..100).map(|n| path.with_file_name(format!("{name}.{n}.old"))).find(|old| {
            let _ = std::fs::remove_file(old);
            !old.exists() && std::fs::rename(path, old).is_ok()
        });
        if moved.is_none() {
            return Err(format!("Can't replace {}. Close it, then try again.", path.display()));
        }
    }
    std::fs::write(path, bytes).map_err(|e| format!("Can't write {}: {e}", path.display()))
}

/// Removes the copies `put` moved out of the way (those still running stay).
fn remove_old_copies(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".old") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Removes the uninstaller and its folder, once it has quit (a running
/// `.exe` can't be removed): a hidden command waits a moment, then does it.
/// The folder is left if anything else is in it.
fn remove_later(dir: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let uninstaller = dir.join(UNINSTALLER);
    let script = format!(
        "ping -n 3 127.0.0.1 > nul & del /f /q \"{}\" & rmdir \"{}\"",
        uninstaller.display(),
        dir.display()
    );
    let _ = std::process::Command::new("cmd.exe").arg("/c").raw_arg(script).creation_flags(CREATE_NO_WINDOW).spawn();
}

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_CREATE, None).map_err(|e| e.message())?;
        let text = path.to_string().unwrap_or_default();
        CoTaskMemFree(Some(path.0 as *const _));
        Ok(PathBuf::from(text))
    }
}

fn shortcut(link: &Path, target: &Path, description: &str) -> windows::core::Result<()> {
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            shell.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        shell.SetDescription(&HSTRING::from(description))?;
        shell.cast::<IPersistFile>()?.Save(&HSTRING::from(link.as_os_str()), true)
    }
}

// ---- The registry (all in HKEY_CURRENT_USER) ----

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn create_key(path: &str) -> Result<Key, String> {
    let mut key = HKEY::default();
    let error = unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None)
    };
    if error != ERROR_SUCCESS {
        return Err(format!("Can't write to the registry ({path}, error {}).", error.0));
    }
    Ok(Key(key))
}

fn set_string(key: &Key, name: &str, value: &str) -> Result<(), String> {
    let wide: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let error = unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_SZ, Some(&wide)) };
    if error != ERROR_SUCCESS {
        return Err(format!("Can't write to the registry ({name}, error {}).", error.0));
    }
    Ok(())
}

fn set_dword(key: &Key, name: &str, value: u32) -> Result<(), String> {
    let error = unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes())) };
    if error != ERROR_SUCCESS {
        return Err(format!("Can't write to the registry ({name}, error {}).", error.0));
    }
    Ok(())
}

/// A text value (`""` names the key's default value).
fn get_string(path: &str, name: &str) -> Option<String> {
    let (path, name) = (HSTRING::from(path), HSTRING::from(name));
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, &path, &name, RRF_RT_REG_SZ, None, None, Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let mut buffer = vec![0u16; size as usize / 2 + 1];
        if RegGetValueW(HKEY_CURRENT_USER, &path, &name, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr() as *mut _), Some(&mut size))
            != ERROR_SUCCESS
        {
            return None;
        }
        let text = String::from_utf16_lossy(&buffer[..size as usize / 2]);
        Some(text.trim_end_matches('\0').to_string())
    }
}

fn delete_tree(path: &str) {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path));
        let _ = windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(path));
    }
}

fn delete_value(path: &str, name: &str) {
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(path), &HSTRING::from(name));
    }
}
