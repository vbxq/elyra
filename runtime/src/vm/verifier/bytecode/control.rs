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
            verify_reg_range(a, 3, num_regs, "ForLoopI")?;
            jump("ForLoopI")?;
        }
        OpCode::WhileLoopLt => {
            verify_reg_range(a, 2, num_regs, "WhileLoopLt")?;
            jump("WhileLoopLt")?;
        }
        OpCode::StringForLoop => {
            verify_reg_range(a, 3, num_regs, "StringForLoop")?;
            jump("StringForLoop")?;
        }
        OpCode::VecForLoop => {
            verify_reg_range(a, 3, num_regs, "VecForLoop")?;
            jump("VecForLoop")?;
        }
        OpCode::ArrayForLoop => {
            verify_reg_range(a, 3, num_regs, "ArrayForLoop")?;
            jump("ArrayForLoop")?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}
