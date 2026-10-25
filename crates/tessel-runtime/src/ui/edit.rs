//! Text editing for text fields: cursor movement, selection, clipboard.
//!
//! The field's text lives in the program's state; the runtime only keeps the
//! selection (as byte offsets), so it survives rebuilds of the view tree.

use super::text::{self, Bounds, TextLayout};

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

    /// Moves the caret to `to`; with `extend`, the selection's other end stays.
    fn go(&mut self, to: usize, extend: bool) {
        if !extend {
            self.anchor = to;
        }
        self.focus = to;
    }

    pub fn collapse_to(&mut self, i: usize) {
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
    #[cfg(not(target_os = "android"))]
    System(arboard::Clipboard),
    /// The phone's (reached through the app's activity).
    #[cfg(target_os = "android")]
    Phone,
    /// For tests: never touches the user's clipboard.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    Memory(String),
    /// Headless runs: the clipboard `copyToClipboard` and `clipboardText`
    /// use there too, so both kinds of copying and pasting meet.
    Headless,
}

impl Clipboard {
    pub fn system() -> Clipboard {
        #[cfg(target_os = "android")]
        return Clipboard::Phone;
        #[cfg(not(target_os = "android"))]
        match arboard::Clipboard::new() {
            Ok(c) => Clipboard::System(c),
            Err(_) => Clipboard::Memory(String::new()),
        }
    }

    fn get(&mut self) -> String {
        match self {
            #[cfg(not(target_os = "android"))]
            Clipboard::System(c) => c.get_text().unwrap_or_default(),
            #[cfg(target_os = "android")]
            Clipboard::Phone => super::android::pasted().unwrap_or_default(),
            Clipboard::Memory(s) => s.clone(),
            Clipboard::Headless => crate::stdlib::headless_clipboard().unwrap_or_default(),
        }
    }

    /// Puts text with styles on the clipboard: as HTML (for apps that take
    /// it) and as plain text (for the rest).
    pub fn set_rich(&mut self, text: &str, html: &str) {
        match self {
            #[cfg(not(target_os = "android"))]
            Clipboard::System(c) => {
                if c.set().html(html, Some(text)).is_err() {
                    let _ = c.set_text(text.to_string());
                }
            }
            _ => {
                self.set(text);
                RICH.with(|r| *r.borrow_mut() = Some((text.to_string(), html.to_string())));
            }
        }
    }

    /// The HTML on the clipboard, if there's some.
    pub fn html(&mut self) -> Option<String> {
        match self {
            #[cfg(not(target_os = "android"))]
            Clipboard::System(c) => c.get().html().ok().filter(|h| !h.trim().is_empty()),
            // (Ours, as long as the text on the clipboard is still what went with it.)
            _ => {
                let now = self.get();
                RICH.with(|r| r.borrow().as_ref().filter(|(t, _)| *t == now).map(|(_, h)| h.clone()))
            }
        }
    }

    fn set(&mut self, text: &str) {
        match self {
            #[cfg(target_os = "android")]
            Clipboard::Phone => {
                super::android::copy(text);
            }
            #[cfg(not(target_os = "android"))]
            Clipboard::System(c) => {
                let _ = c.set_text(text.to_string());
            }
            Clipboard::Memory(s) => *s = text.to_string(),
            Clipboard::Headless => crate::stdlib::set_headless_clipboard(text),
        }
    }
}

thread_local! {
    /// Where the system's clipboard isn't used: the text with styles copied
    /// last, and its HTML.
    static RICH: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// Applies `edit` to `text` (laid out as `layout`). Returns the new text if it
/// changed. `multiline` is true for code editors, false for one-line fields.
pub fn apply(
    text: &str,
    layout: &TextLayout,
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
    let focus = e.focus;
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
                let to = match (unit, forward) {
                    (Unit::Char, false) => layout.previous(focus),
                    (Unit::Char, true) => layout.next(focus),
                    (Unit::Word, false) => text::word_before(text, focus),
                    (Unit::Word, true) => text::word_after(text, focus),
                    (Unit::Line, false) => layout.line_start(focus),
                    (Unit::Line, true) => layout.line_end(focus),
                };
                e.go(to, true);
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
            let to = match (unit, right) {
                (Unit::Char, false) => layout.previous(focus),
                (Unit::Char, true) => layout.next(focus),
                (Unit::Word, false) => text::word_before(text, focus),
                (Unit::Word, true) => text::word_after(text, focus),
                (Unit::Line, false) => layout.line_start(focus),
                (Unit::Line, true) => layout.line_end(focus),
            };
            e.go(to, extend);
            None
        }
        Edit::Up { extend } => {
            e.go(layout.vertical(focus, true), extend);
            None
        }
        Edit::Down { extend } => {
            e.go(layout.vertical(focus, false), extend);
            None
        }
        Edit::SelectAll => {
            e.anchor = 0;
            e.focus = text.len();
            None
        }
        Edit::Click { x, y, extend, clicks } => {
            e.drag = DragBy::Character;
            let at = layout.offset_at(x, y);
            match clicks {
                _ if extend => e.go(at, true),
                1 => e.go(at, false),
                2 => {
                    let word = text::word_at(text, at);
                    (e.anchor, e.focus) = (word.start, word.end);
                }
                _ => {
                    e.anchor = 0;
                    e.focus = text.len();
                    e.drag = DragBy::Nothing;
                    return None;
                }
            }
            if clicks == 2 && !extend {
                let word = e.range();
                e.drag = DragBy::Word(word.start, word.end);
            }
            None
        }
        Edit::Drag { x, y } => {
            match e.drag {
                DragBy::Character => e.go(layout.offset_at(x, y), true),
                // From the clicked word to the word under the mouse: the
                // slightest move after a double click mustn't undo it.
                DragBy::Word(start, end) => {
                    let there = text::word_at(text, layout.offset_at(x, y));
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
pub fn geometry(layout: &TextLayout, e: &Editing) -> (Option<Bounds>, Vec<Bounds>) {
    (layout.caret(e.focus, 1.5), layout.selection(e.range()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::TextEngine;
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
