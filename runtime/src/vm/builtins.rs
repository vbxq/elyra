use super::VM;
use super::Value;
use aelys_common::error::RuntimeError;

// core builtins only - everything else is stdlib
pub fn register_builtins(vm: &mut VM) -> Result<(), RuntimeError> {
    let type_fn = vm.alloc_native("type", 1, builtin_type)?;
    vm.set_global("type".to_string(), Value::ptr(type_fn.index()));

    let tostring_fn = vm.alloc_native("__tostring", 1, builtin_tostring)?;
    vm.set_global("__tostring".to_string(), Value::ptr(tostring_fn.index()));

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
