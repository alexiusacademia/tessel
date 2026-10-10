//! App icons. A program's icon is `icon.png` next to its `.tsl` files, or
//! Tessel's own when there's none. It becomes `AppIcon.icns` in a macOS app
//! bundle and an icon resource in a Windows `.exe`, in the sizes each needs,
//! made here so that building apps needs no other tools.

use std::path::Path;

/// Tessel's icon (drawn by assets/make-icon.py).
const TESSEL_ICON: &[u8] = include_bytes!("../assets/icon.png");

/// The file name of a program's own icon.
pub const FILE_NAME: &str = "icon.png";

/// A square picture: `size` × `size` pixels of red, green, blue and alpha.
pub struct Icon {
    size: u32,
    rgba: Vec<u8>,
}

impl Icon {
    /// The icon `chosen` (the manifest's `icon`), or else the program's:
    /// `icon.png` next to it (it's a `.tsl` file or a folder), or Tessel's
    /// with `None` and for programs without one.
    pub fn of_program(program: Option<&Path>, chosen: Option<&Path>) -> Result<Icon, String> {
        if let Some(file) = chosen {
            let bytes = std::fs::read(file).map_err(|e| format!("can't read the app's icon `{}`: {e}", file.display()))?;
            return Icon::decode(&bytes).map_err(|e| format!("can't use `{}` as the app's icon: {e}", file.display()));
        }
        let folder = program.map(|p| if p.is_dir() { p } else { p.parent().unwrap_or(Path::new("")) });
        let file = folder.map(|f| f.join(FILE_NAME)).filter(|f| f.is_file());
        let Some(file) = file else {
            return Icon::decode(TESSEL_ICON);
        };
        let bytes = std::fs::read(&file).map_err(|e| format!("can't read `{}`: {e}", file.display()))?;
        Icon::decode(&bytes).map_err(|e| format!("can't use `{}` as the app's icon: {e}", file.display()))
    }

