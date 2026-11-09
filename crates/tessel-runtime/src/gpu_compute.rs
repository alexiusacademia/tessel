//! GPU compute (see docs/gpu.md): `GPUBuffer`s, kernels run over a range
//! (`gpu.run`), and reductions (`gpu.sum`, `gpu.min`, `gpu.max`,
//! `gpu.dot`).
//!
//! A kernel comes from the compiler twice: as WGSL, which wgpu runs on the
//! GPU (Metal, DX12, Vulkan or OpenGL), and as native code, which the CPU
//! fallback runs over all cores. The fallback is used where there's no GPU
//! (or only a software one, which is slower than the native code), and
//! always with `TESSEL_GPU=cpu`; `TESSEL_GPU=gpu` takes any GPU wgpu finds,
//! software ones too (for testing the WGSL).
//!
//! Kernel runs are recorded into one command encoder, which is submitted
//! when something waits for the GPU (`read`, a reduction, `gpu.wait()`) or
//! every `BATCH` runs. Their parameters (the range and the kernel's
//! values) go into a ring of uniform memory, written once per submit.
//!
//! Author: Alexius Academia
//! Date: 2026-11-09

use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::{Header, List, Str};

/// What the compiler emits for a kernel.
#[repr(C)]
pub struct KernelDesc {
    pub name: *const c_char,
    pub wgsl: *const c_char,
    /// 1, 2 or 3: the axes of its index.
    pub dims: i64,
    pub nparams: i64,
    /// For each parameter after the index: one of the `K_…` kinds.
    pub kinds: *const u8,
    /// The kernel compiled for the CPU: runs linear indices `lo..hi` of a
    /// range `n0 × n1 × …`, with the values in `args` (a buffer's is a
    /// pointer to its `CpuSlice`).
    pub cpu: unsafe extern "C" fn(args: *const u64, lo: i64, hi: i64, n0: i64, n1: i64),
}

unsafe impl Sync for KernelDesc {}

pub const K_BUF_F32: u8 = 0;
pub const K_BUF_INT: u8 = 1;
pub const K_F32: u8 = 2;
pub const K_INT: u8 = 3;
pub const K_BOOL: u8 = 4;

/// A buffer's numbers in memory, as the CPU kernels see them.
#[repr(C)]
pub struct CpuSlice {
    pub data: *mut u32,
    pub len: i64,
}

/// Runs this many kernels before submitting them, without waiting.
const BATCH: usize = 64;
/// Uniform memory for kernels' parameters, used round and round.
const RING: u64 = 4 << 20;

// ===== buffers =====

/// The object behind a `GPUBuffer`: a header, then its numbers' home.
#[repr(C)]
struct BufferObj {
    header: Header,
    inner: *mut Buffer,
}

struct Buffer {
    id: u64,
    count: usize,
    /// `GPUBuffer<Int>` (32-bit on the GPU), else `Float32`.
    ints: bool,
    store: Store,
}

