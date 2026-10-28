//! Laid-out text, as the rest of the runtime sees it: its size, its lines,
//! and where the caret can stand on each. Text fields, the code editor and
//! Markdown work on this, whatever lays the text out and draws it: the
//! renderer's `TextEngine` (parley for the GPU renderer, Core Text on macOS).
//!
//! Positions in the text are byte offsets. Lines are top to bottom; a line's
//! `stops` are the places the caret can stand on it, left to right, at the
//! start of each cluster (a character, or several drawn as one).

use std::ops::Range;

pub use super::backend::text::TextEngine;
/// Of a laid-out text: its glyphs as they're placed, its outline, the
/// metrics of its font; and the faces of a family.
pub use super::backend::text::{glyphs, metrics, outline, styles};

/// The height of a line of code (its text is 13 points): room to read.
pub const CODE_LINE: f32 = 20.0;

/// A font family: the system's, its monospaced one, or one by name (an
/// index into `FAMILIES`, so that styles stay small and copyable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    #[default]
    System,
    Mono,
    Named(u32),
}

/// What text looks like besides its size and weight: `.font(italic:family:)`,
/// `g.text(italic:family:kerning:tracking:)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Face {
    pub family: Family,
    pub italic: bool,
    /// The font's kerning left out (`kerning: false`).
    pub no_kerning: bool,
    /// Room added between letters, in hundredths of a point.
    pub tracking: i32,
}

impl Face {
    /// The room added between letters, in points.
    pub fn tracking(&self) -> f64 {
        self.tracking as f64 / 100.0
    }
}

/// A glyph of a layout, as it's placed: its origin on its line's baseline,
/// in the layout's coordinates (y down from its top), how far it moves the
/// next one along, and the bytes of the text it shows (none for a glyph
/// that shows part of what another does, or after a ligature's first).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    pub text: std::ops::Range<usize>,
}

/// How a font is laid out at a size, in points: above and below the
/// baseline, the room between lines, and how tall capitals and small
/// letters are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontMetrics {
    pub ascent: f64,
    pub descent: f64,
    pub line_gap: f64,
    pub cap_height: f64,
    pub x_height: f64,
}

/// A face of a font family: its name in the family ("Light", "Bold
/// Italic"), its weight (100 to 900) and whether it's italic.
#[derive(Clone, Debug, PartialEq)]
pub struct FontStyle {
    pub name: String,
    pub weight: f32,
    pub italic: bool,
}

/// Which glyph shows which text: given each glyph's place in the text
/// (`at`, in bytes), each glyph that's first at its place gets the text up
/// to the next place a glyph starts (or `end`); the others, none.
pub fn cluster_ranges(at: &[usize], end: usize) -> Vec<std::ops::Range<usize>> {
    let mut starts: Vec<usize> = at.to_vec();
    starts.sort_unstable();
    starts.dedup();
    let mut seen = std::collections::HashSet::new();
    at.iter()
        .map(|&a| {
            if !seen.insert(a) {
                return a..a;
            }
            let next = starts.iter().copied().find(|&s| s > a).unwrap_or(end).max(a);
            a..next
        })
        .collect()
}

