// C ABI structs for native module descriptors

use crate::{AELYS_ABI_VERSION, AelysValue};
use core::ffi::{c_char, c_void};

#[repr(C)]
pub struct NativeContext {
    _private: [u8; 0],
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeHandle(u64);

pub type AelysNativeFn = unsafe extern "C" fn(
    context: *mut NativeContext,
    args: *const AelysValue,
    arg_count: usize,
    out: *mut AelysValue,
) -> i32;

pub type AelysInitFn = extern "C" fn(api: *const AelysVmApi) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AelysVmApi {
    pub api_version: u32,
    pub size: u32,
    pub register_function:
        Option<extern "C" fn(name: *const c_char, arity: u16, func: AelysNativeFn) -> i32>,
    pub register_constant: Option<extern "C" fn(name: *const c_char, value: AelysValue) -> i32>,
    pub register_type:
        Option<extern "C" fn(name: *const c_char, type_desc: *const AelysTypeDescriptor) -> i32>,
    pub alloc_string: Option<
        extern "C" fn(
            context: *mut NativeContext,
            bytes: *const u8,
            len: usize,
            out: *mut AelysValue,
        ) -> i32,
    >,
    pub read_string: Option<
        extern "C" fn(
            context: *mut NativeContext,
            value: AelysValue,
            out_ptr: *mut *const u8,
            out_len: *mut usize,
        ) -> i32,
    >,
    pub _reserved: [usize; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AelysTypeDescriptor {
    pub size: u32,
    pub drop: Option<extern "C" fn(value: *mut c_void)>,
    pub _reserved: [usize; 2],
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AelysExportKind {
    Function = 1,
    Constant = 2,
    Type = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AelysNativeType {
    Int = 1,
    Float = 2,
    Bool = 3,
    String = 4,
    Unit = 5,
    Dynamic = 6,
    OptionInt = 7,
    OptionFloat = 8,
    OptionBool = 9,
    OptionString = 10,
    OptionUnit = 11,
    ResultIntString = 12,
    ResultFloatString = 13,
    ResultBoolString = 14,
    ResultStringString = 15,
    ResultUnitString = 16,
}

impl TryFrom<u8> for AelysNativeType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Int),
            2 => Ok(Self::Float),
            3 => Ok(Self::Bool),
            4 => Ok(Self::String),
            5 => Ok(Self::Unit),
            6 => Ok(Self::Dynamic),
            7 => Ok(Self::OptionInt),
            8 => Ok(Self::OptionFloat),
            9 => Ok(Self::OptionBool),
            10 => Ok(Self::OptionString),
            11 => Ok(Self::OptionUnit),
            12 => Ok(Self::ResultIntString),
            13 => Ok(Self::ResultFloatString),
            14 => Ok(Self::ResultBoolString),
            15 => Ok(Self::ResultStringString),
            16 => Ok(Self::ResultUnitString),
            _ => Err(()),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AelysFunctionSignature {
    pub arity: u16,
    pub _padding: [u8; 2],
    pub params: *const u8,
    pub result: u8,
    pub _reserved: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AelysExport {
    pub name: *const c_char,
    pub kind: AelysExportKind,
    pub arity: u16,
    pub _padding: [u8; 2],
    pub value: *const c_void,
    pub signature: *const AelysFunctionSignature,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AelysRequiredModule {
    pub name: *const c_char,
    pub version_req: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct AelysModuleDescriptor {
    abi_version: u32,
    descriptor_size: u32,
    module_name: *const c_char,
    module_version: *const c_char,
    vm_version_min: *const c_char,
    vm_version_max: *const c_char,
    descriptor_hash: u64,
    exports_hash: u64,
    export_count: u32,
    exports: *const AelysExport,
    required_module_count: u32,
    required_modules: *const AelysRequiredModule,
    init: Option<AelysInitFn>,
}

impl AelysModuleDescriptor {
    pub const ABI_VERSION: u32 = AELYS_ABI_VERSION;

    /// Construct a descriptor at an FFI boundary.
    ///
    /// # Safety
    /// Every pointer must remain valid for the lifetime promised by the
    /// descriptor, and the counts and function pointers must describe the
    /// corresponding arrays and ABI exactly. Native module registration
    /// validates the resulting descriptor before using it.
    #[allow(clippy::too_many_arguments)]
    pub const unsafe fn from_raw_parts(
        abi_version: u32,
        descriptor_size: u32,
        module_name: *const c_char,
        module_version: *const c_char,
        vm_version_min: *const c_char,
        vm_version_max: *const c_char,
        descriptor_hash: u64,
        exports_hash: u64,
        export_count: u32,
        exports: *const AelysExport,
        required_module_count: u32,
        required_modules: *const AelysRequiredModule,
        init: Option<AelysInitFn>,
    ) -> Self {
        Self {
            abi_version,
            descriptor_size,
            module_name,
            module_version,
            vm_version_min,
            vm_version_max,
            descriptor_hash,
            exports_hash,
            export_count,
            exports,
            required_module_count,
            required_modules,
            init,
        }
    }

    pub const fn abi_version(&self) -> u32 {
        self.abi_version
    }

    pub const fn descriptor_size(&self) -> u32 {
        self.descriptor_size
    }

    pub const fn module_name(&self) -> *const c_char {
        self.module_name
    }

    pub const fn module_version(&self) -> *const c_char {
        self.module_version
    }

    pub const fn vm_version_min(&self) -> *const c_char {
        self.vm_version_min
    }

    pub const fn vm_version_max(&self) -> *const c_char {
        self.vm_version_max
    }

    pub const fn descriptor_hash(&self) -> u64 {
        self.descriptor_hash
    }

    pub const fn exports_hash(&self) -> u64 {
        self.exports_hash
    }

    pub const fn export_count(&self) -> u32 {
        self.export_count
    }

    pub const fn exports(&self) -> *const AelysExport {
        self.exports
    }

    pub const fn required_module_count(&self) -> u32 {
        self.required_module_count
    }

    pub const fn required_modules(&self) -> *const AelysRequiredModule {
        self.required_modules
    }

    pub const fn init(&self) -> Option<AelysInitFn> {
        self.init
    }

    pub(crate) fn set_exports_hash(&mut self, hash: u64) {
        self.exports_hash = hash;
    }
}

impl NativeHandle {
    pub(crate) const fn from_payload(payload: u64) -> Self {
        Self(payload)
    }

    pub(crate) const fn payload(self) -> u64 {
        self.0
    }
}

// SAFETY: these descriptors contain immutable C ABI metadata. Safe Rust can only
// mutate their public fields through exclusive access; dereferencing embedded raw
// pointers remains confined to explicitly unsafe loader and hashing operations.
unsafe impl Sync for AelysExport {}
unsafe impl Sync for AelysFunctionSignature {}
unsafe impl Sync for AelysRequiredModule {}
unsafe impl Sync for AelysModuleDescriptor {}
