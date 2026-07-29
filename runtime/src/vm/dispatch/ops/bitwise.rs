use super::super::decode::decode_abc;
use crate::vm::{VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn execute_xor(
    vm: &mut VM,
    ip: usize,
    base: usize,
    current_frame_idx: usize,
    registers: *mut Value,
    registers_len: usize,
    instr: u32,
) -> Result<(), RuntimeError> {
    let (dest, left, right) = decode_abc(instr);
    let dest = base + usize::from(dest);
    let left = base + usize::from(left);
    let right = base + usize::from(right);
    if dest >= registers_len || left >= registers_len || right >= registers_len {
        vm.frames[current_frame_idx].ip = ip;
        let reg = dest.max(left).max(right);
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg,
            max: registers_len,
        }));
    }
    // SAFETY: all three register indices are checked against the stable register window above.
    let (left, right) = unsafe { (*registers.add(left), *registers.add(right)) };
    if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
        // SAFETY: the destination register index is checked against the stable window above.
        unsafe {
            *registers.add(dest) = Value::int_wrapping(left ^ right);
        }
        Ok(())
    } else {
        vm.frames[current_frame_idx].ip = ip;
        Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation: "^",
            expected: "integer",
            got: format!(
                "{} and {}",
                vm.value_type_name(left),
                vm.value_type_name(right)
            ),
        }))
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn execute(
    vm: &mut VM,
    ip: usize,
    base: usize,
    current_frame_idx: usize,
    registers: *mut Value,
    registers_len: usize,
    opcode_byte: u8,
    instr: u32,
) -> Result<(), RuntimeError> {
    macro_rules! reg_get {
        ($index:expr) => {{
            let index = $index;
            if index >= registers_len {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
                    reg: index,
                    max: registers_len,
                }));
            }
            // SAFETY: bounds checked above
            unsafe { *registers.add(index) }
        }};
    }
    macro_rules! reg_ref {
        ($index:expr) => {
            reg_get!($index)
        };
    }
    macro_rules! reg_set {
        ($index:expr, $value:expr) => {{
            let index = $index;
            if index >= registers_len {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
                    reg: index,
                    max: registers_len,
                }));
            }
            // SAFETY: bounds checked above
            unsafe {
                *registers.add(index) = $value;
            }
        }};
    }

    // Bitwise operations: Shl(105), Shr(106), BitAnd(107), BitOr(108), BitXor(109),
    // BitNot(110), ShlII(111)-XorII(115), NotI(116), ShlIImm(117)-XorIImm(121)

    match opcode_byte {
        // Shl (105) - Generic left shift
        105 => {
            let (a, b, c) = decode_abc(instr);
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);

            if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
                reg_set!(base + a as usize, Value::int_wrapping(l << (r & 63)));
            } else {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "<<",
                    expected: "integer",
                    got: format!(
                        "{} and {}",
                        vm.value_type_name(left),
                        vm.value_type_name(right)
                    ),
                }));
            }
        }

        // Shr (106) - Generic arithmetic right shift
        106 => {
            let (a, b, c) = decode_abc(instr);
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);

            if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
                reg_set!(base + a as usize, Value::int_wrapping(l >> (r & 63)));
            } else {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: ">>",
                    expected: "integer",
                    got: format!(
                        "{} and {}",
                        vm.value_type_name(left),
                        vm.value_type_name(right)
                    ),
                }));
            }
        }

        // BitAnd (107) - Generic bitwise AND
        107 => {
            let (a, b, c) = decode_abc(instr);
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);

            if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                reg_set!(base + a as usize, Value::int_wrapping(l & r));
            } else {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "&",
                    expected: "integer",
                    got: format!(
                        "{} and {}",
                        vm.value_type_name(left),
                        vm.value_type_name(right)
                    ),
                }));
            }
        }

        // BitOr (108) - Generic bitwise OR
        108 => {
            let (a, b, c) = decode_abc(instr);
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);

            if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                reg_set!(base + a as usize, Value::int_wrapping(l | r));
            } else {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "|",
                    expected: "integer",
                    got: format!(
                        "{} and {}",
                        vm.value_type_name(left),
                        vm.value_type_name(right)
                    ),
                }));
            }
        }

        // BitXor (109) - Generic bitwise XOR
        109 => {
            execute_xor(
                vm,
                ip,
                base,
                current_frame_idx,
                registers,
                registers_len,
                instr,
            )?;
        }

        // BitNot (110) - Generic bitwise NOT
        110 => {
            let (a, b, _) = decode_abc(instr);
            let value = reg_get!(base + b as usize);

            if let Some(n) = value.as_int() {
                reg_set!(base + a as usize, Value::int_wrapping(!n));
            } else {
                vm.frames[current_frame_idx].ip = ip;
                return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "~",
                    expected: "integer",
                    got: vm.value_type_name(value).to_string(),
                }));
            }
        }

        // ShlII (111) - Type-specialized left shift
        111 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            let r = reg_ref!(base + c as usize).as_int_unchecked();
            // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
            reg_set!(base + a as usize, Value::int_wrapping(l << (r & 63)));
        }

        // ShrII (112) - Type-specialized right shift
        112 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            let r = reg_ref!(base + c as usize).as_int_unchecked();
            // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
            reg_set!(base + a as usize, Value::int_wrapping(l >> (r & 63)));
        }

        // AndII (113) - Type-specialized bitwise AND
        113 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            let r = reg_ref!(base + c as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l & r));
        }

        // OrII (114) - Type-specialized bitwise OR
        114 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            let r = reg_ref!(base + c as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l | r));
        }

        // XorII (115) - Type-specialized bitwise XOR
        115 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            let r = reg_ref!(base + c as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l ^ r));
        }

        // NotI (116) - Type-specialized bitwise NOT
        116 => {
            let (a, b, _) = decode_abc(instr);
            let n = reg_ref!(base + b as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(!n));
        }

        // ShlIImm (117) - Left shift with immediate
        117 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
            reg_set!(
                base + a as usize,
                Value::int_wrapping(l << ((c & 63) as i64))
            );
        }

        // ShrIImm (118) - Right shift with immediate
        118 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            // Mask shift amount to 0-63 range (branchless, prevents Rust UB)
            reg_set!(
                base + a as usize,
                Value::int_wrapping(l >> ((c & 63) as i64))
            );
        }

        // AndIImm (119) - Bitwise AND with immediate
        119 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l & (c as i64)));
        }

        // OrIImm (120) - Bitwise OR with immediate
        120 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l | (c as i64)));
        }

        // XorIImm (121) - Bitwise XOR with immediate
        121 => {
            let (a, b, c) = decode_abc(instr);
            let l = reg_ref!(base + b as usize).as_int_unchecked();
            reg_set!(base + a as usize, Value::int_wrapping(l ^ (c as i64)));
        }

        _ => unreachable!(),
    }
    Ok(())
}
