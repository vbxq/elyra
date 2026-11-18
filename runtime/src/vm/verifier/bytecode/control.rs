use crate::vm::OpCode;

use super::{verify_jump, verify_reg, verify_reg_range};

#[allow(clippy::too_many_arguments)]
pub(super) fn verify(
    opcode: OpCode,
    ip: usize,
    a: usize,
    _b: usize,
    _c: usize,
    imm: i16,
    num_regs: usize,
    bytecode_len: usize,
    targets: &mut Vec<(usize, usize)>,
) -> Result<bool, String> {
    let mut jump = |name: &str| -> Result<(), String> {
        targets.push((ip, verify_jump(ip, imm, bytecode_len, name)?));
        Ok(())
    };
    match opcode {
        OpCode::Jump => {
            jump("Jump")?;
        }
        OpCode::JumpIf | OpCode::JumpIfNot => {
            verify_reg(a, num_regs, "JumpIf")?;
            jump("JumpIf")?;
        }
        OpCode::Return => {
            verify_reg(a, num_regs, "Return")?;
        }
        OpCode::Return0 => {}
        OpCode::ForLoopI | OpCode::ForLoopIInc => {
            // ForLoopI uses 3 consecutive registers: a (iter), a+1 (limit), a+2 (step)
            verify_reg_range(a, 3, num_regs, "ForLoopI")?;
            jump("ForLoopI")?;
        }
        OpCode::WhileLoopLt => {
            // WhileLoopLt uses 2 consecutive registers: a (value), a+1 (limit)
            verify_reg_range(a, 2, num_regs, "WhileLoopLt")?;
            jump("WhileLoopLt")?;
        }
        OpCode::StringForLoop => {
            // StringForLoop uses 3 consecutive registers: a (char), a+1 (byte_offset), a+2 (string_ptr)
            verify_reg_range(a, 3, num_regs, "StringForLoop")?;
            jump("StringForLoop")?;
        }
        OpCode::VecForLoop => {
            // VecForLoop uses 3 consecutive registers: a (element), a+1 (index), a+2 (vec_ptr)
            verify_reg_range(a, 3, num_regs, "VecForLoop")?;
            jump("VecForLoop")?;
        }
        OpCode::ArrayForLoop => {
            // ArrayForLoop uses 3 consecutive registers: a (element), a+1 (index), a+2 (array_ptr)
            verify_reg_range(a, 3, num_regs, "ArrayForLoop")?;
            jump("ArrayForLoop")?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}
