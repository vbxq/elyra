// FFI types for native modules bump abi_version when changing struct layouts

pub use aelys_native_macros::{aelys_export, aelys_module};

pub const AELYS_ABI_VERSION: u32 = 5;
pub const AELYS_API_VERSION: u32 = 4;
pub const AELYS_NATIVE_INVALID_ARGUMENT: i32 = 1;
pub const AELYS_NATIVE_INTEGER_OVERFLOW: i32 = 2;
pub const AELYS_NATIVE_RESULT_ERROR: i32 = 3;
pub const AELYS_NATIVE_OPTION_NONE: i32 = 4;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AelysValue(u64);

impl AelysValue {
    pub(crate) const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub(crate) const fn bits(self) -> u64 {
        self.0
    }
}

mod abi;
mod hash;
mod macros;
mod value;

pub use abi::{
    AelysExport, AelysExportKind, AelysFunctionSignature, AelysInitFn, AelysModuleDescriptor,
    AelysNativeFn, AelysNativeType, AelysRequiredModule, AelysTypeDescriptor, AelysVmApi,
    NativeContext, NativeHandle,
};
pub use hash::{compute_exports_hash, init_descriptor_exports_hash};
pub use value::{
    value_as_bool, value_as_float, value_as_handle, value_as_int, value_bool, value_float,
    value_from_handle, value_int, value_is_bool, value_is_float, value_is_int, value_is_null,
    value_is_ptr, value_is_unit, value_null, value_unit,
};

use std::sync::OnceLock;

static VM_API: OnceLock<AelysVmApi> = OnceLock::new();

pub fn store_vm_api(api: &AelysVmApi) {
    let _ = VM_API.set(*api);
}

#[allow(clippy::missing_safety_doc)]
pub unsafe fn alloc_string_from_context(
    context: *mut NativeContext,
    value: &str,
) -> Result<AelysValue, i32> {
    let api = VM_API.get().ok_or(AELYS_NATIVE_INVALID_ARGUMENT)?;
    let alloc = api.alloc_string.ok_or(AELYS_NATIVE_INVALID_ARGUMENT)?;
    let mut output = value_null();
    let status = alloc(context, value.as_ptr(), value.len(), &mut output);
    if status == 0 { Ok(output) } else { Err(status) }
}

/// borrow a byte buffer allocated by `bytes::alloc`, valid only until the call returns and never resized or freed while held
pub unsafe fn borrow_bytes_from_handle<'a>(
    context: *mut NativeContext,
    handle: AelysValue,
) -> Option<&'a mut [u8]> {
    let api = VM_API.get()?;
    let borrow = api.borrow_bytes?;
    let mut pointer: *mut u8 = core::ptr::null_mut();
    let mut length: usize = 0;
    let status = borrow(context, handle, &mut pointer, &mut length);
    if status != 0 || pointer.is_null() {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts_mut(pointer, length) })
}

/// read a string value from the VM using the stored API. # safety `VM` must be a valid pointer to a live VM instance (clippy moment)
pub unsafe fn read_string_from_value(
    context: *mut NativeContext,
    value: AelysValue,
) -> Option<String> {
    let api = VM_API.get()?;
    let read_fn = api.read_string?;
    let mut ptr: *const u8 = core::ptr::null();
    let mut len: usize = 0;
    let status = read_fn(context, value, &mut ptr, &mut len);
    if status != 0 || ptr.is_null() {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    String::from_utf8(bytes.to_vec()).ok()
}
