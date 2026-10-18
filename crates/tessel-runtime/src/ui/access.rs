//! Accessibility: what a screen reader (VoiceOver, Narrator) is told is in
//! a window, and what it can do there.
//!
//! Tessel draws every control itself, so to the system a window is one
//! picture. `describe` says what's in it instead: each control and text,
//! with what it's called, what it holds, where it is, and whether it can be
//! pressed. Views that only arrange others (stacks, groups) aren't named:
//! what's in them belongs to the nearest view around them that is.
//!
//! On macOS and Windows that description goes to the system through
//! AccessKit (`system`), whenever a screen reader is listening.

use super::layout::{read_string, Rect};
use super::{Kind, Node, RibbonRole};

/// What a view is, to someone who can't see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Window,
    Text,
    Button,
    TextField,
    /// A text field whose text isn't shown, or said.
    Secret,
    Editor,
    Switch,
    /// A `Picker`: one of its options is chosen.
    Options,
    Option,
    /// A `TabView`: its tabs, then the page that shows.
    Tabs,
    Tab,
    Slider,
    Stepper,
    ColorWell,
    Image,
    Scroll,
    /// Text with headings, lists and links (`Markdown`).
    Document,
    Canvas,
    /// A `Table` and its rows (each selected or not), a `Tree` and its nodes.
    Table,
    Row,
    Tree,
    TreeItem,
}

/// A view, as a screen reader is told of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub role: Role,
    /// What it's called: a button's title, a picture's description.
    pub label: String,
    /// What it holds: a field's text, a text's own words.
    pub value: String,
    pub rect: Rect,
    pub children: Vec<u64>,
    pub disabled: bool,
    /// A switch that's on, an option or tab that's chosen, a group that's open.
    pub on: Option<bool>,
    /// A slider's or stepper's number, with the least, the most and the step.
    pub number: Option<[f64; 4]>,
    /// Whether pressing it does something.
    pub press: bool,
}

/// The window's own item is this one.
pub const WINDOW: u64 = u64::MAX - 1;

/// The id of tab `index` of the tab view `key` (tabs aren't views).
fn tab_id(key: u64, index: usize) -> u64 {
    key.rotate_left(17) ^ (0x7ab5 + index as u64)
}

/// What a view is called by what's in it: its texts' words, or, with no
/// text in it, its icons' names.
fn called_by(views: &[&Node]) -> String {
    fn words(n: &Node, texts: &mut Vec<String>, icons: &mut Vec<String>) {
        if n.mods.hidden {
            return;
        }
        if let Some(label) = &n.mods.access_label {
            texts.push(label.clone());
            return;
        }
        match &n.kind {
            Kind::Text(text) => texts.push(text.clone()),
            Kind::Button { label, .. } | Kind::Toggle { label, .. } | Kind::RibbonItem { label, .. } => texts.push(label.clone()),
            Kind::Svg { icon: Some(name), .. } => icons.push(spaced(name)),
            _ => {}
        }
        for c in n.visible_children() {
            words(c, texts, icons);
        }
    }
    let (mut texts, mut icons) = (Vec::new(), Vec::new());
    for n in views {
        words(n, &mut texts, &mut icons);
    }
    if texts.is_empty() { icons.join(" ") } else { texts.join(" ") }
}

