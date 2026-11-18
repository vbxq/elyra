use super::super::decode::decode_abc;
use crate::vm::{VM, Value};
use aelys_bytecode::{FixedKind, IntOp, OpCode, SizedInt};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

// only 202 to 228, the ops that cannot fail: any other byte is read as ShrW32
#[inline(always)]
pub(crate) fn wrap_unchecked(opcode_byte: u8, left: i64, right: i64) -> i64 {
    match opcode_byte {
        202 => left.wrapping_add(right) as i8 as i64,
        203 => left.wrapping_add(right) as i16 as i64,
        204 => left.wrapping_add(right) as i32 as i64,
        205 => left.wrapping_add(right) as u8 as i64,
        206 => left.wrapping_add(right) as u16 as i64,
        207 => left.wrapping_add(right) as u32 as i64,
        208 => left.wrapping_sub(right) as i8 as i64,
        209 => left.wrapping_sub(right) as i16 as i64,
        210 => left.wrapping_sub(right) as i32 as i64,
        211 => left.wrapping_sub(right) as u8 as i64,
        212 => left.wrapping_sub(right) as u16 as i64,
        213 => left.wrapping_sub(right) as u32 as i64,
        214 => left.wrapping_mul(right) as i8 as i64,
        215 => left.wrapping_mul(right) as i16 as i64,
        216 => left.wrapping_mul(right) as i32 as i64,
        217 => left.wrapping_mul(right) as u8 as i64,
        218 => left.wrapping_mul(right) as u16 as i64,
        219 => left.wrapping_mul(right) as u32 as i64,
        220 => (left << (right & 7)) as i8 as i64,
        221 => (left << (right & 15)) as i16 as i64,
        222 => (left << (right & 31)) as i32 as i64,
        223 => (left << (right & 7)) as u8 as i64,
        224 => (left << (right & 15)) as u16 as i64,
        225 => (left << (right & 31)) as u32 as i64,
        226 => left >> (right & 7),
        227 => left >> (right & 15),
        _ => left >> (right & 31),
    }
}

#[inline(always)]
pub(crate) fn wrap_binary(opcode_byte: u8, left: i64, right: i64) -> Option<i64> {
    match opcode_byte {
        229..=231 => {
            if right == 0 {
                return None;
            }
            let quotient = left.wrapping_div(right);
            Some(match opcode_byte {
                229 => quotient as i8 as i64,
                230 => quotient as i16 as i64,
                _ => quotient as i32 as i64,
            })
        }
        _ => Some(wrap_unchecked(opcode_byte, left, right)),
    }
}

pub(crate) fn sized_binary_value(opcode: OpCode, left: i64, right: i64) -> Option<i64> {
    wrap_binary(opcode as u8, left, right)
}

pub(crate) fn sized_unary_value(opcode: OpCode, kind: SizedInt, value: i64) -> i64 {
    match opcode {
        OpCode::NegW => kind.wrap(value.wrapping_neg()),
        _ => kind.wrap(!value),
    }
}

pub(crate) fn wide_parts(opcode_byte: u8) -> (IntOp, SizedInt) {
    let kind = if (opcode_byte - OpCode::AddWI64 as u8) % 2 == 0 && opcode_byte != OpCode::ModWU64 as u8 {
        SizedInt::I64
    } else {
        SizedInt::U64
    };
    let op = match (opcode_byte - OpCode::AddWI64 as u8) / 2 {
        0 => IntOp::Add,
        1 => IntOp::Sub,
        2 => IntOp::Mul,
        3 => IntOp::Shl,
        4 => IntOp::Shr,
        5 => IntOp::Div,
        _ => IntOp::Rem,
    };
    (op, kind)
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
    let (dest, left, right) = decode_abc(instr);
    let dest = base + usize::from(dest);
    let left = base + usize::from(left);
    if opcode_byte >= OpCode::NegW as u8 {
        if dest >= registers_len || left >= registers_len {
            vm.frames[current_frame_idx].ip = ip;
            return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: dest.max(left),
                max: registers_len,
            }));
        }
        let Some(kind) = SizedInt::from_u8(right) else {
            vm.frames[current_frame_idx].ip = ip;
            return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                "invalid integer width {right}"
            ))));
        };
        let negate = opcode_byte == OpCode::NegW as u8;
        // safety: bounds checked above
        let operand = unsafe { *registers.add(left) };
        if kind.bits() == 64 {
            vm.frames[current_frame_idx].ip = ip;
            let result = vm.sized_unary(negate, kind, operand)?;
            unsafe {
                *registers.add(dest) = result;
            }
            return Ok(());
        }
        let opcode = if negate { OpCode::NegW } else { OpCode::NotW };
        unsafe {
            *registers.add(dest) =
                Value::int_wrapping(sized_unary_value(opcode, kind, operand.as_int_unchecked()));
        }
        return Ok(());
    }
    let right = base + usize::from(right);
    if dest >= registers_len || left >= registers_len || right >= registers_len {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg: dest.max(left).max(right),
            max: registers_len,
        }));
    }
    // safety: bounds checked above
    let (l, r) = unsafe {
        (
            (*registers.add(left)).as_int_unchecked(),
            (*registers.add(right)).as_int_unchecked(),
        )
    };
    let Some(result) = wrap_binary(opcode_byte, l, r) else {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::DivisionByZero));
    };
    unsafe {
        *registers.add(dest) = Value::int_wrapping(result);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_wide64(
    vm: &mut VM,
    ip: usize,
    base: usize,
    current_frame_idx: usize,
    registers: *mut Value,
    registers_len: usize,
    opcode_byte: u8,
    instr: u32,
) -> Result<(), RuntimeError> {
    let (dest, left, right) = decode_abc(instr);
    let (dest, left, right) = (
        base + usize::from(dest),
        base + usize::from(left),
        base + usize::from(right),
    );
    if dest >= registers_len || left >= registers_len || right >= registers_len {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg: dest.max(left).max(right),
            max: registers_len,
        }));
    }
    // safety: bounds checked above
    let (l, r) = unsafe { (*registers.add(left), *registers.add(right)) };
    let (op, kind) = wide_parts(opcode_byte);
    vm.frames[current_frame_idx].ip = ip;
    let result = vm.sized_binary(op, kind, l, r)?;
    unsafe {
        *registers.add(dest) = result;
    }
    Ok(())
}

