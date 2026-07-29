use super::super::decode::decode_aimm;
use super::super::state::DispatchState;
use crate::vm::{GcRef, ObjectKind, VM};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

// Global variable operations: GetGlobal(24), SetGlobal(25)

#[inline(always)]
pub(crate) fn execute(
    vm: &mut VM,
    state: &DispatchState,
    ip: usize,
    opcode: u8,
    instruction: u32,
) -> Result<(), RuntimeError> {
    match opcode {
        // GetGlobal (24)
        24 => {
            let (destination, immediate) = decode_aimm(instruction);
            let index = usize::from(u16::from_ne_bytes(immediate.to_ne_bytes()));
            state.save_ip(vm, ip);
            let name = global_pointer(vm, state, ip, index, "GetGlobal")?;
            let value = match get_named_global(vm, name, "GetGlobal") {
                Ok(value) => value,
                Err(kind) => return Err(vm.runtime_error(kind)),
            };
            state.write_register(vm, state.base + usize::from(destination), value, ip)?;
        }
        // SetGlobal (25)
        25 => {
            let (source, immediate) = decode_aimm(instruction);
            let index = usize::from(u16::from_ne_bytes(immediate.to_ne_bytes()));
            let value = state.read_register(vm, state.base + usize::from(source), ip)?;
            state.save_ip(vm, ip);
            let name = global_pointer(vm, state, ip, index, "SetGlobal")?;
            if let Err(kind) = set_named_global(vm, name, value, "SetGlobal") {
                return Err(vm.runtime_error(kind));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[inline(always)]
fn global_pointer(
    vm: &mut VM,
    state: &DispatchState,
    ip: usize,
    index: usize,
    operation: &str,
) -> Result<GcRef, RuntimeError> {
    let Some(constant) = state.constant(index) else {
        state.save_ip(vm, ip);
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "invalid constant index: {} (max: {})",
            index, state.constants_len
        ))));
    };
    let Some(pointer) = constant.as_ptr() else {
        state.save_ip(vm, ip);
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant is not a string pointer"
        ))));
    };

    Ok(GcRef::new(pointer))
}

#[inline(always)]
fn get_named_global(
    vm: &VM,
    name: GcRef,
    operation: &str,
) -> Result<crate::vm::Value, RuntimeErrorKind> {
    let Some(object) = vm.heap.get(name) else {
        return Err(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant points to invalid heap object"
        )));
    };
    let ObjectKind::String(name) = &object.kind else {
        return Err(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant is not a string"
        )));
    };
    vm.globals
        .get(name.as_str())
        .copied()
        .ok_or_else(|| RuntimeErrorKind::UndefinedVariable(name.as_str().to_owned()))
}

#[inline(always)]
fn set_named_global(
    vm: &mut VM,
    name: GcRef,
    value: crate::vm::Value,
    operation: &str,
) -> Result<(), RuntimeErrorKind> {
    let (heap, globals, globals_by_index_cache) =
        (&vm.heap, &mut vm.globals, &mut vm.globals_by_index_cache);
    let Some(object) = heap.get(name) else {
        return Err(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant points to invalid heap object"
        )));
    };
    let ObjectKind::String(name) = &object.kind else {
        return Err(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant is not a string"
        )));
    };
    if let Some(slot) = globals.get_mut(name.as_str()) {
        *slot = value;
    } else {
        globals.insert(name.as_str().to_owned(), value);
    }
    globals_by_index_cache.clear();
    Ok(())
}
