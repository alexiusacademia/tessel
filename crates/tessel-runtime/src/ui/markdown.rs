//! `Markdown("…")`: text with headings, lists, code, tables, pictures and
//! links, laid out down the width it's given.
//!
//! The text is parsed (CommonMark, with tables) into blocks; laying them out
//! makes a list of things to draw, each at its place from the view's top
//! left. Layout is kept from one frame to the next, as the tree is rebuilt
//! and laid out all the time, and a page of documentation is a lot of text.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::edit::{self, Editing};
use super::image::{self, Picture};
use super::layout::{Rect, Style};
use super::render::{Paint, TextEngine};
use super::theme::{self, theme};
use super::Rgba;

/// How a stretch of text differs from the text around it.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Inline {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// A link: its number in the document.
    pub link: Option<usize>,
}

/// Text with stretches of other styles.
#[derive(Default)]
struct Rich {
    text: String,
    spans: Vec<(Range<usize>, Inline)>,
}

impl Rich {
    fn push(&mut self, text: &str, style: Inline) {
        let start = self.text.len();
        self.text.push_str(text);
        if style != Inline::default() {
            self.spans.push((start..self.text.len(), style));
        }
    }

    fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

enum Block {
    /// A paragraph, a heading (`heading` 1 to 6) or a list item (`marker`).
    Text { rich: Rich, heading: u8, marker: Option<String> },
    Code { text: String, tessel: bool },
    Image { path: String, alt: String },
    /// Its rows of cells; the first row is the head.
    Table(Vec<Vec<Rich>>),
    Rule,
}

/// A block, and where it is: how deep in lists, and whether in a quote.
struct Placed {
    block: Block,
    depth: usize,
    quoted: bool,
}

fn parse(source: &str) -> (Vec<Placed>, Vec<String>) {
    let mut blocks: Vec<Placed> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    // The text being gathered, and the styles it's inside of.
    let mut rich: Option<Rich> = None;
    let mut styles = vec![Inline::default()];
    // Open lists (the next number of a numbered one), and the marker of the
    // item just begun.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut marker: Option<String> = None;
    let mut quotes = 0usize;
    let mut code: Option<(String, bool)> = None;
    let mut picture: Option<(String, String)> = None;
    let mut table: Option<Vec<Vec<Rich>>> = None;
    let mut row: Vec<Rich> = Vec::new();

    for event in Parser::new_ext(source, Options::ENABLE_TABLES) {
        // The end of a paragraph, heading or list item: its text is a block.
        let mut flush = |rich: &mut Option<Rich>, marker: &mut Option<String>, heading: u8| {
            if let Some(r) = rich.take().filter(|r| !r.is_blank()) {
                let block = Block::Text { rich: r, heading, marker: marker.take() };
                blocks.push(Placed { block, depth: lists.len(), quoted: quotes > 0 });
            }
        };
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph | Tag::Heading { .. } => {
                    flush(&mut rich, &mut marker, 0);
                    rich = Some(Rich::default());
                }
                Tag::List(first) => {
                    flush(&mut rich, &mut marker, 0);
                    lists.push(first);
                }
                Tag::Item => {
                    marker = Some(match lists.last_mut() {
                        Some(Some(n)) => {
                            *n += 1;
                            format!("{}.", *n - 1)
                        }
                        _ => "•".into(),
                    });
                    rich = Some(Rich::default());
                }
                Tag::BlockQuote(_) => quotes += 1,
                Tag::CodeBlock(kind) => {
                    flush(&mut rich, &mut marker, 0);
                    let tessel = matches!(&kind, CodeBlockKind::Fenced(lang) if matches!(lang.as_ref(), "tessel" | "tsl"));
                    code = Some((String::new(), tessel));
                }
                Tag::Emphasis => styles.push(Inline { italic: true, ..*styles.last().unwrap() }),
                Tag::Strong => styles.push(Inline { bold: true, ..*styles.last().unwrap() }),
                Tag::Link { dest_url, .. } => {
                    links.push(dest_url.to_string());
                    styles.push(Inline { link: Some(links.len() - 1), ..*styles.last().unwrap() });
                }
                Tag::Image { dest_url, .. } => picture = Some((dest_url.to_string(), String::new())),
                Tag::Table(_) => {
                    flush(&mut rich, &mut marker, 0);
                    table = Some(Vec::new());
                }
                Tag::TableHead | Tag::TableRow => row = Vec::new(),
                Tag::TableCell => rich = Some(Rich::default()),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Item => flush(&mut rich, &mut marker, 0),
                TagEnd::Heading(level) => flush(&mut rich, &mut marker, level as u8),
                TagEnd::List(_) => {
                    lists.pop();
                }
                TagEnd::BlockQuote(_) => quotes = quotes.saturating_sub(1),
                TagEnd::CodeBlock => {
                    if let Some((text, tessel)) = code.take() {
                        let block = Block::Code { text: text.trim_end_matches('\n').to_string(), tessel };
                        blocks.push(Placed { block, depth: lists.len(), quoted: quotes > 0 });
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => {
                    styles.pop();
                }
                TagEnd::Image => {
                    if let Some((path, alt)) = picture.take() {
                        flush(&mut rich, &mut marker, 0);
                        blocks.push(Placed { block: Block::Image { path, alt }, depth: lists.len(), quoted: quotes > 0 });
                        rich = Some(Rich::default());
                    }
                }
                TagEnd::TableCell => row.push(rich.take().unwrap_or_default()),
                TagEnd::TableHead | TagEnd::TableRow => {
                    if let Some(rows) = table.as_mut() {
                        rows.push(std::mem::take(&mut row));
                    }
                }
                TagEnd::Table => {
                    if let Some(rows) = table.take() {
                        blocks.push(Placed { block: Block::Table(rows), depth: lists.len(), quoted: quotes > 0 });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((code, _)) = code.as_mut() {
                    code.push_str(&text);
                } else if let Some((_, alt)) = picture.as_mut() {
                    alt.push_str(&text);
                } else {
                    rich.get_or_insert_with(Rich::default).push(&text, *styles.last().unwrap());
                }
            }
            Event::Code(text) => {
                let style = Inline { code: true, ..*styles.last().unwrap() };
                rich.get_or_insert_with(Rich::default).push(&text, style);
            }
            Event::SoftBreak => rich.get_or_insert_with(Rich::default).push(" ", *styles.last().unwrap()),
            Event::HardBreak => rich.get_or_insert_with(Rich::default).push("\n", *styles.last().unwrap()),
            Event::Rule => {
                flush(&mut rich, &mut marker, 0);
                blocks.push(Placed { block: Block::Rule, depth: lists.len(), quoted: quotes > 0 });
            }
            // (HTML isn't shown.)
            _ => {}
        }
    }
    (blocks, links)
}

/// One thing to draw, at its place from the view's top left.
pub enum Item {
    Text { x: f64, y: f64, layout: parley::Layout<Paint> },
    Fill { rect: Rect, radius: f64, color: Rgba },
    Picture { rect: Rect, picture: Rc<Picture> },
}

impl Item {
    /// The rows of the view it's in: from `y` to `y + h`.
    pub fn rows(&self) -> (f64, f64) {
        match self {
            Item::Text { y, layout, .. } => (*y, *y + layout.height() as f64),
            Item::Fill { rect, .. } | Item::Picture { rect, .. } => (rect.y, rect.y + rect.h),
        }
    }
}

/// A link: where it is (a rectangle for each line it's on), what it says,
/// and where it goes.
pub struct Link {
    pub rects: Vec<Rect>,
    pub text: String,
    pub url: String,
}

/// Markdown laid out at a width.
pub struct Laid {
    pub height: f64,
    pub items: Vec<Item>,
    pub links: Vec<Link>,
    /// The first words, for the view tree that scripts print.
    pub title: String,
}

impl Laid {
    /// The link at a point (from the view's top left), if there's one.
    pub fn link_at(&self, x: f64, y: f64) -> Option<usize> {
        self.links.iter().position(|l| l.rects.iter().any(|r| r.contains(x, y)))
    }
}

/// What a layout depends on.
#[derive(PartialEq)]
struct Key {
    source: u64,
    width: u64,
    size: u64,
    color: [u8; 4],
    dark: bool,
}

thread_local! {
    /// The last few layouts (an app seldom shows more documents at once).
    static CACHE: RefCell<Vec<(Key, Rc<Laid>)>> = const { RefCell::new(Vec::new()) };
}

const LIST_INDENT: f64 = 22.0;
const QUOTE_INDENT: f64 = 14.0;
const CODE_PAD: f64 = 10.0;
const CELL_PAD: (f64, f64) = (8.0, 5.0);

/// Lays out `source` at `width`, in the text style of `style`. Pictures are
/// looked for in `folder`.
pub fn layout(text: &mut TextEngine, source: &str, folder: &str, width: f64, style: &Style) -> Rc<Laid> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (source, folder).hash(&mut h);
    let key =
        Key { source: h.finish(), width: width.to_bits(), size: style.size.to_bits(), color: style.color.0, dark: theme::is_dark() };
    if let Some(laid) = CACHE.with(|c| c.borrow().iter().find(|(k, _)| *k == key).map(|(_, l)| l.clone())) {
        return laid;
    }
    let laid = Rc::new(lay_out(text, source, folder, width, style));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 8 {
            c.remove(0);
        }
        c.push((key, laid.clone()));
    });
    laid
}

