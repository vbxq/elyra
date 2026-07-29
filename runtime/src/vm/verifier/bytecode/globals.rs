use crate::vm::OpCode;

use super::{verify_call_args, verify_const, verify_reg};

#[allow(clippy::too_many_arguments)]
pub(super) fn verify(
    opcode: OpCode,
    _ip: usize,
    a: usize,
    b: usize,
    c: usize,
    _imm: i16,
    num_regs: usize,
    constants_len: usize,
    _bytecode_len: usize,
) -> Result<bool, String> {
    match opcode {
        OpCode::GetGlobal | OpCode::SetGlobal => {
            verify_reg(a, num_regs, "Global")?;
            verify_const(b, constants_len, "Global")?;
        }
        OpCode::GetGlobalIdx | OpCode::SetGlobalIdx => {
            verify_reg(a, num_regs, "GlobalIdx")?;
        }
        OpCode::AddGlobalI => {
            verify_reg(a, num_regs, "global integer add")?;
            verify_reg(b, num_regs, "global integer add")?;
        }
        OpCode::CallGlobal => {
            verify_reg(a, num_regs, "CallGlobal")?;
            verify_call_args(a, c, num_regs, "CallGlobal")?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}
