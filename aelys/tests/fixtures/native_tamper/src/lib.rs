use aelys_native::{
    value_int, AelysExport, AelysExportKind, AelysModuleDescriptor, AelysValue, AELYS_ABI_VERSION,
};
use core::ffi::c_void;

static MODULE_NAME: &[u8] = b"tamper\0";
static MODULE_VERSION: &[u8] = b"0.1.0\0";
static EXPORT_NAME: &[u8] = b"ok\0";

extern "C" fn tamper_ok(
    _vm: *mut c_void,
    _args: *const AelysValue,
    _arg_count: usize,
    out: *mut AelysValue,
) -> i32 {
    unsafe {
        *out = value_int(7).expect("fixture integer fits");
    }
    0
}

static EXPORTS: [AelysExport; 1] = [AelysExport {
    name: EXPORT_NAME.as_ptr() as *const i8,
    kind: AelysExportKind::Function,
    arity: 0,
    _padding: [0; 2],
    value: tamper_ok as *const c_void,
}];

#[unsafe(no_mangle)]
pub static aelys_module_descriptor: AelysModuleDescriptor = unsafe {
    AelysModuleDescriptor::from_raw_parts(
        AELYS_ABI_VERSION,
        core::mem::size_of::<AelysModuleDescriptor>() as u32,
        MODULE_NAME.as_ptr() as *const i8,
        MODULE_VERSION.as_ptr() as *const i8,
        core::ptr::null(),
        core::ptr::null(),
        0,
        1,
        EXPORTS.len() as u32,
        EXPORTS.as_ptr(),
        0,
        core::ptr::null(),
        None,
    )
};
