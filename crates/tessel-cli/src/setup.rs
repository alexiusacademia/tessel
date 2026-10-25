//! The installer of a Windows app: `tessel build --bundle` for Windows makes
//! `<Name> Setup.exe`, which installs the app for the user who runs it (see
//! the runtime's `setup.rs` for what it does).
//!
//! The installer is the runtime's `tessel_setup_main`, linked like an app,
//! with the app's icon and details, and the app inside it: resource RCDATA 1
//! is `TSLSETUP`, the length of a JSON header (4 bytes, little-endian), the
//! header (the app's name, id, version, publisher, file name and document
//! types), then the app's `.exe` compressed with zlib.
//!
//! Author: Alexius Academia
//! Date: 2026-10-24

use std::io::Write;

/// What the installer says about the app.
pub struct Package {
    pub name: String,
    pub id: String,
    pub version: String,
    pub publisher: String,
    /// The app's file name, like `Forebay.exe`.
    pub exe: String,
    pub documents: Vec<DocumentType>,
}

/// A kind of file the app opens.
pub struct DocumentType {
    pub name: String,
    pub extensions: Vec<String>,
    /// The app's own: its files open with it, rather than it only being
    /// offered for them.
    pub owned: bool,
    /// The icon group of its icon in the app's `.exe` (the app's icon
    /// otherwise).
    pub icon: Option<u16>,
}

/// The function of the runtime the installer's `main` calls.
pub const ENTRY: &str = "tessel_setup_main";

/// The installer's file name.
pub fn file_name(app_name: &str) -> String {
    let clean: String = app_name.chars().filter(|c| !r#"<>:"/\|?*"#.contains(*c) && !c.is_control()).collect();
    format!("{} Setup.exe", clean.trim())
}

/// The resources that make the installer what it is, besides its icon and
/// version: the package (the app inside it) and its manifest.
pub fn resources(package: &Package, exe: &[u8]) -> Vec<u8> {
    let documents: Vec<serde_json::Value> = package
        .documents
        .iter()
        .map(|d| serde_json::json!({ "name": d.name, "extensions": d.extensions, "owned": d.owned, "icon": d.icon }))
        .collect();
    let header = serde_json::json!({
        "name": package.name,
        "id": package.id,
        "version": package.version,
        "publisher": package.publisher,
        "exe": package.exe,
        "documents": documents,
    })
    .to_string();
    let mut payload = b"TSLSETUP".to_vec();
    payload.extend((header.len() as u32).to_le_bytes());
    payload.extend(header.as_bytes());
    let mut z = flate2::write::ZlibEncoder::new(payload, flate2::Compression::best());
    z.write_all(exe).expect("compressing into memory");
    let payload = z.finish().expect("compressing into memory");

    const RT_RCDATA: u16 = 10;
    const RT_MANIFEST: u16 = 24;
    let mut out = Vec::new();
    crate::icon::resource(&mut out, RT_RCDATA, 1, 0x0030, &payload);
    crate::icon::resource(&mut out, RT_MANIFEST, 1, 0x0030, MANIFEST.as_bytes());
    out
}

/// The installer's manifest: the dialogs of Windows 10 and 11 (common
/// controls 6), sharp on high-DPI screens, and no administrator rights
/// (without it, Windows asks for them for programs called "Setup").
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_holds_the_app() {
        let package = Package {
            name: "Notes".into(),
            id: "com.example.notes".into(),
            version: "1.2.0".into(),
            publisher: "Ada".into(),
            exe: "Notes.exe".into(),
            documents: vec![DocumentType { name: "Note".into(), extensions: vec!["note".into()], owned: true, icon: Some(2) }],
        };
        let exe = b"MZ pretend app ".repeat(100);
        let res = resources(&package, &exe);
        // The first resource: its data's size, then the 32-byte header.
        let size = u32::from_le_bytes(res[..4].try_into().unwrap()) as usize;
        let data = &res[32..32 + size];
        assert_eq!(&data[..8], b"TSLSETUP");
        let len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&data[12..12 + len]).unwrap();
        assert_eq!(header["exe"], "Notes.exe");
        assert_eq!(header["documents"][0]["extensions"][0], "note");
        assert_eq!(header["documents"][0]["owned"], true);
        assert_eq!(header["documents"][0]["icon"], 2);
        let mut app = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&data[12 + len..]), &mut app).unwrap();
        assert_eq!(app, exe);
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("Forebay"), "Forebay Setup.exe");
        assert_eq!(file_name("A/B: C"), "AB C Setup.exe");
    }
}