/// The rectangles of the bytes `range` of a laid-out text.
fn rects_of(layout: &parley::Layout<Paint>, range: &Range<usize>, x: f64, y: f64) -> Vec<Rect> {
    let mut span = Editing::new(0, "");
    (span.anchor, span.focus) = (range.start, range.end);
    edit::geometry(layout, &span).1.iter().map(|b| Rect { x: x + b.x0, y: y + b.y0, w: b.x1 - b.x0, h: b.y1 - b.y0 }).collect()
}

fn lay_out(text: &mut TextEngine, source: &str, folder: &str, width: f64, style: &Style) -> Laid {
    let (blocks, urls) = parse(source);
    let th = theme();
    let size = style.size;
    let mut items: Vec<Item> = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut y = 0.0f64;
    // The quote being laid out: where it began (the row, and the item its
    // background goes before).
    let mut quote: Option<(f64, usize)> = None;
    let close_quote = |items: &mut Vec<Item>, quote: &mut Option<(f64, usize)>, y: f64| {
        if let Some((top, at)) = quote.take() {
            let rect = Rect { x: 0.0, y: top, w: width, h: y - top + 8.0 };
            items.insert(at, Item::Fill { rect, radius: 6.0, color: th.colors[theme::SIDEBAR] });
            let bar = Rect { x: 0.0, y: top, w: 3.0, h: rect.h };
            items.insert(at + 1, Item::Fill { rect: bar, radius: 1.5, color: th.accent });
        }
    };
    let mut title = String::new();

    for (i, placed) in blocks.iter().enumerate() {
        // The space above: more before a heading, less between list items.
        let item = matches!(&placed.block, Block::Text { marker: Some(_), .. });
        let heading = match &placed.block {
            Block::Text { heading, .. } => *heading,
            _ => 0,
        };
        if !placed.quoted && quote.is_some() {
            close_quote(&mut items, &mut quote, y);
            y += 8.0;
        }
        if i > 0 {
            y += if heading > 0 { size * 1.4 } else if item { size * 0.35 } else { size * 0.8 };
        }
        if placed.quoted && quote.is_none() {
            quote = Some((y, items.len()));
            y += 8.0;
        }
        let x = placed.depth as f64 * LIST_INDENT + if placed.quoted { QUOTE_INDENT } else { 0.0 };
        let room = (width - x - if placed.quoted { QUOTE_INDENT } else { 0.0 }).max(40.0);

        match &placed.block {
            Block::Text { rich, heading, marker } => {
                let (scale, weight) = match heading {
                    0 => (1.0, style.weight),
                    1 => (1.75, 700.0),
                    2 => (1.4, 700.0),
                    3 => (1.15, 700.0),
                    _ => (1.0, 700.0),
                };
                if title.is_empty() {
                    title = rich.text.chars().take(40).collect();
                }
                let layout = text.rich(&rich.text, size * scale, weight, style.color, &rich.spans, Some(room));
                for (range, inline) in &rich.spans {
                    if let Some(url) = inline.link.and_then(|l| urls.get(l)) {
                        links.push(Link { rects: rects_of(&layout, range, x, y), text: rich.text[range.clone()].to_string(), url: url.clone() });
                    }
                }
                if let Some(marker) = marker {
                    let m = text.rich(marker, size, style.weight, th.colors[theme::SECONDARY], &[], None);
                    let mx = x - 8.0 - m.width() as f64;
                    items.push(Item::Text { x: mx.max(0.0), y, layout: m });
                }
                let height = layout.height() as f64;
                items.push(Item::Text { x, y, layout });
                y += height;
                // A line under the two largest headings.
                if matches!(heading, 1 | 2) {
                    y += size * 0.3;
                    items.push(Item::Fill { rect: Rect { x, y, w: room, h: 1.0 }, radius: 0.0, color: th.colors[theme::SEPARATOR] });
                    y += 1.0;
                }
            }
            Block::Code { text: code, tessel } => {
                let inner = (room - 2.0 * CODE_PAD).max(40.0);
                let layout = text.code_block(code, size * 0.9, *tessel, inner);
                let height = layout.height() as f64 + 2.0 * CODE_PAD;
                items.push(Item::Fill { rect: Rect { x, y, w: room, h: height }, radius: 6.0, color: th.gutter });
                items.push(Item::Text { x: x + CODE_PAD, y: y + CODE_PAD, layout });
                y += height;
            }
            Block::Image { path, alt } => {
                let file = if folder.is_empty() || path.starts_with('/') { path.clone() } else { format!("{folder}/{path}") };
                match image::load(&file) {
                    Some(picture) => {
                        let scale = (room / picture.width.max(1.0)).min(1.0);
                        let rect = Rect { x, y, w: picture.width * scale, h: picture.height * scale };
                        items.push(Item::Picture { rect, picture });
                        y += rect.h;
                    }
                    // Without the picture, what it shows (in words).
                    None => {
                        let words = if alt.is_empty() { path } else { alt };
                        let italic = [(0..words.len(), Inline { italic: true, ..Inline::default() })];
                        let layout = text.rich(words, size, style.weight, th.colors[theme::SECONDARY], &italic, Some(room));
                        y += layout.height() as f64;
                        let top = y - layout.height() as f64;
                        items.push(Item::Text { x, y: top, layout });
                    }
                }
            }
            Block::Table(rows) => {
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                if columns > 0 {
                    // Each column as wide as its widest cell, made narrower
                    // in proportion when the table doesn't fit.
                    let mut widths = vec![0.0f64; columns];
                    for (r, row) in rows.iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            let weight = if r == 0 { 600.0 } else { style.weight };
                            let w = text.rich(&cell.text, size * 0.95, weight, style.color, &cell.spans, None).width() as f64;
                            widths[c] = widths[c].max(w + 1.0);
                        }
                    }
                    let padding = columns as f64 * 2.0 * CELL_PAD.0;
                    let total: f64 = widths.iter().sum();
                    if total + padding > room {
                        let scale = ((room - padding) / total).max(0.05);
                        widths.iter_mut().for_each(|w| *w = (*w * scale).max(36.0));
                    }
                    let table_w = (widths.iter().sum::<f64>() + padding).min(room.max(1.0)).max(1.0);
                    let line = th.colors[theme::SEPARATOR];
                    for (r, row) in rows.iter().enumerate() {
                        let weight = if r == 0 { 600.0 } else { style.weight };
                        let mut cx = x;
                        let mut height = 0.0f64;
                        let top = y + CELL_PAD.1;
                        for (c, cell) in row.iter().enumerate() {
                            let layout = text.rich(&cell.text, size * 0.95, weight, style.color, &cell.spans, Some(widths[c]));
                            for (range, inline) in &cell.spans {
                                if let Some(url) = inline.link.and_then(|l| urls.get(l)) {
                                    let rects = rects_of(&layout, range, cx + CELL_PAD.0, top);
                                    links.push(Link { rects, text: cell.text[range.clone()].to_string(), url: url.clone() });
                                }
                            }
                            height = height.max(layout.height() as f64);
                            items.push(Item::Text { x: cx + CELL_PAD.0, y: top, layout });
                            cx += widths[c] + 2.0 * CELL_PAD.0;
                        }
                        y += height + 2.0 * CELL_PAD.1;
                        let h = if r == 0 { 1.5 } else { 1.0 };
                        items.push(Item::Fill { rect: Rect { x, y: y - h, w: table_w, h }, radius: 0.0, color: line });
                    }
                }
            }
            Block::Rule => {
                items.push(Item::Fill { rect: Rect { x, y, w: room, h: 1.0 }, radius: 0.0, color: th.colors[theme::SEPARATOR] });
                y += 1.0;
            }
        }
    }
    close_quote(&mut items, &mut quote, y);
    if blocks.last().is_some_and(|b| b.quoted) {
        y += 8.0;
    }
    Laid { height: y, items, links, title }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "# Title\n\nSome *text* with `code` and a [link](/there/).\n\n- one\n- two\n  1. nested\n\n\
        ```tessel\nlet x = 1\n```\n\n> **Note**\n>\n> Quoted.\n\n| A | B |\n|---|---|\n| 1 | [two](https://example.com) |\n\n---\n\n![a picture](missing.png)\n";

    #[test]
    fn parses_blocks_and_inlines() {
        let (blocks, links) = parse(PAGE);
        let kinds: Vec<String> = blocks
            .iter()
            .map(|p| match &p.block {
                Block::Text { rich, heading, marker } => {
                    format!("{}{}text {:?}", "#".repeat(*heading as usize), marker.clone().unwrap_or_default(), rich.text)
                }
                Block::Code { text, tessel } => format!("code {tessel} {text:?}"),
                Block::Image { path, alt } => format!("image {path} {alt:?}"),
                Block::Table(rows) => format!("table {}x{}", rows.len(), rows[0].len()),
                Block::Rule => "rule".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "#text \"Title\"",
                "text \"Some text with code and a link.\"",
                "•text \"one\"",
                "•text \"two\"",
                "1.text \"nested\"",
                "code true \"let x = 1\"",
                "text \"Note\"",
                "text \"Quoted.\"",
                "table 2x2",
                "rule",
                "image missing.png \"a picture\"",
            ]
        );
        assert_eq!(links, ["/there/", "https://example.com"]);
        assert_eq!(blocks.iter().map(|p| p.depth).collect::<Vec<_>>(), [0, 0, 1, 1, 2, 0, 0, 0, 0, 0, 0]);
        assert_eq!(blocks.iter().filter(|p| p.quoted).count(), 2);
        // The styles of the paragraph: italic, code, link.
        let Block::Text { rich, .. } = &blocks[1].block else { panic!() };
        let styled: Vec<(&str, Inline)> = rich.spans.iter().map(|(r, s)| (&rich.text[r.clone()], *s)).collect();
        assert_eq!(styled.len(), 3);
        assert!(styled[0] == ("text", Inline { italic: true, ..Inline::default() }));
        assert!(styled[1].0 == "code" && styled[1].1.code);
        assert!(styled[2].0 == "link" && styled[2].1.link == Some(0));
    }

    #[test]
    fn lays_out_down_the_page() {
        let mut text = TextEngine::new();
        let style = Style::default();
        let laid = layout(&mut text, PAGE, "", 400.0, &style);
        assert!(laid.height > 200.0, "{}", laid.height);
        assert_eq!(laid.title, "Title");
        // Everything is inside the width, and in order down the page.
        let mut last = 0.0;
        for item in &laid.items {
            let (top, bottom) = item.rows();
            assert!(bottom >= top && top >= 0.0 && bottom <= laid.height + 0.5);
            if let Item::Text { y, .. } = item {
                assert!(*y + 0.5 >= last, "{y} after {last}");
                last = *y;
            }
        }
        // The links can be found where they are.
        assert_eq!(laid.links.iter().map(|l| (l.text.as_str(), l.url.as_str())).collect::<Vec<_>>(), [
            ("link", "/there/"),
            ("two", "https://example.com")
        ]);
        let r = laid.links[0].rects[0];
        assert_eq!(laid.link_at(r.x + r.w / 2.0, r.y + r.h / 2.0), Some(0));
        assert_eq!(laid.link_at(399.0, 1.0), None);
        // Narrower is taller; and the same again comes from the cache.
        let narrow = layout(&mut text, PAGE, "", 160.0, &style);
        assert!(narrow.height > laid.height);
        assert!(Rc::ptr_eq(&laid, &layout(&mut text, PAGE, "", 400.0, &style)));
    }
}