    fn decode(bytes: &[u8]) -> Result<Icon, String> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        // 8 bits per channel, and no palettes.
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| format!("it isn't a PNG image ({e})"))?;
        let size = reader.output_buffer_size().ok_or("the image is too large")?;
        let mut data = vec![0; size];
        let info = reader.next_frame(&mut data).map_err(|e| format!("it isn't a PNG image ({e})"))?;
        if info.width != info.height {
            return Err(format!("it must be square, but it's {} × {} pixels", info.width, info.height));
        }
        let data = &data[..info.buffer_size()];
        let rgba = match info.color_type {
            png::ColorType::Rgba => data.to_vec(),
            png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            png::ColorType::Indexed => return Err("it's a kind of PNG image Tessel can't read".into()),
        };
        Ok(Icon { size: info.width, rgba })
    }

    /// A PNG file of this icon at `size` × `size` pixels.
    pub fn png_at(&self, size: u32) -> Vec<u8> {
        self.resized(size).png()
    }

    /// This icon at `size` × `size` pixels: each new pixel is the average of
    /// the area it covers (weighted by alpha, so transparent pixels' colors
    /// don't show at the edges).
    fn resized(&self, size: u32) -> Icon {
        if size == self.size {
            return Icon { size, rgba: self.rgba.clone() };
        }
        let scale = self.size as f64 / size as f64;
        // The source pixels a new pixel covers, and how much of each.
        let spans: Vec<Vec<(usize, f64)>> = (0..size)
            .map(|i| {
                let (lo, hi) = (i as f64 * scale, (i + 1) as f64 * scale);
                let last = ((hi.ceil() as usize).max(lo as usize + 1)).min(self.size as usize);
                (lo as usize..last).map(|s| (s, (hi.min(s as f64 + 1.0) - lo.max(s as f64)).max(0.0))).collect()
            })
            .collect();
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        for rows in &spans {
            for columns in &spans {
                let (mut color, mut alpha, mut area) = ([0.0; 3], 0.0, 0.0);
                for &(y, wy) in rows {
                    for &(x, wx) in columns {
                        let p = &self.rgba[(y * self.size as usize + x) * 4..][..4];
                        let (w, a) = (wx * wy, p[3] as f64);
                        for c in 0..3 {
                            color[c] += p[c] as f64 * a * w;
                        }
                        alpha += a * w;
                        area += w;
                    }
                }
                let channel = |c: f64| if alpha > 0.0 { (c / alpha).round() as u8 } else { 0 };
                rgba.extend([channel(color[0]), channel(color[1]), channel(color[2]), (alpha / area).round() as u8]);
            }
        }
        Icon { size, rgba }
    }

    /// Of `sizes` (smallest first), those this icon has the pixels for.
    fn sizes<'a, T>(&self, sizes: &'a [(u32, T)]) -> impl Iterator<Item = &'a (u32, T)> {
        let largest = self.size.max(sizes[0].0);
        sizes.iter().filter(move |(size, _)| *size <= largest)
    }

    fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.size, self.size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Writing to memory can't fail.
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&self.rgba).unwrap();
        writer.finish().unwrap();
        out
    }

    /// A macOS icon file (`.icns`): PNG images from 32 to 1024 pixels.
    pub fn icns(&self) -> Vec<u8> {
        const SIZES: &[(u32, &[u8; 4])] =
            &[(32, b"ic11"), (64, b"ic12"), (128, b"ic07"), (256, b"ic08"), (512, b"ic09"), (1024, b"ic10")];
        let mut images = Vec::new();
        for (size, kind) in self.sizes(SIZES) {
            let png = self.resized(*size).png();
            images.extend(*kind);
            images.extend((png.len() as u32 + 8).to_be_bytes());
            images.extend(png);
        }
        let mut out = b"icns".to_vec();
        out.extend((images.len() as u32 + 8).to_be_bytes());
        out.extend(images);
        out
    }

    /// A Windows resource file (`.res`) for the linker, with this icon as
    /// the `.exe`'s: icon group 1 (the runtime gives it to the window too),
    /// from 16 to 256 pixels.
    pub fn windows_resources(&self) -> Vec<u8> {
        const ICON: u16 = 3;
        const GROUP_ICON: u16 = 14;
        // Small sizes are bitmaps, which every version of Windows reads; 256 is a PNG.
        const SIZES: &[(u32, bool)] = &[(16, false), (24, false), (32, false), (48, false), (64, false), (256, true)];
        let images: Vec<(u32, Vec<u8>)> = self
            .sizes(SIZES)
            .map(|&(size, png)| {
                let icon = self.resized(size);
                (size, if png { icon.png() } else { icon.bitmap() })
            })
            .collect();

        // The file starts with an empty resource.
        let mut out = Vec::new();
        resource(&mut out, 0, 0, 0, &[]);
        let mut group = Vec::new();
        for n in [0, 1, images.len() as u16] {
            group.extend(n.to_le_bytes());
        }
        for (i, (size, data)) in images.iter().enumerate() {
            let id = i as u16 + 1;
            resource(&mut out, ICON, id, 0x1010, data);
            // 0 stands for 256 pixels.
            group.extend([*size as u8, *size as u8, 0, 0]);
            group.extend(1u16.to_le_bytes()); // color planes
            group.extend(32u16.to_le_bytes()); // bits per pixel
            group.extend((data.len() as u32).to_le_bytes());
            group.extend(id.to_le_bytes());
        }
        resource(&mut out, GROUP_ICON, 1, 0x1030, &group);
        out
    }

    /// The image as Windows icons hold it: a bitmap header, the pixels from
    /// the bottom row up (blue, green, red, alpha), then an unused 1-bit mask.
    fn bitmap(&self) -> Vec<u8> {
        let size = self.size as usize;
        let mask_row = size.div_ceil(32) * 4;
        let mut out = Vec::with_capacity(40 + size * size * 4 + size * mask_row);
        out.extend(40u32.to_le_bytes());
        out.extend(self.size.to_le_bytes());
        out.extend((self.size * 2).to_le_bytes()); // the image and its mask
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend(0u32.to_le_bytes()); // not compressed
        out.extend((self.size * self.size * 4).to_le_bytes());
        out.extend([0; 16]);
        for row in self.rgba.chunks_exact(size * 4).rev() {
            out.extend(row.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]));
        }
        out.resize(out.len() + size * mask_row, 0);
        out
    }
}

