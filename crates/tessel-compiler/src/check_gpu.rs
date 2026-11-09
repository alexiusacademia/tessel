//! GPU compute in the checker (see docs/gpu.md): `kernel fn`s, what may be
//! in them (the GPU subset), and `gpu.run(…)` and the other `gpu` calls.
//!
//! Author: Alexius Academia
//! Date: 2026-11-09

use super::*;

/// The math a kernel can use (as `sqrt(x)`): it has a GPU counterpart.
const KERNEL_MATH: &[&str] = &[
    "sqrt", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "floor", "ceil", "round", "pow",
    "abs", "min", "max",
];

impl Checker<'_> {
    /// A kernel's signature: its index, then buffers and plain values, and
    /// no result. `None` (with errors) when it isn't one.
    pub(super) fn kernel_sig(&mut self, f: &FnDecl, sig: &FnSig) -> KernelInfo {
        let mut ok = true;
        if let Some(g) = f.generics.first() {
            self.error(g.name.span, "a kernel can't be generic").help("write one for each type of number");
            ok = false;
        }
        if f.is_async {
            self.error(f.name.span, "a kernel can't be `async`");
            ok = false;
        }
        if let Some(r) = &f.ret {
            self.error(r.span(), "a kernel returns nothing: it writes its results into buffers")
                .help("pass a `GPUBuffer` and write into it, like `out[i] = …`");
            ok = false;
        }
        let Some(first) = f.params.first() else {
            self.error(f.name.span, "a kernel's first parameter is its index")
                .help("like `kernel fn double(_ i: Int, data: GPUBuffer<Float32>)`");
            return KernelInfo { dims: 1, params: Vec::new(), valid: false };
        };
        let dims = match &sig.params[0].ty {
            Ty::Int => Some(1),
            Ty::Struct(id) if self.structs[*id].name == "(Int, Int)" => Some(2),
            Ty::Struct(id) if self.structs[*id].name == "(Int, Int, Int)" => Some(3),
            Ty::Error => None,
            t => {
                let what = self.a(t);
                self.error(first.ty.span(), format!("a kernel's index is an `Int`, `(Int, Int)` or `(Int, Int, Int)`, not {what}"))
                    .help("it's where in the range this run is: one number for each axis");
                None
            }
        };
        let mut params = Vec::new();
        for (p, ps) in f.params.iter().zip(&sig.params).skip(1) {
            if p.bind || p.variadic || p.default.is_some() {
                self.error(p.span, "a kernel's parameters are plain: no `bind`, `…` or default value");
                ok = false;
            }
            match &ps.ty {
                Ty::GpuBuffer(_) | Ty::Float32 | Ty::Int | Ty::Bool | Ty::Error => {}
                Ty::Float => {
                    self.error(p.ty.span(), "a kernel computes with 32-bit numbers: use `Float32`, not `Float`")
                        .help("GPUs (Apple's have no 64-bit floats at all) compute in 32 bits; pass `Float32(x)`");
                    ok = false;
                }
                t => {
                    let what = self.a(t);
                    self.error(p.ty.span(), format!("a kernel can take `GPUBuffer`s, `Float32`, `Int` and `Bool`, not {what}"));
                    ok = false;
                }
            }
            params.push(ps.ty.clone());
        }
        KernelInfo { dims: dims.unwrap_or(1), params, valid: ok && dims.is_some() }
    }

    /// `gpu.run(kernel, over: size, values…)`, `gpu.wait()`, `gpu.sum(b)`,
    /// `gpu.min(b)`, `gpu.max(b)`, `gpu.dot(a, b)`.
    pub(super) fn gpu_call(&mut self, name: &Ident, args: &[Arg], trailing: Option<&Expr>, span: Span) -> Ty {
        let n = name.name.as_str();
        if let Some(t) = trailing {
            self.error(t.span, format!("`gpu.{n}` takes no block"));
        }
        self.set_callee(Callee::BuiltinFn(format!("gpu.{n}")));
        let floats = Ty::GpuBuffer(Box::new(Ty::Float32));
        match n {
            "run" => self.gpu_run(args, span),
            "wait" => {
                if !args.is_empty() {
                    self.args_unchecked(args, None);
                    self.error(span, "`gpu.wait()` takes nothing");
                }
                Ty::Void
            }
            "sum" | "min" | "max" | "dot" => {
                let want = if n == "dot" { 2 } else { 1 };
                if args.len() != want || args.iter().any(|a| a.label.is_some()) {
                    self.args_unchecked(args, None);
                    let shape = if n == "dot" { "gpu.dot(a, b)" } else { &format!("gpu.{n}(buffer)") };
                    self.error(span, format!("write it as `{shape}`"));
                    return Ty::Error;
                }
                for a in args {
                    self.expect(&a.value, &floats);
                }
                Ty::Float
            }
            _ => {
                self.args_unchecked(args, None);
                self.error(name.span, format!("`gpu` has no `{n}`"))
                    .help("it has `run`, `wait`, `sum`, `min`, `max` and `dot`, and `device` and `isCPU`");
                Ty::Error
            }
        }
    }

    fn gpu_run(&mut self, args: &[Arg], span: Span) -> Ty {
        let kernel = match args.first() {
            Some(a) if a.label.is_none() => match &a.value.kind {
                ExprKind::Name(k) if self.kernels.contains_key(k) && self.lookup(k).is_none() => {
                    self.note_global(a.value.span, k);
                    self.types.insert(a.value.id, Ty::Void);
                    Some(k.clone())
                }
                ExprKind::Name(k) if matches!(self.globals.get(k), Some(Global::Fn(_))) && self.lookup(k).is_none() => {
                    self.error(a.value.span, format!("`{k}` isn't a kernel"))
                        .help(format!("declare it `kernel fn {k}(…)` to run it on the GPU"));
                    None
                }
                _ => {
                    self.error(a.value.span, "give `gpu.run` a kernel's name first, like `gpu.run(smooth, over: n, …)`");
                    None
                }
            },
            _ => {
                self.error(span, "write it as `gpu.run(kernel, over: size, values…)`");
                None
            }
        };
        let Some(k) = kernel else {
            self.args_unchecked(args.get(1..).unwrap_or(&[]), None);
            return Ty::Void;
        };
        let info = self.kernels[&k].clone();
        let over_ty = match info.dims {
            1 => Ty::Int,
            d => {
                let name = if d == 2 { "(Int, Int)" } else { "(Int, Int, Int)" };
                match self.structs.iter().position(|s| s.name == name) {
                    Some(id) => Ty::Struct(id),
                    None => Ty::Error,
                }
            }
        };
        match args.get(1) {
            Some(a) if a.label.as_ref().is_some_and(|l| l.name == "over") => {
                self.expect(&a.value, &over_ty);
            }
            _ => {
                let size = ["n", "(nx, ny)", "(nx, ny, nz)"][info.dims - 1];
                self.error(span, format!("say how many times `{k}` runs: `gpu.run({k}, over: {size}, …)`"));
                self.args_unchecked(args.get(1..).unwrap_or(&[]), None);
                return Ty::Void;
            }
        }
        let values = &args[2..];
        for (a, t) in values.iter().zip(&info.params) {
            if let Some(l) = &a.label {
                self.error(l.span, "the kernel's values are given in order, without labels");
            }
            self.expect(&a.value, t);
        }
        if values.len() != info.params.len() {
            for a in values.iter().skip(info.params.len()) {
                self.expr(&a.value, None);
            }
            let (want, got) = (info.params.len(), values.len());
            self.error(span, format!("`{k}` takes {want} value{} after its index, and this gives {got}", if want == 1 { "" } else { "s" }));
        }
        Ty::Void
    }

    // ===== the GPU subset =====

    /// Reports what in a kernel's body a GPU can't do. `names`: the
    /// kernel's parameters and the variables declared so far.
    pub(super) fn kernel_block(&mut self, b: &Block, names: &mut Vec<String>) {
        let mark = names.len();
        for s in &b.stmts {
            self.kernel_stmt(s, names);
        }
        names.truncate(mark);
    }

    /// A block that gives a value (a branch of an `if`): its last
    /// expression is that value.
    fn kernel_value_block(&mut self, b: &Block, names: &mut Vec<String>) {
        let mark = names.len();
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            match s {
                Stmt::Expr(e) if i + 1 == n => self.kernel_expr(e, names),
                _ => self.kernel_stmt(s, names),
            }
        }
        names.truncate(mark);
    }

    fn kernel_stmt(&mut self, s: &Stmt, names: &mut Vec<String>) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) => {
                let before = self.diags.list.len();
                self.kernel_expr(&d.value, names);
                let t = self.types.get(&d.value.id).cloned().unwrap_or(Ty::Error);
                // (The index's tuple, taken apart: `let (i, j) = at`.)
                let index = matches!(&t, Ty::Struct(id) if matches!(self.structs[*id].name.as_str(), "(Int, Int)" | "(Int, Int, Int)"));
                if !index && self.diags.list.len() == before {
                    self.kernel_value_type(&t, d.value.span);
                }
                names.push(d.name.name.clone());
            }
            Stmt::State(d) => {
                self.error(d.span, "a kernel can't have `state`");
            }
            Stmt::Fn(f) => {
                self.error(f.name.span, "a kernel can't declare functions").help("declare it outside, as a `kernel fn` of its own");
            }
            Stmt::Assign { target, value, op, .. } => {
                match &target.kind {
                    ExprKind::Name(n) if names.contains(n) => {}
                    ExprKind::Index(object, index) => {
                        let ot = self.types.get(&object.id).cloned().unwrap_or(Ty::Error);
                        if !matches!(ot, Ty::GpuBuffer(_) | Ty::Error) {
                            self.error(target.span, "in a kernel, only a variable or a buffer's element can be changed");
                        }
                        self.kernel_expr(object, names);
                        self.kernel_expr(index, names);
                    }
                    _ => {
                        self.error(target.span, "in a kernel, only a variable or a buffer's element can be changed");
                    }
                }
                if matches!(op, AssignOp::Op(BinOp::Coalesce)) {
                    self.error(value.span, "a kernel has no optional values, so no `??`");
                }
                self.kernel_expr(value, names);
            }
            Stmt::Return { value, span } => {
                if value.is_some() {
                    self.error(*span, "a kernel returns nothing: it writes its results into buffers");
                }
            }
            Stmt::For { var, iter, body, .. } => {
                match &iter.kind {
                    ExprKind::Binary(BinOp::Range, a, b) => {
                        self.kernel_expr(a, names);
                        self.kernel_expr(b, names);
                    }
                    _ => {
                        self.error(iter.span, "in a kernel, `for` goes over a range of numbers, like `for k in 0..n`");
                    }
                }
                names.push(var.name.clone());
                self.kernel_block(body, names);
                names.pop();
            }
            Stmt::While { cond, body, .. } => {
                match cond {
                    Cond::Expr(e) => self.kernel_expr(e, names),
                    Cond::Let(_, _, value) => {
                        self.error(value.span, "a kernel has no optional values, so no `while let`");
                    }
                }
                self.kernel_block(body, names);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(e) => match &e.kind {
                ExprKind::If { .. } | ExprKind::Block(_) => self.kernel_expr(e, names),
                _ => {
                    self.error(e.span, "a kernel can't do this on its own: it computes values and writes them into buffers");
                }
            },
        }
    }

    /// A value a kernel can hold: `Int`, `Float32` or `Bool`.
    fn kernel_value_type(&mut self, t: &Ty, span: Span) -> bool {
        match t {
            Ty::Int | Ty::Float32 | Ty::Bool | Ty::Error => true,
            Ty::Float => {
                self.error(span, "a kernel computes with `Float32`, not `Float`")
                    .help("GPUs compute in 32 bits (Apple's have no 64-bit floats): write `Float32(x)`, or pass a `Float32`");
                false
            }
            t => {
                let what = self.a(t);
                self.error(span, format!("a kernel can only work with `Int`, `Float32` and `Bool` values, not {what}"));
                false
            }
        }
    }

    fn kernel_expr(&mut self, e: &Expr, names: &mut Vec<String>) {
        let t = self.types.get(&e.id).cloned().unwrap_or(Ty::Error);
        // (Once something in it is reported, its type isn't too.)
        let before = self.diags.list.len();
        match &e.kind {
            ExprKind::Int(n) => {
                if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    self.error(e.span, "this number is too big for a kernel: `Int`s there are 32-bit")
                        .help("they go from -2147483648 to 2147483647");
                }
            }
            ExprKind::Float(_) | ExprKind::Bool(_) => {
                self.kernel_value_type(&t, e.span);
            }
            ExprKind::Name(n) => {
                if !names.contains(n) {
                    self.error(e.span, format!("a kernel can only use its parameters and its own variables, not `{n}`"))
                        .help(format!("pass `{n}` to the kernel as a parameter"));
                } else if !matches!(t, Ty::GpuBuffer(_)) && !matches!(&t, Ty::Struct(id) if self.structs[*id].name.starts_with("(Int")) {
                    self.kernel_value_type(&t, e.span);
                }
            }
            ExprKind::Binary(op, a, b) => {
                match op {
                    BinOp::Coalesce => {
                        self.error(e.span, "a kernel has no optional values, so no `??`");
                    }
                    BinOp::Range => {
                        self.error(e.span, "in a kernel, a range is only for `for`");
                    }
                    _ => {}
                }
                self.kernel_expr(a, names);
                self.kernel_expr(b, names);
                if self.diags.list.len() == before {
                    self.kernel_value_type(&t, e.span);
                }
            }
            ExprKind::Unary(_, x) => {
                self.kernel_expr(x, names);
                if self.diags.list.len() == before {
                    self.kernel_value_type(&t, e.span);
                }
            }
            ExprKind::Index(object, index) => {
                let ot = self.types.get(&object.id).cloned().unwrap_or(Ty::Error);
                if !matches!(ot, Ty::GpuBuffer(_) | Ty::Error) {
                    self.error(object.span, "in a kernel, only `GPUBuffer`s can be indexed");
                }
                self.kernel_expr(object, names);
                self.kernel_expr(index, names);
            }
            ExprKind::Member { object, name, .. } => {
                let ot = self.types.get(&object.id).cloned().unwrap_or(Ty::Error);
                let ok = match &ot {
                    Ty::GpuBuffer(_) => name.name == "count",
                    Ty::Struct(id) => self.structs[*id].name.starts_with("(Int") && matches!(name.name.as_str(), "0" | "1" | "2"),
                    Ty::Error => true,
                    _ => false,
                };
                if !ok {
                    self.error(e.span, format!("a kernel can't use `.{}` here", name.name))
                        .help("it can use a buffer's `count`, and the parts of its index (`at.0`, `at.1`, `at.2`)");
                }
                self.kernel_expr(object, names);
            }
            ExprKind::Call { callee, args, trailing } => {
                let builtin = match self.calls.get(&e.id).map(|c| &c.callee) {
                    Some(Callee::BuiltinFn(n)) => Some(n.clone()),
                    _ => None,
                };
                let fname = match &callee.kind {
                    ExprKind::Name(n) => Some(n.clone()),
                    _ => None,
                };
                let allowed = match (&builtin, &fname) {
                    (Some(b), Some(n)) if b == n => KERNEL_MATH.contains(&n.as_str()) || n == "Float32" || n == "Int",
                    // (`abs`, `min`, `max`, the math on `Float32` and the
                    // conversions are checked by their own rules.)
                    (None, Some(n)) => KERNEL_MATH.contains(&n.as_str()) || n == "Float32" || n == "Int",
                    _ => false,
                };
                if !allowed {
                    let what = fname.map_or("this".to_string(), |n| format!("`{n}`"));
                    self.error(callee.span, format!("a kernel can't call {what}"))
                        .help("it can use the math functions (`sqrt`, `exp`, `min`, `max`, `abs`, `floor`…) and `Float32(…)` and `Int(…)`");
                } else if let Some(n) = &fname
                    && (n == "Float32" || n == "Int")
                    && let Some(a) = args.first()
                {
                    let at = self.types.get(&a.value.id).cloned().unwrap_or(Ty::Error);
                    if !matches!(at, Ty::Int | Ty::Float32 | Ty::Error) {
                        let what = self.a(&at);
                        self.error(a.value.span, format!("in a kernel, `{n}(…)` takes an `Int` or a `Float32`, not {what}"));
                    }
                }
                if let Some(t) = trailing {
                    self.error(t.span, "a kernel has no blocks");
                }
                for a in args {
                    self.kernel_expr(&a.value, names);
                }
                if self.diags.list.len() == before {
                    self.kernel_value_type(&t, e.span);
                }
            }
            ExprKind::If { cond, then, otherwise } => {
                match cond {
                    Cond::Expr(c) => self.kernel_expr(c, names),
                    Cond::Let(_, _, value) => {
                        self.error(value.span, "a kernel has no optional values, so no `if let`");
                    }
                }
                self.kernel_value_block(then, names);
                match otherwise {
                    Some(Else::Block(b)) => self.kernel_value_block(b, names),
                    Some(Else::If(x)) => self.kernel_expr(x, names),
                    None => {}
                }
            }
            ExprKind::Block(b) => self.kernel_value_block(b, names),
            ExprKind::Str(_) => {
                self.error(e.span, "a kernel has no text").help("GPUs work with numbers only");
            }
            ExprKind::List(_) | ExprKind::Dict(_) => {
                self.error(e.span, "a kernel can't make lists or dictionaries").help("keep the numbers in `GPUBuffer`s");
            }
            ExprKind::Closure { .. } => {
                self.error(e.span, "a kernel can't have closures");
            }
            ExprKind::Match { .. } => {
                self.error(e.span, "a kernel can't use `match`").help("use `if` … `else if` …");
            }
            ExprKind::Await(_) => {
                self.error(e.span, "a kernel can't wait");
            }
            _ => {
                self.error(e.span, "a kernel can't do this").help("see the GPU subset: numbers, `if`, `for`, `while`, and buffers' elements");
            }
        }
    }
}
