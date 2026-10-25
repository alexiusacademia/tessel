//! The text of a `RichTextEditor`: plain text with stretches (runs) in
//! styles of their own, as the program's `RichText` holds it.
//!
//! The program and the editor pass the text as JSON (the program's
//! `toJson` of its `RichText`), so nothing here depends on how the program
//! keeps its values. A run's `start` and `end` count characters, as the
//! program's `String.count` does; here they're byte offsets.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use serde_json::{Map, Value};

use super::Rgba;

/// How a stretch of text looks, besides the editor's own style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Its size in points (none: the editor's).
    pub size: Option<f64>,
    /// Its `Color`, as the program's JSON says it (none: the editor's).
    pub color: Option<Value>,
    /// Its font family (none: the editor's).
    pub font: Option<String>,
}

impl Marks {
    fn is_plain(&self) -> bool {
        *self == Marks::default()
    }
}

/// A `RichText`: its text, and its runs (byte ranges of the text, in
/// order, apart, each with marks that aren't plain).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub text: String,
    pub runs: Vec<(Range<usize>, Marks)>,
}

/// Where each character starts, in bytes, and the end.
fn char_starts(text: &str) -> Vec<usize> {
    text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect()
}

/// The marks a run of the program's JSON gives.
fn marks_of(r: &Value) -> Marks {
    let flag = |k: &str| r.get(k).and_then(Value::as_bool).unwrap_or(false);
    Marks {
        bold: flag("bold"),
        italic: flag("italic"),
        underline: flag("underline"),
        strike: flag("strikethrough"),
        size: r.get("size").and_then(Value::as_f64).filter(|s| *s > 0.0 && s.is_finite()),
        color: r.get("color").filter(|c| !c.is_null() && c.as_str() != Some("primary")).cloned(),
        font: r.get("font").and_then(Value::as_str).filter(|f| !f.is_empty()).map(str::to_string),
    }
}

impl Doc {
    /// A `RichText` from the program's JSON. Runs that overlap are taken in
    /// order: a later one's marks replace an earlier one's where they meet.
    pub fn parse(json: &str) -> Doc {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let starts = char_starts(&text);
        let at = |c: Option<i64>| starts[(c.unwrap_or(0).max(0) as usize).min(starts.len() - 1)];
        let mut doc = Doc { text, runs: Vec::new() };
        for r in v.get("runs").and_then(Value::as_array).into_iter().flatten() {
            let (start, end) = (at(r.get("start").and_then(Value::as_i64)), at(r.get("end").and_then(Value::as_i64)));
            if start < end {
                let marks = marks_of(r);
                doc.change(start..end, |m| *m = marks.clone());
            }
        }
        doc
    }

    /// The program's JSON of it: only the marks that aren't plain.
    pub fn to_json(&self) -> String {
        let starts = char_starts(&self.text);
        let char_at = |b: usize| starts.partition_point(|&s| s < b) as i64;
        let runs: Vec<Value> = self
            .runs
            .iter()
            .map(|(range, m)| {
                let mut r = Map::new();
                r.insert("start".into(), char_at(range.start).into());
                r.insert("end".into(), char_at(range.end).into());
                for (key, on) in [("bold", m.bold), ("italic", m.italic), ("underline", m.underline), ("strikethrough", m.strike)] {
                    if on {
                        r.insert(key.into(), true.into());
                    }
                }
                if let Some(s) = m.size {
                    r.insert("size".into(), s.into());
                }
                if let Some(c) = &m.color {
                    r.insert("color".into(), c.clone());
                }
                if let Some(f) = &m.font {
                    r.insert("font".into(), f.clone().into());
                }
                Value::Object(r)
            })
            .collect();
        let mut doc = Map::new();
        doc.insert("text".into(), self.text.clone().into());
        doc.insert("runs".into(), runs.into());
        Value::Object(doc).to_string()
    }

    /// The whole text in pieces, each with its marks (plain ones too).
    fn pieces(&self) -> Vec<(Range<usize>, Marks)> {
        let mut out = Vec::new();
        let mut at = 0;
        for (range, m) in &self.runs {
            if range.start > at {
                out.push((at..range.start, Marks::default()));
            }
            out.push((range.clone(), m.clone()));
            at = range.end;
        }
        if at < self.text.len() {
            out.push((at..self.text.len(), Marks::default()));
        }
        out
    }

