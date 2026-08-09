use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_modules::native::{
    NativeError, NativeExport, descriptor_module_name, validate_descriptor,
};
use aelys_native::{AelysExportKind, AelysInitFn, AelysModuleDescriptor, AelysNativeFn};
use aelys_syntax::{Source, Span};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

pub struct NativeModuleRegistration {
    pub alias: String,
    pub functions: Vec<(String, u16, AelysNativeFn)>,
}

impl NativeModuleRegistration {
    pub unsafe fn validate(
        descriptor: &'static AelysModuleDescriptor,
    ) -> Result<ValidatedNativeModule, AelysError> {
        // SAFETY: forwarded to this function's own contract, the caller guarantees a `#[aelys_module]` descriptor whose module outlives the process
        let contents = unsafe { validate_descriptor(descriptor, None) }.map_err(|error| {
            // SAFETY: same contract; the helper re-applies the ABI and size gates itself and gives up rather than read `module_name` out of a layout it does not recognise
            let name = unsafe { descriptor_module_name(descriptor) };
            invalid_module(name.as_deref().unwrap_or(UNKNOWN_MODULE), error)
        })?;

        if !contents.required_modules.is_empty() {
            return Err(invalid_module_reason(
                &contents.name,
                "statically linked modules cannot declare dependencies",
            ));
        }

        Ok(ValidatedNativeModule {
            alias: contents.name,
            exports: contents.exports,
            init: descriptor.init(),
        })
    }

    pub fn qualified_names(&self) -> HashSet<String> {
        self.functions
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    }
}

pub struct ValidatedNativeModule {
    alias: String,
    exports: HashMap<String, NativeExport>,
    init: Option<AelysInitFn>,
}

impl ValidatedNativeModule {
    pub fn alias(&self) -> &str {
        &self.alias
    }
    pub fn initialize(self) -> Result<NativeModuleRegistration, AelysError> {
        let Self {
            alias,
            exports,
            init,
        } = self;

        if let Some(init) = init
            && init(&aelys_runtime::build_native_vm_api()) != 0
        {
            return Err(invalid_module_reason(
                &alias,
                "module rejected the Aelys VM API",
            ));
        }

        let mut functions = Vec::with_capacity(exports.len());
        for (name, export) in exports {
            if export.kind != AelysExportKind::Function || export.value.is_null() {
                continue;
            }
            // SAFETY: `validate_descriptor` has checked that this export is a function whose pointer is non-null, aligned and inside the user-space address range, and `#[aelys_module]`
            // only ever stores an `AelysNativeFn` in the value slot of a `Function` export, so the pointer already has that ABI. the contract accepted at validate keeps the code it addresses alive.
            let function =
                unsafe { std::mem::transmute::<*const c_void, AelysNativeFn>(export.value) };
            functions.push((format!("{alias}::{name}"), export.arity, function));
        }
        // `validate_descriptor` returns exports in a `HashMap`; sort so the resolved order is deterministic across runs.
        functions.sort_by(|(left, _, _), (right, _, _)| left.cmp(right));

        Ok(NativeModuleRegistration { alias, functions })
    }
}

const UNKNOWN_MODULE: &str = "<unknown>";

fn invalid_module(module: &str, error: NativeError) -> AelysError {
    invalid_module_reason(module, &error.to_string())
}

pub(crate) fn invalid_module_reason(module: &str, reason: &str) -> AelysError {
    AelysError::Compile(CompileError::new(
        CompileErrorKind::InvalidNativeModule {
            module: module.to_owned(),
            reason: reason.to_owned(),
        },
        Span::dummy(),
        Source::new("<native-module>", ""),
    ))
}