enum Store {
    /// In memory (the CPU fallback): 32-bit words. Boxed, so its address
    /// stays put while kernels run on it.
    Cpu(Box<CpuSlice>, Vec<u32>),
    #[cfg(feature = "gpu")]
    Gpu(wgpu::Buffer),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

unsafe fn buffer<'a>(b: *mut Header) -> &'a mut Buffer {
    unsafe { &mut *(*(b as *mut BufferObj)).inner }
}

unsafe extern "C" fn buffer_drop(h: *mut Header) {
    unsafe {
        let inner = Box::from_raw((*(h as *mut BufferObj)).inner);
        #[cfg(feature = "gpu")]
        if let Store::Gpu(_) = &inner.store {
            if let Some(m) = STATE.get() {
                if let Mode::Gpu(g) = &mut *m.lock().unwrap() {
                    g.forget(inner.id);
                }
            }
        }
        drop(inner);
        crate::free(h as *mut u8);
    }
}

fn new_buffer(count: usize, ints: bool, words: Option<Vec<u32>>) -> *mut Header {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let store = with_mode(|mode| match mode {
        #[cfg(feature = "gpu")]
        Mode::Gpu(g) => Store::Gpu(g.new_buffer(count, words.as_deref())),
        Mode::Cpu => {
            let mut v = words.unwrap_or_else(|| vec![0; count]);
            v.resize(count, 0);
            let slice = Box::new(CpuSlice { data: v.as_mut_ptr(), len: count as i64 });
            Store::Cpu(slice, v)
        }
    });
    let inner = Box::into_raw(Box::new(Buffer { id, count, ints, store }));
    unsafe {
        let h = crate::new_object(std::mem::size_of::<BufferObj>(), buffer_drop);
        (*(h as *mut BufferObj)).inner = inner;
        h
    }
}

/// A list's numbers as the buffer's 32-bit words: `[Float32]` as they are,
/// `[Int]` cut to 32 bits.
unsafe fn words_of(list: *const List, ints: bool) -> Vec<u32> {
    unsafe {
        let n = if list.is_null() { 0 } else { (*list).len as usize };
        if n == 0 {
            return Vec::new();
        }
        if ints {
            std::slice::from_raw_parts((*list).data as *const i64, n).iter().map(|&x| x as i32 as u32).collect()
        } else {
            std::slice::from_raw_parts((*list).data as *const u32, n).to_vec()
        }
    }
}

/// `GPUBuffer<T>(count: n)`: `n` zeros. `ints`: it holds `Int`s.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_gpu_buffer_new(ints: bool, count: i64) -> *mut Header {
    if count < 0 {
        crate::fatal(&format!("a GPU buffer can't have {count} numbers"));
    }
    new_buffer(count as usize, ints, None)
}

/// `GPUBuffer(from: list)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_from(ints: bool, list: *const List) -> *mut Header {
    let words = unsafe { words_of(list, ints) };
    new_buffer(words.len(), ints, Some(words))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_count(b: *mut Header) -> i64 {
    unsafe { buffer(b).count as i64 }
}

/// `buffer.read()`: its numbers as a new `[Float32]` or `[Int]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_read(b: *mut Header, elem: *const crate::TslType) -> *mut List {
    let buf = unsafe { buffer(b) };
    let words: Vec<u32> = match &buf.store {
        Store::Cpu(_, v) => v.clone(),
        #[cfg(feature = "gpu")]
        Store::Gpu(gb) => with_mode(|mode| match mode {
            Mode::Gpu(g) => g.read(gb, buf.count),
            Mode::Cpu => unreachable!(),
        }),
    };
    unsafe {
        let list = crate::tsl_list_new(elem, words.len() as i64);
        if buf.ints {
            let out = (*list).data as *mut i64;
            for (i, w) in words.iter().enumerate() {
                *out.add(i) = *w as i32 as i64;
            }
        } else {
            std::ptr::copy_nonoverlapping(words.as_ptr(), (*list).data as *mut u32, words.len());
        }
        (*list).len = words.len() as i64;
        list
    }
}

/// `buffer.write(list)`: as many numbers as both have, from the start.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_write(b: *mut Header, list: *const List) {
    let buf = unsafe { buffer(b) };
    let mut words = unsafe { words_of(list, buf.ints) };
    words.truncate(buf.count);
    match &mut buf.store {
        Store::Cpu(_, v) => v[..words.len()].copy_from_slice(&words),
        #[cfg(feature = "gpu")]
        Store::Gpu(gb) => with_mode(|mode| {
            if let Mode::Gpu(g) = mode {
                g.write(gb, &words);
            }
        }),
    }
}

/// `buffer.fill(value)`: the value's 32 bits (a `Float32`'s, or an `Int`
/// cut to 32), everywhere.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_fill(b: *mut Header, bits: u64) {
    let buf = unsafe { buffer(b) };
    let word = bits as u32;
    match &mut buf.store {
        Store::Cpu(_, v) => v.fill(word),
        #[cfg(feature = "gpu")]
        Store::Gpu(gb) => {
            let words = vec![word; buf.count];
            with_mode(|mode| {
                if let Mode::Gpu(g) = mode {
                    g.write(gb, &words);
                }
            })
        }
    }
}