/// Appends one resource to a `.res` file: its header (the type and name are
/// numbers), then its data, up to a multiple of four bytes.
/// What a Windows `.exe` tells of itself (its Properties' Details): from
/// the manifest's `[app]`.
pub struct VersionInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub copyright: String,
    pub file_name: String,
}

/// The version resource (`VS_VERSIONINFO`) of an `.exe`, to add to its
/// resource file.
pub fn version_resource(info: &VersionInfo) -> Vec<u8> {
    const VERSION: u16 = 16;
    // "1.2.3" (and a fourth number, if there is one) as four numbers.
    let mut numbers = [0u16; 4];
    for (n, part) in numbers.iter_mut().zip(info.version.split(['.', '-', '+'])) {
        *n = part.trim().parse().unwrap_or(0);
    }
    let ms = (numbers[0] as u32) << 16 | numbers[1] as u32;
    let ls = (numbers[2] as u32) << 16 | numbers[3] as u32;
    let mut fixed = Vec::new();
    for v in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(v.to_le_bytes());
    }
    let description = if info.description.is_empty() { &info.name } else { &info.description };
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", info.author.as_str()),
        ("FileDescription", description.as_str()),
        ("FileVersion", info.version.as_str()),
        ("InternalName", info.name.as_str()),
        ("LegalCopyright", info.copyright.as_str()),
        ("OriginalFilename", info.file_name.as_str()),
        ("ProductName", info.name.as_str()),
        ("ProductVersion", info.version.as_str()),
    ]
    .iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| {
        let value: Vec<u16> = v.encode_utf16().chain([0]).collect();
        version_node(k, &value.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>(), value.len() as u16, true, &[])
    })
    .collect();
    // US English, Unicode.
    let table = version_node("040904B0", &[], 0, true, &strings);
    let string_info = version_node("StringFileInfo", &[], 0, true, &[table]);
    let translation = version_node("Translation", &0x04B0_0409u32.to_le_bytes(), 4, false, &[]);
    let var_info = version_node("VarFileInfo", &[], 0, true, &[translation]);
    let root = version_node("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[string_info, var_info]);
    let mut out = Vec::new();
    resource(&mut out, VERSION, 1, 0x0030, &root);
    out
}

