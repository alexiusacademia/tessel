//! GPU kernels (see docs/gpu.md): each `kernel fn` becomes WGSL, for the
//! GPU, and a native function, for the CPU fallback, both from its checked
//! body, with the same rules:
//!
//! - `Int`s are 32-bit and wrap; `/` and `%` by zero give what WGSL says
//!   (`a / 0` is `a`, `a % 0` is `0`); shifts use the low 5 bits.
//! - A buffer's index outside it is taken as its last element (as wgpu's
//!   bounds checks do).
//! - `round` rounds halves away from zero (WGSL's own rounds them to even).
//!
//! It also compiles `GPUBuffer`s' making and methods, and the `gpu` calls.
//!
//! Author: Alexius Academia
//! Date: 2026-11-09

use std::collections::HashMap;
use std::fmt::Write as _;

use inkwell::basic_block::BasicBlock;
use inkwell::values::{FloatValue, FunctionValue, IntValue, PointerValue};
use inkwell::{FloatPredicate, IntPredicate};

use super::*;
use crate::ast::{AssignOp, BinOp, Block, Cond, Else, Expr, ExprKind, FnDecl, Stmt, UnaryOp};
use crate::check::KernelInfo;
use crate::types::Ty;

// The kinds of a kernel's parameters, as the runtime has them
// (gpu_compute.rs `K_…`).
const K_BUF_F32: u64 = 0;
const K_BUF_INT: u64 = 1;
const K_F32: u64 = 2;
const K_INT: u64 = 3;
const K_BOOL: u64 = 4;

fn kind_of(t: &Ty) -> u64 {
    match t {
        Ty::GpuBuffer(e) if **e == Ty::Int => K_BUF_INT,
        Ty::GpuBuffer(_) => K_BUF_F32,
        Ty::Float32 => K_F32,
        Ty::Int => K_INT,
        _ => K_BOOL,
    }
}

/// A name as WGSL can have it (the parser's hidden names have a `$`).
fn wgsl_name(n: &str) -> String {
    let clean: String = n.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    format!("v_{clean}")
}

fn f32_literal(x: f64) -> String {
    let v = x as f32;
    if v.is_infinite() { if v > 0.0 { "3.4028235e38f".into() } else { "-3.4028235e38f".into() } } else { format!("{v:?}f") }
}

/// Where a kernel's parameter is: its WGSL name, and its place in the
/// uniform block (scalars) or among the buffers.
struct WgslParam {
    name: String,
    ty: Ty,
    /// For a scalar: its field in the uniform block (`s3`); for a buffer,
    /// the field holding its count (`c1`).
    field: String,
}

/// Writes a kernel's WGSL.
struct Wgsl<'a> {
    types: &'a HashMap<crate::ast::ExprId, Ty>,
    structs: &'a [crate::types::StructDef],
    out: String,
    indent: usize,
    temps: usize,
    /// Buffers' counts, by their WGSL name.
    counts: HashMap<String, String>,
}