/// `buffer.copy(from: other)`: as many numbers as both have.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_buffer_copy(dst: *mut Header, src: *mut Header) {
    if dst == src {
        return;
    }
    let (d, s) = unsafe { (buffer(dst), buffer(src)) };
    let n = d.count.min(s.count);
    match (&mut d.store, &s.store) {
        (Store::Cpu(_, dv), Store::Cpu(_, sv)) => dv[..n].copy_from_slice(&sv[..n]),
        #[cfg(feature = "gpu")]
        (Store::Gpu(db), Store::Gpu(sb)) => with_mode(|mode| {
            if let Mode::Gpu(g) = mode {
                g.copy(sb, db, n);
            }
        }),
        #[cfg(feature = "gpu")]
        _ => unreachable!("buffers are all on the GPU or all in memory"),
    }
}

// ===== the device =====

enum Mode {
    Cpu,
    #[cfg(feature = "gpu")]
    Gpu(Box<gpu::Gpu>),
}

static STATE: OnceLock<Mutex<Mode>> = OnceLock::new();

fn with_mode<T>(f: impl FnOnce(&mut Mode) -> T) -> T {
    let m = STATE.get_or_init(|| Mutex::new(open()));
    let mut mode = m.lock().unwrap();
    f(&mut mode)
}

/// The GPU to compute on, or the CPU: see the top.
fn open() -> Mode {
    let want = std::env::var("TESSEL_GPU").unwrap_or_default();
    if want == "cpu" {
        return Mode::Cpu;
    }
    #[cfg(feature = "gpu")]
    if let Some(g) = gpu::Gpu::open(want == "gpu") {
        return Mode::Gpu(Box::new(g));
    }
    Mode::Cpu
}

/// `gpu.device`: the GPU's name, or `"CPU"`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_gpu_device() -> *mut Str {
    let name = with_mode(|mode| match mode {
        Mode::Cpu => "CPU".to_string(),
        #[cfg(feature = "gpu")]
        Mode::Gpu(g) => g.name.clone(),
    });
    crate::str_new(&name)
}

/// `gpu.isCPU`.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_gpu_is_cpu() -> bool {
    with_mode(|mode| matches!(mode, Mode::Cpu))
}

/// `gpu.wait()`: until the kernels run so far are done.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_gpu_wait() {
    with_mode(|mode| {
        #[cfg(feature = "gpu")]
        if let Mode::Gpu(g) = mode {
            g.wait();
        }
        let _ = mode;
    })
}

/// `gpu.run(kernel, over: (n0, n1, n2), values…)`: `args` has one value
/// for each of the kernel's parameters after its index (a buffer's is its
/// object; a `Float32`'s its bits; an `Int`'s itself; a `Bool`'s 0 or 1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_run(k: *const KernelDesc, n0: i64, n1: i64, n2: i64, args: *const u64) {
    let k = unsafe { &*k };
    let name = unsafe { CStr::from_ptr(k.name) }.to_string_lossy();
    for (axis, n) in [n0, n1, n2].into_iter().enumerate().take(k.dims as usize) {
        if !(0..=i32::MAX as i64).contains(&n) {
            crate::fatal(&format!("`gpu.run({name}, …)`: the range's axis {} is {n}, which isn't 0 to 2147483647", axis + 1));
        }
    }
    let total = (n0 as i128) * if k.dims >= 2 { n1 as i128 } else { 1 } * if k.dims >= 3 { n2 as i128 } else { 1 };
    if total == 0 {
        return;
    }
    let np = k.nparams as usize;
    let kinds = unsafe { std::slice::from_raw_parts(k.kinds, np) };
    let values = unsafe { std::slice::from_raw_parts(args, np) };
    with_mode(|mode| match mode {
        Mode::Cpu => {
            #[cfg(feature = "gpu")]
            check_wgsl(k, &name);
            // A buffer goes to the native kernel as its `CpuSlice`.
            let mut cpu_args: Vec<u64> = values.to_vec();
            for (i, kind) in kinds.iter().enumerate() {
                if matches!(*kind, K_BUF_F32 | K_BUF_INT) {
                    let buf = unsafe { buffer(values[i] as *mut Header) };
                    let Store::Cpu(slice, _) = &buf.store else { unreachable!() };
                    cpu_args[i] = &**slice as *const CpuSlice as u64;
                }
            }
            let total = total as usize;
            let (cpu, a) = (k.cpu, cpu_args.as_ptr() as usize);
            parallel_for(total, &|lo, hi| unsafe { cpu(a as *const u64, lo as i64, hi as i64, n0, n1) });
        }
        #[cfg(feature = "gpu")]
        Mode::Gpu(g) => g.run(k, &name, [n0, n1, n2], kinds, values),
    })
}

