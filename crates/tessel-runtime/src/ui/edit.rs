//! Text editing for text fields: cursor movement, selection, clipboard.
//!
//! The field's text lives in the program's state; the runtime only keeps the
//! selection (as byte offsets), so it survives rebuilds of the view tree.

use parley::{Affinity, Cursor, Layout, Selection};

use super::render::Paint;

/// One undo step: the text and selection before a change.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub text: String,
    pub anchor: usize,
    pub focus: usize,
}

/// Undo and redo for one text field or code editor.
#[derive(Default, Debug)]
pub struct History {
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    /// The text after the last change we made; if the program changes the
    /// text some other way (e.g. opens another file), the history is dropped.
    pub last_text: String,
    /// Consecutive typing of word characters is undone in one step.
    pub typing: bool,
}

/// The text field being edited and its selection.
#[derive(Clone, Debug, Default)]
pub struct Editing {
    pub key: u64,
    pub anchor: usize,
    pub focus: usize,
    /// Horizontal scroll of the text inside the field, in points.
    pub scroll: f64,
    /// What moving the mouse with its button down selects (set by the click
    /// that began it).
    pub drag: DragBy,
}

/// How a drag extends the selection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DragBy {
    /// After one click: character by character.
    #[default]
    Character,
    /// After a double click: word by word, from the word that was clicked
    /// (its bytes), which stays selected.
    Word(usize, usize),
    /// After a triple click, which selected everything: nothing more.
    Nothing,
}

impl Editing {
    pub fn new(key: u64, text: &str) -> Editing {
        Editing { key, anchor: text.len(), focus: text.len(), scroll: 0.0, drag: DragBy::Character }
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }

