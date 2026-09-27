//! Source files and byte spans into them.

/// A byte range in one source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub file: u32,
    pub lo: u32,
    pub hi: u32,
}

impl Span {
    pub fn new(file: u32, lo: usize, hi: usize) -> Span {
        Span { file, lo: lo as u32, hi: hi as u32 }
    }

    /// The smallest span covering both `self` and `other` (same file).
    pub fn to(self, other: Span) -> Span {
        Span { file: self.file, lo: self.lo.min(other.lo), hi: self.hi.max(other.hi) }
    }

    /// An empty span at the end of `self`.
    pub fn end(self) -> Span {
        Span { file: self.file, lo: self.hi, hi: self.hi }
    }
}

pub struct SourceFile {
    pub name: String,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    fn new(name: String, text: String) -> SourceFile {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        SourceFile { name, text, line_starts }
    }

    /// Zero-based line index containing byte offset `pos`.
    pub fn line_index(&self, pos: usize) -> usize {
        match self.line_starts.binary_search(&pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    /// The text of zero-based line `index`, without its line ending.
    pub fn line_text(&self, index: usize) -> &str {
        let start = self.line_starts[index];
        let end = self.line_starts.get(index + 1).copied().unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    /// One-based (line, column) for byte offset `pos`; columns count characters.
    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let line = self.line_index(pos);
        let start = self.line_starts[line];
        let col = self.text[start..pos.min(self.text.len())].chars().count();
        (line + 1, col + 1)
    }
}

/// The bits of `Span::file` that say which file it is.
pub const FILE_MASK: u32 = 0xFFFF;

/// All source files of one program.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> u32 {
        self.files.push(SourceFile::new(name.into(), text.into()));
        (self.files.len() - 1) as u32
    }

    /// The file a span is in. (Names declared in a specialized copy of
    /// generic code carry a copy number in the high bits, so their spans
    /// differ from the original's; see `generics`.)
    pub fn file(&self, id: u32) -> &SourceFile {
        &self.files[(id & FILE_MASK) as usize]
    }

    pub fn files(&self) -> impl Iterator<Item = (u32, &SourceFile)> {
        self.files.iter().enumerate().map(|(i, f)| (i as u32, f))
    }

    pub fn snippet(&self, span: Span) -> &str {
        &self.file(span.file).text[span.lo as usize..span.hi as usize]
    }
}
