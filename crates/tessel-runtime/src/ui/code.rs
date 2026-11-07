//! What only a `CodeEditor` does with its text: several cursors, the
//! commands on whole lines (comments, moving, copying, deleting, indenting),
//! typing that closes brackets and quotes, matching brackets, and folding.
//!
//! Author: Alexius Academia
//! Date: 2026-11-07
//!
//! Selections are (anchor, focus) byte offsets. An editor has a main one
//! (`Editing::anchor`/`focus`) and maybe others (`Editing::others`); here
//! they're handled together as a list sorted by position, with the index of
//! the main one. Each command works out what to replace at every selection
//! (a `Change`), and all of it is applied at once: one edit, one undo step.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use super::edit::{self, Clipboard, Edit, Editing, Unit};
use super::highlight::{tokens, Token};
use super::text::TextLayout;

/// A selection: (anchor, focus).
pub type Sel = (usize, usize);

/// What only a code editor does (the keys are in `App::code_key`).
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    /// Typing: one character (or Enter's `"\n"`) closes brackets and
    /// quotes, skips over closing ones, keeps the indentation; more text
    /// goes in as it is.
    Type(String),
    /// Tab: 4 spaces, or with a selection over several lines, indents them.
    /// With Shift: outdents.
    Tab { shift: bool },
    Indent,
    Outdent,
    ToggleComment,
    MoveLines { up: bool },
    CopyLines { up: bool },
    DeleteLines,
    InsertLine { above: bool },
    SelectLine,
    /// Selects the word at the cursor, or the next place the selection's
    /// text is, as another selection.
    SelectNext,
    /// Another cursor on the line above or below.
    AddCursor { up: bool },
    /// Alt+click: another cursor there (or one less, on a cursor).
    AltClick { x: f64, y: f64 },
    /// Back to the main cursor only.
    SingleCursor,
    /// Home: to the line's first character that isn't a space, or (from
    /// there) its very start.
    Home { extend: bool },
}

/// What to put in place of `range`, and the selections after (relative to
/// where `range` starts, in the new text).
#[derive(Clone, Debug)]
struct Change {
    range: Range<usize>,
    text: String,
    sels: Vec<Sel>,
    main: bool,
}

fn lo(s: Sel) -> usize {
    s.0.min(s.1)
}

fn hi(s: Sel) -> usize {
    s.0.max(s.1)
}

/// All the selections, sorted, with the index of the main one.
pub fn selections(e: &Editing) -> (Vec<Sel>, usize) {
    let mut all: Vec<(Sel, bool)> = vec![((e.anchor, e.focus), true)];
    all.extend(e.others.iter().map(|s| (*s, false)));
    normalize(all)
}

/// Sorts selections and merges those that overlap (or carets that touch a
/// selection or each other). A merged one is the main one if any part was.
pub fn normalize(mut all: Vec<(Sel, bool)>) -> (Vec<Sel>, usize) {
    all.sort_by_key(|(s, _)| (lo(*s), hi(*s)));
    let mut out: Vec<(Sel, bool)> = Vec::new();
    for (s, main) in all {
        if let Some((last, last_main)) = out.last_mut() {
            let (a, b) = (lo(*last), hi(*last));
            let (c, d) = (lo(s), hi(s));
            let touch = c < b || (c == b && (a == b || c == d));
            if touch {
                let end = b.max(d);
                // Backwards if either was.
                *last = if last.1 < last.0 || s.1 < s.0 { (end, a) } else { (a, end) };
                *last_main |= main;
                continue;
            }
        }
        out.push((s, main));
    }
    let main = out.iter().position(|(_, m)| *m).unwrap_or(0);
    (out.into_iter().map(|(s, _)| s).collect(), main)
}

/// Puts the selections back into `e`: `main` as its main one.
pub fn set_selections(e: &mut Editing, sels: Vec<Sel>, main: usize) {
    let tagged = sels.into_iter().enumerate().map(|(i, s)| (s, i == main)).collect();
    let (sels, main) = normalize(tagged);
    let Some(&(a, f)) = sels.get(main) else { return };
    (e.anchor, e.focus) = (a, f);
    e.others = sels.into_iter().enumerate().filter(|(i, _)| *i != main).map(|(_, s)| s).collect();
}

