mod library;
mod pointer;
mod trampoline;

use crate::vm::{GcRef, NativeFunction, NativeFunctionImpl, ObjectKind, VM, Value};
use aelys_bytecode::GcObject;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_common::ffi::{FfiSignature, FfiType};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) use pointer::POINTER_NATIVES;

#[derive(Clone, Copy)]
struct Resolved {
    entry: usize,
    trampoline: trampoline::Trampoline,
}

pub(crate) struct FfiFunction {
    library: String,
    symbol: String,
    signature: FfiSignature,
    origin: String,
    resolved: OnceLock<Resolved>,
}

fn string_arg(vm: &VM, value: Value) -> Option<&str> {
    let reference = GcRef::new(value.as_ptr()?);
    match &vm.heap.get(reference)?.kind {
        ObjectKind::String(text) => Some(text.as_str()),
        _ => None,
    }
}

pub(crate) fn bind(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let text = |vm: &VM, index: usize| {
        string_arg(vm, args[index])
            .map(str::to_string)
            .ok_or_else(|| {
                vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "extern declaration",
                    expected: "string",
                    got: vm.value_type_name(args[index]).to_string(),
                })
            })
    };
    let library = text(vm, 0)?;
    let symbol = text(vm, 1)?;
    let encoded = text(vm, 2)?;
    let origin = text(vm, 3)?;
    let signature = FfiSignature::decode(&encoded).ok_or_else(|| {
        vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "malformed C signature '{encoded}' for '{symbol}'"
        )))
    })?;
    let arity = u16::try_from(signature.params.len()).map_err(|_| {
        vm.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
            count: signature.params.len(),
            max: u16::MAX,
        })
    })?;
    let index = u32::try_from(vm.ffi_functions.len()).map_err(|_| {
        vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
            "too many extern functions".to_string(),
        ))
    })?;
    vm.ffi_functions.push(Arc::new(FfiFunction {
        library,
        symbol: symbol.clone(),
        signature,
        origin,
        resolved: OnceLock::new(),
    }));
    let slot = vm.register_ffi_impl(NativeFunctionImpl::Ffi(index));
    let object = GcObject::new(ObjectKind::Native(NativeFunction::with_slot(
        symbol, arity, slot,
    )));
    let reference = vm.alloc_object(object)?;
    Ok(Value::ptr(reference.index()))
}

fn search_dirs(vm: &VM, origin: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut add = |file: &std::path::Path| {
        if let Some(parent) = file.parent()
            && parent.is_dir()
        {
            let dir = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    };
    let origin = std::path::Path::new(origin);
    if origin.is_file() {
        add(origin);
    }
    if let Some(script) = vm.script_path() {
        add(std::path::Path::new(script));
    }
    dirs
}

fn manifest_path(vm: &VM, library: &str) -> Option<PathBuf> {
    let script = std::path::Path::new(vm.script_path()?);
    let manifest = aelys_modules::manifest::Manifest::for_source_file(script)?;
    let path = manifest.library.get(library)?.path.as_ref()?;
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return Some(path);
    }
    Some(script.parent()?.join(path))
}

fn variadic_stub(target: usize, vector_registers: u8) -> Result<usize, String> {
    static STUBS: OnceLock<Mutex<HashMap<(usize, u8), usize>>> = OnceLock::new();
    let stubs = STUBS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut stubs = stubs
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(&entry) = stubs.get(&(target, vector_registers)) {
        return Ok(entry);
    }
    let entry = trampoline::variadic_entry(target, vector_registers)?;
    stubs.insert((target, vector_registers), entry);
    Ok(entry)
}

