//! Menus: the items of context menus and of the menu bar, read from the
//! view tree, with the standard menus Tessel adds to the menu bar; and the
//! drawn menus' layout (the menu bar on macOS is the system's: see `native`).

use super::layout::Rect;
use super::render::{Paint, TextEngine};
use super::theme::{self, theme};
use super::{Callback, Kind, Node};

/// A menu item's keys: `cmd` is the platform's shortcut key (Cmd on macOS,
/// Ctrl elsewhere).
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    /// A lowercase character, or a key's name (`enter`, `left`, …).
    pub key: String,
}

/// Keys that have names (as `key` script commands and `.shortcut` name them).
pub const NAMED_KEYS: &[&str] =
    &["enter", "escape", "tab", "backspace", "delete", "left", "right", "up", "down", "home", "end", "space"];

impl Shortcut {
    /// Reads `cmd+shift+s`, `shift+cmd+s` or `⇧⌘S` (`alt`, `option` and `⌥`
    /// for Alt). `None` if it isn't one.
    pub fn parse(text: &str) -> Option<Shortcut> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut s = Shortcut { cmd: false, shift: false, alt: false, key: String::new() };
        // Symbols: modifiers, then the key.
        let mut rest = text;
        loop {
            let c = rest.chars().next()?;
            let flag = match c {
                '⌘' => &mut s.cmd,
                '⇧' => &mut s.shift,
                '⌥' => &mut s.alt,
                _ => break,
            };
            *flag = true;
            rest = &rest[c.len_utf8()..];
        }
        // Words: `cmd+shift+s` (a `+` key is written last: `cmd++`).
        let mut parts: Vec<&str> = rest.split('+').collect();
        if rest.ends_with("++") {
            parts.truncate(parts.len() - 2);
            parts.push("+");
        }
        let key = parts.pop()?.trim();
        for m in parts {
            let flag = match m.trim().to_lowercase().as_str() {
                "cmd" | "command" => &mut s.cmd,
                "shift" => &mut s.shift,
                "alt" | "option" | "opt" => &mut s.alt,
                _ => return None,
            };
            *flag = true;
        }
        let lower = key.to_lowercase();
        let mut chars = key.chars();
        s.key = match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_whitespace() => c.to_lowercase().collect(),
            _ if NAMED_KEYS.contains(&lower.as_str()) => lower,
            _ => return None,
        };
        Some(s)
    }

    /// As `parse` reads it in words: `cmd+shift+s`.
    pub fn normalized(&self) -> String {
        let mut out = String::new();
        for (on, name) in [(self.cmd, "cmd+"), (self.shift, "shift+"), (self.alt, "alt+")] {
            if on {
                out.push_str(name);
            }
        }
        out + &self.key
    }

    /// As menus show it: `⇧⌘S` on macOS, `Ctrl+Shift+S` elsewhere.
    pub fn display(&self) -> String {
        let key = match self.key.as_str() {
            "enter" => "↩".to_string(),
            "escape" => "⎋".to_string(),
            "tab" => "⇥".to_string(),
            "backspace" => "⌫".to_string(),
            "delete" => "⌦".to_string(),
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            "home" => "↖".to_string(),
            "end" => "↘".to_string(),
            "space" => "Space".to_string(),
            k => k.to_uppercase(),
        };
        if cfg!(target_os = "macos") {
            let mut out = String::new();
            for (on, sym) in [(self.alt, "⌥"), (self.shift, "⇧"), (self.cmd, "⌘")] {
                if on {
                    out.push_str(sym);
                }
            }
            out + &key
        } else {
            let mut out = String::new();
            for (on, name) in [(self.cmd, "Ctrl+"), (self.shift, "Shift+"), (self.alt, "Alt+")] {
                if on {
                    out.push_str(name);
                }
            }
            out + &key
        }
    }

    pub fn matches(&self, key: &str, cmd: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.cmd == cmd && self.shift == shift && self.alt == alt
    }

    /// Cmd+A, C, X, V, Z (and Shift+Cmd+Z): while text is edited, these edit it.
    pub fn is_editing(&self) -> bool {
        self.cmd && !self.alt && matches!(self.key.as_str(), "a" | "c" | "x" | "v" | "z")
    }
}

/// The commands of the standard menus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Std {
    About,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Zoom,
    CloseWindow,
}

#[derive(Clone)]
pub enum Command {
    User(Callback),
    Std(Std),
}

#[derive(Clone)]
pub enum Entry {
    Item { label: String, shortcut: Option<Shortcut>, enabled: bool, checked: bool, command: Command },
    Divider,
    Sub { title: String, enabled: bool, items: Vec<Entry> },
}