/// Applies the changes (sorted by `range`; overlapping ones are joined) to
/// `text`. Returns the new text and the selections, with the main one's index.
fn apply_changes(text: &str, mut changes: Vec<Change>) -> (String, Vec<Sel>, usize) {
    changes.sort_by_key(|c| (c.range.start, c.range.end));
    let mut joined: Vec<Change> = Vec::new();
    for c in changes {
        if let Some(last) = joined.last_mut() {
            if c.range.start < last.range.end || (c.range.start == last.range.end && c.range.start == c.range.end && last.range.is_empty()) {
                last.range.end = last.range.end.max(c.range.end);
                last.text.push_str(&c.text);
                last.main |= c.main;
                continue;
            }
        }
        joined.push(c);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut sels = Vec::new();
    let mut main = 0;
    for c in &joined {
        out.push_str(&text[at..c.range.start]);
        let start = out.len();
        out.push_str(&c.text);
        at = c.range.end;
        if c.main {
            main = sels.len();
        }
        sels.extend(c.sels.iter().map(|(a, f)| (start + a, start + f)));
    }
    out.push_str(&text[at..]);
    (out, sels, main)
}

/// Replaces each selection with what `f` gives for it, and commits.
fn each(text: &str, e: &mut Editing, mut f: impl FnMut(Sel) -> Change) -> Option<String> {
    let (sels, main) = selections(e);
    let changes = sels.iter().enumerate().map(|(i, s)| Change { main: i == main, ..f(*s) }).collect();
    commit(text, e, changes)
}

fn commit(text: &str, e: &mut Editing, changes: Vec<Change>) -> Option<String> {
    let (new, sels, main) = apply_changes(text, changes);
    set_selections(e, sels, main);
    (new != text).then_some(new)
}

/// Inserting `with` in place of selection `s`, the caret after it.
fn replace(s: Sel, with: &str) -> Change {
    Change { range: lo(s)..hi(s), text: with.to_string(), sels: vec![(with.len(), with.len())], main: false }
}

// ===== the commands =====

/// Applies an edit to a code editor's `text` (laid out as `layout`).
/// Returns the new text if it changed. With one cursor, edits that aren't
/// the editor's own do what they do in any text field (`edit::apply`).
pub fn apply(text: &str, layout: &TextLayout, e: &mut Editing, change: Edit, clip: &mut Clipboard) -> Option<String> {
    e.clamp(text);
    let single = e.others.is_empty();
    match change {
        Edit::Code(cmd) => command(text, layout, e, cmd, clip),
        Edit::Click { .. } | Edit::SelectAll => {
            e.others.clear();
            edit::apply(text, layout, e, change, clip, true)
        }
        Edit::Backspace(Unit::Char) if pair_around(text, e).is_some() => {
            each(text, e, |s| match pair_at(text, s) {
                Some(r) => Change { range: r, text: String::new(), sels: vec![(0, 0)], main: false },
                None => delete(text, layout, s, Unit::Char, false),
            })
        }
        _ if single => edit::apply(text, layout, e, change, clip, true),
        Edit::Insert(s) => each(text, e, |sel| replace(sel, &s)),
        Edit::Backspace(unit) => each(text, e, |s| delete(text, layout, s, unit, false)),
        Edit::Delete(unit) => each(text, e, |s| delete(text, layout, s, unit, true)),
        Edit::Paste => {
            let pasted = clip.get().replace("\r\n", "\n").replace('\r', "\n");
            let (sels, _) = selections(e);
            let body = pasted.strip_suffix('\n').unwrap_or(&pasted);
            let lines: Vec<&str> = body.split('\n').collect();
            // One line for each cursor, when there are as many.
            let spread = lines.len() == sels.len() && sels.len() > 1;
            let mut i = 0;
            each(text, e, |s| {
                let with = if spread { lines[i] } else { &pasted };
                i += 1;
                replace(s, with)
            })
        }
        Edit::Copy | Edit::Cut => {
            let (sels, _) = selections(e);
            let parts: Vec<&str> = sels.iter().filter(|s| s.0 != s.1).map(|s| &text[lo(*s)..hi(*s)]).collect();
            if parts.is_empty() {
                return None;
            }
            clip.set(&parts.join("\n"));
            if matches!(change, Edit::Cut) { each(text, e, |s| replace(s, "")) } else { None }
        }
        // Dragging (after an Alt+click, say) moves the main cursor only.
        Edit::Drag { .. } => {
            let others = std::mem::take(&mut e.others);
            edit::apply(text, layout, e, change, clip, true);
            let (sels, main) = selections(&Editing { others, ..e.clone() });
            set_selections(e, sels, main);
            None
        }
        Edit::Left { .. } | Edit::Right { .. } | Edit::Up { .. } | Edit::Down { .. } => {
            let (sels, main) = selections(e);
            let moved = sels
                .iter()
                .map(|&(anchor, focus)| {
                    let mut one = Editing { key: e.key, anchor, focus, ..Default::default() };
                    edit::apply(text, layout, &mut one, change.clone(), clip, true);
                    (one.anchor, one.focus)
                })
                .collect();
            set_selections(e, moved, main);
            None
        }
    }
}

/// Deleting from selection `s` (or by `unit` from its caret).
fn delete(text: &str, layout: &TextLayout, s: Sel, unit: Unit, forward: bool) -> Change {
    let r = if s.0 != s.1 {
        lo(s)..hi(s)
    } else {
        let to = edit::target(text, layout, s.1, unit, forward);
        to.min(s.1)..to.max(s.1)
    };
    Change { range: r, text: String::new(), sels: vec![(0, 0)], main: false }
}

/// Opening characters and what closes them.
const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];

/// The empty pair a caret is between (`(|)`), to delete both with Backspace.
fn pair_at(text: &str, s: Sel) -> Option<Range<usize>> {
    if s.0 != s.1 || s.1 == 0 {
        return None;
    }
    let before = text[..s.1].chars().next_back()?;
    let after = text[s.1..].chars().next()?;
    PAIRS.iter().any(|&(o, c)| o == before && c == after).then(|| s.1 - 1..s.1 + 1)
}

fn pair_around(text: &str, e: &Editing) -> Option<()> {
    let (sels, _) = selections(e);
    sels.iter().any(|s| pair_at(text, *s).is_some()).then_some(())
}

fn command(text: &str, layout: &TextLayout, e: &mut Editing, cmd: Cmd, clip: &mut Clipboard) -> Option<String> {
    let _ = clip;
    match cmd {
        Cmd::Type(s) => type_text(text, e, &s),
        Cmd::Tab { shift: true } | Cmd::Outdent => lines_command(text, e, LineCmd::Outdent),
        Cmd::Tab { shift: false } => {
            let (sels, _) = selections(e);
            let lines = Lines::new(text);
            if sels.iter().any(|s| lines.line_of(lo(*s)) != lines.line_of(hi(*s))) {
                lines_command(text, e, LineCmd::Indent)
            } else {
                each(text, e, |s| replace(s, "    "))
            }
        }
        Cmd::Indent => lines_command(text, e, LineCmd::Indent),
        Cmd::ToggleComment => lines_command(text, e, LineCmd::Comment),
        Cmd::MoveLines { up } => lines_command(text, e, LineCmd::Move(up)),
        Cmd::CopyLines { up } => lines_command(text, e, LineCmd::Copy(up)),
        Cmd::DeleteLines => lines_command(text, e, LineCmd::Delete),
        Cmd::InsertLine { above } => lines_command(text, e, LineCmd::Insert(above)),
        Cmd::SelectLine => {
            let lines = Lines::new(text);
            let (sels, main) = selections(e);
            let picked = sels
                .iter()
                .map(|&s| {
                    let (a, b) = (lo(s), hi(s));
                    let whole = a != b && lines.start(lines.line_of(a)) == a && (b == text.len() || lines.start(lines.line_of(b)) == b);
                    let start = lines.start(lines.line_of(a));
                    // (Already whole lines: one more, as `b` starts the next.)
                    let end = if whole && b == text.len() { b } else { lines.next_start(lines.line_of(b)) };
                    (start, end)
                })
                .collect();
            set_selections(e, picked, main);
            None
        }
        Cmd::SelectNext => {
            select_next(text, e);
            None
        }
        Cmd::AddCursor { up } => {
            let (sels, main) = selections(e);
            let from = if up { sels.first() } else { sels.last() };
            let Some(&(_, focus)) = from else { return None };
            let to = layout.vertical(focus, up);
            if layout.line_of(to) == layout.line_of(focus) {
                return None;
            }
            let _ = main;
            add(e, &sels, (to, to));
            None
        }
        Cmd::AltClick { x, y } => {
            let at = layout.offset_at(x, y);
            let (sels, main) = selections(e);
            // On a caret (with others): that one goes.
            if let Some(k) = sels.iter().position(|s| *s == (at, at)).filter(|_| sels.len() > 1) {
                let mut rest = sels.clone();
                rest.remove(k);
                // (The main one gone, the last one is.)
                let main = if k == main { rest.len() - 1 } else if k < main { main - 1 } else { main };
                set_selections(e, rest, main);
                return None;
            }
            add(e, &sels, (at, at));
            None
        }
        Cmd::SingleCursor => {
            e.others.clear();
            None
        }
        Cmd::Home { extend } => {
            let lines = Lines::new(text);
            let (sels, main) = selections(e);
            let moved = sels
                .iter()
                .map(|&(anchor, focus)| {
                    let line = lines.line_of(focus);
                    let start = lines.start(line);
                    let first = start + indent_of(lines.get(line)).len();
                    let to = if focus == first { start } else { first };
                    (if extend { anchor } else { to }, to)
                })
                .collect();
            set_selections(e, moved, main);
            None
        }
    }
}