// none is a division by zero, some(none) a result outside the int range
pub(crate) fn fixed_value(
    opcode: OpCode,
    kind: FixedKind,
    left: i64,
    right: i64,
    shift: i64,
) -> Option<Option<i64>> {
    let shift = (shift & 63) as u32;
    if matches!(kind, FixedKind::Sized(SizedInt::U64)) {
        let (left, right) = (u128::from(left as u64), u128::from(right as u64));
        let exact = match opcode {
            OpCode::MulShr => (left * right) >> shift,
            _ => {
                if right == 0 {
                    return None;
                }
                (left << shift) / right
            }
        };
        return Some(Some(exact as u64 as i64));
    }
    let exact = match opcode {
        OpCode::MulShr => (i128::from(left) * i128::from(right)) >> shift,
        _ => {
            if right == 0 {
                return None;
            }
            (i128::from(left) << shift) / i128::from(right)
        }
    };
    Some(match kind {
        FixedKind::Sized(kind) => Some(kind.wrap(exact as i64)),
        FixedKind::Int => i64::try_from(exact).ok(),
    })
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn execute_fixed(
    vm: &mut VM,
    ip: usize,
    base: usize,
    current_frame_idx: usize,
    registers: *mut Value,
    registers_len: usize,
    opcode_byte: u8,
    instr: u32,
    first: u32,
    second: u32,
) -> Result<(), RuntimeError> {
    let (kind, mode, immediate) = decode_abc(instr);
    let dest = base + (first >> 16) as usize;
    let left = base + (first & 0xffff) as usize;
    let right = base + (second >> 16) as usize;
    let shift_register = base + (second & 0xffff) as usize;
    if dest >= registers_len
        || left >= registers_len
        || right >= registers_len
        || (mode != 0 && shift_register >= registers_len)
    {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg: dest.max(left).max(right).max(shift_register),
            max: registers_len,
        }));
    }
    let Some(kind) = FixedKind::from_u8(kind) else {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "invalid fixed-point width {kind}"
        ))));
    };
    // safety: bounds checked above
    let (l, r, shift) = unsafe {
        (
            *registers.add(left),
            *registers.add(right),
            if mode == 0 {
                Value::int(i64::from(immediate))
            } else {
                *registers.add(shift_register)
            },
        )
    };
    let operands = (vm.int_bits(l), vm.int_bits(r), vm.int_bits(shift));
    let (Some(l), Some(r), Some(shift)) = operands else {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation: "fixed-point method",
            expected: "integer",
            got: vm.value_type_name(l).to_string(),
        }));
    };
    let opcode = if opcode_byte == OpCode::MulShr as u8 {
        OpCode::MulShr
    } else {
        OpCode::ShlDiv
    };
    vm.frames[current_frame_idx].ip = ip;
    match fixed_value(opcode, kind, l, r, shift) {
        Some(Some(result)) => {
            let value = match kind {
                FixedKind::Sized(kind) => vm.make_sized(kind, result)?,
                FixedKind::Int => vm.make_int(result)?,
            };
            unsafe {
                *registers.add(dest) = value;
            }
            Ok(())
        }
        Some(None) => Err(vm.runtime_error(RuntimeErrorKind::IntegerOverflow)),
        None => Err(vm.runtime_error(RuntimeErrorKind::DivisionByZero)),
    }
}
