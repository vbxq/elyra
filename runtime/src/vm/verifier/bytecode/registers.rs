use crate::vm::OpCode;

use super::{verify_const, verify_reg};

pub(super) fn verify(
    opcode: OpCode,
    a: usize,
    b: usize,
    c: usize,
    imm: i16,
    num_regs: usize,
    constants_len: usize,
) -> Result<bool, String> {
    match opcode {
        OpCode::Move => {
            verify_reg(a, num_regs, "Move")?;
            verify_reg(b, num_regs, "Move")?;
        }
        OpCode::LoadI
        | OpCode::LoadNull
        | OpCode::LoadUnit
        | OpCode::LoadNone
        | OpCode::LoadBool => {
            verify_reg(a, num_regs, "Load")?;
        }
        OpCode::MakeSum | OpCode::SumTest | OpCode::SumPayload => {
            verify_reg(a, num_regs, "sum destination")?;
            verify_reg(b, num_regs, "sum source")?;
            let invalid_make_tag = opcode == OpCode::MakeSum
                && c > usize::from(aelys_bytecode::object::SumTag::ErrorMessage as u8);
            let invalid_test_tag = opcode == OpCode::SumTest
                && c > usize::from(aelys_bytecode::object::SumTag::ErrorMessage as u8)
                && c != 4;
            if invalid_make_tag || invalid_test_tag {
                return Err(format!("invalid sum tag {c}"));
            }
        }
        OpCode::MatchFail => {
            if a > 2 {
                return Err(format!("invalid match failure family {a}"));
            }
            if c > 1 {
                return Err(format!("invalid match failure flag {c}"));
            }
            if c == 1 {
                verify_reg(b, num_regs, "MatchFail message")?;
            }
        }
        OpCode::LoadK => {
            verify_reg(a, num_regs, "LoadK")?;
            let index = usize::from(u16::from_ne_bytes(imm.to_ne_bytes()));
            verify_const(index, constants_len, "LoadK")?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}