/// Adds selection `new` to `sels`, as the main one.
fn add(e: &mut Editing, sels: &[Sel], new: Sel) {
    let mut tagged: Vec<(Sel, bool)> = sels.iter().map(|s| (*s, false)).collect();
    tagged.push((new, true));
    let (sels, main) = normalize(tagged);
    set_selections(e, sels, main);
}

/// The spaces (and tabs) a line starts with.
fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Whether `c` is part of a word (as for moving by words).
fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Characters before which a bracket or quote typed gets its closing one.
fn closes_before(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || ";:.,=}])>".contains(c))
}

/// Typing `s` at every selection.
fn type_text(text: &str, e: &mut Editing, s: &str) -> Option<String> {
    let mut chars = s.chars();
    let one = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    let Some(c) = one else {
        return each(text, e, |sel| replace(sel, s));
    };
    // (Only worked out for a quote: whether the caret's in a string or comment.)
    let quoted = (c == '"').then(|| tokens(text));
    let lines = Lines::new(text);
    each(text, e, |sel| {
        let (a, b) = (lo(sel), hi(sel));
        let line_start = lines.start(lines.line_of(a));
        let before = &text[line_start..a];
        let next = text[b..].chars().next();
        if c == '\n' {
            // Enter: the same indentation (one more after `{`), without the
            // spaces before the caret; between `{` and `}`, the `}` goes on
            // a line of its own.
            let indent = indent_of(before);
            let kept = before.trim_end_matches([' ', '\t']);
            let opens = kept.ends_with('{');
            let cut = line_start + kept.len();
            let rest = &text[b..];
            let after_spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            let line_rest = &rest[after_spaces..];
            if opens && line_rest.starts_with('}') {
                let inner = format!("\n{indent}    ");
                let with = format!("{inner}\n{indent}");
                let end = b + after_spaces;
                return Change { range: cut..end, text: with, sels: vec![(inner.len(), inner.len())], main: false };
            }
            let with = format!("\n{indent}{}", if opens { "    " } else { "" });
            return Change { range: cut..b, sels: vec![(with.len(), with.len())], text: with, main: false };
        }
        // A closing character just before the caret: step over it.
        let closing = matches!(c, ')' | ']' | '}' | '"');
        if a == b && closing && next == Some(c) {
            return Change { range: a..a, text: String::new(), sels: vec![(1, 1)], main: false };
        }
        // `}` on a line of only spaces: one level less.
        if c == '}' && before.len() >= 4 && before.chars().all(|c| c == ' ') {
            return Change { range: a - 4..b, text: "}".into(), sels: vec![(1, 1)], main: false };
        }
        if let Some(&(open, close)) = PAIRS.iter().find(|(o, _)| *o == c) {
            // With a selection: around it.
            if a != b {
                let inner = &text[a..b];
                let with = format!("{open}{inner}{close}");
                let (sa, sf) = if sel.1 < sel.0 { (1 + inner.len(), 1) } else { (1, 1 + inner.len()) };
                return Change { range: a..b, text: with, sels: vec![(sa, sf)], main: false };
            }
            let mut close_it = closes_before(next);
            if c == '"' {
                let prev = text[..a].chars().next_back();
                // In a comment, or a string (one that isn't closed runs to the end of its line).
                let in_token = quoted.as_ref().is_some_and(|t| {
                    t.iter().any(|(r, k)| match k {
                        Token::Comment => r.start < a && a <= r.end,
                        Token::String => r.start < a && (a < r.end || (a == r.end && !text[r.clone()].ends_with(['"', '{']))),
                        _ => false,
                    })
                });
                close_it &= !prev.is_some_and(|p| wordy(p) || p == '"') && !in_token;
            }
            if close_it {
                return Change { range: a..b, text: format!("{open}{close}"), sels: vec![(1, 1)], main: false };
            }
        }
        replace(sel, s)
    })
}

/// Cmd+D: with carets only, the words at them get selected; with a
/// selection, the next place its text is (after the main one, round from
/// the start) is selected too, and becomes the main one.
fn select_next(text: &str, e: &mut Editing) {
    let (sels, main) = selections(e);
    let m = sels[main];
    if m.0 == m.1 {
        let words = sels
            .iter()
            .map(|s| if s.0 == s.1 { word_at(text, s.1).unwrap_or(*s) } else { *s })
            .collect();
        set_selections(e, words, main);
        return;
    }
    let needle = &text[lo(m)..hi(m)];
    let whole_word = needle.chars().all(wordy);
    let is_word_at = |at: usize| {
        let before = text[..at].chars().next_back();
        let after = text[at + needle.len()..].chars().next();
        !before.is_some_and(wordy) && !after.is_some_and(wordy)
    };
    let taken = |at: usize| sels.iter().any(|s| lo(*s) < at + needle.len() && at < hi(*s).max(lo(*s) + 1));
    let found = text[hi(m)..]
        .match_indices(needle)
        .map(|(i, _)| i + hi(m))
        .chain(text[..hi(m)].match_indices(needle).map(|(i, _)| i))
        .find(|&at| (!whole_word || is_word_at(at)) && !taken(at));
    if let Some(at) = found {
        let mut tagged: Vec<(Sel, bool)> = sels.iter().map(|s| (*s, false)).collect();
        tagged.push(((at, at + needle.len()), true));
        let (sels, main) = normalize(tagged);
        set_selections(e, sels, main);
    }
}