    /// Keeps the runs that aren't plain, joining ones next to each other
    /// that look the same.
    fn set_pieces(&mut self, pieces: Vec<(Range<usize>, Marks)>) {
        let mut runs: Vec<(Range<usize>, Marks)> = Vec::new();
        for (range, m) in pieces {
            if range.is_empty() || m.is_plain() {
                continue;
            }
            match runs.last_mut() {
                Some((last, lm)) if last.end == range.start && *lm == m => last.end = range.end,
                _ => runs.push((range, m)),
            }
        }
        self.runs = runs;
    }

    /// The marks at a byte of the text.
    pub fn marks_at(&self, at: usize) -> Marks {
        self.runs.iter().find(|(r, _)| r.start <= at && at < r.end).map_or_else(Marks::default, |(_, m)| m.clone())
    }

    /// Changes the marks of the text in `range`.
    pub fn change(&mut self, range: Range<usize>, f: impl Fn(&mut Marks)) {
        let mut out = Vec::new();
        for (r, m) in self.pieces() {
            // The part before the range, the part in it, and the part after.
            let (a, b) = (r.start.max(range.start).min(r.end), r.end.min(range.end).max(r.start));
            if r.start < a {
                out.push((r.start..a, m.clone()));
            }
            if a < b {
                let mut changed = m.clone();
                f(&mut changed);
                out.push((a..b, changed));
            }
            if b < r.end {
                out.push((b..r.end, m));
            }
        }
        self.set_pieces(out);
    }

    /// Puts `new` in place of the text in `range`, in `marks`.
    pub fn replace(&mut self, range: Range<usize>, new: &str, marks: &Marks) {
        let shift = |b: usize| if b >= range.end { b - (range.end - range.start) + new.len() } else { b };
        let mut out = Vec::new();
        for (r, m) in self.pieces() {
            // What's left of it outside the range, moved along.
            let before = r.start..r.end.min(range.start);
            let after = r.start.max(range.end)..r.end;
            if !before.is_empty() {
                out.push((before, m.clone()));
            }
            if !after.is_empty() {
                out.push((shift(after.start)..shift(after.end), m));
            }
        }
        // The new text, where the range was.
        if !new.is_empty() {
            let at = range.start..range.start + new.len();
            let i = out.iter().position(|(r, _)| r.start >= range.start).unwrap_or(out.len());
            out.insert(i, (at, marks.clone()));
        }
        self.text.replace_range(range, new);
        self.set_pieces(out);
    }
}

impl Doc {
    /// The part of it in `range` (bytes), as a text of its own.
    pub fn slice(&self, range: Range<usize>) -> Doc {
        let runs = self
            .runs
            .iter()
            .filter_map(|(r, m)| {
                let (a, b) = (r.start.max(range.start), r.end.min(range.end));
                (a < b).then(|| (a - range.start..b - range.start, m.clone()))
            })
            .collect();
        Doc { text: self.text[range].to_string(), runs }
    }

    /// Puts `other` (with its styles) in place of the text in `range`.
    pub fn splice(&mut self, range: Range<usize>, other: &Doc) {
        let at = range.start;
        self.replace(range, &other.text, &Marks::default());
        for (r, m) in &other.runs {
            self.change(at + r.start..at + r.end, |x| *x = m.clone());
        }
    }