/// With `TESSEL_GPU_CHECK` set, the CPU fallback also checks each kernel's
/// WGSL (once), as a GPU would when it compiles it: the tests set it, so
/// computers without a GPU (CI) still check the code made for one.
#[cfg(feature = "gpu")]
fn check_wgsl(k: &KernelDesc, name: &str) {
    use std::collections::HashSet;
    static CHECKED: Mutex<Option<HashSet<usize>>> = Mutex::new(None);
    if std::env::var_os("TESSEL_GPU_CHECK").is_none() {
        return;
    }
    let mut checked = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    if !checked.get_or_insert_with(HashSet::new).insert(k as *const KernelDesc as usize) {
        return;
    }
    let source = unsafe { CStr::from_ptr(k.wgsl) }.to_string_lossy();
    for (what, source) in [(name, &*source), ("the reductions", gpu::REDUCE_WGSL)] {
        if let Err(e) = validate_wgsl(source) {
            crate::fatal(&format!("the WGSL of {what} doesn't validate: {e}\n{source}"));
        }
    }
}

#[cfg(feature = "gpu")]
fn validate_wgsl(source: &str) -> Result<(), String> {
    use wgpu::naga;
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
        .validate(&module)
        .map(|_| ())
        .map_err(|e| e.emit_to_string(source))
}

/// `gpu.sum(b)` (op 0), `gpu.min(b)` (1), `gpu.max(b)` (2), `gpu.dot(a, b)`
/// (3): blocks of 256 combined in 32-bit as a tree, then the blocks in
/// 64-bit, with compensation for the sums (Kahan).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_gpu_reduce(op: i64, a: *mut Header, b: *mut Header) -> f64 {
    let (ab, bb) = unsafe { (buffer(a), buffer(if b.is_null() { a } else { b })) };
    let n = if op == 3 { ab.count.min(bb.count) } else { ab.count };
    let partials: Vec<f32> = match (&ab.store, &bb.store) {
        (Store::Cpu(_, av), Store::Cpu(_, bv)) => {
            let f = |w: &[u32]| -> Vec<f32> { w.iter().map(|x| f32::from_bits(*x)).collect() };
            cpu_partials(op, &f(&av[..n]), &f(&bv[..n]))
        }
        #[cfg(feature = "gpu")]
        (Store::Gpu(ag), Store::Gpu(bg)) => with_mode(|mode| match mode {
            Mode::Gpu(g) => g.partials(op, ag, bg, n),
            Mode::Cpu => unreachable!(),
        }),
        #[cfg(feature = "gpu")]
        _ => unreachable!(),
    };
    combine(op, &partials)
}

/// The 256-number blocks' results, as the GPU works them out.
fn cpu_partials(op: i64, a: &[f32], b: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(a.len().div_ceil(256));
    for (i, chunk) in a.chunks(256).enumerate() {
        let mut s = [identity(op); 256];
        for (j, x) in chunk.iter().enumerate() {
            s[j] = if op == 3 { x * b[i * 256 + j] } else { *x };
        }
        let mut w = 128;
        while w > 0 {
            for l in 0..w {
                s[l] = pair(op, s[l], s[l + w]);
            }
            w >>= 1;
        }
        out.push(s[0]);
    }
    out
}

/// (As the GPU's: the largest finite numbers, as WGSL has no infinity.)
fn identity(op: i64) -> f32 {
    match op {
        1 => f32::MAX,
        2 => f32::MIN,
        _ => 0.0,
    }
}

fn pair(op: i64, a: f32, b: f32) -> f32 {
    match op {
        1 => a.min(b),
        2 => a.max(b),
        _ => a + b,
    }
}

/// The blocks' results in 64-bit (sums with Kahan's compensation).
fn combine(op: i64, partials: &[f32]) -> f64 {
    match op {
        1 => partials.iter().fold(f64::INFINITY, |m, x| m.min(*x as f64)),
        2 => partials.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x as f64)),
        _ => {
            let (mut sum, mut c) = (0.0f64, 0.0f64);
            for x in partials {
                let y = *x as f64 - c;
                let t = sum + y;
                c = (t - sum) - y;
                sum = t;
            }
            sum
        }
    }
}

