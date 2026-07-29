use super::super::decode::decode_abc;
use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{AelysClosure, GcObject, GcRef, ObjectKind, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn execute(
    vm: &mut VM,
    state: &DispatchState,
    instruction_pointer: &mut usize,
    func_ref: GcRef,
    bytecode_ptr: *const u32,
    upvalues_ptr: *const GcRef,
    upvalues_len: usize,
    opcode_byte: u8,
    instr: u32,
) -> Result<DispatchControl, RuntimeError> {
    let mut ip = *instruction_pointer;
    let base = state.base;
    let constants_ptr = state.constants;
    let constants_len = state.constants_len;
    let current_frame_idx = state.frame_index;

    macro_rules! reg_get {
        ($index:expr) => {
            state.read_register(vm, $index, ip)?
        };
    }
    macro_rules! reg_set {
        ($index:expr, $value:expr) => {
            state.write_register(vm, $index, $value, ip)?
        };
    }

    // Closure operations: MakeClosure(35), GetUpval(36),
    // SetUpval(37), CloseUpvals(38)

    match opcode_byte {
        // MakeClosure (35)
        35 | 126 | 181 => {
            let (narrow_dest, narrow_index, aux) = decode_abc(instr);
            let (dest, const_idx, num_upvalues) = if opcode_byte == 126 {
                let operands = unsafe { *bytecode_ptr.add(ip) };
                let index = unsafe { *bytecode_ptr.add(ip + 1) } as usize;
                ip += 2;
                (
                    usize::try_from(operands >> 16).expect("wide destination fits usize"),
                    index,
                    usize::try_from(operands & 0xffff).expect("upvalue count fits usize"),
                )
            } else if opcode_byte == 181 {
                let index = unsafe { *bytecode_ptr.add(ip) } as usize;
                ip += 1;
                (usize::from(narrow_dest), index, usize::from(aux))
            } else {
                (
                    usize::from(narrow_dest),
                    usize::from(narrow_index),
                    usize::from(aux),
                )
            };

            // Get the function value from constants (k{const_idx})
            if const_idx >= constants_len {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                    "invalid constant index: {} (max: {})",
                    const_idx, constants_len
                ))));
            }
            let constant = unsafe { *constants_ptr.add(const_idx) };

            // Check if this is a nested function marker (uses dedicated tag, can't collide with heap ptrs)
            let (nested_func_ref, upvalue_descriptors) =
                if let Some(nested_idx) = constant.as_nested_fn_marker() {
                    // Nested function marker: resolve from nested_functions array
                    vm.frames[current_frame_idx].ip = ip;
                    let nested_func = vm.get_nested_function(func_ref, nested_idx)?;
                    let upvalue_descs = nested_func.upvalue_descriptors.clone();
                    vm.verify_function_value(&nested_func)?;
                    let nested_ref = vm.alloc_function(nested_func)?;
                    (nested_ref, upvalue_descs)
                } else if let Some(ptr_val) = constant.as_ptr() {
                    // Direct heap pointer - get function object
                    let func_ref = GcRef::new(ptr_val);
                    match vm.heap.get(func_ref) {
                        Some(obj) => {
                            if let ObjectKind::Function(f) = &obj.kind {
                                (func_ref, f.function.upvalue_descriptors.clone())
                            } else {
                                vm.frames[current_frame_idx].ip = ip;
                                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                                    operation: "MakeClosure",
                                    expected: "function",
                                    got: "non-function object".to_string(),
                                }));
                            }
                        }
                        None => {
                            vm.frames[current_frame_idx].ip = ip;
                            return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                                "MakeClosure function not found in heap".to_string(),
                            )));
                        }
                    }
                } else {
                    vm.frames[current_frame_idx].ip = ip;
                    return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "MakeClosure",
                        expected: "function pointer",
                        got: vm.value_type_name(constant).to_string(),
                    }));
                };
            reg_set!(base + dest, Value::ptr(nested_func_ref.index()));

            // Collect upvalue refs using the descriptors
            let mut upvalue_refs = Vec::with_capacity(num_upvalues);
            for desc in upvalue_descriptors.iter().take(num_upvalues) {
                if desc.is_local {
                    // Create or reuse upvalue for local variable
                    let upval_ref = vm.capture_upvalue(base, desc.index)?;
                    upvalue_refs.push(upval_ref);
                } else {
                    // Copy upvalue from enclosing function
                    if (desc.index as usize) >= upvalues_len {
                        vm.frames[current_frame_idx].ip = ip;
                        return Err(vm.runtime_error(RuntimeErrorKind::UndefinedVariable(
                            format!("upvalue index {} out of bounds", desc.index),
                        )));
                    }
                    let parent_upval = unsafe { *upvalues_ptr.add(desc.index as usize) };
                    upvalue_refs.push(parent_upval);
                }
            }

            // Get function metadata for the closure
            let (bc_ptr, bc_len, const_ptr, const_len, arity, num_regs) =
                match vm.heap.get(nested_func_ref) {
                    Some(obj) => {
                        if let ObjectKind::Function(f) = &obj.kind {
                            (
                                f.function.bytecode.as_ptr(),
                                f.function.bytecode.len(),
                                f.constants.as_ptr(),
                                f.constants.len(),
                                f.function.arity,
                                f.function.num_registers,
                            )
                        } else {
                            (std::ptr::null(), 0, std::ptr::null(), 0, 0, 0)
                        }
                    }
                    None => (std::ptr::null(), 0, std::ptr::null(), 0, 0, 0),
                };

            // Allocate the closure with proper metadata
            vm.frames[current_frame_idx].ip = ip;
            let closure = AelysClosure::with_cache(
                nested_func_ref,
                upvalue_refs,
                aelys_bytecode::object::ClosureCache {
                    bytecode_ptr: bc_ptr,
                    bytecode_len: bc_len,
                    constants_ptr: const_ptr,
                    constants_len: const_len,
                    arity,
                    num_registers: num_regs,
                },
            );
            match vm.alloc_object(GcObject::new(ObjectKind::Closure(closure))) {
                Ok(closure_ref) => {
                    reg_set!(base + dest, Value::ptr(closure_ref.index()));
                }
                Err(e) => return Err(e),
            }
        }

        // GetUpval (36)
        36 => {
            let (a, b, _) = decode_abc(instr);
            let upval_idx = b as usize;

            if upval_idx >= upvalues_len {
                vm.frames[current_frame_idx].ip = ip;
                return Err(
                    vm.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                        "upvalue index {} out of bounds",
                        upval_idx
                    ))),
                );
            }

            let upval_ref = unsafe { *upvalues_ptr.add(upval_idx) };
            let value = vm.get_upvalue_value(upval_ref);
            reg_set!(base + a as usize, value);
        }

        // SetUpval (37)
        37 => {
            let (a, b, _) = decode_abc(instr);
            let upval_idx = a as usize;
            let value = reg_get!(base + b as usize);

            if upval_idx >= upvalues_len {
                vm.frames[current_frame_idx].ip = ip;
                return Err(
                    vm.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                        "upvalue index {} out of bounds",
                        upval_idx
                    ))),
                );
            }

            let upval_ref = unsafe { *upvalues_ptr.add(upval_idx) };
            vm.set_upvalue_value(upval_ref, value);
        }

        // CloseUpvals (38)
        38 => {
            let (a, _, _) = decode_abc(instr);
            let from_slot = base + a as usize;
            vm.close_upvalues_from(from_slot);
        }

        _ => unreachable!(),
    }
    *instruction_pointer = ip;
    Ok(DispatchControl::Continue)
}
