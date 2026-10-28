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

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetData, TargetMachine, TargetTriple,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};

use crate::ast::*;
use crate::builtins;
use crate::check::{ArgRef, CallInfo, Callee, Checked, TryTarget};
use crate::diag::Diagnostic;
use crate::source::{SourceMap, Span};
use crate::types::{FnSig, Ty};

mod codec;
mod coro;
mod debug;
mod stdlib;
mod ui;

/// Byte offset of a box's data (matches the runtime's `BOX_DATA`).
const BOX_DATA: u64 = 32;

/// Values of structs larger than this many bytes (16 fields that are
/// numbers, strings or lists) are moved as blocks of memory, and kept whole
/// in their variables (see `Gen::big` and `Gen::alloca`). Smaller ones are
/// left to the optimizer, which keeps their fields in registers: that's
/// faster, and costs little while they're small.
const BIG_VALUE: u64 = 128;

/// Compiles a checked program with a `fn main()` to a native object file.
/// The triple for this computer. On macOS, for the oldest supported version
/// rather than this machine's.
fn host_triple() -> TargetTriple {
    let triple = TargetMachine::get_default_triple();
    let host = triple.as_str().to_string_lossy().to_string();
    match host.split_once("-apple-darwin") {
        Some((arch, _)) => TargetTriple::create(&format!("{arch}-apple-macosx11.0.0")),
        None => triple,
    }
}

/// Math functions, in the order the runtime's `tsl_math` numbers them.
const MATH_FNS: &[&str] = &[
    "sqrt", "pow", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "floor", "ceil",
    "round", "abs", "min", "max",
];

/// Built-in methods that change their receiver (as in the checker).
const MUTATING_BUILTINS: &[&str] =
    &["append", "insert", "remove", "removeAll", "removeValue", "removeFirst", "removeLast", "appendAll"];

pub fn compile(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    out: &Path,
    optimize: bool,
    target_triple: Option<&str>,
) -> Result<(), Vec<Diagnostic>> {
    generate(program, checked, sources, optimize, target_triple, Mode::Program, |tm, module| {
        tm.write_to_file(module, FileType::Object, out).map_err(|e| vec![internal(e.to_string())])
    })
}

/// Compiles a checked program for `tessel debug`: like `compile`, for this
/// computer, with the program telling the runtime's debugger where it is
/// and what its variables are as it runs (see `debug`).
pub fn compile_debug(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    out: &Path,
    target_triple: Option<&str>,
) -> Result<(), Vec<Diagnostic>> {
    generate(program, checked, sources, true, target_triple, Mode::Debug, |tm, module| {
        tm.write_to_file(module, FileType::Object, out).map_err(|e| vec![internal(e.to_string())])
    })
}

/// A REPL entry to compile: its number in the session, and the session's
/// variables (in the order of the parameters of `REPL_FN`).
#[derive(Clone, Copy)]
pub struct ReplEntry<'a> {
    pub index: usize,
    pub vars: &'a [crate::ReplVar],
}

/// The name of the function that runs entry `index` of a REPL session.
pub fn repl_entry_symbol(index: usize) -> String {
    format!("tsl_repl_{index}")
}

/// The name of the global of a variable declared by entry `index`. It holds
/// a box with the variable's value.
pub fn repl_var_symbol(index: usize, name: &str) -> String {
    format!("tsl_replvar_{index}_{name}")
}

/// Compiles a checked REPL entry (see `analyze_repl`) for this computer, to
/// an object file in memory for `jit::Jit`. Its statements are the function
/// `repl_entry_symbol(index)`; the variables they declare are left in
/// globals (`repl_var_symbol`), where later entries find them.
pub fn compile_repl(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    entry: ReplEntry,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    generate(program, checked, sources, true, None, Mode::Repl(entry), |tm, module| {
        let buffer = tm.write_to_memory_buffer(module, FileType::Object).map_err(|e| vec![internal(e.to_string())])?;
        Ok(buffer.as_slice().to_vec())
    })
}

/// The name of the function that starts, or reloads, a program compiled by
/// `compile_hot`.
pub fn hot_entry_symbol(generation: usize) -> String {
    format!("tsl_hot_{generation}")
}

/// Compiles a checked program with an `app` for `tessel run --hot`: to an
/// object file in memory for `jit::Jit`, whose entry point
/// (`hot_entry_symbol(generation)`) starts the app the first time, and gives
/// the running app its new code after that.
pub fn compile_hot(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    generation: usize,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    generate(program, checked, sources, true, None, Mode::Hot(generation), |tm, module| {
        let buffer = tm.write_to_memory_buffer(module, FileType::Object).map_err(|e| vec![internal(e.to_string())])?;
        Ok(buffer.as_slice().to_vec())
    })
}

/// Compiles a checked program with an `app` for `tessel preview`: like
/// `compile_hot` (the entry point is `hot_entry_symbol(1)`), but each view
/// on screen knows which view expression of the source it is. Returns the
/// object file and those expressions.
pub fn compile_design(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
) -> Result<(Vec<u8>, crate::design::Views), Vec<Diagnostic>> {
    let views = RefCell::new(crate::design::Views::default());
    let object = generate(program, checked, sources, false, None, Mode::Design(&views), |tm, module| {
        let buffer = tm.write_to_memory_buffer(module, FileType::Object).map_err(|e| vec![internal(e.to_string())])?;
        Ok(buffer.as_slice().to_vec())
    })?;
    Ok((object, views.into_inner()))
}