impl Entry {
    fn std(label: &str, shortcut: &str, std: Std) -> Entry {
        Entry::Item { label: label.into(), shortcut: Shortcut::parse(shortcut), enabled: true, checked: false, command: Command::Std(std) }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Entry::Item { enabled, .. } | Entry::Sub { enabled, .. } => *enabled,
            Entry::Divider => false,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Entry::Item { label, .. } => label,
            Entry::Sub { title, .. } => title,
            Entry::Divider => "",
        }
    }
}

/// The items in a menu's node (a `MenuList` or a `Menu`): custom views'
/// items are in line with the others; hidden ones are left out, and the
/// items of a disabled view are disabled.
pub fn entries(list: &Node) -> Vec<Entry> {
    fn visit(n: &Node, enabled: bool, out: &mut Vec<Entry>) {
        if n.mods.hidden {
            return;
        }
        let enabled = enabled && !n.mods.disabled;
        match &n.kind {
            Kind::MenuItem { label, shortcut, checked, action } => out.push(Entry::Item {
                label: label.clone(),
                shortcut: Shortcut::parse(shortcut),
                enabled,
                checked: *checked,
                command: Command::User(action.clone()),
            }),
            Kind::MenuDivider => out.push(Entry::Divider),
            Kind::Menu { title } => {
                let mut items = Vec::new();
                for c in &n.children {
                    visit(c, enabled, &mut items);
                }
                out.push(Entry::Sub { title: title.clone(), enabled, items });
            }
            _ => {
                for c in &n.children {
                    visit(c, enabled, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for c in &list.children {
        visit(c, true, &mut out);
    }
    out
}

/// The menu bar: the app's menus (`user`, a `MenuList`) with the standard
/// ones. `native`: as macOS has it (an app menu, Edit and Window); drawn
/// in the window: the app's menus with Edit, and Quit at the end of the
/// first menu. `shortcut_keys`: the keys of the views' `.shortcut`s, which
/// standard items give up too.
pub fn bar(user: Option<&Node>, app_name: &str, native: bool, shortcut_keys: &[String]) -> Vec<Entry> {
    let mut menus: Vec<Entry> = user.map(entries).unwrap_or_default();
    // (Items outside a `Menu` have no place in a menu bar; the checker
    // doesn't let them be put there.)
    menus.retain(|m| matches!(m, Entry::Sub { .. }));
    // The app's own items keep their keys: a standard item with the same
    // keys has none.
    fn keys(items: &[Entry], out: &mut Vec<Shortcut>) {
        for e in items {
            match e {
                Entry::Item { shortcut: Some(s), .. } => out.push(s.clone()),
                Entry::Sub { items, .. } => keys(items, out),
                _ => {}
            }
        }
    }
    let mut taken = Vec::new();
    keys(&menus, &mut taken);
    let std = |label: &str, shortcut: &str, std: Std| {
        let mut e = Entry::std(label, shortcut, std);
        if let Entry::Item { shortcut: s @ Some(_), .. } = &mut e {
            let keys = s.as_ref().unwrap();
            if taken.contains(keys) || (keys.cmd && !keys.alt && shortcut_keys.contains(&keys.key)) {
                *s = None;
            }
        }
        e
    };
    let edit = vec![
        std("Undo", "cmd+z", Std::Undo),
        std("Redo", "cmd+shift+z", Std::Redo),
        Entry::Divider,
        std("Cut", "cmd+x", Std::Cut),
        std("Copy", "cmd+c", Std::Copy),
        std("Paste", "cmd+v", Std::Paste),
        std("Select All", "cmd+a", Std::SelectAll),
    ];
    // A standard menu the app has too: the standard items first.
    let merge = |menus: &mut Vec<Entry>, title: &str, standard: Vec<Entry>, at: usize| {
        match menus.iter_mut().find(|m| m.label() == title) {
            Some(Entry::Sub { items, .. }) => {
                let mut all = standard;
                if !items.is_empty() {
                    all.push(Entry::Divider);
                }
                all.append(items);
                *items = all;
            }
            _ => {
                let at = at.min(menus.len());
                menus.insert(at, Entry::Sub { title: title.into(), enabled: true, items: standard });
            }
        }
    };
    if user.is_some() || native {
        merge(&mut menus, "Edit", edit, 1);
    }
    if native {
        let window = vec![
            std("Minimize", "cmd+m", Std::Minimize),
            std("Zoom", "", Std::Zoom),
            Entry::Divider,
            std("Close Window", "cmd+w", Std::CloseWindow),
        ];
        let help = menus.iter().position(|m| m.label() == "Help").unwrap_or(menus.len());
        merge(&mut menus, "Window", window, help);
        let app = vec![
            std(&format!("About {app_name}"), "", Std::About),
            Entry::Divider,
            std(&format!("Hide {app_name}"), "cmd+h", Std::Hide),
            std("Hide Others", "cmd+alt+h", Std::HideOthers),
            std("Show All", "", Std::ShowAll),
            Entry::Divider,
            std(&format!("Quit {app_name}"), "cmd+q", Std::Quit),
        ];
        menus.insert(0, Entry::Sub { title: app_name.into(), enabled: true, items: app });
    } else if let Some(Entry::Sub { items, .. }) = menus.first_mut() {
        if !items.is_empty() {
            items.push(Entry::Divider);
        }
        items.push(std("Quit", "cmd+q", Std::Quit));
    }
    menus
}

/// The enabled item with a shortcut, anywhere in `menus`.
pub fn find_shortcut<'m>(menus: &'m [Entry], key: &str, cmd: bool, shift: bool, alt: bool) -> Option<&'m Entry> {
    menus.iter().find_map(|e| match e {
        Entry::Item { shortcut: Some(s), enabled: true, .. } if s.matches(key, cmd, shift, alt) => Some(e),
        Entry::Sub { items, enabled: true, .. } => find_shortcut(items, key, cmd, shift, alt),
        _ => None,
    })
}

/// The entry at `path` (indices, menu by menu).
pub fn at<'m>(menus: &'m [Entry], path: &[usize]) -> Option<&'m Entry> {
    let (first, rest) = path.split_first()?;
    let e = menus.get(*first)?;
    match (rest.is_empty(), e) {
        (true, _) => Some(e),
        (false, Entry::Sub { items, .. }) => at(items, rest),
        _ => None,
    }
}

/// The items shown at a level: of the menu at `path`.
pub fn items_at<'m>(menus: &'m [Entry], path: &[usize]) -> Option<&'m [Entry]> {
    if path.is_empty() {
        return Some(menus);
    }
    match at(menus, path)? {
        Entry::Sub { items, .. } => Some(items),
        _ => None,
    }
}

