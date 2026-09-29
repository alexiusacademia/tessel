//! Turns source text into tokens.
//!
//! Statements end at newlines, so the lexer emits `Newline` tokens, except
//! while the innermost open bracket is `(` or `[` (a `{` turns them back on).

use crate::diag::{Diagnostic, Diagnostics};
use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(Vec<StrPiece>),

    // Keywords
    App,
    View,
    Struct,
    Enum,
    Interface,
    Fn,
    Let,
    Var,
    State,
    Bind,
    If,
    Else,
    For,
    In,
    Match,
    Return,
    Try,
    While,
    Break,
    Continue,
    True,
    False,
    Nil,
    Underscore,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    DotDot,
    Question,
    QuestionDot,
    QuestionQuestion,
    Arrow,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,

    Newline,
    Eof,
}

/// One part of a string literal: plain text, or an `{expression}` to interpolate.
#[derive(Clone, Debug, PartialEq)]
pub enum StrPiece {
    Text(String),
    /// Span of the expression source between the braces.
    Expr(Span),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "app" => Tok::App,
        "view" => Tok::View,
        "struct" => Tok::Struct,
        "enum" => Tok::Enum,
        "interface" => Tok::Interface,
        "fn" => Tok::Fn,
        "let" => Tok::Let,
        "var" => Tok::Var,
        "state" => Tok::State,
        "bind" => Tok::Bind,
        "if" => Tok::If,
        "else" => Tok::Else,
        "for" => Tok::For,
        "in" => Tok::In,
        "match" => Tok::Match,
        "return" => Tok::Return,
        "try" => Tok::Try,
        "while" => Tok::While,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "true" => Tok::True,
        "false" => Tok::False,
        "nil" => Tok::Nil,
        "_" => Tok::Underscore,
        _ => return None,
    })
}

/// Human-readable name of a token, for error messages.
pub fn describe(tok: &Tok) -> String {
    let s = match tok {
        Tok::Ident(name) => return format!("`{name}`"),
        Tok::Int(_) | Tok::Float(_) => "a number",
        Tok::Str(_) => "a string",
        Tok::Newline => "end of line",
        Tok::Eof => "end of file",
        Tok::App => "`app`",
        Tok::View => "`view`",
        Tok::Struct => "`struct`",
        Tok::Interface => "`interface`",
        Tok::Enum => "`enum`",
        Tok::Fn => "`fn`",
        Tok::Let => "`let`",
        Tok::Var => "`var`",
        Tok::State => "`state`",
        Tok::Bind => "`bind`",
        Tok::If => "`if`",
        Tok::Else => "`else`",
        Tok::For => "`for`",
        Tok::In => "`in`",
        Tok::Match => "`match`",
        Tok::Return => "`return`",
        Tok::Try => "`try`",
        Tok::While => "`while`",
        Tok::Break => "`break`",
        Tok::Continue => "`continue`",
        Tok::True => "`true`",
        Tok::False => "`false`",
        Tok::Nil => "`nil`",
        Tok::Underscore => "`_`",
        Tok::LParen => "`(`",
        Tok::RParen => "`)`",
        Tok::LBrace => "`{`",
        Tok::RBrace => "`}`",
        Tok::LBracket => "`[`",
        Tok::RBracket => "`]`",
        Tok::Comma => "`,`",
        Tok::Colon => "`:`",
        Tok::Dot => "`.`",
        Tok::DotDot => "`..`",
        Tok::Question => "`?`",
        Tok::QuestionDot => "`?.`",
        Tok::QuestionQuestion => "`??`",
        Tok::Arrow => "`->`",
        Tok::Eq => "`=`",
        Tok::PlusEq => "`+=`",
        Tok::MinusEq => "`-=`",
        Tok::StarEq => "`*=`",
        Tok::SlashEq => "`/=`",
        Tok::Plus => "`+`",
        Tok::Minus => "`-`",
        Tok::Star => "`*`",
        Tok::Slash => "`/`",
        Tok::Percent => "`%`",
        Tok::EqEq => "`==`",
        Tok::NotEq => "`!=`",
        Tok::Lt => "`<`",
        Tok::Le => "`<=`",
        Tok::Gt => "`>`",
        Tok::Ge => "`>=`",
        Tok::AndAnd => "`&&`",
        Tok::OrOr => "`||`",
        Tok::Bang => "`!`",
    };
    s.to_string()
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    file: u32,
    base: usize,
    tokens: Vec<Token>,
    brackets: Vec<u8>,
    diags: &'a mut Diagnostics,
}

