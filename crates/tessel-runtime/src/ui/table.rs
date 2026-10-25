//! `Table` and `Tree`: rows of the program's data in columns, of which only
//! the rows that show are built (as a `LazyVStack` builds them).
//!
//! A table is built as a `Kind::Table` with its header drawn by itself, and
//! one child, a scroll view, holding a `Kind::Lazy` list of `Kind::TableRow`s,
//! each with a cell (a `VStack`) for each column. A tree is a table of one
//! column whose rows are the visible nodes of the program's tree, each with
//! its place in it (its path: `[2, 0]` is the first child of the third node
//! at the top), indented and with an arrow to open it.
//!
//! The program keeps the data. The table says what was chosen (the rows
//! selected, the column to sort by) through its `bind`s, and asks for the
//! cells of the rows it builds; the program sorts its data itself.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::rc::Rc;

use super::{with_builder, Callback, Kind};
use crate::{Header, List, Str};

/// How tall the header is.
pub const HEADER: f64 = 26.0;
/// How far each level of a tree is indented, and the room its arrow takes.
pub const INDENT: f64 = 16.0;
/// A column can't be made narrower than this.
pub const MIN_WIDTH: f64 = 24.0;
/// The room inside a cell, at each side.
pub const CELL_PAD: f64 = 6.0;

/// A column, as its `TableColumn` says.
pub struct Column {
    pub title: String,
    /// None: it shares the room the others leave.
    pub width: Option<f64>,
    /// 0 leading, 1 center, 2 trailing.
    pub align: u8,
}

/// What a table (or tree) is: what the program said of it.
pub struct Spec {
    pub columns: Vec<Column>,
    /// How many rows (of a tree: visible nodes).
    pub count: usize,
    pub row_height: f64,
    /// Locators: the rows selected (`[Int]`; of a tree, the selected node's
    /// path), the column to sort by (`Int`, -1 for none), and whether up.
    pub selection: Option<Callback>,
    pub sort: Option<(Callback, Callback)>,
    /// `onSort`: after a click on a column's title sorted by it.
    pub on_sort: Option<Callback>,
    pub multiple: bool,
    /// `onOpen`: a row double-clicked, or Enter on it; of a tree, with the
    /// node's path.
    pub on_open: Option<Callback>,
    /// A tree: how many children a node has, by its path.
    pub tree: Option<Callback>,
}

/// A row: where it is among the rows (and of a tree, the node it shows),
/// and what it looks like.
#[derive(Clone)]
pub struct Row {
    pub table: u64,
    pub index: usize,
    pub path: Option<Rc<[i64]>>,
    pub depth: usize,
    /// A tree's node with children, and whether they show.
    pub expandable: bool,
    pub expanded: bool,
    pub selected: bool,
}

thread_local! {
    /// Columns a user widened or narrowed, by table and column.
    pub static WIDTHS: RefCell<HashMap<u64, Vec<Option<f64>>>> = RefCell::new(HashMap::new());
    /// The nodes of each tree that are open, by their paths.
    pub static OPEN: RefCell<HashMap<u64, HashSet<Vec<i64>>>> = RefCell::new(HashMap::new());
    /// The visible nodes of each tree, as last built: for the keys.
    pub static VISIBLE: RefCell<HashMap<u64, Vec<(Vec<i64>, usize, bool)>>> = RefCell::new(HashMap::new());
    /// The table being built: its columns so far, and their cells' blocks.
    static BUILDING: RefCell<Vec<(Vec<Column>, Vec<Callback>)>> = const { RefCell::new(Vec::new()) };
}

/// The widths of a table's columns when it's `width` wide: as the program
/// gave them (or the user made them), and the rest shared by the columns
/// without one.
pub fn widths(spec: &Spec, key: u64, width: f64) -> Vec<f64> {
    let set = WIDTHS.with(|w| w.borrow().get(&key).cloned()).unwrap_or_default();
    let given: Vec<Option<f64>> =
        spec.columns.iter().enumerate().map(|(i, c)| set.get(i).copied().flatten().or(c.width).map(|w| w.max(MIN_WIDTH))).collect();
    let fixed: f64 = given.iter().flatten().sum();
    let flexible = given.iter().filter(|w| w.is_none()).count();
    let share = if flexible > 0 { ((width - fixed) / flexible as f64).max(80.0) } else { 0.0 };
    given.iter().map(|w| w.unwrap_or(share)).collect()
}

/// A list `[Int]` of `values` (for the program: it takes the reference).
pub fn ints(values: &[i64]) -> *mut List {
    unsafe {
        let mut list = crate::tsl_list_new(&crate::codec::INT_TYPE, values.len() as i64);
        for v in values {
            crate::tsl_list_push(&mut list, v as *const i64 as *const u8);
        }
        list
    }
}