/// The word a caret at `at` is in or touches (the one after it first).
fn word_at(text: &str, at: usize) -> Option<Sel> {
    let after = text[at..].chars().next().is_some_and(wordy);
    let before = text[..at].chars().next_back().filter(|c| wordy(*c));
    let from = if after { at } else { at - before?.len_utf8() };
    let w = super::text::word_at(text, from);
    Some((w.start, w.end))
}

// ===== commands on whole lines =====

/// A text's lines: where each starts.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Lines<'a> {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Lines { text, starts }
    }

    fn count(&self) -> usize {
        self.starts.len()
    }

    fn line_of(&self, at: usize) -> usize {
        self.starts.partition_point(|&s| s <= at) - 1
    }

    fn start(&self, line: usize) -> usize {
        self.starts[line]
    }

    /// Where the line after `line` starts (the end of the text after the last).
    fn next_start(&self, line: usize) -> usize {
        self.starts.get(line + 1).copied().unwrap_or(self.text.len())
    }

    /// The line's text, without its line break.
    fn get(&self, line: usize) -> &'a str {
        let end = self.starts.get(line + 1).map_or(self.text.len(), |s| s - 1);
        &self.text[self.starts[line]..end]
    }

    /// (line, column in bytes) of an offset.
    fn at(&self, offset: usize) -> (usize, usize) {
        let l = self.line_of(offset);
        (l, offset - self.starts[l])
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LineCmd {
    Comment,
    Indent,
    Outdent,
    Move(bool),
    Copy(bool),
    Delete,
    Insert(bool),
}

/// The blocks of lines the selections are on (first, last), joined when
/// they overlap or touch. A selection that ends at the start of a line
/// (and began on an earlier one) leaves that line out.
fn blocks(lines: &Lines, sels: &[Sel]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &s in sels {
        let (first, _) = lines.at(lo(s));
        let (mut last, col) = lines.at(hi(s));
        if last > first && col == 0 {
            last -= 1;
        }
        match out.last_mut() {
            Some(b) if first <= b.1 + 1 => b.1 = b.1.max(last),
            _ => out.push((first, last)),
        }
    }
    out
}

/// A selection as (line, column) at both ends.
type LineSel = ((usize, usize), (usize, usize));

fn lines_command(text: &str, e: &mut Editing, cmd: LineCmd) -> Option<String> {
    let lines = Lines::new(text);
    let (sels, main) = selections(e);
    let blocks = blocks(&lines, &sels);
    let mut out: Vec<String> = (0..lines.count()).map(|l| lines.get(l).to_string()).collect();
    let mut at: Vec<LineSel> = sels.iter().map(|s| (lines.at(s.0), lines.at(s.1))).collect();
    // Which ends start a selection (that has text), for columns that move
    // when text goes in before them.
    let starts: Vec<(bool, bool)> = sels.iter().map(|s| (s.0 < s.1, s.1 < s.0)).collect();
    // Text put in (`n` > 0) or taken out at a column of a line.
    let mut shifts: Vec<(usize, usize, isize)> = Vec::new();
    let in_blocks = |l: usize| blocks.iter().any(|b| b.0 <= l && l <= b.1);
    match cmd {
        LineCmd::Comment => {
            let chosen: Vec<usize> = (0..out.len()).filter(|l| in_blocks(*l)).collect();
            let filled: Vec<usize> = chosen.iter().copied().filter(|l| !out[*l].trim().is_empty()).collect();
            let commented = !filled.is_empty() && filled.iter().all(|l| out[*l].trim_start().starts_with("//"));
            if commented {
                for &l in &filled {
                    let col = indent_of(&out[l]).len();
                    let n = if out[l][col..].starts_with("// ") { 3 } else { 2 };
                    out[l].replace_range(col..col + n, "");
                    shifts.push((l, col, -(n as isize)));
                }
            } else {
                let targets = if filled.is_empty() { chosen.clone() } else { filled };
                let col = targets.iter().map(|l| indent_of(&out[*l]).len()).min().unwrap_or(0);
                for &l in &targets {
                    out[l].insert_str(col, "// ");
                    shifts.push((l, col, 3));
                }
            }
        }
        LineCmd::Indent => {
            let chosen: Vec<usize> = (0..out.len()).filter(|l| in_blocks(*l)).collect();
            let many = chosen.len() > 1;
            for l in chosen {
                if many && out[l].is_empty() {
                    continue;
                }
                out[l].insert_str(0, "    ");
                shifts.push((l, 0, 4));
            }
        }
        LineCmd::Outdent => {
            for l in (0..out.len()).filter(|l| in_blocks(*l)) {
                let n = if out[l].starts_with('\t') { 1 } else { out[l].len() - out[l].trim_start_matches(' ').len() }.min(4);
                if n > 0 {
                    out[l].replace_range(0..n, "");
                    shifts.push((l, 0, -(n as isize)));
                }
            }
            if shifts.is_empty() {
                return None;
            }
        }
        LineCmd::Move(up) => {
            if up && blocks.first().is_some_and(|b| b.0 == 0) || !up && blocks.last().is_some_and(|b| b.1 + 1 >= out.len()) {
                return None;
            }
            for b in &blocks {
                if up {
                    out[b.0 - 1..=b.1].rotate_left(1);
                } else {
                    out[b.0..=b.1 + 1].rotate_right(1);
                }
            }
            for s in &mut at {
                s.0 .0 = if up { s.0 .0 - 1 } else { s.0 .0 + 1 };
                s.1 .0 = if up { s.1 .0 - 1 } else { s.1 .0 + 1 };
            }
        }
        LineCmd::Copy(up) => {
            // From the bottom, so the blocks above stay where they are.
            for b in blocks.iter().rev() {
                let copy: Vec<String> = out[b.0..=b.1].to_vec();
                out.splice(b.1 + 1..b.1 + 1, copy);
            }
            for (i, s) in at.iter_mut().enumerate() {
                let line = lo(sels[i]);
                let l = lines.line_of(line);
                let above: usize = blocks.iter().filter(|b| b.1 < l).map(|b| b.1 - b.0 + 1).sum();
                let own = blocks.iter().find(|b| b.0 <= l && l <= b.1).map_or(0, |b| b.1 - b.0 + 1);
                let by = above + if up { 0 } else { own };
                s.0 .0 += by;
                s.1 .0 += by;
            }
        }
        LineCmd::Delete => {
            let mut carets = Vec::new();
            let mut removed = 0;
            for b in &blocks {
                let col = sels.iter().find(|s| {
                    let l = lines.line_of(s.1);
                    b.0 <= l && l <= b.1
                }).map_or(0, |s| lines.at(s.1).1);
                carets.push((b.0 - removed, col));
                out.drain(b.0 - removed..=b.1 - removed);
                removed += b.1 - b.0 + 1;
            }
            if out.is_empty() {
                out.push(String::new());
            }
            let new = out.join("\n");
            let new_lines = Lines::new(&new);
            let place = |(l, c): (usize, usize)| {
                let l = l.min(new_lines.count() - 1);
                new_lines.start(l) + fit(new_lines.get(l), c)
            };
            let main_line = lines.line_of(sels[main].1);
            let main_block = blocks.iter().position(|b| b.0 <= main_line && main_line <= b.1).unwrap_or(0);
            let sels: Vec<Sel> = carets.iter().map(|c| { let p = place(*c); (p, p) }).collect();
            set_selections(e, sels, main_block);
            return (new != text).then_some(new);
        }
        LineCmd::Insert(above) => {
            let mut targets: Vec<usize> = sels.iter().map(|s| lines.line_of(s.1)).collect();
            targets.dedup();
            let main_line = lines.line_of(sels[main].1);
            let mut carets = Vec::new();
            for (k, &l) in targets.iter().enumerate().rev() {
                let line = lines.get(l);
                let indent = indent_of(line).to_string();
                let extra = if !above && line.trim_end().ends_with('{') { "    " } else { "" };
                let new_line = format!("{indent}{extra}");
                let at_line = if above { l } else { l + 1 };
                out.insert(at_line, new_line.clone());
                carets.push((k, at_line, new_line.len()));
            }
            // (Each new line moves the ones after it down.)
            let mut sels_out = Vec::new();
            let mut main_out = 0;
            for &(k, at_line, col) in carets.iter().rev() {
                let line = at_line + k;
                if targets[k] == main_line {
                    main_out = sels_out.len();
                }
                sels_out.push((line, col));
            }
            let new = out.join("\n");
            let new_lines = Lines::new(&new);
            let sels: Vec<Sel> = sels_out.iter().map(|&(l, c)| { let p = new_lines.start(l) + c; (p, p) }).collect();
            set_selections(e, sels, main_out);
            return Some(new);
        }
    }
    // Columns after text put in or taken out move with it.
    for (i, s) in at.iter_mut().enumerate() {
        for (end, (line, col)) in [&mut s.0, &mut s.1].into_iter().enumerate() {
            let starts_sel = if end == 0 { starts[i].0 } else { starts[i].1 };
            for &(_, c, n) in shifts.iter().filter(|x| x.0 == *line) {
                if n > 0 {
                    if *col > c || (*col == c && !starts_sel) {
                        *col += n as usize;
                    }
                } else if *col > c {
                    *col -= (*col - c).min((-n) as usize);
                }
            }
        }
    }
    let new = out.join("\n");
    let new_lines = Lines::new(&new);
    let place = |(l, c): (usize, usize)| {
        let l = l.min(new_lines.count() - 1);
        new_lines.start(l) + fit(new_lines.get(l), c)
    };
    let sels: Vec<Sel> = at.iter().map(|(a, f)| (place(*a), place(*f))).collect();
    set_selections(e, sels, main);
    (new != text).then_some(new)
}

/// Column `c` on `line`, no further than its end, at a character boundary.
fn fit(line: &str, c: usize) -> usize {
    let mut c = c.min(line.len());
    while !line.is_char_boundary(c) {
        c -= 1;
    }
    c
}

// ===== brackets =====

/// The brackets that are code (not in strings or comments) and the one each
/// pairs with: (offset, partner's offset), by offset. `(`, `[` and `{` each
/// pair with their own kind.
pub fn bracket_pairs(text: &str) -> Vec<(usize, usize)> {
    let skip = tokens(text).into_iter().filter(|(_, k)| matches!(k, Token::String | Token::Comment)).map(|(r, _)| r);
    let mut skip = skip.peekable();
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut out = Vec::new();
    for (i, b) in text.bytes().enumerate() {
        while skip.peek().is_some_and(|r| r.end <= i) {
            skip.next();
        }
        if skip.peek().is_some_and(|r| r.start <= i) {
            continue;
        }
        match b {
            b'(' | b'[' | b'{' => stack.push((b, i)),
            b')' | b']' | b'}' => {
                let open = match b {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                if let Some(k) = stack.iter().rposition(|(c, _)| *c == open) {
                    let (_, at) = stack[k];
                    stack.truncate(k);
                    out.push((at, i));
                    out.push((i, at));
                }
            }
            _ => {}
        }
    }
    out.sort_unstable();
    out
}

/// The bracket next to a caret at `at` and its partner (the one before
/// the caret first).
pub fn matching(pairs: &[(usize, usize)], text: &str, at: usize) -> Option<(usize, usize)> {
    let find = |i: usize| pairs.binary_search_by_key(&i, |p| p.0).ok().map(|k| pairs[k]);
    let before = at.checked_sub(1).filter(|i| matches!(text.as_bytes().get(*i), Some(b')' | b']' | b'}' | b'(' | b'[' | b'{')));
    before.and_then(find).or_else(|| find(at))
}

// ===== folding =====

/// The blocks that can be folded: a `{` whose `}` is on a later line, the
/// first such on each line, as (line from 0, `{` offset, `}` offset).
pub fn foldable(text: &str, pairs: &[(usize, usize)]) -> Vec<(usize, usize, usize)> {
    let lines = Lines::new(text);
    let mut out: Vec<(usize, usize, usize)> = Vec::new();
    for &(open, close) in pairs {
        if close < open || text.as_bytes()[open] != b'{' {
            continue;
        }
        let line = lines.line_of(open);
        if lines.line_of(close) > line && out.last().is_none_or(|f| f.0 != line) {
            out.push((line, open, close));
        }
    }
    out
}

/// The folded blocks of a code editor, as (`{` offset, `}` offset), and
/// the text they were for.
#[derive(Default, Clone)]
struct Folds {
    text: String,
    folds: Vec<(usize, usize)>,
}

thread_local! {
    static FOLDS: RefCell<HashMap<u64, Folds>> = RefCell::new(HashMap::new());
}

/// An editor's folded blocks in `text`, as (`{` offset, `}` offset), by
/// offset. After the text changed, they move with it: a block an edit went
/// into (past its `{`, before its `}`) unfolds, and so does one that no
/// longer is a block over several lines.
pub fn folds(key: u64, text: &str) -> Vec<(usize, usize)> {
    FOLDS.with(|f| {
        let mut all = f.borrow_mut();
        let Some(state) = all.get_mut(&key) else { return Vec::new() };
        if state.text != text {
            let old = std::mem::take(&mut state.text);
            let p = common_prefix(&old, text);
            let s = common_suffix(&old[p..], &text[p..]);
            let old_end = old.len() - s;
            let delta = text.len() as isize - old.len() as isize;
            let shift = |x: usize| if x >= old_end { (x as isize + delta) as usize } else { x };
            let insertion = p == old_end;
            let moved: Vec<(usize, usize)> = state
                .folds
                .iter()
                .filter(|(open, close)| old_end <= *open || p > *close || (insertion && (p == open + 1 || p == *close)))
                .map(|&(open, close)| (shift(open), shift(close)))
                .collect();
            let pairs = if moved.is_empty() { Vec::new() } else { bracket_pairs(text) };
            let blocks = foldable(text, &pairs);
            state.folds = moved.into_iter().filter(|f| blocks.iter().any(|b| (b.1, b.2) == *f)).collect();
            state.text = text.to_string();
        }
        let out = state.folds.clone();
        if out.is_empty() {
            all.remove(&key);
        }
        out
    })
}

/// Folds (or unfolds) the block from `open` to `close` in `text`.
pub fn set_fold(key: u64, text: &str, open: usize, close: usize, folded: bool) {
    let mut now = folds(key, text);
    now.retain(|f| f.0 != open);
    if folded {
        now.push((open, close));
        now.sort_unstable();
    }
    FOLDS.with(|f| {
        let mut all = f.borrow_mut();
        if now.is_empty() {
            all.remove(&key);
        } else {
            all.insert(key, Folds { text: text.to_string(), folds: now });
        }
    });
}

/// Folds (`fold`) or unfolds the innermost block around offset `at` (its
/// lines, from the `{` one to the `}` one) that isn't already so. Returns
/// the block: (`{` offset, `}` offset).
pub fn fold_at(key: u64, text: &str, at: usize, fold: bool) -> Option<(usize, usize)> {
    let blocks = foldable(text, &bracket_pairs(text));
    let lines = Lines::new(text);
    let line = lines.line_of(at.min(text.len()));
    let now = folds(key, text);
    let pick = blocks
        .iter()
        .filter(|b| b.0 <= line && line <= lines.line_of(b.2) && now.iter().any(|f| f.0 == b.1) != fold)
        .max_by_key(|b| b.1)?;
    set_fold(key, text, pick.1, pick.2, fold);
    Some((pick.1, pick.2))
}

/// Folds or unfolds the block that starts on `line` (from 0). Returns the
/// block and whether it's folded now.
pub fn toggle_line(key: u64, text: &str, line: usize) -> Option<(usize, usize, bool)> {
    let blocks = foldable(text, &bracket_pairs(text));
    let b = blocks.iter().find(|b| b.0 == line)?;
    let folded = !folds(key, text).iter().any(|f| f.0 == b.1);
    set_fold(key, text, b.1, b.2, folded);
    Some((b.1, b.2, folded))
}

/// The line (from 0) of offset `at` in `text`.
pub fn line_of(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].matches('\n').count()
}

/// Where line `line` (from 0) of `text` starts, and its length.
pub fn line_span(text: &str, line: usize) -> Option<(usize, usize)> {
    let lines = Lines::new(text);
    (line < lines.count()).then(|| (lines.start(line), lines.get(line).len()))
}

/// Moves the selections' ends that a block just folded hides to just
/// after its `{`.
pub fn out_of_fold(e: &mut Editing, open: usize, close: usize) {
    let fix = |p: &mut usize| {
        if open + 1 < *p && *p < close {
            *p = open + 1;
        }
    };
    fix(&mut e.anchor);
    fix(&mut e.focus);
    for (a, f) in &mut e.others {
        fix(a);
        fix(f);
    }
    let (sels, main) = selections(e);
    set_selections(e, sels, main);
}

/// The text being edited with its selections marked, for the `editing`
/// script command: `|` where a caret (or a selection's moving end) is, `«`
/// and `»` around a selection.
pub fn marked(text: &str, e: &Editing) -> String {
    let (sels, _) = selections(e);
    let mut out = String::new();
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), '\0'))) {
        for s in &sels {
            if s.0 != s.1 && lo(*s) == i {
                out.push('«');
            }
            if s.1 == i {
                out.push('|');
            }
            if s.0 != s.1 && hi(*s) == i {
                out.push('»');
            }
        }
        if c != '\0' {
            out.push(c);
        }
    }
    out
}

