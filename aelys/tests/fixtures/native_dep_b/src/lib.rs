use aelys_native::{
    value_int, AelysExport, AelysExportKind, AelysModuleDescriptor, AelysValue, AELYS_ABI_VERSION,
};
use core::ffi::c_void;

static MODULE_NAME: &[u8] = b"dep_b\0";
static MODULE_VERSION: &[u8] = b"1.0.0\0";
static EXPORT_NAME: &[u8] = b"id\0";

extern "C" fn dep_b_id(
    _vm: *mut c_void,
    _args: *const AelysValue,
    _arg_count: usize,
    out: *mut AelysValue,
) -> i32 {
    unsafe {
        *out = value_int(1).expect("fixture integer fits");
    }
    0
}

static EXPORTS: [AelysExport; 1] = [AelysExport {
    name: EXPORT_NAME.as_ptr() as *const i8,
    kind: AelysExportKind::Function,
    arity: 0,
    _padding: [0; 2],
    value: dep_b_id as *const c_void,
    signature: core::ptr::null(),
}];

#[unsafe(no_mangle)]
pub static mut aelys_module_descriptor: AelysModuleDescriptor = unsafe {
    AelysModuleDescriptor::from_raw_parts(
        AELYS_ABI_VERSION,
        core::mem::size_of::<AelysModuleDescriptor>() as u32,
        MODULE_NAME.as_ptr() as *const i8,
        MODULE_VERSION.as_ptr() as *const i8,
        core::ptr::null(),
        core::ptr::null(),
        0,
        0,
        EXPORTS.len() as u32,
        EXPORTS.as_ptr(),
        0,
        core::ptr::null(),
        None,
    )
};

aelys_native::aelys_init_exports_hash!(aelys_module_descriptor);