/// An icon's name as words: `folderOpen` is "folder open".
fn spaced(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_uppercase() && !out.is_empty() {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// Everything in the window `root` is the tree of (titled `title`, `size`
/// points across and down), the window itself first.
pub fn describe(root: &Node, title: &str, size: (f64, f64)) -> Vec<Item> {
    let mut items = vec![Item {
        id: WINDOW,
        role: Role::Window,
        label: title.to_string(),
        value: String::new(),
        rect: Rect { x: 0.0, y: 0.0, w: size.0, h: size.1 },
        children: Vec::new(),
        disabled: false,
        on: None,
        number: None,
        press: false,
    }];
    visit(root, 0, &mut items);
    items
}

/// Adds `n` (if it's something to name) and what's in it, under item `parent`.
fn visit(n: &Node, parent: usize, items: &mut Vec<Item>) {
    if n.mods.hidden || matches!(n.kind, Kind::Divider | Kind::Spacer | Kind::MenuList | Kind::Window { .. }) {
        return;
    }
    let called = |fallback: &str| n.mods.access_label.clone().or_else(|| n.mods.tooltip.clone()).unwrap_or_else(|| fallback.to_string());
    let item = |role: Role, label: String| Item {
        id: n.key,
        role,
        label,
        value: String::new(),
        rect: n.layout.rect,
        children: Vec::new(),
        disabled: n.layout.style.disabled,
        on: None,
        number: None,
        press: false,
    };
    let bit = |flag: &super::Callback| unsafe { *flag.locate() } & 1 != 0;
    let mut leaf = false;
    let named = match &n.kind {
        Kind::Text(text) => Some(Item { value: text.clone(), ..item(Role::Text, n.mods.access_label.clone().unwrap_or_default()) }),
        Kind::Button { label, .. } => Some(Item { press: true, ..item(Role::Button, called(label)) }),
        Kind::RibbonItem { label, role, .. } => Some(match role {
            RibbonRole::Toggle(is_on) => Item { press: true, on: Some(bit(is_on)), ..item(Role::Switch, called(label)) },
            _ => Item { press: true, ..item(Role::Button, called(label)) },
        }),
        Kind::Toggle { label, is_on } => Some(Item { press: true, on: Some(bit(is_on)), ..item(Role::Switch, called(label)) }),
        Kind::TextField { placeholder, text, secure, lines } => Some(Item {
            value: if *secure { String::new() } else { read_string(text) },
            press: true,
            ..item(
                match (*secure, *lines > 1) {
                    (true, _) => Role::Secret,
                    (false, true) => Role::Editor,
                    _ => Role::TextField,
                },
                called(placeholder),
            )
        }),
        Kind::CodeEditor { text, .. } => {
            // (A screen reader is given a long text in parts it asks for;
            // here it gets the start of one.)
            let all = read_string(text);
            let cut = all.char_indices().nth(20_000).map_or(all.len(), |(i, _)| i);
            Some(Item { value: all[..cut].to_string(), press: true, ..item(Role::Editor, called("Code")) })
        }
        Kind::Slider { value, from, to, step, .. } => {
            let now = unsafe { *(value.locate() as *const f64) };
            Some(Item { number: Some([now, *from, *to, *step]), ..item(Role::Slider, called("")) })
        }
        Kind::Stepper { value, int, step, from, to, .. } => {
            let now = unsafe { if *int { *(value.locate() as *const i64) as f64 } else { *(value.locate() as *const f64) } };
            Some(Item { number: Some([now, *from, *to, *step]), ..item(Role::Stepper, called("")) })
        }
        Kind::ColorPicker { .. } => Some(Item { press: true, ..item(Role::ColorWell, called("Color")) }),
        Kind::Disclosure { title, expanded } => Some(Item { press: true, on: Some(bit(expanded)), ..item(Role::Button, called(title)) }),
        Kind::Svg { icon, .. } => {
            let label = n.mods.access_label.clone().or_else(|| n.mods.tooltip.clone()).or_else(|| icon.map(spaced));
            label.map(|label| item(Role::Image, label))
        }
        Kind::Image { .. } => Some(item(Role::Image, called("Picture"))),
        Kind::Canvas { .. } => Some(item(Role::Canvas, called("Drawing"))),
        Kind::Scene3D { .. } => Some(item(Role::Canvas, called("3D scene"))),
        Kind::Markdown { source, .. } => Some(Item { value: source.to_string(), ..item(Role::Document, called("")) }),
        Kind::Picker { .. } => Some(item(Role::Options, called(""))),
        Kind::TabView { .. } => Some(item(Role::Tabs, called(""))),
        Kind::Scroll => Some(item(Role::Scroll, called(""))),
        Kind::Table(spec) => Some(item(if spec.tree.is_some() { Role::Tree } else { Role::Table }, called(""))),
        Kind::TableRow(row) => {
            let role = if row.path.is_some() { Role::TreeItem } else { Role::Row };
            let cells: Vec<&Node> = n.children.iter().collect();
            Some(Item { on: Some(row.selected), press: true, ..item(role, called_by(&cells)) })
        }
        _ => None,
    };
    // A view that's pressed (`.onTap`) is a button, called what's in it.
    let named = match named {
        Some(item) if !matches!(item.role, Role::Text | Role::Image) || n.mods.on_tap.is_none() => Some(item),
        _ if n.mods.on_tap.is_some() => {
            leaf = true;
            let label = n.mods.access_label.clone().or_else(|| Some(called_by(&[n])).filter(|w| !w.is_empty())).or_else(|| n.mods.tooltip.clone());
            Some(Item { press: true, ..item(Role::Button, label.unwrap_or_default()) })
        }
        other => other,
    };
    let Some(named) = named else {
        // It only arranges what's in it.
        for c in n.visible_children() {
            visit(c, parent, items);
        }
        return;
    };
    let me = items.len();
    items[parent].children.push(named.id);
    let role = named.role;
    items.push(named);
    match &n.kind {
        _ if leaf => {}
        // The options: each child with a tag, called by its words.
        Kind::Picker { selection } => {
            let current = selection.locate();
            for (c, place) in n.children.iter().zip(&n.layout.segments) {
                let chosen = c.mods.tag.as_ref().is_some_and(|t| t.equals(current));
                let option = Item { id: c.key, rect: *place, on: Some(chosen), press: true, ..item(Role::Option, called_by(&[c])) };
                items[me].children.push(option.id);
                items.push(option);
            }
        }
        // The tabs, then the page that shows.
        Kind::TabView { .. } => {
            let chosen = n.selected_tab();
            for (i, (tab, page)) in n.layout.tabs.iter().zip(&n.children).enumerate() {
                let label = page.mods.tab_title.clone().unwrap_or_default();
                let tab = Item { id: tab_id(n.key, i), rect: tab.rect, on: Some(i == chosen), press: true, ..item(Role::Tab, label) };
                items[me].children.push(tab.id);
                items.push(tab);
            }
            for c in n.visible_children() {
                visit(c, me, items);
            }
        }
        // (Its parts are drawn by it: there's nothing more to name.)
        _ if matches!(role, Role::Button | Role::Switch | Role::TextField | Role::Secret | Role::Editor | Role::ColorWell | Role::Slider | Role::Stepper)
            && !matches!(n.kind, Kind::Disclosure { .. }) => {}
        // An open group's content comes after its title, beside it.
        Kind::Disclosure { .. } => {
            for c in n.visible_children() {
                visit(c, parent, items);
            }
        }
        _ => {
            for c in n.visible_children() {
                visit(c, me, items);
            }
        }
    }
}

/// The description as text, a line for each item, for tests (the
/// `accessibility` script command).
pub fn listing(items: &[Item]) -> String {
    fn line(items: &[Item], id: u64, depth: usize, out: &mut String) {
        let Some(item) = items.iter().find(|i| i.id == id) else { return };
        let mut text = format!("{}{:?}", "  ".repeat(depth), item.role);
        if !item.label.is_empty() {
            text += &format!(" {:?}", item.label);
        }
        if !item.value.is_empty() {
            let short: String = item.value.chars().take(60).collect();
            text += &format!(" = {short:?}");
        }
        if let Some(on) = item.on {
            text += match (item.role, on) {
                (Role::Switch, true) => " (on)",
                (Role::Switch, false) => " (off)",
                (Role::Button, true) => " (open)",
                (Role::Button, false) => " (closed)",
                (_, true) => " (chosen)",
                (_, false) => "",
            };
        }
        if let Some([now, from, to, _]) = item.number {
            text += &format!(" = {now} ({from} to {to})");
        }
        if item.disabled {
            text += " (disabled)";
        }
        out.push_str(&text);
        out.push('\n');
        for child in &item.children {
            line(items, *child, depth + 1, out);
        }
    }
    let mut out = String::new();
    line(items, WINDOW, 0, &mut out);
    out
}

/// Telling the system, with AccessKit.
#[cfg(any(target_os = "macos", windows))]
pub mod system {
    use std::sync::Mutex;

    use accesskit::{Action, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId, Toggled, TreeId, TreeInfo, TreeUpdate};
    use winit::window::WindowId;

    use super::{Item, Role, WINDOW};

    pub use accesskit_winit::Adapter;

    /// What a screen reader asked to be done, and in which window: taken up
    /// by the app in its own time (`asked`).
    static ASKED: Mutex<Vec<(WindowId, u64, Action)>> = Mutex::new(Vec::new());

    /// A screen reader started listening, and waits to be told what's in
    /// the windows.
    static WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// Whether one started listening since the last look: the windows are
    /// drawn again then, which is when they're described (`update`).
    pub fn wanted() -> bool {
        WANTED.swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    /// (The description is given when the window is next drawn.)
    struct Later;

    impl ActivationHandler for Later {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            WANTED.store(true, std::sync::atomic::Ordering::SeqCst);
            super::super::app::wake();
            None
        }
    }

    impl DeactivationHandler for Later {
        fn deactivate_accessibility(&mut self) {}
    }

    struct Asks(WindowId);

    impl ActionHandler for Asks {
        fn do_action(&mut self, request: ActionRequest) {
            ASKED.lock().unwrap().push((self.0, request.target_node.0, request.action));
            super::super::app::wake();
        }
    }

    /// What screen readers asked for since the last look.
    pub fn asked() -> Vec<(WindowId, u64, Action)> {
        std::mem::take(&mut *ASKED.lock().unwrap())
    }

    /// The system's hold on a window, which must not be showing yet.
    pub fn adapter(event_loop: &winit::event_loop::ActiveEventLoop, window: &winit::window::Window) -> Adapter {
        Adapter::with_direct_handlers(event_loop, window, Later, Asks(window.id()), Later)
    }

    /// The description for the system: `scale` pixels to the point, with
    /// the keyboard on item `focus`.
    pub fn update(items: &[Item], focus: Option<u64>, scale: f64) -> TreeUpdate {
        let nodes = items
            .iter()
            .map(|item| {
                let mut node = Node::new(match item.role {
                    Role::Window => accesskit::Role::Window,
                    Role::Text => accesskit::Role::Label,
                    Role::Button => accesskit::Role::Button,
                    Role::TextField => accesskit::Role::TextInput,
                    Role::Secret => accesskit::Role::PasswordInput,
                    Role::Editor => accesskit::Role::MultilineTextInput,
                    Role::Switch => accesskit::Role::Switch,
                    Role::Options => accesskit::Role::RadioGroup,
                    Role::Option => accesskit::Role::RadioButton,
                    Role::Tabs => accesskit::Role::Group,
                    Role::Tab => accesskit::Role::Tab,
                    Role::Table => accesskit::Role::Table,
                    Role::Row => accesskit::Role::Row,
                    Role::Tree => accesskit::Role::Tree,
                    Role::TreeItem => accesskit::Role::TreeItem,
                    Role::Slider => accesskit::Role::Slider,
                    Role::Stepper => accesskit::Role::SpinButton,
                    Role::ColorWell => accesskit::Role::ColorWell,
                    Role::Image => accesskit::Role::Image,
                    Role::Scroll => accesskit::Role::ScrollView,
                    Role::Document => accesskit::Role::Document,
                    Role::Canvas => accesskit::Role::Canvas,
                });
                let r = item.rect;
                node.set_bounds(accesskit::Rect { x0: r.x, y0: r.y, x1: r.x + r.w, y1: r.y + r.h });
                if item.role == Role::Window {
                    // (Places are in points; the system counts pixels.)
                    node.set_transform(accesskit::Affine::scale(scale));
                }
                if !item.label.is_empty() {
                    node.set_label(item.label.clone());
                }
                if !item.value.is_empty() {
                    node.set_value(item.value.clone());
                }
                if !item.children.is_empty() {
                    node.set_children(item.children.iter().map(|c| NodeId(*c)).collect::<Vec<_>>());
                }
                if item.disabled {
                    node.set_disabled();
                }
                match (item.role, item.on) {
                    (Role::Switch | Role::Option, Some(on)) => node.set_toggled(if on { Toggled::True } else { Toggled::False }),
                    (Role::Button, Some(open)) => node.set_expanded(open),
                    (_, Some(chosen)) => node.set_selected(chosen),
                    _ => {}
                }
                if let Some([now, from, to, step]) = item.number {
                    node.set_numeric_value(now);
                    if from.is_finite() {
                        node.set_min_numeric_value(from);
                    }
                    if to.is_finite() {
                        node.set_max_numeric_value(to);
                    }
                    if step > 0.0 {
                        node.set_numeric_value_step(step);
                    }
                    node.add_action(Action::Increment);
                    node.add_action(Action::Decrement);
                }
                if item.press {
                    node.add_action(Action::Click);
                    node.add_action(Action::Focus);
                }
                (NodeId(item.id), node)
            })
            .collect();
        let focus = focus.filter(|f| items.iter().any(|i| i.id == *f)).unwrap_or(WINDOW);
        let mut tree = TreeInfo::new(NodeId(WINDOW));
        tree.toolkit_name = Some("Tessel".into());
        TreeUpdate { nodes, tree: Some(tree), tree_id: TreeId::ROOT, focus: NodeId(focus) }
    }
}