    /// The text as HTML, for the clipboard: for other apps, its styles in
    /// tags and CSS; for Tessel's editors, also exactly as the program
    /// keeps it (`data-tessel-rich`).
    pub fn to_html(&self) -> String {
        let escape = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let mut out = format!("<meta charset=\"utf-8\"><div data-tessel-rich=\"{}\">", escape(&self.to_json()));
        for (r, m) in self.pieces() {
            let mut css = Vec::new();
            if let Some(s) = m.size {
                css.push(format!("font-size:{s}px"));
            }
            if let Some(c) = m.color.as_ref().and_then(color_of) {
                css.push(format!("color:#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]));
            }
            if let Some(f) = &m.font {
                css.push(format!("font-family:'{}'", f.replace('\'', "")));
            }
            let mut open = String::new();
            let mut close = String::new();
            for (on, tag) in [(m.bold, "b"), (m.italic, "i"), (m.underline, "u"), (m.strike, "s")] {
                if on {
                    open += &format!("<{tag}>");
                    close = format!("</{tag}>{close}");
                }
            }
            if !css.is_empty() {
                open = format!("<span style=\"{}\">{open}", css.join(";"));
                close += "</span>";
            }
            out += &open;
            out += &escape(&self.text[r]).replace('\n', "<br>");
            out += &close;
        }
        out + "</div>"
    }

    /// A text from HTML on the clipboard: exactly, when a Tessel editor put
    /// it there; otherwise its text, with the styles HTML gives it that an
    /// editor has (bold, italic, underline, strikethrough, sizes, colors and
    /// fonts).
    pub fn from_html(html: &str) -> Doc {
        if let Some(json) = attribute(html, "data-tessel-rich") {
            return Doc::parse(&json);
        }
        let mut doc = Doc::default();
        // Each open element, with the marks inside it.
        let mut stack: Vec<(String, Marks)> = vec![(String::new(), Marks::default())];
        // Whether text is to be left out (a `<style>`, …), or kept as it is (`<pre>`).
        let (mut hidden, mut pre) = (0usize, 0usize);
        let mut pieces: Vec<(String, Marks)> = Vec::new();
        let mut rest = html;
        let mut space = true;
        let newline = |pieces: &mut Vec<(String, Marks)>| {
            let ends = pieces.iter().rev().find(|p| !p.0.is_empty()).is_none_or(|p| p.0.ends_with('\n'));
            if !ends {
                pieces.push(("\n".into(), Marks::default()));
            }
        };
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix("<!--") {
                rest = after.find("-->").map_or("", |i| &after[i + 3..]);
                continue;
            }
            if rest.starts_with('<') {
                let end = rest.find('>').unwrap_or(rest.len() - 1);
                let tag = &rest[1..end];
                rest = &rest[end + 1..];
                let closing = tag.starts_with('/');
                let body = tag.trim_start_matches('/').trim_end_matches('/');
                let name = body.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
                let block = matches!(name.as_str(), "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre");
                if matches!(name.as_str(), "style" | "script" | "head" | "title") {
                    if closing {
                        hidden = hidden.saturating_sub(1);
                    } else if !tag.ends_with('/') {
                        hidden += 1;
                    }
                    continue;
                }
                if name == "br" {
                    pieces.push(("\n".into(), Marks::default()));
                    space = true;
                    continue;
                }
                if closing {
                    if let Some(i) = stack.iter().rposition(|(n, _)| *n == name) {
                        if i > 0 {
                            stack.truncate(i);
                        }
                    }
                    if name == "pre" {
                        pre = pre.saturating_sub(1);
                    }
                    if block {
                        newline(&mut pieces);
                        space = true;
                    }
                    continue;
                }
                if block {
                    newline(&mut pieces);
                    space = true;
                }
                if name == "pre" {
                    pre += 1;
                }
                // Elements that hold nothing aren't kept open.
                if matches!(name.as_str(), "meta" | "img" | "hr" | "input" | "link") || tag.ends_with('/') {
                    continue;
                }
                let mut m = stack.last().map(|s| s.1.clone()).unwrap_or_default();
                match name.as_str() {
                    "b" | "strong" => m.bold = true,
                    "i" | "em" => m.italic = true,
                    "u" | "ins" => m.underline = true,
                    "s" | "strike" | "del" => m.strike = true,
                    "h1" | "h2" | "h3" | "th" => m.bold = true,
                    _ => {}
                }
                if name == "font" {
                    if let Some(c) = attribute(body, "color").and_then(|c| css_color(&c)) {
                        m.color = Some(c);
                    }
                    if let Some(f) = attribute(body, "face").and_then(|f| css_family(&f)) {
                        m.font = Some(f);
                    }
                }
                if let Some(style) = attribute(body, "style") {
                    css_marks(&style, &mut m);
                }
                stack.push((name, m));
                continue;
            }
            let end = rest.find('<').unwrap_or(rest.len());
            let raw = &rest[..end];
            rest = &rest[end..];
            if hidden > 0 {
                continue;
            }
            let mut text = entities(raw);
            if pre == 0 {
                // White space in HTML is one space, and none at a line's start.
                let mut out = String::new();
                for c in text.chars() {
                    if c.is_whitespace() && c != '\u{a0}' {
                        if !space {
                            out.push(' ');
                            space = true;
                        }
                    } else {
                        out.push(if c == '\u{a0}' { ' ' } else { c });
                        space = false;
                    }
                }
                text = out;
            }
            if !text.is_empty() {
                pieces.push((text, stack.last().map(|s| s.1.clone()).unwrap_or_default()));
            }
        }
        for (text, m) in pieces {
            let at = doc.text.len();
            doc.text += &text;
            if !m.is_plain() {
                doc.runs.push((at..doc.text.len(), m));
            }
        }
        // (No line break at the very end.)
        while doc.text.ends_with('\n') || doc.text.ends_with(' ') {
            doc.text.pop();
        }
        let len = doc.text.len();
        let pieces = doc.pieces();
        doc.set_pieces(pieces.into_iter().map(|(r, m)| (r.start.min(len)..r.end.min(len), m)).collect());
        doc
    }
}

/// An attribute's value in an HTML tag's text (`name="value"`, `'…'` or bare).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name).map(|i| i + from) {
        from = i + name.len();
        let before = lower[..i].chars().next_back();
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let after = lower[from..].trim_start();
        let Some(value) = after.strip_prefix('=') else { continue };
        let start = tag.len() - value.len();
        let value = tag[start..].trim_start();
        let found = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or(""),
            _ => value.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(entities(found));
    }
    None
}

