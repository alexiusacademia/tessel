//! Programs compiled for the debugger (`tessel debug`).
//!
//! A debug build tells the runtime where it is as it runs, and the runtime's
//! debugger (`tessel_runtime::debug`) decides when to stop. Nothing of the
//! system's debuggers is used, so it works the same everywhere.
//!
//! Each function of the program's own code:
//!
//! - says it was entered (`tsl_debug_enter`, with its name and file) and
//!   left (`tsl_debug_leave`): the runtime keeps the stack of calls;
//! - says which line it's on before each statement (`tsl_debug_line`),
//!   which is where the runtime may stop it;
//! - says where its variables are, as they come into being
//!   (`tsl_debug_local`), with a description of their type (`dbg_type`)
//!   that lets the runtime read them out of memory.
//!
//! A function that waits (`coro`) leaves the stack while it waits
//! (`tsl_debug_park`) and comes back when it goes on (`tsl_debug_resume`),
//! keeping what it said about its variables.

use inkwell::module::Linkage;
use inkwell::values::{AsValueRef, BasicValueEnum, PointerValue};

use super::{Gen, Store, Var};
use crate::ast::*;
use crate::source::Span;
use crate::types::Ty;

/// What a function compiled for the debugger keeps while it's compiled.
#[derive(Clone)]
pub(super) struct Dbg<'ctx> {
    /// Where the runtime's record of this call is kept (for `resume`).
    frame: PointerValue<'ctx>,
    /// The variables the runtime was told of, as of the line last emitted:
    /// their names and where they are.
    shown: Vec<(String, usize)>,
    /// The function's name, as the stack of calls shows it.
    name: String,
}

