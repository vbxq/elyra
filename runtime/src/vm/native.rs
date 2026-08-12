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
pub enum ForeignPayloadKind {
    Int,
    Float,
    Bool,
    String,
    Unit,
}

impl ForeignPayloadKind {
    fn expected_name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Unit => "unit",
        }
    }
}

#[derive(Clone, Copy)]
pub enum ForeignReturnKind {
    Raw,
    Scalar(ForeignPayloadKind),
    Option(ForeignPayloadKind),
    Result {
        ok: ForeignPayloadKind,
        err: ForeignPayloadKind,
    },
}

impl ForeignReturnKind {
    pub fn from_native_type(value: Option<aelys_native::AelysNativeType>) -> Self {
        match value {
            Some(aelys_native::AelysNativeType::Int) => Self::Scalar(ForeignPayloadKind::Int),
            Some(aelys_native::AelysNativeType::Float) => Self::Scalar(ForeignPayloadKind::Float),
            Some(aelys_native::AelysNativeType::Bool) => Self::Scalar(ForeignPayloadKind::Bool),
            Some(aelys_native::AelysNativeType::String) => Self::Scalar(ForeignPayloadKind::String),
            Some(aelys_native::AelysNativeType::Unit) => Self::Scalar(ForeignPayloadKind::Unit),
            Some(aelys_native::AelysNativeType::OptionInt) => Self::Option(ForeignPayloadKind::Int),
            Some(aelys_native::AelysNativeType::OptionFloat) => {
                Self::Option(ForeignPayloadKind::Float)
            }
            Some(aelys_native::AelysNativeType::OptionBool) => {
                Self::Option(ForeignPayloadKind::Bool)
            }
            Some(aelys_native::AelysNativeType::OptionString) => {
                Self::Option(ForeignPayloadKind::String)
            }
            Some(aelys_native::AelysNativeType::OptionUnit) => {
                Self::Option(ForeignPayloadKind::Unit)
            }
            Some(aelys_native::AelysNativeType::ResultIntString) => Self::Result {
                ok: ForeignPayloadKind::Int,
                err: ForeignPayloadKind::String,
            },
            Some(aelys_native::AelysNativeType::ResultFloatString) => Self::Result {
                ok: ForeignPayloadKind::Float,
                err: ForeignPayloadKind::String,
            },
            Some(aelys_native::AelysNativeType::ResultBoolString) => Self::Result {
                ok: ForeignPayloadKind::Bool,
                err: ForeignPayloadKind::String,
            },
            Some(aelys_native::AelysNativeType::ResultStringString) => Self::Result {
                ok: ForeignPayloadKind::String,
                err: ForeignPayloadKind::String,
            },
            Some(aelys_native::AelysNativeType::ResultUnitString) => Self::Result {
                ok: ForeignPayloadKind::Unit,
                err: ForeignPayloadKind::String,
            },
            Some(aelys_native::AelysNativeType::Dynamic) | None => Self::Raw,
        }
    }
}

#[derive(Clone, Copy)]
pub enum NativeFunctionImpl {
    Rust(NativeFn),
    Foreign {
        function: AelysNativeFn,
        result: ForeignReturnKind,
    },
}

