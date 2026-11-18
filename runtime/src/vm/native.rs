use super::{GcRef, NativeFunction, ObjectKind, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::{AelysNativeFn, AelysValue, AelysVmApi};

const NATIVE_CONTEXT_MAGIC: u64 = 0x4145_4c59_535f_5633;
const INLINE_BORROWS_PER_CALL: usize = 8;
const INLINE_NATIVE_ARGS: usize = 8;

#[derive(Clone, Copy, Default)]
struct Borrow {
    handle: u64,
    exclusive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteParams {
    shared: u64,
    writable: u64,
    untyped: bool,
}

impl ByteParams {
    pub const UNTYPED: Self = Self {
        shared: 0,
        writable: 0,
        untyped: true,
    };
    pub const NONE: Self = Self {
        shared: 0,
        writable: 0,
        untyped: false,
    };

    pub fn from_params(params: Option<&[aelys_native::AelysNativeType]>) -> Self {
        let Some(params) = params else {
            return Self::UNTYPED;
        };
        let mut bytes = Self::NONE;
        for (index, param) in params.iter().enumerate().take(64) {
            match param {
                aelys_native::AelysNativeType::Bytes => bytes.shared |= 1 << index,
                aelys_native::AelysNativeType::BytesMut => bytes.writable |= 1 << index,
                _ => {}
            }
        }
        bytes
    }

    fn admits(self, args: &[Value], pointer: usize, exclusive: bool) -> bool {
        if self.untyped {
            return true;
        }
        let mask = if exclusive {
            self.writable
        } else {
            self.shared | self.writable
        };
        args.iter()
            .take(64)
            .enumerate()
            .any(|(index, arg)| mask & (1 << index) != 0 && arg.as_ptr() == Some(pointer))
    }
}

#[repr(C)]
struct RuntimeNativeContext {
    magic: u64,
    vm: *mut VM,
    args: *const Value,
    arg_count: usize,
    bytes: ByteParams,
    borrowed: [Borrow; INLINE_BORROWS_PER_CALL],
    borrowed_len: usize,
    spilled: Vec<Borrow>,
}

impl RuntimeNativeContext {
    #[cfg(test)]
    fn new(vm: *mut VM) -> Self {
        Self::for_call(vm, &[], ByteParams::UNTYPED)
    }

    fn for_call(vm: *mut VM, args: &[Value], bytes: ByteParams) -> Self {
        Self {
            magic: NATIVE_CONTEXT_MAGIC,
            vm,
            args: args.as_ptr(),
            arg_count: args.len(),
            bytes,
            borrowed: [Borrow::default(); INLINE_BORROWS_PER_CALL],
            borrowed_len: 0,
            spilled: Vec::new(),
        }
    }

    fn borrows(&self) -> impl Iterator<Item = &Borrow> {
        self.borrowed[..self.borrowed_len]
            .iter()
            .chain(&self.spilled)
    }

    fn find(&self, handle: u64) -> Option<Borrow> {
        self.borrows()
            .find(|borrow| borrow.handle == handle)
            .copied()
    }

    fn push(&mut self, borrow: Borrow) {
        if self.borrowed_len < INLINE_BORROWS_PER_CALL {
            self.borrowed[self.borrowed_len] = borrow;
            self.borrowed_len += 1;
        } else {
            self.spilled.push(borrow);
        }
    }
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
            Some(
                aelys_native::AelysNativeType::Dynamic
                | aelys_native::AelysNativeType::Bytes
                | aelys_native::AelysNativeType::BytesMut,
            )
            | None => Self::Raw,
        }
    }
}

#[derive(Clone, Copy)]
pub enum NativeFunctionImpl {
    Rust(NativeFn),
    Foreign {
        function: AelysNativeFn,
        result: ForeignReturnKind,
        bytes: ByteParams,
    },
}

