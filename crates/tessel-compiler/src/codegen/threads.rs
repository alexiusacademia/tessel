//! Values for more than one thread: `Shared<T>`, and the workers of
//! `serveHTTP(…, workers:)`.

use std::collections::BTreeSet;

use inkwell::module::Linkage;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::PointerValue;

use super::{free_block, Gen, Val, Var};
use crate::ast::{Arg, ExprKind};
use crate::builtins;
use crate::types::Ty;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    // ===== Shared<T> =====

    /// `Shared(value)`: a new object holding the value (marked shared).
    pub(super) fn shared_init(&mut self, args: &[crate::ast::Arg], t: &Ty) -> Val<'ctx> {
        let Ty::Shared(e) = t else { unreachable!() };
        let e = (**e).clone();
        let v = self.expr_as(&args[0].value, &e).unwrap();
        let p = self.ptr_ty();
        let size = self.llty(&e).unwrap().size_of().unwrap();
        let drop = self.cell_release_helper(&e);
        let new = self.declare_rt("tsl_shared_new", &[self.i64().into(), p.into()], Some(p.into()));
        let obj = self.b.build_call(new, &[size.into(), drop.into()], "").unwrap().try_as_basic_value().basic().unwrap();
        let cell_of = self.declare_rt("tsl_shared_cell", &[p.into()], Some(p.into()));
        let cell = self.b.build_call(cell_of, &[obj.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
        self.store(&e, cell, v);
        if let Some(h) = self.share_helper(&e) {
            self.b.build_call(h, &[cell.into()], "").unwrap();
        }
        obj
    }

    /// `void helper(ptr)`: releases the value at `ptr` (what a `Shared`
    /// holds, when it's freed); null if the type isn't counted.
    fn cell_release_helper(&mut self, t: &Ty) -> PointerValue<'ctx> {
        if !self.needs_rc(t) {
            return self.ptr_ty().const_null();
        }
        let name = format!("tsl_h_cell_release_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let t = t.clone();
        self.in_helper(f, |g| {
            let at = f.get_nth_param(0).unwrap().into_pointer_value();
            let v = g.load(&t, at);
            g.release(v, &t);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `shared.get()`, `shared.set(value)`, `shared.update { v in … }`.
    pub(super) fn shared_method(
        &mut self,
        recv: &Ty,
        name: &str,
        object: &crate::ast::Expr,
        args: &[crate::ast::Arg],
        trailing: Option<&crate::ast::Expr>,
    ) -> Option<Val<'ctx>> {
        let Ty::Shared(e) = recv else { unreachable!() };
        let e = (**e).clone();
        let p = self.ptr_ty();
        let obj = self.expr_as(object, recv).unwrap();
        self.hold(obj, recv);
        let lock = self.declare_rt("tsl_shared_lock", &[p.into()], Some(p.into()));
        let unlock = self.declare_rt("tsl_shared_unlock", &[p.into()], None);
        let r = match name {
            "get" => {
                let cell = self.b.build_call(lock, &[obj.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
                let v = self.load(&e, cell);
                self.retain(v, &e);
                self.b.build_call(unlock, &[obj.into()], "").unwrap();
                Some(v)
            }
            "set" => {
                let v = self.expr_as(&args[0].value, &e).unwrap();
                let v = self.marked_shared(&e, v);
                let cell = self.b.build_call(lock, &[obj.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
                let old = self.load(&e, cell);
                self.store(&e, cell, v);
                self.b.build_call(unlock, &[obj.into()], "").unwrap();
                self.release(old, &e);
                None
            }
            _ => {
                // The block runs with the lock held, on the value as it is;
                // what it gives is stored (and given back) when it ends.
                let ft = Ty::func(vec![e.clone()], e.clone());
                let block = args.first().map(|a| &a.value).or(trailing).unwrap();
                let f = self.expr_as(block, &ft).unwrap();
                self.hold(f, &ft);
                let cell = self.b.build_call(lock, &[obj.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
                let old = self.load(&e, cell);
                self.retain(old, &e);
                let new = self.call_fn_value(f, &ft, &[old]).unwrap();
                let new = self.marked_shared(&e, new);
                // (What's there now: the block may have `set` it.)
                let stored = self.load(&e, cell);
                self.store(&e, cell, new);
                self.retain(new, &e);
                self.b.build_call(unlock, &[obj.into()], "").unwrap();
                self.release(stored, &e);
                self.unhold(1);
                self.release(f, &ft);
                Some(new)
            }
        };
        self.unhold(1);
        self.release(obj, recv);
        r
    }

    /// `v` (owned) with what it holds marked shared.
    fn marked_shared(&mut self, t: &Ty, v: Val<'ctx>) -> Val<'ctx> {
        match self.share_helper(t) {
            Some(h) => {
                let at = self.spill(t, v);
                self.b.build_call(h, &[at.into()], "").unwrap();
                self.load(t, at)
            }
            None => v,
        }
    }

    // ===== serveHTTP with workers =====

    /// `serveHTTP(port:host:workers:start:)`. What `start` uses is put in a
    /// cell once (as `background` does) and kept: each worker reads it,
    /// without taking it, to make its own block, so a worker that fails can
    /// be replaced from it.
    pub(super) fn serve_workers(&mut self, args: &[Arg], trailing: Option<&crate::ast::Expr>) -> Val<'ctx> {
        let labeled = |l: &str| args.iter().find(|a| a.label.as_ref().is_some_and(|x| x.name == l)).map(|a| &a.value);
        let p = self.ptr_ty();
        let i64t = self.i64();
        let port = self.expr_as(labeled("port").unwrap(), &Ty::Int).unwrap();
        let host = match labeled("host") {
            Some(h) => self.expr_as(h, &Ty::Str).unwrap(),
            None => self.str_lit("127.0.0.1").into(),
        };
        self.hold(host, &Ty::Str);
        let workers = match labeled("workers") {
            Some(w) => self.expr_as(w, &Ty::Int).unwrap(),
            None => i64t.const_int(1, false).into(),
        };
        let start = labeled("start").or(trailing).unwrap();
        let ExprKind::Closure { body, params } = &start.kind else { unreachable!("checked: `start` is a block") };
        let handler_t = Ty::func(vec![builtins::SERVER_REQUEST], Ty::Void);
        let start_t = Ty::func(vec![], handler_t.clone());

        // What `start` uses from here, in a cell: `{ tree, values… }`.
        let mut free = BTreeSet::new();
        free_block(body, &mut params.iter().map(|p| p.name.clone()).collect(), &mut free);
        let captures: Vec<Var<'ctx>> = free.iter().filter_map(|n| self.lookup(n)).collect();
        let (shared, copied): (Vec<&Var<'ctx>>, Vec<&Var<'ctx>>) = captures.iter().partition(|v| self.goes_shared(&v.ty));
        let tree = self.tree_call("tsl_tree_array", &[], Some("ptr"), &[]).unwrap().into_pointer_value();
        let span = start.span;
        for v in &copied {
            let vp = self.var_read_ptr(v, span);
            let enc = self.enc_helper(&v.ty);
            let item = self.b.build_call(enc, &[vp.into()], "").unwrap().try_as_basic_value().basic().unwrap();
            self.tree_call("tsl_tree_push", &["ptr", "ptr"], None, &[tree.into(), item]);
        }
        let mut cell_fields: Vec<BasicTypeEnum<'ctx>> = vec![p.into()];
        cell_fields.extend(shared.iter().map(|v| self.llty(&v.ty).unwrap()));
        let cell_st = self.ctx.struct_type(&cell_fields, false);
        let new_cell = self.declare_rt("tsl_frame_new", &[i64t.into()], Some(p.into()));
        let input = self.b.build_call(new_cell, &[cell_st.size_of().unwrap().into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
        let tp = self.b.build_struct_gep(cell_st, input, 0, "").unwrap();
        self.b.build_store(tp, tree).unwrap();
        for (i, v) in shared.iter().enumerate() {
            let vp = self.var_read_ptr(v, span);
            let val = self.load(&v.ty, vp);
            self.retain(val, &v.ty);
            let fp = self.b.build_struct_gep(cell_st, input, i as u32 + 1, "").unwrap();
            self.store(&v.ty, fp, val);
            if let Some(h) = self.share_helper(&v.ty) {
                self.b.build_call(h, &[fp.into()], "").unwrap();
            }
        }

        // The worker's start: `void tsl_sw_N(ptr input, ptr* f, ptr* env)`.
        self.closures += 1;
        let worker = self.module.add_function(
            &format!("tsl_sw_{}", self.closures),
            self.ctx.void_type().fn_type(&[p.into(), p.into(), p.into()], false),
            Some(Linkage::Internal),
        );
        let copied_names: Vec<(String, Ty)> = copied.iter().map(|v| (v.name.clone(), v.ty.clone())).collect();
        let shared_names: Vec<(String, Ty)> = shared.iter().map(|v| (v.name.clone(), v.ty.clone())).collect();
        self.in_helper(worker, |g| {
            let cell = worker.get_nth_param(0).unwrap().into_pointer_value();
            let out_f = worker.get_nth_param(1).unwrap().into_pointer_value();
            let out_env = worker.get_nth_param(2).unwrap().into_pointer_value();
            let tp = g.b.build_struct_gep(cell_st, cell, 0, "").unwrap();
            let tree = g.b.build_load(p, tp, "tree").unwrap().into_pointer_value();
            for (i, (name, ty)) in copied_names.iter().enumerate() {
                let item = g.tree_call("tsl_tree_at", &["ptr", "i64"], Some("ptr"), &[tree.into(), g.i64().const_int(i as u64, false).into()]);
                let lt = g.llty(ty).unwrap();
                let out = g.alloca(lt, name);
                g.decode_via(ty, item.unwrap().into_pointer_value(), out);
                let v = g.load(ty, out);
                g.declare_value(name, ty, Some(v));
            }
            // (Shared values stay in the cell for the next worker: this one
            // takes a reference of its own.)
            for (i, (name, ty)) in shared_names.iter().enumerate() {
                let fp = g.b.build_struct_gep(cell_st, cell, i as u32 + 1, "").unwrap();
                let v = g.load(ty, fp);
                g.retain(v, ty);
                g.declare_value(name, ty, Some(v));
            }
            let f = g.closure(params, body, &start_t);
            let handler = g.call_fn_value(f, &start_t, &[]).unwrap();
            g.release(f, &start_t);
            let bx = g.box_closure(&handler_t, handler);
            let adapter = g.request_adapter();
            g.b.build_store(out_f, adapter).unwrap();
            g.b.build_store(out_env, bx).unwrap();
            g.finish_function(None);
        });

        let types: Vec<BasicMetadataTypeEnum<'ctx>> = vec![i64t.into(), p.into(), i64t.into(), p.into(), p.into()];
        let f = self.declare_rt("tsl_serve_http_workers", &types, Some(i64t.into()));
        let wp = worker.as_global_value().as_pointer_value();
        let id = self
            .b
            .build_call(f, &[port.into(), host.into(), workers.into(), wp.into(), input.into()], "")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap();
        self.unhold(1);
        self.release(host, &Ty::Str);
        id
    }
}