fn resolve(vm: &VM, function: &FfiFunction) -> Result<Resolved, RuntimeError> {
    if let Some(resolved) = function.resolved.get() {
        return Ok(*resolved);
    }
    let dirs = search_dirs(vm, &function.origin);
    let opened = library::open(
        &function.library,
        &dirs,
        manifest_path(vm, &function.library),
    )
    .map_err(|missing| {
        vm.runtime_error(RuntimeErrorKind::FfiLibraryNotFound {
            library: function.library.clone(),
            function: function.symbol.clone(),
            tried: missing.tried,
            reason: missing.reason,
        })
    })?;
    let target = unsafe {
        opened
            .library
            .get::<unsafe extern "C" fn()>(function.symbol.as_bytes())
            .map(|symbol| *symbol as usize)
    }
    .map_err(|_| {
        vm.runtime_error(RuntimeErrorKind::FfiSymbolNotFound {
            library: function.library.clone(),
            path: opened.path.clone(),
            symbol: function.symbol.clone(),
        })
    })?;
    let failure = |message: String| {
        vm.runtime_error(RuntimeErrorKind::FfiCall {
            function: function.symbol.clone(),
            message,
        })
    };
    let entry = if function.signature.variadic {
        let vector_registers = function
            .signature
            .params
            .iter()
            .filter(|param| param.is_float())
            .count();
        variadic_stub(target, u8::try_from(vector_registers.min(8)).unwrap_or(8))
            .map_err(failure)?
    } else {
        target
    };
    let trampoline = trampoline::trampoline(&function.signature.params, function.signature.ret)
        .map_err(|message| failure(format!("cannot generate the call trampoline: {message}")))?;
    let resolved = Resolved { entry, trampoline };
    let _ = function.resolved.set(resolved);
    Ok(resolved)
}

fn store<const N: usize>(slot: &mut u64, bytes: [u8; N]) {
    let mut wide = [0u8; 8];
    wide[..N].copy_from_slice(&bytes);
    *slot = u64::from_ne_bytes(wide);
}

fn argument_error(vm: &VM, function: &FfiFunction, index: usize, message: String) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::FfiCall {
        function: function.symbol.clone(),
        message: format!("argument {}: {message}", index + 1),
    })
}

pub(crate) fn integer_bits(ty: FfiType, value: i128) -> Option<[u8; 8]> {
    let (low, high) = ty.int_range()?;
    if value < low || value > high {
        return None;
    }
    let mut bytes = [0u8; 8];
    match ty.width_bytes() {
        1 => bytes[..1].copy_from_slice(&(value as u8).to_ne_bytes()),
        2 => bytes[..2].copy_from_slice(&(value as u16).to_ne_bytes()),
        4 => bytes[..4].copy_from_slice(&(value as u32).to_ne_bytes()),
        _ => bytes.copy_from_slice(&(value as u64).to_ne_bytes()),
    }
    Some(bytes)
}

pub(crate) fn address(vm: &VM, value: Value) -> Option<u64> {
    u64::try_from(vm.int_value(value)?).ok()
}

