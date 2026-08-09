use aelys_native::{
    value_int, AelysExport, AelysExportKind, AelysModuleDescriptor, AelysRequiredModule,
    AelysValue, AELYS_ABI_VERSION,
};
use core::ffi::c_void;

static MODULE_NAME: &[u8] = b"cycle_a\0";
static MODULE_VERSION: &[u8] = b"0.1.0\0";
static EXPORT_NAME: &[u8] = b"a\0";

static DEP_NAME: &[u8] = b"cycle_b\0";

extern "C" fn cycle_a_fn(
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
    value: cycle_a_fn as *const c_void,
}];

static REQUIRED: [AelysRequiredModule; 1] = [AelysRequiredModule {
    name: DEP_NAME.as_ptr() as *const i8,
    version_req: core::ptr::null(),
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
        REQUIRED.len() as u32,
        REQUIRED.as_ptr(),
        None,
    )
};

aelys_native::aelys_init_exports_hash!(aelys_module_descriptor);
