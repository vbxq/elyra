use super::SizedInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    And,
    Or,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntFault {
    Overflow,
    DivisionByZero,
}

pub fn checked_int_op(op: IntOp, left: i64, right: i64) -> Result<i64, IntFault> {
    match op {
        IntOp::Add => left.checked_add(right).ok_or(IntFault::Overflow),
        IntOp::Sub => left.checked_sub(right).ok_or(IntFault::Overflow),
        IntOp::Mul => left.checked_mul(right).ok_or(IntFault::Overflow),
        IntOp::Div => {
            if right == 0 {
                return Err(IntFault::DivisionByZero);
            }
            left.checked_div(right).ok_or(IntFault::Overflow)
        }
        IntOp::Rem => {
            if right == 0 {
                return Err(IntFault::DivisionByZero);
            }
            Ok(left.wrapping_rem(right))
        }
        IntOp::Shl => {
            let exact = i128::from(left) << (right & 63);
            i64::try_from(exact).map_err(|_| IntFault::Overflow)
        }
        IntOp::Shr => Ok(left >> (right & 63)),
        IntOp::And => Ok(left & right),
        IntOp::Or => Ok(left | right),
        IntOp::Xor => Ok(left ^ right),
    }
}

pub fn checked_int_neg(value: i64) -> Result<i64, IntFault> {
    value.checked_neg().ok_or(IntFault::Overflow)
}

// operands and result are bit patterns of the kind: a u64 above i64::MAX is negative here
pub fn wrapping_sized_op(op: IntOp, kind: SizedInt, left: i64, right: i64) -> Result<i64, IntFault> {
    let count = (right & i64::from(kind.bits() - 1)) as u32;
    let unsigned64 = matches!(kind, SizedInt::U64);
    let raw = match op {
        IntOp::Add => left.wrapping_add(right),
        IntOp::Sub => left.wrapping_sub(right),
        IntOp::Mul => left.wrapping_mul(right),
        IntOp::Shl => left.wrapping_shl(count),
        IntOp::Shr if unsigned64 => ((left as u64) >> count) as i64,
        IntOp::Shr => left >> count,
        IntOp::Div | IntOp::Rem if right == 0 => return Err(IntFault::DivisionByZero),
        IntOp::Div if unsigned64 => ((left as u64) / (right as u64)) as i64,
        IntOp::Rem if unsigned64 => ((left as u64) % (right as u64)) as i64,
        IntOp::Div => left.wrapping_div(right),
        IntOp::Rem => left.wrapping_rem(right),
        IntOp::And => left & right,
        IntOp::Or => left | right,
        IntOp::Xor => left ^ right,
    };
    Ok(kind.wrap(raw))
}

pub fn wrapping_sized_neg(kind: SizedInt, value: i64) -> i64 {
    kind.wrap(value.wrapping_neg())
}

pub fn wrapping_sized_not(kind: SizedInt, value: i64) -> i64 {
    kind.wrap(!value)
}