impl NativeFunctionImpl {
    pub fn call(self, vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
        let value = match self {
            NativeFunctionImpl::Rust(f) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(vm, args)))
                    .map_err(|_| vm.runtime_error(RuntimeErrorKind::NativePanic))?
            }
            NativeFunctionImpl::Foreign { function, result } => {
                let mut native_args = Vec::with_capacity(args.len());
                for arg in args {
                    native_args.push(unsafe { std::mem::transmute::<Value, AelysValue>(*arg) });
                }
                let mut out = aelys_native::value_null();
                let mut context = RuntimeNativeContext {
                    magic: NATIVE_CONTEXT_MAGIC,
                    vm,
                };
                let status = unsafe {
                    function(
                        (&mut context as *mut RuntimeNativeContext).cast(),
                        native_args.as_ptr(),
                        native_args.len(),
                        &mut out,
                    )
                };
                if status != 0 {
                    return match result {
                        ForeignReturnKind::Result { err, .. }
                            if status == aelys_native::AELYS_NATIVE_RESULT_ERROR =>
                        {
                            let message =
                                vm.validate_foreign_payload(vm.import_native_value(out)?, err)?;
                            let sum =
                                vm.alloc_sum(aelys_bytecode::object::SumTag::ResultErr, message)?;
                            Ok(Value::ptr(sum.index()))
                        }
                        ForeignReturnKind::Option(_)
                            if status == aelys_native::AELYS_NATIVE_OPTION_NONE =>
                        {
                            Ok(Value::none())
                        }
                        _ => Err(vm.runtime_error(RuntimeErrorKind::NativeError { code: status })),
                    };
                }
                let value = vm.import_native_value(out)?;
                match result {
                    ForeignReturnKind::Raw => Ok(value),
                    ForeignReturnKind::Scalar(expected) => {
                        vm.validate_foreign_payload(value, expected)
                    }
                    ForeignReturnKind::Option(expected) => {
                        let value = vm.validate_foreign_payload(value, expected)?;
                        let sum =
                            vm.alloc_sum(aelys_bytecode::object::SumTag::OptionSome, value)?;
                        Ok(Value::ptr(sum.index()))
                    }
                    ForeignReturnKind::Result { ok, .. } => {
                        let value = vm.validate_foreign_payload(value, ok)?;
                        let sum = vm.alloc_sum(aelys_bytecode::object::SumTag::ResultOk, value)?;
                        Ok(Value::ptr(sum.index()))
                    }
                }
            }
        }?;

        if value.is_null() {
            return Err(vm.runtime_error(RuntimeErrorKind::NativeReturnedNull));
        }
        Ok(value)
    }
}

