use crate::vm::OpCode;

use super::{verify_const, verify_reg};

pub(super) fn verify(
    opcode: OpCode,
    a: usize,
    b: usize,
    _c: usize,
    imm: i16,
    num_regs: usize,
    constants_len: usize,
) -> Result<bool, String> {
    match opcode {
        OpCode::Move => {
            verify_reg(a, num_regs, "Move")?;
            verify_reg(b, num_regs, "Move")?;
        }
        OpCode::LoadI | OpCode::LoadNull | OpCode::LoadBool => {
            verify_reg(a, num_regs, "Load")?;
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
