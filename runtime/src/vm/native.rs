use super::{GcRef, ObjectKind, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::{AelysNativeFn, AelysValue, AelysVmApi};

const NATIVE_CONTEXT_MAGIC: u64 = 0x4145_4c59_535f_5633;

#[repr(C)]
struct RuntimeNativeContext {
    magic: u64,
    vm: *mut VM,
}

pub type NativeFn = fn(&mut VM, &[Value]) -> Result<Value, RuntimeError>;

#[derive(Clone, Copy)]
pub enum NativeFunctionImpl {
    Rust(NativeFn),
    Foreign(AelysNativeFn),
}

impl NativeFunctionImpl {
    pub fn call(self, vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
        match self {
            NativeFunctionImpl::Rust(f) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(vm, args)))
                    .map_err(|_| vm.runtime_error(RuntimeErrorKind::NativePanic))?
            }
            NativeFunctionImpl::Foreign(f) => {
                let mut native_args = Vec::with_capacity(args.len());
                for arg in args {
                    native_args.push(unsafe { std::mem::transmute::<Value, AelysValue>(*arg) });
                }
                let mut out = aelys_native::value_null();
                let mut context = RuntimeNativeContext {
                    magic: NATIVE_CONTEXT_MAGIC,
                    vm,
                };
                let status = f(
                    (&mut context as *mut RuntimeNativeContext).cast(),
                    native_args.as_ptr(),
                    native_args.len(),
                    &mut out,
                );
                if status != 0 {
                    return Err(vm.runtime_error(RuntimeErrorKind::NativeError { code: status }));
                }
                vm.import_native_value(out)
            }
        }
    }
}

/// C callback for native modules to read string values from the VM.
extern "C" fn native_read_string_callback(
    context: *mut aelys_native::NativeContext,
    value: AelysValue,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    if context.is_null() {
        return 1;
    }
    let context = unsafe { &*(context as *const RuntimeNativeContext) };
    if context.magic != NATIVE_CONTEXT_MAGIC || context.vm.is_null() {
        return 1;
    }
    let vm = unsafe { &*context.vm };
    let val = unsafe { std::mem::transmute::<AelysValue, Value>(value) };
    if let Some(ptr_idx) = val.as_ptr()
        && let Some(obj) = vm.heap.get(GcRef::new(ptr_idx))
        && let ObjectKind::String(s) = &obj.kind
    {
        let str_ref = s.as_str();
        unsafe {
            *out_ptr = str_ref.as_ptr();
            *out_len = str_ref.len();
        }
        return 0;
    }
    1
}

// VM API struct to pass to native modules during init
pub fn build_native_vm_api() -> AelysVmApi {
    AelysVmApi {
        api_version: aelys_native::AELYS_API_VERSION,
        size: u32::try_from(std::mem::size_of::<AelysVmApi>())
            .expect("native ABI table size fits u32"),
        register_function: None,
        register_constant: None,
        register_type: None,
        alloc_string: None,
        read_string: Some(native_read_string_callback),
        _reserved: [0; 3],
    }
}

impl VM {
    pub fn import_native_value(&self, value: AelysValue) -> Result<Value, RuntimeError> {
        let value = unsafe { std::mem::transmute::<AelysValue, Value>(value) };
        if let Some(raw) = value.as_ptr()
            && self.heap.get(GcRef::new(raw)).is_none()
        {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
        }
        Ok(value)
    }
}