/// C callback for native modules to read string values from the VM.
extern "C" fn native_read_string_callback(
    context: *mut aelys_native::NativeContext,
    value: AelysValue,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    if context.is_null() || out_ptr.is_null() || out_len.is_null() {
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

extern "C" fn native_alloc_string_callback(
    context: *mut aelys_native::NativeContext,
    bytes: *const u8,
    length: usize,
    out: *mut AelysValue,
) -> i32 {
    if context.is_null() || out.is_null() || (bytes.is_null() && length != 0) {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let context = unsafe { &mut *(context as *mut RuntimeNativeContext) };
    if context.magic != NATIVE_CONTEXT_MAGIC || context.vm.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    let Ok(value) = std::str::from_utf8(bytes) else {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    };
    let vm = unsafe { &mut *context.vm };
    let Ok(reference) = vm.alloc_string(value) else {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    };
    unsafe {
        *out = std::mem::transmute::<Value, AelysValue>(Value::ptr(reference.index()));
    }
    0
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
        alloc_string: Some(native_alloc_string_callback),
        read_string: Some(native_read_string_callback),
        _reserved: [0; 3],
    }
}

impl VM {
    fn validate_foreign_payload(
        &self,
        value: Value,
        expected: ForeignPayloadKind,
    ) -> Result<Value, RuntimeError> {
        if value.is_null() {
            return Err(self.runtime_error(RuntimeErrorKind::NativeReturnedNull));
        }
        let valid = match expected {
            ForeignPayloadKind::Int => value.is_int(),
            ForeignPayloadKind::Float => value.is_float(),
            ForeignPayloadKind::Bool => value.is_bool(),
            ForeignPayloadKind::Unit => value.is_unit(),
            ForeignPayloadKind::String => value.as_ptr().is_some_and(|pointer| {
                self.heap
                    .get(GcRef::new(pointer))
                    .is_some_and(|object| matches!(&object.kind, ObjectKind::String(_)))
            }),
        };
        if valid {
            return Ok(value);
        }
        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "native return",
            expected: expected.expected_name(),
            got: self.value_type_name(value).to_string(),
        }))
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use aelys_syntax::Source;

    fn legacy_null(vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
        let _ = vm;
        Ok(Value::null())
    }

    unsafe extern "C" fn foreign_result_ok(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe {
            *out = aelys_native::value_int(7).unwrap();
        }
        0
    }

    unsafe extern "C" fn foreign_result_err(
        context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        let _ = unsafe { allocate_string(context, out, "error") };
        aelys_native::AELYS_NATIVE_RESULT_ERROR
    }

    unsafe fn allocate_string(
        context: *mut aelys_native::NativeContext,
        out: *mut AelysValue,
        value: &str,
    ) -> i32 {
        let api = build_native_vm_api();
        let Some(alloc) = api.alloc_string else {
            return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
        };
        alloc(context, value.as_ptr(), value.len(), out)
    }

    unsafe extern "C" fn foreign_option_string_some(
        context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe { allocate_string(context, out, "option") }
    }

    unsafe extern "C" fn foreign_result_string_ok(
        context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe { allocate_string(context, out, "ok") }
    }

    unsafe extern "C" fn foreign_result_string_err(
        context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        let _ = unsafe { allocate_string(context, out, "error") };
        aelys_native::AELYS_NATIVE_RESULT_ERROR
    }

    unsafe extern "C" fn foreign_result_string_wrong_payload(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe {
            *out = aelys_native::value_int(7).unwrap();
        }
        0
    }

    unsafe extern "C" fn foreign_option_string_null_payload(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe {
            *out = aelys_native::value_null();
        }
        0
    }

    unsafe extern "C" fn foreign_option_none(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        _out: *mut AelysValue,
    ) -> i32 {
        aelys_native::AELYS_NATIVE_OPTION_NONE
    }

    unsafe extern "C" fn foreign_option_unit_some(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe {
            *out = aelys_native::value_unit();
        }
        0
    }

    unsafe extern "C" fn foreign_result_unit_ok(
        _context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        unsafe {
            *out = aelys_native::value_unit();
        }
        0
    }

    unsafe extern "C" fn foreign_result_unit_err(
        context: *mut aelys_native::NativeContext,
        _args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        let _ = unsafe { allocate_string(context, out, "unit error") };
        aelys_native::AELYS_NATIVE_RESULT_ERROR
    }

    #[test]
    fn read_string_callback_rejects_null_output_pointers() {
        let mut vm = VM::new(Source::new("native-null-output", "")).unwrap();
        let mut context = RuntimeNativeContext {
            magic: NATIVE_CONTEXT_MAGIC,
            vm: &mut vm,
        };
        let context = (&mut context as *mut RuntimeNativeContext).cast();
        let value = aelys_native::value_null();
        let mut pointer = std::ptr::null();
        let mut length = 0;

        assert_eq!(
            native_read_string_callback(context, value, std::ptr::null_mut(), &mut length),
            1
        );
        assert_eq!(
            native_read_string_callback(context, value, &mut pointer, std::ptr::null_mut()),
            1
        );
    }

    #[test]
    fn legacy_native_null_is_rejected() {
        let mut vm = VM::new(Source::new("native-null-result", "")).unwrap();
        let error = NativeFunctionImpl::Rust(legacy_null)
            .call(&mut vm, &[])
            .unwrap_err();
        assert!(matches!(error.kind, RuntimeErrorKind::NativeReturnedNull));
    }

    #[test]
    fn foreign_result_contract_returns_aelys_values() {
        let mut vm = VM::new(Source::new("native-result-contract", "")).unwrap();
        let success = NativeFunctionImpl::Foreign {
            function: foreign_result_ok,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::Int,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        let success_object = vm.heap.get(GcRef::new(success.as_ptr().unwrap())).unwrap();
        assert!(matches!(
            success_object.kind,
            ObjectKind::Sum(aelys_bytecode::object::AelysSum {
                tag: aelys_bytecode::object::SumTag::ResultOk,
                ..
            })
        ));

        let failure = NativeFunctionImpl::Foreign {
            function: foreign_result_err,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::Int,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        let failure_object = vm.heap.get(GcRef::new(failure.as_ptr().unwrap())).unwrap();
        assert!(matches!(
            failure_object.kind,
            ObjectKind::Sum(aelys_bytecode::object::AelysSum {
                tag: aelys_bytecode::object::SumTag::ResultErr,
                ..
            })
        ));
    }

    fn sum_payload(vm: &VM, value: Value, tag: aelys_bytecode::object::SumTag) -> Value {
        let pointer = value.as_ptr().expect("native result must be a sum");
        let object = vm.heap.get(GcRef::new(pointer)).expect("sum must be live");
        match &object.kind {
            ObjectKind::Sum(sum) => {
                assert_eq!(sum.tag, tag);
                sum.payload
            }
            other => panic!("expected sum, got {other:?}"),
        }
    }

    fn string_value(vm: &VM, value: Value) -> String {
        let pointer = value.as_ptr().expect("payload must be a string");
        let object = vm
            .heap
            .get(GcRef::new(pointer))
            .expect("string must be live");
        match &object.kind {
            ObjectKind::String(string) => string.as_str().to_string(),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn foreign_string_and_unit_sum_contracts_preserve_payloads() {
        let mut vm = VM::new(Source::new("native-typed-sums", "")).unwrap();
        let option_some = NativeFunctionImpl::Foreign {
            function: foreign_option_string_some,
            result: ForeignReturnKind::Option(ForeignPayloadKind::String),
        }
        .call(&mut vm, &[])
        .unwrap();
        let option_payload =
            sum_payload(&vm, option_some, aelys_bytecode::object::SumTag::OptionSome);
        assert_eq!(string_value(&vm, option_payload), "option");

        let option_none = NativeFunctionImpl::Foreign {
            function: foreign_option_none,
            result: ForeignReturnKind::Option(ForeignPayloadKind::String),
        }
        .call(&mut vm, &[])
        .unwrap();
        assert!(option_none.is_none());

        let result_ok = NativeFunctionImpl::Foreign {
            function: foreign_result_string_ok,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::String,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        let ok_payload = sum_payload(&vm, result_ok, aelys_bytecode::object::SumTag::ResultOk);
        assert_eq!(string_value(&vm, ok_payload), "ok");

        let result_err = NativeFunctionImpl::Foreign {
            function: foreign_result_string_err,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::String,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        let err_payload = sum_payload(&vm, result_err, aelys_bytecode::object::SumTag::ResultErr);
        assert_eq!(string_value(&vm, err_payload), "error");

        let option_unit = NativeFunctionImpl::Foreign {
            function: foreign_option_unit_some,
            result: ForeignReturnKind::Option(ForeignPayloadKind::Unit),
        }
        .call(&mut vm, &[])
        .unwrap();
        assert!(
            sum_payload(&vm, option_unit, aelys_bytecode::object::SumTag::OptionSome).is_unit()
        );

        let result_unit = NativeFunctionImpl::Foreign {
            function: foreign_result_unit_ok,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::Unit,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        assert!(sum_payload(&vm, result_unit, aelys_bytecode::object::SumTag::ResultOk).is_unit());

        let result_unit_err = NativeFunctionImpl::Foreign {
            function: foreign_result_unit_err,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::Unit,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap();
        let unit_err_payload = sum_payload(
            &vm,
            result_unit_err,
            aelys_bytecode::object::SumTag::ResultErr,
        );
        assert_eq!(string_value(&vm, unit_err_payload), "unit error");
    }

    #[test]
    fn foreign_sum_contracts_reject_wrong_and_null_payloads() {
        let mut vm = VM::new(Source::new("native-invalid-sum-payloads", "")).unwrap();
        let wrong_result = NativeFunctionImpl::Foreign {
            function: foreign_result_string_wrong_payload,
            result: ForeignReturnKind::Result {
                ok: ForeignPayloadKind::String,
                err: ForeignPayloadKind::String,
            },
        }
        .call(&mut vm, &[])
        .unwrap_err();
        assert!(matches!(
            wrong_result.kind,
            RuntimeErrorKind::TypeError {
                operation: "native return",
                expected: "string",
                ..
            }
        ));

        let null_option = NativeFunctionImpl::Foreign {
            function: foreign_option_string_null_payload,
            result: ForeignReturnKind::Option(ForeignPayloadKind::String),
        }
        .call(&mut vm, &[])
        .unwrap_err();
        assert!(matches!(
            null_option.kind,
            RuntimeErrorKind::NativeReturnedNull
        ));
    }
}
