//! Light and dark appearance: the colors of the `Color` enum, of the
//! built-in controls and of code, in each.
//!
//! An app follows the system's appearance unless it asks for one
//! (`app Name(appearance: .dark)`). Headless runs are light until the script
//! says `appearance dark`.

use std::cell::Cell;

use super::Rgba;
use crate::Header;

pub struct Theme {
    /// The named cases of the `Color` enum, in order.
    pub colors: [Rgba; 18],
    /// Button labels, the text caret, the selected tab, a field being edited.
    pub accent: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub control_pressed: Rgba,
    /// The inside of text fields and code editors.
    pub field: Rgba,
    pub field_border: Rgba,
    pub placeholder: Rgba,
    pub switch_on: Rgba,
    /// The selected option of a `Picker`.
    pub segment: Rgba,
    pub selection: Rgba,
    /// A code editor's line numbers and the strip behind them.
    pub gutter: Rgba,
    pub line_number: Rgba,
    pub error_line: Rgba,
    pub error_mark: Rgba,
    /// The completion list.
    pub popup: Rgba,
    pub code: Code,
}

/// Syntax colors.
pub struct Code {
    pub keyword: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub comment: Rgba,
    pub type_name: Rgba,
    pub call: Rgba,
    pub case: Rgba,
}

// Indexes into `Theme::colors` the runtime itself uses.
pub const PRIMARY: usize = 0;
pub const SECONDARY: usize = 1;
pub const GRAY: usize = 2;
pub const LIGHT_GRAY: usize = 13;
pub const SIDEBAR: usize = 14;
pub const SEPARATOR: usize = 16;
pub const BACKGROUND: usize = 17;
/// `Color.rgb(r, g, b)` and `Color.rgba(r, g, b, opacity)`, after the named cases.
const RGB: i32 = 18;
const RGBA: i32 = 19;

const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba([r, g, b, 255])
}

pub static LIGHT: Theme = Theme {
    colors: [
        rgb(29, 29, 31),     // primary
        rgb(110, 110, 115),  // secondary
        rgb(142, 142, 147),  // gray
        rgb(255, 59, 48),    // red
        rgb(255, 149, 0),    // orange
        rgb(255, 204, 0),    // yellow
        rgb(52, 199, 89),    // green
        rgb(0, 122, 255),    // blue
        rgb(175, 82, 222),   // purple
        rgb(255, 45, 85),    // pink
        rgb(255, 255, 255),  // white
        rgb(0, 0, 0),        // black
        Rgba([0, 0, 0, 0]),  // clear
        rgb(229, 229, 234),  // lightGray
        rgb(243, 243, 246),  // sidebar
        rgb(214, 228, 252),  // highlight
        rgb(216, 216, 222),  // separator
        rgb(255, 255, 255),  // background
    ],
    accent: rgb(0, 122, 255),
    control: rgb(232, 232, 237),
    control_hover: rgb(221, 221, 227),
    control_pressed: rgb(205, 205, 212),
    field: rgb(255, 255, 255),
    field_border: rgb(199, 199, 204),
    placeholder: rgb(160, 160, 166),
    switch_on: rgb(52, 199, 89),
    segment: rgb(255, 255, 255),
    selection: rgb(179, 215, 255),
    gutter: rgb(245, 245, 247),
    line_number: rgb(160, 160, 166),
    error_line: rgb(255, 235, 235),
    error_mark: rgb(255, 59, 48),
    popup: rgb(255, 255, 255),
    code: Code {
        keyword: rgb(155, 35, 147),
        string: rgb(196, 26, 22),
        number: rgb(28, 0, 207),
        comment: rgb(93, 108, 121),
        type_name: rgb(11, 79, 121),
        call: rgb(50, 109, 116),
        case: rgb(57, 0, 160),
    },
};

pub static DARK: Theme = Theme {
    colors: [
        rgb(245, 245, 247),  // primary
        rgb(152, 152, 157),  // secondary
        rgb(142, 142, 147),  // gray
        rgb(255, 69, 58),    // red
        rgb(255, 159, 10),   // orange
        rgb(255, 214, 10),   // yellow
        rgb(48, 209, 88),    // green
        rgb(10, 132, 255),   // blue
        rgb(191, 90, 242),   // purple
        rgb(255, 55, 95),    // pink
        rgb(255, 255, 255),  // white
        rgb(0, 0, 0),        // black
        Rgba([0, 0, 0, 0]),  // clear
        rgb(58, 58, 60),     // lightGray
        rgb(37, 37, 39),     // sidebar
        rgb(34, 62, 104),    // highlight
        rgb(62, 62, 66),     // separator
        rgb(28, 28, 30),     // background
    ],
    accent: rgb(10, 132, 255),
    control: rgb(58, 58, 60),
    control_hover: rgb(72, 72, 74),
    control_pressed: rgb(92, 92, 96),
    field: rgb(40, 40, 42),
    field_border: rgb(76, 76, 80),
    placeholder: rgb(110, 110, 115),
    switch_on: rgb(48, 209, 88),
    segment: rgb(99, 99, 102),
    selection: rgb(51, 92, 143),
    gutter: rgb(34, 34, 36),
    line_number: rgb(110, 110, 115),
    error_line: rgb(74, 34, 34),
    error_mark: rgb(255, 69, 58),
    popup: rgb(44, 44, 46),
    code: Code {
        keyword: rgb(252, 95, 163),
        string: rgb(252, 106, 93),
        number: rgb(208, 191, 105),
        comment: rgb(127, 140, 152),
        type_name: rgb(93, 216, 255),
        call: rgb(103, 183, 164),
        case: rgb(178, 129, 235),
    },
};

thread_local! {
    static IS_DARK: Cell<bool> = const { Cell::new(false) };
}

/// The colors in use.
pub fn theme() -> &'static Theme {
    if is_dark() { &DARK } else { &LIGHT }
}

pub fn is_dark() -> bool {
    IS_DARK.with(Cell::get)
}

/// Switches appearance. Returns whether it changed (the UI must be rebuilt:
/// views hold the colors they were built with).
pub fn set_dark(dark: bool) -> bool {
    IS_DARK.with(|d| d.replace(dark)) != dark
}

/// A `Color` value: its case's tag, and for `.rgb` and `.rgba` the box
/// holding their values (three Ints, then a Float for the opacity).
pub unsafe fn color(tag: i32, payload: *const Header) -> Rgba {
    let channel = |i: usize| unsafe { *(payload as *const u8).add(crate::BOX_DATA).cast::<i64>().add(i) }.clamp(0, 255) as u8;
    match tag {
        RGB | RGBA if payload.is_null() => Rgba([0, 0, 0, 0]),
        RGB => rgb(channel(0), channel(1), channel(2)),
        RGBA => {
            let opacity = unsafe { *(payload as *const u8).add(crate::BOX_DATA).cast::<f64>().add(3) };
            Rgba([channel(0), channel(1), channel(2), (opacity.clamp(0.0, 1.0) * 255.0).round() as u8])
        }
        _ => theme().colors[(tag.max(0) as usize).min(BACKGROUND)],
    }
}

/// `isDarkMode()`: whether the app is showing in its dark appearance.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_is_dark_mode() -> u8 {
    is_dark() as u8
}