/// Whether an editor has folded blocks.
pub fn has_folds(key: u64) -> bool {
    FOLDS.with(|f| f.borrow().contains_key(&key))
}

/// Unfolds the blocks that hide one of the selections' ends.
pub fn reveal(key: u64, text: &str, e: &Editing) -> bool {
    let now = folds(key, text);
    if now.is_empty() {
        return false;
    }
    let (sels, _) = selections(e);
    let hidden = |f: &(usize, usize)| sels.iter().any(|s| [s.0, s.1].iter().any(|&p| f.0 + 1 < p && p < f.1));
    let gone: Vec<(usize, usize)> = now.iter().copied().filter(hidden).collect();
    for f in &gone {
        set_fold(key, text, f.0, f.1, false);
    }
    !gone.is_empty()
}

/// What shows in place of a folded block's inside.
pub const FOLDED: &str = " … ";

/// The outermost of `folds` (those inside another are hidden with it).
pub fn outermost(folds: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &f in folds {
        if out.last().is_none_or(|l| f.0 > l.1) {
            out.push(f);
        }
    }
    out
}

/// The text that shows with blocks folded: each one's inside (after its
/// `{`, up to its `}`) is `FOLDED`.
pub fn folded_text(text: &str, shown: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for &(open, close) in shown {
        out.push_str(&text[at..open + 1]);
        out.push_str(FOLDED);
        at = close;
    }
    out.push_str(&text[at..]);
    out
}