/// HTML's character references in `text`.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out += &rest[..i];
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if name.starts_with("#x") || name.starts_with("#X") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
                _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// What a `style` attribute says of the marks an editor has.
fn css_marks(style: &str, m: &mut Marks) {
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let raw = v.trim().trim_end_matches("!important").trim();
        let (k, v) = (k.trim().to_ascii_lowercase(), raw.to_ascii_lowercase());
        match k.as_str() {
            "font-weight" => m.bold = v == "bold" || v == "bolder" || v.parse::<f64>().is_ok_and(|w| w >= 600.0),
            "font-style" => m.italic = v == "italic" || v == "oblique",
            "text-decoration" | "text-decoration-line" => {
                if v.contains("underline") {
                    m.underline = true;
                }
                if v.contains("line-through") {
                    m.strike = true;
                }
                if v == "none" {
                    m.underline = false;
                    m.strike = false;
                }
            }
            "font-size" => {
                let n = |suffix: &str| v.strip_suffix(suffix).and_then(|n| n.trim().parse::<f64>().ok());
                if let Some(size) = n("px").or(n("pt")).filter(|s| *s > 1.0 && *s < 1000.0) {
                    m.size = Some((size * 2.0).round() / 2.0);
                }
            }
            "color" => {
                if let Some(c) = css_color(&v) {
                    m.color = Some(c);
                }
            }
            "font-family" => {
                if let Some(f) = css_family(raw) {
                    m.font = Some(f);
                }
            }
            _ => {}
        }
    }
}

/// A CSS color as a `Color`'s JSON (`.rgb`), leaving out black and the
/// system's text color (the editor's own color is better for those).
fn css_color(v: &str) -> Option<Value> {
    let v = v.trim();
    let (r, g, b) = if let Some(hex) = v.strip_prefix('#') {
        let n = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            6 => (n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?),
            3 => (n(&hex[0..1])? * 17, n(&hex[1..2])? * 17, n(&hex[2..3])? * 17),
            _ => return None,
        }
    } else if let Some(args) = v.strip_prefix("rgb(").or(v.strip_prefix("rgba(")) {
        let parts: Vec<u8> = args.trim_end_matches(')').split(',').take(3).filter_map(|p| p.trim().parse::<f64>().ok()).map(|x| x.clamp(0.0, 255.0) as u8).collect();
        if parts.len() < 3 {
            return None;
        }
        (parts[0], parts[1], parts[2])
    } else {
        return None;
    };
    if r < 30 && g < 30 && b < 30 {
        return None;
    }
    Some(serde_json::json!({ "rgb": { "red": r, "green": g, "blue": b } }))
}