/// Lexes `src`, which starts at byte offset `base` of file `file`.
pub fn lex(file: u32, src: &str, base: usize, diags: &mut Diagnostics) -> Vec<Token> {
    let mut lx = Lexer { src, bytes: src.as_bytes(), pos: 0, file, base, tokens: Vec::new(), brackets: Vec::new(), diags };
    lx.run();
    lx.tokens
}

impl Lexer<'_> {
    fn span(&self, lo: usize, hi: usize) -> Span {
        Span::new(self.file, self.base + lo, self.base + hi)
    }

    fn peek(&self, ahead: usize) -> u8 {
        self.bytes.get(self.pos + ahead).copied().unwrap_or(0)
    }

    fn push(&mut self, tok: Tok, lo: usize) {
        let span = self.span(lo, self.pos);
        self.tokens.push(Token { tok, span });
    }

    fn error(&mut self, lo: usize, hi: usize, msg: impl Into<String>) -> &mut Diagnostic {
        let span = self.span(lo, hi);
        self.diags.push(Diagnostic::error(span, msg));
        self.diags.list.last_mut().unwrap()
    }

    fn newlines_significant(&self) -> bool {
        !matches!(self.brackets.last(), Some(b'(' | b'['))
    }

    fn run(&mut self) {
        while self.pos < self.bytes.len() {
            let c = self.peek(0);
            let lo = self.pos;
            match c {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    let last_is_newline = matches!(self.tokens.last(), None | Some(Token { tok: Tok::Newline, .. }));
                    if self.newlines_significant() && !last_is_newline {
                        self.push(Tok::Newline, lo);
                    }
                }
                b'/' if self.peek(1) == b'/' => {
                    while self.pos < self.bytes.len() && self.peek(0) != b'\n' {
                        self.pos += 1;
                    }
                }
                b'"' => self.string(),
                b'0'..=b'9' => self.number(),
                c if c == b'_' || c.is_ascii_alphabetic() || c >= 0x80 => self.word(),
                _ => self.punct(),
            }
        }
        let end = self.bytes.len();
        self.tokens.push(Token { tok: Tok::Eof, span: self.span(end, end) });
    }

    fn word(&mut self) {
        let lo = self.pos;
        while let Some(ch) = self.src[self.pos..].chars().next() {
            if ch == '_' || ch.is_alphanumeric() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == lo {
            // A non-ASCII character that can't start a name.
            let ch = self.src[lo..].chars().next().unwrap();
            self.pos += ch.len_utf8();
            self.error(lo, self.pos, format!("unexpected character `{ch}`"));
            return;
        }
        let word = &self.src[lo..self.pos];
        let tok = keyword(word).unwrap_or_else(|| Tok::Ident(word.to_string()));
        self.push(tok, lo);
    }

    fn number(&mut self) {
        let lo = self.pos;
        let digits = |lx: &mut Self| {
            while lx.peek(0).is_ascii_digit() || lx.peek(0) == b'_' {
                lx.pos += 1;
            }
        };
        digits(self);
        // `pair.0.1` is two member accesses, not `pair.` and `0.1`.
        let member = lo >= 1 && self.src.as_bytes()[lo - 1] == b'.' && (lo < 2 || self.src.as_bytes()[lo - 2] != b'.');
        if member {
            let text = &self.src[lo..self.pos];
            self.push(Tok::Int(text.parse().unwrap_or(0)), lo);
            return;
        }
        let mut is_float = self.peek(0) == b'.' && self.peek(1).is_ascii_digit();
        if is_float {
            self.pos += 1;
            digits(self);
        }
        // An exponent: `1e9`, `6.02e23`, `1.5E-3`.
        if matches!(self.peek(0), b'e' | b'E') {
            let sign = usize::from(matches!(self.peek(1), b'+' | b'-'));
            if self.peek(1 + sign).is_ascii_digit() {
                self.pos += 1 + sign;
                digits(self);
                is_float = true;
            }
        }
        let text: String = self.src[lo..self.pos].chars().filter(|&c| c != '_').collect();
        if is_float {
            self.push(Tok::Float(text.parse().unwrap()), lo);
        } else {
            match text.parse::<i64>() {
                Ok(n) => self.push(Tok::Int(n), lo),
                Err(_) => {
                    self.error(lo, self.pos, "this number is too large for an `Int`").help(
                        "the largest `Int` is 9_223_372_036_854_775_807; write it as a `Float` (add `.0`) for bigger values",
                    );
                    self.push(Tok::Int(0), lo);
                }
            }
        }
    }

    fn string(&mut self) {
        let lo = self.pos;
        self.pos += 1; // opening quote
        let mut pieces = Vec::new();
        let mut text = String::new();
        loop {
            let c = self.peek(0);
            match c {
                0 | b'\n' if self.pos >= self.bytes.len() || c == b'\n' => {
                    self.error(lo, self.pos, "this string is missing its closing `\"`")
                        .label("string starts here")
                        .help("strings must end on the same line they start");
                    break;
                }
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    let esc_lo = self.pos;
                    self.pos += 1;
                    let e = self.peek(0);
                    let ch = match e {
                        b'n' => Some('\n'),
                        b'r' => Some('\r'),
                        b't' => Some('\t'),
                        b'"' => Some('"'),
                        b'\\' => Some('\\'),
                        b'{' => Some('{'),
                        b'}' => Some('}'),
                        _ => None,
                    };
                    match ch {
                        Some(ch) => {
                            text.push(ch);
                            self.pos += 1;
                        }
                        None => {
                            let len = self.src[self.pos..].chars().next().map_or(0, char::len_utf8);
                            self.pos += len;
                            self.error(esc_lo, self.pos, "unknown escape sequence")
                                .help("the escapes are \\n, \\r, \\t, \\\", \\\\ and \\{");
                        }
                    }
                }
                b'{' => {
                    if !text.is_empty() {
                        pieces.push(StrPiece::Text(std::mem::take(&mut text)));
                    }
                    let open = self.pos;
                    self.pos += 1;
                    match self.interpolation_end() {
                        Some(close) => {
                            let expr = self.span(open + 1, close);
                            if close == open + 1 || self.src[open + 1..close].trim().is_empty() {
                                self.error(open, close + 1, "empty `{}` in string").help(
                                    "put a value inside the braces, like \"{count}\", or write \\{ for a literal brace",
                                );
                            } else {
                                pieces.push(StrPiece::Expr(expr));
                            }
                            self.pos = close + 1;
                        }
                        None => {
                            self.error(open, open + 1, "this `{` in a string is never closed")
                                .help("close it with `}`, or write \\{ for a literal brace");
                            // Treat the rest of the string as text, so one
                            // missing brace doesn't turn the string into code.
                            self.pos = open + 1;
                            while self.pos < self.bytes.len() && !matches!(self.peek(0), b'"' | b'\n') {
                                if self.peek(0) == b'\\' {
                                    self.pos += 1;
                                }
                                self.pos += 1;
                            }
                            if self.peek(0) == b'"' {
                                self.pos += 1;
                            }
                            break;
                        }
                    }
                }
                _ => {
                    let ch = self.src[self.pos..].chars().next().unwrap();
                    text.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        if !text.is_empty() || pieces.is_empty() {
            pieces.push(StrPiece::Text(text));
        }
        self.push(Tok::Str(pieces), lo);
    }

    /// Finds the `}` that closes an interpolation starting at `self.pos`,
    /// skipping nested braces and strings. Stops at end of line.
    fn interpolation_end(&mut self) -> Option<usize> {
        let mut depth = 0;
        let mut i = self.pos;
        let mut in_str = false;
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'\n' => return None,
                b'\\' if in_str => i += 1,
                b'"' => in_str = !in_str,
                b'{' if !in_str => depth += 1,
                b'}' if !in_str => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            }
            i += 1;
        }
        self.pos = i;
        None
    }

    fn punct(&mut self) {
        let lo = self.pos;
        let (c0, c1) = (self.peek(0), self.peek(1));
        let (tok, len) = match (c0, c1) {
            (b'-', b'>') => (Tok::Arrow, 2),
            (b'.', b'.') => (Tok::DotDot, 2),
            (b'?', b'.') => (Tok::QuestionDot, 2),
            (b'?', b'?') => (Tok::QuestionQuestion, 2),
            (b'=', b'=') => (Tok::EqEq, 2),
            (b'!', b'=') => (Tok::NotEq, 2),
            (b'<', b'=') => (Tok::Le, 2),
            (b'>', b'=') => (Tok::Ge, 2),
            (b'&', b'&') => (Tok::AndAnd, 2),
            (b'|', b'|') => (Tok::OrOr, 2),
            (b'+', b'=') => (Tok::PlusEq, 2),
            (b'-', b'=') => (Tok::MinusEq, 2),
            (b'*', b'=') => (Tok::StarEq, 2),
            (b'/', b'=') => (Tok::SlashEq, 2),
            (b'(', _) => (Tok::LParen, 1),
            (b')', _) => (Tok::RParen, 1),
            (b'{', _) => (Tok::LBrace, 1),
            (b'}', _) => (Tok::RBrace, 1),
            (b'[', _) => (Tok::LBracket, 1),
            (b']', _) => (Tok::RBracket, 1),
            (b',', _) => (Tok::Comma, 1),
            (b':', _) => (Tok::Colon, 1),
            (b'.', _) => (Tok::Dot, 1),
            (b'?', _) => (Tok::Question, 1),
            (b'=', _) => (Tok::Eq, 1),
            (b'+', _) => (Tok::Plus, 1),
            (b'-', _) => (Tok::Minus, 1),
            (b'*', _) => (Tok::Star, 1),
            (b'/', _) => (Tok::Slash, 1),
            (b'%', _) => (Tok::Percent, 1),
            (b'<', _) => (Tok::Lt, 1),
            (b'>', _) => (Tok::Gt, 1),
            (b'!', _) => (Tok::Bang, 1),
            (b'&', _) => {
                self.pos += 1;
                self.error(lo, self.pos, "unexpected `&`").help("use `&&` for \"and\"");
                self.push(Tok::AndAnd, lo);
                return;
            }
            (b'|', _) => {
                self.pos += 1;
                self.error(lo, self.pos, "unexpected `|`").help("use `||` for \"or\"");
                self.push(Tok::OrOr, lo);
                return;
            }
            (b';', _) => {
                self.pos += 1;
                self.error(lo, self.pos, "Tessel doesn't use semicolons")
                    .help("remove the `;`; statements end at the end of the line");
                return;
            }
            _ => {
                let ch = self.src[lo..].chars().next().unwrap();
                self.pos += ch.len_utf8();
                self.error(lo, self.pos, format!("unexpected character `{ch}`"));
                return;
            }
        };
        self.pos += len;
        match tok {
            Tok::LParen => self.brackets.push(b'('),
            Tok::LBracket => self.brackets.push(b'['),
            Tok::LBrace => self.brackets.push(b'{'),
            Tok::RParen | Tok::RBracket | Tok::RBrace => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.push(tok, lo);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        let mut d = Diagnostics::default();
        let toks = lex(0, src, 0, &mut d);
        assert!(d.is_empty(), "{:?}", d.list);
        toks.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn newlines_are_ignored_inside_parens_but_not_braces() {
        let toks = kinds("f(\n a,\n b\n)\n{\n x\n}");
        let newlines = toks.iter().filter(|t| **t == Tok::Newline).count();
        // after `)`, after `{`, after `x`
        assert_eq!(newlines, 3);
    }

    #[test]
    fn range_is_not_a_float() {
        assert_eq!(kinds("0..10")[..3], [Tok::Int(0), Tok::DotDot, Tok::Int(10)]);
        assert_eq!(kinds("1.5")[0], Tok::Float(1.5));
        assert_eq!(kinds("1e3")[0], Tok::Float(1000.0));
        assert_eq!(kinds("6.02e23")[0], Tok::Float(6.02e23));
        assert_eq!(kinds("1.5E-3")[0], Tok::Float(1.5e-3));
        assert_eq!(kinds("2e+2")[0], Tok::Float(200.0));
        // `e` not followed by digits is a name: `2.each` isn't a number.
        assert_eq!(kinds("3e")[0], Tok::Int(3));
    }

    #[test]
    fn interpolation_pieces() {
        let src = r#""Count: {count}!""#;
        let Tok::Str(pieces) = &kinds(src)[0] else { panic!() };
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], StrPiece::Text("Count: ".into()));
        let StrPiece::Expr(span) = pieces[1] else { panic!() };
        assert_eq!(&src[span.lo as usize..span.hi as usize], "count");
        assert_eq!(pieces[2], StrPiece::Text("!".into()));
    }

    #[test]
    fn escaped_brace_is_text() {
        let Tok::Str(pieces) = &kinds(r#""a \{b}""#)[0] else { panic!() };
        assert_eq!(pieces, &[StrPiece::Text("a {b}".into())]);
    }
}