/// The `[Int]` a locator reaches.
pub fn read_ints(locator: &Callback) -> Vec<i64> {
    unsafe {
        let list = *(locator.locate() as *const *const List);
        if list.is_null() || (*list).len <= 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts((*list).data as *const i64, (*list).len as usize).to_vec()
    }
}

/// Puts `values` where a locator reaches (letting go of what was there).
pub fn write_ints(locator: &Callback, values: &[i64]) {
    unsafe {
        let slot = locator.locate() as *mut *mut List;
        let old = *slot;
        *slot = ints(values);
        crate::tsl_release(old as *mut Header);
    }
}

/// How many children the node at `path` has, as the program says.
pub fn children_of(children: &Callback, path: &[i64]) -> usize {
    children.call_ptr_to_int(ints(path) as *mut u8).clamp(0, 1_000_000) as usize
}

/// The nodes of a tree that show: each with its path, its depth, and
/// whether it has children (shown when it's open).
pub fn visible(children: &Callback, open: &HashSet<Vec<i64>>) -> Vec<(Vec<i64>, usize, bool)> {
    fn add(children: &Callback, open: &HashSet<Vec<i64>>, path: &mut Vec<i64>, out: &mut Vec<(Vec<i64>, usize, bool)>) {
        let n = children_of(children, path);
        for i in 0..n {
            path.push(i as i64);
            // (Far too many rows for a person to look at: the rest are left out.)
            if out.len() >= 2_000_000 {
                path.pop();
                return;
            }
            let has = children_of(children, path) > 0;
            out.push((path.clone(), path.len() - 1, has));
            if has && open.contains(path.as_slice()) {
                add(children, open, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    add(children, open, &mut Vec::new(), &mut out);
    out
}

/// `Table(rows:rowHeight:selection:sortColumn:ascending:multiple:onOpen:)
/// { columns }`: starts a table; its block adds the columns.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_ui_table_begin() {
    // (The header's arrows, and a tree's, are built-in icons.)
    super::svg::enable_icons();
    BUILDING.with(|b| b.borrow_mut().push((Vec::new(), Vec::new())));
}

/// `TableColumn(title, width:alignment:) { i in … }`: a column of the
/// table being built; `f(env, i)` builds its cell in row `i`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_ui_table_column(title: *const Str, has_width: u8, width: f64, align: i32, f: *const c_void, env: *mut Header) {
    let title = unsafe { crate::str_text(title) }.to_string();
    let cells = Callback::new(f, env);
    let width = (has_width != 0 && width.is_finite()).then_some(width.max(MIN_WIDTH));
    let align = match align {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    BUILDING.with(|b| {
        if let Some((columns, blocks)) = b.borrow_mut().last_mut() {
            columns.push(Column { title, width, align });
            blocks.push(cells);
        }
    });
}

/// Ends a table, and builds it with the rows that show.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_table_end(
    rows: i64,
    row_height: f64,
    sel_f: *const c_void,
    sel_env: *mut Header,
    col_f: *const c_void,
    col_env: *mut Header,
    up_f: *const c_void,
    up_env: *mut Header,
    sorted_f: *const c_void,
    sorted_env: *mut Header,
    multiple: u8,
    open_f: *const c_void,
    open_env: *mut Header,
) {
    let Some((columns, blocks)) = BUILDING.with(|b| b.borrow_mut().pop()) else { return };
    let given = |f: *const c_void, env| (!f.is_null()).then(|| Callback::new(f, env));
    let sort = match (given(col_f, col_env), given(up_f, up_env)) {
        (Some(c), Some(u)) => Some((c, u)),
        _ => None,
    };
    let spec = Spec {
        columns,
        count: rows.max(0) as usize,
        row_height: if row_height.is_finite() && row_height >= 8.0 { row_height } else { 24.0 },
        selection: given(sel_f, sel_env),
        sort,
        on_sort: given(sorted_f, sorted_env),
        multiple: multiple != 0,
        on_open: given(open_f, open_env),
        tree: None,
    };
    let selected: HashSet<i64> = spec.selection.as_ref().map(read_ints).unwrap_or_default().into_iter().collect();
    let aligns: Vec<u8> = spec.columns.iter().map(|c| c.align).collect();
    build(spec, |_| None, |row| {
        for (column, cells) in blocks.iter().enumerate() {
            let align = aligns.get(column).copied().unwrap_or(0);
            with_builder(|b| b.begin(Kind::VStack(0.0, align), column as u64));
            cells.call_with_int(row.index as i64);
            with_builder(|b| end_cell(b, align));
        }
        row.selected = selected.contains(&(row.index as i64));
    });
}

/// `Tree(children:rowHeight:selection:onOpen:) { path in … }`: a tree of
/// the program's nodes, of which `children(env, path)` tells how many
/// children each has, and `f(env, path)` builds a node's row.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn tsl_ui_tree(
    children_f: *const c_void,
    children_env: *mut Header,
    row_height: f64,
    sel_f: *const c_void,
    sel_env: *mut Header,
    open_f: *const c_void,
    open_env: *mut Header,
    f: *const c_void,
    env: *mut Header,
) {
    super::svg::enable_icons();
    let given = |f: *const c_void, env| (!f.is_null()).then(|| Callback::new(f, env));
    let children = Callback::new(children_f, children_env);
    let rows = Callback::new(f, env);
    let spec = Spec {
        columns: vec![Column { title: String::new(), width: None, align: 0 }],
        count: 0,
        row_height: if row_height.is_finite() && row_height >= 8.0 { row_height } else { 24.0 },
        selection: given(sel_f, sel_env),
        sort: None,
        on_sort: None,
        multiple: false,
        on_open: given(open_f, open_env),
        tree: Some(children.clone()),
    };
    let selected = spec.selection.as_ref().map(read_ints);
    // The nodes that show, once the tree's identity (which its open nodes
    // are kept by) is known.
    let shown: RefCell<Vec<(Vec<i64>, usize, bool)>> = RefCell::new(Vec::new());
    let open: RefCell<HashSet<Vec<i64>>> = RefCell::new(HashSet::new());
    build(
        spec,
        |key| {
            *open.borrow_mut() = OPEN.with(|o| o.borrow().get(&key).cloned()).unwrap_or_default();
            *shown.borrow_mut() = visible(&children, &open.borrow());
            VISIBLE.with(|v| v.borrow_mut().insert(key, shown.borrow().clone()));
            Some(shown.borrow().len())
        },
        |row| {
        let shown = shown.borrow();
        let open = open.borrow();
        let Some((path, depth, has)) = shown.get(row.index) else { return };
        row.path = Some(path.as_slice().into());
        row.depth = *depth;
        row.expandable = *has;
        row.expanded = *has && open.contains(path);
        row.selected = selected.as_deref() == Some(path.as_slice());
        with_builder(|b| b.begin(Kind::VStack(0.0, 0), 0));
        rows.call_with(ints(path) as *mut u8);
        with_builder(|b| end_cell(b, 0));
    },
    );
}

/// Ends a cell: as wide as its column, with what's in it lined up as the
/// column says, and nothing of it outside.
fn end_cell(b: &mut super::Builder, align: u8) {
    let cell = b.parent();
    cell.mods.max_width = Some(f64::INFINITY);
    cell.mods.max_height = Some(f64::INFINITY);
    cell.mods.frame_align = Some(align);
    cell.mods.clipped = true;
    b.end();
}

/// Builds a table: the table, its scroll view, and the rows of its list
/// that show (see `LazyVStack`), each made by `cells`. `count_of` may say
/// how many rows there are, given the table's identity.
fn build(mut spec: Spec, count_of: impl FnOnce(u64) -> Option<usize>, mut cells: impl FnMut(&mut Row)) {
    let mut table = 0;
    with_builder(|b| table = b.next_table_key());
    if let Some(n) = count_of(table) {
        spec.count = n;
    }
    let (count, row_height, tree) = (spec.count, spec.row_height, spec.tree.is_some());
    with_builder(|b| {
        b.begin(Kind::Table(Rc::new(spec)), 0);
        table = b.parent().key;
        b.begin(Kind::Scroll(0), 0);
        b.begin(Kind::Lazy { count, row_height, first: 0 }, 0);
    });
    let mut list = 0;
    with_builder(|b| list = b.parent().key);
    let (from, to) = super::controls::LAZY.with(|l| l.borrow().get(&list).copied()).unwrap_or((0, 0));
    let to = to.min(count);
    let from = from.min(to);
    with_builder(|b| {
        if let Kind::Lazy { first, .. } = &mut b.parent().kind {
            *first = from;
        }
    });
    for i in from..to {
        let mut row = Row { table, index: i, path: None, depth: 0, expandable: false, expanded: false, selected: false };
        // A table's rows are known by their number; a tree's by the node
        // they show (its rows move as nodes open and close).
        let id = if tree {
            VISIBLE.with(|v| v.borrow().get(&table).and_then(|s| s.get(i)).map(|(p, ..)| super::hash_key(p))).unwrap_or(i as u64)
        } else {
            i as u64
        };
        with_builder(|b| {
            b.pending_id = Some(id);
            b.begin(Kind::TableRow(row.clone()), 0);
        });
        cells(&mut row);
        with_builder(|b| {
            if let Kind::TableRow(r) = &mut b.parent().kind {
                *r = row.clone();
            }
            b.end();
        });
    }
    with_builder(|b| {
        b.end();
        b.end();
        b.end();
    });
}

/// Drops what's kept for tables and trees that aren't in the tree any more.
pub fn keep_only(keys: &HashSet<u64>) {
    WIDTHS.with(|w| w.borrow_mut().retain(|k, _| keys.contains(k)));
    VISIBLE.with(|v| v.borrow_mut().retain(|k, _| keys.contains(k)));
}