/// The first family a CSS `font-family` names, unless it's a generic one.
fn css_family(v: &str) -> Option<String> {
    let first = v.split(',').next()?.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let generic = ["serif", "sans-serif", "monospace", "system-ui", "-apple-system", "cursive", "fantasy", "inherit", "initial"];
    (!first.is_empty() && !generic.contains(&first.to_ascii_lowercase().as_str()) && !first.starts_with('.')).then(|| first.to_string())
}

/// The part of `old` that became something else in `new`: where it starts,
/// where it ends in `old`, and where the new part ends in `new`.
pub fn changed(old: &str, new: &str) -> (usize, usize, usize) {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let room = old.len().min(new.len()) - start;
    let mut same = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - same) || !new.is_char_boundary(new.len() - same) {
        same -= 1;
    }
    (start, old.len() - same, new.len() - same)
}

/// The names of `Color`'s cases, in the program's order (the theme's colors).
const COLOR_NAMES: &[&str] = &[
    "primary", "secondary", "gray", "red", "orange", "yellow", "green", "blue", "purple", "pink", "white", "black",
    "clear", "lightGray", "sidebar", "highlight", "separator", "background",
];

/// The color a `Color`'s JSON stands for.
pub fn color_of(v: &Value) -> Option<Rgba> {
    if let Some(name) = v.as_str() {
        let i = COLOR_NAMES.iter().position(|n| *n == name)?;
        return Some(super::theme::theme().colors[i]);
    }
    let (case, values) = v.as_object()?.iter().next()?;
    let channel = |k: &str| values.get(k).and_then(Value::as_i64).unwrap_or(0).clamp(0, 255) as u8;
    let (r, g, b) = (channel("red"), channel("green"), channel("blue"));
    match case.as_str() {
        "rgb" => Some(Rgba([r, g, b, 255])),
        "rgba" => {
            let a = values.get("opacity").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 1.0);
            Some(Rgba([r, g, b, (a * 255.0).round() as u8]))
        }
        _ => None,
    }
}

// ===== the editors' toolbars =====

/// What a toolbar asks of its editor (`__richApply`), by the editor's id,
/// done when the app next updates.
pub enum Command {
    Bold,
    Italic,
    Underline,
    Strike,
    Bigger,
    Smaller,
    Color(Option<Value>),
    Font(Option<String>),
    Plain,
}

impl Command {
    fn parse(text: &str) -> Option<Command> {
        Some(match text {
            "bold" => Command::Bold,
            "italic" => Command::Italic,
            "underline" => Command::Underline,
            "strikethrough" => Command::Strike,
            "bigger" => Command::Bigger,
            "smaller" => Command::Smaller,
            "plain" => Command::Plain,
            _ => {
                if let Some(c) = text.strip_prefix("color:") {
                    let v: Value = serde_json::from_str(c).ok()?;
                    Command::Color((v.as_str() != Some("primary")).then_some(v))
                } else if let Some(f) = text.strip_prefix("font:") {
                    Command::Font((!f.is_empty()).then(|| f.to_string()))
                } else {
                    return None;
                }
            }
        })
    }
}

/// Text sizes the toolbar's buttons step through.
const SIZES: &[f64] = &[9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 48.0, 64.0, 72.0];

/// The next size up (`bigger`) or down from `size`.
fn step_size(size: f64, bigger: bool) -> f64 {
    if bigger {
        SIZES.iter().copied().find(|&s| s > size + 0.01).unwrap_or(size)
    } else {
        SIZES.iter().rev().copied().find(|&s| s < size - 0.01).unwrap_or(size)
    }
}