pub(crate) fn call(vm: &mut VM, index: u32, args: &[Value]) -> Result<Value, RuntimeError> {
    let Some(function) = vm.ffi_functions.get(index as usize).map(Arc::as_ptr) else {
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
            "unknown extern function".to_string(),
        )));
    };
    // entries are never removed and each lives in its own allocation, so it outlives every use of the vm below
    let function: &FfiFunction = unsafe { &*function };
    let resolved = resolve(vm, function)?;
    let signature = &function.signature;
    if args.len() != signature.params.len() {
        return Err(vm.runtime_error(RuntimeErrorKind::ArityMismatch {
            expected: u16::try_from(signature.params.len()).unwrap_or(u16::MAX),
            got: u16::try_from(args.len()).unwrap_or(u16::MAX),
        }));
    }
    let mut slots: SmallVec<[u64; 8]> = SmallVec::from_elem(0, args.len());
    let mut strings: SmallVec<[CString; 2]> = SmallVec::new();
    let mut lent: SmallVec<[(usize, bool); 2]> = SmallVec::new();
    for (index, (&ty, &value)) in signature.params.iter().zip(args).enumerate() {
        let slot = &mut slots[index];
        match ty {
            FfiType::Bool => match value.as_bool() {
                Some(flag) => store(slot, [u8::from(flag)]),
                None => {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected a bool".into(),
                    ));
                }
            },
            _ if ty.is_integer() => {
                let integer = match value.as_bool() {
                    Some(flag) => Some(i128::from(flag)),
                    None => vm.int_value(value),
                };
                let Some(integer) = integer else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected an integer".into(),
                    ));
                };
                let Some(bits) = integer_bits(ty, integer) else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        format!("{integer} does not fit in {ty}"),
                    ));
                };
                *slot = u64::from_ne_bytes(bits);
            }
            FfiType::F32 | FfiType::F64 => {
                let Some(float) = value.as_float() else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected a float".into(),
                    ));
                };
                if ty == FfiType::F32 {
                    store(slot, (float as f32).to_ne_bytes());
                } else {
                    store(slot, float.to_ne_bytes());
                }
            }
            FfiType::Ptr => match address(vm, value) {
                Some(address) => store(slot, address.to_ne_bytes()),
                None => {
                    return Err(argument_error(vm, function, index, "expected a ptr".into()));
                }
            },
            FfiType::CStr => {
                let Some(text) = string_arg(vm, value) else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected a string".into(),
                    ));
                };
                let owned = match CString::new(text) {
                    Ok(owned) => owned,
                    Err(error) => {
                        return Err(argument_error(
                            vm,
                            function,
                            index,
                            format!(
                                "the string holds a NUL byte at offset {}, which a cstr cannot carry",
                                error.nul_position()
                            ),
                        ));
                    }
                };
                store(slot, (owned.as_ptr() as usize as u64).to_ne_bytes());
                strings.push(owned);
            }
            FfiType::Bytes | FfiType::BytesMut => {
                let exclusive = ty == FfiType::BytesMut;
                let Some(object) = value.as_ptr() else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected a Vec<u8>".into(),
                    ));
                };
                if lent
                    .iter()
                    .any(|(other, held)| *other == object && (exclusive || *held))
                {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "byte buffer borrowed mutably while another borrow of it is live in the same call"
                            .into(),
                    ));
                }
                let Some((data, _)) = crate::vm::byte_vec::lend_bytes(vm, value, exclusive) else {
                    return Err(argument_error(
                        vm,
                        function,
                        index,
                        "expected a Vec<u8>".into(),
                    ));
                };
                lent.push((object, exclusive));
                store(slot, (data as usize as u64).to_ne_bytes());
            }
            _ => {
                return Err(argument_error(
                    vm,
                    function,
                    index,
                    format!("cannot pass {ty}"),
                ));
            }
        }
    }
    let mut out: u64 = 0;
    unsafe {
        (resolved.trampoline)(resolved.entry as *const u8, slots.as_ptr(), &mut out);
    }
    // a returned cstr may point into an argument copy (strchr, strstr), so it is read before the copies are freed
    let result = convert_result(vm, function, out);
    drop(strings);
    result
}

fn convert_result(vm: &mut VM, function: &FfiFunction, out: u64) -> Result<Value, RuntimeError> {
    let bytes = out.to_ne_bytes();
    match function.signature.ret {
        FfiType::Void => Ok(Value::unit()),
        FfiType::Bool => Ok(Value::bool(out != 0)),
        FfiType::U64 => vm.make_wide_int(i128::from(out)),
        FfiType::Ptr => vm.make_wide_int(i128::from(out)),
        ty if ty.is_integer() => vm.make_int(out as i64),
        FfiType::F32 => {
            let mut narrow = [0u8; 4];
            narrow.copy_from_slice(&bytes[..4]);
            Ok(Value::float(f64::from(f32::from_ne_bytes(narrow))))
        }
        FfiType::F64 => Ok(Value::float(f64::from_ne_bytes(bytes))),
        FfiType::CStr => {
            let text = read_c_string(out as usize).map_err(|message| {
                vm.runtime_error(RuntimeErrorKind::FfiCall {
                    function: function.symbol.clone(),
                    message: format!("returned {message}"),
                })
            })?;
            let reference = vm.alloc_string(&text)?;
            Ok(Value::ptr(reference.index()))
        }
        other => Err(vm.runtime_error(RuntimeErrorKind::FfiCall {
            function: function.symbol.clone(),
            message: format!("cannot return {other}"),
        })),
    }
}

pub(crate) fn read_c_string(address: usize) -> Result<String, String> {
    if address == 0 {
        return Err(
            "a null cstr; declare the result `ptr` and test `is_null()` when NULL is possible"
                .to_string(),
        );
    }
    let text = unsafe { CStr::from_ptr(address as *const std::ffi::c_char) };
    text.to_str().map(str::to_string).map_err(|error| {
        format!(
            "a string that is not valid UTF-8 (byte {} of {})",
            error.valid_up_to(),
            text.to_bytes().len()
        )
    })
}
