use super::super::decode::decode_abc;
use crate::vm::{VM, Value};
use aelys_bytecode::IntOp;
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
    let result = if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
        Value::int_wrapping(left ^ right)
    } else {
        vm.frames[current_frame_idx].ip = ip;
        vm.int_binary(IntOp::Xor, left, right)?
    };
    // SAFETY: the destination register index is checked against the stable window above.
    unsafe {
        *registers.add(dest) = result;
    }
    Ok(())
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
    macro_rules! slow {
        ($op:expr, $left:expr, $right:expr) => {{
            vm.frames[current_frame_idx].ip = ip;
            vm.int_binary($op, $left, $right)?
        }};
    }


    let (a, b, c) = decode_abc(instr);
    let dest = base + a as usize;
    match opcode_byte {
        105 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = match (left.as_int(), right.as_int()) {
                (Some(l), Some(r)) => aelys_bytecode::checked_int_op(IntOp::Shl, l, r)
                    .ok()
                    .and_then(Value::inline_int),
                _ => None,
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shl, left, right),
            };
            reg_set!(dest, result);
        }

        106 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = match (left.as_int(), right.as_int()) {
                (Some(l), Some(r)) => Some(Value::int_wrapping(l >> (r & 63))),
                _ => None,
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shr, left, right),
            };
            reg_set!(dest, result);
        }

        107 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = match (left.as_int(), right.as_int()) {
                (Some(l), Some(r)) => Some(Value::int_wrapping(l & r)),
                _ => None,
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::And, left, right),
            };
            reg_set!(dest, result);
        }

        108 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = match (left.as_int(), right.as_int()) {
                (Some(l), Some(r)) => Some(Value::int_wrapping(l | r)),
                _ => None,
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Or, left, right),
            };
            reg_set!(dest, result);
        }

        110 => {
            let value = reg_get!(base + b as usize);
            let result = match value.as_int() {
                Some(n) => Value::int_wrapping(!n),
                None => {
                    vm.frames[current_frame_idx].ip = ip;
                    vm.int_not(value)?
                }
            };
            reg_set!(dest, result);
        }

        111 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = if left.both_inline_int_bits(&right) {
                let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                aelys_bytecode::checked_int_op(IntOp::Shl, l, r)
                    .ok()
                    .and_then(Value::inline_int)
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shl, left, right),
            };
            reg_set!(dest, result);
        }

        112 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = if left.both_inline_int_bits(&right) {
                let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                Some(Value::int_wrapping(l >> (r & 63)))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shr, left, right),
            };
            reg_set!(dest, result);
        }

        113 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = if left.both_inline_int_bits(&right) {
                let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                Some(Value::int_wrapping(l & r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::And, left, right),
            };
            reg_set!(dest, result);
        }

        114 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = if left.both_inline_int_bits(&right) {
                let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                Some(Value::int_wrapping(l | r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Or, left, right),
            };
            reg_set!(dest, result);
        }

        115 => {
            let left = reg_get!(base + b as usize);
            let right = reg_get!(base + c as usize);
            let fast = if left.both_inline_int_bits(&right) {
                let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                Some(Value::int_wrapping(l ^ r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Xor, left, right),
            };
            reg_set!(dest, result);
        }

        116 => {
            let value = reg_get!(base + b as usize);
            let result = if value.has_inline_int_bit() {
                Value::int_wrapping(!value.as_int_unchecked())
            } else {
                vm.frames[current_frame_idx].ip = ip;
                vm.int_not(value)?
            };
            reg_set!(dest, result);
        }

        117 => {
            let left = reg_get!(base + b as usize);
            let r = c as i64;
            let fast = if left.has_inline_int_bit() {
                let l = left.as_int_unchecked();
                aelys_bytecode::checked_int_op(IntOp::Shl, l, r)
                    .ok()
                    .and_then(Value::inline_int)
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shl, left, Value::int(r)),
            };
            reg_set!(dest, result);
        }

        118 => {
            let left = reg_get!(base + b as usize);
            let r = c as i64;
            let fast = if left.has_inline_int_bit() {
                let l = left.as_int_unchecked();
                Some(Value::int_wrapping(l >> (r & 63)))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Shr, left, Value::int(r)),
            };
            reg_set!(dest, result);
        }

        119 => {
            let left = reg_get!(base + b as usize);
            let r = c as i64;
            let fast = if left.has_inline_int_bit() {
                let l = left.as_int_unchecked();
                Some(Value::int_wrapping(l & r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::And, left, Value::int(r)),
            };
            reg_set!(dest, result);
        }

        120 => {
            let left = reg_get!(base + b as usize);
            let r = c as i64;
            let fast = if left.has_inline_int_bit() {
                let l = left.as_int_unchecked();
                Some(Value::int_wrapping(l | r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Or, left, Value::int(r)),
            };
            reg_set!(dest, result);
        }

        121 => {
            let left = reg_get!(base + b as usize);
            let r = c as i64;
            let fast = if left.has_inline_int_bit() {
                let l = left.as_int_unchecked();
                Some(Value::int_wrapping(l ^ r))
            } else {
                None
            };
            let result = match fast {
                Some(result) => result,
                None => slow!(IntOp::Xor, left, Value::int(r)),
            };
            reg_set!(dest, result);
        }

        _ => unreachable!(),
    }
    Ok(())
}