/// One block of a version resource: its length, its value's length, its
/// type (1 for text), its key, its value and its children, each starting
/// at a multiple of 4 bytes.
fn version_node(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend(value_len.to_le_bytes());
    out.extend((text as u16).to_le_bytes());
    for c in key.encode_utf16().chain([0]) {
        out.extend(c.to_le_bytes());
    }
    out.resize(out.len().next_multiple_of(4), 0);
    out.extend(value);
    for child in children {
        out.resize(out.len().next_multiple_of(4), 0);
        out.extend(child);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    out
}

fn resource(out: &mut Vec<u8>, kind: u16, id: u16, memory_flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // the header's size
    for number in [kind, id] {
        out.extend(0xFFFFu16.to_le_bytes());
        out.extend(number.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(memory_flags.to_le_bytes());
    out.extend(0x0409u16.to_le_bytes()); // language: US English, like other tools write
    out.extend([0; 8]); // version, characteristics
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(bytes: &[u8], at: usize, big_endian: bool) -> u32 {
        let b = bytes[at..at + 4].try_into().unwrap();
        if big_endian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
    }

    #[test]
    fn tessel_icon() {
        let icon = Icon::of_program(None, None).unwrap();
        assert_eq!(icon.size, 1024);
        // A corner is transparent; the middle (between the tiles) is white.
        assert_eq!(icon.rgba[3], 0);
        let middle = (512 * 1024 + 512) * 4;
        assert_eq!(icon.rgba[middle..middle + 4], [255, 255, 255, 255]);
    }

    #[test]
    fn resizing_averages_by_alpha() {
        // Opaque red, and three transparent pixels (green, but unseen).
        let icon = Icon { size: 2, rgba: [[255, 0, 0, 255], [0, 255, 0, 0], [0, 255, 0, 0], [0, 255, 0, 0]].concat() };
        assert_eq!(icon.resized(1).rgba, [255, 0, 0, 64]);
        // Larger: each pixel becomes four.
        let large = icon.resized(4);
        assert_eq!(large.rgba[..8], [255, 0, 0, 255, 255, 0, 0, 255]);
        assert_eq!(large.rgba[8..12], [0, 0, 0, 0]);
        // PNG and back.
        assert_eq!(Icon::decode(&large.png()).unwrap().rgba, large.rgba);
    }

    #[test]
    fn icns_lists_every_size() {
        let icns = Icon::of_program(None, None).unwrap().icns();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(u32_at(&icns, 4, true) as usize, icns.len());
        let (mut at, mut kinds) = (8, Vec::new());
        while at < icns.len() {
            kinds.push(String::from_utf8_lossy(&icns[at..at + 4]).into_owned());
            assert_eq!(&icns[at + 8..at + 16], b"\x89PNG\r\n\x1a\n");
            at += u32_at(&icns, at + 4, true) as usize;
        }
        assert_eq!(kinds, ["ic11", "ic12", "ic07", "ic08", "ic09", "ic10"]);
        // A small icon isn't enlarged.
        let small = Icon::of_program(None, None).unwrap().resized(100).icns();
        assert!(small.windows(4).any(|w| w == b"ic12"));
        assert!(!small.windows(4).any(|w| w == b"ic07"));
    }

    #[test]
    fn windows_resources_hold_a_group_of_icons() {
        let res = Icon::of_program(None, None).unwrap().windows_resources();
        // (type, id, data) of each resource.
        let (mut at, mut found) = (0, Vec::new());
        while at < res.len() {
            let (data, header) = (u32_at(&res, at, false) as usize, u32_at(&res, at + 4, false) as usize);
            assert_eq!(header, 32);
            let number = |i: usize| u16::from_le_bytes([res[at + i], res[at + i + 1]]);
            found.push((number(10), number(14), &res[at + header..at + header + data]));
            at += (header + data).next_multiple_of(4);
        }
        assert_eq!(at, res.len());
        let kinds: Vec<(u16, u16)> = found.iter().map(|f| (f.0, f.1)).collect();
        assert_eq!(kinds, [(0, 0), (3, 1), (3, 2), (3, 3), (3, 4), (3, 5), (3, 6), (14, 1)]);
        // 16 pixels: a 40-byte header, the pixels, and a mask of 4 bytes a row.
        assert_eq!(found[1].2.len(), 40 + 16 * 16 * 4 + 16 * 4);
        assert_eq!(&found[6].2[..4], b"\x89PNG");
        // The group: a 6-byte header and 14 bytes for each icon.
        let group = found[7].2;
        assert_eq!(group.len(), 6 + 6 * 14);
        assert_eq!(group[..6], [0, 0, 1, 0, 6, 0]);
        assert_eq!(group[6..8], [16, 16]);
        assert_eq!(u32_at(group, 6 + 8, false) as usize, found[1].2.len());
    }

    #[test]
    fn a_bad_icon_says_why() {
        let dir = std::env::temp_dir().join(format!("tessel-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), "not a picture").unwrap();
        let error = Icon::of_program(Some(&dir), None).err().unwrap();
        assert!(error.contains("icon.png` as the app's icon: it isn't a PNG image"), "{error}");
        // For a program that's one file, the icon is next to it.
        std::fs::write(dir.join(FILE_NAME), Icon { size: 2, rgba: vec![7; 16] }.png()).unwrap();
        assert_eq!(Icon::of_program(Some(&dir.join("app.tsl")), None).unwrap().rgba, vec![7; 16]);

        let mut wide = Vec::new();
        let mut encoder = png::Encoder::new(&mut wide, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.write_header().unwrap().write_image_data(&[0; 6]).unwrap();
        std::fs::write(dir.join(FILE_NAME), wide).unwrap();
        let error = Icon::of_program(Some(&dir), None).err().unwrap();
        assert!(error.ends_with("it must be square, but it's 2 × 1 pixels"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