impl Wgsl<'_> {
    fn ty(&self, t: &Ty) -> &'static str {
        match t {
            Ty::Int => "i32",
            Ty::Float32 => "f32",
            Ty::Bool => "bool",
            Ty::Struct(id) if self.structs[*id].name == "(Int, Int)" => "vec2<i32>",
            Ty::Struct(_) => "vec3<i32>",
            _ => "f32",
        }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn type_of(&self, e: &Expr) -> Ty {
        self.types.get(&e.id).cloned().unwrap_or(Ty::Error)
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) => {
                let v = self.expr(&d.value);
                let t = self.ty(&self.type_of(&d.value));
                let kw = if matches!(s, Stmt::Let(_)) { "let" } else { "var" };
                self.line(&format!("{kw} {}: {t} = {v};", wgsl_name(&d.name.name)));
            }
            Stmt::Assign { target, op, value, .. } => {
                let v = self.expr(value);
                let place = match &target.kind {
                    ExprKind::Name(n) => wgsl_name(n),
                    ExprKind::Index(object, index) => {
                        let ExprKind::Name(b) = &object.kind else { unreachable!("checked") };
                        let i = self.expr(index);
                        let b = wgsl_name(b);
                        format!("{b}[min(u32({i}), arrayLength(&{b}) - 1u)]")
                    }
                    _ => unreachable!("checked"),
                };
                match op {
                    AssignOp::Set => self.line(&format!("{place} = {v};")),
                    AssignOp::Op(op) => {
                        let t = self.type_of(target);
                        let r = self.binary_text(*op, &place, &v, &t);
                        self.line(&format!("{place} = {r};"));
                    }
                }
            }
            Stmt::Return { .. } => self.line("return;"),
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
            Stmt::For { var, iter, body, .. } => {
                let ExprKind::Binary(BinOp::Range, a, b) = &iter.kind else { unreachable!("checked") };
                let (a, b) = (self.expr(a), self.expr(b));
                let v = wgsl_name(&var.name);
                self.temps += 1;
                let end = format!("t_end{}", self.temps);
                self.line("{");
                self.indent += 1;
                self.line(&format!("let {end}: i32 = {b};"));
                self.line(&format!("var {v}: i32 = {a};"));
                self.line("loop {");
                self.indent += 1;
                self.line(&format!("if ({v} >= {end}) {{ break; }}"));
                self.block(body);
                self.line(&format!("continuing {{ {v} = {v} + 1i; }}"));
                self.indent -= 1;
                self.line("}");
                self.indent -= 1;
                self.line("}");
            }
            Stmt::While { cond, body, .. } => {
                let Cond::Expr(c) = cond else { unreachable!("checked") };
                self.line("loop {");
                self.indent += 1;
                let c = self.expr(c);
                self.line(&format!("if (!({c})) {{ break; }}"));
                self.block(body);
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Expr(e) => match &e.kind {
                ExprKind::If { cond, then, otherwise } => self.if_stmt(cond, then, otherwise.as_ref()),
                ExprKind::Block(b) => {
                    self.line("{");
                    self.indent += 1;
                    self.block(b);
                    self.indent -= 1;
                    self.line("}");
                }
                _ => {}
            },
            Stmt::State(_) | Stmt::Fn(_) => unreachable!("checked"),
        }
    }

    fn if_stmt(&mut self, cond: &Cond, then: &Block, otherwise: Option<&Else>) {
        let Cond::Expr(c) = cond else { unreachable!("checked") };
        let c = self.expr(c);
        self.line(&format!("if ({c}) {{"));
        self.indent += 1;
        self.block(then);
        self.indent -= 1;
        match otherwise {
            None => self.line("}"),
            Some(Else::Block(b)) => {
                self.line("} else {");
                self.indent += 1;
                self.block(b);
                self.indent -= 1;
                self.line("}");
            }
            Some(Else::If(x)) => {
                self.line("} else {");
                self.indent += 1;
                let ExprKind::If { cond, then, otherwise } = &x.kind else { unreachable!() };
                self.if_stmt(cond, then, otherwise.as_ref());
                self.indent -= 1;
                self.line("}");
            }
        }
    }

    /// A block's value: its statements, then its last expression.
    fn block_value(&mut self, b: &Block, into: &str) {
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            match s {
                Stmt::Expr(e) if i + 1 == n => {
                    let v = self.expr(e);
                    self.line(&format!("{into} = {v};"));
                }
                _ => self.stmt(s),
            }
        }
    }

    fn binary_text(&self, op: BinOp, a: &str, b: &str, t: &Ty) -> String {
        match op {
            BinOp::Shl => format!("({a} << (u32({b}) & 31u))"),
            BinOp::Shr => format!("({a} >> (u32({b}) & 31u))"),
            // WGSL's own rules for these (`a / 0` is `a`, `a % 0` is 0, MIN /
            // -1 is MIN), spelled out: not every backend keeps them (naga's
            // GLSL doesn't).
            BinOp::Div if *t == Ty::Int => format!("t_div({a}, {b})"),
            BinOp::Rem if *t == Ty::Int => format!("t_rem({a}, {b})"),
            _ => format!("({a} {} {b})", op.symbol()),
        }
    }

    fn expr(&mut self, e: &Expr) -> String {
        let t = self.type_of(e);
        match &e.kind {
            ExprKind::Int(n) => match t {
                Ty::Float32 => f32_literal(*n as f64),
                _ => format!("{}i", *n as i32),
            },
            ExprKind::Float(x) => f32_literal(*x),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Name(n) => wgsl_name(n),
            ExprKind::Binary(op, a, b) => {
                let (x, y) = (self.expr(a), self.expr(b));
                let ot = self.type_of(a);
                self.binary_text(*op, &x, &y, &ot)
            }
            ExprKind::Unary(op, x) => {
                let v = self.expr(x);
                match op {
                    UnaryOp::Neg => format!("(-{v})"),
                    UnaryOp::Not => format!("(!{v})"),
                    _ => format!("(~{v})"),
                }
            }
            ExprKind::Index(object, index) => {
                let ExprKind::Name(b) = &object.kind else { unreachable!("checked") };
                let i = self.expr(index);
                let b = wgsl_name(b);
                format!("{b}[min(u32({i}), arrayLength(&{b}) - 1u)]")
            }
            ExprKind::Member { object, name, .. } => {
                let v = self.expr(object);
                match name.name.as_str() {
                    "count" => {
                        let field = self.counts.get(&v).cloned().unwrap_or_else(|| "0".into());
                        format!("i32({field})")
                    }
                    "0" => format!("{v}.x"),
                    "1" => format!("{v}.y"),
                    _ => format!("{v}.z"),
                }
            }
            ExprKind::Call { callee, args, .. } => {
                let ExprKind::Name(f) = &callee.kind else { unreachable!("checked") };
                let vals: Vec<String> = args.iter().map(|a| self.expr(&a.value)).collect();
                let at = args.first().map(|a| self.type_of(&a.value)).unwrap_or(Ty::Error);
                match f.as_str() {
                    "Float32" if at == Ty::Int => format!("f32({})", vals[0]),
                    "Int" if at == Ty::Float32 => format!("i32({})", vals[0]),
                    "Float32" | "Int" => vals[0].clone(),
                    "log10" => format!("(log({}) * 0.4342944819f)", vals[0]),
                    "round" => format!("t_round({})", vals[0]),
                    "min" | "max" => {
                        // (Any number of values, from the left.)
                        let mut r = vals[0].clone();
                        for v in &vals[1..] {
                            r = format!("{f}({r}, {v})");
                        }
                        r
                    }
                    _ => format!("{f}({})", vals.join(", ")),
                }
            }
            ExprKind::If { cond, then, otherwise } => {
                self.temps += 1;
                let tmp = format!("t_if{}", self.temps);
                let ty = self.ty(&t);
                self.line(&format!("var {tmp}: {ty};"));
                self.if_value(cond, then, otherwise.as_ref(), &tmp);
                tmp
            }
            ExprKind::Block(b) => {
                self.temps += 1;
                let tmp = format!("t_b{}", self.temps);
                let ty = self.ty(&t);
                self.line(&format!("var {tmp}: {ty};"));
                self.line("{");
                self.indent += 1;
                self.block_value(b, &tmp);
                self.indent -= 1;
                self.line("}");
                tmp
            }
            _ => unreachable!("checked: not in the GPU subset"),
        }
    }

    fn if_value(&mut self, cond: &Cond, then: &Block, otherwise: Option<&Else>, into: &str) {
        let Cond::Expr(c) = cond else { unreachable!("checked") };
        let c = self.expr(c);
        self.line(&format!("if ({c}) {{"));
        self.indent += 1;
        self.block_value(then, into);
        self.indent -= 1;
        self.line("} else {");
        self.indent += 1;
        match otherwise {
            Some(Else::Block(b)) => self.block_value(b, into),
            Some(Else::If(x)) => {
                let ExprKind::If { cond, then, otherwise } = &x.kind else { unreachable!() };
                self.if_value(cond, then, otherwise.as_ref(), into);
            }
            None => {}
        }
        self.indent -= 1;
        self.line("}");
    }
}

