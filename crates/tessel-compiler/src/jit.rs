//! Runs compiled code inside this process, for `tessel repl`: object code in
//! memory is linked and loaded by LLVM's JIT (ORC), and stays loaded, so
//! later code can use what earlier code defined.

use std::ffi::{c_char, CStr, CString};

use inkwell::llvm_sys::core::LLVMCreateMemoryBufferWithMemoryRangeCopy;
use inkwell::llvm_sys::error::{LLVMDisposeErrorMessage, LLVMErrorRef, LLVMGetErrorMessage};
use inkwell::llvm_sys::orc2::lljit::*;
use inkwell::llvm_sys::orc2::*;
use inkwell::targets::{InitializationConfig, Target};

pub struct Jit {
    jit: LLVMOrcLLJITRef,
}

// (LLVM's JIT may be used from any thread, one at a time.)
unsafe impl Send for Jit {}

/// `Err` with LLVM's message, if `e` is an error.
unsafe fn check(e: LLVMErrorRef) -> Result<(), String> {
    if e.is_null() {
        return Ok(());
    }
    unsafe {
        let message = LLVMGetErrorMessage(e);
        let text = CStr::from_ptr(message).to_string_lossy().into_owned();
        LLVMDisposeErrorMessage(message);
        Err(text)
    }
}

impl Jit {
    /// A JIT for this computer. `symbols` are functions of this program that
    /// loaded code may call, by name (the runtime's); other names are looked
    /// for in the system's libraries.
    pub fn new(symbols: &[(&str, *const u8)]) -> Result<Jit, String> {
        Target::initialize_native(&InitializationConfig::default())?;
        unsafe {
            let mut jit = std::ptr::null_mut();
            check(LLVMOrcCreateLLJIT(&mut jit, LLVMOrcCreateLLJITBuilder()))?;
            let dylib = LLVMOrcLLJITGetMainJITDylib(jit);
            let flags = LLVMJITSymbolFlags {
                GenericFlags: LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsExported as u8
                    | LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsCallable as u8,
                TargetFlags: 0,
            };
            let mut pairs: Vec<LLVMOrcCSymbolMapPair> = symbols
                .iter()
                .map(|(name, address)| {
                    let name = CString::new(*name).unwrap();
                    LLVMOrcCSymbolMapPair {
                        Name: LLVMOrcLLJITMangleAndIntern(jit, name.as_ptr()),
                        Sym: LLVMJITEvaluatedSymbol { Address: *address as u64, Flags: LLVMJITSymbolFlags { ..flags } },
                    }
                })
                .collect();
            if !pairs.is_empty() {
                check(LLVMOrcJITDylibDefine(dylib, LLVMOrcAbsoluteSymbols(pairs.as_mut_ptr(), pairs.len())))?;
            }
            let mut system = std::ptr::null_mut();
            check(LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(
                &mut system,
                LLVMOrcLLJITGetGlobalPrefix(jit),
                None,
                std::ptr::null_mut(),
            ))?;
            LLVMOrcJITDylibAddGenerator(dylib, system);
            Ok(Jit { jit })
        }
    }

    /// Loads an object file (as `codegen::compile_repl` makes them).
    pub fn add_object(&mut self, object: &[u8]) -> Result<(), String> {
        unsafe {
            let buffer =
                LLVMCreateMemoryBufferWithMemoryRangeCopy(object.as_ptr() as *const c_char, object.len(), c"object".as_ptr());
            check(LLVMOrcLLJITAddObjectFile(self.jit, LLVMOrcLLJITGetMainJITDylib(self.jit), buffer))
        }
    }

    /// The address of a function or variable in the loaded code.
    pub fn lookup(&self, name: &str) -> Result<*const u8, String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let mut address = 0;
        unsafe { check(LLVMOrcLLJITLookup(self.jit, &mut address, name.as_ptr()))? };
        Ok(address as *const u8)
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        unsafe {
            let _ = check(LLVMOrcDisposeLLJIT(self.jit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;
    use inkwell::targets::{CodeModel, FileType, RelocMode, TargetMachine};
    use inkwell::OptimizationLevel;

    extern "C" fn double(n: i64) -> i64 {
        n * 2
    }

    /// An object file with `i64 <name>() { return double(21) + extra; }`.
    fn object(name: &str, extra: u64) -> Vec<u8> {
        Target::initialize_native(&InitializationConfig::default()).unwrap();
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).unwrap();
        let tm = target
            .create_target_machine(&triple, "generic", "", OptimizationLevel::None, RelocMode::PIC, CodeModel::Default)
            .unwrap();
        let ctx = Context::create();
        let module = ctx.create_module("test");
        module.set_triple(&triple);
        let i64t = ctx.i64_type();
        let host = module.add_function("host_double", i64t.fn_type(&[i64t.into()], false), None);
        let f = module.add_function(name, i64t.fn_type(&[], false), None);
        let b = ctx.create_builder();
        b.position_at_end(ctx.append_basic_block(f, "entry"));
        let v = b.build_call(host, &[i64t.const_int(21, false).into()], "").unwrap().try_as_basic_value().basic().unwrap();
        let sum = b.build_int_add(v.into_int_value(), i64t.const_int(extra, false), "").unwrap();
        b.build_return(Some(&sum)).unwrap();
        tm.write_to_memory_buffer(&module, FileType::Object).unwrap().as_slice().to_vec()
    }

    #[test]
    fn loads_and_runs_objects() {
        let mut jit = Jit::new(&[("host_double", double as *const u8)]).unwrap();
        jit.add_object(&object("first", 0)).unwrap();
        jit.add_object(&object("second", 1)).unwrap();
        let call = |name: &str| {
            let f: extern "C" fn() -> i64 = unsafe { std::mem::transmute(jit.lookup(name).unwrap()) };
            f()
        };
        assert_eq!((call("first"), call("second")), (42, 43));
        assert!(jit.lookup("missing").is_err());
    }
}
