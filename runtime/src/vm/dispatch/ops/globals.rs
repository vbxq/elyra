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
            let name = global_name(vm, state, ip, index, "GetGlobal")?;
            state.save_ip(vm, ip);
            let value = vm
                .get_global(&name)
                .ok_or_else(|| vm.runtime_error(RuntimeErrorKind::UndefinedVariable(name)))?;
            state.write_register(vm, state.base + usize::from(destination), value, ip)?;
        }
        // SetGlobal (25)
        25 => {
            let (source, immediate) = decode_aimm(instruction);
            let index = usize::from(u16::from_ne_bytes(immediate.to_ne_bytes()));
            let name = global_name(vm, state, ip, index, "SetGlobal")?;
            let value = state.read_register(vm, state.base + usize::from(source), ip)?;
            state.save_ip(vm, ip);
            vm.set_global(name, value);
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[inline(always)]
fn global_name(
    vm: &mut VM,
    state: &DispatchState,
    ip: usize,
    index: usize,
    operation: &str,
) -> Result<String, RuntimeError> {
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

    // Get the string from the heap
    let Some(object) = vm.heap.get(GcRef::new(pointer)) else {
        state.save_ip(vm, ip);
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant points to invalid heap object"
        ))));
    };
    let ObjectKind::String(string) = &object.kind else {
        state.save_ip(vm, ip);
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} constant is not a string"
        ))));
    };
    Ok(string.as_str().to_string())
}