/// The values of a kernel's locals in the native version.
#[derive(Clone)]
enum Local<'ctx> {
    /// An `Int`, `Float32` or `Bool` in a slot.
    Slot(PointerValue<'ctx>, Ty),
    /// The index's parts (fixed).
    Tuple(Vec<IntValue<'ctx>>),
    /// A buffer: where its numbers are, and how many.
    Buffer { data: PointerValue<'ctx>, len: IntValue<'ctx>, ints: bool },
}

impl<'ctx, 'a> Gen<'ctx, 'a> {
    /// Each kernel's description for the runtime (see `KernelDesc` in
    /// gpu_compute.rs), made before the functions that run them.
    pub(super) fn compile_kernels(&mut self) {
        let names: Vec<String> = self.kernel_decls.keys().cloned().collect();
        for name in names {
            let decl = self.kernel_decls[&name];
            let info = self.checked.kernels[&name].clone();
            let wgsl = self.kernel_wgsl(decl, &info);
            let cpu = self.kernel_cpu(&name, decl, &info);
            let p = self.ptr_ty();
            let i64t = self.i64();
            let name_g = self.b.build_global_string_ptr(&name, &format!("tsl_kname_{name}")).unwrap();
            let wgsl_g = self.b.build_global_string_ptr(&wgsl, &format!("tsl_kwgsl_{name}")).unwrap();
            let kinds: Vec<_> = info.params.iter().map(|t| self.i8().const_int(kind_of(t), false)).collect();
            let kinds_arr = self.i8().const_array(&kinds);
            let kinds_g = self.module.add_global(kinds_arr.get_type(), None, &format!("tsl_kkinds_{name}"));
            kinds_g.set_initializer(&kinds_arr);
            kinds_g.set_constant(true);
            kinds_g.set_linkage(Linkage::Internal);
            let st = self.ctx.struct_type(&[p.into(), p.into(), i64t.into(), i64t.into(), p.into(), p.into()], false);
            let desc = st.const_named_struct(&[
                name_g.as_pointer_value().into(),
                wgsl_g.as_pointer_value().into(),
                i64t.const_int(info.dims as u64, false).into(),
                i64t.const_int(info.params.len() as u64, false).into(),
                kinds_g.as_pointer_value().into(),
                cpu.as_global_value().as_pointer_value().into(),
            ]);
            let g = self.module.add_global(st, None, &format!("tsl_kernel_{name}"));
            g.set_initializer(&desc);
            g.set_constant(true);
            g.set_linkage(Linkage::Internal);
            self.kernel_descs.insert(name, g.as_pointer_value());
        }
    }

    /// The kernel as WGSL: its parameters in a uniform block (with the
    /// range, and each buffer's count) and its buffers as storage, run
    /// once per index.
    fn kernel_wgsl(&self, f: &FnDecl, info: &KernelInfo) -> String {
        let mut params = Vec::new();
        let (mut scalars, mut buffers) = (0, 0);
        for (p, t) in f.params.iter().skip(1).zip(&info.params) {
            let field = match t {
                Ty::GpuBuffer(_) => {
                    buffers += 1;
                    format!("c{}", buffers - 1)
                }
                _ => {
                    scalars += 1;
                    format!("s{}", scalars - 1)
                }
            };
            params.push(WgslParam { name: wgsl_name(&p.name.name), ty: t.clone(), field });
        }
        let mut w = Wgsl {
            types: &self.checked.types,
            structs: &self.checked.structs,
            out: String::new(),
            indent: 0,
            temps: 0,
            counts: HashMap::new(),
        };
        let mut s = String::new();
        // The uniform block: the range, then the values in order (scalars
        // and buffers' counts, as the runtime writes them).
        let _ = writeln!(s, "// Tessel kernel `{}` (see docs/gpu.md).", f.name.name);
        s.push_str("struct P {\n    n0: u32,\n    n1: u32,\n    n2: u32,\n    row: u32,\n");
        for p in &params {
            let t = match &p.ty {
                Ty::GpuBuffer(_) => "u32",
                Ty::Float32 => "f32",
                Ty::Int => "i32",
                _ => "u32",
            };
            let _ = writeln!(s, "    {}: {t},", p.field);
        }
        s.push_str("}\n@group(0) @binding(0) var<uniform> p: P;\n");
        let mut binding = 1;
        for p in &params {
            if let Ty::GpuBuffer(e) = &p.ty {
                let et = if **e == Ty::Int { "i32" } else { "f32" };
                let _ = writeln!(s, "@group(0) @binding({binding}) var<storage, read_write> {}: array<{et}>;", p.name);
                binding += 1;
                w.counts.insert(p.name.clone(), format!("p.{}", p.field));
            }
        }
        s.push_str("\nfn t_div(a: i32, b: i32) -> i32 {\n    if (b == 0i) { return a; }\n    if (b == -1i) { return 0i - a; }\n    return a / b;\n}\n");
        s.push_str("\nfn t_rem(a: i32, b: i32) -> i32 {\n    if (b == 0i || b == -1i) { return 0i; }\n    return a % b;\n}\n");
        s.push_str("\nfn t_round(x: f32) -> f32 {\n    if (abs(x) >= 8388608.0f) { return x; }\n    return trunc(x + select(-0.5f, 0.5f, x >= 0.0f));\n}\n\n");
        let size = match info.dims {
            1 => "256",
            2 => "16, 16",
            _ => "8, 8, 4",
        };
        let _ = writeln!(s, "@compute @workgroup_size({size})\nfn main(@builtin(global_invocation_id) g: vec3<u32>) {{");
        let index = wgsl_name(&f.params[0].name.name);
        match info.dims {
            1 => {
                s.push_str("    let l = g.x + g.y * p.row;\n    if (l >= p.n0) { return; }\n");
                let _ = writeln!(s, "    let {index}: i32 = i32(l);");
            }
            2 => {
                s.push_str("    if (g.x >= p.n0 || g.y >= p.n1) { return; }\n");
                let _ = writeln!(s, "    let {index}: vec2<i32> = vec2<i32>(i32(g.x), i32(g.y));");
            }
            _ => {
                s.push_str("    if (g.x >= p.n0 || g.y >= p.n1 || g.z >= p.n2) { return; }\n");
                let _ = writeln!(s, "    let {index}: vec3<i32> = vec3<i32>(i32(g.x), i32(g.y), i32(g.z));");
            }
        }
        for p in &params {
            match &p.ty {
                Ty::GpuBuffer(_) => {}
                Ty::Bool => {
                    let _ = writeln!(s, "    let {}: bool = p.{} != 0u;", p.name, p.field);
                }
                _ => {
                    let _ = writeln!(s, "    let {} = p.{};", p.name, p.field);
                }
            }
        }
        w.indent = 1;
        w.block(&f.body);
        s.push_str(&w.out);
        s.push_str("}\n");
        s
    }

    // ===== the native version =====

    /// `void kernel(u64* args, i64 lo, i64 hi, i64 n0, i64 n1)`: the kernel
    /// for linear indices `lo..hi`.
    fn kernel_cpu(&mut self, name: &str, f: &FnDecl, info: &KernelInfo) -> FunctionValue<'ctx> {
        let p = self.ptr_ty();
        let i64t = self.i64();
        let i32t = self.ctx.i32_type();
        let fty = self.ctx.void_type().fn_type(&[p.into(), i64t.into(), i64t.into(), i64t.into(), i64t.into()], false);
        let func = self.module.add_function(&format!("tsl_kcpu_{name}"), fty, Some(Linkage::Internal));
        let saved = self.b.get_insert_block();
        let entry = self.ctx.append_basic_block(func, "entry");
        self.b.position_at_end(entry);
        let args = func.get_nth_param(0).unwrap().into_pointer_value();
        let (lo, hi) = (func.get_nth_param(1).unwrap().into_int_value(), func.get_nth_param(2).unwrap().into_int_value());
        let (n0, n1) = (func.get_nth_param(3).unwrap().into_int_value(), func.get_nth_param(4).unwrap().into_int_value());
        // The values, once.
        let mut locals: Vec<(String, Local<'ctx>)> = Vec::new();
        for (i, (prm, t)) in f.params.iter().skip(1).zip(&info.params).enumerate() {
            let at = unsafe { self.b.build_gep(i64t, args, &[i64t.const_int(i as u64, false)], "") }.unwrap();
            let raw = self.b.build_load(i64t, at, "").unwrap().into_int_value();
            let local = match t {
                Ty::GpuBuffer(e) => {
                    let slice = self.b.build_int_to_ptr(raw, p, "").unwrap();
                    let data = self.b.build_load(p, slice, "data").unwrap().into_pointer_value();
                    let len_at = unsafe { self.b.build_gep(i64t, slice, &[i64t.const_int(1, false)], "") }.unwrap();
                    let len = self.b.build_load(i64t, len_at, "len").unwrap().into_int_value();
                    Local::Buffer { data, len, ints: **e == Ty::Int }
                }
                Ty::Float32 => {
                    let bits = self.b.build_int_truncate(raw, i32t, "").unwrap();
                    let v = self.b.build_bit_cast(bits, self.ctx.f32_type(), "").unwrap();
                    let slot = self.b.build_alloca(self.ctx.f32_type(), &prm.name.name).unwrap();
                    self.b.build_store(slot, v).unwrap();
                    Local::Slot(slot, Ty::Float32)
                }
                Ty::Int => {
                    let v = self.b.build_int_truncate(raw, i32t, "").unwrap();
                    let slot = self.b.build_alloca(i32t, &prm.name.name).unwrap();
                    self.b.build_store(slot, v).unwrap();
                    Local::Slot(slot, Ty::Int)
                }
                _ => {
                    let v = self.b.build_int_compare(IntPredicate::NE, raw, i64t.const_zero(), "").unwrap();
                    let slot = self.b.build_alloca(self.ctx.bool_type(), &prm.name.name).unwrap();
                    self.b.build_store(slot, v).unwrap();
                    Local::Slot(slot, Ty::Bool)
                }
            };
            locals.push((prm.name.name.clone(), local));
        }
        // for l in lo..hi
        let l_slot = self.b.build_alloca(i64t, "l").unwrap();
        self.b.build_store(l_slot, lo).unwrap();
        let (head, body, latch, done) = (
            self.ctx.append_basic_block(func, "head"),
            self.ctx.append_basic_block(func, "body"),
            self.ctx.append_basic_block(func, "next"),
            self.ctx.append_basic_block(func, "done"),
        );
        self.b.build_unconditional_branch(head).unwrap();
        self.b.position_at_end(head);
        let l = self.b.build_load(i64t, l_slot, "").unwrap().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, l, hi, "").unwrap();
        self.b.build_conditional_branch(more, body, done).unwrap();
        self.b.position_at_end(body);
        let to32 = |g: &Self, v: IntValue<'ctx>| g.b.build_int_truncate(v, i32t, "").unwrap();
        let index = match info.dims {
            1 => {
                let slot = self.b.build_alloca(i32t, "i").unwrap();
                let v = to32(self, l);
                self.b.build_store(slot, v).unwrap();
                Local::Slot(slot, Ty::Int)
            }
            2 => {
                let i = self.b.build_int_signed_rem(l, n0, "").unwrap();
                let j = self.b.build_int_signed_div(l, n0, "").unwrap();
                Local::Tuple(vec![to32(self, i), to32(self, j)])
            }
            _ => {
                let i = self.b.build_int_signed_rem(l, n0, "").unwrap();
                let t = self.b.build_int_signed_div(l, n0, "").unwrap();
                let j = self.b.build_int_signed_rem(t, n1, "").unwrap();
                let k = self.b.build_int_signed_div(t, n1, "").unwrap();
                Local::Tuple(vec![to32(self, i), to32(self, j), to32(self, k)])
            }
        };
        locals.push((f.params[0].name.name.clone(), index));
        let mut k = KernelCpu { func, locals, loops: Vec::new(), next: latch };
        self.kcpu_block(&mut k, &f.body);
        if self.b.get_insert_block().unwrap().get_terminator().is_none() {
            self.b.build_unconditional_branch(latch).unwrap();
        }
        self.b.position_at_end(latch);
        let l = self.b.build_load(i64t, l_slot, "").unwrap().into_int_value();
        let l2 = self.b.build_int_add(l, i64t.const_int(1, false), "").unwrap();
        self.b.build_store(l_slot, l2).unwrap();
        self.b.build_unconditional_branch(head).unwrap();
        self.b.position_at_end(done);
        self.b.build_return(None).unwrap();
        if let Some(bb) = saved {
            self.b.position_at_end(bb);
        }
        func
    }

    fn kty(&self, e: &Expr) -> Ty {
        self.checked.types.get(&e.id).cloned().unwrap_or(Ty::Error)
    }

    fn kcpu_block(&mut self, k: &mut KernelCpu<'ctx>, b: &Block) {
        let mark = k.locals.len();
        for s in &b.stmts {
            self.kcpu_stmt(k, s);
        }
        k.locals.truncate(mark);
    }

    /// A new block to go on in after a jump (whatever's after is unreachable).
    fn kcpu_after_jump(&mut self, k: &KernelCpu<'ctx>) {
        let bb = self.ctx.append_basic_block(k.func, "after");
        self.b.position_at_end(bb);
    }

    fn kslot_ty(&self, t: &Ty) -> inkwell::types::BasicTypeEnum<'ctx> {
        match t {
            Ty::Int => self.ctx.i32_type().into(),
            Ty::Float32 => self.ctx.f32_type().into(),
            _ => self.ctx.bool_type().into(),
        }
    }

    /// A slot in the function's first block (so it's one slot, not one per
    /// round of a loop).
    fn kalloca(&mut self, k: &KernelCpu<'ctx>, t: &Ty, name: &str) -> PointerValue<'ctx> {
        let entry = k.func.get_first_basic_block().unwrap();
        let here = self.b.get_insert_block().unwrap();
        match entry.get_first_instruction() {
            Some(i) => self.b.position_before(&i),
            None => self.b.position_at_end(entry),
        }
        let lt = self.kslot_ty(t);
        let slot = self.b.build_alloca(lt, name).unwrap();
        self.b.position_at_end(here);
        slot
    }

    fn kcpu_stmt(&mut self, k: &mut KernelCpu<'ctx>, s: &Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) => {
                let t = self.kty(&d.value);
                let v = self.kcpu_expr(k, &d.value);
                let local = match v {
                    KV::Tuple(parts) => Local::Tuple(parts),
                    v => {
                        let slot = self.kalloca(k, &t, &d.name.name);
                        self.b.build_store(slot, v.basic()).unwrap();
                        Local::Slot(slot, t)
                    }
                };
                k.locals.push((d.name.name.clone(), local));
            }
            Stmt::Assign { target, op, value, .. } => {
                let v = self.kcpu_expr(k, value);
                let t = self.kty(target);
                let (place, old) = match &target.kind {
                    ExprKind::Name(n) => {
                        let Some(Local::Slot(slot, st)) = k.local(n) else { unreachable!("checked") };
                        let old = self.b.build_load(self.kslot_ty(&st), slot, "").unwrap();
                        (slot, old)
                    }
                    ExprKind::Index(object, index) => {
                        let ExprKind::Name(n) = &object.kind else { unreachable!("checked") };
                        let i = self.kcpu_expr(k, index).int();
                        let Some(Local::Buffer { data, len, ints }) = k.local(n) else { unreachable!("checked") };
                        let at = self.kbuffer_at(data, len, i, ints);
                        let et = if ints { self.ctx.i32_type().into() } else { self.ctx.f32_type().into() };
                        let old = self.b.build_load::<inkwell::types::BasicTypeEnum>(et, at, "").unwrap();
                        (at, old)
                    }
                    _ => unreachable!("checked"),
                };
                let new = match op {
                    AssignOp::Set => v,
                    AssignOp::Op(op) => self.kbinary(*op, KV::of(old, &t), v, &t),
                };
                self.b.build_store(place, new.basic()).unwrap();
            }
            Stmt::Return { .. } => {
                self.b.build_unconditional_branch(k.next).unwrap();
                self.kcpu_after_jump(k);
            }
            Stmt::Break(_) => {
                let (_, out) = *k.loops.last().expect("checked: in a loop");
                self.b.build_unconditional_branch(out).unwrap();
                self.kcpu_after_jump(k);
            }
            Stmt::Continue(_) => {
                let (cont, _) = *k.loops.last().expect("checked: in a loop");
                self.b.build_unconditional_branch(cont).unwrap();
                self.kcpu_after_jump(k);
            }
            Stmt::For { var, iter, body, .. } => {
                let ExprKind::Binary(BinOp::Range, a, b) = &iter.kind else { unreachable!("checked") };
                let a = self.kcpu_expr(k, a).int();
                let end = self.kcpu_expr(k, b).int();
                let slot = self.kalloca(k, &Ty::Int, &var.name);
                self.b.build_store(slot, a).unwrap();
                let (head, work, step, out) = (
                    self.ctx.append_basic_block(k.func, "for"),
                    self.ctx.append_basic_block(k.func, "for_body"),
                    self.ctx.append_basic_block(k.func, "for_step"),
                    self.ctx.append_basic_block(k.func, "for_out"),
                );
                self.b.build_unconditional_branch(head).unwrap();
                self.b.position_at_end(head);
                let i32t = self.ctx.i32_type();
                let v = self.b.build_load(i32t, slot, "").unwrap().into_int_value();
                let more = self.b.build_int_compare(IntPredicate::SLT, v, end, "").unwrap();
                self.b.build_conditional_branch(more, work, out).unwrap();
                self.b.position_at_end(work);
                k.locals.push((var.name.clone(), Local::Slot(slot, Ty::Int)));
                k.loops.push((step, out));
                self.kcpu_block(k, body);
                k.loops.pop();
                k.locals.pop();
                if self.b.get_insert_block().unwrap().get_terminator().is_none() {
                    self.b.build_unconditional_branch(step).unwrap();
                }
                self.b.position_at_end(step);
                let v = self.b.build_load(i32t, slot, "").unwrap().into_int_value();
                let v = self.b.build_int_add(v, i32t.const_int(1, false), "").unwrap();
                self.b.build_store(slot, v).unwrap();
                self.b.build_unconditional_branch(head).unwrap();
                self.b.position_at_end(out);
            }
            Stmt::While { cond, body, .. } => {
                let Cond::Expr(c) = cond else { unreachable!("checked") };
                let (head, work, out) = (
                    self.ctx.append_basic_block(k.func, "while"),
                    self.ctx.append_basic_block(k.func, "while_body"),
                    self.ctx.append_basic_block(k.func, "while_out"),
                );
                self.b.build_unconditional_branch(head).unwrap();
                self.b.position_at_end(head);
                let c = self.kcpu_expr(k, c).int();
                self.b.build_conditional_branch(c, work, out).unwrap();
                self.b.position_at_end(work);
                k.loops.push((head, out));
                self.kcpu_block(k, body);
                k.loops.pop();
                if self.b.get_insert_block().unwrap().get_terminator().is_none() {
                    self.b.build_unconditional_branch(head).unwrap();
                }
                self.b.position_at_end(out);
            }
            Stmt::Expr(e) => {
                if matches!(e.kind, ExprKind::If { .. } | ExprKind::Block(_)) {
                    self.kcpu_value(k, e, false);
                }
            }
            Stmt::State(_) | Stmt::Fn(_) => unreachable!("checked"),
        }
    }

    /// The address of element `i` of a buffer: `i` taken as unsigned, and
    /// past the end as the last (as wgpu's bounds checks do).
    fn kbuffer_at(&mut self, data: PointerValue<'ctx>, len: IntValue<'ctx>, i: IntValue<'ctx>, ints: bool) -> PointerValue<'ctx> {
        let i64t = self.i64();
        let u = self.b.build_int_z_extend(i, i64t, "").unwrap();
        let one = i64t.const_int(1, false);
        let size = self.b.build_select(self.b.build_int_compare(IntPredicate::SGT, len, i64t.const_zero(), "").unwrap(), len, one, "").unwrap().into_int_value();
        let last = self.b.build_int_sub(size, one, "").unwrap();
        let past = self.b.build_int_compare(IntPredicate::UGT, u, last, "").unwrap();
        let at = self.b.build_select(past, last, u, "").unwrap().into_int_value();
        let et: inkwell::types::BasicTypeEnum = if ints { self.ctx.i32_type().into() } else { self.ctx.f32_type().into() };
        unsafe { self.b.build_gep(et, data, &[at], "") }.unwrap()
    }

    fn kcpu_expr(&mut self, k: &mut KernelCpu<'ctx>, e: &Expr) -> KV<'ctx> {
        self.kcpu_value(k, e, true).expect("a value")
    }

    fn kcpu_value(&mut self, k: &mut KernelCpu<'ctx>, e: &Expr, want: bool) -> Option<KV<'ctx>> {
        let t = self.kty(e);
        let i32t = self.ctx.i32_type();
        let f32t = self.ctx.f32_type();
        Some(match &e.kind {
            ExprKind::Int(n) => match t {
                Ty::Float32 => KV::F(f32t.const_float(*n as f32 as f64)),
                _ => KV::I(i32t.const_int(*n as i32 as u32 as u64, false)),
            },
            ExprKind::Float(x) => KV::F(f32t.const_float(*x as f32 as f64)),
            ExprKind::Bool(b) => KV::B(self.ctx.bool_type().const_int(*b as u64, false)),
            ExprKind::Name(n) => match k.local(n).expect("checked") {
                Local::Slot(slot, st) => {
                    let v = self.b.build_load(self.kslot_ty(&st), slot, n).unwrap();
                    KV::of(v, &st)
                }
                Local::Tuple(parts) => KV::Tuple(parts),
                Local::Buffer { .. } => unreachable!("buffers are only indexed"),
            },
            ExprKind::Binary(op, a, b) => {
                let at = self.kty(a);
                let (x, y) = (self.kcpu_expr(k, a), self.kcpu_expr(k, b));
                self.kbinary(*op, x, y, &at)
            }
            ExprKind::Unary(op, x) => {
                let v = self.kcpu_expr(k, x);
                match (op, v) {
                    (UnaryOp::Neg, KV::F(f)) => KV::F(self.b.build_float_neg(f, "").unwrap()),
                    (UnaryOp::Neg, KV::I(i)) => KV::I(self.b.build_int_sub(i32t.const_zero(), i, "").unwrap()),
                    (UnaryOp::Not, KV::B(b)) => KV::B(self.b.build_not(b, "").unwrap()),
                    (_, KV::I(i)) => KV::I(self.b.build_not(i, "").unwrap()),
                    _ => unreachable!("checked"),
                }
            }
            ExprKind::Index(object, index) => {
                let ExprKind::Name(n) = &object.kind else { unreachable!("checked") };
                let i = self.kcpu_expr(k, index).int();
                let Some(Local::Buffer { data, len, ints }) = k.local(n) else { unreachable!("checked") };
                let at = self.kbuffer_at(data, len, i, ints);
                if ints {
                    KV::I(self.b.build_load(i32t, at, "").unwrap().into_int_value())
                } else {
                    KV::F(self.b.build_load(f32t, at, "").unwrap().into_float_value())
                }
            }
            ExprKind::Member { object, name, .. } => {
                let ExprKind::Name(n) = &object.kind else { unreachable!("checked") };
                match (k.local(n).expect("checked"), name.name.as_str()) {
                    (Local::Buffer { len, .. }, _) => KV::I(self.b.build_int_truncate(len, i32t, "").unwrap()),
                    (Local::Tuple(parts), i) => KV::I(parts[i.parse::<usize>().unwrap()]),
                    _ => unreachable!("checked"),
                }
            }
            ExprKind::Call { callee, args, .. } => {
                let ExprKind::Name(f) = &callee.kind else { unreachable!("checked") };
                let at = args.first().map(|a| self.kty(&a.value)).unwrap_or(Ty::Error);
                let vals: Vec<KV> = args.iter().map(|a| self.kcpu_expr(k, &a.value)).collect();
                self.kcall(f, &at, vals)
            }
            ExprKind::If { cond, then, otherwise } => {
                let slot = if want && t != Ty::Void { Some(self.kalloca(k, &t, "if")) } else { None };
                self.kif(k, cond, then, otherwise.as_ref(), slot);
                match slot {
                    Some(s) => {
                        let v = self.b.build_load(self.kslot_ty(&t), s, "").unwrap();
                        KV::of(v, &t)
                    }
                    None => return None,
                }
            }
            ExprKind::Block(b) => {
                let slot = if want && t != Ty::Void { Some(self.kalloca(k, &t, "block")) } else { None };
                self.kblock_value(k, b, slot);
                match slot {
                    Some(s) => {
                        let v = self.b.build_load(self.kslot_ty(&t), s, "").unwrap();
                        KV::of(v, &t)
                    }
                    None => return None,
                }
            }
            _ => unreachable!("checked: not in the GPU subset"),
        })
    }

    fn kblock_value(&mut self, k: &mut KernelCpu<'ctx>, b: &Block, slot: Option<PointerValue<'ctx>>) {
        let mark = k.locals.len();
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            match (s, slot) {
                (Stmt::Expr(e), Some(slot)) if i + 1 == n => {
                    let v = self.kcpu_expr(k, e);
                    self.b.build_store(slot, v.basic()).unwrap();
                }
                _ => self.kcpu_stmt(k, s),
            }
        }
        k.locals.truncate(mark);
    }

    fn kif(&mut self, k: &mut KernelCpu<'ctx>, cond: &Cond, then: &Block, otherwise: Option<&Else>, slot: Option<PointerValue<'ctx>>) {
        let Cond::Expr(c) = cond else { unreachable!("checked") };
        let c = self.kcpu_expr(k, c).int();
        let (yes, no, done): (BasicBlock, BasicBlock, BasicBlock) = (
            self.ctx.append_basic_block(k.func, "then"),
            self.ctx.append_basic_block(k.func, "else"),
            self.ctx.append_basic_block(k.func, "endif"),
        );
        self.b.build_conditional_branch(c, yes, no).unwrap();
        self.b.position_at_end(yes);
        self.kblock_value(k, then, slot);
        if self.b.get_insert_block().unwrap().get_terminator().is_none() {
            self.b.build_unconditional_branch(done).unwrap();
        }
        self.b.position_at_end(no);
        match otherwise {
            Some(Else::Block(b)) => self.kblock_value(k, b, slot),
            Some(Else::If(x)) => {
                let ExprKind::If { cond, then, otherwise } = &x.kind else { unreachable!() };
                self.kif(k, cond, then, otherwise.as_ref(), slot);
            }
            None => {}
        }
        if self.b.get_insert_block().unwrap().get_terminator().is_none() {
            self.b.build_unconditional_branch(done).unwrap();
        }
        self.b.position_at_end(done);
    }

    /// `a op b` with the GPU's rules (see the top).
    fn kbinary(&mut self, op: BinOp, a: KV<'ctx>, b: KV<'ctx>, t: &Ty) -> KV<'ctx> {
        let i32t = self.ctx.i32_type();
        match (a, b) {
            (KV::F(x), KV::F(y)) => match op {
                BinOp::Add => KV::F(self.b.build_float_add(x, y, "").unwrap()),
                BinOp::Sub => KV::F(self.b.build_float_sub(x, y, "").unwrap()),
                BinOp::Mul => KV::F(self.b.build_float_mul(x, y, "").unwrap()),
                BinOp::Div => KV::F(self.b.build_float_div(x, y, "").unwrap()),
                BinOp::Rem => KV::F(self.b.build_float_rem(x, y, "").unwrap()),
                _ => {
                    let p = match op {
                        BinOp::Eq => FloatPredicate::OEQ,
                        BinOp::Ne => FloatPredicate::UNE,
                        BinOp::Lt => FloatPredicate::OLT,
                        BinOp::Le => FloatPredicate::OLE,
                        BinOp::Gt => FloatPredicate::OGT,
                        _ => FloatPredicate::OGE,
                    };
                    KV::B(self.b.build_float_compare(p, x, y, "").unwrap())
                }
            },
            (KV::I(x), KV::I(y)) => match op {
                BinOp::Add => KV::I(self.b.build_int_add(x, y, "").unwrap()),
                BinOp::Sub => KV::I(self.b.build_int_sub(x, y, "").unwrap()),
                BinOp::Mul => KV::I(self.b.build_int_mul(x, y, "").unwrap()),
                BinOp::Div | BinOp::Rem => {
                    // By zero (or MIN / -1): `a` for `/`, 0 for `%` (WGSL's).
                    let zero = self.b.build_int_compare(IntPredicate::EQ, y, i32t.const_zero(), "").unwrap();
                    let min = self.b.build_int_compare(IntPredicate::EQ, x, i32t.const_int(i32::MIN as u32 as u64, false), "").unwrap();
                    let neg1 = self.b.build_int_compare(IntPredicate::EQ, y, i32t.const_all_ones(), "").unwrap();
                    let over = self.b.build_and(min, neg1, "").unwrap();
                    let odd = self.b.build_or(zero, over, "").unwrap();
                    let safe = self.b.build_select(odd, i32t.const_int(1, false), y, "").unwrap().into_int_value();
                    if op == BinOp::Div {
                        let q = self.b.build_int_signed_div(x, safe, "").unwrap();
                        KV::I(self.b.build_select(odd, x, q, "").unwrap().into_int_value())
                    } else {
                        let r = self.b.build_int_signed_rem(x, safe, "").unwrap();
                        KV::I(self.b.build_select(odd, i32t.const_zero(), r, "").unwrap().into_int_value())
                    }
                }
                BinOp::BitAnd => KV::I(self.b.build_and(x, y, "").unwrap()),
                BinOp::BitOr => KV::I(self.b.build_or(x, y, "").unwrap()),
                BinOp::BitXor => KV::I(self.b.build_xor(x, y, "").unwrap()),
                BinOp::Shl | BinOp::Shr => {
                    let s = self.b.build_and(y, i32t.const_int(31, false), "").unwrap();
                    KV::I(if op == BinOp::Shl {
                        self.b.build_left_shift(x, s, "").unwrap()
                    } else {
                        self.b.build_right_shift(x, s, true, "").unwrap()
                    })
                }
                _ => {
                    let p = match op {
                        BinOp::Eq => IntPredicate::EQ,
                        BinOp::Ne => IntPredicate::NE,
                        BinOp::Lt => IntPredicate::SLT,
                        BinOp::Le => IntPredicate::SLE,
                        BinOp::Gt => IntPredicate::SGT,
                        _ => IntPredicate::SGE,
                    };
                    KV::B(self.b.build_int_compare(p, x, y, "").unwrap())
                }
            },
            (KV::B(x), KV::B(y)) => KV::B(match op {
                BinOp::And | BinOp::BitAnd => self.b.build_and(x, y, "").unwrap(),
                BinOp::Or | BinOp::BitOr => self.b.build_or(x, y, "").unwrap(),
                BinOp::Eq => self.b.build_int_compare(IntPredicate::EQ, x, y, "").unwrap(),
                _ => self.b.build_xor(x, y, "").unwrap(),
            }),
            _ => {
                let _ = t;
                unreachable!("checked: both sides the same type")
            }
        }
    }

    fn kmath(&mut self, name: &str, vals: &[FloatValue<'ctx>]) -> FloatValue<'ctx> {
        let f32t = self.ctx.f32_type();
        let f = match name {
            "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "floor" | "ceil" | "pow" => format!("llvm.{name}.f32"),
            "abs" => "llvm.fabs.f32".into(),
            "min" => "llvm.minnum.f32".into(),
            "max" => "llvm.maxnum.f32".into(),
            other => format!("{other}f"),
        };
        let n = if matches!(name, "pow" | "atan2" | "min" | "max") { 2 } else { 1 };
        let func = self.module.get_function(&f).unwrap_or_else(|| {
            let params: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![f32t.into(); n];
            self.module.add_function(&f, f32t.fn_type(&params, false), None)
        });
        let args: Vec<inkwell::values::BasicMetadataValueEnum> = vals.iter().map(|v| (*v).into()).collect();
        self.b.build_call(func, &args, "").unwrap().try_as_basic_value().basic().unwrap().into_float_value()
    }

    fn kcall(&mut self, f: &str, at: &Ty, vals: Vec<KV<'ctx>>) -> KV<'ctx> {
        let (i32t, f32t) = (self.ctx.i32_type(), self.ctx.f32_type());
        match (f, at) {
            ("Float32", Ty::Int) => KV::F(self.b.build_signed_int_to_float(vals[0].int(), f32t, "").unwrap()),
            ("Int", Ty::Float32) => {
                let intr = inkwell::intrinsics::Intrinsic::find("llvm.fptosi.sat").unwrap();
                let d = intr.get_declaration(&self.module, &[i32t.into(), f32t.into()]).unwrap();
                let v = self.b.build_call(d, &[vals[0].float().into()], "").unwrap().try_as_basic_value().basic().unwrap();
                KV::I(v.into_int_value())
            }
            ("Float32" | "Int", _) => vals[0].clone(),
            ("abs" | "min" | "max", Ty::Int) => {
                let mut r = vals[0].int();
                if f == "abs" {
                    let neg = self.b.build_int_sub(i32t.const_zero(), r, "").unwrap();
                    let is_neg = self.b.build_int_compare(IntPredicate::SLT, r, i32t.const_zero(), "").unwrap();
                    return KV::I(self.b.build_select(is_neg, neg, r, "").unwrap().into_int_value());
                }
                let p = if f == "min" { IntPredicate::SLT } else { IntPredicate::SGT };
                for v in &vals[1..] {
                    let y = v.int();
                    let keep = self.b.build_int_compare(p, r, y, "").unwrap();
                    r = self.b.build_select(keep, r, y, "").unwrap().into_int_value();
                }
                KV::I(r)
            }
            ("min" | "max", _) => {
                let mut r = vals[0].float();
                for v in &vals[1..] {
                    let y = v.float();
                    r = self.kmath(f, &[r, y]);
                }
                KV::F(r)
            }
            ("round", _) => {
                // As the WGSL's `t_round`: halves away from zero.
                let x = vals[0].float();
                let big = {
                    let a = self.kmath("abs", &[x]);
                    self.b.build_float_compare(FloatPredicate::OGE, a, f32t.const_float(8388608.0), "").unwrap()
                };
                let pos = self.b.build_float_compare(FloatPredicate::OGE, x, f32t.const_zero(), "").unwrap();
                let half = self.b.build_select(pos, f32t.const_float(0.5), f32t.const_float(-0.5), "").unwrap().into_float_value();
                let s = self.b.build_float_add(x, half, "").unwrap();
                let intr = inkwell::intrinsics::Intrinsic::find("llvm.trunc").unwrap();
                let d = intr.get_declaration(&self.module, &[f32t.into()]).unwrap();
                let t = self.b.build_call(d, &[s.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_float_value();
                KV::F(self.b.build_select(big, x, t, "").unwrap().into_float_value())
            }
            _ => {
                let fs: Vec<FloatValue> = vals.iter().map(|v| v.float()).collect();
                KV::F(self.kmath(f, &fs))
            }
        }
    }

    // ===== buffers and `gpu` =====

    /// `GPUBuffer<T>(count: n)` and `GPUBuffer(from: list)`.
    pub(super) fn gpu_buffer_init(&mut self, args: &[crate::ast::Arg], t: &Ty) -> Val<'ctx> {
        let Ty::GpuBuffer(e) = t else { unreachable!() };
        let ints = self.i8().const_int((**e == Ty::Int) as u64, false);
        let a = &args[0];
        if a.label.as_ref().is_some_and(|l| l.name == "count") {
            let n = self.expr_as(&a.value, &Ty::Int).unwrap();
            self.call_rt("tsl_gpu_buffer_new", &[ints.into(), n]).unwrap()
        } else {
            let lt = Ty::list((**e).clone());
            let list = self.expr_as(&a.value, &lt).unwrap();
            let b = self.call_rt("tsl_gpu_buffer_from", &[ints.into(), list]).unwrap();
            self.release(list, &lt);
            b
        }
    }

    /// `buffer.read()`, `.write(list)`, `.fill(value)`, `.copy(from:)`.
    pub(super) fn gpu_buffer_method(&mut self, recv: &Ty, name: &str, object: &Expr, args: &[crate::ast::Arg]) -> Option<Val<'ctx>> {
        let Ty::GpuBuffer(e) = recv else { unreachable!() };
        let b = self.expr_as(object, recv).unwrap();
        self.hold(b, recv);
        let r = match name {
            "read" => {
                let ti = self.type_info(e);
                self.call_rt("tsl_gpu_buffer_read", &[b, ti.into()])
            }
            "write" => {
                let lt = Ty::list((**e).clone());
                let list = self.expr_as(&args[0].value, &lt).unwrap();
                self.call_rt("tsl_gpu_buffer_write", &[b, list]);
                self.release(list, &lt);
                None
            }
            "fill" => {
                let v = self.expr_as(&args[0].value, e).unwrap();
                let bits = if **e == Ty::Float32 {
                    let w = self.b.build_bit_cast(v, self.ctx.i32_type(), "").unwrap().into_int_value();
                    self.b.build_int_z_extend(w, self.i64(), "").unwrap()
                } else {
                    v.into_int_value()
                };
                self.call_rt("tsl_gpu_buffer_fill", &[b, bits.into()]);
                None
            }
            _ => {
                let src = self.expr_as(&args[0].value, recv).unwrap();
                self.call_rt("tsl_gpu_buffer_copy", &[b, src]);
                self.release(src, recv);
                None
            }
        };
        self.unhold(1);
        self.release(b, recv);
        r
    }

    /// `gpu.run`, `gpu.wait`, `gpu.sum`, `gpu.min`, `gpu.max`, `gpu.dot`.
    pub(super) fn gpu_builtin(&mut self, name: &str, args: &[crate::ast::Arg]) -> Option<Val<'ctx>> {
        let i64t = self.i64();
        match name {
            "gpu.wait" => {
                self.call_rt("tsl_gpu_wait", &[]);
                None
            }
            "gpu.sum" | "gpu.min" | "gpu.max" | "gpu.dot" => {
                let op = match name {
                    "gpu.sum" => 0,
                    "gpu.min" => 1,
                    "gpu.max" => 2,
                    _ => 3,
                };
                let bt = Ty::GpuBuffer(Box::new(Ty::Float32));
                let a = self.expr_as(&args[0].value, &bt).unwrap();
                self.hold(a, &bt);
                let b = match args.get(1) {
                    Some(x) => self.expr_as(&x.value, &bt).unwrap(),
                    None => self.ptr_ty().const_null().into(),
                };
                let r = self.call_rt("tsl_gpu_reduce", &[i64t.const_int(op, false).into(), a, b]);
                self.unhold(1);
                self.release(a, &bt);
                if args.len() > 1 {
                    self.release(b, &bt);
                }
                r
            }
            _ => {
                // gpu.run(kernel, over: size, values…)
                let ExprKind::Name(k) = &args[0].value.kind else { unreachable!("checked") };
                let info = self.checked.kernels[k].clone();
                let desc = self.kernel_descs[k];
                let over = &args[1].value;
                let ot = self.ty(over);
                let ov = self.expr_as(over, &ot).unwrap();
                let mut n = [i64t.const_int(1, false); 3];
                if info.dims == 1 {
                    n[0] = ov.into_int_value();
                } else {
                    for (i, slot) in n.iter_mut().enumerate().take(info.dims) {
                        *slot = self.b.build_extract_value(ov.into_struct_value(), i as u32, "").unwrap().into_int_value();
                    }
                }
                // The values, each as 64 bits.
                let count = info.params.len().max(1) as u32;
                let arr_t = i64t.array_type(count);
                let arr = self.alloca(arr_t.into(), "gpu_args");
                let mut owned = Vec::new();
                for (i, (a, t)) in args[2..].iter().zip(&info.params).enumerate() {
                    let v = self.expr_as(&a.value, t).unwrap();
                    let word = match t {
                        Ty::GpuBuffer(_) => {
                            self.hold(v, t);
                            owned.push((v, t.clone()));
                            self.b.build_ptr_to_int(v.into_pointer_value(), i64t, "").unwrap()
                        }
                        Ty::Float32 => {
                            let w = self.b.build_bit_cast(v, self.ctx.i32_type(), "").unwrap().into_int_value();
                            self.b.build_int_z_extend(w, i64t, "").unwrap()
                        }
                        Ty::Int => v.into_int_value(),
                        _ => self.b.build_int_z_extend(v.into_int_value(), i64t, "").unwrap(),
                    };
                    let at = unsafe { self.b.build_gep(arr_t, arr, &[i64t.const_zero(), i64t.const_int(i as u64, false)], "") }.unwrap();
                    self.b.build_store(at, word).unwrap();
                }
                self.call_rt("tsl_gpu_run", &[desc.into(), n[0].into(), n[1].into(), n[2].into(), arr.into()]);
                self.unhold(owned.len());
                for (v, t) in owned {
                    self.release(v, &t);
                }
                None
            }
        }
    }
}

/// While a kernel's native version is written.
pub(super) struct KernelCpu<'ctx> {
    func: FunctionValue<'ctx>,
    locals: Vec<(String, Local<'ctx>)>,
    /// `continue`'s and `break`'s blocks, innermost last.
    loops: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    /// Where `return` goes: the next index.
    next: BasicBlock<'ctx>,
}

impl<'ctx> KernelCpu<'ctx> {
    fn local(&self, n: &str) -> Option<Local<'ctx>> {
        self.locals.iter().rev().find(|(name, _)| name == n).map(|(_, l)| l.clone())
    }
}

/// A value in a kernel's native version.
#[derive(Clone)]
pub(super) enum KV<'ctx> {
    I(IntValue<'ctx>),
    F(FloatValue<'ctx>),
    B(IntValue<'ctx>),
    Tuple(Vec<IntValue<'ctx>>),
}

impl<'ctx> KV<'ctx> {
    fn of(v: inkwell::values::BasicValueEnum<'ctx>, t: &Ty) -> KV<'ctx> {
        match t {
            Ty::Float32 => KV::F(v.into_float_value()),
            Ty::Int => KV::I(v.into_int_value()),
            _ => KV::B(v.into_int_value()),
        }
    }

    fn int(&self) -> IntValue<'ctx> {
        match self {
            KV::I(v) | KV::B(v) => *v,
            _ => unreachable!("checked: an Int"),
        }
    }

    fn float(&self) -> FloatValue<'ctx> {
        match self {
            KV::F(v) => *v,
            _ => unreachable!("checked: a Float32"),
        }
    }

    fn basic(&self) -> inkwell::values::BasicValueEnum<'ctx> {
        match self {
            KV::I(v) | KV::B(v) => (*v).into(),
            KV::F(v) => (*v).into(),
            KV::Tuple(_) => unreachable!("a tuple isn't stored"),
        }
    }
}