/// Turns a layout of `folded_text` into one of the whole text: its lines
/// and caret stops get the text's offsets (none inside a folded block, so
/// the caret steps over it).
pub fn unfold_layout(t: &mut TextLayout, shown: &[(usize, usize)], len: usize) {
    if shown.is_empty() {
        return;
    }
    // Where each block's `FOLDED` starts in the shown text.
    let mut marks = Vec::new();
    let mut gone = 0isize;
    for &(open, close) in shown {
        let at = (open as isize + 1 - gone) as usize;
        marks.push((at, open + 1, close));
        gone += close as isize - (open as isize + 1) - FOLDED.len() as isize;
    }
    let map = |d: usize| -> Option<usize> {
        let mut extra = 0isize;
        for &(at, start, end) in &marks {
            if d < at {
                break;
            }
            if d == at {
                return Some(start);
            }
            if d < at + FOLDED.len() {
                return None;
            }
            extra += (end - start) as isize - FOLDED.len() as isize;
        }
        Some((d as isize + extra) as usize)
    };
    let end_map = |d: usize| -> usize {
        let mut extra = 0isize;
        for &(at, start, end) in &marks {
            if d <= at {
                break;
            }
            if d < at + FOLDED.len() {
                return end;
            }
            extra += (end - start) as isize - FOLDED.len() as isize;
        }
        (d as isize + extra) as usize
    };
    for line in &mut t.lines {
        line.range = map(line.range.start).unwrap_or(line.range.start)..end_map(line.range.end);
        line.stops = line.stops.iter().filter_map(|&(o, x)| map(o).map(|o| (o, x))).collect();
    }
    t.len = len;
}

