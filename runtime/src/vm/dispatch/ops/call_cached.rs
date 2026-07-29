use super::super::decode::decode_abc;
use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{CallFrame, GcRef, MAX_REGISTERS, NativeFunction, ObjectKind, VM, Value};
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
    Native(NativeFunction),
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
    global_mapping_id: usize,
    instruction: u32,
) -> Result<DispatchControl, RuntimeError> {
    let (destination, function_register, argument_count) = decode_abc(instruction);
    state.save_ip(vm, instruction_pointer);

    let callee_value = state.read_register(
        vm,
        state.base + usize::from(function_register),
        instruction_pointer,
    )?;
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
            ObjectKind::Native(native) => CallData::Native(native.clone()),
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

    if let CallData::Native(native) = call_data {
        validate_arity(vm, native.arity, argument_count)?;
        let mut arguments = Vec::with_capacity(usize::from(argument_count));
        for argument in 0..usize::from(argument_count) {
            arguments.push(state.read_register(
                vm,
                state.base + usize::from(destination) + 1 + argument,
                instruction_pointer,
            )?);
        }
        let result = vm.call_cached_native(&native, &arguments)?;
        state.write_register(
            vm,
            state.base + usize::from(destination),
            result,
            instruction_pointer,
        )?;
        return Ok(DispatchControl::Continue);
    }

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
        is_closure,
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
                false,
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
                true,
            )
        }
        CallData::Native(_) => unreachable!("native call returned before frame construction"),
        CallData::Invalid => {
            return Err(vm.runtime_error(RuntimeErrorKind::NotCallable(
                "non-callable object".to_string(),
            )));
        }
    };

    if callee_gmap != 0 && callee_gmap != global_mapping_id {
        if global_mapping_id != 0 {
            vm.sync_current_function_globals();
        }
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
    let mut frame = if is_closure {
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
    } else {
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