// ===== the CPU's cores =====

/// A piece of work for every core: `run(lo, hi)` over chunks of `0..total`.
struct Job {
    run: *const (dyn Fn(usize, usize) + Sync),
    total: usize,
    chunk: usize,
    next: AtomicUsize,
    done: AtomicUsize,
}

unsafe impl Sync for Job {}
unsafe impl Send for Job {}

struct Pool {
    threads: usize,
    /// The job, and how many jobs there have been (so a worker knows a new one).
    work: Mutex<(Option<Arc<Job>>, u64)>,
    wake: Condvar,
}

fn pool() -> &'static Arc<Pool> {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let p = Arc::new(Pool { threads, work: Mutex::new((None, 0)), wake: Condvar::new() });
        for _ in 1..threads {
            let p = p.clone();
            std::thread::spawn(move || {
                let mut seen = 0u64;
                loop {
                    let job = {
                        let mut w = p.work.lock().unwrap();
                        while w.1 == seen || w.0.is_none() {
                            w = p.wake.wait(w).unwrap();
                        }
                        seen = w.1;
                        w.0.clone().unwrap()
                    };
                    work_on(&job);
                }
            });
        }
        p
    })
}

fn work_on(job: &Job) {
    loop {
        let lo = job.next.fetch_add(job.chunk, Ordering::Relaxed);
        if lo >= job.total {
            return;
        }
        let hi = (lo + job.chunk).min(job.total);
        unsafe { (*job.run)(lo, hi) };
        job.done.fetch_add(hi - lo, Ordering::Release);
    }
}

/// Runs `run` over `0..total` in chunks, on every core, and returns when
/// it's all done.
fn parallel_for(total: usize, run: &(dyn Fn(usize, usize) + Sync)) {
    let p = pool();
    if p.threads == 1 || total < 4096 {
        run(0, total);
        return;
    }
    let chunk = (total / (p.threads * 8)).max(1024);
    // (The workers only use `run` while this waits for them.)
    let run: *const (dyn Fn(usize, usize) + Sync) = unsafe { std::mem::transmute(run) };
    let job = Arc::new(Job { run, total, chunk, next: AtomicUsize::new(0), done: AtomicUsize::new(0) });
    {
        let mut w = p.work.lock().unwrap();
        w.0 = Some(job.clone());
        w.1 += 1;
    }
    p.wake.notify_all();
    work_on(&job);
    while job.done.load(Ordering::Acquire) < total {
        std::hint::spin_loop();
    }
    p.work.lock().unwrap().0 = None;
}

// ===== the GPU =====

#[cfg(feature = "gpu")]
mod gpu {
    use super::*;

    struct Pipeline {
        pipeline: wgpu::ComputePipeline,
        layout: wgpu::BindGroupLayout,
    }

    pub struct Gpu {
        pub name: String,
        device: wgpu::Device,
        queue: wgpu::Queue,
        encoder: Option<wgpu::CommandEncoder>,
        /// The compute pass the runs go in, while nothing else is recorded
        /// (one pass for many runs: wgpu orders them, as WebGPU says).
        pass: Option<wgpu::ComputePass<'static>>,
        /// For reading buffers back.
        staging: Option<wgpu::Buffer>,
        runs: usize,
        pipelines: HashMap<usize, Pipeline>,
        /// Bind groups by kernel and buffers (the parameters' offset varies).
        groups: HashMap<(usize, Vec<u64>), wgpu::BindGroup>,
        ring: wgpu::Buffer,
        /// Where in `ring` the batch's parameters start, and the bytes.
        ring_start: u64,
        ring_bytes: Vec<u8>,
        align: u64,
        reduce: Option<Pipeline>,
        partials: Option<wgpu::Buffer>,
        max_groups: u32,
    }

