use aelys_common::ffi::FfiType;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) type Trampoline = unsafe extern "C" fn(*const u8, *const u64, *mut u64);

struct Engine {
    module: JITModule,
    cache: HashMap<(Vec<FfiType>, FfiType), usize>,
    builder_context: FunctionBuilderContext,
}

// the engine lives behind a mutex for the whole process: its code pages are never freed, so a cached trampoline stays callable from any thread
struct EngineCell(Engine);
unsafe impl Send for EngineCell {}

fn engine() -> &'static Mutex<Option<EngineCell>> {
    static ENGINE: OnceLock<Mutex<Option<EngineCell>>> = OnceLock::new();
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn new_engine() -> Result<Engine, String> {
    let mut flags = settings::builder();
    for (name, value) in [
        ("use_colocated_libcalls", "false"),
        ("is_pic", "false"),
        ("opt_level", "speed"),
    ] {
        flags.set(name, value).map_err(|error| error.to_string())?;
    }
    let isa = cranelift_native::builder()
        .map_err(|error| error.to_string())?
        .finish(settings::Flags::new(flags))
        .map_err(|error| error.to_string())?;
    let builder = JITBuilder::with_isa(Arc::clone(&isa), cranelift_module::default_libcall_names());
    Ok(Engine {
        module: JITModule::new(builder),
        cache: HashMap::new(),
        builder_context: FunctionBuilderContext::new(),
    })
}

fn clif_type(ty: FfiType, pointer: types::Type) -> types::Type {
    match ty {
        FfiType::I8 | FfiType::U8 | FfiType::Bool => types::I8,
        FfiType::I16 | FfiType::U16 => types::I16,
        FfiType::I32 | FfiType::U32 => types::I32,
        FfiType::I64 | FfiType::U64 => types::I64,
        FfiType::F32 => types::F32,
        FfiType::F64 => types::F64,
        FfiType::Ptr | FfiType::CStr | FfiType::Bytes | FfiType::BytesMut | FfiType::Void => {
            pointer
        }
    }
}

fn abi_param(ty: FfiType, pointer: types::Type) -> AbiParam {
    let param = AbiParam::new(clif_type(ty, pointer));
    match ty {
        FfiType::I8 | FfiType::I16 => param.sext(),
        FfiType::U8 | FfiType::U16 | FfiType::Bool => param.uext(),
        _ => param,
    }
}

pub(crate) fn trampoline(params: &[FfiType], ret: FfiType) -> Result<Trampoline, String> {
    let mut guard = engine()
        .lock()
        .map_err(|_| "FFI engine poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(EngineCell(new_engine()?));
    }
    let Some(EngineCell(engine)) = guard.as_mut() else {
        return Err("FFI engine unavailable".to_string());
    };
    let key = (params.to_vec(), ret);
    if let Some(&code) = engine.cache.get(&key) {
        return Ok(unsafe { std::mem::transmute::<usize, Trampoline>(code) });
    }
    let code = compile(engine, params, ret)?;
    engine.cache.insert(key, code);
    Ok(unsafe { std::mem::transmute::<usize, Trampoline>(code) })
}

fn compile(engine: &mut Engine, params: &[FfiType], ret: FfiType) -> Result<usize, String> {
    let module = &mut engine.module;
    let frontend_config = module.target_config();
    let pointer = frontend_config.pointer_type();
    let call_conv = module.isa().default_call_conv();
    let ordinal = u32::try_from(engine.cache.len()).map_err(|_| "too many FFI signatures")?;

    let mut entry = module.make_signature();
    for _ in 0..3 {
        entry.params.push(AbiParam::new(pointer));
    }
    let id = module
        .declare_function(
            &format!("aelys_ffi_trampoline_{ordinal}"),
            Linkage::Local,
            &entry,
        )
        .map_err(|error| error.to_string())?;
    let mut context = module.make_context();
    context.func.signature = entry;
    context.func.name = UserFuncName::user(0, ordinal);

    let mut target = Signature::new(call_conv);
    for param in params {
        target.params.push(abi_param(*param, pointer));
    }
    if ret != FfiType::Void {
        target.returns.push(abi_param(ret, pointer));
    }

    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut engine.builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let entry_params = builder.block_params(block).to_vec();
        let (function, args, out) = (entry_params[0], entry_params[1], entry_params[2]);
        let signature = builder.import_signature(target);
        let flags = MemFlagsData::trusted();
        let mut values = Vec::with_capacity(params.len());
        for (index, param) in params.iter().enumerate() {
            let offset = i32::try_from(index * 8).map_err(|_| "too many FFI arguments")?;
            values.push(
                builder
                    .ins()
                    .load(clif_type(*param, pointer), flags, args, offset),
            );
        }
        let call = builder.ins().call_indirect(signature, function, &values);
        if ret != FfiType::Void {
            let result = builder.inst_results(call)[0];
            let stored = match ret {
                FfiType::I8 | FfiType::I16 | FfiType::I32 => {
                    builder.ins().sextend(types::I64, result)
                }
                FfiType::U8 | FfiType::U16 | FfiType::U32 | FfiType::Bool => {
                    builder.ins().uextend(types::I64, result)
                }
                _ => result,
            };
            builder.ins().store(flags, stored, out, 0);
        }
        builder.ins().return_(&[]);
        builder.finalize(frontend_config);
    }

    module
        .define_function(id, &mut context)
        .map_err(|error| format!("{error:?}"))?;
    module.clear_context(&mut context);
    module
        .finalize_definitions()
        .map_err(|error| error.to_string())?;
    Ok(module.get_finalized_function(id) as usize)
}

// x86-64 System V reads %al as an upper bound of the vector registers a variadic call uses, and cranelift never sets it
#[cfg(all(target_arch = "x86_64", not(windows)))]
pub(crate) fn variadic_entry(target: usize, vector_registers: u8) -> Result<usize, String> {
    const PAGE: usize = 4096;
    let mut code = [0u8; 18];
    code[0] = 0x49;
    code[1] = 0xBB;
    code[2..10].copy_from_slice(&(target as u64).to_le_bytes());
    code[10] = 0xB8;
    code[11..15].copy_from_slice(&u32::from(vector_registers.min(8)).to_le_bytes());
    code[15..18].copy_from_slice(&[0x41, 0xFF, 0xE3]);
    unsafe {
        let page = libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        if page == libc::MAP_FAILED {
            return Err("cannot map memory for a variadic call stub".to_string());
        }
        std::ptr::copy_nonoverlapping(code.as_ptr(), page.cast::<u8>(), code.len());
        if libc::mprotect(page, PAGE, libc::PROT_READ | libc::PROT_EXEC) != 0 {
            libc::munmap(page, PAGE);
            return Err("cannot seal a variadic call stub".to_string());
        }
        Ok(page as usize)
    }
}

#[cfg(all(target_arch = "aarch64", not(target_vendor = "apple"), not(windows)))]
pub(crate) fn variadic_entry(target: usize, _vector_registers: u8) -> Result<usize, String> {
    Ok(target)
}

#[cfg(not(any(
    all(target_arch = "x86_64", not(windows)),
    all(target_arch = "aarch64", not(target_vendor = "apple"), not(windows))
)))]
pub(crate) fn variadic_entry(_target: usize, _vector_registers: u8) -> Result<usize, String> {
    Err("variadic C functions are not supported on this platform".to_string())
}
