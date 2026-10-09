//! Syntax colors for Tessel code in `CodeEditor`.
//!
//! A small tokenizer (it doesn't need to be exact: the compiler is the judge
//! of what's valid). It must be fast enough to run on every keystroke.

use std::ops::Range;

use super::theme::theme;
use super::Rgba;

const KEYWORDS: &[&str] = &[
    "app", "view", "struct", "enum", "interface", "fn", "let", "var", "state", "bind", "if", "else", "for", "in", "match",
    "return", "try", "while", "break", "continue", "true", "false", "nil", "import", "public", "private", "async",
    "await", "static",
];

/// Colored ranges (byte offsets) for `text`. Text not covered is plain.
pub fn highlight(text: &str) -> Vec<(Range<usize>, Rgba)> {
    let b = text.as_bytes();
    let colors = &theme().code;
    let mut out = Vec::new();
    let mut i = 0;
    let ident = |c: u8| c == b'_' || c.is_ascii_alphanumeric();
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            out.push((start..i, colors.comment));
        } else if c == b'"' {
            i += 1;
            // Strings run to the closing quote or the end of the line; `{…}`
            // interpolations are colored as code.
            let mut seg = start;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == b'{' {
                    out.push((seg..i + 1, colors.string));
                    let inner_start = i + 1;
                    let mut depth = 0;
                    i += 1;
                    while i < b.len() && b[i] != b'\n' {
                        match b[i] {
                            b'{' => depth += 1,
                            b'}' if depth == 0 => break,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    for (r, color) in highlight(&text[inner_start..i]) {
                        out.push((r.start + inner_start..r.end + inner_start, color));
                    }
                    seg = i;
                    if i < b.len() && b[i] == b'}' {
                        i += 1;
                    }
                    continue;
                }
                i += 1;
            }
            if i < b.len() && b[i] == b'"' {
                i += 1;
            }
            out.push((seg..i.min(b.len()), colors.string));
        } else if c.is_ascii_digit() {
            // `0xff`, `0b1010`, `0o17`: letters and digits after the prefix.
            if c == b'0' && matches!(b.get(i + 1), Some(b'x' | b'b' | b'o')) {
                i += 2;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
            }
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_' || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))) {
                i += 1;
            }
            // An exponent: `1e6`, `2.5e-3`.
            if i < b.len() && matches!(b[i], b'e' | b'E') {
                let sign = usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
                if b.get(i + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    i += 1 + sign;
                    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
                        i += 1;
                    }
                }
            }
            out.push((start..i, colors.number));
        } else if ident(c) {
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
            let word = &text[start..i];
            let after_dot = start > 0 && b[start - 1] == b'.' && !(start > 1 && ident(b[start - 2]) || start > 1 && b[start - 2] == b')');
            let color = if KEYWORDS.contains(&word) {
                Some(colors.keyword)
            } else if c.is_ascii_uppercase() {
                Some(colors.type_name)
            } else if after_dot {
                Some(colors.case)
            } else if b.get(i) == Some(&b'(') || (start > 0 && b[start - 1] == b'.' && b.get(i) == Some(&b'(')) {
                Some(colors.call)
            } else {
                None
            };
            if let Some(color) = color {
                out.push((start..i, color));
            }
        } else {
            // Advance by a whole character (text may be non-ASCII).
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored(text: &str, color: Rgba) -> Vec<&str> {
        highlight(text).into_iter().filter(|(_, c)| *c == color).map(|(r, _)| &text[r]).collect()
    }

    #[test]
    fn colors_the_main_token_kinds() {
        let colors = &theme().code;
        let src = "app Counter {\n    state count = 0 // start\n    Text(\"Count: {count + 1}\").font(weight: .bold)\n}";
        assert_eq!(colored(src, colors.keyword), ["app", "state"]);
        assert_eq!(colored(src, colors.type_name), ["Counter", "Text"]);
        assert_eq!(colored(src, colors.number), ["0", "1"]);
        assert_eq!(colored(src, colors.comment), ["// start"]);
        assert_eq!(colored(src, colors.string), ["\"Count: {", "}\""]);
        assert_eq!(colored(src, colors.case), ["bold"]);
        assert_eq!(colored(src, colors.call), ["font"]);
    }

    #[test]
    fn colors_every_kind_of_number() {
        let colors = &theme().code;
        let src = "let a = 0xff + 0b1010 + 0o17 + 1_000 + 2.5e-3 + 1E9";
        assert_eq!(colored(src, colors.number), ["0xff", "0b1010", "0o17", "1_000", "2.5e-3", "1E9"]);
    }
}