/// What's being compiled.
#[derive(Clone, Copy)]
enum Mode<'a> {
    /// A program, to an object file that's linked into an executable.
    Program,
    /// A program for the debugger (see `debug`).
    Debug,
    Repl(ReplEntry<'a>),
    /// A program for hot reload: this many times compiled.
    Hot(usize),
    /// A program for `tessel preview`, whose view expressions are listed here.
    Design(&'a RefCell<crate::design::Views>),
}

/// The machine code is for: `target_triple`, or this computer. `jit` is for
/// code loaded into `tessel` itself (see `generate`).
fn target_machine(target_triple: Option<&str>, optimize: bool, jit: bool) -> Result<(TargetMachine, TargetTriple, String), String> {
    let config = InitializationConfig::default();
    Target::initialize_x86(&config);
    Target::initialize_aarch64(&config);
    let triple = match target_triple {
        Some(t) => TargetTriple::create(t),
        None => host_triple(),
    };
    let name = triple.as_str().to_string_lossy().to_string();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let level = if optimize { OptimizationLevel::Default } else { OptimizationLevel::None };
    // A baseline CPU, not this machine's: apps must run on every computer of
    // their kind, not just ones as new as the one that built them.
    let cpu = if name.starts_with("aarch64-apple") || name.starts_with("arm64-apple") {
        "apple-m1"
    } else if name.starts_with("aarch64") {
        "generic"
    } else {
        "x86-64"
    };
    let model = if jit && name.contains("windows") { CodeModel::Large } else { CodeModel::Default };
    let tm = target
        .create_target_machine(&triple, cpu, "", level, RelocMode::PIC, model)
        .ok_or_else(|| format!("can't create a target machine for `{name}`"))?;
    Ok((tm, triple, name))
}

/// An object file whose `main` only calls `function`, a runtime function
/// that takes nothing and returns the exit code: for programs that are all
/// runtime (the installer of Windows apps, see `tessel build --bundle`).
pub fn compile_entry(function: &str, out: &Path, target_triple: Option<&str>) -> Result<(), String> {
    let (tm, triple, _) = target_machine(target_triple, true, false)?;
    let ctx = Context::create();
    let module = ctx.create_module("entry");
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    let int = ctx.i32_type();
    let ptr = ctx.ptr_type(AddressSpace::default());
    let main = module.add_function("main", int.fn_type(&[int.into(), ptr.into()], false), None);
    let callee = module.add_function(function, int.fn_type(&[], false), Some(Linkage::External));
    let builder = ctx.create_builder();
    builder.position_at_end(ctx.append_basic_block(main, "entry"));
    let code = builder.build_call(callee, &[], "code").map_err(|e| e.to_string())?;
    let code = code.try_as_basic_value().basic().ok_or("the entry function returns nothing")?;
    builder.build_return(Some(&code)).map_err(|e| e.to_string())?;
    module.verify().map_err(|e| e.to_string())?;
    tm.write_to_file(&module, FileType::Object, out).map_err(|e| e.to_string())
}

fn generate<R>(
    program: &Program,
    checked: &Checked,
    sources: &SourceMap,
    optimize: bool,
    target_triple: Option<&str>,
    mode: Mode,
    emit: impl FnOnce(&TargetMachine, &Module) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    // Code loaded into `tessel` (the REPL, hot reload, previews) may land
    // far from `tessel` itself: on Windows its references to the runtime
    // would then not fit in 32 bits, so every address is a full one there.
    let jit = !matches!(mode, Mode::Program | Mode::Debug);
    let (tm, triple, name) = target_machine(target_triple, optimize, jit).map_err(|e| vec![internal(e)])?;

    let ctx = Context::create();
    let module = ctx.create_module("tessel");
    module.set_triple(&triple);
    let td = tm.get_target_data();
    module.set_data_layout(&td.get_data_layout());

    let mut g = Gen::new(&ctx, module, td, program, checked, sources);
    g.android = name.contains("android");
    match mode {
        Mode::Program => {}
        Mode::Debug => g.debug = true,
        Mode::Repl(entry) => g.repl = Some(entry),
        Mode::Hot(generation) => g.hot = Some(generation),
        Mode::Design(views) => {
            g.hot = Some(1);
            g.design = Some(views);
        }
    }
    g.program();
    if !g.errors.is_empty() {
        return Err(g.errors);
    }
    g.drop_unused_copies();
    if let Err(e) = g.module.verify() {
        return Err(vec![internal(format!("generated code failed to verify: {}", e.to_string()))]);
    }
    // For working on the compiler: the LLVM code as it was written, before
    // it's optimized.
    if let Some(path) = std::env::var_os("TESSEL_EMIT_LLVM_RAW") {
        let _ = g.module.print_to_file(path);
    }
    if optimize {
        g.module
            .run_passes("default<O2>", &tm, PassBuilderOptions::create())
            .map_err(|e| vec![internal(e.to_string())])?;
    } else if g.has_coros {
        // Functions that `await` are written as coroutines, which only
        // these passes turn into code a machine can run.
        g.module
            .run_passes("coro-early,cgscc(coro-split),coro-cleanup,globaldce", &tm, PassBuilderOptions::create())
            .map_err(|e| vec![internal(e.to_string())])?;
    }
    // For working on the compiler: the (optimized) LLVM code, as text.
    if let Some(path) = std::env::var_os("TESSEL_EMIT_LLVM") {
        let _ = g.module.print_to_file(path);
    }
    emit(&tm, &g.module)
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
    /// A function declared in a view or app, used as a value (`action: save`):
    /// calling it calls the function.
    ViewFnRef(String),
    /// A function declared inside a function: like `Fn`, and its own name
    /// refers to the closure itself (so it can call itself).
    LocalFn(&'b FnDecl),
    /// Content built for a number (`LazyVStack`'s rows, `{ i in … }`):
    /// calling it with the number emits the views.
    Rows(&'b Ident, &'b Block),
    /// The same for a path (`Tree`'s rows, `{ path in … }`): called with an
    /// `[Int]` (which it takes).
    PathRows(&'b Ident, &'b Block),
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
    /// The caller's storage, given to a function's direct version for a
    /// `bind` parameter (see `binds_direct`): changed in place, not owned.
    Place(PointerValue<'ctx>),
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
    /// A function that can `await` (see `coro`).
    coro: Option<coro::Coro<'ctx>>,
    /// A function that tells the debugger where it is (see `debug`).
    dbg: Option<debug::Dbg<'ctx>>,
}

/// Where `continue` and `break` jump in a loop of function `func`; `depth`
/// is the number of scopes outside the loop body (whose variables stay).
struct LoopTarget<'ctx> {
    func: FunctionValue<'ctx>,
    next: BasicBlock<'ctx>,
    done: BasicBlock<'ctx>,
    depth: usize,
    /// How many values were held when the loop started (those stay).
    held: usize,
}

/// A value held while the rest of an expression is evaluated (see `held`).
struct Held<'ctx> {
    func: FunctionValue<'ctx>,
    ty: Ty,
    value: HeldValue<'ctx>,
}

enum HeldValue<'ctx> {
    Value(Val<'ctx>),
    /// A value being built in a slot (a list literal).
    Slot(PointerValue<'ctx>),
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
    /// Functions with `bind` parameters compiled a second time to take the
    /// caller's storage as a plain pointer (see `binds_direct`): the
    /// version a call uses when every `bind` argument stays put.
    direct_fns: HashMap<String, FunctionValue<'ctx>>,
    /// Set while a function's direct version is compiled.
    direct_now: bool,
    /// Set while the arguments of a call to a direct version are worked out.
    direct_args: bool,
    method_decls: HashMap<(String, String), (&'a FnDecl, Ty)>,
    methods: HashMap<(String, String), FunctionValue<'ctx>>,
    struct_decls: HashMap<usize, &'a StructDecl>,
    struct_types: HashMap<usize, StructType<'ctx>>,
    type_infos: HashMap<String, PointerValue<'ctx>>,
    helpers: HashMap<String, FunctionValue<'ctx>>,
    /// Big values (see `big`) that were just copied out of memory: where
    /// their copy is, and the statement it was made in (`era`). What only
    /// needs the value in memory (passing it on, storing it) uses the copy,
    /// and never touches the value field by field.
    origin: HashMap<Val<'ctx>, (PointerValue<'ctx>, u64)>,
    /// The copies' places, by function and type, with the statement each
    /// was last used in: a place is used again once that statement is over.
    copies: HashMap<(FunctionValue<'ctx>, String), Vec<(PointerValue<'ctx>, u64)>>,
    /// Counts statements: a copy is only trusted within the statement that
    /// made it.
    era: u64,
    strings: HashMap<String, PointerValue<'ctx>>,
    locs: HashMap<(u32, u32), PointerValue<'ctx>>,
    closures: usize,
    /// Loops being emitted, innermost last: where `break` and `continue` go.
    loops: Vec<LoopTarget<'ctx>>,
    /// Owned values computed but not yet used, while the rest of an
    /// expression is evaluated (`f(a(), if c { return 1 } else { 2 })`): an
    /// early `return`, `break` or `continue` in between releases them.
    held: Vec<Held<'ctx>>,
    /// The indexes of a place being reached (`a.b[i].c[j]`), worked out
    /// before the place is (see `place_indexes`): by the index's expression.
    pre_index: HashMap<crate::ast::ExprId, Val<'ctx>>,
    stack: Vec<FnCtx<'ctx>>,
    /// The awaited calls being evaluated, innermost last (see `coro`).
    awaits: Vec<coro::Awaited<'ctx>>,
    /// Whether any function compiled can `await`.
    has_coros: bool,
    /// Compiling an Android app: the system starts it, through the runtime
    /// (which calls `tsl_main`), so it has no `main`.
    android: bool,
    /// Compiling for the debugger (see `debug`), and the descriptions of
    /// types made for it so far.
    debug: bool,
    dbg_types: HashMap<String, PointerValue<'ctx>>,

    /// Views by name (the `app` included), and what's compiled for them.
    view_decls: HashMap<String, ViewSource<'a>>,
    view_builds: HashMap<String, FunctionValue<'ctx>>,
    view_fns: HashMap<(String, String), FunctionValue<'ctx>>,
    records: HashMap<String, (StructType<'ctx>, Vec<Ty>)>,
    app: Option<&'a AppDecl>,
    /// Compiling a REPL entry rather than a program.
    repl: Option<ReplEntry<'a>>,
    /// Compiling for hot reload: how many times the program was compiled.
    hot: Option<usize>,
    /// Compiling for `tessel preview`: the view expressions found so far.
    design: Option<&'a RefCell<crate::design::Views>>,
    /// For `design`: the content blocks being compiled, innermost last, and
    /// the views that have modifiers after them (the expression goes on).
    design_blocks: Vec<Span>,
    design_inner: HashSet<ExprId>,
    /// The body of the `view` (or `app`) being compiled.
    design_body: Option<&'a Block>,
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
            direct_fns: HashMap::new(),
            direct_now: false,
            direct_args: false,
            method_decls: HashMap::new(),
            methods: HashMap::new(),
            struct_decls: HashMap::new(),
            struct_types: HashMap::new(),
            type_infos: HashMap::new(),
            helpers: HashMap::new(),
            origin: HashMap::new(),
            copies: HashMap::new(),
            era: 0,
            strings: HashMap::new(),
            locs: HashMap::new(),
            closures: 0,
            loops: Vec::new(),
            held: Vec::new(),
            pre_index: HashMap::new(),
            stack: Vec::new(),
            awaits: Vec::new(),
            has_coros: false,
            android: false,
            debug: false,
            dbg_types: HashMap::new(),
            view_decls: HashMap::new(),
            view_builds: HashMap::new(),
            view_fns: HashMap::new(),
            records: HashMap::new(),
            app: None,
            repl: None,
            hot: None,
            design: None,
            design_blocks: Vec::new(),
            design_inner: HashSet::new(),
            design_body: None,
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
        let checked = self.checked;
        // The program's items, and the specialized copies of generic ones.
        let items: Vec<&'a Item> = program
            .items
            .iter()
            .filter(|i| !is_generic(i))
            .chain(checked.instances.iter().map(|r| &**r))
            .collect();
        for &item in &items {
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
                Item::Interface(_) => {}
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
        // A REPL entry's statements: compiled last, as the entry point.
        let repl_fn = self.repl.and_then(|_| self.fn_decls.remove(crate::REPL_FN));
        if !self.fn_decls.contains_key("main") && self.app.is_none() && self.repl.is_none() {
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
            let ty = self.fn_type(&param_lltys(&sig.params), &sig.ret, false);
            let f = self.module.add_function(&format!("tsl_f_{name}"), ty, Some(Linkage::Internal));
            let decl = self.fn_decls[&name];
            if self.binds_direct(decl, &sig) {
                let d = self.module.add_function(&format!("tsl_fd_{name}"), ty, Some(Linkage::Internal));
                self.direct_fns.insert(name.clone(), d);
            }
            self.fns.insert(name, f);
        }
        let method_keys: Vec<(String, String)> = self.method_decls.keys().cloned().collect();
        for key in method_keys {
            let (_, owner) = self.method_decls[&key].clone();
            let sig = self.method_sig(&owner, &key.1);
            let ty = self.fn_type(&param_lltys(&sig.params), &sig.ret, true);
            let f = self.module.add_function(&format!("tsl_m_{}_{}", key.0, key.1), ty, Some(Linkage::Internal));
            self.methods.insert(key, f);
        }

        self.declare_views();

        for (name, decl) in self.fn_decls.clone() {
            let f = self.fns[&name];
            let sig = self.checked.fns[&name].clone();
            if decl.constant {
                self.constant_body(f, decl, &sig.ret);
                continue;
            }
            let params: Vec<(Ident, Ty, bool)> =
                decl.params.iter().zip(&sig.params).map(|(p, s)| (p.name.clone(), s.ty.clone(), s.bind)).collect();
            self.function_body(f, &params, &decl.body, &sig.ret, None, 1, decl.is_async, Some((&name, decl.name.span)));
            if let Some(&d) = self.direct_fns.get(&name) {
                self.direct_now = true;
                self.function_body(d, &params, &decl.body, &sig.ret, None, 1, decl.is_async, Some((&name, decl.name.span)));
                self.direct_now = false;
            }
        }
        for (key, (decl, owner)) in self.method_decls.clone() {
            let f = self.methods[&key];
            let sig = self.method_sig(&owner, &key.1);
            let params: Vec<(Ident, Ty, bool)> =
                decl.params.iter().zip(&sig.params).map(|(p, s)| (p.name.clone(), s.ty.clone(), s.bind)).collect();
            let self_ptr = f.get_nth_param(1).unwrap().into_pointer_value();
            let shown = format!("{}.{}", key.0, key.1);
            self.function_body(f, &params, &decl.body, &sig.ret, Some((self_ptr, owner)), 2, decl.is_async, Some((&shown, decl.name.span)));
        }

        self.compile_views();

        if let Some(decl) = repl_fn {
            self.repl_entry(decl);
            return;
        }
        if self.repl.is_some() {
            return;
        }

        // Entry point called by the runtime.
        let entry_name = self.hot.map_or("tsl_main".to_string(), hot_entry_symbol);
        let entry = self.module.add_function(&entry_name, self.ctx.void_type().fn_type(&[], false), None);
        self.b.position_at_end(self.ctx.append_basic_block(entry, "entry"));
        self.set_app_info();
        // The program's constants, worked out before anything else runs, in
        // the order they're declared.
        self.stack.push(FnCtx {
            func: entry,
            scopes: vec![Vec::new()],
            ret: Ty::Void,
            self_ptr: None,
            boxed: HashSet::new(),
            record: None,
            coro: None,
            dbg: None,
        });
        for &item in &items {
            let Item::Fn(f) = item else { continue };
            if f.constant {
                let t = self.checked.fns[&f.name.name].ret.clone();
                if let Some(v) = self.constant(&f.name.name) {
                    self.release(v, &t);
                }
            }
        }
        let main_done = self.fn_decls.get("main").is_some_and(|d| d.is_async).then(|| self.nothing_block());
        self.stack.pop();
        match self.app {
            Some(app) => self.run_app(app, entry),
            None => {
                let null = self.ptr_ty().const_null();
                let mut args: Vec<BasicMetadataValueEnum> = vec![null.into()];
                // An `async fn main()`: nothing waits for it to finish.
                if let Some(done) = main_done {
                    args.push(done.into());
                }
                self.b.build_call(self.fns["main"], &args, "").unwrap();
                self.b.build_return(None).unwrap();
            }
        }
        if self.hot.is_some() || self.android {
            return;
        }
        // The process starts here: the runtime does the rest.
        let i32t = self.ctx.i32_type();
        let main = self.module.add_function("main", i32t.fn_type(&[], false), None);
        self.b.position_at_end(self.ctx.append_basic_block(main, "entry"));
        let status = self.call_rt("tsl_start", &[entry.as_global_value().as_pointer_value().into()]).unwrap();
        self.b.build_return(Some(&status)).unwrap();
    }

    /// Gives the runtime the app's information (`appInfo()`, the About
    /// panel): its `tessel.toml`'s, with defaults from the program.
    fn set_app_info(&mut self) {
        let info = crate::app_info();
        let name =
            info.name.clone().or_else(|| self.app.map(|a| a.name.name.clone())).or_else(|| info.program.clone()).unwrap_or_default();
        let version = info.version.clone().unwrap_or_else(|| "1.0".into());
        let fields = [
            info.id.clone().unwrap_or_else(|| crate::default_app_id(&name)),
            version.clone(),
            info.build.clone().unwrap_or(version),
            info.description.clone().unwrap_or_default(),
            info.author.clone().unwrap_or_default(),
            info.copyright.clone().unwrap_or_default(),
        ];
        let mut args: Vec<BasicMetadataValueEnum> = vec![self.str_lit(&name).into()];
        args.extend(fields.iter().map(|f| BasicMetadataValueEnum::from(self.str_lit(f))));
        let p = self.ptr_ty().into();
        let f = self.declare_rt("tsl_set_app_info", &[p; 7], None);
        self.b.build_call(f, &args, "").unwrap();
    }

    /// The function that runs a REPL entry's statements. The session's
    /// variables are globals of the code loaded for earlier entries, each
    /// holding a box with the value (so closures can share them, and entries
    /// can change them). The variables declared here are left in new globals.
    fn repl_entry(&mut self, decl: &'a FnDecl) {
        let repl = self.repl.expect("a REPL entry");
        let func = self.module.add_function(&repl_entry_symbol(repl.index), self.ctx.void_type().fn_type(&[], false), None);
        self.b.position_at_end(self.ctx.append_basic_block(func, "entry"));
        let sig = self.checked.fns[crate::REPL_FN].clone();
        let mut scope = Vec::new();
        for (v, p) in repl.vars.iter().zip(&sig.params) {
            let global = self.module.add_global(self.ptr_ty(), None, &v.symbol);
            scope.push(Var { name: v.name.clone(), ty: p.ty.clone(), store: Store::EnvBox(global.as_pointer_value()) });
        }
        let declared = self.checked.repl.clone().unwrap_or_default().vars;
        let mut boxed = self.boxed_vars(&decl.body);
        boxed.extend(declared.iter().map(|(name, ..)| name.clone()));
        self.stack.push(FnCtx { func, scopes: vec![scope], ret: Ty::Void, self_ptr: None, boxed, record: None, coro: None, dbg: None });
        self.push_scope();
        for s in &decl.body.stmts {
            if self.terminated() {
                self.continue_dead();
            }
            self.stmt(s);
        }
        if !self.terminated() {
            for (name, ..) in &declared {
                let var = self.stack.last().unwrap().scopes.last().unwrap().iter().rev().find(|v| v.name == *name);
                let Some(Var { store: Store::Boxed(slot), .. }) = var.cloned() else { continue };
                let bx = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                self.call_rt("tsl_retain", &[bx]);
                let global = self.module.add_global(self.ptr_ty(), None, &repl_var_symbol(repl.index, name));
                global.set_initializer(&self.ptr_ty().const_null());
                self.b.build_store(global.as_pointer_value(), bx).unwrap();
            }
        }
        self.pop_scope();
        self.finish_function(None);
        self.stack.pop();
    }

    /// A text for what a value of `t` is in memory, all the way down: the
    /// fields of structs, the cases of enums. `open` are the types being
    /// described (types can contain themselves, through lists).
    fn layout_text(&self, t: &Ty, open: &mut Vec<Ty>) -> String {
        let inner = |g: &Self, t: &Ty, open: &mut Vec<Ty>| g.layout_text(t, open);
        match t {
            Ty::List(x) => format!("[{}]", inner(self, x, open)),
            Ty::Set(x) => format!("Set<{}>", inner(self, x, open)),
            Ty::Opt(x) => format!("{}?", inner(self, x, open)),
            Ty::Dict(k, v) => format!("[{}: {}]", inner(self, k, open), inner(self, v, open)),
            Ty::Struct(_) | Ty::Enum(_) if open.contains(t) => self.checked.show(t),
            Ty::Struct(id) => {
                open.push(t.clone());
                let def = &self.checked.structs[*id];
                let fields: Vec<String> =
                    def.fields.iter().map(|f| format!("{}: {}", f.name, inner(self, &f.ty, open))).collect();
                open.pop();
                format!("{} {{ {} }}", def.name, fields.join(", "))
            }
            Ty::Enum(id) => {
                open.push(t.clone());
                let def = &self.checked.enums[*id];
                let cases: Vec<String> = def
                    .cases
                    .iter()
                    .map(|c| {
                        let values: Vec<String> = c.fields.iter().map(|f| inner(self, &f.ty, open)).collect();
                        format!("{}({})", c.name, values.join(", "))
                    })
                    .collect();
                open.pop();
                format!("{} {{ {} }}", def.name, cases.join(", "))
            }
            _ => self.checked.show(t),
        }
    }

    /// A number for what a view's state record holds (see the runtime's
    /// `tsl_ui_view_begin`): it changes when the record's values, or the
    /// types they're made of, do.
    fn record_layout(&self, view: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let (_, fields) = &self.records[view];
        let names = self.view_decls[view].body.stmts.iter().filter_map(|s| match s {
            Stmt::State(d) => Some(d.name.name.as_str()),
            _ => None,
        });
        let names = names.chain(self.view_decls[view].params.iter().map(|p| p.name.name.as_str()));
        let mut text = String::new();
        for (name, ty) in names.zip(fields) {
            text += &format!("{name}: {}; ", self.layout_text(ty, &mut Vec::new()));
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        h.finish()
    }

    fn method_sig(&self, owner: &Ty, name: &str) -> FnSig {
        let methods = match owner {
            Ty::Struct(id) => &self.checked.structs[*id].methods,
            Ty::Enum(id) => &self.checked.enums[*id].methods,
            _ => unreachable!(),
        };
        methods.iter().find(|m| m.name == name).unwrap().sig.clone()
    }

    /// The `var`s in `body` that closures, or the callees of `bind`
    /// arguments, share with it: they live in boxes.
    fn boxed_vars(&self, body: &Block) -> HashSet<String> {
        let mut boxed = boxed_vars(body);
        let mut shared = self.bind_roots(body);
        // A loop variable that's shared finds its item again through a
        // locator for the list, which shares the list's variable.
        walk_block(body, &mut |node| {
            if let Node::S(Stmt::For { var, iter, body, .. }) = node {
                if let (Some(root), true) = (root_name(iter), self.loop_var_shared(body, &var.name)) {
                    shared.insert(root.to_string());
                }
            }
        });
        let declared = declared_vars(body);
        boxed.extend(shared.into_iter().filter(|n| declared.contains(n)));
        boxed
    }

    /// Whether the loop variable `name` is used after it's looked up: by a
    /// closure, a `bind` argument, or (through it) a loop inside.
    fn loop_var_shared(&self, body: &Block, name: &str) -> bool {
        if captures(body, name) || self.bind_roots(body).contains(name) {
            return true;
        }
        let mut found = false;
        walk_block(body, &mut |node| {
            if let Node::S(Stmt::For { var, iter, body: inner, .. }) = node {
                found |= root_name(iter) == Some(name) && self.loop_var_shared(inner, &var.name);
            }
        });
        found
    }

    /// The variables that `bind` arguments in `body` start at (`todos` for
    /// `todos[0].title`).
    /// Whether a function can also take its `bind` parameters as plain
    /// pointers to the caller's storage: when it only reads and changes
    /// them, here and now (no closure keeps one, none is passed on as a
    /// `bind` or looped over in place), and isn't `async` (which would
    /// outlive the call). A call uses that version only when every `bind`
    /// argument stays where it is meanwhile (`stays_put`). Otherwise a
    /// `bind` is a locator, a closure called at each use, which finds the
    /// storage again however it moved: far slower in a tight loop.
    fn binds_direct(&self, decl: &FnDecl, sig: &FnSig) -> bool {
        let names: Vec<&str> =
            decl.params.iter().zip(&sig.params).filter(|(_, s)| s.bind).map(|(p, _)| p.name.name.as_str()).collect();
        if names.is_empty() || decl.is_async || decl.constant {
            return false;
        }
        let roots = self.bind_roots(&decl.body);
        let mut ok = names.iter().all(|n| !roots.contains(*n) && !captures(&decl.body, n));
        walk_block(&decl.body, &mut |node| {
            if let Node::S(Stmt::For { iter, .. }) = node {
                if root_name(iter).is_some_and(|r| names.contains(&r)) {
                    ok = false;
                }
            }
        });
        ok
    }

    /// Whether the storage `e` names can't move while a call runs: a
    /// variable (or a receiver's field), or a field of one, all the way
    /// down; not what a `bind` or a view's `state` reaches. With `items`, an
    /// item of a list there too: it moves only if the list grows, which the
    /// callee can't do when it has no other `bind` (as with a changing
    /// method called on an item, which has it as `self` the same way).
    fn stays_put(&self, e: &Expr, items: bool) -> bool {
        match &e.kind {
            ExprKind::Name(n) => self
                .lookup(n)
                .is_some_and(|v| matches!(v.store, Store::Slot(_) | Store::Boxed(_) | Store::EnvBox(_) | Store::Field(_) | Store::SelfRef)),
            ExprKind::Member { .. } if self.checked.calls.contains_key(&e.id) || self.page_size(e) => false,
            ExprKind::Member { object, optional: false, .. } => matches!(self.ty(object), Ty::Struct(_)) && self.stays_put(object, items),
            ExprKind::Index(object, _) => items && matches!(self.ty(object), Ty::List(_)) && self.stays_put(object, false),
            _ => false,
        }
    }

    fn bind_roots(&self, body: &Block) -> HashSet<String> {
        let mut roots = HashSet::new();
        walk_block(body, &mut |node| {
            let Node::E(e) = node else { return };
            if !self.checked.bind_args.contains(&e.id) {
                return;
            }
            if let Some(n) = root_name(e) {
                roots.insert(n.to_string());
            }
        });
        roots
    }

    fn function_body(
        &mut self,
        func: FunctionValue<'ctx>,
        params: &[(Ident, Ty, bool)],
        body: &Block,
        ret: &Ty,
        self_info: Option<(PointerValue<'ctx>, Ty)>,
        first_param: u32,
        is_async: bool,
        named: Option<(&str, Span)>,
    ) {
        let entry = self.ctx.append_basic_block(func, "entry");
        self.b.position_at_end(entry);
        let mut scope = Vec::new();
        if let Some((_, owner)) = &self_info {
            scope.extend(self.receiver_vars(owner));
        }
        let boxed = self.boxed_vars(body);
        // (What an `async` function returns goes to its completion block.)
        let ret = &match (is_async, params.last()) {
            (true, Some((_, Ty::Fn(ps, _), _))) => ps.first().cloned().unwrap_or(Ty::Void),
            _ => ret.clone(),
        };
        self.stack.push(FnCtx { func, scopes: vec![scope], ret: ret.clone(), self_ptr: self_info, boxed, record: None, coro: None, dbg: None });
        if is_async {
            self.begin_coro(&[]);
            self.own_receiver();
        }
        if let Some((name, span)) = named {
            self.debug_enter(name, span);
        }
        for (i, (name, ty, bind)) in params.iter().enumerate() {
            let raw = func.get_nth_param(first_param + i as u32).unwrap();
            let value = raw;
            if *bind && self.direct_now {
                // The caller's storage itself (see `binds_direct`): it's
                // read and changed in place, and isn't the function's to let go.
                let at = self.b.build_extract_value(value.into_struct_value(), 1, "").unwrap().into_pointer_value();
                self.push_var(Var { name: name.name.clone(), ty: ty.clone(), store: Store::Place(at) });
            } else if *bind {
                // A `bind` parameter: a locator for the caller's storage.
                let lt = self.llty(&Ty::View).unwrap();
                let slot = self.alloca(lt, &name.name);
                self.b.build_store(slot, value).unwrap();
                self.push_var(Var { name: name.name.clone(), ty: ty.clone(), store: Store::Binding { slot, owned: true } });
            } else {
                self.declare_param(&name.name, ty, value);
            }
        }
        if is_async {
            self.take_done();
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
        self.emit_return(value);
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
            Ty::Str | Ty::Data | Ty::List(_) | Ty::Dict(..) | Ty::Set(_) => self.ptr_ty().into(),
            // A view value is a closure that emits the view when called.
            Ty::View => self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false).into(),
            Ty::Range => self.ctx.struct_type(&[self.i64().into(), self.i64().into()], false).into(),
            Ty::Opt(inner) => {
                let inner = self.llty(inner).expect("optional of nothing");
                self.ctx.struct_type(&[self.ctx.bool_type().into(), inner], false).into()
            }
            Ty::Fn(..) => self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false).into(),
            // An interface value: the method table of its type, and a box
            // holding the value (shared: interface methods can't change it).
            Ty::Iface(_) => self.ctx.struct_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false).into(),
            Ty::Enum(id) => {
                if self.has_payload(*id) {
                    self.ctx.struct_type(&[self.ctx.i32_type().into(), self.ptr_ty().into()], false).into()
                } else {
                    self.ctx.i32_type().into()
                }
            }
            Ty::Struct(id) => self.struct_ty(*id).into(),
            Ty::Error | Ty::Any | Ty::Generic(_) => unreachable!("unresolved type in codegen"),
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
            ps.push(self.abi_type(p));
        }
        // A big result is written where the caller says: a last parameter.
        if self.big(ret) {
            ps.push(self.ptr_ty().into());
            return self.ctx.void_type().fn_type(&ps, false);
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
            Ty::Data => "d".into(),
            Ty::Void => "v".into(),
            Ty::View => "w".into(),
            Ty::Range => "r".into(),
            Ty::List(t) => format!("L{}", self.key(t)),
            Ty::Dict(k, v) => format!("D{}{}", self.key(k), self.key(v)),
            Ty::Set(t) => format!("Z{}", self.key(t)),
            Ty::Opt(t) => format!("O{}", self.key(t)),
            Ty::Fn(ps, r) => format!("F{}_{}", ps.iter().map(|p| self.key(p)).collect::<String>(), self.key(r)),
            Ty::Struct(id) => format!("S{}{}", self.checked.structs[*id].name.len(), self.checked.structs[*id].name),
            Ty::Enum(id) => format!("E{}{}", self.checked.enums[*id].name.len(), self.checked.enums[*id].name),
            Ty::Iface(id) => format!("I{}{}", self.checked.ifaces[*id].name.len(), self.checked.ifaces[*id].name),
            Ty::Error | Ty::Any | Ty::Generic(_) => "x".into(),
        }
    }

    fn needs_rc(&self, t: &Ty) -> bool {
        match t {
            Ty::Str | Ty::Data | Ty::List(_) | Ty::Dict(..) | Ty::Set(_) | Ty::Fn(..) | Ty::View | Ty::Iface(_) => true,
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
    fn alloca(&mut self, t: BasicTypeEnum<'ctx>, name: &str) -> PointerValue<'ctx> {
        let entry = self.func().get_first_basic_block().unwrap();
        let tmp = self.ctx.create_builder();
        match entry.get_first_instruction() {
            Some(first) => tmp.position_before(&first),
            None => tmp.position_at_end(entry),
        }
        let p = tmp.build_alloca(t, name).unwrap();
        // A large struct stays in memory, where it's copied as a block. Left
        // to itself the optimizer takes such a variable apart into its
        // fields, and every copy of it becomes a store for each field (see
        // `big`). Handing its address to a function the optimizer doesn't
        // look into (`pin_helper`) keeps it whole.
        if t.is_struct_type() && self.td.get_store_size(&t) > BIG_VALUE {
            let pin = self.pin_helper();
            match p.as_instruction().and_then(|i| i.get_next_instruction()) {
                Some(next) => tmp.position_before(&next),
                None => tmp.position_at_end(entry),
            }
            tmp.build_call(pin, &[p.into()], "").unwrap();
        }
        p
    }

    /// `void pin(ptr)`: does nothing (see `alloca`).
    fn pin_helper(&mut self) -> FunctionValue<'ctx> {
        let name = "tsl_h_pin";
        if let Some(f) = self.helpers.get(name) {
            return *f;
        }
        let f = self.module.add_function(
            name,
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        self.helpers.insert(name.to_string(), f);
        for attribute in ["noinline", "optnone"] {
            let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(attribute);
            f.add_attribute(inkwell::attributes::AttributeLoc::Function, self.ctx.create_enum_attribute(kind, 0));
        }
        let body = self.ctx.append_basic_block(f, "entry");
        let b = self.ctx.create_builder();
        b.position_at_end(body);
        b.build_return(None).unwrap();
        f
    }

    /// Whether values of a type are moved as a block of memory: structs
    /// (and optional ones) of more than a few fields. Taken apart into their
    /// fields, each use of one would cost an instruction per field, and a
    /// program's size would grow with the size of its structs.
    fn big(&mut self, t: &Ty) -> bool {
        match t {
            Ty::Struct(_) | Ty::Opt(_) => {
                let lt = self.llty(t).unwrap();
                self.td.get_store_size(&lt) > BIG_VALUE
            }
            _ => false,
        }
    }

    /// Copies a value of LLVM type `lt` from `from` to `to`.
    fn copy_bytes(&mut self, lt: BasicTypeEnum<'ctx>, to: PointerValue<'ctx>, from: PointerValue<'ctx>) {
        let size = self.i64().const_int(self.td.get_store_size(&lt), false);
        let align = self.td.get_abi_alignment(&lt);
        self.b.build_memcpy(to, align, from, align, size).unwrap();
    }

    /// Takes out the reads of big values that nothing used: most of them,
    /// since what uses a big value usually goes by its copy in memory (see
    /// `origin`). Left in, each would be followed field by field through
    /// the optimizer's first passes, which costs a large program gigabytes
    /// of memory.
    fn drop_unused_copies(&mut self) {
        for v in self.origin.keys() {
            let Some(read) = inkwell::values::BasicValue::as_instruction_value(v) else { continue };
            if read.get_first_use().is_none() && read.get_parent().is_some() {
                read.erase_from_basic_block();
            }
        }
        self.origin.clear();
    }

    /// Where a big value's copy in memory is, if it was made in this statement.
    fn origin_of(&self, v: Val<'ctx>) -> Option<PointerValue<'ctx>> {
        match self.origin.get(&v) {
            Some((p, era)) if *era == self.era => Some(*p),
            _ => None,
        }
    }

    /// A place for a copy of a big value, in this function: one that no
    /// value of this statement is in.
    fn copy_slot(&mut self, t: &Ty, lt: BasicTypeEnum<'ctx>) -> PointerValue<'ctx> {
        let key = (self.func(), self.key(t));
        let era = self.era;
        if let Some(slots) = self.copies.get_mut(&key) {
            if let Some(slot) = slots.iter_mut().find(|s| s.1 != era) {
                slot.1 = era;
                return slot.0;
            }
        }
        let p = self.alloca(lt, "copy");
        self.copies.entry(key).or_default().push((p, era));
        p
    }

    fn load(&mut self, t: &Ty, p: PointerValue<'ctx>) -> Val<'ctx> {
        let lt = self.llty(t).unwrap();
        if self.big(t) {
            // A copy of the whole value at once; what uses it as a whole
            // (see `origin`) never loads its fields.
            let copy = self.copy_slot(t, lt);
            self.copy_bytes(lt, copy, p);
            let v = self.b.build_load(lt, copy, "").unwrap();
            self.origin.insert(v, (copy, self.era));
            return v;
        }
        self.b.build_load(lt, p, "").unwrap()
    }

    /// A value in memory, to read it from there.
    fn spill(&mut self, t: &Ty, v: Val<'ctx>) -> PointerValue<'ctx> {
        if let Some(p) = self.origin_of(v) {
            return p;
        }
        let lt = self.llty(t).unwrap();
        let p = self.alloca(lt, "tmp");
        self.b.build_store(p, v).unwrap();
        p
    }

    /// Stores a value: a big one that's in memory already is copied from there.
    fn store(&mut self, t: &Ty, to: PointerValue<'ctx>, v: Val<'ctx>) {
        if let Some(from) = self.origin_of(v) {
            let lt = self.llty(t).unwrap();
            self.copy_bytes(lt, to, from);
        } else {
            self.b.build_store(to, v).unwrap();
        }
    }

    /// A function's parameter types, as they're passed: big values by a
    /// pointer to them (the caller's copy, which the function takes over).
    fn abi_type(&mut self, t: &Ty) -> BasicMetadataTypeEnum<'ctx> {
        if self.big(t) { self.ptr_ty().into() } else { self.llty(t).unwrap().into() }
    }

    /// An argument as it's passed (see `abi_type`).
    fn abi_arg(&mut self, v: Val<'ctx>, t: &Ty) -> BasicMetadataValueEnum<'ctx> {
        if self.big(t) { self.spill(t, v).into() } else { v.into() }
    }

    fn abi_args(&mut self, vals: &[Val<'ctx>], tys: &[Ty]) -> Vec<BasicMetadataValueEnum<'ctx>> {
        vals.iter().zip(tys).map(|(v, t)| self.abi_arg(*v, t)).collect()
    }

    /// A parameter as a value, from how it was passed (see `abi_type`).
    fn abi_param(&mut self, raw: Val<'ctx>, t: &Ty) -> Val<'ctx> {
        if !self.big(t) {
            return raw;
        }
        let lt = self.llty(t).unwrap();
        let p = raw.into_pointer_value();
        let v = self.b.build_load(lt, p, "").unwrap();
        self.origin.insert(v, (p, self.era));
        v
    }

    /// The value in a copy's place, known to be there (see `origin`).
    fn value_at(&mut self, t: &Ty, p: PointerValue<'ctx>) -> Val<'ctx> {
        let lt = self.llty(t).unwrap();
        let v = self.b.build_load(lt, p, "").unwrap();
        self.origin.insert(v, (p, self.era));
        v
    }

    /// For a call of a function whose result is big: the place for it,
    /// added to the arguments (see `fn_type`).
    fn result_place(&mut self, ret: &Ty, args: &mut Vec<BasicMetadataValueEnum<'ctx>>) -> Option<PointerValue<'ctx>> {
        if !self.big(ret) {
            return None;
        }
        let lt = self.llty(ret).unwrap();
        let place = self.copy_slot(ret, lt);
        args.push(place.into());
        Some(place)
    }

    /// What a call gave: its result, or the value in the place it was given.
    fn call_result(&mut self, ret: &Ty, place: Option<PointerValue<'ctx>>, result: Option<Val<'ctx>>) -> Option<Val<'ctx>> {
        match place {
            Some(p) => Some(self.value_at(ret, p)),
            None => result,
        }
    }

    /// Returns from the function being written: a big value goes to the
    /// place the caller gave (see `fn_type`).
    fn emit_return(&mut self, v: Option<Val<'ctx>>) {
        if let Some(c) = self.stack.last().unwrap().coro.clone() {
            return self.finish_coro(&c, v);
        }
        let ret = self.stack.last().unwrap().ret.clone();
        self.debug_leave();
        if self.big(&ret) {
            if let Some(v) = v {
                let out = self.func().get_last_param().unwrap().into_pointer_value();
                self.store(&ret, out, v);
            }
            self.b.build_return(None).unwrap();
            return;
        }
        match v {
            Some(v) => self.b.build_return(Some(&v)).unwrap(),
            None => self.b.build_return(None).unwrap(),
        };
    }

    fn undef(&mut self, t: &Ty) -> Option<Val<'ctx>> {
        self.llty(t).map(undef_of)
    }

    fn call_rt(&mut self, name: &str, args: &[Val<'ctx>]) -> Option<Val<'ctx>> {
        // The list operations inside loops are emitted inline (see `list_*_inline`).
        match name {
            "tsl_list_len" => return Some(self.list_len_inline(args[0].into_pointer_value()).into()),
            "tsl_list_get" => return Some(self.list_get_inline(args).into()),
            "tsl_list_elem_mut" => return Some(self.list_elem_mut_inline(args).into()),
            _ => {}
        }
        let f = self.rt(name);
        let args: Vec<BasicMetadataValueEnum> = args.iter().map(|a| (*a).into()).collect();
        self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic()
    }

    // ===== lists, inline =====
    //
    // A list is `{ rc: i64, drop: ptr, len: i64, cap: i64, elem: ptr, data: ptr }`
    // (the runtime's `List`), where `elem` is the element type's `TslType`,
    // which starts with its size. Reading the length and addressing an
    // element are a few instructions, so they're emitted here instead of
    // calling the runtime; only errors and copying a shared list call it.

    fn list_field(&mut self, l: PointerValue<'ctx>, offset: u64, ty: BasicTypeEnum<'ctx>) -> BasicValueEnum<'ctx> {
        let p = unsafe { self.b.build_gep(self.i8(), l, &[self.i64().const_int(offset, false)], "").unwrap() };
        self.b.build_load(ty, p, "").unwrap()
    }

    fn list_len_inline(&mut self, l: PointerValue<'ctx>) -> IntValue<'ctx> {
        let i = self.i64().into();
        self.list_field(l, 16, i).into_int_value()
    }

    /// Stops with the runtime's out-of-range error unless `i` is below `len`.
    fn list_bounds(&mut self, i: IntValue<'ctx>, len: IntValue<'ctx>, fail: impl FnOnce(&mut Self)) {
        // Unsigned, so a negative index is out of range too.
        let ok = self.b.build_int_compare(IntPredicate::ULT, i, len, "inbounds").unwrap();
        let (good, bad) = (self.block_named("inbounds"), self.block_named("outofbounds"));
        let expect = self.module.get_function("llvm.expect.i1").unwrap_or_else(|| {
            let b = self.ctx.bool_type();
            self.module.add_function("llvm.expect.i1", b.fn_type(&[b.into(), b.into()], false), None)
        });
        let likely = self.b.build_call(expect, &[ok.into(), self.ctx.bool_type().const_int(1, false).into()], "").unwrap();
        let likely = likely.try_as_basic_value().basic().unwrap().into_int_value();
        self.b.build_conditional_branch(likely, good, bad).unwrap();
        self.b.position_at_end(bad);
        fail(self);
        self.b.build_unreachable().unwrap();
        self.b.position_at_end(good);
    }

    /// The address of element `i` of list `l` (already checked).
    fn list_elem_addr(&mut self, l: PointerValue<'ctx>, i: IntValue<'ctx>) -> PointerValue<'ctx> {
        let p = self.ptr_ty().into();
        let data = self.list_field(l, 40, p).into_pointer_value();
        let elem = self.list_field(l, 32, p).into_pointer_value();
        let size = self.b.build_load(self.i64(), elem, "size").unwrap().into_int_value();
        let offset = self.b.build_int_mul(i, size, "").unwrap();
        unsafe { self.b.build_gep(self.i8(), data, &[offset], "elem").unwrap() }
    }

    /// `tsl_list_get(list, i, loc)`: element `i`, for reading.
    fn list_get_inline(&mut self, args: &[Val<'ctx>]) -> PointerValue<'ctx> {
        let l = args[0].into_pointer_value();
        let i = args[1].into_int_value();
        let len = self.list_len_inline(l);
        self.list_bounds(i, len, |g| {
            let f = g.rt("tsl_list_get");
            g.b.build_call(f, &[args[0].into(), args[1].into(), args[2].into()], "").unwrap();
        });
        self.list_elem_addr(l, i)
    }

    /// The reference count of the object at `obj` (read atomically: another
    /// thread may be changing a shared one's, see the runtime's `SHARED`).
    fn load_count(&mut self, obj: PointerValue<'ctx>) -> IntValue<'ctx> {
        let rc = self.b.build_load(self.i64(), obj, "rc").unwrap();
        rc.into_int_value().as_instruction().unwrap().set_atomic_ordering(inkwell::AtomicOrdering::Monotonic).unwrap();
        rc.into_int_value()
    }

    /// Appends `v` (of type `elem`, moved in) to the list in `slot`. When
    /// the list isn't shared and has room, it's stored in place; otherwise
    /// the runtime grows (or copies) the list first.
    pub(super) fn push_item(&mut self, slot: PointerValue<'ctx>, v: Val<'ctx>, elem: &Ty) {
        let (i, p) = (self.i64().into(), self.ptr_ty().into());
        let l = self.b.build_load(self.ptr_ty(), slot, "list").unwrap().into_pointer_value();
        let rc = self.load_count(l);
        let len = self.list_len_inline(l);
        let cap = self.list_field(l, 24, i).into_int_value();
        let unique = self.b.build_int_compare(IntPredicate::EQ, rc, self.i64().const_int(1, false), "").unwrap();
        let room = self.b.build_int_compare(IntPredicate::SLT, len, cap, "").unwrap();
        let fast = self.b.build_and(unique, room, "").unwrap();
        let (fast_bb, slow_bb, done) = (self.block_named("push"), self.block_named("grow"), self.block_named("pushed"));
        self.b.build_conditional_branch(fast, fast_bb, slow_bb).unwrap();
        self.b.position_at_end(fast_bb);
        let data = self.list_field(l, 40, p).into_pointer_value();
        let lt = self.llty(elem).unwrap();
        let dst = unsafe { self.b.build_gep(lt, data, &[len], "").unwrap() };
        self.store(elem, dst, v);
        let new_len = self.b.build_int_add(len, self.i64().const_int(1, false), "").unwrap();
        let len_p = unsafe { self.b.build_gep(self.i8(), l, &[self.i64().const_int(16, false)], "").unwrap() };
        self.b.build_store(len_p, new_len).unwrap();
        self.br(done);
        self.b.position_at_end(slow_bb);
        let tmp = self.spill(elem, v);
        let f = self.rt("tsl_list_push");
        self.b.build_call(f, &[slot.into(), tmp.into()], "").unwrap();
        self.br(done);
        self.b.position_at_end(done);
    }

    /// `tsl_list_elem_mut(slot, i, loc)`: element `i` of the list in `slot`,
    /// for writing (the list is copied first if it's shared).
    fn list_elem_mut_inline(&mut self, args: &[Val<'ctx>]) -> PointerValue<'ctx> {
        let slot = args[0].into_pointer_value();
        let i = args[1].into_int_value();
        let l = self.b.build_load(self.ptr_ty(), slot, "list").unwrap().into_pointer_value();
        let len = self.list_len_inline(l);
        self.list_bounds(i, len, |g| {
            let f = g.rt("tsl_list_elem_mut");
            g.b.build_call(f, &[args[0].into(), args[1].into(), args[2].into()], "").unwrap();
        });
        let rc = self.load_count(l);
        let unique = self.b.build_int_compare(IntPredicate::EQ, rc, self.i64().const_int(1, false), "").unwrap();
        let here = self.b.get_insert_block().unwrap();
        let (copy, done) = (self.block_named("copy"), self.block_named("unique"));
        self.b.build_conditional_branch(unique, done, copy).unwrap();
        self.b.position_at_end(copy);
        let f = self.rt("tsl_list_unique");
        let copied = self.b.build_call(f, &[slot.into()], "").unwrap().try_as_basic_value().basic().unwrap();
        let copy_end = self.b.get_insert_block().unwrap();
        self.br(done);
        self.b.position_at_end(done);
        let phi = self.b.build_phi(self.ptr_ty(), "").unwrap();
        phi.add_incoming(&[(&l, here), (&copied, copy_end)]);
        let l = phi.as_basic_value().into_pointer_value();
        self.list_elem_addr(l, i)
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
            "tsl_box_unique" => (vec![p], Some(p)),
            "tsl_iface_eq" => (vec![p, p, p, p], Some(i8t)),
            "tsl_panic" => (vec![p, p], None),
            "tsl_str_concat" => (vec![p, i], Some(p)),
            "tsl_str_from_int" => (vec![i], Some(p)),
            "tsl_str_from_float" => (vec![f64t], Some(p)),
            "tsl_str_from_bool" => (vec![i8t], Some(p)),
            "tsl_data_eq" => (vec![p, p], Some(i8t)),
            "tsl_data_concat" => (vec![p, p], Some(p)),
            "tsl_data_append" => (vec![p, p], None),
            "tsl_str_append" => (vec![p, p], None),
            "tsl_data_append_byte" => (vec![p, i], None),
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
            "tsl_list_reversed" | "tsl_dict_keys" | "tsl_dict_values" | "tsl_str_quoted" => (vec![p], Some(p)),
            "tsl_list_sorted" => (vec![p, p, p], Some(p)),
            "tsl_dict_new" | "tsl_set_from_list" => (vec![p, p], Some(p)),
            "tsl_dict_get" | "tsl_dict_remove" => (vec![p, p, p], Some(i8t)),
            "tsl_dict_set" => (vec![p, p, p], None),
            "tsl_dict_value_slot" => (vec![p, p], Some(p)),
            "tsl_after" => (vec![f64t, p, p], None),
            "tsl_read_file" | "tsl_list_folder" | "tsl_environment" | "tsl_str_lines" => (vec![p], Some(p)),
            "tsl_write_file" => (vec![p, p], Some(i8t)),
            "tsl_file_exists" | "tsl_is_folder" | "tsl_delete_file" => (vec![p], Some(i8t)),
            "tsl_current_folder" => (vec![], Some(p)),
            "tsl_run_command" => (vec![p, p, i8t, p, p, p, p], Some(i)),
            "tsl_stop_command" => (vec![i], None),
            "tsl_str_substring" => (vec![p, i, i], Some(p)),
            "tsl_str_find" => (vec![p, p], Some(i)),
            "tsl_ui_shortcut" => (vec![p, p, p], None),
            "tsl_ui_code_editor" => (vec![p, p, p, p, p, p, i, p, p, p, i, p, p], None),
            "tsl_ui_on_appear" | "tsl_ui_on_submit" => (vec![p, p], None),
            "tsl_ui_begin_tabs" => (vec![p, p, p, p], None),
            "tsl_ui_divider" | "tsl_ui_autofocus" => (vec![], None),
            "tsl_ui_icon" => (vec![i32t], None),
            "tsl_ui_begin_ribbon" => (vec![p, p], None),
            "tsl_ui_begin_ribbon_tab" => (vec![p], None),
            "tsl_ui_begin_ribbon_group" => (vec![p, i32t], None),
            "tsl_ui_ribbon_button" | "tsl_ui_ribbon_toggle" => (vec![p, i32t, i32t, p, p], None),
            "tsl_ui_begin_ribbon_menu" => (vec![p, i32t, i32t], None),
            "tsl_ui_svg" => (vec![p], None),
            "tsl_ui_markdown" => (vec![p, p, p, p], None),
            "tsl_ui_tab_item" => (vec![p], None),
            "tsl_ui_hover_background" => (vec![i32t, p, f64t], None),
            "tsl_create_folder" => (vec![p], Some(i8t)),
            "tsl_rename_file" => (vec![p, p], Some(i8t)),
            "tsl_fetch" => (vec![p, p, p], None),
            "tsl_ui_text" | "tsl_ui_image" => (vec![p], None),
            "tsl_ui_button" | "tsl_ui_toggle" => (vec![p, p, p], None),
            "tsl_ui_text_field" => (vec![p, i8t, i, p, p], None),
            "tsl_ui_spacer" | "tsl_ui_end" | "tsl_ui_view_end" => (vec![], None),
            "tsl_ui_begin" => (vec![i32t, f64t, i32t], None),
            "tsl_ui_on_tap" | "tsl_ui_on_drag" | "tsl_ui_tag" => (vec![p, p], None),
            "tsl_ui_begin_picker" => (vec![i32t, p, p], None),
            "tsl_ui_begin_grid" => (vec![i32t, f64t, f64t], None),
            "tsl_ui_begin_grid_row" => (vec![], None),
            "tsl_ui_rich_editor" => (vec![i, p, p, p], None),
            "tsl_ui_web_view" => (vec![p, p], None),
            "tsl_ui_video" => (vec![p, p, p, p, p, i8t, i8t, f64t], None),
            "tsl_ui_grid_cell_columns" | "tsl_ui_line_limit" => (vec![i], None),
            "tsl_ui_grid_column_alignment" => (vec![i32t], None),
            "tsl_ui_selectable" => (vec![i8t], None),
            "tsl_ui_rotation" => (vec![f64t], None),
            "tsl_ui_begin_tray" => (vec![], None),
            "tsl_ui_end_tray" => (vec![i32t, p, i8t], None),
            "tsl_ui_scale" => (vec![f64t, f64t, f64t], None),
            "tsl_ui_on_pointer" | "tsl_ui_on_key" => (vec![p, p], None),
            "tsl_ui_on_key_event" => (vec![p, p, i8t], None),
            "tsl_ui_slider" => (vec![p, p, f64t, f64t, f64t, p, p], None),
            "tsl_ui_stepper" => (vec![p, p, i8t, f64t, f64t, f64t, i, p, p], None),
            "tsl_ui_color_picker" | "tsl_ui_drop_target" => (vec![p, p, p, p], None),
            "tsl_ui_begin_disclosure" => (vec![p, p, p], Some(i8t)),
            "tsl_ui_lazy" | "tsl_ui_lazy_across" => (vec![i, f64t, p, p], None),
            "tsl_ui_table_begin" => (vec![], None),
            "tsl_ui_table_column" => (vec![p, i8t, f64t, self.ctx.i32_type().into(), p, p, p, p], None),
            "tsl_ui_table_end" => (vec![i, f64t, p, p, p, p, p, p, p, p, i8t, p, p], None),
            "tsl_ui_tree" => (vec![p, p, f64t, p, p, p, p, p, p], None),
            "tsl_ui_draggable" | "tsl_ui_drag_file" => (vec![p], None),
            "tsl_ui_cursor" => (vec![i32t], None),
            "tsl_ui_offset" => (vec![f64t, f64t], None),
            "tsl_ui_clipped" => (vec![i8t], None),
            "tsl_ui_on_drop" | "tsl_ui_on_resize" => (vec![p, p], None),
            "tsl_ui_tooltip" | "tsl_ui_accessibility_label" => (vec![p], None),
            "tsl_ui_menu_item" => (vec![p, p, i8t, p, p], None),
            "tsl_ui_menu_divider" | "tsl_ui_end_popover" | "tsl_ui_end_window" => (vec![], None),
            "tsl_ui_begin_menu" => (vec![p], None),
            "tsl_ui_begin_attached" | "tsl_ui_end_attached" => (vec![i32t], None),
            "tsl_ui_begin_popover" => (vec![p, p, i32t], Some(i8t)),
            "tsl_ui_begin_window" => (vec![p, p, p, f64t, f64t], Some(i8t)),
            "tsl_ui_on_open_file" | "tsl_ui_on_close_request" => (vec![p, p], None),
            "tsl_ui_canvas" | "tsl_ui_scene3d" => (vec![p, p, i8t, i], None),
            "tsl_ui_view_begin" => (vec![i, p, i, p], Some(p)),
            "tsl_ui_opacity" => (vec![f64t], None),
            "tsl_ui_animation" => (vec![f64t, i32t], None),
            "tsl_ui_padding" => (vec![f64t, f64t, f64t, f64t], None),
            "tsl_ui_font" => (vec![f64t, i8t, i32t, i8t, i8t, i8t, p], None),
            "tsl_ui_text_alignment" => (vec![i32t], None),
            "tsl_math" => (vec![i32t, f64t, f64t], Some(f64t)),
            "tsl_float_format" => (vec![f64t, i], Some(p)),
            "tsl_file_dialog" => (vec![i8t, p, p, p], Some(p)),
            "tsl_ui_color" => (vec![i32t, p], None),
            "tsl_ui_background" => (vec![i32t, p, f64t], None),
            "tsl_ui_border" => (vec![i32t, p, f64t, f64t], None),
            "tsl_ui_frame" => (vec![i8t, f64t, i8t, f64t, i8t, f64t, i8t, f64t, i8t, f64t, i8t, f64t, i8t, i32t], None),
            "tsl_ui_hidden" | "tsl_ui_disabled" => (vec![i8t], None),
            "tsl_ui_source" => (vec![i32t], None),
            "tsl_ui_next_id" => (vec![i], None),
            "tsl_scroll_to" => (vec![i, i32t], None),
            "tsl_ui_begin_split" => (vec![i8t, p, p], None),
            "tsl_ui_run" => (vec![p, f64t, f64t, i32t, p, p], None),
            "tsl_start" => (vec![p], Some(i32t)),
            "tsl_ui_hot" => (vec![p, f64t, f64t, i32t, p, p], None),
            "tsl_list_unique" => (vec![p], Some(p)),
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
            Ty::Str | Ty::Data | Ty::List(_) | Ty::Dict(..) | Ty::Set(_) => {
                self.call_rt(f, &[v]);
            }
            Ty::Fn(..) | Ty::Enum(_) | Ty::View | Ty::Iface(_) => {
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

    /// Keeps a big struct's routine a function of its own. Written into
    /// each place that calls it, it would take the struct apart field by
    /// field there, and the optimizer would then do the same to every copy
    /// of the struct around it: the size this is all meant to avoid.
    fn keep_out_of_line(&self, f: FunctionValue<'ctx>) {
        let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("noinline");
        f.add_attribute(inkwell::attributes::AttributeLoc::Function, self.ctx.create_enum_attribute(kind, 0));
    }

    /// `void helper(ptr)`: gives the fields `which` of the struct at `ptr`
    /// their default values. One for each set of fields a program leaves
    /// to their defaults (`Big()` and `Big(x: 1)` are two).
    fn defaults_helper(&mut self, id: usize, which: &[usize]) -> FunctionValue<'ctx> {
        let t = Ty::Struct(id);
        let all = which.len() == self.checked.structs[id].fields.len();
        let set = if all { "all".to_string() } else { which.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("_") };
        let name = format!("tsl_h_defaults_{}_{set}", self.key(&t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let f = self.module.add_function(
            &name,
            self.ctx.void_type().fn_type(&[self.ptr_ty().into()], false),
            Some(Linkage::Internal),
        );
        self.helpers.insert(name, f);
        self.keep_out_of_line(f);
        let st = self.struct_ty(id);
        let fields = self.checked.structs[id].fields.clone();
        let decl = self.struct_decls.get(&id).copied().expect("a struct with defaults has a declaration");
        let which = which.to_vec();
        self.in_helper(f, |g| {
            let p = f.get_nth_param(0).unwrap().into_pointer_value();
            for i in which {
                let default = decl.fields[i].default.as_ref().expect("checked: a field left out has a default");
                let v = g.expr_as(default, &fields[i].ty).unwrap();
                let fp = g.b.build_struct_gep(st, p, i as u32, "").unwrap();
                g.store(&fields[i].ty, fp, v);
            }
            g.b.build_return(None).unwrap();
        });
        f
    }

    /// Releases the value at `p`: a big one where it is, without a copy.
    fn release_at(&mut self, t: &Ty, p: PointerValue<'ctx>) {
        if self.big(t) {
            let helper = self.rc_helper(t, false);
            self.b.build_call(helper, &[p.into()], "").unwrap();
        } else {
            let v = self.load(t, p);
            self.release(v, t);
        }
    }

    /// A parameter as a variable. A big one stays where the caller put it
    /// (see `abi_type`): the place is this function's now, and parameters
    /// don't change.
    fn declare_param(&mut self, name: &str, ty: &Ty, raw: Val<'ctx>) {
        let boxed = self.stack.last().unwrap().boxed.contains(name);
        // (A function that can `await` outlives the caller's place: it
        // keeps a copy of its own.)
        let waits = self.stack.last().unwrap().coro.is_some();
        if self.big(ty) && !boxed && !waits && name != "_" {
            self.push_var(Var { name: name.to_string(), ty: ty.clone(), store: Store::Slot(raw.into_pointer_value()) });
            return;
        }
        let v = self.abi_param(raw, ty);
        self.declare_value(name, ty, Some(v));
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
        if self.big(t) {
            self.keep_out_of_line(f);
        }
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
            coro: None,
            dbg: None,
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
        let ty = self.ctx.struct_type(&[i.into(), i.into(), p.into(), p.into(), p.into(), p.into(), p.into(), i.into()], false);
        // What the runtime can hash and compare itself (see `TslType::kind`).
        let kind = match (&t, payload) {
            (Ty::Int, None) => 1,
            (Ty::Str, None) => 2,
            _ => 0,
        };
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
            i.const_int(kind, false).into(),
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
            let h = g.hash_value(v, t);
            g.b.build_return(Some(&h)).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// The hash of a (borrowed) dictionary key: a number, text, a `Bool`,
    /// an enum without values, or a struct (a tuple) of those.
    fn hash_value(&mut self, v: Val<'ctx>, t: &Ty) -> IntValue<'ctx> {
        match t {
            Ty::Str => self.call_rt("tsl_str_hash", &[v]).unwrap().into_int_value(),
            Ty::Int => v.into_int_value(),
            Ty::Struct(id) => {
                let fields: Vec<Ty> = self.checked.structs[*id].fields.iter().map(|f| f.ty.clone()).collect();
                let s = v.into_struct_value();
                let mut h = self.i64().const_int(0x9E37_79B9, false);
                for (i, ft) in fields.iter().enumerate() {
                    let fv = self.b.build_extract_value(s, i as u32, "").unwrap();
                    let fh = self.hash_value(fv, ft);
                    let m = self.b.build_int_mul(h, self.i64().const_int(0x100_0000_01B3, false), "").unwrap();
                    h = self.b.build_xor(m, fh, "").unwrap();
                }
                h
            }
            _ => self.b.build_int_z_extend(v.into_int_value(), self.i64(), "").unwrap(),
        }
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
            Ty::Data => {
                let r = self.call_rt("tsl_data_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::List(_) => {
                let r = self.call_rt("tsl_list_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::Dict(..) | Ty::Set(_) => {
                let r = self.call_rt("tsl_dict_eq", &[a, b]);
                self.truthy(r)
            }
            Ty::Enum(id) if !self.has_payload(*id) => {
                self.b.build_int_compare(IntPredicate::EQ, a.into_int_value(), b.into_int_value(), "").unwrap()
            }
            Ty::Enum(_) => {
                // The same case, and then its values: two cases' values are
                // laid out differently, so they're compared only within one.
                let (sa, sb) = (a.into_struct_value(), b.into_struct_value());
                let ta = self.b.build_extract_value(sa, 0, "").unwrap().into_int_value();
                let tb = self.b.build_extract_value(sb, 0, "").unwrap().into_int_value();
                let same_tag = self.b.build_int_compare(IntPredicate::EQ, ta, tb, "").unwrap();
                let start = self.b.get_insert_block().unwrap();
                let (values, done) = (self.block_named("same_case"), self.block_named("compared"));
                self.b.build_conditional_branch(same_tag, values, done).unwrap();
                self.b.position_at_end(values);
                let pa = self.b.build_extract_value(sa, 1, "").unwrap();
                let pb = self.b.build_extract_value(sb, 1, "").unwrap();
                let r = self.call_rt("tsl_box_eq", &[pa, pb]);
                let same_values = self.truthy(r);
                let values_end = self.b.get_insert_block().unwrap();
                self.br(done);
                self.b.position_at_end(done);
                let phi = self.b.build_phi(self.ctx.bool_type(), "equal").unwrap();
                phi.add_incoming(&[(&self.ctx.bool_type().const_zero(), start), (&same_values, values_end)]);
                phi.as_basic_value().into_int_value()
            }
            Ty::Range => {
                let (sa, sb) = (a.into_struct_value(), b.into_struct_value());
                let mut same = self.ctx.bool_type().const_int(1, false);
                for i in 0..2 {
                    let x = self.b.build_extract_value(sa, i, "").unwrap().into_int_value();
                    let y = self.b.build_extract_value(sb, i, "").unwrap().into_int_value();
                    let eq = self.b.build_int_compare(IntPredicate::EQ, x, y, "").unwrap();
                    same = self.b.build_and(same, eq, "").unwrap();
                }
                same
            }
            Ty::Struct(_) | Ty::Opt(_) => {
                let f = self.eq_helper(t);
                let (pa, pb) = (self.spill(t, a), self.spill(t, b));
                let r = self.b.build_call(f, &[pa.into(), pb.into()], "").unwrap().try_as_basic_value().basic();
                self.truthy(r)
            }
            // Interface values: the same type (method table), and equal values.
            Ty::Iface(_) => {
                let (sa, sb) = (a.into_struct_value(), b.into_struct_value());
                let parts: Vec<Val> = [sa, sb]
                    .iter()
                    .flat_map(|s| [0, 1].map(|i| self.b.build_extract_value(*s, i, "").unwrap()))
                    .collect();
                let r = self.call_rt("tsl_iface_eq", &parts);
                self.truthy(r)
            }
            // Functions (in a struct that's compared as a whole, or that
            // needs its type's description): the very same one, with the
            // same variables around it.
            Ty::Fn(..) => {
                let (sa, sb) = (a.into_struct_value(), b.into_struct_value());
                let mut same = self.ctx.bool_type().const_int(1, false);
                for i in 0..2 {
                    let x = self.b.build_extract_value(sa, i, "").unwrap().into_pointer_value();
                    let y = self.b.build_extract_value(sb, i, "").unwrap().into_pointer_value();
                    let (x, y) = (
                        self.b.build_ptr_to_int(x, self.i64(), "").unwrap(),
                        self.b.build_ptr_to_int(y, self.i64(), "").unwrap(),
                    );
                    let eq = self.b.build_int_compare(IntPredicate::EQ, x, y, "").unwrap();
                    same = self.b.build_and(same, eq, "").unwrap();
                }
                same
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
            self.store(ty, data, value);
            let slot = self.alloca(self.ptr_ty().into(), name);
            self.b.build_store(slot, bx).unwrap();
            Store::Boxed(slot)
        } else {
            let lt = self.llty(ty).unwrap();
            let slot = self.alloca(lt, name);
            self.store(ty, slot, value);
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

    /// Holds an owned value while the rest of an expression is evaluated.
    fn hold(&mut self, v: Val<'ctx>, ty: &Ty) {
        let func = self.func();
        self.held.push(Held { func, ty: ty.clone(), value: HeldValue::Value(v) });
    }

    /// Holds a value being built in `slot`.
    fn hold_slot(&mut self, slot: PointerValue<'ctx>, ty: &Ty) {
        let func = self.func();
        self.held.push(Held { func, ty: ty.clone(), value: HeldValue::Slot(slot) });
    }

    /// Stops holding the last `n` values (they're used now).
    fn unhold(&mut self, n: usize) {
        self.held.truncate(self.held.len() - n);
    }

    /// Before an early exit: releases this function's held values, from index `from` on.
    fn release_held(&mut self, from: usize) {
        let func = self.func();
        let items: Vec<(Ty, HeldValue<'ctx>)> = self.held[from..]
            .iter()
            .filter(|h| h.func == func)
            .map(|h| (h.ty.clone(), match h.value { HeldValue::Value(v) => HeldValue::Value(v), HeldValue::Slot(p) => HeldValue::Slot(p) }))
            .collect();
        for (ty, v) in items.into_iter().rev() {
            let v = match v {
                HeldValue::Value(v) => v,
                HeldValue::Slot(p) => self.load(&ty, p),
            };
            self.release(v, &ty);
        }
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
                    self.release_at(&v.ty, p);
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
            Store::Slot(p) | Store::EnvValue(p) | Store::Place(p) => p,
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
        if self.readable_place(e) {
            self.place_indexes(e);
        }
        self.read_ptr_of(e)
    }

    /// Whether `e` is `g.width` or `g.height` of a `Graphics`: read from the
    /// runtime (see `property`), not from where `g` is stored.
    fn page_size(&self, e: &Expr) -> bool {
        matches!(&e.kind, ExprKind::Member { object, name, .. }
            if self.ty(object) == builtins::GRAPHICS && matches!(name.name.as_str(), "width" | "height"))
    }

    /// Whether `read_ptr` gives a pointer for `e`.
    fn readable_place(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Name(n) => self.lookup(n).is_some(),
            // (A computed property is a call.)
            ExprKind::Member { .. } if self.checked.calls.contains_key(&e.id) || self.page_size(e) => false,
            ExprKind::Member { object, optional: false, .. } => {
                matches!(self.ty(object), Ty::Struct(_) | Ty::Iface(_)) && self.readable_place(object)
            }
            ExprKind::Index(object, _) => matches!(self.ty(object), Ty::List(_)) && self.readable_place(object),
            _ => false,
        }
    }

    /// Works out the indexes on the way to a place (`a.b[i].c[j]`: `i`, then
    /// `j`) before any of the place is reached. An index can be a call that
    /// changes what it indexes (`list[grow(list)]`, with a `bind`
    /// parameter): reached first, the list's storage would be the one from
    /// before the call.
    fn place_indexes(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Member { object, .. } => self.place_indexes(object),
            ExprKind::Index(object, index) if matches!(self.ty(object), Ty::List(_)) => {
                self.place_indexes(object);
                // (Numbers and names can't change anything: they're read in place.)
                if !matches!(index.kind, ExprKind::Int(_) | ExprKind::Name(_)) && !self.pre_index.contains_key(&index.id) {
                    let i = self.expr(index).unwrap();
                    self.pre_index.insert(index.id, i);
                }
            }
            _ => {}
        }
    }

    /// An index of a place: worked out before (`place_indexes`), or now.
    fn place_index(&mut self, index: &Expr) -> Val<'ctx> {
        match self.pre_index.remove(&index.id) {
            Some(i) => i,
            None => self.expr(index).unwrap(),
        }
    }

    fn read_ptr_of(&mut self, e: &Expr) -> Option<PointerValue<'ctx>> {
        match &e.kind {
            ExprKind::Name(n) => {
                let v = self.lookup(n)?;
                Some(self.var_read_ptr(&v, e.span))
            }
            ExprKind::Member { .. } if self.checked.calls.contains_key(&e.id) || self.page_size(e) => None,
            ExprKind::Member { object, name, optional: false } if matches!(self.ty(object), Ty::Iface(_)) => {
                let Ty::Iface(i) = self.ty(object) else { unreachable!() };
                let base = self.read_ptr(object)?;
                let lt = self.llty(&Ty::Iface(i)).unwrap().into_struct_type();
                let s = self.b.build_load(lt, base, "").unwrap().into_struct_value();
                let vt = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
                let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
                let recv = self.box_data(bx);
                Some(self.iface_prop_ptr(i, &name.name, vt, recv))
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
                let i = self.place_index(index);
                let l = self.loc(e.span);
                Some(self.call_rt("tsl_list_get", &[list, i, l.into()]).unwrap().into_pointer_value())
            }
            _ => None,
        }
    }

    /// A pointer to the storage of `e`, for writing (makes lists unique).
    fn write_ptr(&mut self, e: &Expr) -> PointerValue<'ctx> {
        self.place_indexes(e);
        self.write_ptr_of(e)
    }

    fn write_ptr_of(&mut self, e: &Expr) -> PointerValue<'ctx> {
        match &e.kind {
            ExprKind::Name(n) => {
                let v = self.lookup(n).expect("assignable name");
                self.var_write_ptr(&v, e.span)
            }
            ExprKind::Member { object, name, .. } if matches!(self.ty(object), Ty::Iface(_)) => {
                let Ty::Iface(i) = self.ty(object) else { unreachable!() };
                let slot = self.write_ptr(object);
                let (vt, recv) = self.iface_unique(i, slot);
                self.iface_prop_ptr(i, &name.name, vt, recv)
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
                let i = self.place_index(index);
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
            ExprKind::Member { .. } if self.checked.calls.contains_key(&e.id) => false,
            ExprKind::Member { object, optional: false, .. } => {
                matches!(self.ty(object), Ty::Struct(_) | Ty::Iface(_)) && self.is_place(object)
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
        // A function returning `View` that lists several views: its value
        // builds them all, as a view's body does.
        if want == Some(&Ty::View) && self.checked.view_lists.contains(&b.span) {
            return Some(self.make_closure(Body::Content(b), &Ty::View));
        }
        self.push_scope();
        let mut result = None;
        let n = b.stmts.len();
        let want = want.filter(|t| **t != Ty::Void);
        for (i, s) in b.stmts.iter().enumerate() {
            if self.terminated() {
                self.continue_dead();
            }
            self.debug_line(debug::stmt_span(s));
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
        // (Copies of big values made for the statement before are done with.)
        self.era += 1;
        match s {
            Stmt::Let(d) | Stmt::Var(d) => {
                let ty = self.checked.locals.get(&d.name.span).cloned().unwrap_or_else(|| self.ty(&d.value));
                let v = self.expr_as(&d.value, &ty);
                self.declare_value(&d.name.name, &ty, v);
            }
            Stmt::State(d) => self.unsupported(d.span, "`state`"),
            // A function declared inside a function: a closure, kept in a
            // variable of its name.
            Stmt::Fn(f) => {
                let t = self.checked.locals.get(&f.name.span).cloned().expect("a checked function");
                let v = self.make_closure(Body::LocalFn(f), &t);
                self.declare_value(&f.name.name, &t, Some(v));
            }
            Stmt::Assign { target, op, value, .. } => self.assign(target, *op, value),
            Stmt::Return { value, .. } => {
                let ret = self.stack.last().unwrap().ret.clone();
                let v = value.as_ref().and_then(|v| self.expr_as(v, &ret));
                self.release_held(0);
                self.release_all_scopes();
                self.emit_return(v);
                self.continue_dead();
            }
            Stmt::For { var, iter, body, .. } => self.for_loop(var, iter, body),
            Stmt::While { cond, body, .. } => self.while_loop(cond, body),
            Stmt::Break(_) | Stmt::Continue(_) => {
                let func = self.func();
                let Some(target) = self.loops.iter().rev().find(|l| l.func == func) else {
                    unreachable!("checked: `break` is inside a loop")
                };
                let (to, depth) = (if matches!(s, Stmt::Break(_)) { target.done } else { target.next }, target.depth);
                let held = target.held;
                self.release_held(held);
                // Leaving the body: release its variables, as `return` does.
                let vars: Vec<Var> = self.stack.last().unwrap().scopes[depth..].iter().flatten().cloned().collect();
                for v in vars.iter().rev() {
                    self.release_var(v);
                }
                self.b.build_unconditional_branch(to).unwrap();
                self.continue_dead();
            }
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
        // `data[i] = byte` (and `data[i] += 1`): the byte is set where it
        // is, or in a copy if the `Data` is held elsewhere too.
        if let ExprKind::Index(object, index) = &target.kind
            && self.ty(object) == Ty::Data
        {
            let v = self.expr_as(value, &Ty::Int).unwrap();
            self.place_indexes(object);
            let i = self.expr(index).unwrap();
            let slot = self.write_ptr_of(object);
            let l = self.loc(target.span);
            let v = match op {
                AssignOp::Set => v,
                AssignOp::Op(bop) => {
                    let d = self.b.build_load(self.ptr_ty(), slot, "").unwrap();
                    let old = self.call_rt_generic_loc("tsl_data_get", d, i, l);
                    self.arith(bop, old, v, &Ty::Int, target.span.to(value.span))
                }
            };
            let p = self.ptr_ty();
            let f = self.declare_rt("tsl_data_set", &[p.into(), self.i64().into(), self.i64().into(), p.into()], None);
            self.b.build_call(f, &[slot.into(), i.into(), v.into(), l.into()], "").unwrap();
            return;
        }
        if op == AssignOp::Set || self.is_dict_entry(target) {
            let v = self.expr_as(value, &tt).unwrap();
            self.store_value(target, v);
            return;
        }
        let v = self.expr_as(value, &tt).unwrap();
        let p = self.write_ptr(target);
        // `list += more` adds to the list where it is, rather than make a
        // new list of both (which would copy the whole list every time).
        if let (AssignOp::Op(BinOp::Add), Ty::List(elem)) = (op, &tt) {
            let l = self.loc(target.span.to(value.span));
            self.list_mutating(p, elem, "appendAll", &[v], l);
            return;
        }
        // `data += more` and `text += more` too: the bytes go on the end,
        // where there's room.
        if let (AssignOp::Op(BinOp::Add), Ty::Data | Ty::Str) = (op, &tt) {
            let f = if tt == Ty::Str { "tsl_str_append" } else { "tsl_data_append" };
            self.call_rt(f, &[p.into(), v]);
            self.release(v, &tt);
            return;
        }
        let old = self.load(&tt, p);
        let AssignOp::Op(bop) = op else { unreachable!() };
        let new = self.arith(bop, old, v, &tt, target.span.to(value.span));
        self.release(v, &tt);
        self.b.build_store(p, new).unwrap();
        self.release(old, &tt);
    }

    /// `dict[key]`, as an assignment target?
    fn is_dict_entry(&self, e: &Expr) -> bool {
        matches!(&e.kind, ExprKind::Index(object, _) if matches!(self.ty(object), Ty::Dict(..)))
    }

    /// Can a value be stored into `e` (a variable, field, list element or dictionary entry)?
    fn writable(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Index(object, _) if self.is_dict_entry(e) => self.is_place(object),
            _ => self.is_place(e),
        }
    }

    /// Stores `v` (owned) into `target`, releasing what was there.
    fn store_value(&mut self, target: &Expr, v: Val<'ctx>) {
        let tt = self.ty(target);
        // `dict[key] = value` (or `= nil` to remove)
        if let ExprKind::Index(object, key) = &target.kind {
            if let Ty::Dict(kt, vt) = self.ty(object) {
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
        let p = self.write_ptr(target);
        if self.big(&tt) {
            // (The new value is its own already, so what was there can go first.)
            if self.needs_rc(&tt) {
                self.release_at(&tt, p);
            }
            self.store(&tt, p, v);
            return;
        }
        let old = self.load(&tt, p);
        self.store(&tt, p, v);
        self.release(old, &tt);
    }

    /// `while cond { … }` and `while let x = optional { … }`.
    fn while_loop(&mut self, cond: &Cond, body: &Block) {
        let (cond_bb, body_bb, done) = (self.block_named("while"), self.block_named("body"), self.block_named("done"));
        self.br(cond_bb);
        self.b.position_at_end(cond_bb);
        let depth = self.stack.last().unwrap().scopes.len();
        self.loops.push(LoopTarget { func: self.func(), next: cond_bb, done, depth, held: self.held.len() });
        self.push_scope();
        match cond {
            Cond::Expr(c) => {
                let c = self.expr(c).unwrap().into_int_value();
                self.b.build_conditional_branch(c, body_bb, done).unwrap();
                self.b.position_at_end(body_bb);
            }
            Cond::Let(name, _, value) => {
                let vt = self.ty(value);
                let Ty::Opt(inner) = vt.clone() else { unreachable!() };
                let whole = self.expr(value).unwrap();
                let ov = whole.into_struct_value();
                let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
                self.b.build_conditional_branch(flag, body_bb, done).unwrap();
                self.b.position_at_end(body_bb);
                let v = self.opt_inner(&vt, whole);
                let ty = self.checked.locals.get(&name.span).cloned().unwrap_or(*inner);
                self.declare_value(&name.name, &ty, Some(v));
            }
        }
        self.block(body, Some(&Ty::Void));
        self.pop_scope();
        self.loops.pop();
        self.br(cond_bb);
        self.b.position_at_end(done);
    }

    fn for_loop(&mut self, var: &Ident, iter: &Expr, body: &Block) {
        let captured = self.loop_var_shared(body, &var.name);
        self.for_each_in(var, iter, captured, &mut |g| {
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
    /// `captured`: a closure in the body captures the loop variable.
    fn for_each_in(&mut self, var: &Ident, iter: &Expr, captured: bool, body: &mut dyn FnMut(&mut Self)) {
        if self.ty(iter) == Ty::Range {
            let (lo, hi) = match &iter.kind {
                ExprKind::Binary(BinOp::Range, lo, hi) => {
                    (self.expr(lo).unwrap().into_int_value(), self.expr(hi).unwrap().into_int_value())
                }
                // A range stored in a variable, returned by a function, …
                _ => {
                    let r = self.expr(iter).unwrap().into_struct_value();
                    let lo = self.b.build_extract_value(r, 0, "").unwrap().into_int_value();
                    let hi = self.b.build_extract_value(r, 1, "").unwrap().into_int_value();
                    (lo, hi)
                }
            };
            let i = self.alloca(self.i64().into(), "i");
            self.b.build_store(i, lo).unwrap();
            let (cond_bb, body_bb, next_bb, done) =
                (self.block_named("for"), self.block_named("body"), self.block_named("next"), self.block_named("done"));
            self.b.build_unconditional_branch(cond_bb).unwrap();
            self.b.position_at_end(cond_bb);
            let cur = self.b.build_load(self.i64(), i, "").unwrap().into_int_value();
            let more = self.b.build_int_compare(IntPredicate::SLT, cur, hi, "").unwrap();
            self.b.build_conditional_branch(more, body_bb, done).unwrap();
            self.b.position_at_end(body_bb);
            let depth = self.stack.last().unwrap().scopes.len();
            self.loops.push(LoopTarget { func: self.func(), next: next_bb, done, depth, held: self.held.len() });
            self.push_scope();
            self.declare_value(&var.name, &Ty::Int, Some(cur.into()));
            body(self);
            self.pop_scope();
            self.loops.pop();
            self.br(next_bb);
            self.b.position_at_end(next_bb);
            let cur = self.b.build_load(self.i64(), i, "").unwrap().into_int_value();
            let next = self.b.build_int_add(cur, self.i64().const_int(1, false), "").unwrap();
            self.b.build_store(i, next).unwrap();
            self.br(cond_bb);
            self.b.position_at_end(done);
            return;
        }
        // A set is looped over as a list of its items.
        let (elem, items) = match self.ty(iter) {
            Ty::List(elem) => (elem, None),
            Ty::Set(elem) => {
                let set = self.expr(iter).unwrap();
                let items = self.call_rt("tsl_dict_keys", &[set]).unwrap();
                self.release(set, &Ty::Set(elem.clone()));
                (elem, Some(items))
            }
            // A dictionary, as a list of `(key, value)` tuples.
            Ty::Dict(kt, vt) => {
                let d = self.expr(iter).unwrap();
                let pairs = self.dict_pairs(d, &kt, &vt);
                self.release(d, &Ty::Dict(kt.clone(), vt.clone()));
                let pair = self.tuple_struct(&[(*kt).clone(), (*vt).clone()]);
                (Box::new(pair), Some(pairs))
            }
            _ => unreachable!(),
        };
        let place = items.is_none() && self.checked.loop_places.contains(&var.span) && self.is_place(iter);
        let idx = self.alloca(self.i64().into(), "idx");
        self.b.build_store(idx, self.i64().const_zero()).unwrap();
        // Either iterate the live list in place, or a snapshot of it. The
        // snapshot is a hidden variable, so an early `return` releases it.
        self.push_scope();
        // A list reached through an index (`outer[i].items`) moves when the
        // outer list grows, so the loop finds it again each round.
        let movable = place && through_index(iter);
        // A closure capturing the loop variable must find the element again
        // later; if the list's storage may move, keep a locator for it.
        let relocate = if place && ((captured && !self.in_state(iter)) || movable) {
            let loc = self.make_closure(Body::Locator(iter), &Ty::View);
            let lt = self.llty(&Ty::View).unwrap();
            let slot = self.alloca(lt, "list_locator");
            self.b.build_store(slot, loc).unwrap();
            self.push_var(Var { name: String::new(), ty: Ty::View, store: Store::Binding { slot, owned: true } });
            Some(slot)
        } else {
            None
        };
        let slot = if movable {
            // Not used: the locator finds the list each time.
            self.ptr_ty().const_null()
        } else if place {
            self.write_ptr(iter)
        } else {
            let list = items.unwrap_or_else(|| self.expr(iter).unwrap());
            let slot = self.alloca(self.ptr_ty().into(), "list");
            self.b.build_store(slot, list).unwrap();
            self.push_var(Var { name: String::new(), ty: Ty::List(elem.clone()), store: Store::Slot(slot) });
            slot
        };
        let (cond_bb, body_bb, next_bb, done) =
            (self.block_named("for"), self.block_named("body"), self.block_named("next"), self.block_named("done"));
        self.b.build_unconditional_branch(cond_bb).unwrap();
        self.b.position_at_end(cond_bb);
        let current = match relocate.filter(|_| movable) {
            Some(loc_slot) => {
                let t = self.llty(&Ty::View).unwrap();
                let loc = self.b.build_load(t, loc_slot, "").unwrap();
                self.call_locator(loc)
            }
            None => slot,
        };
        let list = self.b.build_load(self.ptr_ty(), current, "").unwrap();
        let len = self.call_rt("tsl_list_len", &[list]).unwrap().into_int_value();
        let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
        let more = self.b.build_int_compare(IntPredicate::SLT, cur, len, "").unwrap();
        self.b.build_conditional_branch(more, body_bb, done).unwrap();
        self.b.position_at_end(body_bb);
        let depth = self.stack.last().unwrap().scopes.len();
        self.loops.push(LoopTarget { func: self.func(), next: next_bb, done, depth, held: self.held.len() });
        self.push_scope();
        if let (true, Some(locator)) = (movable, relocate) {
            self.push_var(Var { name: var.name.clone(), ty: (*elem).clone(), store: Store::ListElemVia { locator, idx } });
        } else if place {
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
        self.loops.pop();
        self.br(next_bb);
        self.b.position_at_end(next_bb);
        let cur = self.b.build_load(self.i64(), idx, "").unwrap().into_int_value();
        let next = self.b.build_int_add(cur, self.i64().const_int(1, false), "").unwrap();
        self.b.build_store(idx, next).unwrap();
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
        // A plain value where a `SqlValue` is wanted: the prelude's
        // `__sqlInt`, `__sqlOptText`, … make it one.
        if let Some(id) = self.checked.sql_value.filter(|id| *to == Ty::Enum(*id)) {
            let kind = |t: &Ty| match t {
                Ty::Int => Some("Int"),
                Ty::Float => Some("Float"),
                Ty::Str => Some("Text"),
                Ty::Bool => Some("Bool"),
                Ty::Data => Some("Data"),
                _ => None,
            };
            let name = match from {
                Ty::Opt(inner) => kind(inner).map(|k| format!("__sqlOpt{k}")),
                t => kind(t).map(|k| format!("__sql{k}")),
            };
            if let Some(f) = name.and_then(|n| self.fns.get(&n).copied()) {
                let null = self.ptr_ty().const_null();
                let _ = id;
                return self.b.build_call(f, &[null.into(), v.into()], "").unwrap().try_as_basic_value().basic().unwrap();
            }
        }
        match to {
            Ty::Opt(inner) if !matches!(from, Ty::Opt(_)) => {
                let inner_v = self.coerce(v, from, inner);
                self.some(to, inner_v)
            }
            // `Circle?` as `Shape?`
            Ty::Opt(inner) if matches!(from, Ty::Opt(_)) && from != to => {
                let Ty::Opt(from_inner) = from else { unreachable!() };
                let lt = self.llty(to).unwrap();
                let slot = self.alloca(lt, "converted");
                let none = self.none(to);
                self.b.build_store(slot, none).unwrap();
                let s = v.into_struct_value();
                let flag = self.b.build_extract_value(s, 0, "").unwrap().into_int_value();
                let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
                self.b.build_conditional_branch(flag, some_bb, done).unwrap();
                self.b.position_at_end(some_bb);
                let iv = self.b.build_extract_value(s, 1, "").unwrap();
                let (fi, ti) = ((**from_inner).clone(), (**inner).clone());
                let c = self.coerce(iv, &fi, &ti);
                let some = self.some(to, c);
                self.b.build_store(slot, some).unwrap();
                self.br(done);
                self.b.position_at_end(done);
                self.load(to, slot)
            }
            // `[Circle]` as `[Shape]`: a new list of converted items.
            Ty::List(to_elem) if matches!(from, Ty::List(_)) && from != to => {
                let Ty::List(from_elem) = from else { unreachable!() };
                let (fe, te) = ((**from_elem).clone(), (**to_elem).clone());
                let n = self.call_rt("tsl_list_len", &[v]).unwrap().into_int_value();
                let info = self.type_info(&te);
                let out = self.call_rt("tsl_list_new", &[info.into(), n.into()]).unwrap();
                let slot = self.spill(to, out);
                let null = self.ptr_ty().const_null();
                self.counted_loop(n, &mut |g, i| {
                    let p = g.call_rt("tsl_list_get", &[v, i.into(), null.into()]).unwrap().into_pointer_value();
                    let item = g.load(&fe, p);
                    g.retain(item, &fe);
                    let c = g.coerce(item, &fe, &te);
                    let cp = g.spill(&te, c);
                    g.call_rt("tsl_list_push", &[slot.into(), cp.into()]);
                });
                self.release(v, from);
                self.load(to, slot)
            }
            // A struct or enum where an interface is wanted: box it with its method table.
            Ty::Iface(i) if matches!(from, Ty::Struct(_) | Ty::Enum(_)) => {
                let info = self.type_info(from);
                let bx = self.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
                let data = self.box_data(bx);
                self.b.build_store(data, v).unwrap();
                let vt = self.vtable(from, *i);
                let st = self.llty(to).unwrap().into_struct_type();
                let s = self.b.build_insert_value(st.get_undef(), vt, 0, "").unwrap();
                self.b.build_insert_value(s, bx, 1, "").unwrap().into_struct_value().into()
            }
            _ => v,
        }
    }

    fn some(&mut self, opt_ty: &Ty, v: Val<'ctx>) -> Val<'ctx> {
        let st = self.llty(opt_ty).unwrap().into_struct_type();
        if let (true, Ty::Opt(inner)) = (self.big(opt_ty), opt_ty) {
            // A big value, wrapped where it is in memory.
            let place = self.copy_slot(opt_ty, st.into());
            let flag = self.b.build_struct_gep(st, place, 0, "").unwrap();
            self.b.build_store(flag, self.ctx.bool_type().const_int(1, false)).unwrap();
            let value = self.b.build_struct_gep(st, place, 1, "").unwrap();
            self.store(inner, value, v);
            return self.value_at(opt_ty, place);
        }
        let s = st.get_undef();
        let s = self.b.build_insert_value(s, self.ctx.bool_type().const_int(1, false), 0, "").unwrap();
        self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value().into()
    }

    /// The value in an optional that has one (`ov` is the optional).
    fn opt_inner(&mut self, opt_ty: &Ty, ov: Val<'ctx>) -> Val<'ctx> {
        if let (Some(p), Ty::Opt(inner)) = (self.origin_of(ov), opt_ty) {
            // (From where the optional is in memory, not field by field.)
            let st = self.llty(opt_ty).unwrap().into_struct_type();
            let value = self.b.build_struct_gep(st, p, 1, "").unwrap();
            let inner = (**inner).clone();
            return if self.big(&inner) {
                self.value_at(&inner, value)
            } else {
                let lt = self.llty(&inner).unwrap();
                self.b.build_load(lt, value, "").unwrap()
            };
        }
        self.b.build_extract_value(ov.into_struct_value(), 1, "").unwrap()
    }

    fn none(&mut self, opt_ty: &Ty) -> Val<'ctx> {
        self.llty(opt_ty).unwrap().const_zero()
    }

    /// Evaluates `e` to an owned value (`None` if it produces no value).
    fn expr(&mut self, e: &Expr) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        // A constant (`limit`, `Rect.zero`): the function that keeps it.
        if let Some(name) = self.checked.const_refs.get(&e.id) {
            return self.constant(&name.clone());
        }
        // A computed property (`rect.area`, or `area` in the type's own
        // methods): its method, called.
        if matches!(e.kind, ExprKind::Name(_) | ExprKind::Member { .. }) && self.checked.calls.contains_key(&e.id) {
            return self.call(e, e, &[], None);
        }
        match &e.kind {
            ExprKind::GenericName(..) => unreachable!("checked: a generic type isn't a value"),
            ExprKind::Try(inner) => self.try_expr(e, inner),
            ExprKind::Await(call) => self.await_expr(e, call),
            // A list passed as a variadic parameter's values: the list itself.
            ExprKind::Spread(list) => self.expr_as(list, &t),
            ExprKind::Continuation => Some(self.continuation(&t)),
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
            ExprKind::List(items) if matches!(t, Ty::Set(_)) => {
                let Ty::Set(elem) = &t else { unreachable!() };
                let (ki, vi) = (self.type_info(elem), self.type_info(&Ty::Bool));
                let d = self.call_rt("tsl_dict_new", &[ki.into(), vi.into()]).unwrap();
                let slot = self.spill(&t, d);
                self.hold_slot(slot, &t);
                for item in items {
                    let v = self.expr_as(item, elem).unwrap();
                    self.set_insert(slot, v, elem);
                }
                self.unhold(1);
                Some(self.load(&t, slot))
            }
            ExprKind::List(items) => Some(self.list_of(&items.iter().collect::<Vec<_>>(), &t)),
            ExprKind::Dict(pairs) => {
                let Ty::Dict(kt, vt) = &t else { unreachable!() };
                let (ki, vi) = (self.type_info(kt), self.type_info(vt));
                let d = self.call_rt("tsl_dict_new", &[ki.into(), vi.into()]).unwrap();
                let slot = self.spill(&t, d);
                self.hold_slot(slot, &t);
                for (k, v) in pairs {
                    let kv = self.expr_as(k, kt).unwrap();
                    self.hold(kv, kt);
                    let vv = self.expr_as(v, vt).unwrap();
                    self.unhold(1);
                    let (kp, vp) = (self.spill(kt, kv), self.spill(vt, vv));
                    self.call_rt("tsl_dict_set", &[slot.into(), kp.into(), vp.into()]);
                }
                self.unhold(1);
                Some(self.load(&t, slot))
            }
            ExprKind::Cast { value, test, .. } => {
                let Ty::Iface(i) = self.ty(value) else { unreachable!("checked: an interface value") };
                let target = self.checked.cast_targets[&e.id].clone();
                let v = self.expr(value).unwrap();
                let s = v.into_struct_value();
                let vt = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
                let table = self.vtable(&target, i);
                let is = self.b.build_int_compare(IntPredicate::EQ, vt, table, "").unwrap();
                if *test {
                    self.release(v, &Ty::Iface(i));
                    return Some(is.into());
                }
                let lt = self.llty(&t).unwrap();
                let slot = self.alloca(lt, "cast");
                let none = self.none(&t);
                self.b.build_store(slot, none).unwrap();
                let (yes, done) = (self.block_named("is"), self.block_named("done"));
                self.b.build_conditional_branch(is, yes, done).unwrap();
                self.b.position_at_end(yes);
                let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
                let data = self.box_data(bx);
                let c = self.load(&target, data);
                self.retain(c, &target);
                let some = self.some(&t, c);
                self.b.build_store(slot, some).unwrap();
                self.br(done);
                self.b.position_at_end(done);
                self.release(v, &Ty::Iface(i));
                Some(self.load(&t, slot))
            }
            ExprKind::Unary(UnaryOp::Not, x) => {
                let v = self.expr(x).unwrap().into_int_value();
                Some(self.b.build_not(v, "").unwrap().into())
            }
            ExprKind::Unary(UnaryOp::BitNot, x) => {
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
            if matches!(v.store, Store::ViewFn) {
                return Some(self.make_closure(Body::ViewFnRef(n.to_string()), t));
            }
            let p = self.var_read_ptr(&v, span);
            let val = self.load(&v.ty, p);
            self.retain(val, &v.ty);
            return Some(val);
        }
        if n == "pi" && !self.fns.contains_key(n) {
            return Some(self.ctx.f64_type().const_float(std::f64::consts::PI).into());
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
            // "0..3"
            Ty::Range => {
                let r = v.into_struct_value();
                let lo = self.b.build_extract_value(r, 0, "").unwrap();
                let hi = self.b.build_extract_value(r, 1, "").unwrap();
                let lo = self.call_rt("tsl_str_from_int", &[lo]).unwrap();
                let hi = self.call_rt("tsl_str_from_int", &[hi]).unwrap();
                let dots = self.str_lit("..").into();
                let arr_ty = self.ptr_ty().array_type(3);
                let arr = self.alloca(arr_ty.into(), "range");
                for (i, part) in [lo, dots, hi].into_iter().enumerate() {
                    let idx = [self.i64().const_zero(), self.i64().const_int(i as u64, false)];
                    let slot = unsafe { self.b.build_gep(arr_ty, arr, &idx, "").unwrap() };
                    self.b.build_store(slot, part).unwrap();
                }
                let three = self.i64().const_int(3, false);
                let text = self.call_rt("tsl_str_concat", &[arr.into(), three.into()]).unwrap();
                self.release(lo, &Ty::Str);
                self.release(hi, &Ty::Str);
                text
            }
            // Lists, dictionaries, sets and tuples: `[1, 2]`, `["a": 1]`, `(1, "one")`.
            Ty::List(_) | Ty::Dict(..) | Ty::Set(_) | Ty::Struct(_) | Ty::Opt(_) => {
                let text = self.repr(v, t);
                self.release(v, t);
                text
            }
            _ => unreachable!("not showable"),
        }
    }

    /// Text for a (borrowed) value inside a list, dictionary, set or tuple,
    /// as it would be written in code: Strings in quotes, `nil` for nothing.
    fn repr(&mut self, v: Val<'ctx>, t: &Ty) -> Val<'ctx> {
        match t {
            Ty::Str => self.call_rt("tsl_str_quoted", &[v]).unwrap(),
            Ty::Enum(_) => {
                self.retain(v, t);
                self.stringify(v, t)
            }
            Ty::Opt(inner) => {
                let slot = self.alloca(self.ptr_ty().into(), "text");
                let flag = self.b.build_extract_value(v.into_struct_value(), 0, "").unwrap().into_int_value();
                let (some, none, done) = (self.block_named("some"), self.block_named("none"), self.block_named("shown"));
                self.b.build_conditional_branch(flag, some, none).unwrap();
                self.b.position_at_end(some);
                let iv = self.b.build_extract_value(v.into_struct_value(), 1, "").unwrap();
                let text = self.repr(iv, inner);
                self.b.build_store(slot, text).unwrap();
                self.br(done);
                self.b.position_at_end(none);
                let nil = self.str_lit("nil");
                self.b.build_store(slot, nil).unwrap();
                self.br(done);
                self.b.position_at_end(done);
                self.b.build_load(self.ptr_ty(), slot, "").unwrap()
            }
            Ty::List(_) | Ty::Dict(..) | Ty::Set(_) | Ty::Struct(_) => {
                let f = self.show_helper(t);
                let p = self.spill(t, v);
                self.b.build_call(f, &[p.into()], "").unwrap().try_as_basic_value().basic().unwrap()
            }
            _ => self.stringify(v, t),
        }
    }

    /// `"[" + a + "]"`: joins pieces (borrowed) into a new String.
    fn concat(&mut self, parts: &[Val<'ctx>]) -> Val<'ctx> {
        let arr_ty = self.ptr_ty().array_type(parts.len() as u32);
        let arr = self.alloca(arr_ty.into(), "parts");
        for (i, part) in parts.iter().enumerate() {
            let idx = [self.i64().const_zero(), self.i64().const_int(i as u64, false)];
            let slot = unsafe { self.b.build_gep(arr_ty, arr, &idx, "").unwrap() };
            self.b.build_store(slot, *part).unwrap();
        }
        let n = self.i64().const_int(parts.len() as u64, false);
        self.call_rt("tsl_str_concat", &[arr.into(), n.into()]).unwrap()
    }

    /// `ptr helper(ptr value)`: the text of a list, dictionary, set or tuple.
    fn show_helper(&mut self, t: &Ty) -> FunctionValue<'ctx> {
        let name = format!("tsl_h_show_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return *f;
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, p.fn_type(&[p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        self.in_helper(f, |g| {
            let vp = f.get_nth_param(0).unwrap().into_pointer_value();
            let v = g.load(t, vp);
            // The items' texts, joined with ", " between brackets.
            let si = g.type_info(&Ty::Str);
            let zero = g.i64().const_zero();
            let parts = g.call_rt("tsl_list_new", &[si.into(), zero.into()]).unwrap();
            let strs = Ty::list(Ty::Str);
            let slot = g.spill(&strs, parts);
            let span = Span::default();
            let (open, close) = match t {
                Ty::List(elem) | Ty::Set(elem) => {
                    let elem = (**elem).clone();
                    let items = if matches!(t, Ty::Set(_)) { g.call_rt("tsl_dict_keys", &[v]).unwrap() } else { v };
                    g.each(items, &elem, span, |g, item| {
                        let text = g.repr(item, &elem);
                        g.release(item, &elem);
                        g.push_item(slot, text, &Ty::Str);
                    });
                    if matches!(t, Ty::Set(_)) {
                        g.release(items, &Ty::list(elem));
                    }
                    ("[", "]")
                }
                Ty::Dict(kt, vt) => {
                    let (kt, vt) = ((**kt).clone(), (**vt).clone());
                    let keys = g.call_rt("tsl_dict_keys", &[v]).unwrap();
                    let values = g.call_rt("tsl_dict_values", &[v]).unwrap();
                    let n = g.call_rt("tsl_list_len", &[keys]).unwrap().into_int_value();
                    let null = g.ptr_ty().const_null();
                    let sep = g.str_lit(": ");
                    g.counted_loop(n, &mut |g, i| {
                        let kp = g.call_rt("tsl_list_get", &[keys, i.into(), null.into()]).unwrap().into_pointer_value();
                        let vp = g.call_rt("tsl_list_get", &[values, i.into(), null.into()]).unwrap().into_pointer_value();
                        let (k, val) = (g.load(&kt, kp), g.load(&vt, vp));
                        let (ks, vs) = (g.repr(k, &kt), g.repr(val, &vt));
                        let entry = g.concat(&[ks, sep.into(), vs]);
                        g.release(ks, &Ty::Str);
                        g.release(vs, &Ty::Str);
                        g.push_item(slot, entry, &Ty::Str);
                    });
                    g.release(keys, &Ty::list(kt));
                    g.release(values, &Ty::list(vt));
                    ("[", "]")
                }
                Ty::Struct(id) => {
                    let fields: Vec<Ty> = g.checked.structs[*id].fields.iter().map(|f| f.ty.clone()).collect();
                    for (i, ft) in fields.iter().enumerate() {
                        let fv = g.b.build_extract_value(v.into_struct_value(), i as u32, "").unwrap();
                        let text = g.repr(fv, ft);
                        g.push_item(slot, text, &Ty::Str);
                    }
                    ("(", ")")
                }
                _ => unreachable!(),
            };
            let parts = g.b.build_load(g.ptr_ty(), slot, "").unwrap();
            let comma = g.str_lit(", ");
            let body = g.call_rt("tsl_list_joined", &[parts, comma.into()]).unwrap();
            g.release(parts, &strs);
            // An empty dictionary is `[:]`.
            let (open, close) = if matches!(t, Ty::Dict(..)) {
                let n = g.call_rt("tsl_dict_len", &[v]).unwrap().into_int_value();
                let empty = g.b.build_int_compare(IntPredicate::EQ, n, g.i64().const_zero(), "").unwrap();
                let (o, c) = (g.str_lit("["), g.str_lit(":]"));
                let c2 = g.str_lit("]");
                let close = g.b.build_select(empty, c, c2, "").unwrap();
                (o.into(), close)
            } else {
                (g.str_lit(open).into(), g.str_lit(close).into())
            };
            let text = g.concat(&[open, body, close]);
            g.release(body, &Ty::Str);
            g.b.build_return(Some(&text)).unwrap();
        });
        f
    }

    fn interpolate(&mut self, parts: &[StrPart]) -> Val<'ctx> {
        if let [StrPart::Text(t)] = parts {
            return self.str_lit(t).into();
        }
        // An array of (tag, value) pieces for `tsl_str_interpolate`: Strings
        // by address (tag 0), and Ints (1) and Floats (2, as bits) directly,
        // so they need no string of their own.
        let n = parts.len() as u32;
        let i64t = self.i64();
        let piece_ty = self.ctx.struct_type(&[i64t.into(), i64t.into()], false);
        let arr_ty = piece_ty.array_type(n);
        let arr = self.alloca(arr_ty.into(), "pieces");
        let mut owned = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let (tag, value): (u64, IntValue) = match part {
                StrPart::Text(t) => {
                    let p = self.str_lit(t);
                    (0, self.b.build_ptr_to_int(p, i64t, "").unwrap())
                }
                StrPart::Expr(x) => {
                    let t = self.ty(x);
                    let v = self.expr(x).unwrap();
                    match t {
                        Ty::Int => (1, v.into_int_value()),
                        Ty::Float => (2, self.b.build_bit_cast(v, i64t, "").unwrap().into_int_value()),
                        _ => {
                            let s = self.stringify(v, &t);
                            self.hold(s, &Ty::Str);
                            owned.push(s);
                            (0, self.b.build_ptr_to_int(s.into_pointer_value(), i64t, "").unwrap())
                        }
                    }
                }
            };
            let idx = [self.i64().const_zero(), self.i64().const_int(i as u64, false)];
            let slot = unsafe { self.b.build_gep(arr_ty, arr, &idx, "").unwrap() };
            let piece = piece_ty.const_named_struct(&[i64t.const_int(tag, false).into(), i64t.const_zero().into()]);
            let piece = self.b.build_insert_value(piece, value, 1, "").unwrap();
            self.b.build_store(slot, piece).unwrap();
        }
        let count = self.i64().const_int(n as u64, false);
        self.unhold(owned.len());
        let p = self.ptr_ty();
        let f = self.declare_rt("tsl_str_interpolate", &[p.into(), i64t.into()], Some(p.into()));
        let r = self.b.build_call(f, &[arr.into(), count.into()], "").unwrap().try_as_basic_value().basic().unwrap();
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
                // `x == nil`: only whether there's a value matters.
                let other = match (&l.kind, &r.kind) {
                    (_, ExprKind::Nil) => Some(l),
                    (ExprKind::Nil, _) => Some(r),
                    _ => None,
                };
                if let Some(x) = other {
                    let xt = self.ty(x);
                    let v = self.expr(x).unwrap();
                    let present = self.b.build_extract_value(v.into_struct_value(), 0, "").unwrap().into_int_value();
                    self.release(v, &xt);
                    let res = if op == BinOp::Eq { self.b.build_not(present, "").unwrap() } else { present };
                    return Some(res.into());
                }
                let (lt, rt) = (self.ty(l), self.ty(r));
                // Compare as the optional type if only one side is optional.
                let common = if matches!(lt, Ty::Opt(_)) { lt.clone() } else { rt.clone() };
                let lv = self.expr_as(l, &common).unwrap();
                self.hold(lv, &common);
                let rv = self.expr_as(r, &common).unwrap();
                self.unhold(1);
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
                self.hold(lv, &ot);
                let rv = self.expr(r).unwrap();
                self.unhold(1);
                let res = self.compare(op, lv, rv, &ot);
                self.release(lv, &ot);
                self.release(rv, &ot);
                Some(res.into())
            }
            BinOp::Coalesce => {
                let lt = self.ty(l);
                let whole = self.expr(l).unwrap();
                let lv = whole.into_struct_value();
                let flag = self.b.build_extract_value(lv, 0, "").unwrap().into_int_value();
                let lt_ = self.llty(t).unwrap();
                let slot = self.alloca(lt_, "coalesce");
                let (some_bb, none_bb, done) = (self.block_named("some"), self.block_named("none"), self.block_named("done"));
                self.b.build_conditional_branch(flag, some_bb, none_bb).unwrap();
                self.b.position_at_end(some_bb);
                let Ty::Opt(inner) = &lt else { unreachable!() };
                let inner_v = self.opt_inner(&lt, whole);
                let v = self.coerce(inner_v, inner, t);
                self.store(t, slot, v);
                self.br(done);
                self.b.position_at_end(none_bb);
                if let Some(rv) = self.expr_as(r, t) {
                    self.store(t, slot, rv);
                }
                self.br(done);
                self.b.position_at_end(done);
                Some(self.load(t, slot))
            }
            // A range value: its start and (excluded) end.
            BinOp::Range => {
                let lo = self.expr(l).unwrap();
                let hi = self.expr(r).unwrap();
                let st = self.llty(&Ty::Range).unwrap().into_struct_type();
                let v = self.b.build_insert_value(st.get_undef(), lo, 0, "").unwrap();
                Some(self.b.build_insert_value(v, hi, 1, "").unwrap().into_struct_value().into())
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
            Ty::Data => self.call_rt("tsl_data_concat", &[l, r]).unwrap(),
            _ => {
                let (a, b) = (l.into_int_value(), r.into_int_value());
                match op {
                    BinOp::BitAnd => self.b.build_and(a, b, "").unwrap().into(),
                    BinOp::BitOr => self.b.build_or(a, b, "").unwrap().into(),
                    BinOp::BitXor => self.b.build_xor(a, b, "").unwrap().into(),
                    // A shift by 64 or more leaves no bits (or only the sign,
                    // for `>>`); a negative shift stops the program.
                    BinOp::Shl | BinOp::Shr => {
                        let zero = self.i64().const_zero();
                        let negative = self.b.build_int_compare(IntPredicate::SLT, b, zero, "").unwrap();
                        self.panic_if(negative, "negative shift amount", span);
                        let max = self.i64().const_int(63, false);
                        let big = self.b.build_int_compare(IntPredicate::SGT, b, max, "").unwrap();
                        let amount = self.b.build_select(big, max, b, "").unwrap().into_int_value();
                        if op == BinOp::Shl {
                            let shifted = self.b.build_left_shift(a, amount, "").unwrap();
                            self.b.build_select(big, zero, shifted, "").unwrap()
                        } else {
                            self.b.build_right_shift(a, amount, true, "").unwrap().into()
                        }
                    }
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
        // `Result<Int>.none`
        if let (ExprKind::GenericName(..), Ty::Enum(id)) = (&object.kind, &t) {
            return Some(self.enum_value(*id, &name.name));
        }
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
        if let (Some(call), Some(pt)) = (builtins::runtime_property(t, name), builtins::property(t, name)) {
            return self.call_rt_generic(call.rt, &[(v, t.clone())], &pt).unwrap();
        }
        match t {
            // `g.width`, `g.height`: the page being drawn now, which
            // `g.newPage(width:height:)` can change (the field is the first
            // page's).
            Ty::Struct(id) if *t == builtins::GRAPHICS && matches!(name, "width" | "height") => {
                let idx = self.field_index(*id, name);
                let f = self.b.build_extract_value(v.into_struct_value(), idx, "").unwrap();
                let rt = if name == "width" { "tsl_canvas_page_width" } else { "tsl_canvas_page_height" };
                self.call_rt_generic(rt, &[(f, Ty::Float)], &Ty::Float).unwrap()
            }
            Ty::Struct(id) => {
                let idx = self.field_index(*id, name);
                let ft = self.checked.structs[*id].fields[idx as usize].ty.clone();
                let f = self.b.build_extract_value(v.into_struct_value(), idx, "").unwrap();
                self.retain(f, &ft);
                f
            }
            Ty::Iface(i) => {
                let (i, pt) = (*i, self.iface_prop_ty(*i, name));
                let s = v.into_struct_value();
                let vt = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
                let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
                let recv = self.box_data(bx);
                let p = self.iface_prop_ptr(i, name, vt, recv);
                let f = self.load(&pt, p);
                self.retain(f, &pt);
                f
            }
            Ty::Str => match name {
                "count" => self.call_rt("tsl_str_count", &[v]).unwrap(),
                _ => {
                    let n = self.call_rt("tsl_str_count", &[v]).unwrap().into_int_value();
                    self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_zero(), "").unwrap().into()
                }
            },
            Ty::List(_) if name == "indices" => self.list_indices(v),
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
            Ty::Set(_) => match name {
                "count" => self.call_rt("tsl_dict_len", &[v]).unwrap(),
                "isEmpty" => {
                    let n = self.call_rt("tsl_dict_len", &[v]).unwrap().into_int_value();
                    self.b.build_int_compare(IntPredicate::EQ, n, self.i64().const_zero(), "").unwrap().into()
                }
                _ => self.call_rt("tsl_dict_keys", &[v]).unwrap(),
            },
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

    /// `x?.change()` where the call changes the value inside the optional:
    /// if `object` (a place) holds a value, runs `f` on a copy of it in a
    /// slot and stores the changed copy back. Returns `f`'s result as an
    /// optional of `result` (or nothing, for `Void`).
    fn chain_mutate(
        &mut self,
        object: &Expr,
        opt_ty: &Ty,
        result: &Ty,
        f: impl FnOnce(&mut Self, PointerValue<'ctx>) -> Option<Val<'ctx>>,
    ) -> Option<Val<'ctx>> {
        let Ty::Opt(inner) = opt_ty else { unreachable!() };
        let inner = (**inner).clone();
        let out = if *result == Ty::Void {
            None
        } else {
            let lt = self.llty(result).unwrap();
            let slot = self.alloca(lt, "chain");
            let none = self.none(result);
            self.b.build_store(slot, none).unwrap();
            Some(slot)
        };
        // `dict[key]?.append(x)`: changed where it is in the dictionary. A
        // copy would share the value with the dictionary, so changing it
        // would copy it whole each time (issue #29).
        if let ExprKind::Index(dict, key) = &object.kind
            && let Ty::Dict(kt, _) = self.ty(dict)
            && self.stays_put(dict, false)
        {
            let k = self.expr_as(key, &kt).unwrap();
            let slot = self.write_ptr(dict);
            let kp = self.spill(&kt, k);
            let vp = self.call_rt("tsl_dict_value_slot", &[slot.into(), kp.into()]).unwrap().into_pointer_value();
            let k = self.load(&kt, kp);
            self.release(k, &kt);
            let present = self.b.build_is_not_null(vp, "present").unwrap();
            let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
            self.b.build_conditional_branch(present, some_bb, done).unwrap();
            self.b.position_at_end(some_bb);
            let r = f(self, vp);
            if let (Some(slot), Some(r)) = (out, r) {
                let r = if Some(r.get_type()) == self.llty(result) { r } else { self.some(result, r) };
                self.b.build_store(slot, r).unwrap();
            }
            self.br(done);
            self.b.position_at_end(done);
            return out.map(|slot| self.load(result, slot));
        }
        let ov = self.expr(object).unwrap().into_struct_value();
        let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
        self.b.build_conditional_branch(flag, some_bb, done).unwrap();
        self.b.position_at_end(some_bb);
        // The copy we own moves into the slot, is changed there, and moves back.
        let iv = self.b.build_extract_value(ov, 1, "").unwrap();
        let recv = self.spill(&inner, iv);
        let r = f(self, recv);
        if let (Some(slot), Some(r)) = (out, r) {
            let r = if Some(r.get_type()) == self.llty(result) { r } else { self.some(result, r) };
            self.b.build_store(slot, r).unwrap();
        }
        let changed = self.load(&inner, recv);
        let changed = self.some(opt_ty, changed);
        self.store_value(object, changed);
        self.br(done);
        self.b.position_at_end(done);
        out.map(|slot| self.load(result, slot))
    }

    fn index(&mut self, e: &Expr, object: &Expr, index: &Expr) -> Option<Val<'ctx>> {
        let t = self.ty(e);
        match self.ty(object) {
            Ty::Data => {
                let d = self.expr(object).unwrap();
                let i = self.expr(index).unwrap();
                let l = self.loc(e.span);
                let r = self.call_rt_generic_loc("tsl_data_get", d, i, l);
                self.release(d, &Ty::Data);
                Some(r)
            }
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

    /// The tuple struct for items of these types (made by the checker).
    fn tuple_struct(&self, items: &[Ty]) -> Ty {
        let name = format!("({})", items.iter().map(|t| self.checked.show(t)).collect::<Vec<_>>().join(", "));
        let id = self.checked.structs.iter().position(|s| s.name == name).expect("the checker made this tuple type");
        Ty::Struct(id)
    }

    /// A list of `n` pairs of type `pair`, `get(i)` giving each pair's two
    /// items (owned).
    fn build_pairs(
        &mut self,
        n: IntValue<'ctx>,
        pair: &Ty,
        get: &mut dyn FnMut(&mut Self, IntValue<'ctx>) -> (Val<'ctx>, Val<'ctx>),
    ) -> Val<'ctx> {
        let info = self.type_info(pair);
        let list = self.call_rt("tsl_list_new", &[info.into(), n.into()]).unwrap();
        let lt = Ty::list(pair.clone());
        let slot = self.spill(&lt, list);
        let Ty::Struct(id) = pair else { unreachable!() };
        let st = self.struct_ty(*id);
        self.counted_loop(n, &mut |g, i| {
            let (a, b) = get(g, i);
            let v = g.b.build_insert_value(st.get_undef(), a, 0, "").unwrap();
            let v = g.b.build_insert_value(v, b, 1, "").unwrap().into_struct_value();
            g.push_item(slot, v.into(), pair);
        });
        self.load(&lt, slot)
    }

    /// The item at `i` of `list` (owned).
    fn item_at(&mut self, list: Val<'ctx>, i: IntValue<'ctx>, elem: &Ty) -> Val<'ctx> {
        let null = self.ptr_ty().const_null();
        let p = self.call_rt("tsl_list_get", &[list, i.into(), null.into()]).unwrap().into_pointer_value();
        let v = self.load(elem, p);
        self.retain(v, elem);
        v
    }

    /// A dictionary's entries (borrowed dictionary) as `(key, value)` tuples.
    fn dict_pairs(&mut self, d: Val<'ctx>, kt: &Ty, vt: &Ty) -> Val<'ctx> {
        let keys = self.call_rt("tsl_dict_keys", &[d]).unwrap();
        let values = self.call_rt("tsl_dict_values", &[d]).unwrap();
        let n = self.call_rt("tsl_list_len", &[keys]).unwrap().into_int_value();
        let pair = self.tuple_struct(&[kt.clone(), vt.clone()]);
        let r = self.build_pairs(n, &pair, &mut |g, i| (g.item_at(keys, i, kt), g.item_at(values, i, vt)));
        self.release(keys, &Ty::list(kt.clone()));
        self.release(values, &Ty::list(vt.clone()));
        r
    }

    /// Adds `v` (owned) to the set in `slot`.
    fn set_insert(&mut self, slot: PointerValue<'ctx>, v: Val<'ctx>, elem: &Ty) {
        let kp = self.spill(elem, v);
        let yes = self.ctx.bool_type().const_int(1, false);
        let vp = self.spill(&Ty::Bool, yes.into());
        self.call_rt("tsl_dict_set", &[slot.into(), kp.into(), vp.into()]);
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
            Cond::Let(name, _, value) => {
                let vt = self.ty(value);
                let Ty::Opt(inner) = vt.clone() else { unreachable!() };
                let whole = self.expr(value).unwrap();
                let ov = whole.into_struct_value();
                let flag = self.b.build_extract_value(ov, 0, "").unwrap().into_int_value();
                self.b.build_conditional_branch(flag, then_bb, else_bb).unwrap();
                self.b.position_at_end(then_bb);
                let inner_v = self.opt_inner(&vt, whole);
                let ty = self.checked.locals.get(&name.span).cloned().unwrap_or(*inner);
                self.declare_value(&name.name, &ty, Some(inner_v));
            }
        }
        let v = self.block(then, Some(t));
        if let (Some(slot), Some(v)) = (slot, v) {
            if !self.terminated() {
                self.store(t, slot, v);
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
                self.store(t, slot, v);
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
            // Each pattern in turn: the arm runs at the first that matches.
            for (i, pattern) in arm.patterns.iter().enumerate() {
                let test = match pattern {
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
                let otherwise = if i + 1 == arm.patterns.len() { next_bb } else { self.block_named("or") };
                self.b.build_conditional_branch(test, body_bb, otherwise).unwrap();
                self.b.position_at_end(otherwise);
            }
            self.b.position_at_end(body_bb);
            self.push_scope();
            if let [Pattern::Case { name, bindings, .. }] = arm.patterns.as_slice() {
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
            (Body::Fn(..) | Body::ViewFnRef(_) | Body::LocalFn(_), Ty::Fn(ptys, ret)) => {
                (self.fn_type(ptys, ret, false), ptys.clone(), (**ret).clone())
            }
            (Body::Locator(_), _) => (self.ptr_ty().fn_type(&[self.ptr_ty().into()], false), vec![], Ty::Void),
            (Body::Rows(..), _) => {
                (self.ctx.void_type().fn_type(&[self.ptr_ty().into(), self.i64().into()], false), vec![Ty::Int], Ty::Void)
            }
            (Body::PathRows(..), _) => (
                self.ctx.void_type().fn_type(&[self.ptr_ty().into(), self.ptr_ty().into()], false),
                vec![Ty::list(Ty::Int)],
                Ty::Void,
            ),
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
            Body::Rows(p, b) | Body::PathRows(p, b) => free_block(b, &mut vec![p.name.clone()], &mut free),
            Body::ContentExpr(e) | Body::Locator(e) => free_expr(e, &mut Vec::new(), &mut free),
            Body::ViewFnRef(name) => {
                free.insert(name.clone());
            }
            Body::LocalFn(d) => free_fn(d, &mut vec![d.name.name.clone()], &mut free),
        }
        let mut captures: Vec<(Var<'ctx>, Capture)> = Vec::new();
        let mut needs_self = false;
        let mut needs_record = false;
        for name in &free {
            let Some(v) = self.lookup(name) else {
                // (A method or property of the receiver, by its bare name.)
                if let Some((_, owner)) = &self.stack.last().unwrap().self_ptr {
                    let methods = match owner {
                        Ty::Struct(id) => &self.checked.structs[*id].methods,
                        Ty::Enum(id) => &self.checked.enums[*id].methods,
                        _ => continue,
                    };
                    needs_self |= methods.iter().any(|m| m.name == *name) && !self.fns.contains_key(name);
                }
                continue;
            };
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
            Body::Fn(_, b) | Body::Content(b) | Body::Rows(_, b) | Body::PathRows(_, b) => self.boxed_vars(b),
            Body::LocalFn(d) => self.boxed_vars(&d.body),
            _ => HashSet::new(),
        };
        // A block that waits, or an `async fn` declared in a function: it
        // keeps its environment for as long as it goes on.
        let (waits, is_async) = match &body {
            Body::Fn(_, b) => (coro::awaits(b), false),
            Body::LocalFn(d) => (d.is_async, d.is_async),
            _ => (false, false),
        };
        let ret = match (is_async, ptys.last()) {
            (true, Some(Ty::Fn(ps, _))) => ps.first().cloned().unwrap_or(Ty::Void),
            _ => ret,
        };
        // What the debugger calls it, and where it is.
        let named = match &body {
            Body::Fn(_, b) | Body::Content(b) | Body::Rows(_, b) | Body::PathRows(_, b) => Some((self.debug_block_name(), b.span)),
            Body::LocalFn(d) => Some((d.name.name.clone(), d.name.span)),
            _ => None,
        };
        self.stack.push(FnCtx { func: f, scopes: vec![scope], ret: ret.clone(), self_ptr, boxed, record: inner_record, coro: None, dbg: None });
        if waits {
            let keep: Vec<PointerValue> = (!env_fields.is_empty()).then_some(env_param).into_iter().collect();
            self.begin_coro(&keep);
            self.own_receiver();
        }
        if let Some((name, span)) = named {
            self.debug_enter(&name, span);
        }
        match body {
            Body::Fn(params, b) => {
                for (i, (p, pt)) in params.iter().zip(&ptys).enumerate() {
                    let raw = f.get_nth_param(1 + i as u32).unwrap();
                    self.declare_param(&p.name, pt, raw);
                }
                let v = self.block(b, Some(&ret));
                self.finish_function(v);
            }
            Body::LocalFn(d) => {
                // Its own name: this closure, with this environment.
                let st = self.llty(t).unwrap().into_struct_type();
                let me = self.b.build_insert_value(st.get_undef(), f.as_global_value().as_pointer_value(), 0, "").unwrap();
                let me = self.b.build_insert_value(me, env_param, 1, "").unwrap();
                let slot = self.alloca(st.into(), &d.name.name);
                self.b.build_store(slot, me).unwrap();
                self.push_var(Var { name: d.name.name.clone(), ty: t.clone(), store: Store::EnvValue(slot) });
                for (i, (p, pt)) in d.params.iter().zip(&ptys).enumerate() {
                    let raw = f.get_nth_param(1 + i as u32).unwrap();
                    self.declare_param(&p.name.name, pt, raw);
                }
                if is_async {
                    self.take_done();
                }
                let v = self.block(&d.body, Some(&ret));
                self.finish_function(v);
            }
            Body::Content(b) => {
                self.content_block(b, false);
                self.finish_function(None);
            }
            Body::Rows(p, b) => {
                let v = f.get_nth_param(1).unwrap();
                self.declare_value(&p.name, &Ty::Int, Some(v));
                self.content_block(b, false);
                self.finish_function(None);
            }
            Body::PathRows(p, b) => {
                let v = f.get_nth_param(1).unwrap();
                self.declare_value(&p.name, &Ty::list(Ty::Int), Some(v));
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
            Body::ViewFnRef(name) => {
                // Pass the closure's arguments (which it owns) straight on.
                let r = self.stack.last().unwrap().record.clone().expect("view function outside its view");
                let target = self.view_fns[&(r.view.clone(), name)];
                let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into(), r.data.into()];
                for i in 0..ptys.len() {
                    all.push(f.get_nth_param(1 + i as u32).unwrap().into());
                }
                // (And the place for a big result, which it's given too.)
                if self.big(&ret) {
                    all.push(f.get_last_param().unwrap().into());
                }
                let v = self.b.build_call(target, &all, "").unwrap().try_as_basic_value().basic();
                self.finish_function(v);
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
        all.extend(self.abi_args(args, ptys));
        let place = self.result_place(ret, &mut all);
        let r = self.b.build_indirect_call(fn_ty, fp, &all, "").unwrap().try_as_basic_value().basic();
        self.call_result(ret, place, r)
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
                // (The direct version when every `bind` argument stays put.)
                let items = sig.params.iter().filter(|p| p.bind).count() == 1;
                let direct = self.direct_fns.contains_key(name)
                    && sig.params.iter().enumerate().all(|(i, p)| {
                        !p.bind || info.args.get(i).copied().flatten().is_some_and(|r| self.stays_put(arg(r), items))
                    });
                self.direct_args = direct;
                let vals = self.call_args(&info, &sig, &decl.params, &arg);
                let f = if direct { self.direct_fns[name] } else { self.fns[name] };
                let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into()];
                all.extend(self.abi_args(&vals, &param_lltys(&sig.params)));
                let place = self.result_place(&sig.ret, &mut all);
                let r = self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic();
                self.call_result(&sig.ret, place, r)
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
                    let (owner, name) = (owner.clone(), name.clone());
                    // `box?.add()`: a changing method changes the value inside the optional.
                    if self.method_mutating(&owner, &name) && self.writable(object) {
                        let info2 = info.clone();
                        return self.chain_mutate(object, &ot, &t, |g, recv| g.method_call(&owner, &name, recv, &info2, &arg));
                    }
                    let ov = self.expr(object).unwrap();
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
            Callee::IfaceMethod { iface, index } => {
                let (iface, index) = (*iface, *index);
                let ExprKind::Member { object, .. } = &callee.kind else { unreachable!() };
                let ot = self.ty(object);
                let it = Ty::Iface(iface);
                if self.checked.ifaces[iface].mutating[index] {
                    // A method that changes the value: work on its storage, with a box of its own.
                    if let Ty::Opt(_) = ot {
                        let info2 = info.clone();
                        return self.chain_mutate(object, &ot, &t, |g, slot| {
                            let (vt, recv) = g.iface_unique(iface, slot);
                            g.iface_invoke(iface, index, vt, recv, &info2, &arg)
                        });
                    }
                    if self.is_place(object) {
                        let slot = self.write_ptr(object);
                        let (vt, recv) = self.iface_unique(iface, slot);
                        return self.iface_invoke(iface, index, vt, recv, &info, &arg);
                    }
                    // A temporary: change a copy, then let it go.
                    let v = self.expr(object).unwrap();
                    let slot = self.spill(&it, v);
                    let (vt, recv) = self.iface_unique(iface, slot);
                    self.hold_slot(slot, &it);
                    let r = self.iface_invoke(iface, index, vt, recv, &info, &arg);
                    self.unhold(1);
                    let cur = self.load(&it, slot);
                    self.release(cur, &it);
                    return r;
                }
                let ov = self.expr(object).unwrap();
                if let Ty::Opt(_) = ot {
                    // `shape?.area()`
                    if t == Ty::Void {
                        let flag = self.b.build_extract_value(ov.into_struct_value(), 0, "").unwrap().into_int_value();
                        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
                        self.b.build_conditional_branch(flag, some_bb, done).unwrap();
                        self.b.position_at_end(some_bb);
                        let iv = self.b.build_extract_value(ov.into_struct_value(), 1, "").unwrap();
                        self.hold(iv, &it);
                        self.iface_call(iface, index, iv, &info, &arg);
                        self.unhold(1);
                        self.release(iv, &it);
                        self.br(done);
                        self.b.position_at_end(done);
                        return None;
                    }
                    let info2 = info.clone();
                    return Some(self.optional_chain(ov, &ot, &t, |g, v| g.iface_call(iface, index, v, &info2, &arg).unwrap()));
                }
                self.hold(ov, &it);
                let r = self.iface_call(iface, index, ov, &info, &arg);
                self.unhold(1);
                self.release(ov, &it);
                r
            }
            Callee::StructInit(id) => {
                let id = *id;
                let st = self.struct_ty(id);
                let fields = self.checked.structs[id].fields.clone();
                // (Built-in structs have no declaration, and no defaults.)
                let decl = self.struct_decls.get(&id).copied();
                let t = Ty::Struct(id);
                if self.big(&t) {
                    // A big struct is put together in memory: the values
                    // given, then the rest by a routine the struct's uses
                    // share (see `defaults_helper`).
                    let mut given: Vec<(usize, Val<'ctx>)> = Vec::new();
                    let mut defaulted: Vec<usize> = Vec::new();
                    for (i, f) in fields.iter().enumerate() {
                        match info.args[i] {
                            Some(r) => {
                                let v = self.expr_as(arg(r), &f.ty).unwrap();
                                self.hold(v, &f.ty);
                                given.push((i, v));
                            }
                            None => defaulted.push(i),
                        }
                    }
                    self.unhold(given.len());
                    let place = self.copy_slot(&t, st.into());
                    if !defaulted.is_empty() {
                        let helper = self.defaults_helper(id, &defaulted);
                        self.b.build_call(helper, &[place.into()], "").unwrap();
                    }
                    for (i, v) in given {
                        let fp = self.b.build_struct_gep(st, place, i as u32, "").unwrap();
                        self.store(&fields[i].ty, fp, v);
                    }
                    return Some(self.value_at(&t, place));
                }
                let mut s = st.get_undef();
                for (i, f) in fields.iter().enumerate() {
                    let v = match info.args[i] {
                        Some(r) => self.expr_as(arg(r), &f.ty).unwrap(),
                        None => {
                            let default = decl.unwrap().fields[i].default.as_ref().unwrap();
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
                    self.store(&f.ty, fp, v);
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
                    let (recv, name) = (recv.clone(), name.clone());
                    // `list?.append(x)`: change the value inside the optional, in place.
                    if MUTATING_BUILTINS.contains(&name.as_str()) && self.writable(object) {
                        let info2 = info.clone();
                        return self.chain_mutate(object, &ot, &t, |g, slot| {
                            g.builtin_method(&recv, &name, object, &info2, &arg, args, trailing, &t, e.span, Some(slot))
                        });
                    }
                    let ov = self.expr(object).unwrap();
                    // `job?.cancel()`: a method that gives nothing runs only when there's a value.
                    if t == Ty::Void {
                        let Ty::Opt(inner) = &ot else { unreachable!() };
                        let flag = self.b.build_extract_value(ov.into_struct_value(), 0, "").unwrap().into_int_value();
                        let (some_bb, done) = (self.block_named("some"), self.block_named("done"));
                        self.b.build_conditional_branch(flag, some_bb, done).unwrap();
                        self.b.position_at_end(some_bb);
                        let iv = self.b.build_extract_value(ov.into_struct_value(), 1, "").unwrap();
                        self.builtin_on_value(&recv, &name, iv, &info, &arg, e.span);
                        self.release(iv, inner);
                        self.br(done);
                        self.b.position_at_end(done);
                        return None;
                    }
                    let info2 = info.clone();
                    let r = self.optional_chain(ov, &ot, &t, |g, v| match (&recv, name.as_str()) {
                        // `x?.list.map { … }`: the closure is compiled here, not passed as a value.
                        (Ty::List(elem), "map") => g.list_map(v, elem, args, trailing, e.span),
                        _ => g.builtin_on_value(&recv, &name, v, &info2, &arg, e.span).unwrap(),
                    });
                    return Some(r);
                }
                self.builtin_method(recv, name, object, &info, &arg, args, trailing, &t, e.span, None)
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
                // (An awaited call's completion block comes after a trailing block.)
                let done = e.continuation();
                let given = &args[..args.len() - done.is_some() as usize];
                let exprs: Vec<&Expr> = given.iter().map(|a| &a.value).chain(trailing).chain(done).collect();
                let mut vals = Vec::new();
                for (x, pt) in exprs.into_iter().zip(ptys.clone().iter()) {
                    let v = self.expr_as(x, pt).unwrap();
                    self.hold(v, pt);
                    vals.push(v);
                }
                self.unhold(vals.len());
                let r = self.call_fn_value(fv, &ft, &vals);
                self.release(fv, &ft);
                r
            }
            // A view used as a value: wrap it so it can be emitted later.
            Callee::View(_) | Callee::BuiltinView(_) | Callee::Modifier(_) => {
                Some(self.make_closure(Body::ContentExpr(e), &Ty::View))
            }
            Callee::LocalFn { name, decl } => {
                let (sig, params) = self.checked.local_fns[decl].clone();
                let ft = Ty::Fn(sig.params.iter().map(|p| p.ty.clone()).collect(), Box::new(sig.ret.clone()));
                let vals = self.call_args(&info, &sig, &params, &arg);
                let fv = self.name(name, &ft, callee.span).unwrap();
                let r = self.call_fn_value(fv, &ft, &vals);
                self.release(fv, &ft);
                r
            }
            Callee::ViewFn(name) => {
                let r = self.stack.last().unwrap().record.clone().expect("view function outside its view");
                let sig = self.checked.view_fns[&(r.view.clone(), name.clone())].clone();
                let decl = self.view_fn_decl(&r.view, name);
                let vals = self.call_args(&info, &sig, &decl.params, &arg);
                let f = self.view_fns[&(r.view.clone(), name.clone())];
                let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into(), r.data.into()];
                // (As the view's functions are declared: see `declare_views`.)
                let tys: Vec<Ty> = sig.params.iter().map(|p| p.ty.clone()).collect();
                all.extend(self.abi_args(&vals, &tys));
                let place = self.result_place(&sig.ret, &mut all);
                let r = self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic();
                self.call_result(&sig.ret, place, r)
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

    /// A new list `t` of these items.
    fn list_of(&mut self, items: &[&Expr], t: &Ty) -> Val<'ctx> {
        let Ty::List(elem) = t else { unreachable!() };
        let info = self.type_info(elem);
        let n = self.i64().const_int(items.len() as u64, false);
        let list = self.call_rt("tsl_list_new", &[info.into(), n.into()]).unwrap();
        let slot = self.spill(t, list);
        self.hold_slot(slot, t);
        for item in items {
            let v = self.expr_as(item, elem).unwrap();
            self.push_item(slot, v, elem);
        }
        self.unhold(1);
        self.load(t, slot)
    }

    /// An argument's value, as type `t`: values given one by one to a
    /// variadic parameter are made into its list.
    fn arg_val<'e>(&mut self, r: ArgRef, t: &Ty, arg: &dyn Fn(ArgRef) -> &'e Expr) -> Val<'ctx> {
        match r {
            ArgRef::Many(from, count) => {
                let items: Vec<&Expr> = (from..from + count).map(|i| arg(ArgRef::Arg(i))).collect();
                self.list_of(&items, t)
            }
            r => self.expr_as(arg(r), t).unwrap(),
        }
    }

    /// Evaluates arguments in parameter order, filling in defaults.
    fn call_args<'e>(
        &mut self,
        info: &CallInfo,
        sig: &FnSig,
        decl_params: &[Param],
        arg: &dyn Fn(ArgRef) -> &'e Expr,
    ) -> Vec<Val<'ctx>> {
        // (Taken now: the arguments may be calls with arguments of their own.)
        let direct = std::mem::replace(&mut self.direct_args, false);
        let mut vals = Vec::new();
        // (A direct `bind` argument is the caller's own storage: nothing to hold.)
        let mut held = 0;
        for (i, p) in sig.params.iter().enumerate() {
            let v = match info.args.get(i).copied().flatten() {
                // The callee changes the caller's value through a locator.
                // The storage itself, for a function's direct version.
                Some(r) if p.bind && direct => {
                    let at = self.write_ptr(arg(r));
                    let lt = self.llty(&Ty::View).unwrap().into_struct_type();
                    let none = self.ptr_ty().const_null();
                    let pair = self.b.build_insert_value(lt.get_undef(), none, 0, "").unwrap().into_struct_value();
                    let pair = self.b.build_insert_value(pair, at, 1, "").unwrap().into_struct_value();
                    vals.push(pair.into());
                    continue;
                }
                Some(r) if p.bind => {
                    let loc = self.make_closure(Body::Locator(arg(r)), &Ty::View);
                    self.hold(loc, &Ty::View);
                    held += 1;
                    vals.push(loc);
                    continue;
                }
                Some(r) => Some(self.arg_val(r, &p.ty, arg)),
                None => {
                    let default = decl_params[i].default.as_ref().expect("default value");
                    self.expr_as(default, &p.ty)
                }
            };
            let v = v.unwrap();
            self.hold(v, &p.ty);
            held += 1;
            vals.push(v);
        }
        self.unhold(held);
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
        all.extend(self.abi_args(&vals, &param_lltys(&sig.params)));
        let place = self.result_place(&sig.ret, &mut all);
        let r = self.b.build_call(f, &all, "").unwrap().try_as_basic_value().basic();
        self.call_result(&sig.ret, place, r)
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
        slot: Option<PointerValue<'ctx>>,
    ) -> Option<Val<'ctx>> {
        // Methods that change the receiver work on its storage (`slot`, if
        // the caller already has it).
        match (recv, name) {
            (Ty::List(elem), "removeFirst" | "removeLast" | "appendAll") => {
                let elem = (**elem).clone();
                let slot = slot.unwrap_or_else(|| self.write_ptr(object));
                let mut vals = Vec::new();
                if name == "appendAll" {
                    vals.push(self.expr_as(arg(info.args[0].unwrap()), &Ty::list(elem.clone())).unwrap());
                }
                let l = self.loc(span);
                self.list_mutating(slot, &elem, name, &vals, l).unwrap()
            }
            (Ty::List(elem), "append" | "insert" | "remove" | "removeAll") => {
                let elem = (**elem).clone();
                let slot = slot.unwrap_or_else(|| self.write_ptr(object));
                let l = self.loc(span);
                match name {
                    "append" | "insert" => {
                        let v = self.expr_as(arg(info.args[0].unwrap()), &elem).unwrap();
                        if name == "append" {
                            self.push_item(slot, v, &elem);
                        } else {
                            let p = self.spill(&elem, v);
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
            // `data.append(other)` and `data.append(byte: 255)`, in place.
            (Ty::Data, "append") => {
                let slot = slot.unwrap_or_else(|| self.write_ptr(object));
                let byte = args.first().and_then(|a| a.label.as_ref()).is_some_and(|l| l.name == "byte");
                if byte {
                    let b = self.expr(arg(info.args[0].unwrap())).unwrap();
                    self.call_rt("tsl_data_append_byte", &[slot.into(), b]);
                } else {
                    let d = self.expr_as(arg(info.args[0].unwrap()), &Ty::Data).unwrap();
                    self.call_rt("tsl_data_append", &[slot.into(), d]);
                    self.release(d, &Ty::Data);
                }
                None
            }
            (Ty::Set(elem), "insert" | "remove") => {
                let elem = (**elem).clone();
                let slot = slot.unwrap_or_else(|| self.write_ptr(object));
                let v = self.expr_as(arg(info.args[0].unwrap()), &elem).unwrap();
                if name == "insert" {
                    self.set_insert(slot, v, &elem);
                } else {
                    let kp = self.spill(&elem, v);
                    let out = self.alloca(self.ctx.bool_type().into(), "removed");
                    self.call_rt("tsl_dict_remove", &[slot.into(), kp.into(), out.into()]);
                    self.release(v, &elem);
                }
                None
            }
            (Ty::Dict(kt, vt), "removeValue") => {
                let (kt, vt) = ((**kt).clone(), (**vt).clone());
                let slot = slot.unwrap_or_else(|| self.write_ptr(object));
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
                let list = self.expr(object).unwrap();
                let r = self.list_map(list, elem, args, trailing, span);
                self.release(list, recv);
                Some(r)
            }
            _ => {
                let v = self.expr(object).unwrap();
                self.hold(v, recv);
                let r = self.builtin_on_value(recv, name, v, info, arg, span);
                self.unhold(1);
                self.release(v, recv);
                r
            }
        }
    }

    /// `list.map(f)`: a new list of `f(item)` for each item of a borrowed `list`.
    fn list_map<'e>(&mut self, list: Val<'ctx>, elem: &Ty, args: &'e [Arg], trailing: Option<&'e Expr>, span: Span) -> Val<'ctx> {
        let f_expr = match (args, trailing) {
            ([], Some(tr)) => tr,
            ([a], _) => &a.value,
            _ => unreachable!(),
        };
        let ft = self.ty(f_expr);
        let Ty::Fn(_, out_t) = &ft else { unreachable!() };
        let out_t = (**out_t).clone();
        let f = self.expr(f_expr).unwrap();
        let info_ptr = self.type_info(&out_t);
        let len = self.call_rt("tsl_list_len", &[list]).unwrap();
        let out = self.call_rt("tsl_list_new", &[info_ptr.into(), len]).unwrap();
        let out_slot = self.spill(&Ty::List(Box::new(out_t.clone())), out);
        self.each(list, elem, span, |g, item| {
            let r = g.call_fn_value(f, &ft, &[item]).unwrap();
            g.push_item(out_slot, r, &out_t);
        });
        self.release(f, &ft);
        self.b.build_load(self.ptr_ty(), out_slot, "").unwrap()
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
            g.push_item(out_slot, item, elem);
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
        if let (Some(call), Some(m)) = (builtins::runtime_method(recv, name), builtins::method_of(recv, name)) {
            // (The receiver is borrowed; the arguments are released below.)
            let mut vals = if call.no_receiver {
                Vec::new()
            } else if *recv == builtins::JOB || *recv == builtins::SOUND {
                // A `BackgroundJob` goes as its `id`.
                let id = self.b.build_extract_value(v.into_struct_value(), 0, "").unwrap();
                vec![(id, Ty::Int)]
            } else {
                vec![(v, recv.clone())]
            };
            let own = vals.len();
            for (i, p) in m.sig.params.iter().enumerate() {
                let x = match a(self, i, &p.ty) {
                    Some(x) => x,
                    None => self.default_value(call.defaults.get(i).copied().unwrap_or(builtins::Dflt::Req), &p.ty),
                };
                self.hold(x, &p.ty);
                vals.push((x, p.ty.clone()));
            }
            self.unhold(vals.len() - own);
            let r = self.call_rt_located(call.rt, &vals, &m.sig.ret, call.located.then_some(span));
            for (x, ty) in vals.into_iter().skip(own) {
                self.release(x, &ty);
            }
            return r;
        }
        if let (Ty::Float, "formatted") = (recv, name) {
            let decimals = a(self, 0, &Ty::Int).unwrap();
            return self.call_rt("tsl_float_format", &[v, decimals]);
        }
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
                    "enumerated" => {
                        let n = self.call_rt("tsl_list_len", &[v]).unwrap().into_int_value();
                        let pair = self.tuple_struct(&[Ty::Int, elem.clone()]);
                        self.build_pairs(n, &pair, &mut |g, i| (i.into(), g.item_at(v, i, &elem)))
                    }
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
                    _ => {
                        let m = builtins::method_of(recv, name).expect("checked list method");
                        let mut vals = Vec::new();
                        for (i, p) in m.sig.params.iter().enumerate() {
                            if let Some(x) = a(self, i, &p.ty) {
                                vals.push(x);
                            }
                        }
                        return self.list_method(v, &elem, name, &vals, span);
                    }
                })
            }
            Ty::Dict(kt, _) | Ty::Set(kt) if name == "contains" => {
                let key = a(self, 0, kt).unwrap();
                let p = self.spill(kt, key);
                let r = self.call_rt("tsl_dict_contains", &[v, p.into()]);
                self.release(key, kt);
                Some(self.truthy(r).into())
            }
            // `filter` and `sorted` work on the items as a list.
            Ty::Set(elem) => {
                let elem = (**elem).clone();
                let items = self.call_rt("tsl_dict_keys", &[v]).unwrap();
                let lt = Ty::list(elem.clone());
                self.hold(items, &lt);
                let r = self.builtin_on_value(&lt, name, items, info, arg, span);
                self.unhold(1);
                self.release(items, &lt);
                if name == "filter" {
                    let kept = r.unwrap();
                    let bi = self.type_info(&Ty::Bool);
                    let set = self.call_rt("tsl_set_from_list", &[kept, bi.into()]);
                    self.release(kept, &lt);
                    return set;
                }
                r
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
    /// A box holding `closure` (taken over), for adapters the runtime calls
    /// as `adapter(box, …)`.
    pub(super) fn box_closure(&mut self, t: &Ty, closure: Val<'ctx>) -> PointerValue<'ctx> {
        let ti = self.type_info(t);
        let bx = self.call_rt("tsl_box_new", &[ti.into()]).unwrap().into_pointer_value();
        let data = self.box_data(bx);
        self.b.build_store(data, closure).unwrap();
        bx
    }

    /// `adapter(box, ResponseC*)`: builds an `HttpResponse` and calls the `done` block in the box.
    fn response_adapter(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_h_http_done";
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![builtins::HTTP_RESPONSE], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let rc = f.get_nth_param(1).unwrap().into_pointer_value();
            let closure = g.load(&ft, g.box_data(bx));
            // ResponseC: { i64 status, ptr text, ptr data, ptr headers, ptr error }
            let p = g.ptr_ty();
            let ct = g.ctx.struct_type(&[g.i64().into(), p.into(), p.into(), p.into(), p.into()], false);
            let field = |g: &mut Self, i: u32, t: BasicTypeEnum<'ctx>| {
                let fp = g.b.build_struct_gep(ct, rc, i, "").unwrap();
                g.b.build_load(t, fp, "").unwrap()
            };
            let status = field(g, 0, g.i64().into()).into_int_value();
            let ok_low = g.b.build_int_compare(IntPredicate::SGE, status, g.i64().const_int(200, false), "").unwrap();
            let ok_high = g.b.build_int_compare(IntPredicate::SLT, status, g.i64().const_int(300, false), "").unwrap();
            let ok = g.b.build_and(ok_low, ok_high, "").unwrap();
            let st = g.struct_ty(0);
            let mut s = st.get_undef();
            s = g.b.build_insert_value(s, status, 0, "").unwrap().into_struct_value();
            s = g.b.build_insert_value(s, ok, 1, "").unwrap().into_struct_value();
            for (i, src) in [(2u32, 1u32), (3, 2), (4, 3), (5, 4)] {
                let v = field(g, src, p.into());
                s = g.b.build_insert_value(s, v, i, "").unwrap().into_struct_value();
            }
            g.call_fn_value(closure, &ft, &[s.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `adapter(box, width, height)`: calls the `Canvas` block in the box
    /// with a `Graphics` of that size.
    pub(super) fn graphics_adapter(&mut self) -> PointerValue<'ctx> {
        self.sized_adapter("tsl_h_canvas_draw", builtins::GRAPHICS)
    }

    /// The same for a `Scene3D` block, with a `Scene`.
    pub(super) fn scene_adapter(&mut self) -> PointerValue<'ctx> {
        self.sized_adapter("tsl_h_scene_draw", builtins::SCENE)
    }

    /// `adapter(box, width, height)`: calls the block in the box with a
    /// `with` (a struct of a width and a height).
    fn sized_adapter(&mut self, name: &str, with: Ty) -> PointerValue<'ctx> {
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let (p, f64t) = (self.ptr_ty(), self.ctx.f64_type());
        let f = self.module.add_function(
            name,
            self.ctx.void_type().fn_type(&[p.into(), f64t.into(), f64t.into()], false),
            Some(Linkage::Internal),
        );
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![with.clone()], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let closure = g.load(&ft, g.box_data(bx));
            let st = g.llty(&with).unwrap().into_struct_type();
            let mut s = st.get_undef();
            for i in 0..2 {
                s = g.b.build_insert_value(s, f.get_nth_param(1 + i).unwrap(), i, "").unwrap().into_struct_value();
            }
            g.call_fn_value(closure, &ft, &[s.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `adapter(box, PointerC*)`: builds a `PointerEvent` and calls the
    /// `.onPointer` block in the box.
    pub(super) fn pointer_adapter(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_h_on_pointer";
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![builtins::POINTER_EVENT], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let ec = f.get_nth_param(1).unwrap().into_pointer_value();
            let closure = g.load(&ft, g.box_data(bx));
            // PointerC: { i32 kind, i32 button, i64 clicks, f64 x, y, dx, dy,
            // i8 shift, alt, command, f64 window_x, window_y }
            let (i32t, i8t, f64t) = (g.ctx.i32_type(), g.i8(), g.ctx.f64_type());
            let ct = g.ctx.struct_type(
                &[
                    i32t.into(),
                    i32t.into(),
                    g.i64().into(),
                    f64t.into(),
                    f64t.into(),
                    f64t.into(),
                    f64t.into(),
                    i8t.into(),
                    i8t.into(),
                    i8t.into(),
                    f64t.into(),
                    f64t.into(),
                ],
                false,
            );
            let field = |g: &mut Self, i: u32, t: BasicTypeEnum<'ctx>| {
                let fp = g.b.build_struct_gep(ct, ec, i, "").unwrap();
                g.b.build_load(t, fp, "").unwrap()
            };
            let st = g.llty(&builtins::POINTER_EVENT).unwrap().into_struct_type();
            let mut s = st.get_undef();
            // PointerEvent: kind, x, y, windowX, windowY, dx, dy, button,
            // clicks, shift, alt, command
            let plain: [(u32, u32, BasicTypeEnum<'ctx>); 9] = [
                (0, 0, i32t.into()),
                (1, 3, f64t.into()),
                (2, 4, f64t.into()),
                (3, 10, f64t.into()),
                (4, 11, f64t.into()),
                (5, 5, f64t.into()),
                (6, 6, f64t.into()),
                (7, 1, i32t.into()),
                (8, 2, g.i64().into()),
            ];
            for (to, from, t) in plain {
                let v = field(g, from, t);
                s = g.b.build_insert_value(s, v, to, "").unwrap().into_struct_value();
            }
            for (to, from) in [(9u32, 7u32), (10, 8), (11, 9)] {
                let v = field(g, from, i8t.into());
                let flag = g.truthy(Some(v));
                s = g.b.build_insert_value(s, flag, to, "").unwrap().into_struct_value();
            }
            g.call_fn_value(closure, &ft, &[s.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `adapter(box, KeyC*)`: builds a `KeyEvent` (taking over its strings)
    /// and calls the `.onKeyEvent` block in the box.
    pub(super) fn key_event_adapter(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_h_on_key_event";
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![builtins::KEY_EVENT], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let ec = f.get_nth_param(1).unwrap().into_pointer_value();
            let closure = g.load(&ft, g.box_data(bx));
            // KeyC: { ptr key, ptr text, i8 shift, alt, command, control, repeated, up }
            let i8t = g.i8();
            let ct = g.ctx.struct_type(
                &[p.into(), p.into(), i8t.into(), i8t.into(), i8t.into(), i8t.into(), i8t.into(), i8t.into()],
                false,
            );
            let st = g.llty(&builtins::KEY_EVENT).unwrap().into_struct_type();
            let mut s = st.get_undef();
            for i in 0..8u32 {
                let fp = g.b.build_struct_gep(ct, ec, i, "").unwrap();
                let v = if i < 2 {
                    g.b.build_load(p, fp, "").unwrap()
                } else {
                    let v = g.b.build_load(i8t, fp, "").unwrap();
                    g.truthy(Some(v)).into()
                };
                s = g.b.build_insert_value(s, v, i, "").unwrap().into_struct_value();
            }
            g.call_fn_value(closure, &ft, &[s.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// `background(work: { … }) { result in … }`. The values `work` uses
    /// are encoded (as for `toBinary`) into one tree here; a worker function,
    /// run on another thread, decodes them into its own copies, runs `work`
    /// and encodes the result; the done adapter decodes that back on the UI
    /// thread. So no value is ever shared between threads.
    ///
    /// The worker gets the job's id too, for a `work: { job in … }` block,
    /// and the job (a `BackgroundJob`) is the value of the call.
    fn background<'e>(&mut self, args: &'e [Arg], arg: &dyn Fn(ArgRef) -> &'e Expr, span: Span) -> Val<'ctx> {
        let label = |a: &&Arg, l: &str| a.label.as_ref().is_some_and(|x| x.name == l);
        // (Awaited: the result goes on from the `await`, and a block after
        // the call is the work.)
        let awaited = args.last().map(|a| &a.value).filter(|v| matches!(v.kind, ExprKind::Continuation));
        let work = match args.iter().find(|a| label(a, "work")) {
            Some(a) => &a.value,
            None => arg(ArgRef::Trailing),
        };
        let done = match (awaited, args.iter().find(|a| label(a, "done"))) {
            (Some(done), _) => done,
            (None, Some(a)) => &a.value,
            (None, None) => arg(ArgRef::Trailing),
        };
        let progress = args.iter().find(|a| label(a, "progress")).map(|a| &a.value);
        let ExprKind::Closure { body, params } = &work.kind else { unreachable!("checked: `work` is a block") };
        let Ty::Fn(_, result) = self.ty(work) else { unreachable!() };
        let result = *result;
        // What `work` uses from here, in a cell: `{ tree, values… }`. Values
        // go as they are, marked shared (so both threads may hold them, see
        // the runtime's `SHARED`); values holding functions or interface
        // values can't be, and go as copies, in the tree.
        let mut free = BTreeSet::new();
        free_block(body, &mut params.iter().map(|p| p.name.clone()).collect(), &mut free);
        let captures: Vec<Var<'ctx>> = free.iter().filter_map(|n| self.lookup(n)).collect();
        let (shared, copied): (Vec<&Var<'ctx>>, Vec<&Var<'ctx>>) = captures.iter().partition(|v| self.goes_shared(&v.ty));
        let p = self.ptr_ty();
        let i64t = self.i64();
        let tree = self.tree_call("tsl_tree_array", &[], Some("ptr"), &[]).unwrap().into_pointer_value();
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

        // The worker: `ptr tsl_bg_N(ptr input, i64 job)`.
        self.closures += 1;
        let worker =
            self.module.add_function(&format!("tsl_bg_{}", self.closures), p.fn_type(&[p.into(), i64t.into()], false), Some(Linkage::Internal));
        let copied_names: Vec<(String, Ty)> = copied.iter().map(|v| (v.name.clone(), v.ty.clone())).collect();
        let shared_names: Vec<(String, Ty)> = shared.iter().map(|v| (v.name.clone(), v.ty.clone())).collect();
        let takes_job = !params.is_empty();
        let work_t = Ty::func(if takes_job { vec![builtins::JOB] } else { vec![] }, result.clone());
        let job_st = self.llty(&builtins::JOB).unwrap().into_struct_type();
        let result_shared = self.goes_shared(&result);
        self.in_helper(worker, |g| {
            let cell = worker.get_nth_param(0).unwrap().into_pointer_value();
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
            let free = g.declare_rt("tsl_tree_free", &[p.into()], None);
            g.b.build_call(free, &[tree.into()], "").unwrap();
            for (i, (name, ty)) in shared_names.iter().enumerate() {
                let fp = g.b.build_struct_gep(cell_st, cell, i as u32 + 1, "").unwrap();
                let v = g.load(ty, fp);
                g.declare_value(name, ty, Some(v));
            }
            let free_cell = g.declare_rt("tsl_frame_free", &[p.into()], None);
            g.b.build_call(free_cell, &[cell.into()], "").unwrap();
            // `work` as a closure over those copies, so `return` works as in any block.
            let f = g.closure(params, body, &work_t);
            let job_args: Vec<Val<'ctx>> = if takes_job {
                let id = worker.get_nth_param(1).unwrap();
                vec![g.b.build_insert_value(job_st.get_undef(), id, 0, "").unwrap().into_struct_value().into()]
            } else {
                vec![]
            };
            let v = g.call_fn_value(f, &work_t, &job_args).unwrap();
            g.release(f, &work_t);
            let out = if result_shared {
                // The result, moved into a cell of its own, and marked shared.
                let lt = g.llty(&result).unwrap();
                let new_cell = g.declare_rt("tsl_frame_new", &[g.i64().into()], Some(p.into()));
                let out = g.b.build_call(new_cell, &[lt.size_of().unwrap().into()], "").unwrap().try_as_basic_value().basic().unwrap();
                g.store(&result, out.into_pointer_value(), v);
                if let Some(h) = g.share_helper(&result) {
                    g.b.build_call(h, &[out.into()], "").unwrap();
                }
                out
            } else {
                let vp = g.spill(&result, v);
                let enc = g.enc_helper(&result);
                let out = g.b.build_call(enc, &[vp.into()], "").unwrap().try_as_basic_value().basic().unwrap();
                g.release(v, &result);
                out
            };
            g.finish_function(Some(out));
        });

        // The done block, called on the UI thread with the result tree.
        let dt = Ty::func(vec![result.clone()], Ty::Void);
        let closure = self.expr_as(done, &dt).unwrap();
        let bx = self.box_closure(&dt, closure);
        let adapter = self.background_adapter(&result);
        let discard = self.background_discard(&result);
        // The progress block, `void fn(env, Float, String)`, or null.
        let (pf, penv): (Val, Val) = match progress {
            Some(x) => {
                let c = self.expr_as(x, &Ty::func(vec![Ty::Float, Ty::Str], Ty::Void)).unwrap().into_struct_value();
                (self.b.build_extract_value(c, 0, "").unwrap(), self.b.build_extract_value(c, 1, "").unwrap())
            }
            None => (p.const_null().into(), p.const_null().into()),
        };
        let f = self.declare_rt("tsl_background", &[p.into(), p.into(), p.into(), p.into(), p.into(), p.into(), p.into()], Some(i64t.into()));
        let wp = worker.as_global_value().as_pointer_value();
        let id = self
            .b
            .build_call(f, &[wp.into(), input.into(), adapter.into(), bx.into(), pf.into(), penv.into(), discard.into()], "")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap();
        self.b.build_insert_value(job_st.get_undef(), id, 0, "").unwrap().into_struct_value().into()
    }

    /// `adapter(box, tree)`: decodes a `background` result and calls the `fn(T)` block in the box.
    fn background_adapter(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let name = format!("tsl_h_bg_done_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[p.into(), p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let ft = Ty::func(vec![t.clone()], Ty::Void);
        let t = t.clone();
        let shared = self.goes_shared(&t);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let tree = f.get_nth_param(1).unwrap().into_pointer_value();
            let closure = g.load(&ft, g.box_data(bx));
            let v = if shared {
                // (A cell, not a tree: see `background`.)
                let v = g.load(&t, tree);
                let free = g.declare_rt("tsl_frame_free", &[p.into()], None);
                g.b.build_call(free, &[tree.into()], "").unwrap();
                v
            } else {
                let lt = g.llty(&t).unwrap();
                let out = g.alloca(lt, "result");
                g.decode_via(&t, tree, out);
                let free = g.declare_rt("tsl_tree_free", &[p.into()], None);
                g.b.build_call(free, &[tree.into()], "").unwrap();
                g.load(&t, out)
            };
            g.call_fn_value(closure, &ft, &[v]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// Whether a `background` job's value of type `t` (a value it uses, or
    /// its result) goes to the other thread as it is, marked shared, rather
    /// than copied.
    fn goes_shared(&mut self, t: &Ty) -> bool {
        self.shareable(t) && self.llty(t).is_some()
    }

    /// `void discard(ptr)`: frees a `background` result its block won't get.
    fn background_discard(&mut self, t: &Ty) -> PointerValue<'ctx> {
        let p = self.ptr_ty();
        if !self.goes_shared(t) {
            return self.declare_rt("tsl_tree_free", &[p.into()], None).as_global_value().as_pointer_value();
        }
        let name = format!("tsl_h_bg_discard_{}", self.key(t));
        if let Some(f) = self.helpers.get(&name) {
            return f.as_global_value().as_pointer_value();
        }
        let f = self.module.add_function(&name, self.ctx.void_type().fn_type(&[p.into()], false), Some(Linkage::Internal));
        self.helpers.insert(name, f);
        let t = t.clone();
        self.in_helper(f, |g| {
            let cell = f.get_nth_param(0).unwrap().into_pointer_value();
            let v = g.load(&t, cell);
            g.release(v, &t);
            let free = g.declare_rt("tsl_frame_free", &[p.into()], None);
            g.b.build_call(free, &[cell.into()], "").unwrap();
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

    /// Returns `v` from the current function, releasing what's held and
    /// every variable in scope, as `return` does.
    fn early_return(&mut self, v: Option<Val<'ctx>>) {
        self.release_held(0);
        self.release_all_scopes();
        self.emit_return(v);
        self.continue_dead();
    }

    fn case_index(&self, id: usize, name: &str) -> usize {
        self.checked.enums[id].cases.iter().position(|c| c.name == name).expect("Result has this case")
    }

    /// `try x`: the value of a `Result` (none for an `Error?`), or the failure
    /// goes where the checker says (returned, or `fn main` exits with 1).
    fn try_expr(&mut self, e: &Expr, inner: &Expr) -> Option<Val<'ctx>> {
        let it = self.ty(inner);
        let v = self.expr(inner).unwrap();
        let s = v.into_struct_value();
        let (fail_bb, ok_bb) = (self.block_named("failed"), self.block_named("ok"));
        // Whether it failed, and (in the failed block) the `Error`, owned.
        let result_id = match it {
            Ty::Enum(id) => Some(id),
            _ => None,
        };
        let failed = match result_id {
            Some(id) => {
                let tag = self.b.build_extract_value(s, 0, "").unwrap().into_int_value();
                let f = self.ctx.i32_type().const_int(self.case_index(id, "failure") as u64, false);
                self.b.build_int_compare(IntPredicate::EQ, tag, f, "failed").unwrap()
            }
            None => self.b.build_extract_value(s, 0, "").unwrap().into_int_value(),
        };
        self.b.build_conditional_branch(failed, fail_bb, ok_bb).unwrap();

        self.b.position_at_end(fail_bb);
        let err_ty = match &it {
            Ty::Opt(inner) => (**inner).clone(),
            _ => {
                let id = result_id.unwrap();
                self.checked.enums[id].cases[self.case_index(id, "failure")].fields[0].ty.clone()
            }
        };
        let err = match result_id {
            Some(id) => {
                let pt = self.payload_ty(id, self.case_index(id, "failure"));
                let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
                let data = self.box_data(bx);
                let fp = self.b.build_struct_gep(pt, data, 0, "").unwrap();
                let err = self.load(&err_ty, fp);
                self.retain(err, &err_ty);
                self.release(v, &it);
                err
            }
            // The `Error?`'s value moves out.
            None => self.b.build_extract_value(s, 1, "").unwrap(),
        };
        match self.checked.tries.get(&e.id).cloned().expect("checked: a try has a target") {
            TryTarget::Result(rt) => {
                let Ty::Enum(rid) = rt else { unreachable!() };
                let case = self.case_index(rid, "failure");
                let pt = self.payload_ty(rid, case);
                let info = self.payload_info(rid, case);
                let bx = self.call_rt("tsl_box_new", &[info.into()]).unwrap().into_pointer_value();
                let data = self.box_data(bx);
                let fp = self.b.build_struct_gep(pt, data, 0, "").unwrap();
                self.b.build_store(fp, err).unwrap();
                let st = self.llty(&rt).unwrap().into_struct_type();
                let tag = self.ctx.i32_type().const_int(case as u64, false);
                let r = self.b.build_insert_value(st.get_undef(), tag, 0, "").unwrap();
                let r = self.b.build_insert_value(r, bx, 1, "").unwrap().into_struct_value();
                self.early_return(Some(r.into()));
            }
            TryTarget::ErrorOpt(rt) => {
                let some = self.some(&rt, err);
                self.early_return(Some(some));
            }
            TryTarget::Exit => {
                // `error: <message>` on stderr, and exit status 1.
                let msg = self.b.build_extract_value(err.into_struct_value(), 0, "").unwrap();
                let p = self.ptr_ty();
                let print = self.declare_rt("tsl_print_to", &[self.i8().into(), p.into(), p.into()], None);
                let (prefix, empty, newline) = (self.str_lit("error: "), self.str_lit(""), self.str_lit("\n"));
                let one = self.i8().const_int(1, false);
                self.b.build_call(print, &[one.into(), prefix.into(), empty.into()], "").unwrap();
                self.b.build_call(print, &[one.into(), msg.into(), newline.into()], "").unwrap();
                self.call_rt_generic("tsl_exit", &[(self.i64().const_int(1, false).into(), Ty::Int)], &Ty::Void);
                self.b.build_unreachable().unwrap();
                self.continue_dead();
            }
        }

        // (Nothing follows the failure path: close the block `return` left.)
        self.b.build_unreachable().unwrap();
        self.b.position_at_end(ok_bb);
        let id = result_id?;
        let ok = self.case_index(id, "ok");
        let vt = self.checked.enums[id].cases[ok].fields[0].ty.clone();
        let pt = self.payload_ty(id, ok);
        let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
        let data = self.box_data(bx);
        let fp = self.b.build_struct_gep(pt, data, 0, "").unwrap();
        let value = self.load(&vt, fp);
        self.retain(value, &vt);
        self.release(v, &it);
        Some(value)
    }

    /// The method table of `ty` for interface `iface`: its methods, in the
    /// order the interface lists them.
    fn vtable(&mut self, ty: &Ty, iface: usize) -> PointerValue<'ctx> {
        let name = format!("tsl_vt_{}_{}", self.key(ty), self.key(&Ty::Iface(iface)));
        if let Some(g) = self.module.get_global(&name) {
            return g.as_pointer_value();
        }
        let owner = match ty {
            Ty::Struct(id) => self.checked.structs[*id].name.clone(),
            Ty::Enum(id) => self.checked.enums[*id].name.clone(),
            _ => unreachable!("only structs and enums conform"),
        };
        let mut fns: Vec<PointerValue> = self.checked.ifaces[iface]
            .methods
            .iter()
            .map(|(m, _)| self.methods[&(owner.clone(), m.clone())].as_global_value().as_pointer_value())
            .collect();
        // Then, for each required property, a function giving the field's address.
        let props: Vec<String> = self.checked.ifaces[iface].props.iter().map(|(p, _)| p.clone()).collect();
        for p in props {
            let Ty::Struct(id) = ty else { unreachable!("checked: only structs have properties") };
            let f = self.field_thunk(*id, &p);
            fns.push(f.as_global_value().as_pointer_value());
        }
        let arr_ty = self.ptr_ty().array_type(fns.len() as u32);
        let g = self.module.add_global(arr_ty, None, &name);
        g.set_linkage(Linkage::Internal);
        g.set_constant(true);
        g.set_initializer(&self.ptr_ty().const_array(&fns));
        g.as_pointer_value()
    }

    /// Calls method `index` of interface `iface` on interface value `v`
    /// (borrowed): looks the method up in the value's table, and passes it
    /// the boxed value as the receiver.
    fn iface_call<'e>(
        &mut self,
        iface: usize,
        index: usize,
        v: Val<'ctx>,
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
    ) -> Option<Val<'ctx>> {
        let s = v.into_struct_value();
        let vt = self.b.build_extract_value(s, 0, "").unwrap().into_pointer_value();
        let bx = self.b.build_extract_value(s, 1, "").unwrap().into_pointer_value();
        let recv = self.box_data(bx);
        self.iface_invoke(iface, index, vt, recv, info, arg)
    }

    /// Calls method `index` with receiver `recv` (the boxed value), found in method table `vt`.
    fn iface_invoke<'e>(
        &mut self,
        iface: usize,
        index: usize,
        vt: PointerValue<'ctx>,
        recv: PointerValue<'ctx>,
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
    ) -> Option<Val<'ctx>> {
        let sig = self.checked.ifaces[iface].methods[index].1.clone();
        let mut vals = Vec::new();
        for (i, p) in sig.params.iter().enumerate() {
            let v = self.arg_val(info.args[i].expect("interface methods have no defaults"), &p.ty, arg);
            self.hold(v, &p.ty);
            vals.push(v);
        }
        self.unhold(vals.len());
        let ptys: Vec<Ty> = sig.params.iter().map(|p| p.ty.clone()).collect();
        let fn_ty = self.fn_type(&ptys, &sig.ret, true);
        let fp = self.table_entry(vt, index);
        let mut all: Vec<BasicMetadataValueEnum> = vec![self.ptr_ty().const_null().into(), recv.into()];
        all.extend(self.abi_args(&vals, &ptys));
        let place = self.result_place(&sig.ret, &mut all);
        let r = self.b.build_indirect_call(fn_ty, fp, &all, "").unwrap().try_as_basic_value().basic();
        self.call_result(&sig.ret, place, r)
    }

    /// Entry `index` of a method table.
    fn table_entry(&mut self, vt: PointerValue<'ctx>, index: usize) -> PointerValue<'ctx> {
        let slot = unsafe { self.b.build_gep(self.ptr_ty(), vt, &[self.i64().const_int(index as u64, false)], "").unwrap() };
        self.b.build_load(self.ptr_ty(), slot, "entry").unwrap().into_pointer_value()
    }

    fn iface_prop_ty(&self, iface: usize, name: &str) -> Ty {
        self.checked.ifaces[iface].props.iter().find(|(p, _)| p == name).unwrap().1.clone()
    }

    /// The address of required property `name` inside the boxed value `recv`.
    fn iface_prop_ptr(&mut self, iface: usize, name: &str, vt: PointerValue<'ctx>, recv: PointerValue<'ctx>) -> PointerValue<'ctx> {
        let def = &self.checked.ifaces[iface];
        let index = def.methods.len() + def.props.iter().position(|(p, _)| p == name).unwrap();
        let f = self.table_entry(vt, index);
        let fn_ty = self.ptr_ty().fn_type(&[self.ptr_ty().into()], false);
        let r = self.b.build_indirect_call(fn_ty, f, &[recv.into()], "field").unwrap();
        r.try_as_basic_value().basic().unwrap().into_pointer_value()
    }

    /// `ptr field(ptr value)`: the address of struct `id`'s field `name`.
    fn field_thunk(&mut self, id: usize, name: &str) -> FunctionValue<'ctx> {
        let fname = format!("tsl_field_{}_{name}", self.key(&Ty::Struct(id)));
        if let Some(f) = self.module.get_function(&fname) {
            return f;
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(&fname, p.fn_type(&[p.into()], false), Some(Linkage::Internal));
        let idx = self.field_index(id, name);
        self.in_helper(f, |g| {
            let recv = f.get_nth_param(0).unwrap().into_pointer_value();
            let st = g.struct_ty(id);
            let fp = g.b.build_struct_gep(st, recv, idx, "").unwrap();
            g.b.build_return(Some(&fp)).unwrap();
        });
        f
    }

    /// Before changing the interface value stored at `slot`: gives it a box
    /// of its own (copy-on-write). Returns its method table and the boxed value.
    fn iface_unique(&mut self, iface: usize, slot: PointerValue<'ctx>) -> (PointerValue<'ctx>, PointerValue<'ctx>) {
        let lt = self.llty(&Ty::Iface(iface)).unwrap().into_struct_type();
        let vt_p = self.b.build_struct_gep(lt, slot, 0, "").unwrap();
        let bx_p = self.b.build_struct_gep(lt, slot, 1, "").unwrap();
        let vt = self.b.build_load(self.ptr_ty(), vt_p, "").unwrap().into_pointer_value();
        let bx = self.call_rt("tsl_box_unique", &[bx_p.into()]).unwrap().into_pointer_value();
        (vt, self.box_data(bx))
    }

    /// `adapter(box, i64 flag)`: calls the `fn(Bool)` block in the box.
    fn bool_adapter(&mut self) -> PointerValue<'ctx> {
        let name = "tsl_h_bool_done";
        if let Some(f) = self.helpers.get(name) {
            return f.as_global_value().as_pointer_value();
        }
        let p = self.ptr_ty();
        let f = self.module.add_function(name, self.ctx.void_type().fn_type(&[p.into(), self.i64().into()], false), Some(Linkage::Internal));
        self.helpers.insert(name.to_string(), f);
        let ft = Ty::func(vec![Ty::Bool], Ty::Void);
        self.in_helper(f, |g| {
            let bx = f.get_nth_param(0).unwrap().into_pointer_value();
            let flag = f.get_nth_param(1).unwrap().into_int_value();
            let closure = g.load(&ft, g.box_data(bx));
            let b = g.b.build_int_compare(IntPredicate::NE, flag, g.i64().const_zero(), "").unwrap();
            g.call_fn_value(closure, &ft, &[b.into()]);
            g.b.build_return(None).unwrap();
        });
        f.as_global_value().as_pointer_value()
    }

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

    /// The C type a value of type `t` is passed to (or returned from) the runtime as.
    fn c_type(&mut self, t: &Ty) -> BasicTypeEnum<'ctx> {
        match t {
            Ty::Bool => self.i8().into(),
            Ty::Int => self.i64().into(),
            Ty::Float => self.ctx.f64_type().into(),
            _ => self.ptr_ty().into(),
        }
    }

    /// Declares (once) a runtime function with the given C signature.
    fn declare_rt(&mut self, name: &str, params: &[BasicMetadataTypeEnum<'ctx>], ret: Option<BasicTypeEnum<'ctx>>) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let fty = match ret {
            Some(r) => r.fn_type(params, false),
            None => self.ctx.void_type().fn_type(params, false),
        };
        self.module.add_function(name, fty, None)
    }

    /// `data[i]`: `tsl_data_get(data, i, location)`.
    fn call_rt_generic_loc(&mut self, name: &str, d: Val<'ctx>, i: Val<'ctx>, loc: PointerValue<'ctx>) -> Val<'ctx> {
        let p = self.ptr_ty();
        let f = self.declare_rt(name, &[p.into(), self.i64().into(), p.into()], Some(self.i64().into()));
        self.b.build_call(f, &[d.into(), i.into(), loc.into()], "").unwrap().try_as_basic_value().basic().unwrap()
    }

    /// Calls a table-driven runtime function (see `builtins::RtCall`) with
    /// already evaluated arguments (borrowed), and returns the result as `ret`.
    fn call_rt_generic(&mut self, rt: &str, args: &[(Val<'ctx>, Ty)], ret: &Ty) -> Option<Val<'ctx>> {
        self.call_rt_located(rt, args, ret, None)
    }

    /// Like `call_rt_generic`; with `at`, the call's place in the source goes
    /// after the arguments, for the message of a runtime error.
    fn call_rt_located(&mut self, rt: &str, args: &[(Val<'ctx>, Ty)], ret: &Ty, at: Option<Span>) -> Option<Val<'ctx>> {
        // An optional Int/Float comes back through an out pointer.
        let out_param = match ret {
            Ty::Opt(inner) if matches!(**inner, Ty::Int | Ty::Float | Ty::Bool) => Some((**inner).clone()),
            _ => None,
        };
        let f = match self.module.get_function(rt) {
            Some(f) => f,
            None => {
                // A Range goes as two Ints (start, end).
                let mut ps: Vec<BasicMetadataTypeEnum> = Vec::new();
                for (_, t) in args {
                    match t {
                        Ty::Range => {
                            let i = BasicMetadataTypeEnum::from(self.i64());
                            ps.extend([i, i]);
                        }
                        // An enum goes as its case's place, then the box of the case's values if it has cases with some.
                        Ty::Enum(id) => {
                            ps.push(self.ctx.i32_type().into());
                            if self.has_payload(*id) {
                                ps.push(self.ptr_ty().into());
                            }
                        }
                        _ => ps.push(self.c_type(t).into()),
                    }
                }
                if at.is_some() {
                    ps.push(self.ptr_ty().into());
                }
                if out_param.is_some() {
                    ps.push(self.ptr_ty().into());
                }
                let fty = match (ret, &out_param) {
                    (Ty::Void, _) => self.ctx.void_type().fn_type(&ps, false),
                    (_, Some(_)) => self.i8().fn_type(&ps, false),
                    (Ty::Opt(_), None) => self.ptr_ty().fn_type(&ps, false),
                    _ => self.c_type(ret).fn_type(&ps, false),
                };
                self.module.add_function(rt, fty, None)
            }
        };
        let mut vals: Vec<BasicMetadataValueEnum> = Vec::new();
        for (v, t) in args {
            if *t == Ty::Range {
                let r = v.into_struct_value();
                for i in 0..2 {
                    vals.push(self.b.build_extract_value(r, i, "").unwrap().into());
                }
                continue;
            }
            if let Ty::Enum(id) = t {
                if self.has_payload(*id) {
                    let e = v.into_struct_value();
                    for i in 0..2 {
                        vals.push(self.b.build_extract_value(e, i, "").unwrap().into());
                    }
                    continue;
                }
            }
            let v = if *t == Ty::Bool { self.to_c_bool(v.into_int_value()) } else { *v };
            vals.push(v.into());
        }
        if let Some(span) = at {
            vals.push(self.loc(span).into());
        }
        let out = out_param.as_ref().map(|inner| {
            let lt = self.llty(inner).unwrap();
            let slot = self.alloca(lt, "out");
            self.b.build_store(slot, lt.const_zero()).unwrap();
            slot
        });
        if let Some(slot) = out {
            vals.push(slot.into());
        }
        let r = self.b.build_call(f, &vals, "").unwrap().try_as_basic_value().basic();
        Some(match (ret, out_param) {
            (Ty::Void, _) => return None,
            (_, Some(inner)) => {
                let present = self.truthy(r);
                let v = self.load(&inner, out.unwrap());
                let st = self.llty(ret).unwrap().into_struct_type();
                let s = self.b.build_insert_value(st.get_undef(), present, 0, "").unwrap();
                self.b.build_insert_value(s, v, 1, "").unwrap().into_struct_value().into()
            }
            (Ty::Opt(_), None) => self.opt_from_ptr(r.unwrap().into_pointer_value(), ret),
            (Ty::Bool, _) => self.truthy(r).into(),
            _ => r.unwrap(),
        })
    }

    /// A default argument of a table-driven runtime call.
    fn default_value(&mut self, d: builtins::Dflt, t: &Ty) -> Val<'ctx> {
        match d {
            builtins::Dflt::Int(n) => self.i64().const_int(n as u64, true).into(),
            builtins::Dflt::Float(x) => self.ctx.f64_type().const_float(x).into(),
            builtins::Dflt::Bool(b) => self.ctx.bool_type().const_int(b as u64, false).into(),
            builtins::Dflt::Str(s) => self.str_lit(s).into(),
            builtins::Dflt::EmptyList => {
                let Ty::List(elem) = t else { unreachable!("EmptyList default for a non-list") };
                let info = self.type_info(elem);
                let zero = self.i64().const_zero();
                self.call_rt("tsl_list_new", &[info.into(), zero.into()]).unwrap()
            }
            builtins::Dflt::Case(case) => {
                let Ty::Enum(id) = t else { unreachable!("Case default for a non-enum") };
                let tag = self.ctx.i32_type().const_int(case as u64, false);
                if self.has_payload(*id) {
                    let st = self.llty(t).unwrap().into_struct_type();
                    let s = self.b.build_insert_value(st.get_undef(), tag, 0, "").unwrap();
                    self.b.build_insert_value(s, self.ptr_ty().const_null(), 1, "").unwrap().into_struct_value().into()
                } else {
                    tag.into()
                }
            }
            builtins::Dflt::Req => unreachable!("required argument missing"),
        }
    }

    fn builtin_fn<'e>(
        &mut self,
        name: &str,
        args: &'e [Arg],
        info: &CallInfo,
        arg: &dyn Fn(ArgRef) -> &'e Expr,
        t: &Ty,
        span: Span,
    ) -> Option<Val<'ctx>> {
        match name {
            "zip" => {
                let Ty::List(pair) = t else { unreachable!() };
                let Ty::Struct(id) = **pair else { unreachable!() };
                let fields: Vec<Ty> = self.checked.structs[id].fields.iter().map(|f| f.ty.clone()).collect();
                let (at, bt) = (Ty::list(fields[0].clone()), Ty::list(fields[1].clone()));
                let a = self.expr_as(&args[0].value, &at).unwrap();
                self.hold(a, &at);
                let b = self.expr_as(&args[1].value, &bt).unwrap();
                self.unhold(1);
                let na = self.call_rt("tsl_list_len", &[a]).unwrap().into_int_value();
                let nb = self.call_rt("tsl_list_len", &[b]).unwrap().into_int_value();
                let shorter = self.b.build_int_compare(IntPredicate::SLT, na, nb, "").unwrap();
                let n = self.b.build_select(shorter, na, nb, "").unwrap().into_int_value();
                let r = self.build_pairs(n, pair, &mut |g, i| (g.item_at(a, i, &fields[0]), g.item_at(b, i, &fields[1])));
                self.release(a, &at);
                self.release(b, &bt);
                Some(r)
            }
            // `Set(list)`, `Set<Int>()`
            "Set" => {
                let Ty::Set(elem) = t else { unreachable!() };
                let bi = self.type_info(&Ty::Bool);
                Some(match args.first() {
                    Some(a) => {
                        let lt = Ty::list((**elem).clone());
                        let list = self.expr_as(&a.value, &lt).unwrap();
                        let set = self.call_rt("tsl_set_from_list", &[list, bi.into()]).unwrap();
                        self.release(list, &lt);
                        set
                    }
                    None => {
                        let ki = self.type_info(elem);
                        self.call_rt("tsl_dict_new", &[ki.into(), bi.into()]).unwrap()
                    }
                })
            }
            "scrollTo" => {
                // Ids are kept as numbers: a `String`'s hash (as `.id` does).
                let x = arg(info.args[0].unwrap());
                let t = self.ty(x);
                let v = self.expr(x).unwrap();
                let id = match t {
                    Ty::Str => self.call_rt("tsl_str_hash", &[v]).unwrap(),
                    _ => v,
                };
                if t == Ty::Str {
                    self.release(v, &t);
                }
                let align = match info.args.get(1).copied().flatten() {
                    Some(r) => self.expr(arg(r)).unwrap(),
                    None => self.ctx.i32_type().const_zero().into(),
                };
                self.call_rt("tsl_scroll_to", &[id, align]);
                None
            }
            "print" | "printError" => {
                // The values' text, with the separator between them.
                let Some(ArgRef::Many(from, count)) = info.args[0] else { unreachable!("checked: print's values") };
                let sep = match info.args.get(1).copied().flatten() {
                    Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                    None => self.str_lit(" ").into(),
                };
                self.hold(sep, &Ty::Str);
                let mut parts = Vec::new();
                for i in from..from + count {
                    let x = arg(ArgRef::Arg(i));
                    let xt = self.ty(x);
                    let v = self.expr(x).unwrap();
                    let s = self.stringify(v, &xt);
                    self.hold(s, &Ty::Str);
                    parts.push(s);
                }
                let s = match parts.len() {
                    0 => self.str_lit("").into(),
                    1 => parts[0],
                    _ => {
                        let mut pieces = Vec::new();
                        for (k, p) in parts.iter().enumerate() {
                            if k > 0 {
                                pieces.push(sep);
                            }
                            pieces.push(*p);
                        }
                        let joined = self.concat(&pieces);
                        for p in &parts {
                            self.release(*p, &Ty::Str);
                        }
                        joined
                    }
                };
                self.unhold(parts.len() + 1);
                self.release(sep, &Ty::Str);
                self.hold(s, &Ty::Str);
                let end = match info.args.get(2).copied().flatten() {
                    Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                    None => self.str_lit("\n").into(),
                };
                self.unhold(1);
                let err = self.i8().const_int((name == "printError") as u64, false);
                let p = self.ptr_ty();
                let f = self.declare_rt("tsl_print_to", &[self.i8().into(), p.into(), p.into()], None);
                self.b.build_call(f, &[err.into(), s.into(), end.into()], "").unwrap();
                self.release(s, &Ty::Str);
                self.release(end, &Ty::Str);
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
            _ if MATH_FNS.contains(&name) && *t == Ty::Float => {
                // Math arguments are always positional (`pow(x, 2.0)`).
                let a = self.expr_as(&args[0].value, &Ty::Float).unwrap();
                let b = args.get(1).map(|x| self.expr_as(&x.value, &Ty::Float).unwrap());
                // LLVM's own operations where it has them (so they're
                // optimized, and `sqrt` is one instruction); else the C library.
                let f = match name {
                    "sqrt" => "llvm.sqrt.f64",
                    "pow" => "llvm.pow.f64",
                    "exp" => "llvm.exp.f64",
                    "log" => "llvm.log.f64",
                    "log10" => "llvm.log10.f64",
                    "sin" => "llvm.sin.f64",
                    "cos" => "llvm.cos.f64",
                    "floor" => "llvm.floor.f64",
                    "ceil" => "llvm.ceil.f64",
                    "round" => "llvm.round.f64",
                    "abs" => "llvm.fabs.f64",
                    "min" => "llvm.minnum.f64",
                    "max" => "llvm.maxnum.f64",
                    other => other,
                };
                let f64t = self.ctx.f64_type();
                let n = if b.is_some() { 2 } else { 1 };
                let func = self.module.get_function(f).unwrap_or_else(|| {
                    let params: Vec<BasicMetadataTypeEnum> = vec![f64t.into(); n];
                    self.module.add_function(f, f64t.fn_type(&params, false), None)
                });
                let mut call_args: Vec<BasicMetadataValueEnum> = vec![a.into()];
                if let Some(b) = b {
                    call_args.push(b.into());
                }
                let mut r = self.b.build_call(func, &call_args, "").unwrap().try_as_basic_value().basic();
                // `min` and `max` of more than two values, from the left.
                for x in args.iter().skip(2) {
                    let c = self.expr_as(&x.value, &Ty::Float).unwrap();
                    r = self.b.build_call(func, &[r.unwrap().into(), c.into()], "").unwrap().try_as_basic_value().basic();
                }
                r
            }
            // `abs`, `min`, `max` on Ints.
            "abs" | "min" | "max" => {
                let a = self.expr_as(&args[0].value, &Ty::Int).unwrap().into_int_value();
                if name == "abs" {
                    let neg = self.b.build_int_neg(a, "").unwrap();
                    let is_neg = self.b.build_int_compare(IntPredicate::SLT, a, self.i64().const_zero(), "").unwrap();
                    return Some(self.b.build_select(is_neg, neg, a, "").unwrap());
                }
                // (Any number of values, from the left.)
                let pred = if name == "min" { IntPredicate::SLT } else { IntPredicate::SGT };
                let mut best = a;
                for x in &args[1..] {
                    let b = self.expr_as(&x.value, &Ty::Int).unwrap().into_int_value();
                    let keep = self.b.build_int_compare(pred, best, b, "").unwrap();
                    best = self.b.build_select(keep, best, b, "").unwrap().into_int_value();
                }
                Some(best.into())
            }
            "openFile" | "saveFile" => {
                // openFile(title:types:), saveFile(title:name:types:)
                let texts: &[usize] = if name == "openFile" { &[0] } else { &[0, 1] };
                let mut strs = Vec::new();
                for &i in texts {
                    strs.push(match info.args[i] {
                        Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                        None => self.str_lit("").into(),
                    });
                }
                if name == "openFile" {
                    strs.push(self.str_lit("").into());
                }
                let list_t = Ty::list(Ty::Str);
                let types = match info.args[texts.len()] {
                    Some(r) => self.expr_as(arg(r), &list_t).unwrap(),
                    None => {
                        let info_ptr = self.type_info(&Ty::Str);
                        let zero = self.i64().const_zero();
                        self.call_rt("tsl_list_new", &[info_ptr.into(), zero.into()]).unwrap()
                    }
                };
                let save = self.i8().const_int((name == "saveFile") as u64, false);
                let p = self.call_rt("tsl_file_dialog", &[save.into(), strs[0], strs[1], types]).unwrap().into_pointer_value();
                for v in strs {
                    self.release(v, &Ty::Str);
                }
                self.release(types, &list_t);
                Some(self.opt_from_ptr(p, t))
            }
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
                // (`input: true`: the program's input stays open, for `sendToCommand`.)
                let input = match info.args[2] {
                    Some(r) => {
                        let on = self.expr_as(arg(r), &Ty::Bool).unwrap().into_int_value();
                        self.to_c_bool(on)
                    }
                    None => self.i8().const_zero().into(),
                };
                let on_out = self.expr_as(arg(info.args[3].unwrap()), &out_t).unwrap().into_struct_value();
                let on_exit = self.expr_as(arg(info.args[4].unwrap()), &exit_t).unwrap().into_struct_value();
                let mut all = vec![path, args, input];
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
            "every" => {
                let secs = self.expr_as(arg(info.args[0].unwrap()), &Ty::Float).unwrap();
                let action = self.expr_as(arg(info.args[1].unwrap()), &Ty::func(vec![], Ty::Void)).unwrap();
                let s = action.into_struct_value();
                let f = self.b.build_extract_value(s, 0, "").unwrap();
                let env = self.b.build_extract_value(s, 1, "").unwrap();
                let p = self.ptr_ty();
                let rt = self.declare_rt("tsl_every", &[self.ctx.f64_type().into(), p.into(), p.into()], Some(self.i64().into()));
                // The runtime keeps the closure (its reference moves there).
                self.b.build_call(rt, &[secs.into(), f.into(), env.into()], "").unwrap().try_as_basic_value().basic()
            }
            "request" => {
                let url = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let method = match info.args[1] {
                    Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                    None => self.str_lit("GET").into(),
                };
                let dict_t = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Str));
                let headers = match info.args[2] {
                    Some(r) => self.expr_as(arg(r), &dict_t).unwrap(),
                    None => {
                        let si = self.type_info(&Ty::Str);
                        self.call_rt("tsl_dict_new", &[si.into(), si.into()]).unwrap()
                    }
                };
                let body = match info.args[3] {
                    Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                    None => self.str_lit("").into(),
                };
                let ft = Ty::func(vec![builtins::HTTP_RESPONSE], Ty::Void);
                let done = self.expr_as(arg(info.args[4].unwrap()), &ft).unwrap();
                let bx = self.box_closure(&ft, done);
                let adapter = self.response_adapter();
                let p = self.ptr_ty();
                let f = self.declare_rt(
                    "tsl_request",
                    &[p.into(), p.into(), p.into(), p.into(), p.into(), p.into()],
                    Some(self.i64().into()),
                );
                let args = [url.into(), method.into(), headers.into(), body.into(), adapter.into(), bx.into()];
                let id = self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic();
                self.release(url, &Ty::Str);
                self.release(method, &Ty::Str);
                self.release(headers, &dict_t);
                self.release(body, &Ty::Str);
                id
            }
            "download" => {
                let url = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let to = self.expr_as(arg(info.args[1].unwrap()), &Ty::Str).unwrap();
                let pt = Ty::opt(Ty::func(vec![Ty::Float], Ty::Void));
                // The progress block, or null for none.
                let (pf, penv): (Val, Val) = match info.args[2] {
                    Some(r) => {
                        let opt = self.expr_as(arg(r), &pt).unwrap().into_struct_value();
                        let present = self.b.build_extract_value(opt, 0, "").unwrap().into_int_value();
                        let closure = self.b.build_extract_value(opt, 1, "").unwrap().into_struct_value();
                        let f = self.b.build_extract_value(closure, 0, "").unwrap().into_pointer_value();
                        let env = self.b.build_extract_value(closure, 1, "").unwrap();
                        let null = self.ptr_ty().const_null();
                        (self.b.build_select(present, f, null, "").unwrap(), env)
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                let dt = Ty::func(vec![Ty::Bool], Ty::Void);
                let done = self.expr_as(arg(info.args[3].unwrap()), &dt).unwrap();
                let bx = self.box_closure(&dt, done);
                let adapter = self.bool_adapter();
                let p = self.ptr_ty();
                let f = self.declare_rt(
                    "tsl_download",
                    &[p.into(), p.into(), p.into(), p.into(), p.into(), p.into()],
                    Some(self.i64().into()),
                );
                let args = [url.into(), to.into(), pf.into(), penv.into(), adapter.into(), bx.into()];
                let id = self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic();
                self.release(url, &Ty::Str);
                self.release(to, &Ty::Str);
                id
            }
            "openWebSocket" => {
                let url = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let ft = Ty::func(vec![Ty::Str], Ty::Void);
                let on_msg = self.expr_as(arg(info.args[1].unwrap()), &ft).unwrap().into_struct_value();
                // The block for binary messages, or null for none.
                let dt = Ty::opt(Ty::func(vec![Ty::Data], Ty::Void));
                let (data_f, data_env): (Val, Val) = match info.args[2] {
                    Some(r) => {
                        let opt = self.expr_as(arg(r), &dt).unwrap().into_struct_value();
                        let present = self.b.build_extract_value(opt, 0, "").unwrap().into_int_value();
                        let closure = self.b.build_extract_value(opt, 1, "").unwrap().into_struct_value();
                        let f = self.b.build_extract_value(closure, 0, "").unwrap().into_pointer_value();
                        let env = self.b.build_extract_value(closure, 1, "").unwrap();
                        let null = self.ptr_ty().const_null();
                        (self.b.build_select(present, f, null, "").unwrap(), env)
                    }
                    None => (self.ptr_ty().const_null().into(), self.ptr_ty().const_null().into()),
                };
                let on_close = self.expr_as(arg(info.args[3].unwrap()), &ft).unwrap().into_struct_value();
                let mut vals: Vec<inkwell::values::BasicMetadataValueEnum> = vec![url.into()];
                vals.push(self.b.build_extract_value(on_msg, 0, "").unwrap().into());
                vals.push(self.b.build_extract_value(on_msg, 1, "").unwrap().into());
                vals.push(data_f.into());
                vals.push(data_env.into());
                vals.push(self.b.build_extract_value(on_close, 0, "").unwrap().into());
                vals.push(self.b.build_extract_value(on_close, 1, "").unwrap().into());
                let p = self.ptr_ty();
                let f = self.declare_rt(
                    "tsl_open_web_socket",
                    &[p.into(), p.into(), p.into(), p.into(), p.into(), p.into(), p.into()],
                    Some(self.i64().into()),
                );
                let id = self.b.build_call(f, &vals, "").unwrap().try_as_basic_value().basic();
                self.release(url, &Ty::Str);
                id
            }
            "renderImage" => {
                let path = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let width = self.expr_as(arg(info.args[1].unwrap()), &Ty::Float).unwrap();
                let height = self.expr_as(arg(info.args[2].unwrap()), &Ty::Float).unwrap();
                let scale = match info.args[3] {
                    Some(r) => self.expr_as(arg(r), &Ty::Float).unwrap(),
                    None => self.ctx.f64_type().const_float(1.0).into(),
                };
                // The runtime calls `adapter(box, width, height)`; the box holds the block.
                let ft = Ty::func(vec![builtins::GRAPHICS], Ty::Void);
                let draw = self.expr_as(arg(info.args[4].unwrap()), &ft).unwrap();
                let bx = self.box_closure(&ft, draw);
                let adapter = self.graphics_adapter();
                let (p, f64t) = (self.ptr_ty(), self.ctx.f64_type());
                let f = self.declare_rt(
                    "tsl_render_image",
                    &[p.into(), f64t.into(), f64t.into(), f64t.into(), p.into(), p.into()],
                    Some(self.i8().into()),
                );
                let args = [path.into(), width.into(), height.into(), scale.into(), adapter.into(), bx.into()];
                let saved = self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic();
                self.release(path, &Ty::Str);
                Some(self.truthy(saved).into())
            }
            "writePDF" => {
                let path = self.expr_as(arg(info.args[0].unwrap()), &Ty::Str).unwrap();
                let title = match info.args[1] {
                    Some(r) => self.expr_as(arg(r), &Ty::Str).unwrap(),
                    None => self.str_lit("").into(),
                };
                // A4 when no size is given: 210 × 297 mm, in points.
                let mut size = |i: usize, default: f64| match info.args[i] {
                    Some(r) => self.expr_as(arg(r), &Ty::Float).unwrap(),
                    None => self.ctx.f64_type().const_float(default).into(),
                };
                let width = size(2, 595.28);
                let height = size(3, 841.89);
                let ft = Ty::func(vec![builtins::GRAPHICS], Ty::Void);
                let draw = self.expr_as(arg(info.args[4].unwrap()), &ft).unwrap();
                let bx = self.box_closure(&ft, draw);
                let adapter = self.graphics_adapter();
                let (p, f64t) = (self.ptr_ty(), self.ctx.f64_type());
                let f = self.declare_rt(
                    "tsl_write_pdf",
                    &[p.into(), p.into(), f64t.into(), f64t.into(), p.into(), p.into()],
                    Some(self.i8().into()),
                );
                let args = [path.into(), title.into(), width.into(), height.into(), adapter.into(), bx.into()];
                let saved = self.b.build_call(f, &args, "").unwrap().try_as_basic_value().basic();
                self.release(path, &Ty::Str);
                self.release(title, &Ty::Str);
                Some(self.truthy(saved).into())
            }
            "after" => {
                let secs = self.expr_as(arg(info.args[0].unwrap()), &Ty::Float).unwrap();
                let action = self.expr_as(arg(info.args[1].unwrap()), &Ty::func(vec![], Ty::Void)).unwrap();
                self.pass_closure("tsl_after", &[secs], action);
                None
            }
            "Data" => Some(match args.first().and_then(|a| a.label.as_ref()).map(|l| l.name.as_str()) {
                Some("bytes") => {
                    let lt = Ty::list(Ty::Int);
                    let l = self.expr_as(&args[0].value, &lt).unwrap();
                    let d = self.call_rt_generic("tsl_data_from_ints", &[(l, lt.clone())], &Ty::Data).unwrap();
                    self.release(l, &lt);
                    d
                }
                Some(_) => {
                    let s = self.expr_as(&args[0].value, &Ty::Str).unwrap();
                    let d = self.call_rt_generic("tsl_data_from_str", &[(s, Ty::Str)], &Ty::Data).unwrap();
                    self.release(s, &Ty::Str);
                    d
                }
                None => self.call_rt_generic("tsl_data_empty", &[], &Ty::Data).unwrap(),
            }),
            "background" => Some(self.background(args, arg, span)),
            "random" => {
                // An empty range stops the program, so it needs the location.
                let r = self.expr_as(arg(info.args[0].unwrap()), &Ty::Range).unwrap().into_struct_value();
                let lo = self.b.build_extract_value(r, 0, "").unwrap();
                let hi = self.b.build_extract_value(r, 1, "").unwrap();
                let l = self.loc(span);
                let i = self.i64();
                let f = self.declare_rt("tsl_random_int", &[i.into(), i.into(), self.ptr_ty().into()], Some(i.into()));
                self.b.build_call(f, &[lo.into(), hi.into(), l.into()], "").unwrap().try_as_basic_value().basic()
            }
            "toJson" | "toBinary" => {
                let x = &args[0].value;
                let xt = self.ty(x);
                let v = self.expr(x).unwrap();
                let p = self.spill(&xt, v);
                let enc = self.enc_helper(&xt);
                let tree = self.b.build_call(enc, &[p.into()], "").unwrap().try_as_basic_value().basic().unwrap();
                self.release(v, &xt);
                let ptr = self.ptr_ty();
                let r = if name == "toJson" {
                    let pretty = match args.get(1) {
                        Some(a) => {
                            let b = self.expr_as(&a.value, &Ty::Bool).unwrap().into_int_value();
                            self.to_c_bool(b)
                        }
                        None => self.i8().const_zero().into(),
                    };
                    let f = self.declare_rt("tsl_tree_to_json", &[ptr.into(), self.i8().into()], Some(ptr.into()));
                    self.b.build_call(f, &[tree.into(), pretty.into()], "").unwrap()
                } else {
                    let f = self.declare_rt("tsl_tree_to_binary", &[ptr.into()], Some(ptr.into()));
                    self.b.build_call(f, &[tree.into()], "").unwrap()
                };
                r.try_as_basic_value().basic()
            }
            "fromJson" | "fromBinary" => {
                let Ty::Opt(inner) = t else { unreachable!("fromJson has an optional type") };
                let inner = (**inner).clone();
                let input_t = if name == "fromJson" { Ty::Str } else { Ty::Data };
                let s = self.expr_as(&args[0].value, &input_t).unwrap();
                let ptr = self.ptr_ty();
                let parse = self.declare_rt(if name == "fromJson" { "tsl_json_parse" } else { "tsl_binary_parse" }, &[ptr.into()], Some(ptr.into()));
                let tree = self.b.build_call(parse, &[s.into()], "").unwrap().try_as_basic_value().basic().unwrap().into_pointer_value();
                self.release(s, &input_t);
                let lt = self.llty(t).unwrap();
                let slot = self.alloca(lt, "decoded");
                let none = self.none(t);
                self.b.build_store(slot, none).unwrap();
                let (parsed, done) = (self.block_named("parsed"), self.block_named("decoded"));
                let is_tree = self.b.build_is_not_null(tree, "").unwrap();
                self.b.build_conditional_branch(is_tree, parsed, done).unwrap();
                self.b.position_at_end(parsed);
                let il = self.llty(&inner).unwrap();
                let out = self.alloca(il, "value");
                let (good, freed) = (self.block_named("good"), self.block_named("free"));
                // A plain `null` is nil, whatever the type (a `Float` would
                // otherwise read it as NaN).
                if !matches!(inner, Ty::Opt(_)) {
                    let decode = self.block_named("decode");
                    let is_null = self.kind_is(tree, 0);
                    self.b.build_conditional_branch(is_null, freed, decode).unwrap();
                    self.b.position_at_end(decode);
                }
                let dec = self.dec_helper(&inner);
                let ok = self.b.build_call(dec, &[tree.into(), out.into()], "").unwrap().try_as_basic_value().basic();
                let ok = self.truthy(ok);
                self.b.build_conditional_branch(ok, good, freed).unwrap();
                self.b.position_at_end(good);
                let v = self.load(&inner, out);
                let some = self.some(t, v);
                self.b.build_store(slot, some).unwrap();
                self.br(freed);
                self.b.position_at_end(freed);
                let free = self.declare_rt("tsl_tree_free", &[ptr.into()], None);
                self.b.build_call(free, &[tree.into()], "").unwrap();
                self.br(done);
                self.b.position_at_end(done);
                Some(self.load(t, slot))
            }
            _ => {
                let (Some(call), Some(sig)) = (builtins::runtime_fn(name), builtins::Builtins::standard().function(name)) else {
                    self.unsupported(span, &format!("`{name}`"));
                    return self.undef(t);
                };
                let mut vals = Vec::new();
                for (i, p) in sig.params.iter().enumerate() {
                    let v = match info.args.get(i).copied().flatten() {
                        Some(r) => self.expr_as(arg(r), &p.ty).unwrap(),
                        None => self.default_value(call.defaults.get(i).copied().unwrap_or(builtins::Dflt::Req), &p.ty),
                    };
                    self.hold(v, &p.ty);
                    vals.push((v, p.ty.clone()));
                }
                self.unhold(vals.len());
                let r = self.call_rt_generic(call.rt, &vals, t);
                for (v, ty) in vals {
                    self.release(v, &ty);
                }
                r
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
        ArgRef::Many(..) => unreachable!("a variadic parameter's values are taken one by one (`arg_val`)"),
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
        Ty::List(t) | Ty::Opt(t) | Ty::Set(t) => crate_equatable(t),
        Ty::Dict(k, v) => crate_equatable(k) && crate_equatable(v),
        _ => true,
    }
}

fn hashable(t: &Ty, g: &Gen) -> bool {
    match t {
        Ty::Int | Ty::Str | Ty::Bool => true,
        Ty::Enum(id) => !g.has_payload(*id),
        Ty::Struct(id) => g.checked.structs[*id].fields.iter().all(|f| hashable(&f.ty, g)),
        _ => false,
    }
}

/// `var`s in `body` whose names are used inside closures: they must live in
/// boxes so the closure and the function share them. (Over-approximating is
/// safe; boxing a variable never changes what the program does.)
/// A function's parameter types as passed: a `bind` parameter is a locator.
fn param_lltys(params: &[crate::types::ParamSig]) -> Vec<Ty> {
    params.iter().map(|p| if p.bind { Ty::View } else { p.ty.clone() }).collect()
}

/// The variable a place starts at (`todos` for `todos[0].title`).
fn root_name(e: &Expr) -> Option<&str> {
    let mut root = e;
    while let ExprKind::Member { object: o, .. } | ExprKind::Index(o, _) = &root.kind {
        root = o;
    }
    match &root.kind {
        ExprKind::Name(n) => Some(n),
        _ => None,
    }
}

/// The names of the `var`s declared in `body` (in nested blocks too).
fn declared_vars(body: &Block) -> HashSet<String> {
    let mut vars = HashSet::new();
    walk_block(body, &mut |node| {
        if let Node::S(Stmt::Var(d)) = node {
            vars.insert(d.name.name.clone());
        }
    });
    vars
}

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
        // A function declared inside the function is a closure too.
        Node::S(Stmt::Fn(d)) => free_fn(d, &mut vec![d.name.name.clone()], &mut captured),
        // A loop over `outer[i].items` finds its list through a locator,
        // which must share the variable rather than copy it.
        Node::S(Stmt::For { iter, .. }) if through_index(iter) => {
            let mut root = iter;
            while let ExprKind::Member { object: o, .. } | ExprKind::Index(o, _) = &root.kind {
                root = o;
            }
            if let ExprKind::Name(n) = &root.kind {
                captured.insert(n.clone());
            }
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
        Stmt::While { cond, body, .. } => {
            match cond {
                Cond::Expr(c) | Cond::Let(_, _, c) => walk_expr(c, f),
            }
            walk_block(body, f);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
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
                Cond::Expr(c) | Cond::Let(_, _, c) => walk_expr(c, f),
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
                for pattern in &arm.patterns {
                    if let Pattern::Literal(l) = pattern {
                        walk_expr(l, f);
                    }
                }
                walk_expr(&arm.body, f);
            }
        }
        _ => for_each_child(e, &mut |c| walk_expr(c, f)),
    }
}

/// Does a closure in `b` capture the variable `name`?
pub(super) fn captures(b: &Block, name: &str) -> bool {
    let mut found = false;
    walk_block(b, &mut |node| {
        let mut free = BTreeSet::new();
        match node {
            Node::E(Expr { kind: ExprKind::Closure { params, body }, .. }) => {
                let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
                free_block(body, &mut bound, &mut free);
            }
            Node::S(Stmt::Fn(d)) => free_fn(d, &mut vec![d.name.name.clone()], &mut free),
            _ => {}
        }
        found |= free.contains(name);
    });
    found
}

/// A generic declaration (compiled only through its specialized copies).
fn is_generic(item: &Item) -> bool {
    match item {
        Item::Fn(f) => !f.generics.is_empty(),
        Item::Struct(d) => !d.generics.is_empty(),
        Item::Enum(d) => !d.generics.is_empty(),
        _ => false,
    }
}

/// Is `e` a place reached through an index, like `outer[i].items`?
fn through_index(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Index(..) => true,
        ExprKind::Member { object, .. } => through_index(object),
        _ => false,
    }
}

/// Collects names used in `b` that aren't declared inside it.
pub(crate) fn free_block(b: &Block, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
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
            Stmt::While { cond, body, .. } => {
                let mark = bound.len();
                match cond {
                    Cond::Expr(c) => free_expr(c, bound, out),
                    Cond::Let(name, _, v) => {
                        free_expr(v, bound, out);
                        bound.push(name.name.clone());
                    }
                }
                free_block(body, bound, out);
                bound.truncate(mark);
            }
            Stmt::Expr(e) => free_expr(e, bound, out),
            // A function declared here: its name is known from here on (and
            // in its own body); what else its body uses comes from outside.
            Stmt::Fn(d) => {
                bound.push(d.name.name.clone());
                free_fn(d, bound, out);
            }
            _ => {}
        }
    }
    bound.truncate(mark);
}

/// The names a function declared inside a function uses from outside it
/// (its own name, already in `bound`, isn't one).
fn free_fn(d: &FnDecl, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
    let mark = bound.len();
    bound.extend(d.params.iter().map(|p| p.name.name.clone()));
    free_block(&d.body, bound, out);
    bound.truncate(mark);
}

pub(crate) fn free_expr(e: &Expr, bound: &mut Vec<String>, out: &mut BTreeSet<String>) {
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
                Cond::Let(name, _, v) => {
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
                for pattern in &arm.patterns {
                    if let Pattern::Case { bindings, .. } = pattern {
                        bound.extend(bindings.iter().map(|b| b.name.clone()));
                    }
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
        ExprKind::Unary(_, x)
        | ExprKind::Cast { value: x, .. }
        | ExprKind::Try(x)
        | ExprKind::Await(x)
        | ExprKind::Spread(x) => f(x),
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