// The kinds of type the runtime's debugger knows (its `Kind`).
const INT: u64 = 0;
const FLOAT: u64 = 1;
const BOOL: u64 = 2;
const STR: u64 = 3;
const DATA: u64 = 4;
const LIST: u64 = 5;
const DICT: u64 = 6;
const SET: u64 = 7;
const OPT: u64 = 8;
const STRUCT: u64 = 9;
const ENUM: u64 = 10;
const FUNCTION: u64 = 11;
const VIEW: u64 = 12;
const IFACE: u64 = 13;
const RANGE: u64 = 14;
const OTHER: u64 = 15;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    /// The function being started (its context is pushed, and it's a
    /// coroutine already if it's to be one) tells the debugger it was
    /// entered. `name` is what the stack of calls shows; `span` is in its file.
    pub(super) fn debug_enter(&mut self, name: &str, span: Span) {
        if !self.debug {
            return;
        }
        let file = self.sources.file(span.file).name.clone();
        let shown_as = name.to_string();
        let (name, file) = (self.c_str(name), self.c_str(&file));
        let p = self.ptr_ty();
        let ty = self.ctx.struct_type(&[p.into(), p.into()], false);
        let info = self.module.add_global(ty, None, "dbgfn");
        info.set_linkage(Linkage::Private);
        info.set_constant(true);
        info.set_initializer(&ty.const_named_struct(&[name.into(), file.into()]));
        let enter = self.declare_rt("tsl_debug_enter", &[p.into()], Some(p.into()));
        let record = self.b.build_call(enter, &[info.as_pointer_value().into()], "").unwrap().try_as_basic_value().basic().unwrap();
        let frame = self.alloca(p.into(), "dbgframe");
        self.b.build_store(frame, record).unwrap();
        // A function that waits is left where its frame is freed, whichever
        // way it ends.
        if let Some(c) = self.stack.last().unwrap().coro.clone() {
            let here = self.b.get_insert_block().unwrap();
            let free = c.free_block();
            self.b.position_before(&free.get_first_instruction().expect("a coroutine's last block"));
            self.debug_leave_call();
            self.b.position_at_end(here);
        }
        self.ctx_mut().dbg = Some(Dbg { frame, shown: Vec::new(), name: shown_as });
    }

    fn debug_leave_call(&mut self) {
        let leave = self.declare_rt("tsl_debug_leave", &[], None);
        self.b.build_call(leave, &[], "").unwrap();
    }

    /// Before a function returns (one that doesn't wait: see `debug_enter`).
    pub(super) fn debug_leave(&mut self) {
        if self.stack.last().is_some_and(|c| c.dbg.is_some()) {
            self.debug_leave_call();
        }
    }

    /// Before a function that waits is suspended: it's off the stack of
    /// calls until it goes on.
    pub(super) fn debug_park(&mut self) {
        if self.stack.last().is_some_and(|c| c.dbg.is_some()) {
            let park = self.declare_rt("tsl_debug_park", &[], None);
            self.b.build_call(park, &[], "").unwrap();
        }
    }

    /// When a function that waited goes on.
    pub(super) fn debug_resume(&mut self) {
        let Some(dbg) = self.stack.last().and_then(|c| c.dbg.clone()) else { return };
        let p = self.ptr_ty();
        let record = self.b.build_load(p, dbg.frame, "").unwrap();
        let resume = self.declare_rt("tsl_debug_resume", &[p.into()], None);
        self.b.build_call(resume, &[record.into()], "").unwrap();
    }

    /// A function that waits is done, and about to give its result to
    /// whoever waited for it (which goes on right then, inside this call).
    pub(super) fn debug_done(&mut self) {
        if self.debugged() {
            let done = self.declare_rt("tsl_debug_done", &[], None);
            self.b.build_call(done, &[], "").unwrap();
        }
    }

    /// Whether the function being compiled talks to the debugger.
    pub(super) fn debugged(&self) -> bool {
        self.stack.last().is_some_and(|c| c.dbg.is_some())
    }

    /// Before the statement at `span`: the function is on that line, with
    /// the variables that are in scope there.
    pub(super) fn debug_line(&mut self, span: Span) {
        if !self.debugged() || self.terminated() {
            return;
        }
        let vars: Vec<Var<'ctx>> = self
            .stack
            .last()
            .unwrap()
            .scopes
            .iter()
            .flatten()
            .filter(|v| {
                // (A receiver's fields are found in `self`.)
                // (And what the compiler keeps for itself has no name to show.)
                v.name.starts_with(|c: char| c.is_alphabetic() || c == '_')
                    && !(v.name == "_" || matches!(v.store, Store::Field(_) | Store::ViewFn))
                    && !matches!(v.ty, Ty::Void | Ty::Error | Ty::Any | Ty::Generic(_))
            })
            .cloned()
            .collect();
        let place = |v: &Var<'ctx>| match v.store {
            Store::Slot(p) | Store::Boxed(p) | Store::EnvValue(p) | Store::EnvBox(p) | Store::Place(p) => p.as_value_ref() as usize,
            Store::ListElem { idx, .. } | Store::ListElemVia { idx, .. } => idx.as_value_ref() as usize,
            Store::Binding { slot, .. } => slot.as_value_ref() as usize,
            Store::State(i) | Store::Field(i) => i as usize,
            Store::SelfRef | Store::ViewFn => 0,
        };
        let now: Vec<(String, usize)> = vars.iter().map(|v| (v.name.clone(), place(v))).collect();
        let shown = self.stack.last().unwrap().dbg.as_ref().unwrap().shown.clone();
        let same = now.iter().zip(&shown).take_while(|(a, b)| a == b).count();
        let (p, i32t) = (self.ptr_ty(), self.ctx.i32_type());
        for (index, v) in vars.iter().enumerate().skip(same) {
            // Where the value is: its address, or for the kinds of variable
            // whose value moves (an item of a list that may grow, a `bind`),
            // what the runtime finds it with when it's asked for.
            let null = p.const_null();
            let (kind, a, b) = match v.store {
                Store::ListElem { slot, idx, .. } => (1, slot, idx),
                Store::ListElemVia { locator, idx } => (2, locator, idx),
                Store::Binding { slot, .. } => (3, slot, null),
                _ => (0, self.var_read_ptr(v, Span::default()), null),
            };
            let name = self.c_str(&v.name);
            let ty = self.dbg_type(&v.ty);
            let local = self.declare_rt("tsl_debug_local", &[i32t.into(), p.into(), i32t.into(), p.into(), p.into(), p.into()], None);
            let args: [BasicValueEnum; 6] = [
                i32t.const_int(index as u64, false).into(),
                name.into(),
                i32t.const_int(kind, false).into(),
                a.into(),
                b.into(),
                ty.into(),
            ];
            let args: Vec<_> = args.iter().map(|a| (*a).into()).collect();
            self.b.build_call(local, &args, "").unwrap();
        }
        self.ctx_mut().dbg.as_mut().unwrap().shown = now;
        let (line, _) = self.sources.file(span.file).line_col(span.lo as usize);
        let at = self.declare_rt("tsl_debug_line", &[i32t.into(), i32t.into()], None);
        let args = [i32t.const_int(line as u64, false).into(), i32t.const_int(vars.len() as u64, false).into()];
        self.b.build_call(at, &args, "").unwrap();
    }

    /// The description of type `t` for the runtime's debugger (its
    /// `Type`): what kind of value it is, and where its parts are.
    fn dbg_type(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let key = self.key(t);
        self.dbg_type_for(&key, t, None)
    }

    fn dbg_type_for(&mut self, key: &str, t: &Ty, payload: Option<usize>) -> PointerValue<'ctx> {
        if let Some(found) = self.dbg_types.get(key) {
            return *found;
        }
        let (i, p) = (self.i64(), self.ptr_ty());
        let ty = self.ctx.struct_type(&[i.into(), p.into(), i.into(), i.into(), p.into(), p.into(), p.into()], false);
        let global = self.module.add_global(ty, None, &format!("tsl_dbg_{key}"));
        global.set_linkage(Linkage::Private);
        global.set_constant(true);
        // (Known before its parts are described: a type may hold itself.)
        self.dbg_types.insert(key.to_string(), global.as_pointer_value());

        let null = p.const_null();
        let mut name = self.checked.show(t);
        let mut size = self.llty(t).map_or(0, |lt| self.td.get_abi_size(&lt));
        let (mut count, mut items, mut first, mut second) = (0u64, null, null, null);
        // The parts of a value kept as a struct: fields, or a case's values.
        let parts = |g: &mut Self, lt: inkwell::types::StructType<'ctx>, fields: Vec<(String, Ty)>| {
            let field_ty = g.ctx.struct_type(&[p.into(), i.into(), p.into()], false);
            let described: Vec<_> = fields
                .iter()
                .enumerate()
                .map(|(n, (name, ft))| {
                    let offset = g.td.offset_of_element(&lt, n as u32).unwrap_or(0);
                    let (name, ft) = (g.c_str(name), g.dbg_type(ft));
                    field_ty.const_named_struct(&[name.into(), i.const_int(offset, false).into(), ft.into()])
                })
                .collect();
            let array = field_ty.const_array(&described);
            let list = g.module.add_global(array.get_type(), None, "dbgfields");
            list.set_linkage(Linkage::Private);
            list.set_constant(true);
            list.set_initializer(&array);
            (fields.len() as u64, list.as_pointer_value())
        };
        let kind = match (t, payload) {
            (Ty::Enum(id), Some(case)) => {
                let def = self.checked.enums[*id].cases[case].clone();
                let fields = def.fields.iter().map(|f| (f.label.clone(), f.ty.clone())).collect();
                let lt = self.payload_ty(*id, case);
                name = def.name;
                size = self.td.get_abi_size(&lt);
                (count, items) = parts(self, lt, fields);
                STRUCT
            }
            (Ty::Int, _) => INT,
            (Ty::Float, _) => FLOAT,
            (Ty::Bool, _) => BOOL,
            (Ty::Str, _) => STR,
            (Ty::Data, _) => DATA,
            (Ty::Range, _) => RANGE,
            (Ty::View, _) => VIEW,
            (Ty::Fn(..), _) => FUNCTION,
            (Ty::Iface(_), _) => IFACE,
            (Ty::List(elem), _) => {
                first = self.dbg_type(elem);
                LIST
            }
            (Ty::Set(elem), _) => {
                first = self.dbg_type(elem);
                SET
            }
            (Ty::Dict(k, v), _) => {
                (first, second) = (self.dbg_type(k), self.dbg_type(v));
                DICT
            }
            (Ty::Opt(inner), _) => {
                first = self.dbg_type(inner);
                // (Where the value is, after the flag that says there is one.)
                let lt = self.llty(t).unwrap().into_struct_type();
                count = self.td.offset_of_element(&lt, 1).unwrap_or(0);
                OPT
            }
            (Ty::Struct(id), _) => {
                let fields = self.checked.structs[*id].fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
                let lt = self.struct_ty(*id);
                (count, items) = parts(self, lt, fields);
                STRUCT
            }
            (Ty::Enum(id), _) => {
                // Each case: its name, and what its values are (if any).
                let cases = self.checked.enums[*id].cases.clone();
                let field_ty = self.ctx.struct_type(&[p.into(), i.into(), p.into()], false);
                let described: Vec<_> = cases
                    .iter()
                    .enumerate()
                    .map(|(n, case)| {
                        let values = if case.fields.is_empty() {
                            null
                        } else {
                            let key = format!("P{key}_{n}");
                            self.dbg_type_for(&key, t, Some(n))
                        };
                        let name = self.c_str(&case.name);
                        field_ty.const_named_struct(&[name.into(), i.const_zero().into(), values.into()])
                    })
                    .collect();
                let array = field_ty.const_array(&described);
                let list = self.module.add_global(array.get_type(), None, "dbgcases");
                list.set_linkage(Linkage::Private);
                list.set_constant(true);
                list.set_initializer(&array);
                (count, items) = (cases.len() as u64, list.as_pointer_value());
                ENUM
            }
            _ => OTHER,
        };
        let name = self.c_str(&name);
        global.set_initializer(&ty.const_named_struct(&[
            i.const_int(kind, false).into(),
            name.into(),
            i.const_int(size, false).into(),
            i.const_int(count, false).into(),
            items.into(),
            first.into(),
            second.into(),
        ]));
        global.as_pointer_value()
    }

    /// The name the stack of calls shows for a block `{ … }` in the function
    /// being compiled.
    pub(super) fn debug_block_name(&self) -> String {
        match self.stack.last().and_then(|c| c.dbg.as_ref()).map(|d| &d.name) {
            Some(outer) if outer.starts_with("block in ") => outer.clone(),
            Some(outer) => format!("block in {outer}"),
            None => "block".to_string(),
        }
    }
}

/// Where a statement is, for the line the debugger shows.
pub(super) fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => d.span,
        Stmt::Fn(f) => f.name.span,
        Stmt::Assign { span, .. } | Stmt::Return { span, .. } | Stmt::For { span, .. } | Stmt::While { span, .. } => *span,
        Stmt::Break(span) | Stmt::Continue(span) => *span,
        Stmt::Expr(e) => e.span,
    }
}
