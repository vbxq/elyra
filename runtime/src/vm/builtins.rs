use super::VM;
use super::Value;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

pub fn register_builtins(vm: &mut VM) -> Result<(), RuntimeError> {
    let tostring_fn = vm.alloc_native("__tostring", 1, builtin_tostring)?;
    vm.set_global("__tostring".to_string(), Value::ptr(tostring_fn.index()));
    let panic_fn = vm.alloc_native("panic", 1, builtin_panic)?;
    vm.set_global("panic".to_string(), Value::ptr(panic_fn.index()));
    let assert_fn = vm.alloc_native("assert", 2, builtin_assert)?;
    vm.set_global("assert".to_string(), Value::ptr(assert_fn.index()));

    Ok(())
}

pub fn builtin_type(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let value = args[0];
    let type_name = vm.value_type_name(value);
    let str_ref = vm.alloc_string(type_name)?;
    Ok(Value::ptr(str_ref.index()))
}

pub fn builtin_tostring(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let s = vm.value_to_string(args[0]);
    let str_ref = vm.alloc_string(&s)?;
    Ok(Value::ptr(str_ref.index()))
}

pub fn builtin_panic(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let message = vm.value_to_string(args[0]);
    Err(vm.runtime_error(RuntimeErrorKind::Panic { message }))
}

pub fn builtin_assert(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    if args[0].as_bool() == Some(true) {
        return Ok(Value::unit());
    }
    let message = format!("assertion failed: {}", vm.value_to_string(args[1]));
    Err(vm.runtime_error(RuntimeErrorKind::Panic { message }))
}