/// Lists menus as `tree` and `menus` print them.
pub fn describe(items: &[Entry], depth: usize, out: &mut String) {
    for e in items {
        let pad = "  ".repeat(depth);
        match e {
            Entry::Item { label, shortcut, enabled, checked, .. } => {
                let keys = shortcut.as_ref().map(|s| format!(" {}", s.normalized())).unwrap_or_default();
                let flags = format!("{}{}", if *checked { " checked" } else { "" }, if *enabled { "" } else { " disabled" });
                out.push_str(&format!("{pad}MenuItem {label:?}{keys}{flags}\n"));
            }
            Entry::Divider => out.push_str(&format!("{pad}MenuDivider\n")),
            Entry::Sub { title, enabled, items } => {
                out.push_str(&format!("{pad}Menu {title:?}{}\n", if *enabled { "" } else { " disabled" }));
                describe(items, depth + 1, out);
            }
        }
    }
}

/// Lists menu items on one line, a submenu's in brackets after it.
pub fn describe_inline(items: &[Entry]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|e| match e {
            Entry::Sub { title, enabled, items } => {
                format!("Menu {title:?}{} [{}]", if *enabled { "" } else { " disabled" }, describe_inline(items))
            }
            e => {
                let mut line = String::new();
                describe(std::slice::from_ref(e), 0, &mut line);
                line.trim_end().to_string()
            }
        })
        .collect();
    parts.join(", ")
}

// ===== drawn menus =====

pub const ROW_H: f64 = 22.0;
pub const DIVIDER_H: f64 = 9.0;
pub const PAD: f64 = 5.0;
/// The height of the menu bar drawn in the window.
pub const BAR_H: f64 = 26.0;

/// One menu shown (a context menu, a menu of the drawn menu bar, or one of
/// their submenus), laid out.
pub struct Panel {
    pub rect: Rect,
    pub rows: Vec<Row>,
    /// The row under the pointer or chosen with the keys.
    pub selected: Option<usize>,
}

pub struct Row {
    pub rect: Rect,
    /// `None` for a divider.
    pub label: Option<parley::Layout<Paint>>,
    pub shortcut: Option<parley::Layout<Paint>>,
    pub enabled: bool,
    pub checked: bool,
    pub submenu: bool,
}

