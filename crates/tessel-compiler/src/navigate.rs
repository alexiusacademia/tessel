//! Finding a name's declaration and its uses, for editors: go to
//! definition, find references, and rename.
//!
//! The type checker notes, as it resolves each name the program declares,
//! where that name is declared (`Checked::refs`). This reads those notes.

use crate::source::{SourceMap, Span, FILE_MASK};
use crate::Analysis;

/// A name the program declares, as the caret found it.
pub struct Symbol {
    pub name: String,
    /// The name in its declaration.
    pub declared: Span,
    /// Every place the name is written: its declaration and its uses, in
    /// the order of the files and of the text.
    pub places: Vec<Span>,
    /// Why it can't be renamed, if it can't.
    pub fixed: Option<&'static str>,
}

/// The name at byte `offset` of file `file` (on it, or right after it):
/// one the program declares, where it's used or where it's declared.
pub fn symbol_at(analysis: &Analysis, sources: &SourceMap, file: u32, offset: u32) -> Option<Symbol> {
    let checked = &analysis.checked;
    let here = |s: &Span| s.file & FILE_MASK == file && s.lo <= offset && offset <= s.hi;
    // Among what the caret touches, the shortest: `geometry.area` is a use
    // of `area`, and the caret may be on a name inside it.
    let used = checked.refs.iter().filter(|(at, _)| here(at)).min_by_key(|(at, _)| at.hi - at.lo).map(|(_, decl)| *decl);
    let declared = used.or_else(|| {
        let members = checked.structs.iter().flat_map(|s| s.fields.iter().map(|f| f.span).chain(s.methods.iter().map(|m| m.span)));
        let cases = checked.enums.iter().flat_map(|e| e.cases.iter().map(|c| c.span).chain(e.methods.iter().map(|m| m.span)));
        let known = checked.refs.iter().map(|(_, decl)| *decl);
        let all = known.chain(checked.declared.iter().copied()).chain(checked.locals.keys().copied()).chain(members).chain(cases);
        all.map(|s| Span { file: s.file & FILE_MASK, ..s }).find(|s| s.hi > s.lo && here(s))
    })?;
    let name = sources.snippet(declared).to_string();
    let mut fixed = None;
    let mut places = vec![declared];
    for (at, decl) in &checked.refs {
        if *decl != declared {
            continue;
        }
        // A use is written as the name, or ends with it (`geometry.area`).
        // (Where a generic declaration is used, the checker also notes the
        // types it's used with, at the whole use: those aren't the name.)
        let text = sources.snippet(*at);
        if text == name {
            places.push(*at);
        } else if text.ends_with(&name) && text[..text.len() - name.len()].ends_with('.') {
            places.push(Span { lo: at.hi - name.len() as u32, ..*at });
        }
    }
    places.sort_by_key(|s| (s.file, s.lo));
    places.dedup();

    // A member that an interface asks for by name is also the interface's,
    // and every other conforming type's.
    let member = checked.structs.iter().any(|s| s.fields.iter().any(|f| f.span == declared) || s.methods.iter().any(|m| m.span == declared))
        || checked.enums.iter().any(|e| e.methods.iter().any(|m| m.span == declared));
    if member && checked.ifaces.iter().any(|i| i.methods.iter().any(|(m, _)| *m == name) || i.props.iter().any(|(p, _)| *p == name)) {
        fixed = Some("an interface asks for a member of this name");
    }
    if name == "main" || !name.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        fixed = Some("this name can't be changed");
    }
    Some(Symbol { name, declared, places, fixed })
}

/// Whether `name` can be what something is called.
pub fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && crate::lexer::keyword(name).is_none()
}
