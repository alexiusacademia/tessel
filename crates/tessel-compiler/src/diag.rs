//! Diagnostics: error messages that point at source code and suggest a fix.

use crate::source::{SourceMap, Span};

#[derive(Clone, Debug)]
pub struct Label {
    pub span: Span,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            message: message.into(),
            primary: Label { span, text: String::new() },
            secondary: Vec::new(),
            help: None,
        }
    }

    /// Text shown under the primary span's carets.
    pub fn label(&mut self, text: impl Into<String>) -> &mut Diagnostic {
        self.primary.text = text.into();
        self
    }

    /// An extra location, e.g. where a name was declared.
    pub fn note_at(&mut self, span: Span, text: impl Into<String>) -> &mut Diagnostic {
        self.secondary.push(Label { span, text: text.into() });
        self
    }

    /// `help`, for a diagnostic being built by value.
    pub fn with_help(mut self, text: impl Into<String>) -> Diagnostic {
        self.help(text);
        self
    }

    pub fn help(&mut self, text: impl Into<String>) -> &mut Diagnostic {
        self.help = Some(text.into());
        self
    }

    pub fn span(&self) -> Span {
        self.primary.span
    }
}

/// Collects diagnostics while compiling.
#[derive(Default)]
pub struct Diagnostics {
    pub list: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(&mut self, d: Diagnostic) {
        self.list.push(d);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

struct Style {
    error: &'static str,
    bold: &'static str,
    blue: &'static str,
    reset: &'static str,
}

const COLOR: Style = Style { error: "\x1b[1;31m", bold: "\x1b[1m", blue: "\x1b[1;34m", reset: "\x1b[0m" };
const PLAIN: Style = Style { error: "", bold: "", blue: "", reset: "" };

/// Renders a diagnostic in the familiar compiler style:
///
/// ```text
/// error: cannot find `cnt`
///  --> app.tsl:5:23
///   |
/// 5 |         Text("Count: {cnt}")
///   |                       ^^^ not found
///   |
///   = help: did you mean `count`?
/// ```
pub fn render(d: &Diagnostic, sources: &SourceMap, color: bool) -> String {
    let s = if color { &COLOR } else { &PLAIN };
    let mut labels: Vec<(&Label, bool)> = vec![(&d.primary, true)];
    labels.extend(d.secondary.iter().map(|l| (l, false)));

    let gutter = labels
        .iter()
        .map(|(l, _)| sources.file(l.span.file).line_col(l.span.lo as usize).0.to_string().len())
        .max()
        .unwrap_or(1);
    let pad = " ".repeat(gutter);

    let mut out = format!("{}error{}{}: {}{}\n", s.error, s.reset, s.bold, d.message, s.reset);
    for (label, primary) in labels {
        let file = sources.file(label.span.file);
        let (line, col) = file.line_col(label.span.lo as usize);
        let text = file.line_text(line - 1);
        let arrow = if primary { "-->" } else { ":::" };
        out += &format!("{pad}{}{arrow}{} {}:{line}:{col}\n", s.blue, s.reset, file.name);
        out += &format!("{pad} {}|{}\n", s.blue, s.reset);
        out += &format!("{}{line:>gutter$} |{} {}\n", s.blue, s.reset, text.replace('\t', " "));

        // Underline to the end of the span, or the end of the line if it spans several.
        let start = col - 1;
        let line_chars = text.chars().count();
        let span_chars = sources.snippet(label.span).chars().take_while(|&c| c != '\n').count();
        let width = span_chars.min(line_chars.saturating_sub(start)).max(1);
        let (mark, mark_color) = if primary { ('^', s.error) } else { ('-', s.blue) };
        out += &format!(
            "{pad} {}|{} {}{mark_color}{}{}{}\n",
            s.blue,
            s.reset,
            " ".repeat(start),
            mark.to_string().repeat(width),
            if label.text.is_empty() { String::new() } else { format!(" {}", label.text) },
            s.reset,
        );
    }
    if let Some(help) = &d.help {
        out += &format!("{pad} {}|{}\n", s.blue, s.reset);
        out += &format!("{pad} {}={} {}help{}: {help}\n", s.blue, s.reset, s.bold, s.reset);
    }
    out
}

/// Edit distance where swapping two adjacent characters counts as one edit
/// (optimal string alignment), used to suggest names for typos.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The closest candidate to `name`, if it is close enough to be a likely typo.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .filter(|c| *c != name)
        .map(|c| (edit_distance(&name.to_lowercase(), &c.to_lowercase()), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swapped_letters_are_one_edit() {
        assert_eq!(edit_distance("cuont", "count"), 1);
        assert_eq!(suggest("cuont", ["count", "draft"]), Some("count"));
        assert_eq!(suggest("xyz", ["count"]), None);
    }
}