impl NativeFunctionImpl {
    pub fn call(self, vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
        let value = match self {
            NativeFunctionImpl::Rust(f) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || f(vm, args),
            ))
            .map_err(|_| {
                vm.runtime_error(RuntimeErrorKind::NativePanic {
                    name: String::new(),
                })
            })?,
            NativeFunctionImpl::Foreign {
                function,
                result,
                bytes,
            } => {
                let roots_base = vm.native_call_roots.len();
                let value = Self::call_foreign(vm, function, result, bytes, args);
                vm.native_call_roots.truncate(roots_base);
                value
            }
        }?;

        if value.is_null() {
            return Err(vm.runtime_error(RuntimeErrorKind::NativeReturnedNull));
        }
        Ok(value)
    }

    fn call_foreign(
        vm: &mut VM,
        function: AelysNativeFn,
        result: ForeignReturnKind,
        bytes: ByteParams,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let mut out = aelys_native::value_null();
        let mut context = RuntimeNativeContext::for_call(vm, args, bytes);
        // both types are repr(transparent) over the same nan-boxed u64
        let status = unsafe {
            function(
                (&mut context as *mut RuntimeNativeContext).cast(),
                args.as_ptr().cast::<AelysValue>(),
                args.len(),
                &mut out,
            )
        };
        drop(context);
        if status != 0 {
            return match result {
                _ if status == aelys_native::AELYS_NATIVE_PANIC => {
                    Err(vm.runtime_error(RuntimeErrorKind::NativePanic {
                        name: String::new(),
                    }))
                }
                ForeignReturnKind::Result { err, .. }
                    if status == aelys_native::AELYS_NATIVE_RESULT_ERROR =>
                {
                    let message = vm.validate_foreign_payload(vm.import_native_value(out)?, err)?;
                    let sum = vm.alloc_sum(aelys_bytecode::object::SumTag::ResultErr, message)?;
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
            ForeignReturnKind::Scalar(expected) => vm.validate_foreign_payload(value, expected),
            ForeignReturnKind::Option(expected) => {
                let value = vm.validate_foreign_payload(value, expected)?;
                let sum = vm.alloc_sum(aelys_bytecode::object::SumTag::OptionSome, value)?;
                Ok(Value::ptr(sum.index()))
            }
            ForeignReturnKind::Result { ok, .. } => {
                let value = vm.validate_foreign_payload(value, ok)?;
                let sum = vm.alloc_sum(aelys_bytecode::object::SumTag::ResultOk, value)?;
                Ok(Value::ptr(sum.index()))
            }
        }
    }
}

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
    if context.is_null() || out.is_null() || bytes.is_null() {
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
    vm.native_call_roots.push(reference);
    unsafe {
        *out = std::mem::transmute::<Value, AelysValue>(Value::ptr(reference.index()));
    }
    0
}

extern "C" fn native_read_int_callback(
    context: *mut aelys_native::NativeContext,
    value: AelysValue,
    out: *mut i64,
) -> i32 {
    if context.is_null() || out.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let context = unsafe { &mut *(context as *mut RuntimeNativeContext) };
    if context.magic != NATIVE_CONTEXT_MAGIC || context.vm.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let vm = unsafe { &*context.vm };
    let value = unsafe { std::mem::transmute::<AelysValue, Value>(value) };
    match vm.int64_value(value) {
        Some(int) => {
            unsafe { *out = int };
            0
        }
        None => aelys_native::AELYS_NATIVE_INVALID_ARGUMENT,
    }
}

extern "C" fn native_alloc_int_callback(
    context: *mut aelys_native::NativeContext,
    value: i64,
    out: *mut AelysValue,
) -> i32 {
    if context.is_null() || out.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let context = unsafe { &mut *(context as *mut RuntimeNativeContext) };
    if context.magic != NATIVE_CONTEXT_MAGIC || context.vm.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let vm = unsafe { &mut *context.vm };
    let Ok(int) = vm.make_int(value) else {
        return aelys_native::AELYS_NATIVE_INTEGER_OVERFLOW;
    };
    if let Some(raw) = int.as_ptr() {
        vm.native_call_roots.push(GcRef::new(raw));
    }
    unsafe {
        *out = std::mem::transmute::<Value, AelysValue>(int);
    }
    0
}

extern "C" fn native_borrow_bytes_callback(
    context: *mut aelys_native::NativeContext,
    handle: AelysValue,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    borrow_bytes(context, handle, out_ptr, out_len, true)
}

extern "C" fn native_borrow_bytes_shared_callback(
    context: *mut aelys_native::NativeContext,
    handle: AelysValue,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    borrow_bytes(context, handle, out_ptr.cast(), out_len, false)
}

fn borrow_bytes(
    context: *mut aelys_native::NativeContext,
    handle: AelysValue,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    exclusive: bool,
) -> i32 {
    if context.is_null() || out_ptr.is_null() || out_len.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let context = unsafe { &mut *(context as *mut RuntimeNativeContext) };
    if context.magic != NATIVE_CONTEXT_MAGIC || context.vm.is_null() {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let vm = unsafe { &mut *context.vm };
    let value = unsafe { std::mem::transmute::<AelysValue, Value>(handle) };
    let args = if context.args.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(context.args, context.arg_count) }
    };
    let Some(pointer) = value.as_ptr() else {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    };
    if !context.bytes.admits(args, pointer, exclusive) {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    }
    let key = pointer as u64;
    let held = context.find(key);
    if held.is_some_and(|held| exclusive || held.exclusive) {
        return aelys_native::AELYS_NATIVE_BORROW_CONFLICT;
    }
    let Some((data, len)) = crate::vm::byte_vec::lend_bytes(vm, value, exclusive) else {
        return aelys_native::AELYS_NATIVE_INVALID_ARGUMENT;
    };
    if held.is_none() {
        context.push(Borrow {
            handle: key,
            exclusive,
        });
    }
    unsafe {
        *out_ptr = data;
        *out_len = len;
    }
    0
}

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
        borrow_bytes: Some(native_borrow_bytes_callback),
        borrow_bytes_shared: Some(native_borrow_bytes_shared_callback),
        _reserved: [0; 1],
        read_int: Some(native_read_int_callback),
        alloc_int: Some(native_alloc_int_callback),
    }
}

