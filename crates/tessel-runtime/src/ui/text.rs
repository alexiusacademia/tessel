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

/// The height of a line of code (its text is 13 points): room to read.
pub const CODE_LINE: f32 = 20.0;

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
}
