use super::super::decode::decode_abc;
use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{CallFrame, GcRef, MAX_REGISTERS, ObjectKind, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

enum CallData {
    Function {
        arity: u16,
        callee_gmap: usize,
        num_registers: u32,
        bytecode: *const u32,
        bytecode_len: usize,
        constants: *const Value,
        constants_len: usize,
    },
    Closure {
        arity: u16,
        callee_gmap: usize,
        num_registers: u32,
        inner_function: GcRef,
        bytecode: *const u32,
        bytecode_len: usize,
        constants: *const Value,
        constants_len: usize,
        upvalues: *const GcRef,
        upvalues_len: usize,
    },
    Invalid,
}

#[inline]
pub(crate) fn execute(
    vm: &mut VM,
    state: &DispatchState,
    instruction_pointer: usize,
    upvalues: *const GcRef,
    upvalues_len: usize,
    global_mapping_id: usize,
    instruction: u32,
) -> Result<DispatchControl, RuntimeError> {
    let (destination, upvalue_index, argument_count) = decode_abc(instruction);
    state.save_ip(vm, instruction_pointer);

    let upvalue_index = usize::from(upvalue_index);
    if upvalue_index >= upvalues_len {
        return Err(vm.runtime_error(RuntimeErrorKind::UndefinedVariable("upvalue".to_string())));
    }

    let upvalue = unsafe { *upvalues.add(upvalue_index) };
    let callee_value = vm.get_upvalue_value(upvalue);
    let callee = callee_value.as_ptr().map(GcRef::new).ok_or_else(|| {
        vm.runtime_error(RuntimeErrorKind::NotCallable(
            vm.value_type_name(callee_value).to_string(),
        ))
    })?;

    let call_data = match vm.heap.get(callee) {
        Some(object) => match &object.kind {
            ObjectKind::Function(function) => CallData::Function {
                arity: function.arity(),
                callee_gmap: vm.global_mapping_id_for_layout(&function.function.global_layout),
                num_registers: function.num_registers(),
                bytecode: function.function.bytecode.as_ptr(),
                bytecode_len: function.function.bytecode.len(),
                constants: function.constants.as_ptr(),
                constants_len: function.constants.len(),
            },
            ObjectKind::Closure(closure) => {
                let callee_gmap = vm
                    .heap
                    .get(closure.function)
                    .and_then(|inner| match &inner.kind {
                        ObjectKind::Function(function) => {
                            Some(vm.global_mapping_id_for_layout(&function.function.global_layout))
                        }
                        _ => None,
                    })
                    .unwrap_or(0);
                CallData::Closure {
                    arity: closure.arity,
                    callee_gmap,
                    num_registers: closure.num_registers,
                    inner_function: closure.function,
                    bytecode: closure.bytecode_ptr,
                    bytecode_len: closure.bytecode_len,
                    constants: closure.constants_ptr,
                    constants_len: closure.constants_len,
                    upvalues: closure.upvalues.as_ptr(),
                    upvalues_len: closure.upvalues.len(),
                }
            }
            _ => CallData::Invalid,
        },
        None => {
            return Err(vm.runtime_error(RuntimeErrorKind::NotCallable(
                "invalid reference".to_string(),
            )));
        }
    };

    let (
        function,
        callee_gmap,
        num_registers,
        bytecode,
        bytecode_len,
        constants,
        constants_len,
        callee_upvalues,
        callee_upvalues_len,
    ) = match call_data {
        CallData::Function {
            arity,
            callee_gmap,
            num_registers,
            bytecode,
            bytecode_len,
            constants,
            constants_len,
        } => {
            validate_arity(vm, arity, argument_count)?;
            vm.ensure_function_verified(callee)?;
            (
                callee,
                callee_gmap,
                num_registers,
                bytecode,
                bytecode_len,
                constants,
                constants_len,
                std::ptr::null(),
                0,
            )
        }
        CallData::Closure {
            arity,
            callee_gmap,
            num_registers,
            inner_function,
            bytecode,
            bytecode_len,
            constants,
            constants_len,
            upvalues,
            upvalues_len,
        } => {
            validate_arity(vm, arity, argument_count)?;
            vm.ensure_function_verified(inner_function)?;
            (
                inner_function,
                callee_gmap,
                num_registers,
                bytecode,
                bytecode_len,
                constants,
                constants_len,
                upvalues,
                upvalues_len,
            )
        }
        CallData::Invalid => {
            return Err(vm.runtime_error(RuntimeErrorKind::NotCallable("non-callable".to_string())));
        }
    };

    if callee_gmap != global_mapping_id {
        vm.sync_current_function_globals();
        vm.prepare_globals_for_function(function);
    }

    let new_base = state
        .base
        .checked_add(usize::from(destination))
        .and_then(|base| base.checked_add(1))
        .ok_or_else(|| vm.runtime_error(RuntimeErrorKind::StackOverflow))?;
    let num_registers = usize::try_from(num_registers).map_err(|_| {
        vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
            "register count does not fit this target".to_string(),
        ))
    })?;
    let required_registers = new_base
        .checked_add(num_registers)
        .ok_or_else(|| vm.runtime_error(RuntimeErrorKind::StackOverflow))?;
    if required_registers > MAX_REGISTERS {
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg: required_registers.saturating_sub(1),
            max: MAX_REGISTERS.saturating_sub(1),
        }));
    }
    if required_registers > vm.registers.len() {
        vm.registers.resize(required_registers, Value::null());
    }

    let num_registers = u32::try_from(num_registers).expect("validated register count fits u32");
    let mut frame = if callee_upvalues_len == 0 {
        CallFrame::with_return_dest(
            function,
            new_base,
            destination,
            bytecode,
            bytecode_len,
            constants,
            constants_len,
            num_registers,
        )
    } else {
        CallFrame::with_upvalues(
            function,
            new_base,
            destination,
            bytecode,
            bytecode_len,
            constants,
            constants_len,
            callee_upvalues,
            callee_upvalues_len,
            num_registers,
        )
    };
    frame.global_mapping_id = callee_gmap;
    vm.push_frame(frame)?;

    Ok(DispatchControl::ReloadFrame)
}

#[inline(always)]
fn validate_arity(vm: &mut VM, expected: u16, actual: u8) -> Result<(), RuntimeError> {
    let actual = u16::from(actual);
    if expected != actual {
        return Err(vm.runtime_error(RuntimeErrorKind::ArityMismatch {
            expected,
            got: actual,
        }));
    }
    Ok(())
}