    /// Keeps offsets valid after the text changed from outside.
    pub fn clamp(&mut self, text: &str) {
        let fix = |i: usize| {
            let mut i = i.min(text.len());
            while !text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        self.anchor = fix(self.anchor);
        self.focus = fix(self.focus);
    }

    fn to_parley(&self, layout: &Layout<Paint>) -> Selection {
        Selection::new(
            Cursor::from_byte_index(layout, self.anchor, Affinity::Downstream),
            Cursor::from_byte_index(layout, self.focus, Affinity::Downstream),
        )
    }

    fn set(&mut self, s: Selection) {
        self.anchor = s.anchor().index();
        self.focus = s.focus().index();
    }

    fn collapse_to(&mut self, i: usize) {
        self.anchor = i;
        self.focus = i;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Char,
    Word,
    Line,
}

#[derive(Clone, Debug)]
pub enum Edit {
    Insert(String),
    Backspace(Unit),
    Delete(Unit),
    Left { unit: Unit, extend: bool },
    Right { unit: Unit, extend: bool },
    Up { extend: bool },
    Down { extend: bool },
    SelectAll,
    Copy,
    Cut,
    Paste,
    /// A click at a point in the text's coordinates; `clicks` counts double and triple clicks.
    Click { x: f64, y: f64, extend: bool, clicks: u32 },
    Drag { x: f64, y: f64 },
}

pub enum Clipboard {
    System(arboard::Clipboard),
    /// For tests: never touches the user's clipboard.
    Memory(String),
    /// Headless runs: the clipboard `copyToClipboard` and `clipboardText`
    /// use there too, so both kinds of copying and pasting meet.
    Headless,
}

impl Clipboard {
    pub fn system() -> Clipboard {
        match arboard::Clipboard::new() {
            Ok(c) => Clipboard::System(c),
            Err(_) => Clipboard::Memory(String::new()),
        }
    }

    fn get(&mut self) -> String {
        match self {
            Clipboard::System(c) => c.get_text().unwrap_or_default(),
            Clipboard::Memory(s) => s.clone(),
            Clipboard::Headless => crate::stdlib::headless_clipboard().unwrap_or_default(),
        }
    }

    fn set(&mut self, text: &str) {
        match self {
            Clipboard::System(c) => {
                let _ = c.set_text(text.to_string());
            }
            Clipboard::Memory(s) => *s = text.to_string(),
            Clipboard::Headless => crate::stdlib::set_headless_clipboard(text),
        }
    }
}

/// Applies `edit` to `text` (laid out as `layout`). Returns the new text if it
/// changed. `multiline` is true for code editors, false for one-line fields.
pub fn apply(
    text: &str,
    layout: &Layout<Paint>,
    e: &mut Editing,
    edit: Edit,
    clip: &mut Clipboard,
    multiline: bool,
) -> Option<String> {
    e.clamp(text);
    let replace = |e: &mut Editing, with: &str| {
        let r = e.range();
        let mut out = String::with_capacity(text.len() + with.len());
        out.push_str(&text[..r.start]);
        out.push_str(with);
        out.push_str(&text[r.end..]);
        e.collapse_to(r.start + with.len());
        Some(out)
    };
    let sel = e.to_parley(layout);
    match edit {
        Edit::Insert(s) => replace(e, &s),
        Edit::Paste => {
            let s = clip.get().replace("\r\n", "\n");
            // A single-line field: newlines become spaces.
            let s = if multiline { s.replace('\r', "\n") } else { s.replace(['\n', '\r'], " ") };
            replace(e, &s)
        }
        Edit::Copy | Edit::Cut => {
            let r = e.range();
            if r.is_empty() {
                return None;
            }
            clip.set(&text[r]);
            if matches!(edit, Edit::Cut) { replace(e, "") } else { None }
        }
        Edit::Backspace(unit) | Edit::Delete(unit) => {
            if e.range().is_empty() {
                let forward = matches!(edit, Edit::Delete(_));
                let s = match (unit, forward) {
                    (Unit::Char, false) => sel.previous_visual(layout, true),
                    (Unit::Char, true) => sel.next_visual(layout, true),
                    (Unit::Word, false) => sel.previous_visual_word(layout, true),
                    (Unit::Word, true) => sel.next_visual_word(layout, true),
                    (Unit::Line, false) => sel.line_start(layout, true),
                    (Unit::Line, true) => sel.line_end(layout, true),
                };
                e.set(s);
                if e.range().is_empty() {
                    return None;
                }
            }
            replace(e, "")
        }
        Edit::Left { unit, extend } | Edit::Right { unit, extend } => {
            let right = matches!(edit, Edit::Right { .. });
            let r = e.range();
            if !extend && !r.is_empty() && unit == Unit::Char {
                // Arrow keys collapse a selection to its start or end.
                e.collapse_to(if right { r.end } else { r.start });
                return None;
            }
            let s = match (unit, right) {
                (Unit::Char, false) => sel.previous_visual(layout, extend),
                (Unit::Char, true) => sel.next_visual(layout, extend),
                (Unit::Word, false) => sel.previous_visual_word(layout, extend),
                (Unit::Word, true) => sel.next_visual_word(layout, extend),
                (Unit::Line, false) => sel.line_start(layout, extend),
                (Unit::Line, true) => sel.line_end(layout, extend),
            };
            e.set(s);
            None
        }
        Edit::Up { extend } => {
            e.set(sel.previous_line(layout, extend));
            None
        }
        Edit::Down { extend } => {
            e.set(sel.next_line(layout, extend));
            None
        }
        Edit::SelectAll => {
            e.anchor = 0;
            e.focus = text.len();
            None
        }
        Edit::Click { x, y, extend, clicks } => {
            let (x, y) = (x as f32, y as f32);
            e.drag = DragBy::Character;
            let s = match clicks {
                _ if extend => sel.shift_click_extension(layout, x, y),
                1 => Selection::from_point(layout, x, y),
                2 => Selection::word_from_point(layout, x, y),
                _ => {
                    e.anchor = 0;
                    e.focus = text.len();
                    e.drag = DragBy::Nothing;
                    return None;
                }
            };
            e.set(s);
            if clicks == 2 && !extend {
                let word = e.range();
                e.drag = DragBy::Word(word.start, word.end);
            }
            None
        }
        Edit::Drag { x, y } => {
            match e.drag {
                DragBy::Character => e.set(sel.extend_to_point(layout, x as f32, y as f32)),
                // From the clicked word to the word under the mouse: the
                // slightest move after a double click mustn't undo it.
                DragBy::Word(start, end) => {
                    let mut there = e.clone();
                    there.set(Selection::word_from_point(layout, x as f32, y as f32));
                    let there = there.range();
                    if there.start < start {
                        (e.anchor, e.focus) = (end, there.start);
                    } else {
                        (e.anchor, e.focus) = (start, there.end.max(end));
                    }
                }
                DragBy::Nothing => {}
            }
            None
        }
    }
}

/// Caret and selection rectangles, in the text's coordinates.
pub fn geometry(layout: &Layout<Paint>, e: &Editing) -> (Option<parley::BoundingBox>, Vec<parley::BoundingBox>) {
    let sel = e.to_parley(layout);
    let caret = Cursor::from_byte_index(layout, e.focus, Affinity::Downstream).geometry(layout, 1.5);
    let rects = if sel.is_collapsed() { Vec::new() } else { sel.geometry(layout).into_iter().map(|(b, _)| b).collect() };
    let caret = (caret.y1 > caret.y0).then_some(caret);
    (caret, rects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::render::TextEngine;
    use crate::ui::Rgba;

    const COLOR: Rgba = Rgba([0, 0, 0, 255]);

    fn run(text: &str, e: &mut Editing, edits: Vec<Edit>) -> String {
        let mut text = text.to_string();
        let mut engine = TextEngine::new();
        let mut clip = Clipboard::Memory(String::new());
        for edit in edits {
            let layout = engine.layout(&text, 14.0, 400.0, COLOR, None);
            if let Some(t) = apply(&text, &layout, e, edit, &mut clip, false) {
                text = t;
            }
        }
        text
    }

    #[test]
    fn typing_moving_and_deleting() {
        let mut e = Editing::new(1, "hello");
        let t = run("hello", &mut e, vec![
            Edit::Left { unit: Unit::Char, extend: false },
            Edit::Left { unit: Unit::Char, extend: false },
            Edit::Insert("XY".into()),
            Edit::Backspace(Unit::Char),
            Edit::Right { unit: Unit::Line, extend: false },
            Edit::Insert("!".into()),
        ]);
        assert_eq!(t, "helXlo!");
    }

    #[test]
    fn selection_copy_paste() {
        let mut e = Editing::new(1, "one two");
        let t = run("one two", &mut e, vec![
            Edit::Left { unit: Unit::Word, extend: true },
            Edit::Cut,
            Edit::Left { unit: Unit::Line, extend: false },
            Edit::Paste,
            Edit::Insert(" ".into()),
        ]);
        assert_eq!(t, "two one ");
    }

    #[test]
    fn paste_keeps_lines_only_when_multiline() {
        let mut engine = TextEngine::new();
        let layout = engine.layout("", 14.0, 400.0, COLOR, None);
        let mut clip = Clipboard::Memory("a\r\nb\nc".into());
        let one = apply("", &layout, &mut Editing::new(1, ""), Edit::Paste, &mut clip, false);
        let many = apply("", &layout, &mut Editing::new(1, ""), Edit::Paste, &mut clip, true);
        assert_eq!(one.as_deref(), Some("a b c"));
        assert_eq!(many.as_deref(), Some("a\nb\nc"));
    }

    #[test]
    fn dragging_after_a_double_click_keeps_whole_words() {
        let text = "let greeting = remaining + 1";
        let mut engine = TextEngine::new();
        let layout = engine.code_layout(text, 13.0);
        let mut clip = Clipboard::Memory(String::new());
        let mut e = Editing::new(1, text);
        let mut edit = |e: &mut Editing, edit: Edit| {
            apply(text, &layout, e, edit, &mut clip, true);
            text[e.range()].to_string()
        };
        // Where the middle of byte `i` is.
        let x_of = |i: usize| {
            let mut at = Editing::new(1, "");
            (at.anchor, at.focus) = (i, i + 1);
            let r = geometry(&layout, &at).1[0];
            (r.x0 + r.x1) / 2.0
        };
        let (y, word) = (5.0, text.find("greeting").unwrap());
        assert_eq!(edit(&mut e, Edit::Click { x: x_of(word + 3), y, extend: false, clicks: 2 }), "greeting");
        // The mouse moves a little while the button is down: still the word.
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(word + 4), y }), "greeting");
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(word + 1), y }), "greeting");
        // Further: word by word, to the right and to the left.
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(text.find("remaining").unwrap() + 2), y }), "greeting = remaining");
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(1), y }), "let greeting");
        // A single click, and dragging is by characters again.
        assert_eq!(edit(&mut e, Edit::Click { x: x_of(word), y, extend: false, clicks: 1 }), "");
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(word + 3), y }).len(), 3);
        // After a triple click everything is selected, and stays so.
        assert_eq!(edit(&mut e, Edit::Click { x: x_of(word), y, extend: false, clicks: 3 }), text);
        assert_eq!(edit(&mut e, Edit::Drag { x: x_of(2), y }), text);
    }

    #[test]
    fn select_all_replaces_everything() {
        let mut e = Editing::new(1, "draft");
        let t = run("draft", &mut e, vec![Edit::SelectAll, Edit::Insert("new".into())]);
        assert_eq!(t, "new");
        assert_eq!(e.range(), 3..3);
    }
}