impl VM {
    pub(crate) fn native_impl(&self, native: &NativeFunction) -> Option<NativeFunctionImpl> {
        let slot = if native.slot == NativeFunction::UNREGISTERED {
            *self.native_registry.get(&native.name)?
        } else {
            native.slot
        };
        self.native_impls.get(usize::try_from(slot).ok()?).copied()
    }

    pub(crate) fn register_native_impl(&mut self, name: &str, function: NativeFunctionImpl) -> u32 {
        if let Some(&slot) = self.native_registry.get(name) {
            self.native_impls[slot as usize] = function;
            return slot;
        }
        let slot = u32::try_from(self.native_impls.len()).expect("native function count fits u32");
        self.native_impls.push(function);
        self.native_registry.insert(name.to_string(), slot);
        slot
    }

    #[inline]
    pub(crate) fn call_native_registers(
        &mut self,
        function: Option<NativeFunctionImpl>,
        callee: GcRef,
        first: usize,
        count: usize,
    ) -> Result<Value, RuntimeError> {
        let Some(function) = function else {
            let name = match self.heap.get(callee).map(|object| &object.kind) {
                Some(ObjectKind::Native(native)) => native.name.clone(),
                _ => "<native>".to_string(),
            };
            return Err(self.runtime_error(RuntimeErrorKind::UndefinedVariable(name)));
        };
        let Some(source) = self.registers.get(first..first + count) else {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: first + count,
                max: self.registers.len(),
            }));
        };
        let result = if count <= INLINE_NATIVE_ARGS {
            let mut inline = [Value::null(); INLINE_NATIVE_ARGS];
            inline[..count].copy_from_slice(source);
            function.call(self, &inline[..count])
        } else {
            let spilled = source.to_vec();
            function.call(self, &spilled)
        };
        result.map_err(|error| {
            self.name_panicking_native(error, |vm| match vm.heap.get(callee) {
                Some(object) => match &object.kind {
                    ObjectKind::Native(native) => native.name.clone(),
                    _ => String::new(),
                },
                None => String::new(),
            })
        })
    }

    pub(crate) fn name_panicking_native(
        &self,
        mut error: RuntimeError,
        name: impl FnOnce(&VM) -> String,
    ) -> RuntimeError {
        if let RuntimeErrorKind::NativePanic { name: slot } = &mut error.kind
            && slot.is_empty()
        {
            *slot = name(self);
        }
        error
    }

    fn validate_foreign_payload(
        &self,
        value: Value,
        expected: ForeignPayloadKind,
    ) -> Result<Value, RuntimeError> {
        if value.is_null() {
            return Err(self.runtime_error(RuntimeErrorKind::NativeReturnedNull));
        }
        let valid = match expected {
            ForeignPayloadKind::Int => self.is_int_value(value),
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
        let mut context = RuntimeNativeContext::new(&mut vm);
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
    fn alloc_string_callback_rejects_null_input_even_when_empty() {
        let mut vm = VM::new(Source::new("native-null-input", "")).unwrap();
        let mut context = RuntimeNativeContext::new(&mut vm);
        let context = (&mut context as *mut RuntimeNativeContext).cast();
        let mut output = aelys_native::value_null();

        assert_eq!(
            native_alloc_string_callback(context, std::ptr::null(), 0, &mut output),
            aelys_native::AELYS_NATIVE_INVALID_ARGUMENT
        );
    }

    fn byte_vec(vm: &mut VM, bytes: &[u8]) -> Value {
        let vector = crate::vm::byte_vec::bytes_vec(bytes);
        Value::ptr(vm.alloc_vec(vector).unwrap().index())
    }

    fn vec_ints(vm: &VM, value: Value) -> Vec<i64> {
        match &vm
            .heap
            .get(GcRef::new(value.as_ptr().unwrap()))
            .unwrap()
            .kind
        {
            ObjectKind::Vec(vector) => vector.data.as_ints().unwrap().to_vec(),
            other => panic!("expected a vec, got {other:?}"),
        }
    }

    fn raw(value: Value) -> AelysValue {
        unsafe { std::mem::transmute::<Value, AelysValue>(value) }
    }

    unsafe extern "C" fn foreign_write_789(
        context: *mut aelys_native::NativeContext,
        args: *const AelysValue,
        _arg_count: usize,
        out: *mut AelysValue,
    ) -> i32 {
        let mut pointer = std::ptr::null_mut();
        let mut length = 0;
        let status =
            native_borrow_bytes_callback(context, unsafe { *args }, &mut pointer, &mut length);
        if status != 0 {
            return status;
        }
        unsafe { std::slice::from_raw_parts_mut(pointer, length) }.copy_from_slice(&[7, 8, 9]);
        unsafe { *out = aelys_native::value_unit() };
        0
    }

    #[test]
    fn borrow_bytes_callback_hands_back_the_whole_vector() {
        let mut vm = VM::new(Source::new("native-borrow-bytes", "")).unwrap();
        let value = byte_vec(&mut vm, &[1, 2, 3, 255]);
        let mut context = RuntimeNativeContext::new(&mut vm);
        let context = (&mut context as *mut RuntimeNativeContext).cast();
        let mut pointer = std::ptr::null_mut();
        let mut length = 0;

        assert_eq!(
            native_borrow_bytes_callback(context, raw(value), &mut pointer, &mut length),
            0
        );
        assert_eq!(length, 4);
        let seen = unsafe { std::slice::from_raw_parts(pointer, length) };
        assert_eq!(seen, &[1, 2, 3, 255]);
    }

    #[test]
    fn a_mutable_byte_parameter_writes_back_into_the_vector() {
        let mut vm = VM::new(Source::new("native-borrow-bytes-write", "")).unwrap();
        let value = byte_vec(&mut vm, &[0, 0, 0]);
        NativeFunctionImpl::Foreign {
            function: foreign_write_789,
            result: ForeignReturnKind::Scalar(ForeignPayloadKind::Unit),
            bytes: ByteParams::from_params(Some(&[aelys_native::AelysNativeType::BytesMut])),
        }
        .call(&mut vm, &[value])
        .unwrap();
        assert_eq!(vec_ints(&vm, value), vec![7, 8, 9]);
    }

    #[test]
    fn a_shared_byte_parameter_cannot_be_borrowed_mutably() {
        let mut vm = VM::new(Source::new("native-borrow-bytes-shared", "")).unwrap();
        let value = byte_vec(&mut vm, &[1, 2, 3]);
        let error = NativeFunctionImpl::Foreign {
            function: foreign_write_789,
            result: ForeignReturnKind::Scalar(ForeignPayloadKind::Unit),
            bytes: ByteParams::from_params(Some(&[aelys_native::AelysNativeType::Bytes])),
        }
        .call(&mut vm, &[value])
        .unwrap_err();
        assert!(matches!(
            error.kind,
            RuntimeErrorKind::NativeError {
                code: aelys_native::AELYS_NATIVE_INVALID_ARGUMENT
            }
        ));
        assert_eq!(vec_ints(&vm, value), vec![1, 2, 3]);
    }

    #[test]
    fn borrow_bytes_callback_refuses_a_value_that_is_not_a_vector() {
        let mut vm = VM::new(Source::new("native-borrow-bytes-wrong", "")).unwrap();
        let text = Value::ptr(vm.alloc_string("abc").unwrap().index());
        let mut context = RuntimeNativeContext::new(&mut vm);
        let context = (&mut context as *mut RuntimeNativeContext).cast();
        let mut pointer = std::ptr::null_mut();
        let mut length = 0;

        for value in [
            aelys_native::value_int(3).unwrap(),
            aelys_native::value_null(),
            raw(text),
        ] {
            assert_eq!(
                native_borrow_bytes_callback(context, value, &mut pointer, &mut length),
                aelys_native::AELYS_NATIVE_INVALID_ARGUMENT
            );
        }
    }

    #[test]
    fn borrow_bytes_callback_shares_read_only_views_and_keeps_a_mutable_one_alone() {
        let mut vm = VM::new(Source::new("native-borrow-once", "")).unwrap();
        let vectors: Vec<Value> = (0..INLINE_BORROWS_PER_CALL + 3)
            .map(|_| byte_vec(&mut vm, &[0, 0]))
            .collect();
        let mut context = RuntimeNativeContext::new(&mut vm);
        let raw_context = (&mut context as *mut RuntimeNativeContext).cast();
        let mut pointer = std::ptr::null_mut();
        let mut shared = std::ptr::null();
        let mut length = 0;
        let mut exclusive = |value: Value| {
            native_borrow_bytes_callback(raw_context, raw(value), &mut pointer, &mut length)
        };
        let conflict = aelys_native::AELYS_NATIVE_BORROW_CONFLICT;

        assert_eq!(exclusive(vectors[0]), 0);
        assert_eq!(exclusive(vectors[0]), conflict);
        for &value in &vectors[1..INLINE_BORROWS_PER_CALL + 2] {
            assert_eq!(exclusive(value), 0);
        }
        assert_eq!(exclusive(vectors[INLINE_BORROWS_PER_CALL + 1]), conflict);
        let last = vectors[INLINE_BORROWS_PER_CALL + 2];
        let mut share = |value: Value| {
            native_borrow_bytes_shared_callback(raw_context, raw(value), &mut shared, &mut length)
        };
        assert_eq!(share(vectors[0]), conflict);
        assert_eq!(share(vectors[INLINE_BORROWS_PER_CALL + 1]), conflict);
        assert_eq!(share(last), 0);
        assert_eq!(share(last), 0);
        assert_eq!(
            native_borrow_bytes_callback(raw_context, raw(last), &mut pointer, &mut length),
            conflict
        );

        let mut next_call = RuntimeNativeContext::new(&mut vm);
        let raw_context = (&mut next_call as *mut RuntimeNativeContext).cast();
        assert_eq!(
            native_borrow_bytes_callback(raw_context, raw(vectors[0]), &mut pointer, &mut length),
            0
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
        }
        .call(&mut vm, &[])
        .unwrap();
        let option_payload =
            sum_payload(&vm, option_some, aelys_bytecode::object::SumTag::OptionSome);
        assert_eq!(string_value(&vm, option_payload), "option");

        let option_none = NativeFunctionImpl::Foreign {
            function: foreign_option_none,
            result: ForeignReturnKind::Option(ForeignPayloadKind::String),
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
        }
        .call(&mut vm, &[])
        .unwrap();
        let err_payload = sum_payload(&vm, result_err, aelys_bytecode::object::SumTag::ResultErr);
        assert_eq!(string_value(&vm, err_payload), "error");

        let option_unit = NativeFunctionImpl::Foreign {
            function: foreign_option_unit_some,
            result: ForeignReturnKind::Option(ForeignPayloadKind::Unit),
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
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
            bytes: ByteParams::UNTYPED,
        }
        .call(&mut vm, &[])
        .unwrap_err();
        assert!(matches!(
            null_option.kind,
            RuntimeErrorKind::NativeReturnedNull
        ));
    }
}