    impl Gpu {
        pub fn open(any: bool) -> Option<Gpu> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                display: None,
                backends: wgpu::Backends::from_env().unwrap_or_default(),
                flags: wgpu::InstanceFlags::from_build_config().with_env(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
                backend_options: wgpu::BackendOptions::from_env_or_default(),
            });
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .ok()?;
            let info = adapter.get_info();
            // A software GPU is slower than the native kernels.
            if info.device_type == wgpu::DeviceType::Cpu && !any {
                return None;
            }
            if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
                return None;
            }
            let limits = adapter.limits();
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("tessel compute"),
                required_features: wgpu::Features::empty(),
                required_limits: limits.clone(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))
            .ok()?;
            device.on_uncaptured_error(std::sync::Arc::new(|e| {
                crate::fatal(&format!("the GPU: {e}"));
            }));
            let ring = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kernel parameters"),
                size: RING,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Some(Gpu {
                name: info.name.clone(),
                align: limits.min_uniform_buffer_offset_alignment as u64,
                max_groups: limits.max_compute_workgroups_per_dimension,
                device,
                queue,
                encoder: None,
                pass: None,
                staging: None,
                runs: 0,
                pipelines: HashMap::new(),
                groups: HashMap::new(),
                ring,
                ring_start: 0,
                ring_bytes: Vec::new(),
                reduce: None,
                partials: None,
            })
        }

        pub fn new_buffer(&mut self, count: usize, words: Option<&[u32]>) -> wgpu::Buffer {
            let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (count.max(1) * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if let Some(w) = words.filter(|w| !w.is_empty()) {
                self.flush();
                self.queue.write_buffer(&b, 0, bytes(w));
            }
            b
        }

        /// A buffer is gone: its bind groups too (they'd keep it).
        pub fn forget(&mut self, id: u64) {
            self.groups.retain(|(_, ids), _| !ids.contains(&id));
        }

        /// The encoder, to record something other than a run (the pass ends).
        fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
            self.pass = None;
            let device = &self.device;
            self.encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default()))
        }

        fn pass(&mut self) -> &mut wgpu::ComputePass<'static> {
            if self.pass.is_none() {
                let encoder = self.encoder();
                let pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                self.pass = Some(pass.forget_lifetime());
            }
            self.pass.as_mut().unwrap()
        }

        /// Submits what's been recorded (with its parameters).
        pub fn flush(&mut self) {
            self.pass = None;
            if !self.ring_bytes.is_empty() {
                self.queue.write_buffer(&self.ring, self.ring_start, &self.ring_bytes);
                self.ring_start += self.ring_bytes.len() as u64;
                self.ring_bytes.clear();
            }
            if let Some(e) = self.encoder.take() {
                self.queue.submit([e.finish()]);
            }
            self.runs = 0;
        }

        pub fn wait(&mut self) {
            self.flush();
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = done.clone();
            self.queue.on_submitted_work_done(move || flag.store(true, Ordering::Release));
            self.wait_until(&done);
        }

        /// Until `done` is set by the GPU's callback. wgpu's own waiting
        /// sleeps a millisecond at a time on some systems (Metal), which
        /// would be most of a short round trip: this asks without blocking
        /// for the first 2 ms, then waits as wgpu does.
        fn wait_until(&self, done: &std::sync::atomic::AtomicBool) {
            let start = std::time::Instant::now();
            while !done.load(Ordering::Acquire) {
                let _ = self.device.poll(wgpu::PollType::Poll);
                if done.load(Ordering::Acquire) {
                    return;
                }
                if start.elapsed() > std::time::Duration::from_millis(2) {
                    let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                } else {
                    std::thread::yield_now();
                }
            }
        }

        /// A uniform block of `words` for a run: where it is in the ring.
        fn params(&mut self, words: &[u32]) -> u32 {
            let size = (words.len() * 4) as u64;
            let padded = size.div_ceil(self.align) * self.align;
            if self.ring_start + self.ring_bytes.len() as u64 + padded > RING {
                // Round again, once what's using the ring is done.
                self.wait();
                self.ring_start = 0;
            }
            let at = self.ring_start + self.ring_bytes.len() as u64;
            self.ring_bytes.extend_from_slice(bytes(words));
            self.ring_bytes.resize(self.ring_bytes.len() + (padded - size) as usize, 0);
            at as u32
        }

        fn pipeline(&mut self, k: &KernelDesc, name: &str, kinds: &[u8]) -> usize {
            let key = k as *const KernelDesc as usize;
            if !self.pipelines.contains_key(&key) {
                let source = unsafe { CStr::from_ptr(k.wgsl) }.to_string_lossy();
                let buffers = kinds.iter().filter(|k| matches!(**k, K_BUF_F32 | K_BUF_INT)).count();
                let p = make_pipeline(&self.device, name, &source, buffers, false);
                self.pipelines.insert(key, p);
            }
            key
        }

        pub fn run(&mut self, k: &KernelDesc, name: &str, n: [i64; 3], kinds: &[u8], values: &[u64]) {
            let key = self.pipeline(k, name, kinds);
            // The parameters: the range (and the 1D range's row length),
            // then the kernel's values.
            let groups: [u32; 3] = match k.dims {
                1 => {
                    let total = n[0] as u64;
                    let blocks = total.div_ceil(256);
                    let x = blocks.min(self.max_groups as u64);
                    let y = blocks.div_ceil(x);
                    if y > self.max_groups as u64 {
                        crate::fatal(&format!("`gpu.run({name}, …)`: {total} is more than this GPU can run at once"));
                    }
                    [x as u32, y as u32, 1]
                }
                2 => [(n[0] as u64).div_ceil(16) as u32, (n[1] as u64).div_ceil(16) as u32, 1],
                _ => [(n[0] as u64).div_ceil(8) as u32, (n[1] as u64).div_ceil(8) as u32, (n[2] as u64).div_ceil(4) as u32],
            };
            if groups.iter().any(|g| *g > self.max_groups) {
                crate::fatal(&format!("`gpu.run({name}, …)`: the range is bigger than this GPU can run at once"));
            }
            let row = groups[0] * 256;
            let mut words: Vec<u32> = vec![n[0] as u32, n[1] as u32, n[2] as u32, row];
            let mut ids = Vec::new();
            let mut bufs: Vec<&wgpu::Buffer> = Vec::new();
            for (kind, v) in kinds.iter().zip(values) {
                match *kind {
                    K_BUF_F32 | K_BUF_INT => {
                        let b = unsafe { buffer(*v as *mut Header) };
                        let Store::Gpu(gb) = &b.store else { unreachable!() };
                        ids.push(b.id);
                        bufs.push(gb);
                        // (Its count, for `buffer.count` in the kernel.)
                        words.push(b.count as u32);
                    }
                    K_F32 => words.push(*v as u32),
                    K_INT => words.push(*v as i64 as i32 as u32),
                    K_BOOL => words.push((*v != 0) as u32),
                    _ => unreachable!("a kernel parameter of kind {kind}"),
                }
            }
            let offset = self.params(&words);
            let group_key = (key, ids);
            if !self.groups.contains_key(&group_key) {
                let p = &self.pipelines[&key];
                let mut entries = vec![wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.ring,
                        offset: 0,
                        size: std::num::NonZeroU64::new(256.max(((words.len() * 4) as u64).div_ceil(16) * 16)),
                    }),
                }];
                for (i, b) in bufs.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: b.as_entire_binding() });
                }
                let g = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.layout, entries: &entries });
                self.groups.insert(group_key.clone(), g);
            }
            let pipeline = self.pipelines[&key].pipeline.clone();
            let group = self.groups[&group_key].clone();
            let pass = self.pass();
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[offset]);
            pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
            self.runs += 1;
            if self.runs >= BATCH {
                self.flush();
            }
        }

        pub fn write(&mut self, b: &wgpu::Buffer, words: &[u32]) {
            // (After the kernels before it, as the program says.)
            self.flush();
            if !words.is_empty() {
                self.queue.write_buffer(b, 0, bytes(words));
            }
        }

        pub fn copy(&mut self, from: &wgpu::Buffer, to: &wgpu::Buffer, n: usize) {
            if n > 0 {
                self.encoder().copy_buffer_to_buffer(from, 0, to, 0, (n * 4) as u64);
            }
        }

        /// The first `n` words of `b`, once what's before is done.
        pub fn read(&mut self, b: &wgpu::Buffer, n: usize) -> Vec<u32> {
            if n == 0 {
                return Vec::new();
            }
            let size = (n * 4) as u64;
            if self.staging.as_ref().is_none_or(|s| s.size() < size) {
                self.staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("read"),
                    size: size.next_power_of_two(),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            let staging = self.staging.take().unwrap();
            self.encoder().copy_buffer_to_buffer(b, 0, &staging, 0, size);
            self.flush();
            let slice = staging.slice(..size);
            let mapped = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = mapped.clone();
            slice.map_async(wgpu::MapMode::Read, move |_| flag.store(true, Ordering::Release));
            self.wait_until(&mapped);
            let data = slice.get_mapped_range();
            let words: Vec<u32> = data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            drop(data);
            staging.unmap();
            self.staging = Some(staging);
            words
        }

        /// The 256-number blocks' results of a reduction (see `tsl_gpu_reduce`).
        pub fn partials(&mut self, op: i64, a: &wgpu::Buffer, b: &wgpu::Buffer, n: usize) -> Vec<f32> {
            if n == 0 {
                return Vec::new();
            }
            if self.reduce.is_none() {
                self.reduce = Some(make_pipeline(&self.device, "reduce", REDUCE_WGSL, 3, true));
            }
            let blocks = n.div_ceil(256) as u64;
            let x = blocks.min(self.max_groups as u64);
            let y = blocks.div_ceil(x);
            let need = (blocks * 4).max(4);
            if self.partials.as_ref().is_none_or(|p| p.size() < need) {
                self.partials = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("partials"),
                    size: need.next_power_of_two(),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }));
            }
            let offset = self.params(&[n as u32, (x * 256) as u32, op as u32, 0]);
            let partials = self.partials.take().unwrap();
            {
                let p = self.reduce.as_ref().unwrap();
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &p.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.ring,
                                offset: 0,
                                size: std::num::NonZeroU64::new(256),
                            }),
                        },
                        wgpu::BindGroupEntry { binding: 1, resource: a.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: b.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: partials.as_entire_binding() },
                    ],
                });
                let pipeline = p.pipeline.clone();
                let pass = self.pass();
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[offset]);
                pass.dispatch_workgroups(x as u32, y as u32, 1);
            }
            let words = self.read(&partials, blocks as usize);
            self.partials = Some(partials);
            words.into_iter().map(f32::from_bits).collect()
        }
    }

    fn bytes(words: &[u32]) -> &[u8] {
        unsafe { std::slice::from_raw_parts(words.as_ptr() as *const u8, words.len() * 4) }
    }

    /// A kernel's pipeline: its parameters at binding 0 (a uniform block at
    /// a dynamic offset), its buffers at 1, 2, …; `reduce`'s first two
    /// buffers are only read.
    fn make_pipeline(device: &wgpu::Device, name: &str, source: &str, buffers: usize, reduce: bool) -> Pipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        }];
        for i in 0..buffers {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + i as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: reduce && i < 2 },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            });
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(name), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(name),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(name),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Pipeline { pipeline, layout }
    }

    /// Each block of 256 numbers to one, as a tree (see `cpu_partials`).
    pub(super) const REDUCE_WGSL: &str = r#"