thread_local! {
    /// The last id given to an editor (`__richId`).
    static LAST_ID: RefCell<i64> = const { RefCell::new(0) };
    /// What toolbars asked, not yet done.
    pub static COMMANDS: RefCell<Vec<(i64, Command)>> = const { RefCell::new(Vec::new()) };
    /// For each editor (by id): the marks of what's selected, for its
    /// toolbar (`__richMarks`): 1 bold, 2 italic, 4 underline, 8 strikethrough.
    pub static MARKS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
    /// The marks typing takes next in an editor (by key), when they were
    /// changed with nothing selected, while the caret stays at that place.
    pub static TYPING: RefCell<Option<(u64, usize, Marks)>> = const { RefCell::new(None) };
    /// Where each editor's selection was (by key), for its toolbar's
    /// commands once the editor isn't being edited.
    pub static SELECTED: RefCell<HashMap<u64, (usize, usize)>> = RefCell::new(HashMap::new());
    /// The editor typed in last (by key): typing on in it is undone in one step.
    pub static TYPED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// Each editor's undo and redo steps (by key): its text and selection before a change.
    pub static HISTORY: RefCell<HashMap<u64, (Vec<(Doc, usize, usize)>, Vec<(Doc, usize, usize)>)>> = RefCell::new(HashMap::new());
}

/// The marks typing in `doc` with `selection` takes: those changed for the
/// caret there, or the text's before it (after it, at the start).
pub fn typing_marks(key: u64, doc: &Doc, selection: Range<usize>) -> Marks {
    if selection.is_empty() {
        if let Some(m) = TYPING.with(|t| t.borrow().as_ref().filter(|(k, at, _)| *k == key && *at == selection.start).map(|t| t.2.clone())) {
            return m;
        }
    }
    let at = selection.start;
    let before = doc.text[..at].chars().next_back().map(|c| at - c.len_utf8());
    match before {
        // (Not across a line break: a new line starts as the next one does.)
        Some(b) if &doc.text[b..at] != "\n" => doc.marks_at(b),
        _ => doc.marks_at(at),
    }
}

/// The toolbar's marks for what's selected (or for typing, at the caret).
pub fn marks_bits(key: u64, doc: &Doc, selection: Range<usize>) -> i64 {
    let m = if selection.is_empty() {
        typing_marks(key, doc, selection)
    } else {
        // On when all of it is.
        let all = |f: fn(&Marks) -> bool| {
            let mut at = selection.start;
            while at < selection.end {
                let next = doc.runs.iter().find(|(r, _)| r.start <= at && at < r.end).map_or_else(
                    || doc.runs.iter().map(|(r, _)| r.start).find(|&s| s > at).unwrap_or(selection.end),
                    |(r, _)| r.end,
                );
                if !f(&doc.marks_at(at)) {
                    return false;
                }
                at = next.max(at + 1);
            }
            true
        };
        Marks { bold: all(|m| m.bold), italic: all(|m| m.italic), underline: all(|m| m.underline), strike: all(|m| m.strike), ..Default::default() }
    };
    m.bold as i64 | (m.italic as i64) << 1 | (m.underline as i64) << 2 | (m.strike as i64) << 3
}

/// Does a toolbar's command to `doc` with `selection` (in an editor whose
/// text is `base` points). Nothing selected: it's for what's typed next
/// (returns no new text).
pub fn apply(key: u64, doc: &Doc, selection: Range<usize>, command: &Command, base: f64) -> Option<Doc> {
    let bits = marks_bits(key, doc, selection.clone());
    let size_now = doc.marks_at(selection.start).size.unwrap_or(base);
    let change = |m: &mut Marks| match command {
        Command::Bold => m.bold = bits & 1 == 0,
        Command::Italic => m.italic = bits & 2 == 0,
        Command::Underline => m.underline = bits & 4 == 0,
        Command::Strike => m.strike = bits & 8 == 0,
        Command::Bigger | Command::Smaller => {
            let s = step_size(m.size.unwrap_or(base), matches!(command, Command::Bigger));
            m.size = ((s - base).abs() > 0.01).then_some(s);
        }
        Command::Color(c) => m.color = c.clone(),
        Command::Font(f) => m.font = f.clone(),
        Command::Plain => *m = Marks::default(),
    };
    if selection.is_empty() {
        let mut m = typing_marks(key, doc, selection.clone());
        if matches!(command, Command::Bigger | Command::Smaller) {
            m.size = Some(size_now);
        }
        change(&mut m);
        TYPING.with(|t| *t.borrow_mut() = Some((key, selection.start, m)));
        return None;
    }
    let mut d = doc.clone();
    d.change(selection, change);
    Some(d)
}

