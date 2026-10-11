//! Constants, and functions that wait (`async fn`, `await`).
//!
//! A function that can `await` is an LLVM coroutine: what it keeps while it
//! waits is in a frame on the heap, and LLVM's coroutine passes split it
//! into the part that starts it and the part that goes on.
//!
//! Waiting is built on completion blocks. To its callers, an `async fn` is a
//! function whose last parameter is the block that gets its result
//! (`DONE_PARAM`), and `await call(…)` gives the call such a block: a
//! *continuation*, which stores the result in the waiting function's frame
//! and resumes it. So anything that ends with a completion block can be
//! awaited: `fetch`, `after`, `background`, and a program's own functions.
//!
//! A continuation is `{ thunk, box }`. The box holds the coroutine's handle,
//! where its state and the result go, and whether it's still expected to be
//! called. The state (one per coroutine) says how the awaited call is doing:
//!
//! - 0: the call hasn't returned yet
//! - 1: the coroutine is suspended, waiting for the block to be called
//! - 2: the block was called (the result is there)
//! - 3: the block was dropped, uncalled: nothing will ever go on
//!
//! A block that's dropped without having been called (a cancelled request, a
//! timer that's stopped) destroys the coroutine waiting for it, which
//! releases what it holds: its own completion block too, so whatever waits
//! for *it* goes the same way.

use inkwell::basic_block::BasicBlock;
use inkwell::llvm_sys::core::{
    LLVMBuildCall2, LLVMConstNull, LLVMCountParamTypes, LLVMGetIntrinsicDeclaration, LLVMGlobalGetValueType,
    LLVMLookupIntrinsicID, LLVMTokenTypeInContext,
};
use inkwell::llvm_sys::prelude::{LLVMTypeRef, LLVMValueRef};
use inkwell::module::Linkage;
use inkwell::types::{AsTypeRef, StructType};
use inkwell::values::{AsValueRef, FunctionValue, IntValue, PointerValue};
use inkwell::IntPredicate;

use super::{FnCtx, Gen, Store, Val, Var};
use crate::ast::*;
use crate::source::Span;
use crate::types::Ty;

/// A function that can `await`, while it's compiled.
#[derive(Clone)]
pub(super) struct Coro<'ctx> {
    handle: PointerValue<'ctx>,
    /// How the awaited call is doing (see the module's notes).
    state: PointerValue<'ctx>,
    /// Frees the frame, then leaves.
    free: BasicBlock<'ctx>,
    /// Leaves (the frame stays, or is gone already).
    leave: BasicBlock<'ctx>,
    /// An `async fn`'s completion block (in a place of its own, kept until
    /// the function is done), and its type.
    done: Option<(PointerValue<'ctx>, Ty)>,
    /// Boxes kept for as long as the function goes on: a block's
    /// environment, a view's state.
    keep: Vec<PointerValue<'ctx>>,
}

/// An awaited call being evaluated: whose it is, and where its result goes.
pub(super) struct Awaited<'ctx> {
    func: FunctionValue<'ctx>,
    result: Option<PointerValue<'ctx>>,
}

/// Does `b` wait itself (not counting the blocks and functions inside it,
/// which wait on their own)?
pub(super) fn awaits(b: &Block) -> bool {
    fn in_expr(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Await(_) => true,
            ExprKind::Closure { .. } => false,
            ExprKind::Block(b) => in_block(b),
            ExprKind::If { cond, then, otherwise } => {
                let (Cond::Expr(c) | Cond::Let(_, _, c)) = cond;
                in_expr(c)
                    || in_block(then)
                    || match otherwise {
                        Some(Else::Block(b)) => in_block(b),
                        Some(Else::If(x)) => in_expr(x),
                        None => false,
                    }
            }
            ExprKind::Match { subject, arms } => in_expr(subject) || arms.iter().any(|a| in_expr(&a.body)),
            _ => {
                let mut found = false;
                super::for_each_child(e, &mut |c| found |= in_expr(c));
                found
            }
        }
    }
    fn in_block(b: &Block) -> bool {
        b.stmts.iter().any(|s| match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => in_expr(&d.value),
            Stmt::Assign { target, value, .. } => in_expr(target) || in_expr(value),
            Stmt::Return { value, .. } => value.as_ref().is_some_and(in_expr),
            Stmt::For { iter, body, .. } => in_expr(iter) || in_block(body),
            Stmt::While { cond, body, .. } => {
                let (Cond::Expr(c) | Cond::Let(_, _, c)) = cond;
                in_expr(c) || in_block(body)
            }
            Stmt::Expr(e) => in_expr(e),
            Stmt::Fn(_) | Stmt::Break(_) | Stmt::Continue(_) => false,
        })
    }
    in_block(b)
}

