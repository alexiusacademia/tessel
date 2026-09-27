//! LLVM code generation.
//!
//! Memory model (see docs/design.md §10.2): every expression produces an
//! *owned* value (+1 reference); storing it moves it, and every variable is
//! released when its scope ends. Heap objects (strings, lists, dictionaries,
//! boxes) are reference counted by the runtime.
//!
//! Value layout:
//! - `Int` i64, `Float` f64, `Bool` i1, `String`/lists/dictionaries: ptr
//! - structs: LLVM structs (plain values)
//! - enums: `i32` tag, or `{ i32 tag, ptr payload-box }` if any case has values
//! - optionals: `{ i1 present, T }`
//! - functions: `{ ptr fn, ptr env-box }`; every function takes `env` first
//!
//! Functions receive owned parameters and release them on exit. Methods also
//! take a pointer to their receiver (`self`) after `env`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetData, TargetMachine};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};

use crate::ast::*;
use crate::check::{ArgRef, CallInfo, Callee, Checked};
use crate::diag::Diagnostic;
use crate::source::{SourceMap, Span};
use crate::types::{FnSig, Ty};

mod ui;

/// Byte offset of a box's data (matches the runtime's `BOX_DATA`).
const BOX_DATA: u64 = 32;

/// Compiles a checked program with a `fn main()` to a native object file.
pub fn compile(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    out: &Path,
    optimize: bool,
) -> Result<(), Vec<Diagnostic>> {
    Target::initialize_native(&InitializationConfig::default()).map_err(|e| vec![internal(e)])?;
    let mut triple = TargetMachine::get_default_triple();
    // On macOS, target the oldest supported version rather than this machine's.
    let host = triple.as_str().to_string_lossy().to_string();
    if let Some((arch, _)) = host.split_once("-apple-darwin") {
        triple = inkwell::targets::TargetTriple::create(&format!("{arch}-apple-macosx11.0.0"));
    }
    let target = Target::from_triple(&triple).map_err(|e| vec![internal(e.to_string())])?;
    let level = if optimize { OptimizationLevel::Default } else { OptimizationLevel::None };
    let tm = target
        .create_target_machine(
            &triple,
            &TargetMachine::get_host_cpu_name().to_string(),
            &TargetMachine::get_host_cpu_features().to_string(),
            level,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| vec![internal("can't create a target machine for this computer")])?;

    let ctx = Context::create();
    let module = ctx.create_module("tessel");
    module.set_triple(&triple);
    let td = tm.get_target_data();
    module.set_data_layout(&td.get_data_layout());

    let mut g = Gen::new(&ctx, module, td, program, checked, sources);
    g.program();
    if !g.errors.is_empty() {
        return Err(g.errors);
    }
    if let Err(e) = g.module.verify() {
        return Err(vec![internal(format!("generated code failed to verify: {}", e.to_string()))]);
    }
    if optimize {
        g.module
            .run_passes("default<O2>", &tm, PassBuilderOptions::create())
            .map_err(|e| vec![internal(e.to_string())])?;
    }
    tm.write_to_file(&g.module, FileType::Object, out).map_err(|e| vec![internal(e.to_string())])
}

fn internal(msg: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Span::default(), format!("internal compiler error: {}", msg.into()))
}