/// Drops what's kept for editors that aren't in the trees any more.
pub fn keep_only(keys: &std::collections::HashSet<u64>) {
    HISTORY.with(|h| h.borrow_mut().retain(|k, _| keys.contains(k)));
    SELECTED.with(|s| s.borrow_mut().retain(|k, _| keys.contains(k)));
}

/// `__richId()`: a new editor's id, for its toolbar.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_rich_id() -> i64 {
    LAST_ID.with(|l| {
        *l.borrow_mut() += 1;
        *l.borrow()
    })
}

/// `__richMarks(id)`: the marks of what's selected in an editor (see `MARKS`).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_rich_marks(id: i64) -> i64 {
    MARKS.with(|m| m.borrow().get(&id).copied().unwrap_or(0))
}

/// `__richApply(id, command)`: a toolbar's command for its editor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_rich_apply(id: i64, command: *const crate::Str) {
    let text = unsafe { crate::str_text(command) };
    if let Some(c) = Command::parse(text) {
        COMMANDS.with(|q| q.borrow_mut().push((id, c)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_other_apps_html() {
        let html = r#"<html><head><style>p { margin: 0 }</style></head><body>
<p>Plain <b>bold</b> and <span style="font-style:italic; color:#d61f1f">red italic</span></p>
<p><b style="font-weight:normal"><span style="font-size:18pt;font-family:'Avenir Next', sans-serif">Big</span></b> &amp; <u>under</u><br>x&nbsp;y <s>gone</s></p>
</body></html>"#;
        let d = Doc::from_html(html);
        assert_eq!(d.text, "Plain bold and red italic\nBig & under\nx y gone");
        let at = |t: &str| d.marks_at(d.text.find(t).unwrap());
        assert!(at("bold").bold && !at("Plain").bold);
        assert!(at("red").italic && at("red").color.is_some());
        assert_eq!(at("Big").size, Some(18.0));
        assert_eq!(at("Big").font.as_deref(), Some("Avenir Next"));
        assert!(!at("Big").bold);
        assert!(at("under").underline && at("gone").strike);
    }

    #[test]
    fn slices_and_splices() {
        let d = Doc::parse(r#"{"text":"one two three","runs":[{"start":4,"end":7,"bold":true}]}"#);
        let part = d.slice(2..9);
        assert_eq!(part.text, "e two t");
        assert_eq!(part.runs[0].0, 2..5);
        let mut e = Doc::parse(r#"{"text":"[]","runs":[]}"#);
        e.splice(1..1, &part);
        assert_eq!(e.text, "[e two t]");
        assert_eq!(e.runs[0].0, 3..6);
    }

    #[test]
    fn runs_change_and_move() {
        let mut d = Doc::parse(r#"{"text":"héllo world","runs":[{"start":0,"end":5,"bold":true}]}"#);
        assert_eq!(d.runs.len(), 1);
        assert_eq!(d.runs[0].0, 0..6);
        // Typing inside the bold word, in bold.
        d.replace(3..3, "XY", &d.marks_at(2));
        assert_eq!(d.text, "héXYllo world");
        assert_eq!(d.runs[0].0, 0..8);
        // Italic over the end of the bold word and the space.
        d.change(6..9, |m| m.italic = true);
        assert_eq!(d.runs.len(), 3);
        assert_eq!(Doc::parse(&d.to_json()), d);
        // Deleting across runs.
        d.replace(3..10, "", &Marks::default());
        assert_eq!(d.text, "héorld");
        assert!(d.runs.iter().all(|(r, _)| r.end <= d.text.len()));
        assert_eq!(changed("abcd", "abXcd"), (2, 2, 3));
        // To HTML and back, exactly.
        let back = Doc::from_html(&d.to_html());
        assert_eq!(back, d);
        assert_eq!(changed("aaa", "aa"), (2, 3, 2));
    }
}