thread_local! {
    /// The families asked for by name, in the order they were first asked for.
    static FAMILIES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Family {
    /// The family a program names: `""` or `"system"` for the system's,
    /// `"monospaced"` for its monospaced one, or any installed family (a
    /// family that isn't installed is drawn in the system's).
    pub fn named(name: &str) -> Family {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("system") {
            return Family::System;
        }
        if name.eq_ignore_ascii_case("monospaced") {
            return Family::Mono;
        }
        FAMILIES.with(|f| {
            let mut f = f.borrow_mut();
            let i = f.iter().position(|n| n == name).unwrap_or_else(|| {
                f.push(name.to_string());
                f.len() - 1
            });
            Family::Named(i as u32)
        })
    }

    /// The name of a family asked for by name.
    pub fn name(&self) -> Option<String> {
        match self {
            Family::Named(i) => FAMILIES.with(|f| f.borrow().get(*i as usize).cloned()),
            _ => None,
        }
    }
}

/// `fontFamilies()`: the families installed, sorted, without the system's
/// hidden ones (whose names start with a dot).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_font_families() -> *mut crate::List {
    let mut names: Vec<String> = super::backend::text::families().into_iter().filter(|n| !n.starts_with('.') && !n.is_empty()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    unsafe { crate::str_list(names.iter().map(String::as_str)) }
}

/// A stretch of text in a style of its own (a `RichTextEditor`'s runs), or
/// the style of all of it (its `range` left aside).
#[derive(Clone, Debug)]
pub struct Styled {
    pub range: Range<usize>,
    pub family: Family,
    pub size: f64,
    pub weight: f32,
    pub italic: bool,
    pub color: super::Rgba,
}

/// A rectangle in a layout's coordinates: from (x0, y0) to (x1, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A line of laid-out text.
#[derive(Clone, Debug)]
pub struct Line {
    /// Where it starts and ends in the text (with its line break, if any).
    pub range: Range<usize>,
    pub top: f64,
    pub bottom: f64,
    /// Where the caret can stand: (offset, x), left to right.
    pub stops: Vec<(usize, f64)>,
    /// Where the line's last character ends.
    pub end_x: f64,
}

pub struct TextLayout {
    pub width: f64,
    /// The width with the spaces at the ends of its lines (`width` leaves
    /// them out, so that text lines up by what shows).
    pub advance: f64,
    pub height: f64,
    pub lines: Vec<Line>,
    /// The length of the text.
    pub len: usize,
    /// What draws it (the renderer's).
    pub shaped: super::backend::text::Shaped,
}

impl TextLayout {
    pub fn width(&self) -> f64 {
        self.width
    }

    /// The width with the spaces at the end of the text, but no wider than
    /// `max_w`: a `Text`'s size keeps them (so `Text("int ")` beside
    /// `Text("total")` shows "int total"). Spaces where a line wraps are
    /// still left out.
    pub fn width_with_end(&self, max_w: Option<f64>) -> f64 {
        let end = self.lines.last().map_or(0.0, |l| l.end_x);
        let w = end.min(max_w.unwrap_or(f64::INFINITY));
        w.max(self.width)
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    /// The line holding `offset` (the next line's start belongs to it).
    pub fn line_of(&self, offset: usize) -> usize {
        match self.lines.iter().rposition(|l| l.range.start <= offset) {
            Some(i) => i,
            None => 0,
        }
    }

    /// Where the caret at `offset` stands on line `line`.
    fn x_on(&self, line: usize, offset: usize) -> f64 {
        let Some(l) = self.lines.get(line) else { return 0.0 };
        if offset >= l.range.end && l.stops.last().is_none_or(|s| s.0 < offset) {
            return l.end_x;
        }
        l.stops.iter().rev().find(|s| s.0 <= offset).or(l.stops.first()).map_or(0.0, |s| s.1)
    }

    /// The caret at `offset`: a thin rectangle the line's height.
    pub fn caret(&self, offset: usize, width: f64) -> Option<Bounds> {
        let line = self.line_of(offset);
        let l = self.lines.get(line)?;
        let x = self.x_on(line, offset);
        (l.bottom > l.top).then_some(Bounds { x0: x, y0: l.top, x1: x + width, y1: l.bottom })
    }

    /// The rectangles that show a selection, one per line it's on.
    pub fn selection(&self, range: Range<usize>) -> Vec<Bounds> {
        if range.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (i, l) in self.lines.iter().enumerate() {
            if l.range.end <= range.start && i + 1 < self.lines.len() || l.range.start >= range.end {
                continue;
            }
            let x0 = self.x_on(i, range.start.max(l.range.start));
            // (A selection that goes on past the line takes in its end.)
            let past = range.end >= l.range.end && i + 1 < self.lines.len();
            let x1 = if past { l.end_x.max(x0 + 4.0) } else { self.x_on(i, range.end) };
            if x1 > x0 {
                out.push(Bounds { x0, y0: l.top, x1, y1: l.bottom });
            }
        }
        out
    }

    /// The offset nearest to (x, y).
    pub fn offset_at(&self, x: f64, y: f64) -> usize {
        let line = self.lines.iter().position(|l| y < l.bottom).unwrap_or(self.lines.len().saturating_sub(1));
        self.offset_on(line, x)
    }

    /// The offset on line `line` nearest to `x`.
    pub fn offset_on(&self, line: usize, x: f64) -> usize {
        let Some(l) = self.lines.get(line) else { return 0 };
        let last = line + 1 == self.lines.len();
        for (i, s) in l.stops.iter().enumerate() {
            let next_x = l.stops.get(i + 1).map_or(l.end_x, |n| n.1);
            if x < (s.1 + next_x) / 2.0 {
                return s.0;
            }
        }
        // Past the end: before the line break, or at the end of the text.
        match l.stops.last() {
            Some(&(i, _)) if !last && l.range.end > i && self.ends_line(line) => i,
            _ if last => self.len,
            _ => l.range.end,
        }
    }

    /// Whether line `line` ends with a line break (rather than wrapping).
    fn ends_line(&self, line: usize) -> bool {
        self.lines.get(line + 1).is_some_and(|next| {
            let l = &self.lines[line];
            l.stops.last().is_some_and(|s| s.0 + 1 == next.range.start && l.range.end == next.range.start)
        })
    }

    /// The caret stop before `offset` (to the left), or `offset` at the start.
    pub fn previous(&self, offset: usize) -> usize {
        self.lines.iter().flat_map(|l| l.stops.iter().map(|s| s.0)).filter(|&s| s < offset).max().unwrap_or(0)
    }

    /// The caret stop after `offset`, or the end of the text.
    pub fn next(&self, offset: usize) -> usize {
        self.lines.iter().flat_map(|l| l.stops.iter().map(|s| s.0)).filter(|&s| s > offset).min().unwrap_or(self.len)
    }

    /// Where the line holding `offset` starts.
    pub fn line_start(&self, offset: usize) -> usize {
        self.lines.get(self.line_of(offset)).map_or(0, |l| l.range.start)
    }

    /// Where the line holding `offset` ends: before its line break.
    pub fn line_end(&self, offset: usize) -> usize {
        let line = self.line_of(offset);
        if line + 1 == self.lines.len() {
            return self.len;
        }
        let l = &self.lines[line];
        if self.ends_line(line) { l.stops.last().map_or(l.range.end, |s| s.0) } else { l.range.end }
    }

    /// The offset on the line above (`up`) or below, at the same x; the
    /// start or end of the text past the first or last line.
    pub fn vertical(&self, offset: usize, up: bool) -> usize {
        let line = self.line_of(offset);
        let x = self.x_on(line, offset);
        match (up, line) {
            (true, 0) => 0,
            (true, _) => self.offset_on(line - 1, x),
            (false, _) if line + 1 >= self.lines.len() => self.len,
            (false, _) => self.offset_on(line + 1, x),
        }
    }
}

/// Whether a character is part of a word, for moving and selecting by word.
fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The start of the word before `offset` (Option+Left).
pub fn word_before(text: &str, offset: usize) -> usize {
    let head = &text[..offset];
    let mut chars = head.char_indices().rev().peekable();
    // Skip what isn't a word, then the word.
    while chars.next_if(|(_, c)| !wordy(*c)).is_some() {}
    let mut start = chars.peek().map_or(0, |(i, _)| *i);
    while let Some((i, _)) = chars.next_if(|(_, c)| wordy(*c)) {
        start = i;
    }
    if chars.peek().is_none() && head.chars().next().is_some_and(wordy) { 0 } else { start }
}

/// The end of the word after `offset` (Option+Right).
pub fn word_after(text: &str, offset: usize) -> usize {
    let tail = &text[offset..];
    let mut chars = tail.char_indices().peekable();
    while chars.next_if(|(_, c)| !wordy(*c)).is_some() {}
    while chars.next_if(|(_, c)| wordy(*c)).is_some() {}
    offset + chars.peek().map_or(tail.len(), |(i, _)| *i)
}

/// The word (or space, or other character) at `offset`, for a double click.
pub fn word_at(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    // The character clicked: the one after the point, or before it at the end.
    let (at, c) = match text[offset..].chars().next() {
        Some(c) if c != '\n' => (offset, c),
        _ => match text[..offset].chars().next_back() {
            Some(c) if c != '\n' => (offset - c.len_utf8(), c),
            _ => return offset..offset,
        },
    };
    let same = |x: char| if wordy(c) { wordy(x) } else if c.is_whitespace() { x.is_whitespace() && x != '\n' } else { x == c };
    let start = text[..at].char_indices().rev().take_while(|(_, x)| same(*x)).last().map_or(at, |(i, _)| i);
    let end = at + text[at..].char_indices().take_while(|(_, x)| same(*x)).last().map_or(0, |(i, x)| i + x.len_utf8());
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        let t = "let total = a_b + 12";
        assert_eq!(word_after(t, 0), 3);
        assert_eq!(word_after(t, 3), 9);
        assert_eq!(word_before(t, 9), 4);
        assert_eq!(word_before(t, 4), 0);
        assert_eq!(word_at(t, 6), 4..9);
        assert_eq!(word_at(t, 13), 12..15);
        assert_eq!(word_at(t, t.len()), 18..20);
    }

    #[test]
    fn lines_and_stops() {
        let mut e = TextEngine::new();
        let t = "ab\ncd";
        let l = e.code_layout(t, 13.0);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.line_of(3), 1);
        assert_eq!(l.line_end(0), 2);
        assert_eq!(l.line_start(4), 3);
        assert_eq!(l.next(2), 3);
        assert_eq!(l.previous(3), 2);
        assert_eq!(l.vertical(1, false), 4);
        assert_eq!(l.vertical(4, true), 1);
        assert_eq!(l.offset_at(1000.0, 1.0), 2);
        assert_eq!(l.offset_at(1000.0, 30.0), 5);
    }

    /// After Enter in a block, the caret stands after the new line's indent,
    /// with the line's break after it or at the end of the text.
    #[test]
    fn caret_after_indent() {
        let mut e = TextEngine::new();
        let space = e.code_layout("abcd", 13.0).caret(4, 1.0).unwrap().x0 / 4.0;
        for t in ["a {\n    \n}", "a {\n    "] {
            let l = e.code_layout(t, 13.0);
            let x = l.caret(8, 1.0).unwrap().x0;
            assert!((x - 4.0 * space).abs() < 0.5, "{t:?}: caret at {x}, a space is {space}");
        }
    }

    /// A space typed at the end of a line moves the caret past it, at the
    /// end of the text and before a line break.
    #[test]
    fn caret_after_trailing_space() {
        let mut e = TextEngine::new();
        let space = e.code_layout("abcd", 13.0).caret(4, 1.0).unwrap().x0 / 4.0;
        for t in ["ab ", "ab \ncd", "ab  "] {
            let n = t.find('\n').unwrap_or(t.len());
            let x = e.code_layout(t, 13.0).caret(n, 1.0).unwrap().x0;
            assert!((x - n as f64 * space).abs() < 0.5, "{t:?}: caret at {x}, a space is {space}");
        }
    }

    /// A text's width keeps the spaces at its end, but not those where a
    /// line wraps.
    #[test]
    fn width_keeps_trailing_spaces() {
        let mut e = TextEngine::new();
        let black = crate::ui::Rgba([0, 0, 0, 255]);
        let mut width = |t: &str| e.layout(t, 13.0, 400.0, black, None).width_with_end(None);
        let (plain, spaced, total, joined) = (width("int"), width("int "), width("total"), width("int total"));
        assert!(spaced > plain + 2.0, "{spaced} vs {plain}");
        assert!((spaced + total - joined).abs() < 1.0, "{spaced} + {total} vs {joined}");
        let wrapped = e.layout("words and words and words ", 13.0, 400.0, black, Some(60.0));
        assert!(wrapped.lines.len() > 1);
        assert!(wrapped.width_with_end(Some(60.0)) <= 60.0);
    }
}