/// What a closure is made from.
enum Body<'b> {
    /// An ordinary closure `{ a, b in … }`.
    Fn(&'b [Ident], &'b Block),
    /// A view's content block; calling it emits the views.
    Content(&'b Block),
    /// A single view expression, wrapped so it can be passed around.
    ContentExpr(&'b Expr),
    /// A place (for `bind`); calling it returns a pointer to the storage.
    Locator(&'b Expr),
}

/// How a closure captures a variable.
#[derive(Clone, Copy)]
enum Capture {
    Value,
    Box,
    Binding,
    /// A stable list element: the list slot pointer and the index.
    Elem,
    /// A list element found through a locator for its list, and the index.
    ElemVia,
}

/// Where a variable lives.
#[derive(Clone, Copy)]
enum Store<'ctx> {
    /// An alloca holding the value (owned).
    Slot(PointerValue<'ctx>),
    /// An alloca holding a box pointer; the value is in the box (owned).
    /// Used for `var`s captured by closures, so changes are shared.
    Boxed(PointerValue<'ctx>),
    /// Element `idx` of the list in `slot`: a loop variable over a changeable list.
    /// `stable` if `slot` outlives the function (it's in a view's state record),
    /// so closures may keep pointing at it.
    ListElem { slot: PointerValue<'ctx>, idx: PointerValue<'ctx>, stable: bool, relocate: Option<PointerValue<'ctx>> },
    /// Element `idx` of the list a locator (in `locator`, a slot holding
    /// `{ fn, env }`) points at: a loop variable captured by a closure when
    /// the list's storage may move. Found again on every use.
    ListElemVia { locator: PointerValue<'ctx>, idx: PointerValue<'ctx> },
    /// A captured value inside a closure's environment (not owned).
    EnvValue(PointerValue<'ctx>),
    /// A captured box pointer inside a closure's environment (not owned).
    EnvBox(PointerValue<'ctx>),
    /// A field of the method's receiver.
    Field(u32),
    /// The method's receiver itself.
    SelfRef,
    /// A view `state` value: field `i` of the view's state record.
    State(u32),
    /// A `bind` parameter: `slot` holds a locator closure that returns a
    /// pointer to the storage. `owned` if this function must release it.
    Binding { slot: PointerValue<'ctx>, owned: bool },
    /// A function declared inside a view (called with the state record).
    ViewFn,
}

#[derive(Clone)]
struct Var<'ctx> {
    name: String,
    ty: Ty,
    store: Store<'ctx>,
}

struct FnCtx<'ctx> {
    func: FunctionValue<'ctx>,
    scopes: Vec<Vec<Var<'ctx>>>,
    ret: Ty,
    /// Pointer to the receiver in methods (and closures inside them).
    self_ptr: Option<(PointerValue<'ctx>, Ty)>,
    /// `var`s captured by closures in this function; they live in boxes.
    boxed: HashSet<String>,
    /// Inside a view (or its functions and closures): its state record.
    record: Option<Record<'ctx>>,
}

/// A view's persistent state: the data of a box holding a struct of its
/// `state` values.
#[derive(Clone)]
struct Record<'ctx> {
    data: PointerValue<'ctx>,
    ty: StructType<'ctx>,
    view: String,
}

struct Gen<'ctx, 'a> {
    ctx: &'ctx Context,
    module: Module<'ctx>,
    b: Builder<'ctx>,
    td: TargetData,
    program: &'a Program,
    checked: &'a Checked,
    sources: &'a SourceMap,
    errors: Vec<Diagnostic>,

    fn_decls: HashMap<String, &'a FnDecl>,
    fns: HashMap<String, FunctionValue<'ctx>>,
    method_decls: HashMap<(String, String), (&'a FnDecl, Ty)>,
    methods: HashMap<(String, String), FunctionValue<'ctx>>,
    struct_decls: HashMap<usize, &'a StructDecl>,
    struct_types: HashMap<usize, StructType<'ctx>>,
    type_infos: HashMap<String, PointerValue<'ctx>>,
    helpers: HashMap<String, FunctionValue<'ctx>>,
    strings: HashMap<String, PointerValue<'ctx>>,
    locs: HashMap<(u32, u32), PointerValue<'ctx>>,
    closures: usize,
    stack: Vec<FnCtx<'ctx>>,

    /// Views by name (the `app` included), and what's compiled for them.
    view_decls: HashMap<String, ViewSource<'a>>,
    view_builds: HashMap<String, FunctionValue<'ctx>>,
    view_fns: HashMap<(String, String), FunctionValue<'ctx>>,
    records: HashMap<String, (StructType<'ctx>, Vec<Ty>)>,
    app: Option<&'a AppDecl>,
}

/// A view's source: a `view` declaration or the `app`.
#[derive(Clone, Copy)]
struct ViewSource<'a> {
    params: &'a [Param],
    body: &'a Block,
}

type Val<'ctx> = BasicValueEnum<'ctx>;

impl<'ctx, 'a> Gen<'ctx, 'a> {
    fn new(
        ctx: &'ctx Context,
        module: Module<'ctx>,
        td: TargetData,
        program: &'a Program,
        checked: &'a Checked,
        sources: &'a SourceMap,
    ) -> Self {
        Gen {
            ctx,
            module,
            b: ctx.create_builder(),
            td,
            program,
            checked,
            sources,
            errors: Vec::new(),
            fn_decls: HashMap::new(),
            fns: HashMap::new(),
            method_decls: HashMap::new(),
            methods: HashMap::new(),
            struct_decls: HashMap::new(),
            struct_types: HashMap::new(),
            type_infos: HashMap::new(),
            helpers: HashMap::new(),
            strings: HashMap::new(),
            locs: HashMap::new(),
            closures: 0,
            stack: Vec::new(),
            view_decls: HashMap::new(),
            view_builds: HashMap::new(),
            view_fns: HashMap::new(),
            records: HashMap::new(),
            app: None,
        }
    }

    fn unsupported(&mut self, span: Span, what: &str) {
        self.errors.push(Diagnostic::error(span, format!("{what} can't be compiled yet")).help(
            "UI features (views, `app`, `fetch`, `after`) arrive in milestone M3; use `fn main()` for now",
        ).clone());
    }

    // ===== program =====

    fn program(&mut self) {
        let program = self.program;
        for item in &program.items {
            match item {
                Item::Struct(d) => {
                    let id = self.checked.structs.iter().position(|s| s.name == d.name.name).unwrap();
                    self.struct_decls.insert(id, d);
                    for m in &d.methods {
                        self.method_decls.insert((d.name.name.clone(), m.name.name.clone()), (m, Ty::Struct(id)));
                    }
                }
                Item::Enum(d) => {
                    let id = self.checked.enums.iter().position(|e| e.name == d.name.name).unwrap();
                    for m in &d.methods {
                        self.method_decls.insert((d.name.name.clone(), m.name.name.clone()), (m, Ty::Enum(id)));
                    }
                }
                Item::Fn(f) => {
                    self.fn_decls.insert(f.name.name.clone(), f);
                }
                Item::View(v) => {
                    self.view_decls.insert(v.name.name.clone(), ViewSource { params: &v.params, body: &v.body });
                }
                Item::App(a) => {
                    self.view_decls.insert(a.name.name.clone(), ViewSource { params: &[], body: &a.body });
                    self.app = Some(a);
                }
            }
        }
        if !self.fn_decls.contains_key("main") && self.app.is_none() {
            if self.errors.is_empty() {
                self.errors.push(Diagnostic::error(Span::default(), "this program has no `app` or `fn main()` to run"));
            }
            return;
        }
        if !self.errors.is_empty() {
            return;
        }

        // Declare every function first so bodies can call each other.
        let fn_names: Vec<String> = self.fn_decls.keys().cloned().collect();
        for name in fn_names {
            let sig = self.checked.fns[&name].clone();
            let ty = self.fn_type(&sig.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>(), &sig.ret, false);
            let f = self.module.add_function(&format!("tsl_f_{name}"), ty, Some(Linkage::Internal));
            self.fns.insert(name, f);
        }
        let method_keys: Vec<(String, String)> = self.method_decls.keys().cloned().collect();
        for key in method_keys {
            let (_, owner) = self.method_decls[&key].clone();
            let sig = self.method_sig(&owner, &key.1);
            let ty = self.fn_type(&sig.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>(), &sig.ret, true);
            let f = self.module.add_function(&format!("tsl_m_{}_{}", key.0, key.1), ty, Some(Linkage::Internal));
            self.methods.insert(key, f);
        }

        self.declare_views();

        for (name, decl) in self.fn_decls.clone() {
            let f = self.fns[&name];
            let sig = self.checked.fns[&name].clone();
            let params: Vec<(Ident, Ty)> =
                decl.params.iter().zip(&sig.params).map(|(p, s)| (p.name.clone(), s.ty.clone())).collect();
            self.function_body(f, &params, &decl.body, &sig.ret, None, 1);
        }
        for (key, (decl, owner)) in self.method_decls.clone() {
            let f = self.methods[&key];
            let sig = self.method_sig(&owner, &key.1);
            let params: Vec<(Ident, Ty)> =
                decl.params.iter().zip(&sig.params).map(|(p, s)| (p.name.clone(), s.ty.clone())).collect();
            let self_ptr = f.get_nth_param(1).unwrap().into_pointer_value();
            self.function_body(f, &params, &decl.body, &sig.ret, Some((self_ptr, owner)), 2);
        }

        self.compile_views();

        // Entry point called by the runtime.
        let entry = self.module.add_function("tsl_main", self.ctx.void_type().fn_type(&[], false), None);
        self.b.position_at_end(self.ctx.append_basic_block(entry, "entry"));
        match self.app {
            Some(app) => self.run_app(app, entry),
            None => {
                let null = self.ptr_ty().const_null();
                self.b.build_call(self.fns["main"], &[null.into()], "").unwrap();
                self.b.build_return(None).unwrap();
            }
        }
    }

    fn method_sig(&self, owner: &Ty, name: &str) -> FnSig {
        let methods = match owner {
            Ty::Struct(id) => &self.checked.structs[*id].methods,
            Ty::Enum(id) => &self.checked.enums[*id].methods,
            _ => unreachable!(),
        };
        methods.iter().find(|m| m.name == name).unwrap().sig.clone()
    }

    fn function_body(
        &mut self,
        func: FunctionValue<'ctx>,
        params: &[(Ident, Ty)],
        body: &Block,
        ret: &Ty,
        self_info: Option<(PointerValue<'ctx>, Ty)>,
        first_param: u32,
    ) {
        let entry = self.ctx.append_basic_block(func, "entry");
        self.b.position_at_end(entry);
        let mut scope = Vec::new();
        if let Some((_, owner)) = &self_info {
            scope.extend(self.receiver_vars(owner));
        }
        let boxed = boxed_vars(body);
        self.stack.push(FnCtx { func, scopes: vec![scope], ret: ret.clone(), self_ptr: self_info, boxed, record: None });
        for (i, (name, ty)) in params.iter().enumerate() {
            let value = func.get_nth_param(first_param + i as u32).unwrap();
            self.declare_value(&name.name, ty, Some(value));
        }
        let value = self.block(body, Some(ret));
        self.finish_function(value);
        self.stack.pop();
    }

    /// Fields and `self` of a method's receiver.
    fn receiver_vars(&self, owner: &Ty) -> Vec<Var<'ctx>> {
        let mut vars = vec![Var { name: "self".into(), ty: owner.clone(), store: Store::SelfRef }];
        if let Ty::Struct(id) = owner {
            for (i, f) in self.checked.structs[*id].fields.iter().enumerate() {
                vars.push(Var { name: f.name.clone(), ty: f.ty.clone(), store: Store::Field(i as u32) });
            }
        }
        vars
    }

    /// Emits the return at the end of a function body.
    fn finish_function(&mut self, value: Option<Val<'ctx>>) {
        if self.terminated() {
            return;
        }
        self.release_all_scopes();
        match value {
            Some(v) => self.b.build_return(Some(&v)).unwrap(),
            None => self.b.build_return(None).unwrap(),
        };
    }

    // ===== types =====

    fn ptr_ty(&self) -> inkwell::types::PointerType<'ctx> {
        self.ctx.ptr_type(AddressSpace::default())
    }

    fn i64(&self) -> inkwell::types::IntType<'ctx> {
        self.ctx.i64_type()
    }

    fn i8(&self) -> inkwell::types::IntType<'ctx> {
        self.ctx.i8_type()
    }

    fn has_payload(&self, id: usize) -> bool {
        self.checked.enums[id].cases.iter().any(|c| !c.fields.is_empty())
    }

    fn llty(&mut self, t: &Ty) -> Option<BasicTypeEnum<'ctx>> {
        Some(match t {
            Ty::Void => return None,
            Ty::Int => self.i64().into(),
            Ty::Float => self.ctx.f64_type().into(),
            Ty::Bool => self.ctx.bool_type().into(),
            Ty::Str | Ty::List(_) | Ty::Dict(..) => self.ptr_ty().into(),
            // A view value is a closure that emits the view when called.
            Ty::View => self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false).into(),
            Ty::Range => self.ctx.struct_type(&[self.i64().into(), self.i64().into()], false).into(),
            Ty::Opt(inner) => {
                let inner = self.llty(inner).expect("optional of nothing");
                self.ctx.struct_type(&[self.ctx.bool_type().into(), inner], false).into()
            }
            Ty::Fn(..) => self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false).into(),
            Ty::Enum(id) => {
                if self.has_payload(*id) {
                    self.ctx.struct_type(&[self.ctx.i32_type().into(), self.ptr_ty().into()], false).into()
                } else {
                    self.ctx.i32_type().into()
                }
            }
            Ty::Struct(id) => self.struct_ty(*id).into(),
            Ty::Error | Ty::Any => unreachable!("unresolved type in codegen"),
        })
    }

    fn struct_ty(&mut self, id: usize) -> StructType<'ctx> {
        if let Some(t) = self.struct_types.get(&id) {
            return *t;
        }
        let def = &self.checked.structs[id];
        let t = self.ctx.opaque_struct_type(&format!("tsl.{}", def.name));
        self.struct_types.insert(id, t);
        let fields: Vec<Ty> = def.fields.iter().map(|f| f.ty.clone()).collect();
        let body: Vec<BasicTypeEnum> = fields.iter().map(|f| self.llty(f).unwrap()).collect();
        t.set_body(&body, false);
        t
    }

    /// The payload struct of an enum case (its values, in order).
    fn payload_ty(&mut self, id: usize, case: usize) -> StructType<'ctx> {
        let fields: Vec<Ty> = self.checked.enums[id].cases[case].fields.iter().map(|f| f.ty.clone()).collect();
        let body: Vec<BasicTypeEnum> = fields.iter().map(|f| self.llty(f).unwrap()).collect();
        self.ctx.struct_type(&body, false)
    }

    fn fn_type(&mut self, params: &[Ty], ret: &Ty, method: bool) -> FunctionType<'ctx> {
        let mut ps: Vec<BasicMetadataTypeEnum> = vec![self.ptr_ty().into()];
        if method {
            ps.push(self.ptr_ty().into());
        }
        for p in params {
            ps.push(self.llty(p).unwrap().into());
        }
        match self.llty(ret) {
            Some(r) => r.fn_type(&ps, false),
            None => self.ctx.void_type().fn_type(&ps, false),
        }
    }

    fn key(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "i".into(),
            Ty::Float => "f".into(),
            Ty::Bool => "b".into(),
            Ty::Str => "s".into(),
            Ty::Void => "v".into(),
            Ty::View => "w".into(),
            Ty::Range => "r".into(),
            Ty::List(t) => format!("L{}", self.key(t)),
            Ty::Dict(k, v) => format!("D{}{}", self.key(k), self.key(v)),
            Ty::Opt(t) => format!("O{}", self.key(t)),
            Ty::Fn(ps, r) => format!("F{}_{}", ps.iter().map(|p| self.key(p)).collect::<String>(), self.key(r)),
            Ty::Struct(id) => format!("S{}{}", self.checked.structs[*id].name.len(), self.checked.structs[*id].name),
            Ty::Enum(id) => format!("E{}{}", self.checked.enums[*id].name.len(), self.checked.enums[*id].name),
            Ty::Error | Ty::Any => "x".into(),
        }
    }

    fn needs_rc(&self, t: &Ty) -> bool {
        match t {
            Ty::Str | Ty::List(_) | Ty::Dict(..) | Ty::Fn(..) | Ty::View => true,
            Ty::Enum(id) => self.has_payload(*id),
            Ty::Struct(id) => self.checked.structs[*id].fields.iter().any(|f| self.needs_rc(&f.ty)),
            Ty::Opt(inner) => self.needs_rc(inner),
            _ => false,
        }
    }

    // ===== small builders =====

    fn terminated(&self) -> bool {
        self.b.get_insert_block().and_then(|b| b.get_terminator()).is_some()
    }

    fn func(&self) -> FunctionValue<'ctx> {
        self.stack.last().unwrap().func
    }

    fn block_named(&self, name: &str) -> BasicBlock<'ctx> {
        self.ctx.append_basic_block(self.func(), name)
    }

    /// Continues emitting in a fresh block after a return or panic.
    fn continue_dead(&mut self) {
        let dead = self.block_named("dead");
        self.b.position_at_end(dead);
    }

    fn br(&self, to: BasicBlock<'ctx>) {
        if !self.terminated() {
            self.b.build_unconditional_branch(to).unwrap();
        }
    }

    /// An alloca in the entry block (so it's allocated once per call).
    fn alloca(&self, t: BasicTypeEnum<'ctx>, name: &str) -> PointerValue<'ctx> {
        let entry = self.func().get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(first) => tmp.position_before(&first),
            None => tmp.position_at_end(entry),
        }
        tmp.build_alloca(t, name).unwrap()
    }

    fn load(&mut self, t: &Ty, p: PointerValue<'ctx>) -> Val<'ctx> {
        let lt = self.llty(t).unwrap();
        self.b.build_load(lt, p, "").unwrap()
    }

    fn spill(&mut self, t: &Ty, v: Val<'ctx>) -> PointerValue<'ctx> {
        let lt = self.llty(t).unwrap();
        let p = self.alloca(lt, "tmp");
        self.b.build_store(p, v).unwrap();
        p
    }

    fn undef(&mut self, t: &Ty) -> Option<Val<'ctx>> {
        self.llty(t).map(undef_of)
    }

    fn call_rt(&mut self, name: &str, args: &[Val<'ctx>]) -> Option<Val<'ctx>> {
        let f = self.rt(name);
        let args: Vec<BasicMetadataValueEnum> = args.iter().map(|a| (*a).into()).collect();
        self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic()
    }

    /// Declares a runtime function on first use.
    fn rt(&mut self, name: &str) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let p = self.ptr_ty().as_basic_type_enum();
        let i = self.i64().as_basic_type_enum();
        let f64t = self.ctx.f64_type().as_basic_type_enum();
        let i8t = self.i8().as_basic_type_enum();
        let i32t = self.ctx.i32_type().as_basic_type_enum();
        let (params, ret): (Vec<BasicTypeEnum>, Option<BasicTypeEnum>) = match name {
            "tsl_retain" | "tsl_release" | "tsl_print" => (vec![p], None),
            "tsl_box_new" => (vec![p], Some(p)),
            "tsl_box_eq" => (vec![p, p], Some(i8t)),
            "tsl_panic" => (vec![p, p], None),
            "tsl_str_concat" => (vec![p, i], Some(p)),
            "tsl_str_from_int" => (vec![i], Some(p)),
            "tsl_str_from_float" => (vec![f64t], Some(p)),
            "tsl_str_from_bool" => (vec![i8t], Some(p)),
            "tsl_str_eq" | "tsl_str_contains" | "tsl_str_has_prefix" | "tsl_str_has_suffix" => {
                (vec![p, p], Some(i8t))
            }
            "tsl_str_cmp" => (vec![p, p], Some(i32t)),
            "tsl_str_hash" | "tsl_str_count" => (vec![p], Some(i)),
            "tsl_str_trim" | "tsl_str_uppercase" | "tsl_str_lowercase" => (vec![p], Some(p)),
            "tsl_str_replace" => (vec![p, p, p], Some(p)),
            "tsl_str_split" => (vec![p, p], Some(p)),
            "tsl_str_to_int" | "tsl_str_to_float" => (vec![p, p], Some(i8t)),
            "tsl_list_new" => (vec![p, i], Some(p)),
            "tsl_list_len" | "tsl_dict_len" => (vec![p], Some(i)),
            "tsl_list_get" | "tsl_list_elem_mut" => (vec![p, i, p], Some(p)),
            "tsl_list_push" => (vec![p, p], None),
            "tsl_list_insert" => (vec![p, p, i, p], None),
            "tsl_list_remove" => (vec![p, i, p, p], None),
            "tsl_list_concat" | "tsl_list_joined" => (vec![p, p], Some(p)),
            "tsl_list_eq" | "tsl_list_contains" | "tsl_dict_eq" | "tsl_dict_contains" => (vec![p, p], Some(i8t)),
            "tsl_list_reversed" | "tsl_dict_keys" | "tsl_dict_values" => (vec![p], Some(p)),
            "tsl_list_sorted" => (vec![p, p, p], Some(p)),
            "tsl_dict_new" => (vec![p, p], Some(p)),
            "tsl_dict_get" | "tsl_dict_remove" => (vec![p, p, p], Some(i8t)),
            "tsl_dict_set" => (vec![p, p, p], None),
            "tsl_after" => (vec![f64t, p, p], None),
            "tsl_read_file" | "tsl_list_folder" | "tsl_environment" | "tsl_str_lines" => (vec![p], Some(p)),
            "tsl_write_file" => (vec![p, p], Some(i8t)),
            "tsl_file_exists" | "tsl_is_folder" | "tsl_delete_file" => (vec![p], Some(i8t)),
            "tsl_current_folder" => (vec![], Some(p)),
            "tsl_run_command" => (vec![p, p, p, p, p, p], Some(i)),
            "tsl_stop_command" => (vec![i], None),
            "tsl_str_substring" => (vec![p, i, i], Some(p)),
            "tsl_str_find" => (vec![p, p], Some(i)),
            "tsl_ui_shortcut" => (vec![p, p, p], None),
            "tsl_ui_code_editor" => (vec![p, p, p, p, p], None),
            "tsl_ui_on_appear" | "tsl_ui_on_submit" => (vec![p, p], None),
            "tsl_ui_begin_tabs" => (vec![p, p, p, p], None),
            "tsl_ui_divider" | "tsl_ui_autofocus" => (vec![], None),
            "tsl_ui_icon" => (vec![i32t], None),
            "tsl_ui_svg" => (vec![p], None),
            "tsl_ui_tab_item" => (vec![p], None),
            "tsl_ui_hover_background" => (vec![i32t, f64t], None),
            "tsl_create_folder" => (vec![p], Some(i8t)),
            "tsl_rename_file" => (vec![p, p], Some(i8t)),
            "tsl_fetch" => (vec![p, p, p], None),
            "tsl_ui_text" | "tsl_ui_image" => (vec![p], None),
            "tsl_ui_button" | "tsl_ui_text_field" | "tsl_ui_toggle" => (vec![p, p, p], None),
            "tsl_ui_spacer" | "tsl_ui_end" | "tsl_ui_view_end" => (vec![], None),
            "tsl_ui_begin" => (vec![i32t, f64t, i32t], None),
            "tsl_ui_begin_picker" | "tsl_ui_on_tap" | "tsl_ui_tag" => (vec![p, p], None),
            "tsl_ui_view_begin" => (vec![i, p, p], Some(p)),
            "tsl_ui_padding" | "tsl_ui_opacity" => (vec![f64t], None),
            "tsl_ui_font" => (vec![f64t, i8t, i32t, i8t], None),
            "tsl_ui_text_alignment" => (vec![i32t], None),
            "tsl_ui_color" => (vec![i32t], None),
            "tsl_ui_background" => (vec![i32t, f64t], None),
            "tsl_ui_border" => (vec![i32t, f64t, f64t], None),
            "tsl_ui_frame" => (vec![i8t, f64t, i8t, f64t, i8t, f64t, i8t, f64t, i8t, i32t], None),
            "tsl_ui_hidden" | "tsl_ui_disabled" => (vec![i8t], None),
            "tsl_ui_next_id" => (vec![i], None),
            "tsl_ui_run" => (vec![p, f64t, f64t, p, p], None),
            _ => panic!("unknown runtime function {name}"),
        };
        let ps: Vec<BasicMetadataTypeEnum> = params.into_iter().map(Into::into).collect();
        let ty = match ret {
            Some(r) => r.fn_type(&ps, false),
            None => self.ctx.void_type().fn_type(&ps, false),
        };
        self.module.add_function(name, ty, Some(Linkage::External))
    }

    /// A C boolean (i8) from the runtime as an i1.
    fn truthy(&self, v: Option<Val<'ctx>>) -> IntValue<'ctx> {
        let v = v.unwrap().into_int_value();
        self.b.build_int_compare(IntPredicate::NE, v, v.get_type().const_zero(), "").unwrap()
    }

    fn to_c_bool(&self, v: IntValue<'ctx>) -> Val<'ctx> {
        self.b.build_int_z_extend(v, self.i8(), "").unwrap().into()
    }

    /// An immortal string constant.
    fn str_lit(&mut self, text: &str) -> PointerValue<'ctx> {
        if let Some(p) = self.strings.get(text) {
            return *p;
        }
        let bytes = self.ctx.const_string(text.as_bytes(), false);
        let ty = self.ctx.struct_type(
            &[self.i64().into(), self.ptr_ty().into(), self.i64().into(), bytes.get_type().into()],
            false,
        );
        let g = self.module.add_global(ty, None, "str");
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&ty.const_named_struct(&[
            self.i64().const_int(u64::MAX, true).into(),
            self.ptr_ty().const_null().into(),
            self.i64().const_int(text.len() as u64, false).into(),
            bytes.into(),
        ]));
        let p = g.as_pointer_value();
        self.strings.insert(text.to_string(), p);
        p
    }

    fn c_str(&mut self, text: &str) -> PointerValue<'ctx> {
        let bytes = self.ctx.const_string(text.as_bytes(), true);
        let g = self.module.add_global(bytes.get_type(), None, "cstr");
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&bytes);
        g.as_pointer_value()
    }

    /// A source location for runtime errors.
    fn loc(&mut self, span: Span) -> PointerValue<'ctx> {
        if let Some(p) = self.locs.get(&(span.file, span.lo)) {
            return *p;
        }
        let file = self.sources.file(span.file);
        let (line, col) = file.line_col(span.lo as usize);
        let name = file.name.clone();
        let fname = self.c_str(&name);
        let i32t = self.ctx.i32_type();
        let ty = self.ctx.struct_type(&[self.ptr_ty().into(), i32t.into(), i32t.into()], false);
        let g = self.module.add_global(ty, None, "loc");
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&ty.const_named_struct(&[
            fname.into(),
            i32t.const_int(line as u64, false).into(),
            i32t.const_int(col as u64, false).into(),
        ]));
        let p = g.as_pointer_value();
        self.locs.insert((span.file, span.lo), p);
        p
    }

    /// Stops the program with `msg` at `span` when `cond` is true.
    fn panic_if(&mut self, cond: IntValue<'ctx>, msg: &str, span: Span) {
        let bad = self.block_named("panic");
        let ok = self.block_named("ok");
        self.b.build_conditional_branch(cond, bad, ok).unwrap();
        self.b.position_at_end(bad);
        let m = self.c_str(msg);
        let l = self.loc(span);
        self.call_rt("tsl_panic", &[m.into(), l.into()]);
        self.b.build_unreachable().unwrap();
        self.b.position_at_end(ok);
    }

    // ===== reference counting =====

    fn retain(&mut self, v: Val<'ctx>, t: &Ty) {
        self.rc(v, t, true);
    }

    fn release(&mut self, v: Val<'ctx>, t: &Ty) {
        self.rc(v, t, false);
    }

    fn rc(&mut self, v: Val<'ctx>, t: &Ty, retain: bool) {
        if !self.needs_rc(t) {
            return;
        }
        let f = if retain { "tsl_retain" } else { "tsl_release" };
        match t {
            Ty::Str | Ty::List(_) | Ty::Dict(..) => {
                self.call_rt(f, &[v]);
            }
            Ty::Fn(..) | Ty::Enum(_) | Ty::View => {
                let p = self.b.build_extract_value(v.into_struct_value(), 1, "").unwrap();
                self.call_rt(f, &[p]);
            }
            Ty::Struct(_) | Ty::Opt(_) => {
                let helper = self.rc_helper(t, retain);
                let p = self.spill(t, v);
                self.b.build_call(helper, &[p.into()], "").unwrap();
            }
            _ => {}
        }
    }

    /// `void helper(ptr)`: retains or releases the value at `ptr`.
    fn rc_helper(&mut self, t: &Ty, retain: bool) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_{}_{}", if retain { "retain" } else { "release" }, self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let f = self.module.add_function(
            &name,
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        self.helpers.insert(name, f);
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            match t {
                Ty::Struct(id) => {
                    let st = g.struct_ty(*id);
                    let fields: Vec<Ty> = g.checked.structs[*id].fields.iter().map(|f| f.ty.clone()).collect();
                    for (i, ft) in fields.iter().enumerate() {
                        if g.needs_rc(ft) {
                            let fp = g.b.build_struct_gep(st, p, i as u32, "").unwrap();
                            let fv = g.load(ft, fp);
                            g.rc(fv, ft, retain);
                        }
                    }
                }
                Ty::Opt(inner) => {
                    let ot = g.llty(t).unwrap().into_struct_type();
                    let flag_p = g.b.build_struct_gep(ot, p, 0, "").unwrap();
                    let flag = g.b.build_load(g.ctx.bool_type(), flag_p, "").unwrap().into_int_value();
                    let some = g.block_named("some");
                    let done = g.block_named("done");
                    g.b.build_conditional_branch(flag, some, done).unwrap();
                    g.b.position_at_end(some);
                    let vp = g.b.build_struct_gep(ot, p, 1, "").unwrap();
                    let v = g.load(inner, vp);
                    g.rc(v, inner, retain);
                    g.br(done);
                    g.b.position_at_end(done);
                }
                _ => {
                    let v = g.load(t, p);
                    g.rc(v, t, retain);
                }
            }
            g.b.build_return(None).unwrap();
        });
        f
    }

    /// Emits a helper function body, then restores the current position.
    fn in_helper(&mut self, f: FunctionValue<'ctx>, body: impl FnOnce(&mut Self)) {
        let saved = self.b.get_insert_block();
        self.stack.push(FnCtx {
            func: f,
            scopes: vec![Vec::new()],
            ret: Ty::Void,
            self_ptr: None,
            boxed: HashSet::new(),
            record: None,
        });
        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        body(self);
        self.stack.pop();
        if let Some(bb) = saved {
            self.b.position_at_end(bb);
        }
    }

    /// Runtime type descriptor (`TslType`) for values of type `t`.
    fn type_info(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let key = self.key(t);
        self.type_info_for(&key, t.clone(), None)
    }

    fn payload_info(&mut self, id: usize, case: usize) -> PointerValue<'ctx> {
        let key = format!("P{}_{case}", self.key(&Ty::Enum(id)));
        self.type_info_for(&key, Ty::Enum(id), Some(case))
    }

    fn type_info_for(&mut self, key: &str, t: Ty, payload: Option<usize>) -> PointerValue<'ctx> {
        if let Some(p) = self.type_infos.get(key) {
            return *p;
        }
        let (lt, rc) = match payload {
            Some(case) => {
                let Ty::Enum(id) = t else { unreachable!() };
                let fields: Vec<Ty> = self.checked.enums[id].cases[case].fields.iter().map(|f| f.ty.clone()).collect();
                (self.payload_ty(id, case).as_basic_type_enum(), fields.iter().any(|f| self.needs_rc(f)))
            }
            None => (self.llty(&t).unwrap(), self.needs_rc(&t)),
        };
        let size = self.td.get_abi_size(&lt);
        let align = self.td.get_abi_alignment(&lt) as u64;
        let null = self.ptr_ty().const_null();
        let (retain, release) = if rc {
            match payload {
                Some(case) => (self.payload_rc(key, &t, case, true), self.payload_rc(key, &t, case, false)),
                None => {
                    (self.rc_helper(&t, true).as_global_value().as_pointer_value(), self.rc_helper(&t, false).as_global_value().as_pointer_value())
                }
            }
        } else {
            (null, null)
        };
        let eq = match payload {
            Some(case) => self.payload_eq(key, &t, case),
            None if crate_equatable(&t) => self.eq_helper(&t).as_global_value().as_pointer_value(),
            None => null,
        };
        let hash = if payload.is_none() && hashable(&t, self) { self.hash_helper(&t) } else { null };
        let cmp = if payload.is_none() && matches!(t, Ty::Int | Ty::Float | Ty::Str) { self.cmp_helper(&t) } else { null };
        let i = self.i64();
        let p = self.ptr_ty();
        let ty = self.ctx.struct_type(&[i.into(), i.into(), p.into(), p.into(), p.into(), p.into(), p.into()], false);
        let g = self.module.add_global(ty, None, &format!("tsl_t_{key}"));
        g.set_linkage(Linkage::Private);
        g.set_constant(true);
        g.set_initializer(&ty.const_named_struct(&[
            i.const_int(size, false).into(),
            i.const_int(align, false).into(),
            retain.into(),
            release.into(),
            eq.into(),
            hash.into(),
            cmp.into(),
        ]));
        let ptr = g.as_pointer_value();
        self.type_infos.insert(key.to_string(), ptr);
        ptr
    }

    fn payload_rc(&mut self, key: &str, t: &Ty, case: usize, retain: bool) -> PointerValue<'ctx> {
        let Ty::Enum(id) = *t else { unreachable!() };
        let name = format!("tsl_h_{}_{key}", if retain { "retain" } else { "release" });
        let f = self.module.add_function(
            &name,
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            let pt = g.payload_ty(id, case);
            let fields: Vec<Ty> = g.checked.enums[id].cases[case].fields.iter().map(|f| f.ty.clone()).collect();
            for (i, ft) in fields.iter().enumerate() {
                let fp = g.b.build_struct_gep(pt, p, i as u32, "").unwrap();
                let v = g.load(ft, fp);
                g.rc(v, ft, retain);
            }
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    fn payload_eq(&mut self, key: &str, t: &Ty, case: usize) -> PointerValue<'ctx> {
        let Ty::Enum(id) = *t else { unreachable!() };
        let p = self.ptr_ty();
        let f = self.module.add_function(&format!("tsl_h_eq_{key}"), self.i8().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.in_helper(f, |g| {
            let (a, b) = (f.get_nth_param(0).unwrap().into_pointer_value(), f.get_nth_param(1).unwrap().into_pointer_value());
            let pt = g.payload_ty(id, case);
            let fields: Vec<Ty> = g.checked.enums[id].cases[case].fields.iter().map(|f| f.ty.clone()).collect();
            let mut all = g.ctx.bool_type().const_int(1, false);
            for (i, ft) in fields.iter().enumerate() {
                let ap = g.b.build_struct_gep(pt, a, i as u32, "").unwrap();
                let bp = g.b.build_struct_gep(pt, b, i as u32, "").unwrap();
                let (av, bv) = (g.load(ft, ap), g.load(ft, bp));
                let e = g.eq(av, bv, ft);
                all = g.b.build_and(all, e, "").unwrap();
            }
            let r = g.to_c_bool(all);
            g.b.build_return(Some(&r)).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `i8 helper(ptr a, ptr b)`: are the values equal?
    fn eq_helper(&mut self, t: &Ty) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_eq_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.i8().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        self.in_helper(f, |g| {
            let (a, b) = (f.get_nth_param(0).unwrap().into_pointer_value(), f.get_nth_param(1).unwrap().into_pointer_value());
            let r = match t {
                Ty::Struct(id) => {
                    let st = g.struct_ty(*id);
                    let fields: Vec<Ty> = g.checked.structs[*id].fields.iter().map(|f| f.ty.clone()).collect();
                    let mut all = g.ctx.bool_type().const_int(1, false);
                    for (i, ft) in fields.iter().enumerate() {
                        let ap = g.b.build_struct_gep(st, a, i as u32, "").unwrap();
                        let bp = g.b.build_struct_gep(st, b, i as u32, "").unwrap();
                        let (av, bv) = (g.load(ft, ap), g.load(ft, bp));
                        let e = g.eq(av, bv, ft);
                        all = g.b.build_and(all, e, "").unwrap();
                    }
                    all
                }
                Ty::Opt(inner) => {
                    let ot = g.llty(t).unwrap().into_struct_type();
                    let fa_p = g.b.build_struct_gep(ot, a, 0, "").unwrap();
                    let fb_p = g.b.build_struct_gep(ot, b, 0, "").unwrap();
                    let fa = g.b.build_load(g.ctx.bool_type(), fa_p, "").unwrap().into_int_value();
                    let fb = g.b.build_load(g.ctx.bool_type(), fb_p, "").unwrap().into_int_value();
                    let both = g.b.build_and(fa, fb, "").unwrap();
                    let cmp_bb = g.block_named("both");
                    let quick = g.block_named("quick");
                    g.b.build_conditional_branch(both, cmp_bb, quick).unwrap();
                    g.b.position_at_end(quick);
                    // Equal only if both are nil.
                    let same = g.b.build_int_compare(IntPredicate::EQ, fa, fb, "").unwrap();
                    let r = g.to_c_bool(same);
                    g.b.build_return(Some(&r)).unwrap();
                    g.b.position_at_end(cmp_bb);
                    let ap = g.b.build_struct_gep(ot, a, 1, "").unwrap();
                    let bp = g.b.build_struct_gep(ot, b, 1, "").unwrap();
                    let (av, bv) = (g.load(inner, ap), g.load(inner, bp));
                    g.eq(av, bv, inner)
                }
                _ => {
                    let (av, bv) = (g.load(t, a), g.load(t, b));
                    g.eq(av, bv, t)
                }
            };
            let r = g.to_c_bool(r);
            g.b.build_return(Some(&r)).unwrap();
        });
        f
    }

    fn hash_helper(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let name = format!("tsl_h_hash_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let f = self.module.add_function(&name, self.i64().fn_type(&[self.ptr_ty().into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            let v = g.load(t, p);
            let h: IntValue = match t {
                Ty::Str => g.call_rt("tsl_str_hash", &[v]).unwrap().into_int_value(),
                Ty::Int => v.into_int_value(),
                _ => g.b.build_int_z_extend(v.into_int_value(), g.i64(), "").unwrap(),
            };
            g.b.build_return(Some(&h)).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    fn cmp_helper(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let name = format!("tsl_h_cmp_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let i32t = self.ctx.i32_type();
        let f = self.module.add_function(&name, i32t.fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        self.in_helper(f, |g| {
            let (a, b) = (f.get_nth_param(0).unwrap().into_pointer_value(), f.get_nth_param(1).unwrap().into_pointer_value());
            let (av, bv) = (g.load(t, a), g.load(t, b));
            let r = if *t == Ty::Str {
                g.call_rt("tsl_str_cmp", &[av, bv]).unwrap().into_int_value()
            } else {
                let lt = g.compare(BinOp::Lt, av, bv, t);
                let gt = g.compare(BinOp::Gt, av, bv, t);
                let lt = g.b.build_int_z_extend(lt, i32t, "").unwrap();
                let gt = g.b.build_int_z_extend(gt, i32t, "").unwrap();
                g.b.build_int_sub(gt, lt, "").unwrap()
            };
            g.b.build_return(Some(&r)).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `a == b` for two values of type `t` (borrowed).
    fn eq(&mut self, a: Val<'ctx>, b: Val<'ctx>, t: &Ty) -> IntValue<'ctx> {
        match t {
            Ty::Int | Ty::Bool => {
                self.b.build_int_compare(IntPredicate::EQ, a.into_int_value(), b.into_int_value(), "").unwrap()
            }
            Ty::Float => self
                .b
                .build_float_compare(FloatPredicate::OEQ, a.into_float_value(), b.into_float_value(), "")
                .unwrap(),
            Ty::Str => {
                let r = self.call_rt("tsl_str_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::List(_) => {
                let r = self.call_rt("tsl_list_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::Dict(..) => {
                let r = self.call_rt("tsl_dict_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::Enum(id) if !self.has_payload(*id) => {
                self.b.build_int_compare(IntPredicate::EQ, a.into_int_value(), b.into_int_value(), "").unwrap()
            }
            Ty::Enum(_) => {
                let (sa, sb) = (a.into_struct_value(), b.into_struct_value());
                let ta = self.b.build_extract_value(sa, 0, "").unwrap().into_int_value();
                let tb = self.b.build_extract_value(sb, 0, "").unwrap().into_int_value();
                let same_tag = self.b.build_int_compare(IntPredicate::EQ, ta, tb, "").unwrap();
                let pa = self.b.build_extract_value(sa, 1, "").unwrap();
                let pb = self.b.build_extract_value(sb, 1, "").unwrap();
                let r = self.call_rt("tsl_box_eq", &[pa, pb]);
                let same_payload = self.truthy(r);
                self.b.build_and(same_tag, same_payload, "").unwrap()
            }
            Ty::Struct(_) | Ty::Opt(_) => {
                let f = self.eq_helper(t);
                let (pa, pb) = (self.spill(t, a), self.spill(t, b));
                let r = self.b.build_call(f, &[pa.into(), pb.into()], "").unwrap().try_as_basic_value().basic();
                self.truthy(r)
            }
            _ => unreachable!("values of this type can't be compared"),
        }
    }

    fn compare(&mut self, op: BinOp, a: Val<'ctx>, b: Val<'ctx>, t: &Ty) -> IntValue<'ctx> {
        match t {
            Ty::Float => {
                let p = match op {
                    BinOp::Lt => FloatPredicate::OLT,
                    BinOp::Le => FloatPredicate::OLE,
                    BinOp::Gt => FloatPredicate::OGT,
                    _ => FloatPredicate::OGE,
                };
                self.b.build_float_compare(p, a.into_float_value(), b.into_float_value(), "").unwrap()
            }
            Ty::Str => {
                let c = self.call_rt("tsl_str_cmp", &[a, b]).unwrap().into_int_value();
                let zero = c.get_type().const_zero();
                let p = int_pred(op);
                self.b.build_int_compare(p, c, zero, "").unwrap()
            }
            _ => self.b.build_int_compare(int_pred(op), a.into_int_value(), b.into_int_value(), "").unwrap(),
        }
    }

    // ===== variables and scopes =====

    fn ctx_mut(&mut self) -> &mut FnCtx<'ctx> {
        self.stack.last_mut().unwrap()
    }

    fn lookup(&self, name: &str) -> Option<Var<'ctx>> {
        self.stack.last()?.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|v| v.name == name).cloned()
    }

    /// Declares a variable holding `value` (owned, moved in).
    fn declare_value(&mut self, name: &str, ty: &Ty, value: Option<Val<'ctx>>) {
        let Some(value) = value else {
            return;
        };
        if name == "_" {
            self.release(value, ty);
            return;
        }
        let boxed = self.stack.last().unwrap().boxed.contains(name);
        let store = if boxed {
            let info = self.type_info(ty);
            let bx = self.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
            let data = self.box_data(bx);
            self.b.build_store(data, value).unwrap();
            let slot = self.alloca(self.ptr_ty().into(), name);
            self.b.build_store(slot, bx).unwrap();
            Store::Boxed(slot)
        } else {
            let lt = self.llty(ty).unwrap();
            let slot = self.alloca(lt, name);
            self.b.build_store(slot, value).unwrap();
            Store::Slot(slot)
        };
        self.ctx_mut().scopes.last_mut().unwrap().push(Var { name: name.to_string(), ty: ty.clone(), store });
    }

    fn push_var(&mut self, var: Var<'ctx>) {
        self.ctx_mut().scopes.last_mut().unwrap().push(var);
    }

    fn box_data(&self, bx: PointerValue<'ctx>) -> PointerValue<'ctx> {
        unsafe { self.b.build_gep(self.i8(), bx, &[self.i64().const_int(BOX_DATA, false)], "data").unwrap() }
    }

    fn push_scope(&mut self) {
        self.ctx_mut().scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        let scope = self.ctx_mut().scopes.pop().unwrap();
        if !self.terminated() {
            for v in scope.iter().rev() {
                self.release_var(v);
            }
        }
    }

    fn release_all_scopes(&mut self) {
        let vars: Vec<Var> = self.stack.last().unwrap().scopes.iter().flatten().cloned().collect();
        for v in vars.iter().rev() {
            self.release_var(v);
        }
    }

    fn release_var(&mut self, v: &Var<'ctx>) {
        match v.store {
            Store::Slot(p) => {
                if self.needs_rc(&v.ty) {
                    let val = self.load(&v.ty, p);
                    self.release(val, &v.ty);
                }
            }
            Store::Boxed(slot) => {
                let bx = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                self.call_rt("tsl_release", &[bx]);
            }
            Store::Binding { slot, owned: true } => {
                let t = self.llty(&Ty::View).unwrap();
                let loc = self.b.build_load(t, slot, "").unwrap();
                self.release(loc, &Ty::View);
            }
            _ => {}
        }
    }

    fn self_ptr(&self) -> (PointerValue<'ctx>, Ty) {
        self.stack.last().unwrap().self_ptr.clone().expect("no receiver")
    }

    /// Pointer to a variable's current value, for reading.
    fn var_read_ptr(&mut self, v: &Var<'ctx>, span: Span) -> PointerValue<'ctx> {
        match v.store {
            Store::Slot(p) | Store::EnvValue(p) => p,
            Store::Boxed(slot) => {
                let bx = self.b.build_load(self.ptr_ty(), slot, "").unwrap().into_pointer_value();
                self.box_data(bx)
            }
            Store::EnvBox(p) => {
                let bx = self.b.build_load(self.ptr_ty(), p, "").unwrap().into_pointer_value();
                self.box_data(bx)
            }
            Store::ListElem { slot, idx, .. } => {
                let list = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                let i = self.b.build_load(self.i64(), idx, "").unwrap();
                let l = self.loc(span);
                self.call_rt("tsl_list_get", &[list, i, l.into()]).unwrap().into_pointer_value()
            }
            Store::Field(i) => {
                let (p, owner) = self.self_ptr();
                let Ty::Struct(id) = owner else { unreachable!() };
                let st = self.struct_ty(id);
                self.b.build_struct_gep(st, p, i, "").unwrap()
            }
            Store::SelfRef => self.self_ptr().0,
            Store::State(i) => {
                let r = self.stack.last().unwrap().record.clone().expect("state outside a view");
                self.b.build_struct_gep(r.ty, r.data, i, "").unwrap()
            }
            Store::ListElemVia { locator, idx } => {
                let t = self.llty(&Ty::View).unwrap();
                let loc = self.b.build_load(t, locator, "").unwrap();
                let slot = self.call_locator(loc);
                let list = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                let i = self.b.build_load(self.i64(), idx, "").unwrap();
                let l = self.loc(span);
                self.call_rt("tsl_list_get", &[list, i, l.into()]).unwrap().into_pointer_value()
            }
            Store::Binding { slot, .. } => {
                let t = self.llty(&Ty::View).unwrap();
                let loc = self.b.build_load(t, slot, "").unwrap();
                self.call_locator(loc)
            }
            Store::ViewFn => unreachable!("a view function isn't a value"),
        }
    }

    /// Calls a locator closure `{ fn, env }`, giving a pointer to storage.
    fn call_locator(&mut self, loc: Val<'ctx>) -> PointerValue<'ctx> {
        let s = loc.into_struct_value();
        let f = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
        let env = self.b.build_extract_value(s, 1, "").unwrap();
        let ty = self.ptr_ty().fn_type(&[self.ptr_ty().into()], false);
        self.b.build_indirect_call(ty, f, &[env.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value()
    }

    /// Pointer to a variable's storage, for writing.
    fn var_write_ptr(&mut self, v: &Var<'ctx>, span: Span) -> PointerValue<'ctx> {
        match v.store {
            Store::ListElem { slot, idx, .. } => {
                let i = self.b.build_load(self.i64(), idx, "").unwrap();
                let l = self.loc(span);
                self.call_rt("tsl_list_elem_mut", &[slot.into(), i, l.into()]).unwrap().into_pointer_value()
            }
            Store::ListElemVia { locator, idx } => {
                let t = self.llty(&Ty::View).unwrap();
                let loc = self.b.build_load(t, locator, "").unwrap();
                let slot = self.call_locator(loc);
                let i = self.b.build_load(self.i64(), idx, "").unwrap();
                let l = self.loc(span);
                self.call_rt("tsl_list_elem_mut", &[slot.into(), i, l.into()]).unwrap().into_pointer_value()
            }
            _ => self.var_read_ptr(v, span),
        }
    }

    fn ty(&self, e: &Expr) -> Ty {
        self.checked.types.get(&e.id).cloned().unwrap_or(Ty::Void)
    }

    // ===== places =====

    /// A pointer to the value of `e` for reading, if `e` names storage
    /// (a variable, field or list element). Doesn't copy anything.
    fn read_ptr(&mut self, e: &Expr) -> Option<PointerValue<'ctx>> {
        match &e.kind {
            ExprKind::Name(n) => {
                let v = self.lookup(n)?;
                Some(self.var_read_ptr(&v, e.span))
            }
            ExprKind::Member { object, name, optional: false } => {
                let Ty::Struct(id) = self.ty(object) else { return None };
                let base = self.read_ptr(object)?;
                let idx = self.field_index(id, &name.name);
                let st = self.struct_ty(id);
                Some(self.b.build_struct_gep(st, base, idx, "").unwrap())
            }
            ExprKind::Index(object, index) if matches!(self.ty(object), Ty::List(_)) => {
                let base = self.read_ptr(object)?;
                let list = self.b.build_load(self.ptr_ty(), base, "").unwrap();
                let i = self.expr(index).unwrap();
                let l = self.loc(e.span);
                Some(self.call_rt("tsl_list_get", &[list, i, l.into()]).unwrap().into_pointer_value())
            }
            _ => None,
        }
    }

    /// A pointer to the storage of `e`, for writing (makes lists unique).
    fn write_ptr(&mut self, e: &Expr) -> PointerValue<'ctx> {
        match &e.kind {
            ExprKind::Name(n) => {
                let v = self.lookup(n).expect("assignable name");
                self.var_write_ptr(&v, e.span)
            }
            ExprKind::Member { object, name, .. } => {
                let Ty::Struct(id) = self.ty(object) else { unreachable!() };
                let base = self.write_ptr(object);
                let idx = self.field_index(id, &name.name);
                let st = self.struct_ty(id);
                self.b.build_struct_gep(st, base, idx, "").unwrap()
            }
            ExprKind::Index(object, index) => {
                let slot = self.write_ptr(object);
                let i = self.expr(index).unwrap();
                let l = self.loc(e.span);
                self.call_rt("tsl_list_elem_mut", &[slot.into(), i, l.into()]).unwrap().into_pointer_value()
            }
            _ => unreachable!("checker guarantees assignable targets"),
        }
    }

    /// Is `e` storage that can be changed (so it has a write pointer)?
    fn is_place(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(n) => self.lookup(n).is_some_and(|v| !matches!(v.store, Store::EnvValue(_) | Store::ViewFn)),
            ExprKind::Member { object, optional: false, .. } => {
                matches!(self.ty(object), Ty::Struct(_)) && self.is_place(object)
            }
            ExprKind::Index(object, _) => matches!(self.ty(object), Ty::List(_)) && self.is_place(object),
            _ => false,
        }
    }

    fn field_index(&self, id: usize, name: &str) -> u32 {
        self.checked.structs[id].fields.iter().position(|f| f.name == name).unwrap() as u32
    }

    // ===== statements =====

    fn block(&mut self, b: &Block, want: Option<&Ty>) -> Option<Val<'ctx>> {
        self.push_scope();
        let mut result = None;
        let n = b.stmts.len();
        let want = want.filter(|t| **t != Ty::Void);
        for (i, s) in b.stmts.iter().enumerate() {
            if self.terminated() {
                self.continue_dead();
            }
            match (s, want) {
                (Stmt::Expr(e), Some(t)) if i + 1 == n => result = self.expr_as(e, t),
                _ => self.stmt(s),
            }
        }
        if result.is_none() {
            if let Some(t) = want {
                result = self.undef(t);
            }
        }
        self.pop_scope();
        result
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(d) | Stmt::Var(d) => {
                let ty = self.checked.locals.get(&d.name.span).cloned().unwrap_or_else(|| self.ty(&d.value));
                let v = self.expr_as(&d.value, &ty);
                self.declare_value(&d.name.name, &ty, v);
            }
            Stmt::State(d) => self.unsupported(d.span, "`state`"),
            Stmt::Fn(f) => self.unsupported(f.name.span, "functions inside views"),
            Stmt::Assign { target, op, value, .. } => self.assign(target, *op, value),
            Stmt::Return { value, .. } => {
                let ret = self.stack.last().unwrap().ret.clone();
                let v = value.as_ref().and_then(|v| self.expr_as(v, &ret));
                self.release_all_scopes();
                match v {
                    Some(v) => self.b.build_return(Some(&v)).unwrap(),
                    None => self.b.build_return(None).unwrap(),
                };
                self.continue_dead();
            }
            Stmt::For { var, iter, body, .. } => self.for_loop(var, iter, body),
            Stmt::Expr(e) => {
                let t = self.ty(e);
                if let Some(v) = self.expr(e) {
                    self.release(v, &t);
                }
            }
        }
    }

    fn assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) {
        let tt = self.ty(target);
        // `dict[key] = value` (or `= nil` to remove)
        if let ExprKind::Index(object, key) = &target.kind {
            if let Ty::Dict(kt, vt) = self.ty(object) {
                let v = self.expr_as(value, &tt).unwrap();
                let k = self.expr_as(key, &kt).unwrap();
                let slot = self.write_ptr(object);
                let kp = self.spill(&kt, k);
                let ov = v.into_struct_value();
                let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
                let (set_bb, del_bb, done) = (self.block_named("set"), self.block_named("del"), self.block_named("done"));
                self.b.build_conditional_branch(flag, set_bb, del_bb).unwrap();
                self.b.position_at_end(set_bb);
                let inner = self.b.build_extract_value(ov, 1, "").unwrap();
                let vp = self.spill(&vt, inner);
                self.call_rt("tsl_dict_set", &[slot.into(), kp.into(), vp.into()]);
                self.br(done);
                self.b.position_at_end(del_bb);
                let vlt = self.llty(&vt).unwrap();
                let out = self.alloca(vlt, "removed");
                let removed = self.call_rt("tsl_dict_remove", &[slot.into(), kp.into(), out.into()]);
                let removed = self.truthy(removed);
                let (rel_bb, after) = (self.block_named("rel"), self.block_named("after"));
                self.b.build_conditional_branch(removed, rel_bb, after).unwrap();
                self.b.position_at_end(rel_bb);
                let old = self.load(&vt, out);
                self.release(old, &vt);
                self.br(after);
                self.b.position_at_end(after);
                let key_val = self.load(&kt, kp);
                self.release(key_val, &kt);
                self.br(done);
                self.b.position_at_end(done);
                return;
            }
        }
        let v = self.expr_as(value, &tt).unwrap();
        let p = self.write_ptr(target);
        let old = self.load(&tt, p);
        let new = match op {
            AssignOp::Set => v,
            _ => {
                let bop = match op {
                    AssignOp::Add => BinOp::Add,
                    AssignOp::Sub => BinOp::Sub,
                    AssignOp::Mul => BinOp::Mul,
                    _ => BinOp::Div,
                };
                let r = self.arith(bop, old, v, &tt, value.span);
                self.release(v, &tt);
                r
            }
        };
        self.b.build_store(p, new).unwrap();
        self.release(old, &tt);
    }

    fn for_loop(&mut self, var: &Ident, iter: &Expr, body: &Block) {
        self.for_each_in(var, iter, &mut |g| {
            g.block(body, Some(&Ty::Void));
        });
    }

    /// Is `e` rooted in a view's `state` (so its storage outlives this function)?
    fn in_state(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(n) => self.lookup(n).is_some_and(|v| matches!(v.store, Store::State(_))),
            ExprKind::Member { object, .. } => self.in_state(object),
            _ => false,
        }
    }

    /// A `for` loop whose body is emitted by `body`.
    fn for_each_in(&mut self, var: &Ident, iter: &Expr, body: &mut dyn FnMut(&mut Self)) {
        if let ExprKind::Binary(BinOp::Range, lo, hi) = &iter.kind {
            let lo = self.expr(lo).unwrap().into_int_value();
            let hi = self.expr(hi).unwrap().into_int_value();
            let i = self.alloca(self.i64().into(), "i");
            self.b.build_store(i, lo).unwrap();
            let (cond_bb, body_bb, done) = (self.block_named("for"), self.block_named("body"), self.block_named("done"));
            self.b.build_unconditional_branch(cond_bb).unwrap();
            self.b.position_at_end(cond_bb);
            let cur = self.b.build_load(self.i64(), i, "").unwrap().into_int_value();
            let more = self.b.build_int_compare(IntPredicate::SLT, cur, hi, "").unwrap();
            self.b.build_conditional_branch(more, body_bb, done).unwrap();
            self.b.position_at_end(body_bb);
            self.push_scope();
            self.declare_value(&var.name, &Ty::Int, Some(cur.into()));
            body(self);
            self.pop_scope();
            if !self.terminated() {
                let next = self.b.build_int_add(cur, self.i64().const_int(1, false), "").unwrap();
                self.b.build_store(i, next).unwrap();
            }
            self.br(cond_bb);
            self.b.position_at_end(done);
            return;
        }
        let Ty::List(elem) = self.ty(iter) else { unreachable!() };
        let place = self.checked.loop_places.contains(&var.span) && self.is_place(iter);
        let idx = self.alloca(self.i64().into(), "idx");
        self.b.build_store(idx, self.i64().const_zero()).unwrap();
        // Either iterate the live list in place, or a snapshot of it. The
        // snapshot is a hidden variable, so an early `return` releases it.
        self.push_scope();
        // A closure capturing the loop variable must find the element again
        // later; if the list's storage may move, keep a locator for it.
        let relocate = if place && !self.in_state(iter) {
            let loc = self.make_closure(Body::Locator(iter), &Ty::View);
            let lt = self.llty(&Ty::View).unwrap();
            let slot = self.alloca(lt, "list_locator");
            self.b.build_store(slot, loc).unwrap();
            self.push_var(Var { name: String::new(), ty: Ty::View, store: Store::Binding { slot, owned: true } });
            Some(slot)
        } else {
            None
        };
        let slot = if place {
            self.write_ptr(iter)
        } else {
            let list = self.expr(iter).unwrap();
            let slot = self.alloca(self.ptr_ty().into(), "list");
            self.b.build_store(slot, list).unwrap();
            self.push_var(Var { name: String::new(), ty: Ty::List(elem.clone()), store: Store::Slot(slot) });
            slot
        };
        let (cond_bb, body_bb, done) = (self.block_named("for"), self.block_named("body"), self.block_named("done"));
        self.b.build_unconditional_branch(cond_bb).unwrap();
        self.b.position_at_end(cond_bb);
        let list = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
        let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
        let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, cur, len, "").unwrap();
        self.b.build_conditional_branch(more, body_bb, done).unwrap();
        self.b.position_at_end(body_bb);
        self.push_scope();
        if place {
            let stable = self.in_state(iter);
            self.push_var(Var { name: var.name.clone(), ty: (*elem).clone(), store: Store::ListElem { slot, idx, stable, relocate } });
        } else {
            let l = self.loc(iter.span);
            let p = self.call_rt("tsl_list_get", &[list, cur.into(), l.into()]).unwrap().into_pointer_value();
            let v = self.load(&elem, p);
            self.retain(v, &elem);
            self.declare_value(&var.name, &elem, Some(v));
        }
        body(self);
        self.pop_scope();
        if !self.terminated() {
            let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
            let next = self.b.build_int_add(cur, self.i64().const_int(1, false), "").unwrap();
            self.b.build_store(idx, next).unwrap();
        }
        self.br(cond_bb);
        self.b.position_at_end(done);
        self.pop_scope();
    }

    // ===== expressions =====

    /// Evaluates `e` and converts it to `to` (wrapping in an optional if needed).
    fn expr_as(&mut self, e: &Expr, to: &Ty) -> Option<Val<'ctx>> {
        let from = self.ty(e);
        let v = self.expr(e)?;
        Some(self.coerce(v, &from, to))
    }

    fn coerce(&mut self, v: Val<'ctx>, from: &Ty, to: &Ty) -> Val<'ctx> {
        if from == to {
            return v;
        }
        match to {
            Ty::Opt(inner) if !matches!(from, Ty::Opt(_)) => {
                let inner_v = self.coerce(v, from, inner);
                self.some(to, inner_v)
            }
            _ => v,
        }
    }

    fn some(&mut self, opt_ty: &Ty, v: Val<'ctx>) -> Val<'ctx> {
        let st = self.llty(opt_ty).unwrap().into_struct_type();
        let s = st.get_undef();
        let s = self.b.build_insert_value(s, self.ctx.bool_type().const_int(1, false), 0, "").unwrap();
        self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value().into()
    }

    fn none(&mut self, opt_ty: &Ty) -> Val<'ctx> {
        self.llty(opt_ty).unwrap().const_zero()
    }

    /// Evaluates `e` to an owned value (`None` if it produces no value).
    fn expr(&mut self, e: &Expr) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        match &e.kind {
            ExprKind::Int(n) => Some(if t == Ty::Float {
                self.ctx.f64_type().const_float(*n as f64).into()
            } else {
                self.i64().const_int(*n as u64, true).into()
            }),
            ExprKind::Float(x) => Some(self.ctx.f64_type().const_float(*x).into()),
            ExprKind::Bool(b) => Some(self.ctx.bool_type().const_int(*b as u64, false).into()),
            ExprKind::Nil => Some(self.none(&t)),
            ExprKind::Str(parts) => Some(self.interpolate(parts)),
            ExprKind::Name(n) => self.name(n, &t, e.span),
            ExprKind::DotName(_) if t == Ty::Float => Some(self.ctx.f64_type().const_float(f64::INFINITY).into()),
            ExprKind::DotName(n) => {
                let Ty::Enum(id) = t else { unreachable!() };
                Some(self.enum_value(id, n))
            }
            ExprKind::List(items) => {
                let Ty::List(elem) = &t else { unreachable!() };
                let info = self.type_info(elem);
                let n = self.i64().const_int(items.len() as u64, false);
                let list = self.call_rt("tsl_list_new", &[info.into(), n.into()]).unwrap();
                let slot = self.spill(&t, list);
                for item in items {
                    let v = self.expr_as(item, elem).unwrap();
                    let p = self.spill(elem, v);
                    self.call_rt("tsl_list_push", &[slot.into(), p.into()]);
                }
                Some(self.load(&t, slot))
            }
            ExprKind::Dict(pairs) => {
                let Ty::Dict(kt, vt) = &t else { unreachable!() };
                let (ki, vi) = (self.type_info(kt), self.type_info(vt));
                let d = self.call_rt("tsl_dict_new", &[ki.into(), vi.into()]).unwrap();
                let slot = self.spill(&t, d);
                for (k, v) in pairs {
                    let kv = self.expr_as(k, kt).unwrap();
                    let vv = self.expr_as(v, vt).unwrap();
                    let (kp, vp) = (self.spill(kt, kv), self.spill(vt, vv));
                    self.call_rt("tsl_dict_set", &[slot.into(), kp.into(), vp.into()]);
                }
                Some(self.load(&t, slot))
            }
            ExprKind::Unary(UnaryOp::Not, x) => {
                let v = self.expr(x).unwrap().into_int_value();
                Some(self.b.build_not(v, "").unwrap().into())
            }
            ExprKind::Unary(UnaryOp::Neg, x) => {
                let v = self.expr(x).unwrap();
                Some(if t == Ty::Float {
                    self.b.build_float_neg(v.into_float_value(), "").unwrap().into()
                } else {
                    let zero = self.i64().const_zero().into();
                    self.arith(BinOp::Sub, zero, v, &Ty::Int, e.span)
                })
            }
            ExprKind::Binary(op, l, r) => self.binary(*op, l, r, &t, e.span),
            ExprKind::Call { callee, args, trailing } => self.call(e, callee, args, trailing.as_deref()),
            ExprKind::Member { object, name, optional } => self.member(e, object, name, *optional),
            ExprKind::Index(object, index) => self.index(e, object, index),
            ExprKind::Closure { params, body } => Some(self.closure(params, body, &t)),
            ExprKind::If { cond, then, otherwise } => self.if_expr(cond, then, otherwise.as_ref(), &t),
            ExprKind::Match { subject, arms } => self.match_expr(subject, arms, &t),
            ExprKind::Block(b) => self.block(b, Some(&t)),
            ExprKind::Error => None,
        }
    }

    fn name(&mut self, n: &str, t: &Ty, span: Span) -> Option<Val<'ctx>> {
        if let Some(v) = self.lookup(n) {
            let p = self.var_read_ptr(&v, span);
            let val = self.load(&v.ty, p);
            self.retain(val, &v.ty);
            return Some(val);
        }
        // A top-level function used as a value.
        let f = *self.fns.get(n).expect("resolved name");
        let st = self.llty(t).unwrap().into_struct_type();
        let s = st.const_named_struct(&[f.as_global_value().as_pointer_value().into(), self.ptr_ty().const_null().into()]);
        Some(s.into())
    }

    fn enum_value(&mut self, id: usize, case: &str) -> Val<'ctx> {
        let idx = self.checked.enums[id].cases.iter().position(|c| c.name == case).unwrap();
        let tag = self.ctx.i32_type().const_int(idx as u64, false);
        if self.has_payload(id) {
            let st = self.llty(&Ty::Enum(id)).unwrap().into_struct_type();
            st.const_named_struct(&[tag.into(), self.ptr_ty().const_null().into()]).into()
        } else {
            tag.into()
        }
    }

    /// Converts a value to an owned `String` (for interpolation and `print`).
    fn stringify(&mut self, v: Val<'ctx>, t: &Ty) -> Val<'ctx> {
        match t {
            Ty::Str => v,
            Ty::Int => self.call_rt("tsl_str_from_int", &[v]).unwrap(),
            Ty::Float => self.call_rt("tsl_str_from_float", &[v]).unwrap(),
            Ty::Bool => {
                let b = self.to_c_bool(v.into_int_value());
                self.call_rt("tsl_str_from_bool", &[b]).unwrap()
            }
            Ty::Enum(id) => {
                let tag = if self.has_payload(*id) {
                    let tag = self.b.build_extract_value(v.into_struct_value(), 0, "").unwrap();
                    self.release(v, t);
                    tag.into_int_value()
                } else {
                    v.into_int_value()
                };
                let names: Vec<String> = self.checked.enums[*id].cases.iter().map(|c| c.name.clone()).collect();
                let slot = self.alloca(self.ptr_ty().into(), "name");
                let done = self.block_named("named");
                let cases: Vec<(IntValue, BasicBlock)> = names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        let bb = self.block_named("case");
                        let saved = self.b.get_insert_block().unwrap();
                        self.b.position_at_end(bb);
                        let s = self.str_lit(name);
                        self.b.build_store(slot, s).unwrap();
                        self.b.build_unconditional_branch(done).unwrap();
                        self.b.position_at_end(saved);
                        (self.ctx.i32_type().const_int(i as u64, false), bb)
                    })
                    .collect();
                self.b.build_switch(tag, done, &cases).unwrap();
                self.b.position_at_end(done);
                self.b.build_load(self.ptr_ty(), slot, "").unwrap()
            }
            _ => unreachable!("not showable"),
        }
    }

    fn interpolate(&mut self, parts: &[StrPart]) -> Val<'ctx> {
        if let [StrPart::Text(t)] = parts {
            return self.str_lit(t).into();
        }
        let n = parts.len() as u32;
        let arr_ty = self.ptr_ty().array_type(n);
        let arr = self.alloca(arr_ty.into(), "parts");
        let mut owned = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let s = match part {
                StrPart::Text(t) => self.str_lit(t).into(),
                StrPart::Expr(x) => {
                    let t = self.ty(x);
                    let v = self.expr(x).unwrap();
                    let s = self.stringify(v, &t);
                    owned.push(s);
                    s
                }
            };
            let slot = unsafe {
                self.b
                    .build_gep(arr_ty, arr, &[self.i64().const_zero(), self.i64().const_int(i as u64, false)], "")
                    .unwrap()
            };
            self.b.build_store(slot, s).unwrap();
        }
        let count = self.i64().const_int(n as u64, false);
        let r = self.call_rt("tsl_str_concat", &[arr.into(), count.into()]).unwrap();
        for s in owned {
            self.release(s, &Ty::Str);
        }
        r
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr, t: &Ty, span: Span) -> Option<Val<'ctx>> {
        match op {
            BinOp::And | BinOp::Or => {
                let lv = self.expr(l).unwrap().into_int_value();
                let start = self.b.get_insert_block().unwrap();
                let (rhs_bb, done) = (self.block_named("rhs"), self.block_named("logic"));
                if op == BinOp::And {
                    self.b.build_conditional_branch(lv, rhs_bb, done).unwrap();
                } else {
                    self.b.build_conditional_branch(lv, done, rhs_bb).unwrap();
                }
                self.b.position_at_end(rhs_bb);
                let rv = self.expr(r).unwrap().into_int_value();
                let rhs_end = self.b.get_insert_block().unwrap();
                self.b.build_unconditional_branch(done).unwrap();
                self.b.position_at_end(done);
                let phi = self.b.build_phi(self.ctx.bool_type(), "").unwrap();
                phi.add_incoming(&[(&lv, start), (&rv, rhs_end)]);
                Some(phi.as_basic_value())
            }
            BinOp::Eq | BinOp::Ne => {
                let (lt, rt) = (self.ty(l), self.ty(r));
                // Compare as the optional type if only one side is optional.
                let common = if matches!(lt, Ty::Opt(_)) { lt.clone() } else { rt.clone() };
                let lv = self.expr_as(l, &common).unwrap();
                let rv = self.expr_as(r, &common).unwrap();
                let mut res = self.eq(lv, rv, &common);
                if op == BinOp::Ne {
                    res = self.b.build_not(res, "").unwrap();
                }
                self.release(lv, &common);
                self.release(rv, &common);
                Some(res.into())
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ot = self.ty(l);
                let lv = self.expr(l).unwrap();
                let rv = self.expr(r).unwrap();
                let res = self.compare(op, lv, rv, &ot);
                self.release(lv, &ot);
                self.release(rv, &ot);
                Some(res.into())
            }
            BinOp::Coalesce => {
                let lt = self.ty(l);
                let lv = self.expr(l).unwrap().into_struct_value();
                let flag = self.b.build_extract_value(lv, 0, "").unwrap().into_int_value();
                let lt_ = self.llty(t).unwrap();
                let slot = self.alloca(lt_, "coalesce");
                let (some_bb, none_bb, done) = (self.block_named("some"), self.block_named("none"), self.block_named("done"));
                self.b.build_conditional_branch(flag, some_bb, none_bb).unwrap();
                self.b.position_at_end(some_bb);
                let Ty::Opt(inner) = &lt else { unreachable!() };
                let inner_v = self.b.build_extract_value(lv, 1, "").unwrap();
                let v = self.coerce(inner_v, inner, t);
                self.b.build_store(slot, v).unwrap();
                self.br(done);
                self.b.position_at_end(none_bb);
                if let Some(rv) = self.expr_as(r, t) {
                    self.b.build_store(slot, rv).unwrap();
                }
                self.br(done);
                self.b.position_at_end(done);
                Some(self.load(t, slot))
            }
            BinOp::Range => {
                self.unsupported(span, "ranges outside `for` loops");
                self.undef(t)
            }
            _ => {
                let lv = self.expr_as(l, t).unwrap();
                let rv = self.expr_as(r, t).unwrap();
                let res = self.arith(op, lv, rv, t, span);
                self.release(lv, t);
                self.release(rv, t);
                Some(res)
            }
        }
    }

    /// Arithmetic on borrowed operands; the result is owned.
    fn arith(&mut self, op: BinOp, l: Val<'ctx>, r: Val<'ctx>, t: &Ty, span: Span) -> Val<'ctx> {
        match t {
            Ty::Float => {
                let (a, b) = (l.into_float_value(), r.into_float_value());
                match op {
                    BinOp::Add => self.b.build_float_add(a, b, "").unwrap(),
                    BinOp::Sub => self.b.build_float_sub(a, b, "").unwrap(),
                    BinOp::Mul => self.b.build_float_mul(a, b, "").unwrap(),
                    BinOp::Div => self.b.build_float_div(a, b, "").unwrap(),
                    _ => self.b.build_float_rem(a, b, "").unwrap(),
                }
                .into()
            }
            Ty::Str => {
                let arr_ty = self.ptr_ty().array_type(2);
                let arr = self.alloca(arr_ty.into(), "pair");
                for (i, v) in [l, r].into_iter().enumerate() {
                    let p = unsafe {
                        self.b
                            .build_gep(arr_ty, arr, &[self.i64().const_zero(), self.i64().const_int(i as u64, false)], "")
                            .unwrap()
                    };
                    self.b.build_store(p, v).unwrap();
                }
                let two = self.i64().const_int(2, false);
                self.call_rt("tsl_str_concat", &[arr.into(), two.into()]).unwrap()
            }
            Ty::List(_) => self.call_rt("tsl_list_concat", &[l, r]).unwrap(),
            _ => {
                let (a, b) = (l.into_int_value(), r.into_int_value());
                match op {
                    BinOp::Div | BinOp::Rem => {
                        let zero = self.i64().const_zero();
                        let is_zero = self.b.build_int_compare(IntPredicate::EQ, b, zero, "").unwrap();
                        self.panic_if(is_zero, "division by zero", span);
                        let min = self.i64().const_int(i64::MIN as u64, true);
                        let neg1 = self.i64().const_int(u64::MAX, true);
                        let a_min = self.b.build_int_compare(IntPredicate::EQ, a, min, "").unwrap();
                        let b_neg = self.b.build_int_compare(IntPredicate::EQ, b, neg1, "").unwrap();
                        let over = self.b.build_and(a_min, b_neg, "").unwrap();
                        self.panic_if(over, "integer overflow", span);
                        if op == BinOp::Div {
                            self.b.build_int_signed_div(a, b, "").unwrap().into()
                        } else {
                            self.b.build_int_signed_rem(a, b, "").unwrap().into()
                        }
                    }
                    _ => {
                        let name = match op {
                            BinOp::Add => "llvm.sadd.with.overflow",
                            BinOp::Sub => "llvm.ssub.with.overflow",
                            _ => "llvm.smul.with.overflow",
                        };
                        let intr = Intrinsic::find(name).unwrap();
                        let f = intr.get_declaration(&self.module, &[self.i64().into()]).unwrap();
                        let r = self.b.build_call(f, &[a.into(), b.into()], "").unwrap().try_as_basic_value().basic().unwrap();
                        let r = r.into_struct_value();
                        let over = self.b.build_extract_value(r, 1, "").unwrap().into_int_value();
                        self.panic_if(over, "integer overflow", span);
                        self.b.build_extract_value(r, 0, "").unwrap()
                    }
                }
            }
        }
    }

    fn member(&mut self, e: &Expr, object: &Expr, name: &Ident, optional: bool) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        // `Filter.all`
        if let ExprKind::Name(tn) = &object.kind {
            if self.lookup(tn).is_none() && !self.fns.contains_key(tn) {
                if let Ty::Enum(id) = t {
                    return Some(self.enum_value(id, &name.name));
                }
            }
        }
        if optional || (object.in_optional_chain() && matches!(self.ty(object), Ty::Opt(_))) {
            let ot = self.ty(object);
            let Ty::Opt(inner) = ot.clone() else { unreachable!() };
            let ov = self.expr(object).unwrap();
            return Some(self.optional_chain(ov, &ot, &t, |g, v| g.property(v, &inner, &name.name, e.span)));
        }
        // Read a field directly from storage when possible.
        if let Some(p) = self.read_ptr(e) {
            let v = self.load(&t, p);
            self.retain(v, &t);
            return Some(v);
        }
        let ot = self.ty(object);
        let v = self.expr(object).unwrap();
        let r = self.property(v, &ot, &name.name, e.span);
        self.release(v, &ot);
        Some(r)
    }

    /// `v.name` where `v` (borrowed) has type `t`. Returns an owned value.
    fn property(&mut self, v: Val<'ctx>, t: &Ty, name: &str, span: Span) -> Val<'ctx> {
        match t {
            Ty::Struct(id) => {
                let idx = self.field_index(*id, name);
                let ft = self.checked.structs[*id].fields[idx as usize].ty.clone();
                let f = self.b.build_extract_value(v.into_struct_value(), idx, "").unwrap();
                self.retain(f, &ft);
                f
            }
            Ty::Str => match name {
                "count" => self.call_rt("tsl_str_count", &[v]).unwrap(),
                _ => {
                    let n = self.call_rt("tsl_str_count", &[v]).unwrap().into_int_value();
                    self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_zero(), "").unwrap().into()
                }
            },
            Ty::List(elem) => {
                let len = self.call_rt("tsl_list_len", &[v]).unwrap().into_int_value();
                match name {
                    "count" => len.into(),
                    "isEmpty" => self.b.build_int_compare(IntPredicate::EQ, len, self.i64().const_zero(), "").unwrap().into(),
                    _ => {
                        let opt = Ty::opt((**elem).clone());
                        let empty = self.b.build_int_compare(IntPredicate::EQ, len, self.i64().const_zero(), "").unwrap();
                        let lt = self.llty(&opt).unwrap();
                        let slot = self.alloca(lt, "item");
                        let none = self.none(&opt);
                        self.b.build_store(slot, none).unwrap();
                        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
                        self.b.build_conditional_branch(empty, done, some_bb).unwrap();
                        self.b.position_at_end(some_bb);
                        let i = if name == "first" {
                            self.i64().const_zero()
                        } else {
                            self.b.build_int_sub(len, self.i64().const_int(1, false), "").unwrap()
                        };
                        let l = self.loc(span);
                        let p = self.call_rt("tsl_list_get", &[v, i.into(), l.into()]).unwrap().into_pointer_value();
                        let item = self.load(elem, p);
                        self.retain(item, elem);
                        let s = self.some(&opt, item);
                        self.b.build_store(slot, s).unwrap();
                        self.br(done);
                        self.b.position_at_end(done);
                        self.load(&opt, slot)
                    }
                }
            }
            Ty::Dict(..) => match name {
                "count" => self.call_rt("tsl_dict_len", &[v]).unwrap(),
                "isEmpty" => {
                    let n = self.call_rt("tsl_dict_len", &[v]).unwrap().into_int_value();
                    self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_zero(), "").unwrap().into()
                }
                "keys" => self.call_rt("tsl_dict_keys", &[v]).unwrap(),
                _ => self.call_rt("tsl_dict_values", &[v]).unwrap(),
            },
            _ => unreachable!("no property {name}"),
        }
    }

    /// `opt?.something`: applies `f` to the inner value (borrowed) when present.
    fn optional_chain(
        &mut self,
        ov: Val<'ctx>,
        opt_ty: &Ty,
        result: &Ty,
        f: impl FnOnce(&mut Self, Val<'ctx>) -> Val<'ctx>,
    ) -> Val<'ctx> {
        let Ty::Opt(inner) = opt_ty else { unreachable!() };
        let lt = self.llty(result).unwrap();
        let slot = self.alloca(lt, "chain");
        let none = self.none(result);
        self.b.build_store(slot, none).unwrap();
        let flag = self.b.build_extract_value(ov.into_struct_value(), 0, "").unwrap().into_int_value();
        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
        self.b.build_conditional_branch(flag, some_bb, done).unwrap();
        self.b.position_at_end(some_bb);
        let iv = self.b.build_extract_value(ov.into_struct_value(), 1, "").unwrap();
        let r = f(self, iv);
        // `f` gives either the inner value (wrap it) or an optional already.
        let r = if Some(r.get_type()) == self.llty(result) { r } else { self.some(result, r) };
        self.b.build_store(slot, r).unwrap();
        self.release(iv, inner);
        self.br(done);
        self.b.position_at_end(done);
        self.load(result, slot)
    }

    fn index(&mut self, e: &Expr, object: &Expr, index: &Expr) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        match self.ty(object) {
            Ty::Dict(kt, vt) => {
                let d = self.expr(object).unwrap();
                let k = self.expr_as(index, &kt).unwrap();
                let kp = self.spill(&kt, k);
                let vlt = self.llty(&vt).unwrap();
                let out = self.alloca(vlt, "value");
                self.b.build_store(out, vlt.const_zero()).unwrap();
                let found = self.call_rt("tsl_dict_get", &[d, kp.into(), out.into()]);
                let found = self.truthy(found);
                let v = self.load(&vt, out);
                let st = self.llty(&t).unwrap().into_struct_type();
                let s = self.b.build_insert_value(st.get_undef(), found, 0, "").unwrap();
                let s = self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value();
                self.release(k, &kt);
                self.release(d, &Ty::Dict(kt, vt));
                Some(s.into())
            }
            lt @ Ty::List(_) => {
                if let Some(p) = self.read_ptr(e) {
                    let v = self.load(&t, p);
                    self.retain(v, &t);
                    return Some(v);
                }
                let list = self.expr(object).unwrap();
                let i = self.expr(index).unwrap();
                let l = self.loc(e.span);
                let p = self.call_rt("tsl_list_get", &[list, i, l.into()]).unwrap().into_pointer_value();
                let v = self.load(&t, p);
                self.retain(v, &t);
                self.release(list, &lt);
                Some(v)
            }
            _ => unreachable!(),
        }
    }

    fn if_expr(&mut self, cond: &Cond, then: &Block, otherwise: Option<&Else>, t: &Ty) -> Option<Val<'ctx>> {
        let slot = self.llty(t).map(|lt| self.alloca(lt, "if"));
        let (then_bb, else_bb, done) = (self.block_named("then"), self.block_named("else"), self.block_named("endif"));
        self.push_scope();
        match cond {
            Cond::Expr(c) => {
                let c = self.expr(c).unwrap().into_int_value();
                self.b.build_conditional_branch(c, then_bb, else_bb).unwrap();
                self.b.position_at_end(then_bb);
            }
            Cond::Let(name, value) => {
                let vt = self.ty(value);
                let Ty::Opt(inner) = vt else { unreachable!() };
                let ov = self.expr(value).unwrap().into_struct_value();
                let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
                self.b.build_conditional_branch(flag, then_bb, else_bb).unwrap();
                self.b.position_at_end(then_bb);
                let inner_v = self.b.build_extract_value(ov, 1, "").unwrap();
                let ty = self.checked.locals.get(&name.span).cloned().unwrap_or(*inner);
                self.declare_value(&name.name, &ty, Some(inner_v));
            }
        }
        let v = self.block(then, Some(t));
        if let (Some(slot), Some(v)) = (slot, v) {
            if !self.terminated() {
                self.b.build_store(slot, v).unwrap();
            }
        }
        self.pop_scope();
        self.br(done);
        self.b.position_at_end(else_bb);
        let v = match otherwise {
            Some(Else::Block(b)) => self.block(b, Some(t)),
            Some(Else::If(e)) => self.expr_as(e, t),
            None => None,
        };
        if let (Some(slot), Some(v)) = (slot, v) {
            if !self.terminated() {
                self.b.build_store(slot, v).unwrap();
            }
        }
        self.br(done);
        self.b.position_at_end(done);
        slot.map(|s| self.load(t, s))
    }

    fn match_expr(&mut self, subject: &Expr, arms: &[Arm], t: &Ty) -> Option<Val<'ctx>> {
        self.match_with(subject, arms, t, false)
    }

    /// A `match`; with `content`, its arms emit views instead of producing a value.
    fn match_with(&mut self, subject: &Expr, arms: &[Arm], t: &Ty, content: bool) -> Option<Val<'ctx>> {
        let st = self.ty(subject);
        let sv = self.expr(subject).unwrap();
        // The subject is a hidden variable, so a `return` in an arm releases it.
        self.push_scope();
        let subject_slot = self.spill(&st, sv);
        self.push_var(Var { name: String::new(), ty: st.clone(), store: Store::Slot(subject_slot) });
        let slot = self.llty(t).map(|lt| self.alloca(lt, "match"));
        let done = self.block_named("endmatch");
        let (tag, payload) = match &st {
            Ty::Enum(id) if self.has_payload(*id) => {
                let s = sv.into_struct_value();
                (
                    Some(self.b.build_extract_value(s, 0, "").unwrap().into_int_value()),
                    Some(self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value()),
                )
            }
            Ty::Enum(_) => (Some(sv.into_int_value()), None),
            _ => (None, None),
        };
        for arm in arms {
            let (body_bb, next_bb) = (self.block_named("arm"), self.block_named("next"));
            let test = match &arm.pattern {
                Pattern::Case { name, .. } => {
                    let Ty::Enum(id) = st else { unreachable!() };
                    let idx = self.checked.enums[id].cases.iter().position(|c| c.name == name.name).unwrap();
                    let want = self.ctx.i32_type().const_int(idx as u64, false);
                    self.b.build_int_compare(IntPredicate::EQ, tag.unwrap(), want, "").unwrap()
                }
                Pattern::Wildcard(_) | Pattern::Error(_) => self.ctx.bool_type().const_int(1, false),
                Pattern::Literal(lit) => {
                    let lv = self.expr_as(lit, &st).unwrap();
                    let r = self.eq(sv, lv, &st);
                    self.release(lv, &st);
                    r
                }
            };
            self.b.build_conditional_branch(test, body_bb, next_bb).unwrap();
            self.b.position_at_end(body_bb);
            self.push_scope();
            if let Pattern::Case { name, bindings, .. } = &arm.pattern {
                let Ty::Enum(id) = st else { unreachable!() };
                let idx = self.checked.enums[id].cases.iter().position(|c| c.name == name.name).unwrap();
                if !bindings.is_empty() {
                    let pt = self.payload_ty(id, idx);
                    let data = self.box_data(payload.unwrap());
                    let fields: Vec<Ty> = self.checked.enums[id].cases[idx].fields.iter().map(|f| f.ty.clone()).collect();
                    for (i, (b, ft)) in bindings.iter().zip(&fields).enumerate() {
                        if b.name == "_" {
                            continue;
                        }
                        let fp = self.b.build_struct_gep(pt, data, i as u32, "").unwrap();
                        let v = self.load(ft, fp);
                        self.retain(v, ft);
                        self.declare_value(&b.name, ft, Some(v));
                    }
                }
            }
            let v = match &arm.body.kind {
                ExprKind::Block(b) if content => {
                    self.content_block(b, false);
                    None
                }
                _ if content => {
                    self.content_expr(&arm.body);
                    None
                }
                ExprKind::Block(b) => self.block(b, Some(t)),
                _ if *t == Ty::Void => {
                    let bt = self.ty(&arm.body);
                    if let Some(v) = self.expr(&arm.body) {
                        self.release(v, &bt);
                    }
                    None
                }
                _ => self.expr_as(&arm.body, t),
            };
            if let (Some(slot), Some(v)) = (slot, v) {
                if !self.terminated() {
                    self.b.build_store(slot, v).unwrap();
                }
            }
            self.pop_scope();
            self.br(done);
            self.b.position_at_end(next_bb);
        }
        // Checked matches are exhaustive.
        self.b.build_unreachable().unwrap();
        self.b.position_at_end(done);
        self.pop_scope();
        slot.map(|s| self.load(t, s))
    }

    // ===== closures =====

    fn closure(&mut self, params: &[Ident], body: &Block, t: &Ty) -> Val<'ctx> {
        self.make_closure(Body::Fn(params, body), t)
    }

    /// Builds a closure value `{ fn, env }`. Captured variables: values are
    /// copied; boxed `var`s share their box; bindings share their locator;
    /// fields and `self` are reached through the receiver pointer; view
    /// `state` through the view's state record (whose box is kept alive).
    fn make_closure(&mut self, body: Body, t: &Ty) -> Val<'ctx> {
        let (fn_ty, ptys, ret) = match (&body, t) {
            (Body::Fn(..), Ty::Fn(ptys, ret)) => (self.fn_type(ptys, ret, false), ptys.clone(), (**ret).clone()),
            (Body::Locator(_), _) => (self.ptr_ty().fn_type(&[self.ptr_ty().into()], false), vec![], Ty::Void),
            _ => (self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false), vec![], Ty::Void),
        };
        self.closures += 1;
        let f = self.module.add_function(&format!("tsl_c_{}", self.closures), fn_ty, Some(Linkage::Internal));

        let mut free = BTreeSet::new();
        match &body {
            Body::Fn(params, b) => {
                let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                free_block(b, &mut bound, &mut free);
            }
            Body::Content(b) => free_block(b, &mut Vec::new(), &mut free),
            Body::ContentExpr(e) | Body::Locator(e) => free_expr(e, &mut Vec::new(), &mut free),
        }
        let mut captures: Vec<(Var<'ctx>, Capture)> = Vec::new();
        let mut needs_self = false;
        let mut needs_record = false;
        for name in &free {
            let Some(v) = self.lookup(name) else { continue };
            let kind = match v.store {
                Store::Field(_) | Store::SelfRef => {
                    needs_self = true;
                    continue;
                }
                Store::State(_) | Store::ViewFn => {
                    needs_record = true;
                    continue;
                }
                Store::Boxed(_) | Store::EnvBox(_) => Capture::Box,
                Store::Binding { .. } => Capture::Binding,
                Store::ListElem { stable: true, .. } => Capture::Elem,
                Store::ListElem { relocate: Some(_), .. } | Store::ListElemVia { .. } => Capture::ElemVia,
                _ => Capture::Value,
            };
            captures.push((v, kind));
        }
        let mut env_fields: Vec<BasicTypeEnum> = Vec::new();
        for (v, kind) in &captures {
            match kind {
                Capture::Value => env_fields.push(self.llty(&v.ty).unwrap()),
                Capture::Box => env_fields.push(self.ptr_ty().into()),
                Capture::Binding => env_fields.push(self.llty(&Ty::View).unwrap()),
                Capture::Elem => {
                    env_fields.push(self.ptr_ty().into());
                    env_fields.push(self.i64().into());
                }
                Capture::ElemVia => {
                    env_fields.push(self.llty(&Ty::View).unwrap());
                    env_fields.push(self.i64().into());
                }
            }
        }
        let self_info = if needs_self { self.stack.last().unwrap().self_ptr.clone() } else { None };
        let self_field = env_fields.len() as u32;
        if self_info.is_some() {
            env_fields.push(self.ptr_ty().into());
        }
        let record = if needs_record { self.stack.last().unwrap().record.clone() } else { None };
        let record_field = env_fields.len() as u32;
        if record.is_some() {
            env_fields.push(self.ptr_ty().into());
        }
        let env_ty = self.ctx.struct_type(&env_fields, false);

        // Fill the environment.
        let env = if env_fields.is_empty() {
            self.ptr_ty().const_null()
        } else {
            let info = self.env_info(env_ty, &captures, record.is_some().then_some(record_field));
            let bx = self.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
            let data = self.box_data(bx);
            let mut field = 0;
            for (v, kind) in &captures {
                let fp = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                field += 1;
                match (kind, v.store) {
                    (Capture::Box, Store::Boxed(slot) | Store::EnvBox(slot)) => {
                        let bxp = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                        self.call_rt("tsl_retain", &[bxp]);
                        self.b.build_store(fp, bxp).unwrap();
                    }
                    (Capture::Binding, Store::Binding { slot, .. }) => {
                        let lt = self.llty(&Ty::View).unwrap();
                        let loc = self.b.build_load(lt, slot, "").unwrap();
                        self.retain(loc, &Ty::View);
                        self.b.build_store(fp, loc).unwrap();
                    }
                    (Capture::Elem, Store::ListElem { slot, idx, .. }) => {
                        self.b.build_store(fp, slot).unwrap();
                        let ip = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                        field += 1;
                        let i = self.b.build_load(self.i64(), idx, "").unwrap();
                        self.b.build_store(ip, i).unwrap();
                    }
                    (Capture::ElemVia, Store::ListElem { relocate: Some(locator), idx, .. } | Store::ListElemVia { locator, idx }) => {
                        let lt = self.llty(&Ty::View).unwrap();
                        let loc = self.b.build_load(lt, locator, "").unwrap();
                        self.retain(loc, &Ty::View);
                        self.b.build_store(fp, loc).unwrap();
                        let ip = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                        field += 1;
                        let i = self.b.build_load(self.i64(), idx, "").unwrap();
                        self.b.build_store(ip, i).unwrap();
                    }
                    _ => {
                        let p = self.var_read_ptr(v, Span::default());
                        let val = self.load(&v.ty, p);
                        self.retain(val, &v.ty);
                        self.b.build_store(fp, val).unwrap();
                    }
                }
            }
            if let Some((sp, _)) = &self_info {
                let fp = self.b.build_struct_gep(env_ty, data, self_field, "").unwrap();
                self.b.build_store(fp, *sp).unwrap();
            }
            if let Some(r) = &record {
                let fp = self.b.build_struct_gep(env_ty, data, record_field, "").unwrap();
                let rb = self.record_box(r.data);
                self.call_rt("tsl_retain", &[rb.into()]);
                self.b.build_store(fp, rb).unwrap();
            }
            bx
        };

        // The closure's own body.
        let saved = self.b.get_insert_block();
        let entry = self.ctx.append_basic_block(f, "entry");
        self.b.position_at_end(entry);
        let env_param = f.get_nth_param(0).unwrap().into_pointer_value();
        let mut scope = Vec::new();
        let mut self_ptr = None;
        let mut inner_record = None;
        if !env_fields.is_empty() {
            let data = self.box_data(env_param);
            let mut field = 0;
            for (v, kind) in &captures {
                let fp = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                field += 1;
                let store = match kind {
                    Capture::Value => Store::EnvValue(fp),
                    Capture::Box => Store::EnvBox(fp),
                    Capture::Binding => Store::Binding { slot: fp, owned: false },
                    Capture::Elem => {
                        let slot = self.b.build_load(self.ptr_ty(), fp, "slot").unwrap().into_pointer_value();
                        let idx = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                        field += 1;
                        Store::ListElem { slot, idx, stable: true, relocate: None }
                    }
                    Capture::ElemVia => {
                        let idx = self.b.build_struct_gep(env_ty, data, field, "").unwrap();
                        field += 1;
                        Store::ListElemVia { locator: fp, idx }
                    }
                };
                scope.push(Var { name: v.name.clone(), ty: v.ty.clone(), store });
            }
            if let Some((_, owner)) = &self_info {
                let fp = self.b.build_struct_gep(env_ty, data, self_field, "").unwrap();
                let sp = self.b.build_load(self.ptr_ty(), fp, "self").unwrap().into_pointer_value();
                self_ptr = Some((sp, owner.clone()));
                let mut vars = self.receiver_vars(owner);
                vars.append(&mut scope);
                scope = vars;
            }
            if let Some(r) = &record {
                let fp = self.b.build_struct_gep(env_ty, data, record_field, "").unwrap();
                let rb = self.b.build_load(self.ptr_ty(), fp, "record").unwrap().into_pointer_value();
                let data = self.box_data(rb);
                inner_record = Some(Record { data, ty: r.ty, view: r.view.clone() });
                let mut vars = self.record_vars();
                vars.append(&mut scope);
                scope = vars;
            }
        }
        let boxed = match &body {
            Body::Fn(_, b) | Body::Content(b) => boxed_vars(b),
            _ => HashSet::new(),
        };
        self.stack.push(FnCtx { func: f, scopes: vec![scope], ret: ret.clone(), self_ptr, boxed, record: inner_record });
        match body {
            Body::Fn(params, b) => {
                for (i, (p, pt)) in params.iter().zip(&ptys).enumerate() {
                    let v = f.get_nth_param(1 + i as u32).unwrap();
                    self.declare_value(&p.name, pt, Some(v));
                }
                let v = self.block(b, Some(&ret));
                self.finish_function(v);
            }
            Body::Content(b) => {
                self.content_block(b, false);
                self.finish_function(None);
            }
            Body::ContentExpr(e) => {
                self.content_expr(e);
                self.finish_function(None);
            }
            Body::Locator(e) => {
                let p = self.write_ptr(e);
                self.b.build_return(Some(&p)).unwrap();
            }
        }
        self.stack.pop();
        if let Some(bb) = saved {
            self.b.position_at_end(bb);
        }

        let st = self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false);
        let s = self.b.build_insert_value(st.get_undef(), f.as_global_value().as_pointer_value(), 0, "").unwrap();
        self.b.build_insert_value(s, env, 1, "").unwrap().into_struct_value().into()
    }

    /// The box holding a view's state record.
    fn record_box(&mut self, data: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let back = self.i64().const_int(BOX_DATA, false);
        let neg = self.b.build_int_neg(back, "").unwrap();
        unsafe { self.b.build_gep(self.i8(), data, &[neg], "record").unwrap() }
    }

    /// Variables for the current view's `state` values and functions.
    fn record_vars(&self) -> Vec<Var<'ctx>> {
        self.stack
            .last()
            .map(|c| {
                c.scopes
                    .iter()
                    .flatten()
                    .filter(|v| matches!(v.store, Store::State(_) | Store::ViewFn))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Type info for a closure environment: releases the captures when freed.
    fn env_info(
        &mut self,
        env_ty: StructType<'ctx>,
        captures: &[(Var<'ctx>, Capture)],
        record_field: Option<u32>,
    ) -> PointerValue<'ctx> {
        let n = self.closures;
        let f = self.module.add_function(
            &format!("tsl_h_env_{n}"),
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        let caps: Vec<(Ty, Capture)> = captures.iter().map(|(v, k)| (v.ty.clone(), *k)).collect();
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            let mut field = 0;
            for (ty, kind) in &caps {
                let fp = g.b.build_struct_gep(env_ty, p, field, "").unwrap();
                field += 1;
                match kind {
                    Capture::Box => {
                        let bx = g.b.build_load(g.ptr_ty(), fp, "").unwrap();
                        g.call_rt("tsl_release", &[bx]);
                    }
                    Capture::Binding => {
                        let v = g.load(&Ty::View, fp);
                        g.release(v, &Ty::View);
                    }
                    Capture::Elem => field += 1,
                    Capture::ElemVia => {
                        let v = g.load(&Ty::View, fp);
                        g.release(v, &Ty::View);
                        field += 1;
                    }
                    Capture::Value => {
                        let v = g.load(ty, fp);
                        g.release(v, ty);
                    }
                }
            }
            if let Some(rf) = record_field {
                let fp = g.b.build_struct_gep(env_ty, p, rf, "").unwrap();
                let bx = g.b.build_load(g.ptr_ty(), fp, "").unwrap();
                g.call_rt("tsl_release", &[bx]);
            }
            g.b.build_return(None).unwrap();
        });
        let size = self.td.get_abi_size(&env_ty);
        let i = self.i64();
        let p = self.ptr_ty();
        let ty = self.ctx.struct_type(&[i.into(), i.into(), p.into(), p.into(), p.into(), p.into(), p.into()], false);
        let g = self.module.add_global(ty, None, &format!("tsl_t_env_{n}"));
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

    /// Calls a function value `{fn, env}` with owned arguments.
    fn call_fn_value(&mut self, fv: Val<'ctx>, t: &Ty, args: &[Val<'ctx>]) -> Option<Val<'ctx>> {
        let Ty::Fn(ptys, ret) = t else { unreachable!() };
        let fn_ty = self.fn_type(ptys, ret, false);
        let s = fv.into_struct_value();
        let fp = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
        let env = self.b.build_extract_value(s, 1, "").unwrap();
        let mut all: Vec<BasicMetadataValueEnum> = vec![env.into()];
        all.extend(args.iter().map(|a| BasicMetadataValueEnum::from(*a)));
        self.b.build_indirect_call(fn_ty, fp, &all, "").unwrap().try_as_basic_value().basic()
    }

    // ===== calls =====

    fn call<'e>(&mut self, e: &Expr, callee: &Expr, args: &'e [Arg], trailing: Option<&'e Expr>) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        let Some(info) = self.checked.calls.get(&e.id).cloned() else {
            // Conversions and `map` are recorded without argument mapping.
            return self.unsupported_call(e);
        };
        let arg = move |r: ArgRef| pick(args, trailing, r);
        match &info.callee {
            Callee::Fn(name) => {
                let sig = self.checked.fns[name].clone();
                let decl = self.fn_decls[name];
                let vals = self.call_args(&info, &sig, &decl.params, &arg);
                let f = self.fns[name];
                let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into()];
                all.extend(vals.iter().map(|v| BasicMetadataValueEnum::from(*v)));
                self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic()
            }
            Callee::Method { owner, name } | Callee::SelfMethod { owner, name } => {
                let optional = match &callee.kind {
                    ExprKind::Member { object, optional, .. } => {
                        *optional || (object.in_optional_chain() && matches!(self.ty(object), Ty::Opt(_)))
                    }
                    _ => false,
                };
                if optional {
                    let ExprKind::Member { object, .. } = &callee.kind else { unreachable!() };
                    let ot = self.ty(object);
                    let ov = self.expr(object).unwrap();
                    let (owner, name) = (owner.clone(), name.clone());
                    if t == Ty::Void {
                        let flag = self.b.build_extract_value(ov.into_struct_value(), 0, "").unwrap().into_int_value();
                        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
                        self.b.build_conditional_branch(flag, some_bb, done).unwrap();
                        self.b.position_at_end(some_bb);
                        let iv = self.b.build_extract_value(ov.into_struct_value(), 1, "").unwrap();
                        let recv = self.spill(&owner, iv);
                        self.method_call(&owner, &name, recv, &info, &arg);
                        let cur = self.load(&owner, recv);
                        self.release(cur, &owner);
                        self.br(done);
                        self.b.position_at_end(done);
                        return None;
                    }
                    let info2 = info.clone();
                    let r = self.optional_chain(ov, &ot, &t, |g, v| {
                        let recv = g.spill(&owner, v);
                        let r = g.method_call(&owner, &name, recv, &info2, &arg).unwrap();
                        // The receiver copy is released by `optional_chain`.
                        r
                    });
                    return Some(r);
                }
                let (owner, name) = (owner.clone(), name.clone());
                let (recv, temp) = match (&info.callee, &callee.kind) {
                    (Callee::SelfMethod { .. }, _) => (self.self_ptr().0, None),
                    (_, ExprKind::Member { object, .. }) => {
                        let mutating = self.method_mutating(&owner, &name);
                        if mutating && self.is_place(object) {
                            (self.write_ptr(object), None)
                        } else if let (false, Some(p)) = (mutating, self.read_ptr(object)) {
                            (p, None)
                        } else {
                            let v = self.expr(object).unwrap();
                            (self.spill(&owner, v), Some(v))
                        }
                    }
                    _ => unreachable!(),
                };
                let r = self.method_call(&owner, &name, recv, &info, &arg);
                if temp.is_some() {
                    // A method may have changed the temporary; release what it holds now.
                    let cur = self.load(&owner, recv);
                    self.release(cur, &owner);
                }
                r
            }
            Callee::StructInit(id) => {
                let id = *id;
                let st = self.struct_ty(id);
                let fields = self.checked.structs[id].fields.clone();
                let decl = self.struct_decls[&id];
                let mut s = st.get_undef();
                for (i, f) in fields.iter().enumerate() {
                    let v = match info.args[i] {
                        Some(r) => self.expr_as(arg(r), &f.ty).unwrap(),
                        None => {
                            let default = decl.fields[i].default.as_ref().unwrap();
                            self.expr_as(default, &f.ty).unwrap()
                        }
                    };
                    s = self.b.build_insert_value(s, v, i as u32, "").unwrap().into_struct_value();
                }
                Some(s.into())
            }
            Callee::EnumCase(id, case) => {
                let (id, case) = (*id, *case);
                let pt = self.payload_ty(id, case);
                let fields = self.checked.enums[id].cases[case].fields.clone();
                let info_ptr = self.payload_info(id, case);
                let bx = self.call_rt("tsl_box_new", &[info_ptr.into()]).unwrap().into_pointer_value();
                let data = self.box_data(bx);
                for (i, f) in fields.iter().enumerate() {
                    let v = match info.args[i] {
                        Some(r) => self.expr_as(arg(r), &f.ty).unwrap(),
                        None => unreachable!("enum case values have no defaults"),
                    };
                    let fp = self.b.build_struct_gep(pt, data, i as u32, "").unwrap();
                    self.b.build_store(fp, v).unwrap();
                }
                let st = self.llty(&Ty::Enum(id)).unwrap().into_struct_type();
                let tag = self.ctx.i32_type().const_int(case as u64, false);
                let s = self.b.build_insert_value(st.get_undef(), tag, 0, "").unwrap();
                Some(self.b.build_insert_value(s, bx, 1, "").unwrap().into_struct_value().into())
            }
            Callee::Builtin { recv, name } => {
                let ExprKind::Member { object, optional, .. } = &callee.kind else { unreachable!() };
                if *optional || (object.in_optional_chain() && matches!(self.ty(object), Ty::Opt(_))) {
                    let ot = self.ty(object);
                    let ov = self.expr(object).unwrap();
                    let (recv, name) = (recv.clone(), name.clone());
                    let info2 = info.clone();
                    let r = self.optional_chain(ov, &ot, &t, |g, v| {
                        g.builtin_on_value(&recv, &name, v, &info2, &arg, e.span).unwrap()
                    });
                    return Some(r);
                }
                self.builtin_method(recv, name, object, &info, &arg, args, trailing, &t, e.span)
            }
            Callee::BuiltinFn(name) => self.builtin_fn(name, args, &info, &arg, &t, e.span),
            Callee::Value => {
                // A local holding a function: the checker records its type on the variable.
                let ft = match &callee.kind {
                    ExprKind::Name(n) => self.lookup(n).map(|v| v.ty).unwrap_or_else(|| self.ty(callee)),
                    _ => self.ty(callee),
                };
                let Ty::Fn(ptys, _) = &ft else { unreachable!() };
                let fv = self.expr(callee).unwrap();
                let exprs: Vec<&Expr> = args.iter().map(|a| &a.value).chain(trailing).collect();
                let mut vals = Vec::new();
                for (x, pt) in exprs.into_iter().zip(ptys.clone().iter()) {
                    vals.push(self.expr_as(x, pt).unwrap());
                }
                let r = self.call_fn_value(fv, &ft, &vals);
                self.release(fv, &ft);
                r
            }
            // A view used as a value: wrap it so it can be emitted later.
            Callee::View(_) | Callee::BuiltinView(_) | Callee::Modifier(_) => {
                Some(self.make_closure(Body::ContentExpr(e), &Ty::View))
            }
            Callee::ViewFn(name) => {
                let r = self.stack.last().unwrap().record.clone().expect("view function outside its view");
                let sig = self.checked.view_fns[&(r.view.clone(), name.clone())].clone();
                let decl = self.view_fn_decl(&r.view, name);
                let vals = self.call_args(&info, &sig, &decl.params, &arg);
                let f = self.view_fns[&(r.view.clone(), name.clone())];
                let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into(), r.data.into()];
                all.extend(vals.iter().map(|v| BasicMetadataValueEnum::from(*v)));
                self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic()
            }
        }
    }

    fn unsupported_call(&mut self, e: &Expr) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        self.unsupported(e.span, "this call");
        self.undef(&t)
    }

    fn method_mutating(&self, owner: &Ty, name: &str) -> bool {
        let methods = match owner {
            Ty::Struct(id) => &self.checked.structs[*id].methods,
            Ty::Enum(id) => &self.checked.enums[*id].methods,
            _ => return false,
        };
        methods.iter().any(|m| m.name == name && m.mutating)
    }

    /// Evaluates arguments in parameter order, filling in defaults.
    fn call_args<'e>(
        &mut self,
        info: &CallInfo,
        sig: &FnSig,
        decl_params: &[Param],
        arg: &dyn Fn(ArgRef) -> &'e Expr,
    ) -> Vec<Val<'ctx>> {
        let mut vals = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            let v = match info.args.get(i).copied().flatten() {
                Some(r) => self.expr_as(arg(r), &p.ty),
                None => {
                    let default = decl_params[i].default.as_ref().expect("default value");
                    self.expr_as(default, &p.ty)
                }
            };
            vals.push(v.unwrap());
        }
        vals
    }

    fn method_call<'e>(
        &mut self,
        owner: &Ty,
        name: &str,
        recv: PointerValue<'ctx>,
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
    ) -> Option<Val<'ctx>> {
        let owner_name = match owner {
            Ty::Struct(id) => self.checked.structs[*id].name.clone(),
            Ty::Enum(id) => self.checked.enums[*id].name.clone(),
            _ => unreachable!(),
        };
        let key = (owner_name, name.to_string());
        let sig = self.method_sig(owner, name);
        let (decl, _) = self.method_decls[&key].clone();
        let vals = self.call_args(info, &sig, &decl.params, arg);
        let f = self.methods[&key];
        let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into(), recv.into()];
        all.extend(vals.iter().map(|v| BasicMetadataValueEnum::from(*v)));
        self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic()
    }

    #[allow(clippy::too_many_arguments)]
    fn builtin_method<'e>(
        &mut self,
        recv: &Ty,
        name: &str,
        object: &Expr,
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
        args: &'e [Arg],
        trailing: Option<&'e Expr>,
        t: &Ty,
        span: Span,
    ) -> Option<Val<'ctx>> {
        // Methods that change the receiver work on its storage.
        match (recv, name) {
            (Ty::List(elem), "append" | "insert" | "remove" | "removeAll") => {
                let elem = (**elem).clone();
                let slot = self.write_ptr(object);
                let l = self.loc(span);
                match name {
                    "append" | "insert" => {
                        let v = self.expr_as(arg(info.args[0].unwrap()), &elem).unwrap();
                        let p = self.spill(&elem, v);
                        if name == "append" {
                            self.call_rt("tsl_list_push", &[slot.into(), p.into()]);
                        } else {
                            let i = self.expr(arg(info.args[1].unwrap())).unwrap();
                            self.call_rt("tsl_list_insert", &[slot.into(), p.into(), i, l.into()]);
                        }
                        None
                    }
                    "remove" => {
                        let i = self.expr(arg(info.args[0].unwrap())).unwrap();
                        let lt = self.llty(&elem).unwrap();
                        let out = self.alloca(lt, "removed");
                        self.call_rt("tsl_list_remove", &[slot.into(), i, out.into(), l.into()]);
                        Some(self.load(&elem, out))
                    }
                    _ => {
                        // removeAll(where:): keep the items the predicate rejects.
                        let pred_t = Ty::func(vec![elem.clone()], Ty::Bool);
                        let pred = self.expr_as(arg(info.args[0].unwrap()), &pred_t).unwrap();
                        let list = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                        let kept = self.filter_list(list, &elem, pred, &pred_t, true);
                        let old = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                        self.b.build_store(slot, kept).unwrap();
                        self.release(old, &Ty::List(Box::new(elem)));
                        self.release(pred, &pred_t);
                        None
                    }
                }
            }
            (Ty::Dict(kt, vt), "removeValue") => {
                let (kt, vt) = ((**kt).clone(), (**vt).clone());
                let slot = self.write_ptr(object);
                let k = self.expr_as(arg(info.args[0].unwrap()), &kt).unwrap();
                let kp = self.spill(&kt, k);
                let vlt = self.llty(&vt).unwrap();
                let out = self.alloca(vlt, "removed");
                self.b.build_store(out, vlt.const_zero()).unwrap();
                let found = self.call_rt("tsl_dict_remove", &[slot.into(), kp.into(), out.into()]);
                let found = self.truthy(found);
                let v = self.load(&vt, out);
                self.release(k, &kt);
                let st = self.llty(t).unwrap().into_struct_type();
                let s = self.b.build_insert_value(st.get_undef(), found, 0, "").unwrap();
                Some(self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value().into())
            }
            (Ty::List(elem), "map") => {
                let f_expr = match (args, trailing) {
                    ([], Some(tr)) => tr,
                    ([a], _) => &a.value,
                    _ => unreachable!(),
                };
                let ft = self.ty(f_expr);
                let Ty::Fn(_, out_t) = &ft else { unreachable!() };
                let out_t = (**out_t).clone();
                let f = self.expr(f_expr).unwrap();
                let list = self.expr(object).unwrap();
                let elem = (**elem).clone();
                let info_ptr = self.type_info(&out_t);
                let len = self.call_rt("tsl_list_len", &[list]).unwrap();
                let out = self.call_rt("tsl_list_new", &[info_ptr.into(), len]).unwrap();
                let out_slot = self.spill(&Ty::List(Box::new(out_t.clone())), out);
                self.each(list, &elem, span, |g, item| {
                    let r = g.call_fn_value(f, &ft, &[item]).unwrap();
                    let p = g.spill(&out_t, r);
                    g.call_rt("tsl_list_push", &[out_slot.into(), p.into()]);
                });
                self.release(list, recv);
                self.release(f, &ft);
                Some(self.b.build_load(self.ptr_ty(), out_slot, "").unwrap())
            }
            _ => {
                let v = self.expr(object).unwrap();
                let r = self.builtin_on_value(recv, name, v, info, arg, span);
                self.release(v, recv);
                r
            }
        }
    }

    /// Calls `f(item)` (with an owned item) for each element of `list`.
    fn each(&mut self, list: Val<'ctx>, elem: &Ty, span: Span, f: impl FnOnce(&mut Self, Val<'ctx>)) {
        let idx = self.alloca(self.i64().into(), "i");
        self.b.build_store(idx, self.i64().const_zero()).unwrap();
        let (cond_bb, body_bb, done) = (self.block_named("each"), self.block_named("item"), self.block_named("done"));
        self.b.build_unconditional_branch(cond_bb).unwrap();
        self.b.position_at_end(cond_bb);
        let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
        let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, cur, len, "").unwrap();
        self.b.build_conditional_branch(more, body_bb, done).unwrap();
        self.b.position_at_end(body_bb);
        let l = self.loc(span);
        let p = self.call_rt("tsl_list_get", &[list, cur.into(), l.into()]).unwrap().into_pointer_value();
        let item = self.load(elem, p);
        self.retain(item, elem);
        f(self, item);
        let next = self.b.build_int_add(cur, self.i64().const_int(1, false), "").unwrap();
        self.b.build_store(idx, next).unwrap();
        self.br(cond_bb);
        self.b.position_at_end(done);
    }

    /// A new list of the items for which `pred` is true (or false, with `invert`).
    fn filter_list(&mut self, list: Val<'ctx>, elem: &Ty, pred: Val<'ctx>, pred_t: &Ty, invert: bool) -> Val<'ctx> {
        let info = self.type_info(elem);
        let zero = self.i64().const_zero();
        let out = self.call_rt("tsl_list_new", &[info.into(), zero.into()]).unwrap();
        let out_slot = self.spill(&Ty::List(Box::new(elem.clone())), out);
        let span = Span::default();
        self.each(list, elem, span, |g, item| {
            g.retain(item, elem);
            let keep = g.call_fn_value(pred, pred_t, &[item]).unwrap().into_int_value();
            let keep = if invert { g.b.build_not(keep, "").unwrap() } else { keep };
            let (yes, no, next) = (g.block_named("keep"), g.block_named("drop"), g.block_named("next"));
            g.b.build_conditional_branch(keep, yes, no).unwrap();
            g.b.position_at_end(yes);
            let p = g.spill(elem, item);
            g.call_rt("tsl_list_push", &[out_slot.into(), p.into()]);
            g.br(next);
            g.b.position_at_end(no);
            g.release(item, elem);
            g.br(next);
            g.b.position_at_end(next);
        });
        self.b.build_load(self.ptr_ty(), out_slot, "").unwrap()
    }

    /// Non-changing methods of built-in types, on a borrowed receiver.
    fn builtin_on_value<'e>(
        &mut self,
        recv: &Ty,
        name: &str,
        v: Val<'ctx>,
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
        span: Span,
    ) -> Option<Val<'ctx>> {
        let a = |g: &mut Self, i: usize, t: &Ty| -> Option<Val<'ctx>> {
            info.args.get(i).copied().flatten().map(|r| g.expr_as(arg(r), t).unwrap())
        };
        match recv {
            Ty::Str => {
                let r = match name {
                    "contains" | "hasPrefix" | "hasSuffix" => {
                        let s = a(self, 0, &Ty::Str).unwrap();
                        let f = match name {
                            "contains" => "tsl_str_contains",
                            "hasPrefix" => "tsl_str_has_prefix",
                            _ => "tsl_str_has_suffix",
                        };
                        let r = self.call_rt(f, &[v, s]);
                        self.release(s, &Ty::Str);
                        self.truthy(r).into()
                    }
                    "split" => {
                        let s = a(self, 0, &Ty::Str).unwrap();
                        let r = self.call_rt("tsl_str_split", &[v, s]).unwrap();
                        self.release(s, &Ty::Str);
                        r
                    }
                    "trim" => self.call_rt("tsl_str_trim", &[v]).unwrap(),
                    "lines" => self.call_rt("tsl_str_lines", &[v]).unwrap(),
                    "substring" => {
                        let from = a(self, 0, &Ty::Int).unwrap();
                        let to = a(self, 1, &Ty::Int).unwrap();
                        self.call_rt("tsl_str_substring", &[v, from, to]).unwrap()
                    }
                    "find" => {
                        let part = a(self, 0, &Ty::Str).unwrap();
                        let i = self.call_rt("tsl_str_find", &[v, part]).unwrap().into_int_value();
                        self.release(part, &Ty::Str);
                        let found = self.b.build_int_compare(IntPredicate::SGE, i, self.i64().const_zero(), "").unwrap();
                        let opt = Ty::opt(Ty::Int);
                        let st = self.llty(&opt).unwrap().into_struct_type();
                        let s = self.b.build_insert_value(st.get_undef(), found, 0, "").unwrap();
                        self.b.build_insert_value(s, i, 1, "").unwrap().into_struct_value().into()
                    }
                    "uppercase" => self.call_rt("tsl_str_uppercase", &[v]).unwrap(),
                    "lowercase" => self.call_rt("tsl_str_lowercase", &[v]).unwrap(),
                    "replace" => {
                        let old = a(self, 0, &Ty::Str).unwrap();
                        let new = a(self, 1, &Ty::Str).unwrap();
                        let r = self.call_rt("tsl_str_replace", &[v, old, new]).unwrap();
                        self.release(old, &Ty::Str);
                        self.release(new, &Ty::Str);
                        r
                    }
                    _ => unreachable!(),
                };
                Some(r)
            }
            Ty::List(elem) => {
                let elem = (**elem).clone();
                Some(match name {
                    "filter" => {
                        let pred_t = Ty::func(vec![elem.clone()], Ty::Bool);
                        let pred = a(self, 0, &pred_t).unwrap();
                        let r = self.filter_list(v, &elem, pred, &pred_t, false);
                        self.release(pred, &pred_t);
                        r
                    }
                    "first" | "last" => {
                        let pred_t = Ty::func(vec![elem.clone()], Ty::Bool);
                        let pred = a(self, 0, &pred_t).unwrap();
                        let r = self.find(v, &elem, pred, &pred_t, name == "last", span);
                        self.release(pred, &pred_t);
                        r
                    }
                    "contains" => {
                        let item = a(self, 0, &elem).unwrap();
                        let p = self.spill(&elem, item);
                        let r = self.call_rt("tsl_list_contains", &[v, p.into()]);
                        self.release(item, &elem);
                        self.truthy(r).into()
                    }
                    "reversed" => self.call_rt("tsl_list_reversed", &[v]).unwrap(),
                    "joined" => {
                        let sep = a(self, 0, &Ty::Str).unwrap_or_else(|| self.str_lit("").into());
                        let r = self.call_rt("tsl_list_joined", &[v, sep]).unwrap();
                        self.release(sep, &Ty::Str);
                        r
                    }
                    "sorted" => match a(self, 0, &Ty::func(vec![elem.clone(), elem.clone()], Ty::Bool)) {
                        None => {
                            let null = self.ptr_ty().const_null();
                            self.call_rt("tsl_list_sorted", &[v, null.into(), null.into()]).unwrap()
                        }
                        Some(less) => {
                            let lt = Ty::func(vec![elem.clone(), elem.clone()], Ty::Bool);
                            let adapter = self.sort_adapter(&elem);
                            let ctx = self.spill(&lt, less);
                            let r = self
                                .call_rt("tsl_list_sorted", &[v, adapter.into(), ctx.into()])
                                .unwrap();
                            self.release(less, &lt);
                            r
                        }
                    },
                    _ => unreachable!("list method {name}"),
                })
            }
            Ty::Dict(kt, _) => {
                let key = a(self, 0, kt).unwrap();
                let p = self.spill(kt, key);
                let r = self.call_rt("tsl_dict_contains", &[v, p.into()]);
                self.release(key, kt);
                Some(self.truthy(r).into())
            }
            _ => unreachable!(),
        }
    }

    /// The first (or last) item matching `pred`, as an optional.
    fn find(&mut self, list: Val<'ctx>, elem: &Ty, pred: Val<'ctx>, pred_t: &Ty, last: bool, span: Span) -> Val<'ctx> {
        let opt = Ty::opt(elem.clone());
        let lt = self.llty(&opt).unwrap();
        let slot = self.alloca(lt, "found");
        let none = self.none(&opt);
        self.b.build_store(slot, none).unwrap();
        let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
        let idx = self.alloca(self.i64().into(), "i");
        let start = if last { self.b.build_int_sub(len, self.i64().const_int(1, false), "").unwrap() } else { self.i64().const_zero() };
        self.b.build_store(idx, start).unwrap();
        let (cond_bb, body_bb, hit, done) =
            (self.block_named("find"), self.block_named("test"), self.block_named("hit"), self.block_named("done"));
        self.b.build_unconditional_branch(cond_bb).unwrap();
        self.b.position_at_end(cond_bb);
        let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
        let more = if last {
            self.b.build_int_compare(IntPredicate::SGE, cur, self.i64().const_zero(), "").unwrap()
        } else {
            self.b.build_int_compare(IntPredicate::SLT, cur, len, "").unwrap()
        };
        self.b.build_conditional_branch(more, body_bb, done).unwrap();
        self.b.position_at_end(body_bb);
        let l = self.loc(span);
        let p = self.call_rt("tsl_list_get", &[list, cur.into(), l.into()]).unwrap().into_pointer_value();
        let item = self.load(elem, p);
        self.retain(item, elem);
        let ok = self.call_fn_value(pred, pred_t, &[item]).unwrap().into_int_value();
        let step = self.block_named("step");
        self.b.build_conditional_branch(ok, hit, step).unwrap();
        self.b.position_at_end(step);
        let delta = self.i64().const_int(1, false);
        let next = if last { self.b.build_int_sub(cur, delta, "").unwrap() } else { self.b.build_int_add(cur, delta, "").unwrap() };
        self.b.build_store(idx, next).unwrap();
        self.br(cond_bb);
        self.b.position_at_end(hit);
        let item = self.load(elem, p);
        self.retain(item, elem);
        let s = self.some(&opt, item);
        self.b.build_store(slot, s).unwrap();
        self.br(done);
        self.b.position_at_end(done);
        self.load(&opt, slot)
    }

    /// An optional from a pointer that may be null (runtime results).
    fn opt_from_ptr(&mut self, p: PointerValue<'ctx>, opt_ty: &Ty) -> Val<'ctx> {
        let present = self.b.build_is_not_null(p, "").unwrap();
        let st = self.llty(opt_ty).unwrap().into_struct_type();
        let s = self.b.build_insert_value(st.get_undef(), present, 0, "").unwrap();
        self.b.build_insert_value(s, p, 1, "").unwrap().into_struct_value().into()
    }

    /// `void adapter(ptr box, ptr text)`: calls the `fn(String?)` closure in
    /// `box` with the fetched text (owned), or `nil` if `text` is null.
    fn fetch_adapter(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_h_fetch_done";
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![Ty::opt(Ty::Str)], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let text = f.get_nth_param(1).unwrap().into_pointer_value();
            let data = g.box_data(bx);
            let closure = g.load(&ft, data);
            let opt_t = Ty::opt(Ty::Str);
            let present = g.b.build_is_not_null(text, "").unwrap();
            let st = g.llty(&opt_t).unwrap().into_struct_type();
            let s = g.b.build_insert_value(st.get_undef(), present, 0, "").unwrap();
            let s = g.b.build_insert_value(s, text, 1, "").unwrap().into_struct_value();
            g.call_fn_value(closure, &ft, &[s.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `i8 adapter(ptr closure, ptr a, ptr b)`: calls a `fn(T, T) -> Bool` closure for the runtime's sort.
    fn sort_adapter(&mut self, elem: &Ty) -> PointerValue<'ctx> {
        let name = format!("tsl_h_less_{}", self.key(elem));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.i8().fn_type(&[p.into(), p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let lt = Ty::func(vec![elem.clone(), elem.clone()], Ty::Bool);
        self.in_helper(f, |g| {
            let ctx = f.get_nth_param(0).unwrap().into_pointer_value();
            let (a, b) = (f.get_nth_param(1).unwrap().into_pointer_value(), f.get_nth_param(2).unwrap().into_pointer_value());
            let closure = g.load(&lt, ctx);
            let (av, bv) = (g.load(elem, a), g.load(elem, b));
            g.retain(av, elem);
            g.retain(bv, elem);
            let r = g.call_fn_value(closure, &lt, &[av, bv]).unwrap().into_int_value();
            let r = g.to_c_bool(r);
            g.b.build_return(Some(&r)).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    fn builtin_fn<'e>(
        &mut self,
        name: &str,
        args: &[Arg],
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
        t: &Ty,
        span: Span,
    ) -> Option<Val<'ctx>> {
        match name {
            "print" => {
                let x = arg(info.args[0].unwrap());
                let xt = self.ty(x);
                let v = self.expr(x).unwrap();
                let s = self.stringify(v, &xt);
                self.call_rt("tsl_print", &[s]);
                self.release(s, &Ty::Str);
                None
            }
            "Int" | "Float" | "String" => {
                let x = &args[0].value;
                let xt = self.ty(x);
                let v = self.expr(x).unwrap();
                Some(match (name, &xt) {
                    ("String", _) => self.stringify(v, &xt),
                    ("Int", Ty::Int) | ("Float", Ty::Float) => v,
                    ("Int", Ty::Float) => {
                        let intr = Intrinsic::find("llvm.fptosi.sat").unwrap();
                        let f = intr.get_declaration(&self.module, &[self.i64().into(), self.ctx.f64_type().into()]).unwrap();
                        self.b.build_call(f, &[v.into()], "").unwrap().try_as_basic_value().basic().unwrap()
                    }
                    ("Float", Ty::Int) => {
                        self.b.build_signed_int_to_float(v.into_int_value(), self.ctx.f64_type(), "").unwrap().into()
                    }
                    (_, Ty::Str) => {
                        let (inner, f) = if name == "Int" { (Ty::Int, "tsl_str_to_int") } else { (Ty::Float, "tsl_str_to_float") };
                        let lt = self.llty(&inner).unwrap();
                        let out = self.alloca(lt, "parsed");
                        self.b.build_store(out, lt.const_zero()).unwrap();
                        let ok = self.call_rt(f, &[v, out.into()]);
                        let ok = self.truthy(ok);
                        let n = self.load(&inner, out);
                        self.release(v, &Ty::Str);
                        let st = self.llty(t).unwrap().into_struct_type();
                        let s = self.b.build_insert_value(st.get_undef(), ok, 0, "").unwrap();
                        self.b.build_insert_value(s, n, 1, "").unwrap().into_struct_value().into()
                    }
                    _ => unreachable!(),
                })
            }
            "fetch" => {
                let url = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let ft = Ty::func(vec![Ty::opt(Ty::Str)], Ty::Void);
                let done = self.expr_as(arg(info.args[1].unwrap()), &ft).unwrap();
                // The runtime calls `adapter(box, text-or-null)`; the box holds `done`.
                let ti = self.type_info(&ft);
                let bx = self.call_rt("tsl_box_new", &[ti.into()]).unwrap().into_pointer_value();
                let data = self.box_data(bx);
                self.b.build_store(data, done).unwrap();
                let adapter = self.fetch_adapter();
                self.call_rt("tsl_fetch", &[url, adapter.into(), bx.into()]);
                self.release(url, &Ty::Str);
                None
            }
            "readFile" | "listFolder" | "environment" => {
                let s = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let f = match name {
                    "readFile" => "tsl_read_file",
                    "listFolder" => "tsl_list_folder",
                    _ => "tsl_environment",
                };
                let p = self.call_rt(f, &[s]).unwrap().into_pointer_value();
                self.release(s, &Ty::Str);
                Some(self.opt_from_ptr(p, t))
            }
            "writeFile" | "fileExists" | "isFolder" | "deleteFile" | "createFolder" | "renameFile" => {
                let mut vals = Vec::new();
                for r in info.args.iter().flatten() {
                    vals.push(self.expr_as(arg(*r), &Ty::Str).unwrap());
                }
                let f = match name {
                    "writeFile" => "tsl_write_file",
                    "fileExists" => "tsl_file_exists",
                    "deleteFile" => "tsl_delete_file",
                    "createFolder" => "tsl_create_folder",
                    "renameFile" => "tsl_rename_file",
                    _ => "tsl_is_folder",
                };
                let r = self.call_rt(f, &vals);
                for v in vals {
                    self.release(v, &Ty::Str);
                }
                Some(self.truthy(r).into())
            }
            "currentFolder" => self.call_rt("tsl_current_folder", &[]),
            "runCommand" => {
                let path = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let list_t = Ty::list(Ty::Str);
                let args = match info.args[1] {
                    Some(r) => self.expr_as(arg(r), &list_t).unwrap(),
                    None => {
                        let ti = self.type_info(&Ty::Str);
                        let zero = self.i64().const_zero();
                        self.call_rt("tsl_list_new", &[ti.into(), zero.into()]).unwrap()
                    }
                };
                let out_t = Ty::func(vec![Ty::Str], Ty::Void);
                let exit_t = Ty::func(vec![Ty::Int], Ty::Void);
                let on_out = self.expr_as(arg(info.args[2].unwrap()), &out_t).unwrap().into_struct_value();
                let on_exit = self.expr_as(arg(info.args[3].unwrap()), &exit_t).unwrap().into_struct_value();
                let mut all = vec![path, args];
                for c in [on_out, on_exit] {
                    all.push(self.b.build_extract_value(c, 0, "").unwrap());
                    all.push(self.b.build_extract_value(c, 1, "").unwrap());
                }
                let id = self.call_rt("tsl_run_command", &all);
                self.release(path, &Ty::Str);
                self.release(args, &list_t);
                id
            }
            "stopCommand" => {
                let id = self.expr_as(arg(info.args[0].unwrap()), &Ty::Int).unwrap();
                self.call_rt("tsl_stop_command", &[id]);
                None
            }
            "after" => {
                let secs = self.expr_as(arg(info.args[0].unwrap()), &Ty::Float).unwrap();
                let action = self.expr_as(arg(info.args[1].unwrap()), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_after", &[secs], action);
                None
            }
            _ => {
                self.unsupported(span, &format!("`{name}`"));
                self.undef(t)
            }
        }
    }
}

// ===== helpers =====

fn undef_of(t: BasicTypeEnum) -> BasicValueEnum {
    match t {
        BasicTypeEnum::IntType(t) => t.get_undef().into(),
        BasicTypeEnum::FloatType(t) => t.get_undef().into(),
        BasicTypeEnum::PointerType(t) => t.get_undef().into(),
        BasicTypeEnum::StructType(t) => t.get_undef().into(),
        BasicTypeEnum::ArrayType(t) => t.get_undef().into(),
        BasicTypeEnum::VectorType(t) => t.get_undef().into(),
        BasicTypeEnum::ScalableVectorType(t) => t.get_undef().into(),
    }
}

fn pick<'e>(args: &'e [Arg], trailing: Option<&'e Expr>, r: ArgRef) -> &'e Expr {
    match r {
        ArgRef::Arg(i) => &args[i].value,
        ArgRef::Trailing => trailing.expect("trailing block"),
    }
}

fn int_pred(op: BinOp) -> IntPredicate {
    match op {
        BinOp::Lt => IntPredicate::SLT,
        BinOp::Le => IntPredicate::SLE,
        BinOp::Gt => IntPredicate::SGT,
        _ => IntPredicate::SGE,
    }
}

fn crate_equatable(t: &Ty) -> bool {
    match t {
        Ty::Fn(..) | Ty::View | Ty::Any | Ty::Void | Ty::Range => false,
        Ty::List(t) | Ty::Opt(t) => crate_equatable(t),
        Ty::Dict(k, v) => crate_equatable(k) && crate_equatable(v),
        _ => true,
    }
}

fn hashable(t: &Ty, g: &Gen) -> bool {
    match t {
        Ty::Int | Ty::Str | Ty::Bool => true,
        Ty::Enum(id) => !g.has_payload(*id),
        _ => false,
    }
}

/// `var`s in `body` whose names are used inside closures: they must live in
/// boxes so the closure and the function share them. (Over-approximating is
/// safe; boxing a variable never changes what the program does.)
fn boxed_vars(body: &Block) -> HashSet<String> {
    let mut vars = HashSet::new();
    let mut captured = BTreeSet::new();
    walk_block(body, &mut |node| match node {
        Node::S(Stmt::Var(d)) => {
            vars.insert(d.name.name.clone());
        }
        Node::E(Expr { kind: ExprKind::Closure { params, body }, .. }) => {
            let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            free_block(body, &mut bound, &mut captured);
        }
        _ => {}
    });
    vars.into_iter().filter(|v| captured.contains(v)).collect()
}

enum Node<'a> {
    S(&'a Stmt),
    E(&'a Expr),
}

/// Visits every statement and expression in `b`, including nested blocks and closures.
fn walk_block<'a>(b: &'a Block, f: &mut dyn FnMut(Node<'a>)) {
    for s in &b.stmts {
        walk_stmt(s, f);
    }
}

fn walk_stmt<'a>(s: &'a Stmt, f: &mut dyn FnMut(Node<'a>)) {
    f(Node::S(s));
    match s {
        Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => walk_expr(&d.value, f),
        Stmt::Assign { target, value, .. } => {
            walk_expr(target, f);
            walk_expr(value, f);
        }
        Stmt::Return { value: Some(v), .. } => walk_expr(v, f),
        Stmt::Return { .. } => {}
        Stmt::For { iter, body, .. } => {
            walk_expr(iter, f);
            walk_block(body, f);
        }
        Stmt::Expr(e) => walk_expr(e, f),
        Stmt::Fn(d) => walk_block(&d.body, f),
    }
}

fn walk_expr<'a>(e: &'a Expr, f: &mut dyn FnMut(Node<'a>)) {
    f(Node::E(e));
    match &e.kind {
        ExprKind::Closure { body, .. } | ExprKind::Block(body) => walk_block(body, f),
        ExprKind::If { cond, then, otherwise } => {
            match cond {
                Cond::Expr(c) | Cond::Let(_, c) => walk_expr(c, f),
            }
            walk_block(then, f);
            match otherwise {
                Some(Else::Block(b)) => walk_block(b, f),
                Some(Else::If(x)) => walk_expr(x, f),
                None => {}
            }
        }
        ExprKind::Match { subject, arms } => {
            walk_expr(subject, f);
            for arm in arms {
                if let Pattern::Literal(l) = &arm.pattern {
                    walk_expr(l, f);
                }
                walk_expr(&arm.body, f);
            }
        }
        _ => for_each_child(e, &mut |c| walk_expr(c, f)),
    }
}

/// Collects names used in `b` that aren't declared inside it.
fn free_block(b: &Block, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
    let mark = bound.len();
    for s in &b.stmts {
        match s {
            Stmt::Let(d) | Stmt::Var(d) | Stmt::State(d) => {
                free_expr(&d.value, bound, out);
                bound.push(d.name.name.clone());
            }
            Stmt::Assign { target, value, .. } => {
                free_expr(target, bound, out);
                free_expr(value, bound, out);
            }
            Stmt::Return { value: Some(v), .. } => free_expr(v, bound, out),
            Stmt::For { var, iter, body, .. } => {
                free_expr(iter, bound, out);
                bound.push(var.name.clone());
                free_block(body, bound, out);
                bound.pop();
            }
            Stmt::Expr(e) => free_expr(e, bound, out),
            _ => {}
        }
    }
    bound.truncate(mark);
}

fn free_expr(e: &Expr, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Name(n) => {
            if !bound.contains(n) {
                out.insert(n.clone());
            }
        }
        ExprKind::Closure { params, body } => {
            let mark = bound.len();
            bound.extend(params.iter().map(|p| p.name.clone()));
            free_block(body, bound, out);
            bound.truncate(mark);
        }
        ExprKind::If { cond, then, otherwise } => {
            let mark = bound.len();
            match cond {
                Cond::Expr(c) => free_expr(c, bound, out),
                Cond::Let(name, v) => {
                    free_expr(v, bound, out);
                    bound.push(name.name.clone());
                }
            }
            free_block(then, bound, out);
            bound.truncate(mark);
            match otherwise {
                Some(Else::Block(b)) => free_block(b, bound, out),
                Some(Else::If(e)) => free_expr(e, bound, out),
                None => {}
            }
        }
        ExprKind::Match { subject, arms } => {
            free_expr(subject, bound, out);
            for arm in arms {
                let mark = bound.len();
                if let Pattern::Case { bindings, .. } = &arm.pattern {
                    bound.extend(bindings.iter().map(|b| b.name.clone()));
                }
                free_expr(&arm.body, bound, out);
                bound.truncate(mark);
            }
        }
        ExprKind::Block(b) => free_block(b, bound, out),
        _ => for_each_child(e, &mut |c| free_expr(c, bound, out)),
    }
}

/// Direct sub-expressions of expressions that contain no blocks.
fn for_each_child<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    match &e.kind {
        ExprKind::Str(parts) => {
            for p in parts {
                if let StrPart::Expr(x) = p {
                    f(x);
                }
            }
        }
        ExprKind::List(items) => items.iter().for_each(&mut *f),
        ExprKind::Dict(pairs) => {
            for (k, v) in pairs {
                f(k);
                f(v);
            }
        }
        ExprKind::Unary(_, x) => f(x),
        ExprKind::Binary(_, l, r) | ExprKind::Index(l, r) => {
            f(l);
            f(r);
        }
        ExprKind::Call { callee, args, trailing } => {
            f(callee);
            for a in args {
                f(&a.value);
            }
            if let Some(t) = trailing {
                f(t);
            }
        }
        ExprKind::Member { object, .. } => f(object),
        _ => {}
    }
}