/// The line (from 0) each shown line of a folded text is in the whole text.
pub fn shown_lines(text: &str, t: &TextLayout) -> Vec<usize> {
    let lines = Lines::new(text);
    t.lines.iter().map(|l| lines.line_of(l.range.start.min(text.len()))).collect()
}

/// How many bytes two texts start with in common (at a character boundary).
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices().zip(b.chars()).find(|((_, x), y)| x != y).map_or(a.len().min(b.len()), |((i, _), _)| i)
}

/// How many bytes two texts end with in common (at a character boundary).
fn common_suffix(a: &str, b: &str) -> usize {
    a.chars().rev().zip(b.chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum()
}

/// What a code editor has worked out about its text, for drawing it: its
/// brackets' pairs, the blocks that can be folded and those that are, and
/// the line of the whole text each shown line is.
#[derive(Default, Debug)]
pub struct Info {
    pub pairs: Vec<(usize, usize)>,
    pub foldable: Vec<(usize, usize, usize)>,
    pub folded: Vec<(usize, usize)>,
    pub lines: Vec<usize>,
}

impl Info {
    /// The shown line (from 0) a line of the text (from 0) is on, if it shows.
    pub fn shown_line(&self, line: usize) -> Option<usize> {
        if self.folded.is_empty() {
            return Some(line);
        }
        self.lines.binary_search(&line).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::TextEngine;

    /// Text with selections marked: `|` a caret (or a selection's focus), `«`
    /// and `»` a selection's ends. The first marked is the main one.
    fn parse(marked: &str) -> (String, Editing) {
        let mut text = String::new();
        let mut sels: Vec<Sel> = Vec::new();
        let mut open: Option<usize> = None;
        let mut focus: Option<usize> = None;
        for c in marked.chars() {
            match c {
                '«' => open = Some(text.len()),
                '»' => {
                    let a = open.take().unwrap();
                    let f = focus.take().unwrap();
                    let other = if f == a { text.len() } else { a };
                    sels.push((other, f));
                }
                '|' if open.is_some() => focus = Some(text.len()),
                '|' => sels.push((text.len(), text.len())),
                _ => text.push(c),
            }
        }
        let mut e = Editing::new(1, &text);
        (e.anchor, e.focus) = sels[0];
        e.others = sels[1..].to_vec();
        (text, e)
    }

    fn show(text: &str, e: &Editing) -> String {
        marked(text, e)
    }

    fn run(marked: &str, edits: Vec<Edit>) -> String {
        let (mut text, mut e) = parse(marked);
        let mut engine = TextEngine::new();
        let mut clip = Clipboard::Memory(String::new());
        for edit in edits {
            let layout = engine.code_layout(&text, 13.0);
            if let Some(t) = apply(&text, &layout, &mut e, edit, &mut clip) {
                text = t;
            }
        }
        show(&text, &e)
    }

    fn cmd(marked: &str, c: Cmd) -> String {
        run(marked, vec![Edit::Code(c)])
    }

    #[test]
    fn markers_round_trip() {
        for m in ["a|b", "«ab|»c", "«|ab»c", "x|\ny|"] {
            let (t, e) = parse(m);
            assert_eq!(show(&t, &e), m);
        }
    }

    #[test]
    fn toggles_line_comments() {
        assert_eq!(cmd("let a|", Cmd::ToggleComment), "// let a|");
        assert_eq!(cmd("// let a|", Cmd::ToggleComment), "let a|");
        assert_eq!(cmd("//let a|", Cmd::ToggleComment), "let a|");
        // At the selection's least indentation; empty lines stay empty.
        assert_eq!(cmd("    «if a {\n\n        b\n    }|»", Cmd::ToggleComment), "    «// if a {\n\n    //     b\n    // }|»");
        // All commented: uncommented; some not: all commented.
        assert_eq!(cmd("«// a\n// b|»", Cmd::ToggleComment), "«a\nb|»");
        assert_eq!(cmd("«// a\nb|»", Cmd::ToggleComment), "«// // a\n// b|»");
        // An empty line gets one.
        assert_eq!(cmd("|", Cmd::ToggleComment), "// |");
        // A selection ending at the start of a line leaves that line out.
        assert_eq!(cmd("«a\n|»b", Cmd::ToggleComment), "«// a\n|»b");
    }

    #[test]
    fn moves_and_copies_lines() {
        assert_eq!(cmd("a\nb|\nc", Cmd::MoveLines { up: true }), "b|\na\nc");
        assert_eq!(cmd("a|\nb\nc", Cmd::MoveLines { up: true }), "a|\nb\nc");
        assert_eq!(cmd("a\nb|\nc", Cmd::MoveLines { up: false }), "a\nc\nb|");
        assert_eq!(cmd("a\nb\nc|", Cmd::MoveLines { up: false }), "a\nb\nc|");
        assert_eq!(cmd("a\n«b\nc|»\nd", Cmd::MoveLines { up: true }), "«b\nc|»\na\nd");
        assert_eq!(cmd("x|y\nz", Cmd::CopyLines { up: false }), "xy\nx|y\nz");
        assert_eq!(cmd("x|y\nz", Cmd::CopyLines { up: true }), "x|y\nxy\nz");
        // (Lines next to each other are one block.)
        assert_eq!(cmd("a|\nb|", Cmd::CopyLines { up: false }), "a\nb\na|\nb|");
    }

    #[test]
    fn deletes_and_inserts_lines() {
        assert_eq!(cmd("one\ntw|o\nthree", Cmd::DeleteLines), "one\nth|ree");
        assert_eq!(cmd("one\ntwo|", Cmd::DeleteLines), "one|");
        assert_eq!(cmd("only|", Cmd::DeleteLines), "|");
        assert_eq!(cmd("    if a {|", Cmd::InsertLine { above: false }), "    if a {\n        |");
        assert_eq!(cmd("  a|b", Cmd::InsertLine { above: true }), "  |\n  ab");
        assert_eq!(cmd("a|\nb|", Cmd::InsertLine { above: false }), "a\n|\nb\n|");
    }

    #[test]
    fn indents_and_outdents() {
        assert_eq!(cmd("«a\n\nb|»", Cmd::Tab { shift: false }), "«    a\n\n    b|»");
        assert_eq!(cmd("a|b", Cmd::Tab { shift: false }), "a    |b");
        assert_eq!(cmd("      a|", Cmd::Tab { shift: true }), "  a|");
        assert_eq!(cmd("  a|", Cmd::Outdent), "a|");
        assert_eq!(cmd("a|", Cmd::Outdent), "a|");
        assert_eq!(cmd("a|", Cmd::Indent), "    a|");
    }

    #[test]
    fn closes_brackets_and_quotes() {
        let t = |m: &str, s: &str| cmd(m, Cmd::Type(s.into()));
        assert_eq!(t("|", "("), "(|)");
        assert_eq!(t("f|", "["), "f[|]");
        assert_eq!(t("|x", "("), "(|x");
        assert_eq!(t("(|)", ")"), "()|");
        assert_eq!(t("|", "\""), "\"|\"");
        assert_eq!(t("ab|", "\""), "ab\"|");
        assert_eq!(t("\"ab|\"", "\""), "\"ab\"|");
        assert_eq!(t("\"a |b\"", "\""), "\"a \"|b\"");
        assert_eq!(t("«ab|»", "("), "(«ab|»)");
        assert_eq!(t("«ab|»", "\""), "\"«ab|»\"");
        assert_eq!(run("(|)", vec![Edit::Backspace(Unit::Char)]), "|");
        assert_eq!(run("(|x)", vec![Edit::Backspace(Unit::Char)]), "|x)");
    }

    #[test]
    fn enter_keeps_indentation() {
        let t = |m: &str| cmd(m, Cmd::Type("\n".into()));
        assert_eq!(t("    a|"), "    a\n    |");
        assert_eq!(t("    a {|"), "    a {\n        |");
        assert_eq!(t("    a {|}"), "    a {\n        |\n    }");
        assert_eq!(t("a   |"), "a\n|");
        assert_eq!(t("    |"), "\n    |");
        assert_eq!(t("        |", ), "\n        |");
        assert_eq!(cmd("    |", Cmd::Type("}".into())), "}|");
    }

    #[test]
    fn several_cursors_type_delete_and_paste() {
        assert_eq!(run("a|\nb|", vec![Edit::Code(Cmd::Type("x".into()))]), "ax|\nbx|");
        assert_eq!(run("ab|\ncd|", vec![Edit::Backspace(Unit::Char)]), "a|\nc|");
        assert_eq!(run("ab|\ncd|", vec![Edit::Left { unit: Unit::Char, extend: true }, Edit::Code(Cmd::Type("Z".into()))]), "aZ|\ncZ|");
        // Carets that meet become one.
        assert_eq!(run("a|b|", vec![Edit::Left { unit: Unit::Line, extend: false }]), "|ab");
        let mut clip = Clipboard::Memory("1\n2".into());
        let (text, mut e) = parse("a|\nb|");
        let mut engine = TextEngine::new();
        let layout = engine.code_layout(&text, 13.0);
        let new = apply(&text, &layout, &mut e, Edit::Paste, &mut clip).unwrap();
        assert_eq!(show(&new, &e), "a1|\nb2|");
        // Copying joins the selections' text, a line each.
        let (text, mut e) = parse("«a|»\n«b|»");
        apply(&text, &layout, &mut e, Edit::Copy, &mut clip);
        assert_eq!(clip.get(), "a\nb");
    }

    #[test]
    fn select_next_and_lines() {
        assert_eq!(cmd("let ab| = ab + abc", Cmd::SelectNext), "let «ab|» = ab + abc");
        assert_eq!(run("let ab| = ab + abc", vec![Edit::Code(Cmd::SelectNext), Edit::Code(Cmd::SelectNext)]), "let «ab|» = «ab|» + abc");
        // Whole words only, and round from the start.
        assert_eq!(cmd("ab + abc + «ab|»", Cmd::SelectNext), "«ab|» + abc + «ab|»");
        assert_eq!(cmd("a\nb|\nc", Cmd::SelectLine), "a\n«b\n|»c");
        assert_eq!(cmd("a\n«b\n|»c", Cmd::SelectLine), "a\n«b\nc|»");
        assert_eq!(cmd("    ab|", Cmd::Home { extend: false }), "    |ab");
        assert_eq!(cmd("    |ab", Cmd::Home { extend: false }), "|    ab");
    }

    #[test]
    fn finds_brackets_outside_strings() {
        let text = "f(\"(\") { a[1] } // (";
        let pairs = bracket_pairs(text);
        assert_eq!(matching(&pairs, text, 0), None);
        assert_eq!(matching(&pairs, text, 1), Some((1, 5)));
        assert_eq!(matching(&pairs, text, 6), Some((5, 1)));
        assert_eq!(matching(&pairs, text, 8), Some((7, 14)));
        assert_eq!(matching(&pairs, text, 15), Some((14, 7)));
        // The `(` in the string has no partner.
        assert_eq!(matching(&pairs, text, 4), None);
    }

    #[test]
    fn folds_move_with_edits() {
        let text = "a {\n    b\n}\nc {\n    d\n}";
        let pairs = bracket_pairs(text);
        let blocks = foldable(text, &pairs);
        assert_eq!(blocks.iter().map(|b| b.0).collect::<Vec<_>>(), [0, 3]);
        set_fold(9, text, blocks[1].1, blocks[1].2, true);
        assert_eq!(folded_text(text, &folds(9, text)), "a {\n    b\n}\nc { … }");
        // An edit before it moves it; one inside unfolds it.
        let moved = format!("x{text}");
        assert_eq!(folds(9, &moved), [(15, 23)]);
        let inside = moved.replace("d", "dd");
        assert!(folds(9, &inside).is_empty());
    }
}
