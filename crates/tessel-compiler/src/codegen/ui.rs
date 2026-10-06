//! Code generation for views: view bodies emit the UI tree through the
//! runtime's `tsl_ui_*` calls each time the UI is rebuilt.
//!
//! - A view compiles to `void tsl_v_<Name>(env, params…)`. It opens a view
//!   instance in the tree, which gives it a persistent *state record* (a
//!   struct of its `state` values, kept by the runtime across rebuilds).
//! - Functions declared in a view compile to `tsl_vf_<View>_<name>(env,
//!   record, params…)`; closures inside a view keep the record alive.
//! - A `View` value is a closure that emits the view when called.
//! - A `bind` argument is a *locator*: a closure returning a pointer to the
//!   storage it names, evaluated fresh each time it's used.

use std::hash::{Hash, Hasher};

use inkwell::module::Linkage;
use inkwell::types::{BasicMetadataTypeEnum, BasicTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, FunctionValue, PointerValue};
use inkwell::IntPredicate;

use super::{pick, Body, FnCtx, Gen, Record, Store, Val, Var};
use crate::ast::*;
use crate::builtins;
use crate::check::{ArgRef, CallInfo, Callee};
use crate::types::Ty;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    // ===== declarations =====

    /// Declares every view's build function, state record and functions.
    pub(super) fn declare_views(&mut self) {
        let names: Vec<String> = self.view_decls.keys().cloned().collect();
        for name in names {
            let src = self.view_decls[&name];
            // The record holds the view's `state`, then its parameters (so
            // the view's functions can use them; `bind` ones as locators).
            let mut fields: Vec<Ty> = src
                .body
                .stmts
                .iter()
                .filter_map(|s| match s {
                    Stmt::State(d) => Some(self.checked.locals[&d.name.span].clone()),
                    _ => None,
                })
                .collect();
            for sig in self.checked.views.get(&name).into_iter().flatten() {
                fields.push(if sig.bind { Ty::View } else { sig.ty.clone() });
            }
            let body: Vec<BasicTypeEnum> = fields.iter().map(|t| self.llty(t).unwrap()).collect();
            let st = self.ctx.opaque_struct_type(&format!("tsl.state.{name}"));
            st.set_body(&body, false);
            self.records.insert(name.clone(), (st, fields));

            let sigs = self.checked.views.get(&name).cloned().unwrap_or_default();
            let mut ps: Vec<BasicMetadataTypeEnum> = vec![self.ptr_ty().into()];
            for sig in &sigs {
                let t = if sig.bind { Ty::View } else { sig.ty.clone() };
                ps.push(self.llty(&t).unwrap().into());
            }
            let f = self.module.add_function(
                &format!("tsl_v_{name}"),
                self.ctx.void_type().fn_type(&ps, false),
                Some(Linkage::Internal),
            );
            self.view_builds.insert(name.clone(), f);

            for s in &src.body.stmts {
                if let Stmt::Fn(fd) = s {
                    let key = (name.clone(), fd.name.name.clone());
                    let sig = self.checked.view_fns[&key].clone();
                    let params: Vec<Ty> = sig.params.iter().map(|p| p.ty.clone()).collect();
                    let ty = self.fn_type(&params, &sig.ret, true);
                    let f = self.module.add_function(
                        &format!("tsl_vf_{name}_{}", fd.name.name),
                        ty,
                        Some(Linkage::Internal),
                    );
                    self.view_fns.insert(key, f);
                }
            }
        }
    }

    pub(super) fn view_fn_decl(&self, view: &str, name: &str) -> &'a FnDecl {
        self.view_decls[view]
            .body
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Fn(f) if f.name.name == name => Some(f),
                _ => None,
            })
            .expect("view function")
    }

    pub(super) fn compile_views(&mut self) {
        // (In the same order every time: `tessel preview` numbers the views
        // as they're compiled.)
        let mut names: Vec<String> = self.view_decls.keys().cloned().collect();
        names.sort();
        for name in names {
            // (For `tessel preview`: where the views being compiled are declared.)
            self.design_body = Some(self.view_decls[&name].body);
            self.compile_view(&name);
            let src = self.view_decls[&name];
            for s in &src.body.stmts {
                if let Stmt::Fn(fd) = s {
                    self.compile_view_fn(&name, fd);
                }
            }
            self.design_body = None;
        }
    }

    /// How many `state` values a view has (its record's first fields).
    fn state_count(&self, name: &str) -> usize {
        self.view_decls[name].body.stmts.iter().filter(|s| matches!(s, Stmt::State(_))).count()
    }

    /// Vars for a view's state values and functions.
    fn view_vars(&self, name: &str) -> Vec<Var<'ctx>> {
        let src = self.view_decls[name];
        let mut vars = Vec::new();
        let mut i = 0;
        for s in &src.body.stmts {
            match s {
                Stmt::State(d) => {
                    let ty = self.checked.locals[&d.name.span].clone();
                    vars.push(Var { name: d.name.name.clone(), ty, store: Store::State(i) });
                    i += 1;
                }
                Stmt::Fn(f) => vars.push(Var { name: f.name.name.clone(), ty: Ty::Void, store: Store::ViewFn }),
                _ => {}
            }
        }
        vars
    }

    fn compile_view(&mut self, name: &str) {
        let src = self.view_decls[name];
        let f = self.view_builds[name];
        let sigs = self.checked.views.get(name).cloned().unwrap_or_default();
        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        self.stack.push(FnCtx {
            func: f,
            scopes: vec![Vec::new()],
            ret: Ty::Void,
            self_ptr: None,
            boxed: self.boxed_vars(src.body),
            record: None,
        });
        let mut args = Vec::new();
        for (i, (p, sig)) in src.params.iter().zip(&sigs).enumerate() {
            let v = f.get_nth_param(1 + i as u32).unwrap();
            args.push((v, if sig.bind { Ty::View } else { sig.ty.clone() }));
            if sig.bind {
                let lt = self.llty(&Ty::View).unwrap();
                let slot = self.alloca(lt, &p.name.name);
                self.b.build_store(slot, v).unwrap();
                self.push_var(Var { name: p.name.name.clone(), ty: sig.ty.clone(), store: Store::Binding { slot, owned: true } });
            } else {
                self.declare_value(&p.name.name, &sig.ty, Some(v));
            }
        }

        // Open the view instance and get its state record.
        let (st, fields) = self.records[name].clone();
        let info = if fields.is_empty() { self.ptr_ty().const_null() } else { self.record_info(name) };
        let fresh = self.alloca(self.i8().into(), "fresh");
        let id = self.i64().const_int(view_id(name), false);
        let layout = self.i64().const_int(self.record_layout(name), false);
        let data = self.call_rt("tsl_ui_view_begin", &[id.into(), info.into(), layout.into(), fresh.into()]).unwrap().into_pointer_value();
        self.stack.last_mut().unwrap().record = Some(Record { data, ty: st, view: name.to_string() });
        for v in self.view_vars(name) {
            self.push_var(v);
        }

        // Keep this build's arguments in the record, for the view's functions.
        if !args.is_empty() {
            let states = self.state_count(name);
            let flag = self.b.build_load(self.i8(), fresh, "").unwrap().into_int_value();
            let is_fresh = self.b.build_int_compare(IntPredicate::NE, flag, self.i8().const_zero(), "").unwrap();
            let (first, again, stored) = (self.block_named("args"), self.block_named("args_again"), self.block_named("args_stored"));
            self.b.build_conditional_branch(is_fresh, first, again).unwrap();
            for (fresh_record, bb) in [(true, first), (false, again)] {
                self.b.position_at_end(bb);
                for (k, (v, ty)) in args.iter().enumerate() {
                    let p = self.b.build_struct_gep(st, data, (states + k) as u32, "").unwrap();
                    self.retain(*v, ty);
                    // A new record's memory holds nothing to release yet.
                    let old = (!fresh_record).then(|| self.load(ty, p));
                    self.b.build_store(p, *v).unwrap();
                    if let Some(old) = old {
                        self.release(old, ty);
                    }
                }
                self.br(stored);
            }
            self.b.position_at_end(stored);
        }

        // First time this instance appears: initialize its state.
        if !fields.is_empty() {
            let flag = self.b.build_load(self.i8(), fresh, "").unwrap().into_int_value();
            let is_fresh = self.b.build_int_compare(IntPredicate::NE, flag, self.i8().const_zero(), "").unwrap();
            let (init, cont) = (self.block_named("init"), self.block_named("built"));
            self.b.build_conditional_branch(is_fresh, init, cont).unwrap();
            self.b.position_at_end(init);
            let mut i = 0;
            for s in &src.body.stmts {
                if let Stmt::State(d) = s {
                    let ty = fields[i].clone();
                    let v = self.expr_as(&d.value, &ty).unwrap();
                    let p = self.b.build_struct_gep(st, data, i as u32, "").unwrap();
                    self.b.build_store(p, v).unwrap();
                    i += 1;
                }
            }
            self.br(cont);
            self.b.position_at_end(cont);
        }

        self.content_block(src.body, true);
        self.call_rt("tsl_ui_view_end", &[]);
        self.finish_function(None);
        self.stack.pop();
    }

    fn compile_view_fn(&mut self, view: &str, fd: &FnDecl) {
        let key = (view.to_string(), fd.name.name.clone());
        let f = self.view_fns[&key];
        let sig = self.checked.view_fns[&key].clone();
        let (st, _) = self.records[view].clone();
        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        let data = f.get_nth_param(1).unwrap().into_pointer_value();
        let vars = self.view_vars(view);
        self.stack.push(FnCtx {
            func: f,
            scopes: vec![vars],
            ret: sig.ret.clone(),
            self_ptr: None,
            boxed: self.boxed_vars(&fd.body),
            record: Some(Record { data, ty: st, view: view.to_string() }),
        });
        // The view's parameters, as kept in the record by its last build.
        let states = self.state_count(view);
        let view_params = self.view_decls[view].params.iter().zip(self.checked.views.get(view).cloned().unwrap_or_default());
        for (k, (p, psig)) in view_params.enumerate() {
            let index = (states + k) as u32;
            let store = if psig.bind {
                let slot = self.b.build_struct_gep(st, data, index, "").unwrap();
                Store::Binding { slot, owned: false }
            } else {
                Store::State(index)
            };
            self.push_var(Var { name: p.name.name.clone(), ty: psig.ty.clone(), store });
        }
        for (i, (p, ps)) in fd.params.iter().zip(&sig.params).enumerate() {
            let v = f.get_nth_param(2 + i as u32).unwrap();
            self.declare_value(&p.name.name, &ps.ty, Some(v));
        }
        let v = self.block(&fd.body, Some(&sig.ret));
        self.finish_function(v);
        self.stack.pop();
    }

    /// Type info for a view's state record: releases its values when the
    /// view instance goes away.
    fn record_info(&mut self, view: &str) -> PointerValue<'ctx> {
        let (st, fields) = self.records[view].clone();
        let f = self.module.add_function(
            &format!("tsl_h_state_{view}"),
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            for (i, t) in fields.iter().enumerate() {
                let fp = g.b.build_struct_gep(st, p, i as u32, "").unwrap();
                let v = g.load(t, fp);
                g.release(v, t);
            }
            g.b.build_return(None).unwrap();
        });
        let size = self.td.get_abi_size(&st);
        let i = self.i64();
        let p = self.ptr_ty();
        let ty = self.ctx.struct_type(&[i.into(), i.into(), p.into(), p.into(), p.into(), p.into(), p.into()], false);
        let g = self.module.add_global(ty, None, &format!("tsl_t_state_{view}"));
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&ty.const_named_struct(&[
            i.const_int(size, false).into(),
            i.const_int(8, false).into(),
            p.const_null().into(),
            f.as_global_value().as_pointer_value().into(),
            p.const_null().into(),
            p.const_null().into(),
            p.const_null().into(),
        ]));
        g.as_pointer_value()
    }

    /// `tsl_main` for an app: hands the root view to the runtime's event loop.
    pub(super) fn run_app(&mut self, app: &'a AppDecl, entry: FunctionValue<'ctx>) {
        self.stack.push(FnCtx {
            func: entry,
            scopes: vec![Vec::new()],
            ret: Ty::Void,
            self_ptr: None,
            boxed: Default::default(),
            record: None,
        });
        let arg = |label: &str| app.args.iter().find(|a| a.label.as_ref().is_some_and(|l| l.name == label));
        // `title:`, or the manifest's name for the app, or its own.
        let title = match arg("title") {
            Some(a) => self.expr_as(&a.value, &Ty::Str).unwrap(),
            None => {
                let name = crate::app_info().name.unwrap_or_else(|| app.name.name.clone());
                self.str_lit(&name).into()
            }
        };
        let mut size = |label: &str, default: f64| match arg(label) {
            Some(a) => self.expr_as(&a.value, &Ty::Float).unwrap(),
            None => self.ctx.f64_type().const_float(default).into(),
        };
        let w = size("width", 640.0);
        let h = size("height", 480.0);
        // The `Appearance` case's tag; 0 is `.system`.
        let appearance = match arg("appearance") {
            Some(a) => self.expr_as(&a.value, &builtins::Builtins::standard().appearance).unwrap(),
            None => self.ctx.i32_type().const_zero().into(),
        };
        let root = self.view_builds[&app.name.name].as_global_value().as_pointer_value();
        let null = self.ptr_ty().const_null();
        if self.hot.is_some() {
            // (Returns when the app is running already: it was a reload.)
            self.call_rt("tsl_ui_hot", &[title, w, h, appearance, root.into(), null.into()]);
            self.b.build_return(None).unwrap();
        } else {
            self.call_rt("tsl_ui_run", &[title, w, h, appearance, root.into(), null.into()]);
            self.b.build_unreachable().unwrap();
        }
        self.stack.pop();
    }

    // ===== content =====

    /// Emits the views in a block (a view body or a content block).
    pub(super) fn content_block(&mut self, b: &Block, top: bool) {
        let _ = top;
        self.push_scope();
        self.design_blocks.push(b.span);
        for s in &b.stmts {
            if self.terminated() {
                self.continue_dead();
            }
            match s {
                Stmt::Expr(e) => self.content_expr(e),
                Stmt::For { var, iter, body, .. } => {
                    // In a view, the loop variable may also go to a view's
                    // `bind` parameter, which finds it again later like a closure.
                    let captured = true;
                    self.for_each_in(var, iter, captured, &mut |g| g.content_block(body, false));
                }
                Stmt::Let(_) | Stmt::Var(_) => self.stmt(s),
                // `state` and functions belong to the view; the checker rejects the rest.
                _ => {}
            }
        }
        self.design_blocks.pop();
        self.pop_scope();
    }

    /// Emits the view(s) described by `e`.
    pub(super) fn content_expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::If { cond, then, otherwise } => {
                let (then_bb, else_bb, done) = (self.block_named("then"), self.block_named("else"), self.block_named("endif"));
                self.push_scope();
                match cond {
                    Cond::Expr(c) => {
                        let c = self.expr(c).unwrap().into_int_value();
                        self.b.build_conditional_branch(c, then_bb, else_bb).unwrap();
                        self.b.position_at_end(then_bb);
                    }
                    Cond::Let(name, _, value) => {
                        let ov = self.expr(value).unwrap().into_struct_value();
                        let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
                        self.b.build_conditional_branch(flag, then_bb, else_bb).unwrap();
                        self.b.position_at_end(then_bb);
                        let inner = self.b.build_extract_value(ov, 1, "").unwrap();
                        let ty = self.checked.locals[&name.span].clone();
                        self.declare_value(&name.name, &ty, Some(inner));
                    }
                }
                self.content_block(then, false);
                self.pop_scope();
                self.br(done);
                self.b.position_at_end(else_bb);
                match otherwise {
                    Some(Else::Block(b)) => self.content_block(b, false),
                    Some(Else::If(e)) => self.content_expr(e),
                    None => {}
                }
                self.br(done);
                self.b.position_at_end(done);
            }
            ExprKind::Match { subject, arms } => {
                self.match_with(subject, arms, &Ty::Void, true);
            }
            ExprKind::Block(b) => self.content_block(b, false),
            ExprKind::Call { callee, args, trailing }
                if matches!(
                    self.checked.calls.get(&e.id).map(|c| &c.callee),
                    Some(Callee::View(_) | Callee::BuiltinView(_) | Callee::Modifier(_))
                ) =>
            {
                self.emit_view(e, callee, args, trailing.as_deref());
                // For `tessel preview`: the view says which expression it is
                // (the whole one, with its modifiers).
                if let (Some(views), false) = (self.design, self.design_inner.contains(&e.id)) {
                    let parent = self.design_blocks.last().copied().unwrap_or(e.span);
                    let number = views.borrow_mut().number(e, parent, self.design_body, self.checked, self.sources);
                    if let Some(n) = number {
                        let n = self.ctx.i32_type().const_int(n as u64, false);
                        self.call_rt("tsl_ui_source", &[n.into()]);
                    }
                }
            }
            _ => {
                // A view value (a parameter, a `let`, a function's result): call it.
                let t = self.ty(e);
                let Some(v) = self.expr(e) else { return };
                if t == Ty::View {
                    self.call_emitter(v);
                }
                self.release(v, &t);
            }
        }
    }

    fn call_emitter(&mut self, v: Val<'ctx>) {
        let s = v.into_struct_value();
        let f = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
        let env = self.b.build_extract_value(s, 1, "").unwrap();
        let ty = self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false);
        self.b.build_indirect_call(ty, f, &[env.into()], "").unwrap();
    }

    fn emit_view(&mut self, e: &Expr, callee: &Expr, args: &[Arg], trailing: Option<&Expr>) {
        let info = self.checked.calls[&e.id].clone();
        let arg = move |r: ArgRef| pick(args, trailing, r);
        match &info.callee {
            Callee::View(name) => {
                let sigs = self.checked.views[name].clone();
                let src = self.view_decls[name];
                let mut vals: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into()];
                for (i, sig) in sigs.iter().enumerate() {
                    let v = match info.args.get(i).copied().flatten() {
                        Some(r) if sig.bind => self.locator(arg(r)),
                        Some(r) if sig.ty == Ty::View => self.emitter(arg(r)),
                        Some(r) => self.expr_as(arg(r), &sig.ty).unwrap(),
                        None => self.expr_as(src.params[i].default.as_ref().expect("default"), &sig.ty).unwrap(),
                    };
                    vals.push(v.into());
                }
                let f = self.view_builds[name];
                self.b.build_call(f, &vals, "").unwrap();
            }
            Callee::BuiltinView(name) => self.builtin_view(name, &info, &arg),
            Callee::Modifier(name) if name == "id" => {
                // The id must be known before the view is built.
                let ExprKind::Member { object, .. } = &callee.kind else { unreachable!() };
                let x = &args[0].value;
                let t = self.ty(x);
                let v = self.expr(x).unwrap();
                let h = match t {
                    Ty::Str => self.call_rt("tsl_str_hash", &[v]).unwrap(),
                    _ => v,
                };
                self.call_rt("tsl_ui_next_id", &[h]);
                self.release(v, &t);
                self.design_inner.insert(object.id);
                self.content_expr(object);
            }
            Callee::Modifier(name) => {
                let ExprKind::Member { object, .. } = &callee.kind else { unreachable!() };
                self.design_inner.insert(object.id);
                self.content_expr(object);
                self.modifier(name, &info, &arg, args);
            }
            _ => unreachable!(),
        }
    }

    /// A `View` argument for a custom view: a closure that emits it.
    fn emitter(&mut self, x: &Expr) -> Val<'ctx> {
        match &x.kind {
            ExprKind::Closure { body, .. } => self.make_closure(Body::Content(body), &Ty::View),
            _ => self.expr(x).unwrap(),
        }
    }

    /// A `bind` argument: a closure that locates the storage `x` names.
    fn locator(&mut self, x: &Expr) -> Val<'ctx> {
        self.make_closure(Body::Locator(x), &Ty::View)
    }

    /// Content given to a built-in container: emitted in place.
    fn content_inline(&mut self, x: &Expr) {
        match &x.kind {
            ExprKind::Closure { body, .. } => self.content_block(body, false),
            _ => self.content_expr(x),
        }
    }

    /// Calls a runtime function with some leading arguments, then a closure
    /// `{fn, env}` whose reference passes to the runtime.
    pub(super) fn pass_closure(&mut self, rt: &str, leading: &[Val<'ctx>], closure: Val<'ctx>) {
        let s = closure.into_struct_value();
        let f = self.b.build_extract_value(s, 0, "").unwrap();
        let env = self.b.build_extract_value(s, 1, "").unwrap();
        let mut all = leading.to_vec();
        all.push(f);
        all.push(env);
        self.call_rt(rt, &all);
    }

    fn builtin_view<'e>(&mut self, name: &str, info: &CallInfo, arg: &dyn Fn(ArgRef) -> &'e Expr) {
        let a = |i: usize| info.args.get(i).copied().flatten().map(arg);
        let string = |g: &mut Self, x: Option<&Expr>| match x {
            Some(x) => g.expr_as(x, &Ty::Str).unwrap(),
            None => g.str_lit("").into(),
        };
        match name {
            "Text" | "Image" => {
                let s = string(self, a(0));
                self.call_rt(if name == "Text" { "tsl_ui_text" } else { "tsl_ui_image" }, &[s]);
                self.release(s, &Ty::Str);
            }
            "Button" => {
                let label = string(self, a(0));
                let action = self.expr_as(a(1).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_button", &[label], action);
                self.release(label, &Ty::Str);
            }
            "TextField" => {
                let label = string(self, a(0));
                let loc = self.locator(a(1).unwrap());
                let secure = match a(2) {
                    Some(x) => {
                        let v = self.expr_as(x, &Ty::Bool).unwrap().into_int_value();
                        self.to_c_bool(v)
                    }
                    None => self.i8().const_zero().into(),
                };
                self.pass_closure("tsl_ui_text_field", &[label, secure], loc);
                self.release(label, &Ty::Str);
            }
            "Toggle" => {
                let label = string(self, a(0));
                let loc = self.locator(a(1).unwrap());
                self.pass_closure("tsl_ui_toggle", &[label], loc);
                self.release(label, &Ty::Str);
            }
            "CodeEditor" => {
                let loc = self.locator(a(0).unwrap());
                let list_t = Ty::list(Ty::Int);
                let lines = match a(1) {
                    Some(x) => self.expr_as(x, &list_t).unwrap(),
                    None => {
                        let ti = self.type_info(&Ty::Int);
                        let zero = self.i64().const_zero();
                        self.call_rt("tsl_list_new", &[ti.into(), zero.into()]).unwrap()
                    }
                };
                let s = loc.into_struct_value();
                let f = self.b.build_extract_value(s, 0, "").unwrap();
                let env = self.b.build_extract_value(s, 1, "").unwrap();
                let (line_f, line_env) = match a(2) {
                    Some(x) => {
                        let l = self.locator(x).into_struct_value();
                        (self.b.build_extract_value(l, 0, "").unwrap(), self.b.build_extract_value(l, 1, "").unwrap())
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                // Completion: the suggestions, the offset they're for, and the block asking for them.
                let comp_t = Ty::list(builtins::COMPLETION);
                let completions = match a(3) {
                    Some(x) => self.expr_as(x, &comp_t).unwrap(),
                    None => {
                        let ti = self.type_info(&builtins::COMPLETION);
                        let zero = self.i64().const_zero();
                        self.call_rt("tsl_list_new", &[ti.into(), zero.into()]).unwrap()
                    }
                };
                let start = match a(4) {
                    Some(x) => self.expr_as(x, &Ty::Int).unwrap(),
                    None => self.i64().const_int(u64::MAX, true).into(),
                };
                let (oc_f, oc_env) = match a(5) {
                    Some(x) => {
                        let c = self.expr_as(x, &Ty::func(vec![Ty::Int], Ty::Void)).unwrap().into_struct_value();
                        (self.b.build_extract_value(c, 0, "").unwrap(), self.b.build_extract_value(c, 1, "").unwrap())
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                self.call_rt("tsl_ui_code_editor", &[f, env, lines, line_f, line_env, completions, start, oc_f, oc_env]);
                self.release(lines, &list_t);
                self.release(completions, &comp_t);
            }
            "Picker" => {
                let loc = self.locator(a(0).unwrap());
                self.pass_closure("tsl_ui_begin_picker", &[], loc);
                self.content_inline(a(1).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "TabView" => {
                let loc = self.locator(a(0).unwrap()).into_struct_value();
                let mut vals = vec![
                    self.b.build_extract_value(loc, 0, "").unwrap(),
                    self.b.build_extract_value(loc, 1, "").unwrap(),
                ];
                match a(1) {
                    Some(x) => {
                        // The runtime calls `adapter(box, ptr to tag)`; the box holds the closure.
                        let ft = self.ty(x);
                        let Ty::Fn(ps, _) = &ft else { unreachable!() };
                        let tag_t = ps[0].clone();
                        let closure = self.expr(x).unwrap();
                        let ti = self.type_info(&ft);
                        let bx = self.call_rt("tsl_box_new", &[ti.into()]).unwrap().into_pointer_value();
                        let data = self.box_data(bx);
                        self.b.build_store(data, closure).unwrap();
                        let adapter = self.tag_adapter(&tag_t);
                        vals.push(adapter.into());
                        vals.push(bx.into());
                    }
                    None => {
                        vals.push(self.ptr_ty().const_null().into());
                        vals.push(self.ptr_ty().const_null().into());
                    }
                }
                self.call_rt("tsl_ui_begin_tabs", &vals);
                self.content_inline(a(2).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "Divider" => {
                self.call_rt("tsl_ui_divider", &[]);
            }
            "Icon" => {
                let i = self.expr(a(0).unwrap()).unwrap();
                self.call_rt("tsl_ui_icon", &[i]);
            }
            "Ribbon" => {
                let loc = self.locator(a(0).unwrap());
                self.pass_closure("tsl_ui_begin_ribbon", &[], loc);
                self.content_inline(a(1).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "RibbonTab" | "RibbonGroup" => {
                let title = string(self, a(0));
                let f = if name == "RibbonTab" { "tsl_ui_begin_ribbon_tab" } else { "tsl_ui_begin_ribbon_group" };
                self.call_rt(f, &[title]);
                self.release(title, &Ty::Str);
                self.content_inline(a(1).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "RibbonButton" | "RibbonToggle" | "RibbonMenu" => {
                let label = string(self, a(0));
                let icon = self.expr(a(1).unwrap()).unwrap();
                // `.small` unless it says otherwise.
                let size = match a(2) {
                    Some(x) => self.expr(x).unwrap(),
                    None => self.ctx.i32_type().const_zero().into(),
                };
                match name {
                    "RibbonButton" => {
                        let action = self.expr_as(a(3).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                        self.pass_closure("tsl_ui_ribbon_button", &[label, icon, size], action);
                    }
                    "RibbonToggle" => {
                        let loc = self.locator(a(3).unwrap());
                        self.pass_closure("tsl_ui_ribbon_toggle", &[label, icon, size], loc);
                    }
                    _ => {
                        self.call_rt("tsl_ui_begin_ribbon_menu", &[label, icon, size]);
                        self.content_inline(a(3).unwrap());
                        self.call_rt("tsl_ui_end", &[]);
                    }
                }
                self.release(label, &Ty::Str);
            }
            "Svg" => {
                let s = string(self, a(0));
                self.call_rt("tsl_ui_svg", &[s]);
                self.release(s, &Ty::Str);
            }
            "Markdown" => {
                let source = string(self, a(0));
                let folder = string(self, a(1));
                // The block for links, or null for none.
                let lt = Ty::opt(Ty::func(vec![Ty::Str], Ty::Void));
                let (f, env): (Val, Val) = match a(2) {
                    Some(x) => {
                        let opt = self.expr_as(x, &lt).unwrap().into_struct_value();
                        let present = self.b.build_extract_value(opt, 0, "").unwrap().into_int_value();
                        let closure = self.b.build_extract_value(opt, 1, "").unwrap().into_struct_value();
                        let f = self.b.build_extract_value(closure, 0, "").unwrap().into_pointer_value();
                        let env = self.b.build_extract_value(closure, 1, "").unwrap();
                        let null = self.ptr_ty().const_null();
                        (self.b.build_select(present, f, null, "").unwrap(), env)
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                self.call_rt("tsl_ui_markdown", &[source, folder, f, env]);
                self.release(source, &Ty::Str);
                self.release(folder, &Ty::Str);
            }
            "VStack" | "HStack" | "ZStack" | "Scroll" => {
                // (runtime kind, index of `alignment:`, index of the content)
                let (kind, align_at, content) = match name {
                    "VStack" => (0, Some(1), 2),
                    "HStack" => (1, Some(1), 2),
                    "ZStack" => (2, Some(0), 1),
                    _ => (3, None, 0),
                };
                let spacing = match (content, a(0)) {
                    (2, Some(x)) => self.expr_as(x, &Ty::Float).unwrap(),
                    _ => self.ctx.f64_type().const_float(8.0).into(),
                };
                // The `Alignment` case's tag; 1 is `.center`.
                let align = match align_at.and_then(a) {
                    Some(x) => self.expr(x).unwrap(),
                    None => self.ctx.i32_type().const_int(1, false).into(),
                };
                let k = self.ctx.i32_type().const_int(kind, false);
                self.call_rt("tsl_ui_begin", &[k.into(), spacing, align]);
                self.content_inline(a(content).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "Spacer" => {
                self.call_rt("tsl_ui_spacer", &[]);
            }
            "Canvas" => {
                // The runtime calls `adapter(box, width, height)`; the box holds the block.
                let ft = Ty::func(vec![builtins::GRAPHICS], Ty::Void);
                let (has_redraw, redraw): (Val, Val) = match a(0) {
                    Some(x) => (self.i8().const_int(1, false).into(), self.expr_as(x, &Ty::Int).unwrap()),
                    None => (self.i8().const_zero().into(), self.i64().const_zero().into()),
                };
                let draw = self.expr_as(a(1).unwrap(), &ft).unwrap();
                let bx = self.box_closure(&ft, draw);
                let adapter = self.graphics_adapter();
                self.call_rt("tsl_ui_canvas", &[adapter.into(), bx.into(), has_redraw, redraw]);
            }
            "Slider" | "Stepper" => {
                let loc = self.locator(a(0).unwrap()).into_struct_value();
                let (f, env) = (self.b.build_extract_value(loc, 0, "").unwrap(), self.b.build_extract_value(loc, 1, "").unwrap());
                let f64t = self.ctx.f64_type();
                let num = |g: &mut Self, x: Option<&Expr>, default: f64| match x {
                    Some(x) => g.expr_as(x, &Ty::Float).unwrap(),
                    None => f64t.const_float(default).into(),
                };
                if name == "Slider" {
                    let (from, to, step) = (num(self, a(1), 0.0), num(self, a(2), 1.0), num(self, a(3), 0.0));
                    let (cf, cenv) = self.optional_action(a(4));
                    self.call_rt("tsl_ui_slider", &[f, env, from, to, step, cf, cenv]);
                } else {
                    let int = self.i8().const_int((self.ty(a(0).unwrap()) == Ty::Int) as u64, false);
                    let step = num(self, a(1), 1.0);
                    let (from, to) = (num(self, a(2), f64::NEG_INFINITY), num(self, a(3), f64::INFINITY));
                    let decimals = match a(4) {
                        Some(x) => self.expr_as(x, &Ty::Int).unwrap(),
                        None => self.i64().const_int(u64::MAX, true).into(),
                    };
                    let (cf, cenv) = self.optional_action(a(5));
                    self.call_rt("tsl_ui_stepper", &[f, env, int.into(), step, from, to, decimals, cf, cenv]);
                }
            }
            "ColorPicker" => {
                let loc = self.locator(a(0).unwrap()).into_struct_value();
                let (f, env) = (self.b.build_extract_value(loc, 0, "").unwrap(), self.b.build_extract_value(loc, 1, "").unwrap());
                let (cf, cenv) = self.optional_action(a(1));
                self.call_rt("tsl_ui_color_picker", &[f, env, cf, cenv]);
            }
            "DisclosureGroup" => {
                // The content is built only while it's open.
                let title = string(self, a(0));
                let loc = self.locator(a(1).unwrap()).into_struct_value();
                let (f, env) = (self.b.build_extract_value(loc, 0, "").unwrap(), self.b.build_extract_value(loc, 1, "").unwrap());
                let open = self.call_rt("tsl_ui_begin_disclosure", &[title, f, env]).unwrap().into_int_value();
                self.release(title, &Ty::Str);
                let is_open = self.b.build_int_compare(IntPredicate::NE, open, self.i8().const_zero(), "").unwrap();
                let (content, done) = (self.block_named("disclosed"), self.block_named("closed"));
                self.b.build_conditional_branch(is_open, content, done).unwrap();
                self.b.position_at_end(content);
                self.content_inline(a(2).unwrap());
                self.br(done);
                self.b.position_at_end(done);
                self.call_rt("tsl_ui_end", &[]);
            }
            "LazyVStack" => {
                let count = self.expr_as(a(0).unwrap(), &Ty::Int).unwrap();
                let height = self.expr_as(a(1).unwrap(), &Ty::Float).unwrap();
                let ExprKind::Closure { params, body } = &a(2).unwrap().kind else { unreachable!("checked: the rows are a block") };
                let rows = self.make_closure(Body::Rows(&params[0], body), &Ty::View);
                self.pass_closure("tsl_ui_lazy", &[count, height], rows);
    }

            "MenuItem" => {
                let label = string(self, a(0));
                let shortcut = string(self, a(1));
                let checked = match a(2) {
                    Some(x) => {
                        let b = self.expr(x).unwrap().into_int_value();
                        self.to_c_bool(b)
                    }
                    None => self.i8().const_zero().into(),
                };
                let action = self.expr_as(a(3).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_menu_item", &[label, shortcut, checked], action);
                self.release(label, &Ty::Str);
                self.release(shortcut, &Ty::Str);
            }
            "MenuDivider" => {
                self.call_rt("tsl_ui_menu_divider", &[]);
            }
            "Menu" => {
                let title = string(self, a(0));
                self.call_rt("tsl_ui_begin_menu", &[title]);
                self.release(title, &Ty::Str);
                self.content_inline(a(1).unwrap());
                self.call_rt("tsl_ui_end", &[]);
            }
            "Window" => {
                let title = string(self, a(0));
                let loc = self.locator(a(1).unwrap()).into_struct_value();
                let f = self.b.build_extract_value(loc, 0, "").unwrap();
                let env = self.b.build_extract_value(loc, 1, "").unwrap();
                let f64t = self.ctx.f64_type();
                let w = match a(2) {
                    Some(x) => self.expr_as(x, &Ty::Float).unwrap(),
                    None => f64t.const_float(480.0).into(),
                };
                let h = match a(3) {
                    Some(x) => self.expr_as(x, &Ty::Float).unwrap(),
                    None => f64t.const_float(360.0).into(),
                };
                let open = self.call_rt("tsl_ui_begin_window", &[title, f, env, w, h]).unwrap();
                self.release(title, &Ty::Str);
                // Only an open window's content is built.
                self.content_if(open, a(4).unwrap(), "tsl_ui_end_window");
            }
            _ => unreachable!("built-in view {name}"),
        }
    }

    /// An optional `fn()` argument as `(fn, env)`, both null when it's not given.
    fn optional_action(&mut self, x: Option<&Expr>) -> (Val<'ctx>, Val<'ctx>) {
        match x {
            Some(x) => {
                let c = self.expr_as(x, &Ty::func(vec![], Ty::Void)).unwrap().into_struct_value();
                (self.b.build_extract_value(c, 0, "").unwrap(), self.b.build_extract_value(c, 1, "").unwrap())
            }
            None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
        }
    }

    /// Emits `content` (then calls `end`) if the runtime's answer `flag` (a
    /// u8) is nonzero.
    fn content_if(&mut self, flag: Val<'ctx>, content: &Expr, end: &str) {
        let flag = flag.into_int_value();
        let shown = self.b.build_int_compare(IntPredicate::NE, flag, self.i8().const_zero(), "").unwrap();
        let (then_bb, done) = (self.block_named("shown"), self.block_named("not_shown"));
        self.b.build_conditional_branch(shown, then_bb, done).unwrap();
        self.b.position_at_end(then_bb);
        self.content_inline(content);
        self.call_rt(end, &[]);
        self.br(done);
        self.b.position_at_end(done);
    }

    fn modifier<'e>(&mut self, name: &str, info: &CallInfo, arg: &dyn Fn(ArgRef) -> &'e Expr, args: &[Arg]) {
        let a = |i: usize| info.args.get(i).copied().flatten().map(arg);
        let f64t = self.ctx.f64_type();
        let float = |g: &mut Self, x: Option<&Expr>, default: f64| match x {
            Some(x) => g.expr_as(x, &Ty::Float).unwrap(),
            None => f64t.const_float(default).into(),
        };
        let flag = |g: &mut Self, x: Option<&Expr>| -> Val<'ctx> {
            let b = match x {
                Some(x) => g.expr(x).unwrap().into_int_value(),
                None => g.ctx.bool_type().const_int(1, false),
            };
            g.to_c_bool(b)
        };
        let int = |g: &mut Self, x: &Expr| g.expr(x).unwrap();
        // Calls a runtime function with a `Color` (its case's tag, and the box
        // of an `.rgb` or `.rgba` color's values), then `rest`.
        let colored = |g: &mut Self, rt: &str, x: &Expr, rest: &[Val<'ctx>]| {
            let t = builtins::Builtins::standard().color;
            let v = g.expr_as(x, &t).unwrap();
            let s = v.into_struct_value();
            let mut vals = vec![g.b.build_extract_value(s, 0, "").unwrap(), g.b.build_extract_value(s, 1, "").unwrap()];
            vals.extend_from_slice(rest);
            g.call_rt(rt, &vals);
            g.release(v, &t);
        };
        match name {
            "padding" => {
                // Arguments: amount, horizontal, vertical, top, leading, bottom,
                // trailing. The most specific one given sets each edge; with
                // only some edges named, the others get 0, and with nothing
                // named, every edge gets 8.
                let given: Vec<Option<Val<'ctx>>> =
                    (0..7).map(|i| a(i).map(|x| self.expr_as(x, &Ty::Float).unwrap())).collect();
                let base = match given[0] {
                    Some(v) => v,
                    None if given.iter().any(Option::is_some) => f64t.const_zero().into(),
                    None => f64t.const_float(8.0).into(),
                };
                let edge = |own: usize, axis: usize| given[own].or(given[axis]).unwrap_or(base);
                let edges = [edge(3, 2), edge(4, 1), edge(5, 2), edge(6, 1)];
                self.call_rt("tsl_ui_padding", &edges);
            }
            "opacity" => {
                let v = float(self, a(0), 1.0);
                self.call_rt("tsl_ui_opacity", &[v]);
            }
            "font" => {
                let size = float(self, a(0), 0.0);
                let has_size = self.i8().const_int(a(0).is_some() as u64, false);
                let weight = match a(1) {
                    Some(x) => int(self, x),
                    None => self.ctx.i32_type().const_zero().into(),
                };
                let has_weight = self.i8().const_int(a(1).is_some() as u64, false);
                let italic = match a(2) {
                    Some(x) => {
                        let v = self.expr_as(x, &Ty::Bool).unwrap().into_int_value();
                        self.to_c_bool(v)
                    }
                    None => self.i8().const_zero().into(),
                };
                let has_italic = self.i8().const_int(a(2).is_some() as u64, false);
                // (No family is a null string.)
                let family = a(3).map(|x| self.expr_as(x, &Ty::Str).unwrap());
                let null = self.ptr_ty().const_null().into();
                self.call_rt("tsl_ui_font", &[size, has_size.into(), weight, has_weight.into(), italic, has_italic.into(), family.unwrap_or(null)]);
                if let Some(f) = family {
                    self.release(f, &Ty::Str);
                }
            }
            "color" => {
                colored(self, "tsl_ui_color", a(0).unwrap(), &[]);
            }
            "background" => {
                let r = float(self, a(1), 0.0);
                colored(self, "tsl_ui_background", a(0).unwrap(), &[r]);
            }
            "border" => {
                let w = float(self, a(1), 1.0);
                let r = float(self, a(2), 0.0);
                colored(self, "tsl_ui_border", a(0).unwrap(), &[w, r]);
            }
            "frame" => {
                let mut dims = Vec::new();
                // width, height, minWidth, maxWidth, minHeight, maxHeight: each as (has, value)
                for i in 0..6 {
                    match a(i) {
                        Some(x) => {
                            let v = self.expr_as(x, &Ty::opt(Ty::Float)).unwrap().into_struct_value();
                            let has = self.b.build_extract_value(v, 0, "").unwrap().into_int_value();
                            let val = self.b.build_extract_value(v, 1, "").unwrap();
                            dims.push(self.to_c_bool(has));
                            dims.push(val);
                        }
                        None => {
                            dims.push(self.i8().const_zero().into());
                            dims.push(f64t.const_zero().into());
                        }
                    }
                }
                match a(6) {
                    Some(x) => {
                        dims.push(self.i8().const_int(1, false).into());
                        dims.push(int(self, x));
                    }
                    None => {
                        dims.push(self.i8().const_zero().into());
                        dims.push(self.ctx.i32_type().const_zero().into());
                    }
                }
                self.call_rt("tsl_ui_frame", &dims);
            }
            "hidden" | "disabled" => {
                let b = flag(self, a(0));
                self.call_rt(if name == "hidden" { "tsl_ui_hidden" } else { "tsl_ui_disabled" }, &[b]);
            }
            "shortcut" => {
                let key = self.expr_as(a(0).unwrap(), &Ty::Str).unwrap();
                let action = self.expr_as(a(1).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_shortcut", &[key], action);
                self.release(key, &Ty::Str);
            }
            "onAppear" | "onSubmit" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                let f = if name == "onAppear" { "tsl_ui_on_appear" } else { "tsl_ui_on_submit" };
                self.pass_closure(f, &[], action);
            }
            "textAlignment" => {
                let a = int(self, a(0).unwrap());
                self.call_rt("tsl_ui_text_alignment", &[a]);
            }
            "autofocus" => {
                self.call_rt("tsl_ui_autofocus", &[]);
            }
            "animation" => {
                let seconds = float(self, a(0), 0.25);
                self.call_rt("tsl_ui_animation", &[seconds]);
            }
            "tabItem" => {
                let title = self.expr_as(a(0).unwrap(), &Ty::Str).unwrap();
                self.call_rt("tsl_ui_tab_item", &[title]);
                self.release(title, &Ty::Str);
            }
            "hoverBackground" => {
                let r = float(self, a(1), 0.0);
                colored(self, "tsl_ui_hover_background", a(0).unwrap(), &[r]);
            }
            "onTap" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_tap", &[], action);
            }
            "onDrag" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![Ty::Float, Ty::Float], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_drag", &[], action);
            }
            "onPointer" => {
                // The runtime calls `adapter(box, event)`; the box holds the block.
                let ft = Ty::func(vec![builtins::POINTER_EVENT], Ty::Void);
                let action = self.expr_as(a(0).unwrap(), &ft).unwrap();
                let bx = self.box_closure(&ft, action);
                let adapter = self.pointer_adapter();
                self.call_rt("tsl_ui_on_pointer", &[adapter.into(), bx.into()]);
            }
            "onKey" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![Ty::Str], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_key", &[], action);
            }
            "onKeyEvent" => {
                // The runtime calls `adapter(box, event)`; the box holds the block.
                let ft = Ty::func(vec![builtins::KEY_EVENT], Ty::Void);
                let action = self.expr_as(a(0).unwrap(), &ft).unwrap();
                let bx = self.box_closure(&ft, action);
                let adapter = self.key_event_adapter();
                self.call_rt("tsl_ui_on_key_event", &[adapter.into(), bx.into()]);
            }
            "cursor" => {
                let style = int(self, a(0).unwrap());
                self.call_rt("tsl_ui_cursor", &[style]);
            }
            "offset" => {
                let x = float(self, a(0), 0.0);
                let y = float(self, a(1), 0.0);
                self.call_rt("tsl_ui_offset", &[x, y]);
            }
            "clipped" => {
                let b = flag(self, a(0));
                self.call_rt("tsl_ui_clipped", &[b]);
            }
            "onDrop" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![Ty::Str], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_drop", &[], action);
            }
            "draggable" => {
                let payload = self.expr_as(a(0).unwrap(), &Ty::Str).unwrap();
                self.call_rt("tsl_ui_draggable", &[payload]);
                self.release(payload, &Ty::Str);
            }
            "dropTarget" => {
                let action = self.expr_as(a(1).unwrap(), &Ty::func(vec![Ty::Str, Ty::Float, Ty::Float], Ty::Void)).unwrap();
                let action = action.into_struct_value();
                let (f, env) = (self.b.build_extract_value(action, 0, "").unwrap(), self.b.build_extract_value(action, 1, "").unwrap());
                let (tf, tenv): (Val, Val) = match a(0) {
                    Some(x) => {
                        let l = self.locator(x).into_struct_value();
                        (self.b.build_extract_value(l, 0, "").unwrap(), self.b.build_extract_value(l, 1, "").unwrap())
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                self.call_rt("tsl_ui_drop_target", &[f, env, tf, tenv]);
            }
            "onResize" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![Ty::Float, Ty::Float], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_resize", &[], action);
            }
            "contextMenu" | "menuBar" => {
                let which = self.ctx.i32_type().const_int((name == "menuBar") as u64, false);
                self.call_rt("tsl_ui_begin_attached", &[which.into()]);
                self.content_inline(a(0).unwrap());
                self.call_rt("tsl_ui_end_attached", &[which.into()]);
            }
            "popover" => {
                let loc = self.locator(a(0).unwrap()).into_struct_value();
                let f = self.b.build_extract_value(loc, 0, "").unwrap();
                let env = self.b.build_extract_value(loc, 1, "").unwrap();
                // The `Edge` case's tag; 1 is `.bottom`.
                let edge = match a(1) {
                    Some(x) => int(self, x),
                    None => self.ctx.i32_type().const_int(1, false).into(),
                };
                let shown = self.call_rt("tsl_ui_begin_popover", &[f, env, edge]).unwrap();
                self.content_if(shown, a(2).unwrap(), "tsl_ui_end_popover");
            }
            "onOpenFile" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![Ty::Str], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_open_file", &[], action);
            }
            "onCloseRequest" => {
                let action = self.expr_as(a(0).unwrap(), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_ui_on_close_request", &[], action);
            }
            "tooltip" => {
                let text = self.expr_as(a(0).unwrap(), &Ty::Str).unwrap();
                self.call_rt("tsl_ui_tooltip", &[text]);
                self.release(text, &Ty::Str);
            }
            "tag" => {
                let x = &args[0].value;
                let t = self.ty(x);
                let v = self.expr(x).unwrap();
                let p = self.spill(&t, v);
                let info = self.type_info(&t);
                self.call_rt("tsl_ui_tag", &[p.into(), info.into()]);
                self.release(v, &t);
            }
            _ => unreachable!("modifier {name}"),
        }
    }
}

impl<'ctx> Gen<'ctx, '_> {
    /// `void adapter(ptr box, ptr value)`: calls the `fn(T)` closure held in
    /// `box` with a copy of the `T` at `value`.
    fn tag_adapter(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let name = format!("tsl_h_tagcall_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let ft = Ty::func(vec![t.clone()], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let vp = f.get_nth_param(1).unwrap().into_pointer_value();
            let data = g.box_data(bx);
            let closure = g.load(&ft, data);
            let v = g.load(t, vp);
            g.retain(v, t);
            g.call_fn_value(closure, &ft, &[v]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }
}

/// A stable number identifying a view type (for state identity).
fn view_id(name: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    h.finish()
}
