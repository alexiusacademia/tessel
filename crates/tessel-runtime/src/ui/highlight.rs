//! Syntax colors for Tessel code in `CodeEditor`.
//!
//! A small tokenizer (it doesn't need to be exact: the compiler is the judge
//! of what's valid). It must be fast enough to run on every keystroke.

use std::ops::Range;

use super::Rgba;

pub const PLAIN: Rgba = Rgba([29, 29, 31, 255]);
const KEYWORD: Rgba = Rgba([155, 35, 147, 255]);
const STRING: Rgba = Rgba([196, 26, 22, 255]);
const NUMBER: Rgba = Rgba([28, 0, 207, 255]);
const COMMENT: Rgba = Rgba([93, 108, 121, 255]);
const TYPE: Rgba = Rgba([11, 79, 121, 255]);
const CALL: Rgba = Rgba([50, 109, 116, 255]);
const CASE: Rgba = Rgba([57, 0, 160, 255]);

const KEYWORDS: &[&str] = &[
    "app", "view", "struct", "enum", "interface", "fn", "let", "var", "state", "bind", "if", "else", "for", "in", "match",
    "return", "try", "while", "break", "continue", "true", "false", "nil",
];

/// Colored ranges (byte offsets) for `text`. Text not covered is plain.
pub fn highlight(text: &str) -> Vec<(Range<usize>, Rgba)> {
    let b = text.as_bytes();
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
            out.push((start..i, COMMENT));
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
                    out.push((seg..i + 1, STRING));
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
            out.push((seg..i.min(b.len()), STRING));
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_' || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))) {
                i += 1;
            }
            out.push((start..i, NUMBER));
        } else if ident(c) {
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
            let word = &text[start..i];
            let after_dot = start > 0 && b[start - 1] == b'.' && !(start > 1 && ident(b[start - 2]) || start > 1 && b[start - 2] == b')');
            let color = if KEYWORDS.contains(&word) {
                Some(KEYWORD)
            } else if c.is_ascii_uppercase() {
                Some(TYPE)
            } else if after_dot {
                Some(CASE)
            } else if b.get(i) == Some(&b'(') || (start > 0 && b[start - 1] == b'.' && b.get(i) == Some(&b'(')) {
                Some(CALL)
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
        let src = "app Counter {\n    state count = 0 // start\n    Text(\"Count: {count + 1}\").font(weight: .bold)\n}";
        assert_eq!(colored(src, KEYWORD), ["app", "state"]);
        assert_eq!(colored(src, TYPE), ["Counter", "Text"]);
        assert_eq!(colored(src, NUMBER), ["0", "1"]);
        assert_eq!(colored(src, COMMENT), ["// start"]);
        assert_eq!(colored(src, STRING), ["\"Count: {", "}\""]);
        assert_eq!(colored(src, CASE), ["bold"]);
        assert_eq!(colored(src, CALL), ["font"]);
    }
}