impl Panel {
    /// Lays out `items` with its top left at `at`, inside a `w` × `h` window
    /// (moved left or up to stay inside it). `beside`: for a submenu, the
    /// left edge of the menu it opens from, to go to that side instead when
    /// there's no room on the right.
    pub fn layout(text: &mut TextEngine, items: &[Entry], at: (f64, f64), window: (f64, f64), beside: Option<f64>) -> Panel {
        let color = theme().colors[theme::PRIMARY];
        let gray = theme().colors[theme::GRAY];
        let mut rows = Vec::new();
        let mut widest: f64 = 0.0;
        let mut y = PAD;
        for e in items {
            match e {
                Entry::Divider => {
                    rows.push(Row { rect: Rect { x: 0.0, y, w: 0.0, h: DIVIDER_H }, label: None, shortcut: None, enabled: false, checked: false, submenu: false });
                    y += DIVIDER_H;
                }
                Entry::Item { label, shortcut, enabled, checked, .. } => {
                    let tint = if *enabled { color } else { gray };
                    let label = text.layout(label, 13.0, 400.0, tint, None);
                    let shortcut = shortcut.as_ref().map(|s| text.layout(&s.display(), 13.0, 400.0, gray, None));
                    widest = widest.max(label.width() as f64 + shortcut.as_ref().map_or(0.0, |s| s.width() as f64 + 28.0));
                    rows.push(Row { rect: Rect { x: 0.0, y, w: 0.0, h: ROW_H }, label: Some(label), shortcut, enabled: *enabled, checked: *checked, submenu: false });
                    y += ROW_H;
                }
                Entry::Sub { title, enabled, .. } => {
                    let tint = if *enabled { color } else { gray };
                    let label = text.layout(title, 13.0, 400.0, tint, None);
                    widest = widest.max(label.width() as f64 + 24.0);
                    rows.push(Row { rect: Rect { x: 0.0, y, w: 0.0, h: ROW_H }, label: Some(label), shortcut: None, enabled: *enabled, checked: false, submenu: true });
                    y += ROW_H;
                }
            }
        }
        let w = (widest + 2.0 * 22.0).max(160.0).min((window.0 - 8.0).max(60.0));
        let h = y + PAD;
        let (mut x, mut top) = at;
        if x + w > window.0 - 4.0 {
            x = match beside {
                Some(left) if left - w + 4.0 >= 4.0 => left - w + 4.0,
                _ => window.0 - w - 4.0,
            };
        }
        if top + h > window.1 - 4.0 {
            top = window.1 - h - 4.0;
        }
        let (x, top) = (x.max(4.0), top.max(4.0));
        for r in &mut rows {
            r.rect = Rect { x: x + PAD, y: top + r.rect.y, w: w - 2.0 * PAD, h: r.rect.h };
        }
        Panel { rect: Rect { x, y: top, w, h }, rows, selected: None }
    }

    /// The row at a point (dividers included).
    pub fn row_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.rect.contains(x, y) {
            return None;
        }
        self.rows.iter().position(|r| y >= r.rect.y && y < r.rect.y + r.rect.h)
    }
}

/// The drawn menu bar: each menu's title and where it is.
pub struct Bar {
    pub titles: Vec<(parley::Layout<Paint>, Rect)>,
    /// The menu that's open.
    pub open: Option<usize>,
    pub width: f64,
}

impl Bar {
    pub fn layout(text: &mut TextEngine, menus: &[Entry], width: f64) -> Bar {
        let mut x = 6.0;
        let mut titles = Vec::new();
        for m in menus {
            let t = text.layout(m.label(), 13.0, 400.0, theme().colors[theme::PRIMARY], None);
            let w = t.width() as f64 + 20.0;
            titles.push((t, Rect { x, y: 0.0, w, h: BAR_H }));
            x += w;
        }
        Bar { titles, open: None, width }
    }

    pub fn title_at(&self, x: f64, y: f64) -> Option<usize> {
        self.titles.iter().position(|(_, r)| r.contains(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shortcuts() {
        let s = |t: &str| Shortcut::parse(t).map(|s| s.normalized());
        assert_eq!(s("⌘O").as_deref(), Some("cmd+o"));
        assert_eq!(s("⇧⌘S").as_deref(), Some("cmd+shift+s"));
        assert_eq!(s("cmd+shift+S").as_deref(), Some("cmd+shift+s"));
        assert_eq!(s("alt+cmd+left").as_deref(), Some("cmd+alt+left"));
        assert_eq!(s("cmd++").as_deref(), Some("cmd++"));
        assert_eq!(s("delete").as_deref(), Some("delete"));
        assert_eq!(s("ctrl+s"), None);
        assert_eq!(s("cmd+ab"), None);
        assert_eq!(s(""), None);
    }
}