impl<'ctx> Coro<'ctx> {
    /// Where the function's frame is freed, whichever way it ends.
    pub(super) fn free_block(&self) -> BasicBlock<'ctx> {
        self.free
    }
}

impl<'ctx, 'a> Gen<'ctx, 'a> {
    // ===== constants =====

    /// The value of constant `name` (owned): a call of the function that
    /// keeps it.
    pub(super) fn constant(&mut self, name: &str) -> Option<Val<'ctx>> {
        let f = self.fns[name];
        let ret = self.checked.fns[name].ret.clone();
        let mut all = vec![self.ptr_ty().const_null().into()];
        let place = self.result_place(&ret, &mut all);
        let r = self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic();
        self.call_result(&ret, place, r)
    }

    /// The function of a constant: works its value out the first time it's
    /// called, and gives (a reference to) that value from then on.
    pub(super) fn constant_body(&mut self, f: FunctionValue<'ctx>, decl: &FnDecl, t: &Ty) {
        let Some(lt) = self.llty(t) else { return };
        let name = &decl.name.name;
        let value = self.module.add_global(lt, None, &format!("tsl_k_{name}"));
        value.set_linkage(Linkage::Internal);
        value.set_initializer(&lt.const_zero());
        // 0: not worked out yet; 1: being worked out; 2: there.
        let state = self.module.add_global(self.i8(), None, &format!("tsl_ks_{name}"));
        state.set_linkage(Linkage::Internal);
        state.set_initializer(&self.i8().const_zero());
        let (value, state) = (value.as_pointer_value(), state.as_pointer_value());

        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        self.stack.push(FnCtx {
            func: f,
            scopes: vec![Vec::new()],
            ret: t.clone(),
            self_ptr: None,
            boxed: self.boxed_vars(&decl.body),
            record: None,
            coro: None,
            dbg: None,
        });
        let (work, ready) = (self.block_named("work"), self.block_named("ready"));
        let s = self.b.build_load(self.i8(), state, "").unwrap().into_int_value();
        let there = self.b.build_int_compare(IntPredicate::EQ, s, self.i8().const_int(2, false), "").unwrap();
        self.b.build_conditional_branch(there, ready, work).unwrap();

        self.b.position_at_end(work);
        let busy = self.b.build_int_compare(IntPredicate::EQ, s, self.i8().const_int(1, false), "").unwrap();
        self.panic_if(busy, &format!("the constant `{name}` is used while its own value is worked out"), decl.name.span);
        self.b.build_store(state, self.i8().const_int(1, false)).unwrap();
        let v = self.block(&decl.body, Some(t));
        if !self.terminated() {
            self.store(t, value, v.unwrap());
            // Never freed, and safe to read from background work.
            if let Some(helper) = self.immortal_helper(t) {
                self.b.build_call(helper, &[value.into()], "").unwrap();
            }
            self.b.build_store(state, self.i8().const_int(2, false)).unwrap();
            self.b.build_unconditional_branch(ready).unwrap();
        }

        self.b.position_at_end(ready);
        let v = self.load(t, value);
        self.retain(v, t);
        self.emit_return(Some(v));
        self.stack.pop();
    }

    /// `void helper(ptr)`: makes what the value at `ptr` holds immortal (see
    /// the runtime's `tsl_immortal`). None if it holds nothing counted, or
    /// only what can't be followed (functions, interface values), which
    /// stays counted.
    fn immortal_helper(&mut self, t: &Ty) -> Option<FunctionValue<'ctx>> {
        let follows = |g: &Self, t: &Ty| g.needs_rc(t) && !matches!(t, Ty::Fn(..) | Ty::View | Ty::Iface(_));
        if !follows(self, t) {
            return None;
        }
        let name = format!("tsl_h_immortal_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return Some(*f);
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let t = t.clone();
        let fn_or_null = |g: &mut Self, t: &Ty| match g.immortal_helper(t) {
            Some(h) => h.as_global_value().as_pointer_value(),
            None => g.ptr_ty().const_null(),
        };
        self.in_helper(f, |g| {
            let at = f.get_nth_param(0).unwrap().into_pointer_value();
            match &t {
                Ty::Str | Ty::Data => {
                    let obj = g.b.build_load(p, at, "").unwrap();
                    let rt = g.declare_rt("tsl_immortal", &[p.into()], None);
                    g.b.build_call(rt, &[obj.into()], "").unwrap();
                }
                Ty::List(elem) => {
                    let obj = g.b.build_load(p, at, "").unwrap();
                    let each = fn_or_null(g, elem);
                    let rt = g.declare_rt("tsl_list_immortal", &[p.into(), p.into()], None);
                    g.b.build_call(rt, &[obj.into(), each.into()], "").unwrap();
                }
                Ty::Dict(..) | Ty::Set(_) => {
                    let (k, v) = match &t {
                        Ty::Dict(k, v) => (fn_or_null(g, k), fn_or_null(g, v)),
                        Ty::Set(k) => (fn_or_null(g, k), p.const_null()),
                        _ => unreachable!(),
                    };
                    let obj = g.b.build_load(p, at, "").unwrap();
                    let rt = g.declare_rt("tsl_dict_immortal", &[p.into(), p.into(), p.into()], None);
                    g.b.build_call(rt, &[obj.into(), k.into(), v.into()], "").unwrap();
                }
                Ty::Struct(id) => {
                    let st = g.struct_ty(*id);
                    let fields: Vec<Ty> = g.checked.structs[*id].fields.iter().map(|f| f.ty.clone()).collect();
                    for (i, ft) in fields.iter().enumerate() {
                        if let Some(h) = g.immortal_helper(ft) {
                            let fp = g.b.build_struct_gep(st, at, i as u32, "").unwrap();
                            g.b.build_call(h, &[fp.into()], "").unwrap();
                        }
                    }
                }
                Ty::Opt(inner) => {
                    if let Some(h) = g.immortal_helper(inner) {
                        let st = g.llty(&t).unwrap().into_struct_type();
                        let flag = g.b.build_struct_gep(st, at, 0, "").unwrap();
                        let present = g.b.build_load(g.ctx.bool_type(), flag, "").unwrap().into_int_value();
                        let (some, none) = (g.block_named("some"), g.block_named("none"));
                        g.b.build_conditional_branch(present, some, none).unwrap();
                        g.b.position_at_end(some);
                        let ip = g.b.build_struct_gep(st, at, 1, "").unwrap();
                        g.b.build_call(h, &[ip.into()], "").unwrap();
                        g.b.build_unconditional_branch(none).unwrap();
                        g.b.position_at_end(none);
                    }
                }
                Ty::Enum(id) => {
                    // The case's values (in its box), then the box.
                    let st = g.llty(&t).unwrap().into_struct_type();
                    let tag = g.b.build_struct_gep(st, at, 0, "").unwrap();
                    let tag = g.b.build_load(g.ctx.i32_type(), tag, "").unwrap().into_int_value();
                    let bp = g.b.build_struct_gep(st, at, 1, "").unwrap();
                    let bx = g.b.build_load(p, bp, "").unwrap().into_pointer_value();
                    let done = g.block_named("done");
                    let cases = g.checked.enums[*id].cases.clone();
                    let mut arms = Vec::new();
                    for (ci, case) in cases.iter().enumerate() {
                        if case.fields.is_empty() {
                            continue;
                        }
                        let bb = g.block_named("case");
                        arms.push((g.ctx.i32_type().const_int(ci as u64, false), bb));
                        g.b.position_at_end(bb);
                        let pt = g.payload_ty(*id, ci);
                        let data = g.box_data(bx);
                        for (fi, field) in case.fields.iter().enumerate() {
                            if let Some(h) = g.immortal_helper(&field.ty) {
                                let fp = g.b.build_struct_gep(pt, data, fi as u32, "").unwrap();
                                g.b.build_call(h, &[fp.into()], "").unwrap();
                            }
                        }
                        let rt = g.declare_rt("tsl_immortal", &[p.into()], None);
                        g.b.build_call(rt, &[bx.into()], "").unwrap();
                        g.b.build_unconditional_branch(done).unwrap();
                    }
                    let entry = f.get_first_basic_block().unwrap();
                    g.b.position_at_end(entry);
                    g.b.build_switch(tag, done, &arms).unwrap();
                    g.b.position_at_end(done);
                }
                _ => {}
            }
            g.b.build_return(None).unwrap();
        });
        Some(f)
    }

    /// A block that does nothing: `{ }`, as a value.
    pub(super) fn nothing_block(&mut self) -> Val<'ctx> {
        let name = "tsl_h_nothing";
        let f = match self.helpers.get(name) {
            Some(f) => *f,
            None => {
                let ty = self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false);
                let f = self.module.add_function(name, ty, Some(Linkage::Internal));
                self.helpers.insert(name.to_string(), f);
                let b = self.ctx.create_builder();
                b.position_at_end(self.ctx.append_basic_block(f, "entry"));
                b.build_return(None).unwrap();
                f
            }
        };
        let st = self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false);
        st.const_named_struct(&[f.as_global_value().as_pointer_value().into(), self.ptr_ty().const_null().into()]).into()
    }

    // ===== coroutines =====

    /// Calls an LLVM intrinsic (the coroutine ones use token values, which
    /// inkwell has no type for).
    fn intrinsic(&self, name: &str, overload: &[LLVMTypeRef], args: &[LLVMValueRef]) -> LLVMValueRef {
        unsafe {
            let id = LLVMLookupIntrinsicID(name.as_ptr().cast(), name.len());
            assert!(id != 0, "LLVM has no `{name}`");
            let mut overload = overload.to_vec();
            let f = LLVMGetIntrinsicDeclaration(self.module.as_mut_ptr(), id, overload.as_mut_ptr(), overload.len());
            let ty = LLVMGlobalGetValueType(f);
            // (LLVM versions differ in whether `coro.end` takes a token.)
            let mut args = args.to_vec();
            args.truncate(LLVMCountParamTypes(ty) as usize);
            LLVMBuildCall2(self.b.as_mut_ptr(), ty, f, args.as_mut_ptr(), args.len() as u32, c"".as_ptr())
        }
    }

    fn no_token(&self) -> LLVMValueRef {
        unsafe { LLVMConstNull(LLVMTokenTypeInContext(self.ctx.raw())) }
    }

    /// Makes the function being started (its context is pushed, and the
    /// builder is in its entry block) a coroutine. It keeps the boxes in
    /// `keep` until it's done.
    pub(super) fn begin_coro(&mut self, keep: &[PointerValue<'ctx>]) {
        self.has_coros = true;
        let func = self.func();
        let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("presplitcoroutine");
        func.add_attribute(inkwell::attributes::AttributeLoc::Function, self.ctx.create_enum_attribute(kind, 0));
        let (p, i1, i32t) = (self.ptr_ty(), self.ctx.bool_type(), self.ctx.i32_type());
        let null = p.const_null().as_value_ref();
        let id = self.intrinsic("llvm.coro.id", &[], &[i32t.const_zero().as_value_ref(), null, null, null]);
        let size = self.intrinsic("llvm.coro.size", &[self.i64().as_type_ref()], &[]);
        let alloc = self.declare_rt("tsl_frame_new", &[self.i64().into()], Some(p.into()));
        let size = unsafe { IntValue::new(size) };
        let frame = self.b.build_call(alloc, &[size.into()], "").unwrap().try_as_basic_value().basic().unwrap();
        let handle = self.intrinsic("llvm.coro.begin", &[], &[id, frame.as_value_ref()]);
        let handle = unsafe { PointerValue::new(handle) };
        let state = self.alloca(self.i8().into(), "awaiting");
        for k in keep {
            self.call_rt("tsl_retain", &[(*k).into()]);
        }
        let body = self.b.get_insert_block().unwrap();

        let leave = self.block_named("leave");
        self.b.position_at_end(leave);
        self.intrinsic("llvm.coro.end", &[], &[null, i1.const_zero().as_value_ref(), self.no_token()]);
        self.b.build_return(None).unwrap();

        let free = self.block_named("free");
        self.b.position_at_end(free);
        let mem = self.intrinsic("llvm.coro.free", &[], &[id, handle.as_value_ref()]);
        let release = self.declare_rt("tsl_frame_free", &[p.into()], None);
        self.b.build_call(release, &[unsafe { PointerValue::new(mem) }.into()], "").unwrap();
        self.b.build_unconditional_branch(leave).unwrap();

        self.b.position_at_end(body);
        self.ctx_mut().coro = Some(Coro { handle, state, free, leave, done: None, keep: keep.to_vec() });
    }

    /// In a method that waits (or a block in a method): works on a copy of
    /// the receiver, since the caller's may be gone when it goes on.
    pub(super) fn own_receiver(&mut self) {
        let Some((at, owner)) = self.stack.last().unwrap().self_ptr.clone() else { return };
        let v = self.load(&owner, at);
        self.retain(v, &owner);
        let lt = self.llty(&owner).unwrap();
        let slot = self.alloca(lt, "self");
        self.store(&owner, slot, v);
        self.ctx_mut().self_ptr = Some((slot, owner.clone()));
        // (Released with the function's variables.)
        self.ctx_mut().scopes[0].push(Var { name: "$self".into(), ty: owner, store: Store::Slot(slot) });
    }

    /// In an `async fn`, once its parameters are declared: keeps its
    /// completion block for when it's done.
    pub(super) fn take_done(&mut self) {
        let var = self.lookup(DONE_PARAM).expect("an async function's completion block");
        let at = self.var_read_ptr(&var, Span::default());
        let block = self.load(&var.ty, at);
        self.retain(block, &var.ty);
        let lt = self.llty(&var.ty).unwrap();
        let slot = self.alloca(lt, "done");
        self.b.build_store(slot, block).unwrap();
        self.ctx_mut().coro.as_mut().expect("a coroutine").done = Some((slot, var.ty));
    }

    /// Lets go of what a coroutine keeps besides its variables, without
    /// calling its completion block.
    fn drop_kept(&mut self, c: &Coro<'ctx>) {
        if let Some((slot, ft)) = &c.done {
            let block = self.load(ft, *slot);
            self.release(block, ft);
        }
        for k in &c.keep {
            self.call_rt("tsl_release", &[(*k).into()]);
        }
    }

    /// The end of a coroutine (its variables are released already): gives
    /// the result to the completion block, and frees the frame.
    pub(super) fn finish_coro(&mut self, c: &Coro<'ctx>, v: Option<Val<'ctx>>) {
        self.debug_done();
        if let Some((slot, ft)) = &c.done {
            let Ty::Fn(ps, _) = ft else { unreachable!() };
            let block = self.load(ft, *slot);
            let args: Vec<Val<'ctx>> = if ps.is_empty() { Vec::new() } else { vec![v.expect("a result")] };
            self.call_fn_value(block, ft, &args);
        }
        self.drop_kept(c);
        self.b.build_unconditional_branch(c.free).unwrap();
    }

    /// The box of a continuation: `{ handle, state, result, expected }`.
    fn continuation_ty(&self) -> StructType<'ctx> {
        let p = self.ptr_ty();
        self.ctx.struct_type(&[p.into(), p.into(), p.into(), self.i8().into()], false)
    }

    /// `await call(…)`.
    pub(super) fn await_expr(&mut self, e: &Expr, call: &Expr) -> Option<Val<'ctx>> {
        let c = self.stack.last().unwrap().coro.clone().expect("checked: `await` is in a function that can wait");
        let t = self.ty(e);
        let i8t = self.i8();
        let result = match self.llty(&t) {
            Some(lt) => Some(self.alloca(lt, "awaited")),
            None => None,
        };
        self.b.build_store(c.state, i8t.const_zero()).unwrap();
        self.awaits.push(Awaited { func: self.func(), result });
        // (What the call itself returns, like a request's number, isn't used.)
        let ct = self.ty(call);
        if let Some(v) = self.expr(call) {
            self.release(v, &ct);
        }
        self.awaits.pop();

        let (wait, gone, on) = (self.block_named("wait"), self.block_named("gone"), self.block_named("on"));
        let s = self.b.build_load(i8t, c.state, "").unwrap().into_int_value();
        self.b.build_switch(s, wait, &[(i8t.const_int(2, false), on), (i8t.const_int(3, false), gone)]).unwrap();

        self.b.position_at_end(wait);
        self.b.build_store(c.state, i8t.const_int(1, false)).unwrap();
        // (For the debugger, the function is off the stack of calls while
        // it waits, and back on it when it goes on, or is done away with.)
        self.debug_park();
        let no = self.ctx.bool_type().const_zero().as_value_ref();
        let woke = self.intrinsic("llvm.coro.suspend", &[], &[self.no_token(), no]);
        let woke = unsafe { IntValue::new(woke) };
        let (back_on, back_gone) = if self.debugged() {
            let blocks = (self.block_named("back"), self.block_named("back_gone"));
            for (from, to) in [(blocks.0, on), (blocks.1, gone)] {
                self.b.position_at_end(from);
                self.debug_resume();
                self.b.build_unconditional_branch(to).unwrap();
            }
            self.b.position_at_end(wait);
            blocks
        } else {
            (on, gone)
        };
        self.b.build_switch(woke, c.leave, &[(i8t.const_zero(), back_on), (i8t.const_int(1, false), back_gone)]).unwrap();

        // Nothing will ever go on from here: let go of everything.
        self.b.position_at_end(gone);
        self.release_held(0);
        self.release_all_scopes();
        self.drop_kept(&c);
        self.b.build_unconditional_branch(c.free).unwrap();

        self.b.position_at_end(on);
        result.map(|at| self.load(&t, at))
    }

    /// The completion block of the awaited call being evaluated (of type
    /// `t`): stores what it's given, and wakes the function that waits.
    pub(super) fn continuation(&mut self, t: &Ty) -> Val<'ctx> {
        let Ty::Fn(ps, _) = t else { unreachable!("checked: a completion block") };
        let c = self.stack.last().unwrap().coro.clone().expect("a function that can wait");
        let site = self.awaits.last().expect("an awaited call");
        assert!(site.func == self.func(), "a continuation outside its function");
        let result = site.result.unwrap_or_else(|| self.ptr_ty().const_null());
        let info = self.continuation_info();
        let bx = self.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
        let (data, st) = (self.box_data(bx), self.continuation_ty());
        let fields: [Val<'ctx>; 4] = [c.handle.into(), c.state.into(), result.into(), self.i8().const_int(1, false).into()];
        for (i, v) in fields.into_iter().enumerate() {
            let fp = self.b.build_struct_gep(st, data, i as u32, "").unwrap();
            self.b.build_store(fp, v).unwrap();
        }
        let thunk = self.continuation_thunk(ps.first());
        let block = self.llty(t).unwrap().into_struct_type();
        let v = self.b.build_insert_value(block.get_undef(), thunk.as_global_value().as_pointer_value(), 0, "").unwrap();
        self.b.build_insert_value(v, bx, 1, "").unwrap().into_struct_value().into()
    }

    /// `void thunk(box, value)`: what a continuation runs when it's called.
    fn continuation_thunk(&mut self, t: Option<&Ty>) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_resume_{}", t.map_or("v".to_string(), |t| self.key(t)));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let params: Vec<Ty> = t.into_iter().cloned().collect();
        let ty = self.fn_type(&params, &Ty::Void, false);
        let f = self.module.add_function(&name, ty, Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let (st, p, i8t) = (self.continuation_ty(), self.ptr_ty(), self.i8());
        let t = t.cloned();
        self.in_helper(f, |g| {
            let data = g.box_data(f.get_nth_param(0).unwrap().into_pointer_value());
            let expected_at = g.b.build_struct_gep(st, data, 3, "").unwrap();
            let expected = g.b.build_load(i8t, expected_at, "").unwrap().into_int_value();
            let expected = g.b.build_int_compare(IntPredicate::NE, expected, i8t.const_zero(), "").unwrap();
            let (first, again) = (g.block_named("first"), g.block_named("again"));
            g.b.build_conditional_branch(expected, first, again).unwrap();

            // Called a second time: there's no one to give the value to.
            g.b.position_at_end(again);
            if let Some(t) = &t {
                let v = g.abi_param(f.get_nth_param(1).unwrap(), t);
                g.release(v, t);
            }
            g.b.build_return(None).unwrap();

            g.b.position_at_end(first);
            g.b.build_store(expected_at, i8t.const_zero()).unwrap();
            if let Some(t) = &t {
                let v = g.abi_param(f.get_nth_param(1).unwrap(), t);
                let to = g.b.build_struct_gep(st, data, 2, "").unwrap();
                let to = g.b.build_load(p, to, "").unwrap().into_pointer_value();
                g.store(t, to, v);
            }
            let state = g.b.build_struct_gep(st, data, 1, "").unwrap();
            let state = g.b.build_load(p, state, "").unwrap().into_pointer_value();
            let s = g.b.build_load(i8t, state, "").unwrap().into_int_value();
            g.b.build_store(state, i8t.const_int(2, false)).unwrap();
            // (Called before the awaited call even returned: the function
            // hasn't stopped, and finds the result when it looks.)
            let waiting = g.b.build_int_compare(IntPredicate::EQ, s, i8t.const_int(1, false), "").unwrap();
            let (resume, out) = (g.block_named("resume"), g.block_named("out"));
            g.b.build_conditional_branch(waiting, resume, out).unwrap();
            g.b.position_at_end(resume);
            let handle = g.b.build_struct_gep(st, data, 0, "").unwrap();
            let handle = g.b.build_load(p, handle, "").unwrap();
            g.intrinsic("llvm.coro.resume", &[], &[handle.as_value_ref()]);
            g.b.build_unconditional_branch(out).unwrap();
            g.b.position_at_end(out);
            g.b.build_return(None).unwrap();
        });
        f
    }

    /// Type info for a continuation's box. When the box is freed without
    /// the continuation having been called, the function waiting for it is
    /// destroyed (or told that it needn't wait, if it's still running).
    fn continuation_info(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_t_continuation";
        if let Some(g) = self.module.get_global(name) {
            return g.as_pointer_value();
        }
        let (st, p, i8t, i) = (self.continuation_ty(), self.ptr_ty(), self.i8(), self.i64());
        let f = self.module.add_function(
            "tsl_h_continuation_drop",
            self.ctx.void_type().fn_type(&[p.into()], false),
            Some(Linkage::Internal),
        );
        self.in_helper(f, |g| {
            let data = f.get_nth_param(0).unwrap().into_pointer_value();
            let expected_at = g.b.build_struct_gep(st, data, 3, "").unwrap();
            let expected = g.b.build_load(i8t, expected_at, "").unwrap().into_int_value();
            let expected = g.b.build_int_compare(IntPredicate::NE, expected, i8t.const_zero(), "").unwrap();
            let (uncalled, out) = (g.block_named("uncalled"), g.block_named("out"));
            g.b.build_conditional_branch(expected, uncalled, out).unwrap();
            g.b.position_at_end(uncalled);
            g.b.build_store(expected_at, i8t.const_zero()).unwrap();
            let state = g.b.build_struct_gep(st, data, 1, "").unwrap();
            let state = g.b.build_load(p, state, "").unwrap().into_pointer_value();
            let s = g.b.build_load(i8t, state, "").unwrap().into_int_value();
            let waiting = g.b.build_int_compare(IntPredicate::EQ, s, i8t.const_int(1, false), "").unwrap();
            let (destroy, tell) = (g.block_named("destroy"), g.block_named("tell"));
            g.b.build_conditional_branch(waiting, destroy, tell).unwrap();
            g.b.position_at_end(destroy);
            let handle = g.b.build_struct_gep(st, data, 0, "").unwrap();
            let handle = g.b.build_load(p, handle, "").unwrap();
            g.intrinsic("llvm.coro.destroy", &[], &[handle.as_value_ref()]);
            g.b.build_unconditional_branch(out).unwrap();
            g.b.position_at_end(tell);
            g.b.build_store(state, i8t.const_int(3, false)).unwrap();
            g.b.build_unconditional_branch(out).unwrap();
            g.b.position_at_end(out);
            g.b.build_return(None).unwrap();
        });
        let ty = self.ctx.struct_type(&[i.into(), i.into(), p.into(), p.into(), p.into(), p.into(), p.into(), i.into()], false);
        let g = self.module.add_global(ty, None, name);
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&ty.const_named_struct(&[
            i.const_int(self.td.get_abi_size(&st), false).into(),
            i.const_int(8, false).into(),
            p.const_null().into(),
            f.as_global_value().as_pointer_value().into(),
            p.const_null().into(),
            p.const_null().into(),
            p.const_null().into(),
            i.const_zero().into(),
        ]));
        g.as_pointer_value()
    }
}