struct P { n: u32, row: u32, op: u32, pad: u32 }
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;
var<workgroup> s: array<f32, 256>;

fn pair(x: f32, y: f32) -> f32 {
    if (p.op == 1u) { return min(x, y); }
    if (p.op == 2u) { return max(x, y); }
    return x + y;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) g: vec3<u32>, @builtin(local_invocation_index) l: u32,
        @builtin(workgroup_id) w: vec3<u32>, @builtin(num_workgroups) nw: vec3<u32>) {
    let i = g.x + g.y * p.row;
    var v = 0.0;
    if (p.op == 1u) { v = 3.4028235e38; }
    if (p.op == 2u) { v = -3.4028235e38; }
    if (i < p.n) {
        if (p.op == 3u) { v = a[i] * b[i]; } else { v = a[i]; }
    }
    s[l] = v;
    workgroupBarrier();
    for (var k = 128u; k > 0u; k = k >> 1u) {
        if (l < k) { s[l] = pair(s[l], s[l + k]); }
        workgroupBarrier();
    }
    if (l == 0u) { out[w.x + w.y * nw.x] = s[0]; }
}
"#;
}

#[cfg(all(test, feature = "gpu"))]
mod tests {
    #[test]
    fn wgsl_check() {
        assert!(super::validate_wgsl(super::gpu::REDUCE_WGSL).is_ok());
        // A type mistake a GPU's compiler would refuse.
        let bad = "@compute @workgroup_size(1) fn main() { let x: f32 = 1u; }";
        assert!(super::validate_wgsl(bad).is_err());
    }
}
