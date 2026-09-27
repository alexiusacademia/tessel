//! Generic declarations are compiled by specialization: for each set of type
//! arguments used (`Stack<Int>`, `lastOr<String>`), the checker makes a copy
//! of the declaration with the type parameters replaced, named after them
//! (`Stack<Int>`), and checks and compiles that copy like any other
//! declaration. This module makes the copies.

use std::collections::HashMap;

use crate::ast::*;

/// A copy of generic item `item` named `name`, with the type parameters
/// replaced as `subst` says and new expression ids from `next_id`. Inside a
/// generic struct or enum, its own bare name (`Stack` in `Stack(items: [])`)
/// means this copy.
///
/// The copy's declared names (variables, parameters, loop variables) get
/// spans tagged with `copy` (in the file number's high bits), since the
/// checker records their types by span.
pub fn specialize(
    item: &Item,
    name: &str,
    subst: &HashMap<String, TypeExpr>,
    next_id: &mut ExprId,
    copy: u32,
) -> Item {
    let mut item = item.clone();
    let own = item.name().name.clone();
    let mut c = Copier { subst, own: own.clone(), own_args: Vec::new(), next_id, copy };
    match &mut item {
        Item::Fn(f) => {
            let params: Vec<String> = f.generics.iter().map(|g| g.name.name.clone()).collect();
            c.own_args = params.iter().map(|p| subst[p].clone()).collect();
            c.own = String::new();
            f.generics.clear();
            f.name.name = name.to_string();
            c.func(f);
        }
        Item::Struct(d) => {
            c.own_args = d.generics.iter().map(|g| subst[&g.name.name].clone()).collect();
            d.generics.clear();
            d.name.name = name.to_string();
            for f in &mut d.fields {
                c.ty(&mut f.ty);
                if let Some(e) = &mut f.default {
                    c.expr(e);
                }
            }
            for m in &mut d.methods {
                c.func(m);
            }
        }
        Item::Enum(d) => {
            c.own_args = d.generics.iter().map(|g| subst[&g.name.name].clone()).collect();
            d.generics.clear();
            d.name.name = name.to_string();
            for case in &mut d.cases {
                for p in &mut case.fields {
                    c.param(p);
                }
            }
            for m in &mut d.methods {
                c.func(m);
            }
        }
        _ => unreachable!("only functions, structs and enums are generic"),
    }
    item
}

struct Copier<'a> {
    subst: &'a HashMap<String, TypeExpr>,
    /// The generic struct or enum being copied (empty for a function), and
    /// its type arguments, for references to it by its bare name.
    own: String,
    own_args: Vec<TypeExpr>,
    next_id: &'a mut ExprId,
    copy: u32,
}

impl Copier<'_> {
    /// Tags a declared name's span with the copy number.
    fn name(&self, id: &mut Ident) {
        id.span.file = (id.span.file & crate::source::FILE_MASK) | (self.copy << 16);
    }

    fn ty(&mut self, t: &mut TypeExpr) {
        match t {
            TypeExpr::Named(id) => {
                if let Some(r) = self.subst.get(&id.name) {
                    *t = r.clone();
                } else if !self.own.is_empty() && id.name == self.own {
                    *t = TypeExpr::Generic(id.clone(), self.own_args.clone(), id.span);
                }
            }
            TypeExpr::List(x, _) | TypeExpr::Optional(x, _) => self.ty(x),
            TypeExpr::Dict(k, v, _) => {
                self.ty(k);
                self.ty(v);
            }
            TypeExpr::Fn(ps, r, _) => {
                ps.iter_mut().for_each(|p| self.ty(p));
                if let Some(r) = r {
                    self.ty(r);
                }
            }
            TypeExpr::Generic(_, args, _) => args.iter_mut().for_each(|a| self.ty(a)),
        }
    }

    fn param(&mut self, p: &mut Param) {
        self.name(&mut p.name);
        self.ty(&mut p.ty);
        if let Some(d) = &mut p.default {
            self.expr(d);
        }
    }

    fn func(&mut self, f: &mut FnDecl) {
        f.params.iter_mut().for_each(|p| self.param(p));
        if let Some(r) = &mut f.ret {
            self.ty(r);
        }
        self.block(&mut f.body);
    }

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => {
                self.name(&mut d.name);
                if let Some(t) = &mut d.ty {
                    self.ty(t);
                }
                self.expr(&mut d.value);
            }
            Stmt::Fn(f) => self.func(f),
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            Stmt::For { var, iter, body, .. } => {
                self.name(var);
                self.expr(iter);
                self.block(body);
            }
            Stmt::While { cond, body, .. } => {
                self.cond(cond);
                self.block(body);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn cond(&mut self, c: &mut Cond) {
        match c {
            Cond::Expr(e) => self.expr(e),
            Cond::Let(n, t, e) => {
                self.name(n);
                if let Some(t) = t {
                    self.ty(t);
                }
                self.expr(e);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        e.id = *self.next_id;
        *self.next_id += 1;
        match &mut e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Nil | ExprKind::DotName(_) | ExprKind::Error => {}
            ExprKind::Name(n) => {
                if !self.own.is_empty() && *n == self.own {
                    e.kind = ExprKind::GenericName(n.clone(), self.own_args.clone());
                }
            }
            ExprKind::GenericName(_, args) => args.iter_mut().for_each(|a| self.ty(a)),
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::List(items) => items.iter_mut().for_each(|x| self.expr(x)),
            ExprKind::Dict(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Unary(_, x) => self.expr(x),
            ExprKind::Cast { value, ty, .. } => {
                self.expr(value);
                self.ty(ty);
            }
            ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => {
                self.expr(l);
                self.expr(r);
            }
            ExprKind::Call { callee, args, trailing } => {
                self.expr(callee);
                args.iter_mut().for_each(|a| self.expr(&mut a.value));
                if let Some(t) = trailing {
                    self.expr(t);
                }
            }
            ExprKind::Member { object, .. } => self.expr(object),
            ExprKind::Closure { params, body } => {
                params.iter_mut().for_each(|p| self.name(p));
                self.block(body);
            }
            ExprKind::Block(body) => self.block(body),
            ExprKind::If { cond, then, otherwise } => {
                self.cond(cond);
                self.block(then);
                match otherwise {
                    Some(Else::Block(b)) => self.block(b),
                    Some(Else::If(x)) => self.expr(x),
                    None => {}
                }
            }
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    match &mut arm.pattern {
                        Pattern::Literal(l) => self.expr(l),
                        Pattern::Case { bindings, .. } => bindings.iter_mut().for_each(|b| self.name(b)),
                        _ => {}
                    }
                    self.expr(&mut arm.body);
                }
            }
        }
    }
}
