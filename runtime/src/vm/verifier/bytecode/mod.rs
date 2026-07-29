use crate::vm::{Function, InstructionFormat, OpCode, WideRegisterOperands};

mod arithmetic;
mod arrays;
mod calls;
mod closures;
mod control;
mod globals;
mod memory;
mod registers;

use super::checks::{
    check_call_args, check_const_index, check_jump, check_reg, check_reg_range, check_upval_index,
};

pub(super) fn verify_bytecode(func: &Function) -> Result<(), String> {
    let num_regs = usize::try_from(func.num_registers)
        .map_err(|_| "register count does not fit this target".to_string())?;
    let constants_len = func.constants.len();
    let upvalues_len = func.upvalue_descriptors.len();
    let bytecode = &func.bytecode;

    // security: Validate function size limits to prevent integer truncation
    if bytecode.len() > u32::MAX as usize {
        return Err(format!(
            "bytecode length {} exceeds maximum {} (u32::MAX)",
            bytecode.len(),
            u32::MAX
        ));
    }
    if constants_len > u32::MAX as usize {
        return Err(format!(
            "constants length {} exceeds maximum {} (u32::MAX)",
            constants_len,
            u32::MAX
        ));
    }

    let mut ip = 0;
    while ip < bytecode.len() {
        let instr = bytecode[ip];
        let opcode_byte = u8::try_from(instr >> 24).expect("opcode occupies one byte");
        let opcode = OpCode::from_u8(opcode_byte)
            .ok_or_else(|| format!("invalid opcode {} at {}", opcode_byte, ip))?;
        let a = ((instr >> 16) & 0xFF) as usize;
        let b = ((instr >> 8) & 0xFF) as usize;
        let c = (instr & 0xFF) as usize;
        let imm_bits = u16::try_from(instr & 0xFFFF).expect("immediate occupies two bytes");
        let imm = i16::from_ne_bytes(imm_bits.to_ne_bytes());

        if opcode == OpCode::Wide {
            let first = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("wide instruction at {ip} is missing operand word 1"))?;
            let second = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("wide instruction at {ip} is missing operand word 2"))?;
            let inner = OpCode::from_u8(u8::try_from(a).expect("operand occupies one byte"))
                .ok_or_else(|| format!("wide instruction at {ip} has invalid inner opcode {a}"))?;
            let wide_a = (first >> 16) as usize;
            let wide_b = (first & 0xffff) as usize;
            let wide_c = (second >> 16) as usize;
            if !inner.supports_wide_registers() {
                return Err(format!(
                    "opcode {inner:?} does not have a verified wide-register form"
                ));
            }
            match inner.wide_register_operands() {
                Some(WideRegisterOperands::A) => verify_reg(wide_a, num_regs, "wide operand")?,
                Some(WideRegisterOperands::A2) => {
                    verify_reg_range(wide_a, 2, num_regs, "wide register pair")?;
                }
                Some(WideRegisterOperands::B) => verify_reg(wide_b, num_regs, "wide operand")?,
                Some(WideRegisterOperands::Ab) => {
                    verify_reg(wide_a, num_regs, "wide operand")?;
                    verify_reg(wide_b, num_regs, "wide operand")?;
                }
                Some(WideRegisterOperands::Abc) => {
                    verify_reg(wide_a, num_regs, "wide ternary")?;
                    verify_reg(wide_b, num_regs, "wide ternary")?;
                    verify_reg(wide_c, num_regs, "wide ternary")?;
                }
                None => unreachable!("wide support was checked above"),
            }
            if inner == OpCode::LoadK {
                let index = (wide_b << 16) | wide_c;
                verify_const(index, constants_len, "LoadKWideRegister")?;
            }
            if inner == OpCode::WhileLoopLt {
                let offset_bits = u16::try_from(wide_b).expect("wide immediate fits u16");
                let offset = i16::from_ne_bytes(offset_bits.to_ne_bytes());
                let adjusted_ip = ip
                    .checked_add(2)
                    .ok_or_else(|| "wide while ip overflow".to_string())?;
                super::checks::check_jump(adjusted_ip, offset, bytecode.len(), "wide while loop")?;
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::Abc16 {
            let first = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 1"))?;
            let second = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 2"))?;
            if instr & 0x00ff_ffff != 0 || second & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let wide_a = (first >> 16) as usize;
            let wide_b = (first & 0xffff) as usize;
            let wide_c = (second >> 16) as usize;
            match opcode {
                OpCode::CallWide => {
                    calls::verify(OpCode::Call, wide_a, wide_b, wide_c, num_regs, upvalues_len)?;
                }
                OpCode::ArrayLitWide => {
                    arrays::verify(OpCode::ArrayLit, wide_a, wide_b, wide_c, num_regs)?;
                }
                OpCode::VecLitWide => {
                    arrays::verify(OpCode::VecLit, wide_a, wide_b, wide_c, num_regs)?;
                }
                _ => return Err(format!("invalid 16-bit operand opcode {opcode:?}")),
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::RegisterOffset32 {
            let register_word = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its register word"))?;
            let offset_word = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its offset word"))?;
            if instr & 0x00ff_ffff != 0 || register_word & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let register = (register_word >> 16) as usize;
            verify_reg(register, num_regs, "wide conditional jump")?;
            let offset = i32::from_ne_bytes(offset_word.to_ne_bytes());
            let adjusted_ip = ip
                .checked_add(1)
                .ok_or_else(|| format!("{opcode:?} ip overflow"))?;
            super::checks::check_jump_i32(
                adjusted_ip,
                offset,
                bytecode.len(),
                "wide conditional jump",
            )?;
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::RegisterIndex32Aux {
            let operands = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its operand word"))?;
            let index = *bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its index word"))?
                as usize;
            if instr & 0x00ff_ffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let register = (operands >> 16) as usize;
            let upvalue_count = (operands & 0xffff) as usize;
            verify_reg(register, num_regs, "wide closure destination")?;
            verify_const(index, constants_len, "wide closure")?;
            let function_index = func.constants[index].as_nested_fn_marker().ok_or_else(|| {
                format!("wide closure constant {index} is not a nested function marker")
            })?;
            let nested = func.nested_functions.get(function_index).ok_or_else(|| {
                format!("wide closure nested function index {function_index} out of bounds")
            })?;
            if nested.upvalue_descriptors.len() != upvalue_count {
                return Err(format!(
                    "wide closure upvalue count {upvalue_count} does not match descriptors {}",
                    nested.upvalue_descriptors.len()
                ));
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::WideRegisterOffset32 {
            let register_word = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its register word"))?;
            let offset_word = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its offset word"))?;
            if instr & 0xffff != 0 || register_word & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let inner = OpCode::from_u8(u8::try_from(a).expect("opcode occupies one byte"))
                .ok_or_else(|| format!("{opcode:?} at {ip} has invalid inner opcode {a}"))?;
            if !matches!(
                inner,
                OpCode::ForLoopILong
                    | OpCode::ForLoopIIncLong
                    | OpCode::StringForLoopLong
                    | OpCode::VecForLoopLong
                    | OpCode::ArrayForLoopLong
            ) {
                return Err(format!("{opcode:?} at {ip} cannot wrap {inner:?}"));
            }
            let register = (register_word >> 16) as usize;
            verify_reg_range(register, 3, num_regs, "wide loop")?;
            let offset = i32::from_ne_bytes(offset_word.to_ne_bytes());
            let adjusted_ip = ip
                .checked_add(1)
                .ok_or_else(|| format!("{opcode:?} ip overflow"))?;
            super::checks::check_jump_i32(adjusted_ip, offset, bytecode.len(), "wide loop")?;
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::AOffset32 {
            let extension = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("long jump at {} is missing its extension word", ip))?;
            match opcode {
                OpCode::JumpLong => {}
                OpCode::JumpIfLong | OpCode::JumpIfNotLong => {
                    verify_reg(a, num_regs, "conditional long jump")?;
                }
                OpCode::ForLoopILong | OpCode::ForLoopIIncLong => {
                    verify_reg_range(a, 3, num_regs, "ForLoopILong")?;
                }
                OpCode::StringForLoopLong | OpCode::VecForLoopLong | OpCode::ArrayForLoopLong => {
                    verify_reg_range(a, 3, num_regs, "collection loop long")?;
                }
                _ => return Err(format!("invalid long-offset opcode {opcode:?}")),
            }
            let offset = i32::from_ne_bytes(extension.to_ne_bytes());
            super::checks::check_jump_i32(ip, offset, bytecode.len(), "long-offset instruction")?;
            ip += 2;
            continue;
        }

        if opcode.format() == InstructionFormat::AIndex32 {
            let extension = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("wide index at {} is missing its extension word", ip))?;
            let index = *extension as usize;
            verify_reg(a, num_regs, "wide-index instruction")?;
            if matches!(opcode, OpCode::LoadKWide | OpCode::MakeClosureWide) {
                verify_const(index, constants_len, "wide-index instruction")?;
            }
            if opcode == OpCode::MakeClosureWide {
                let constant = &func.constants[index];
                let function_index = constant.as_nested_fn_marker().ok_or_else(|| {
                    format!("MakeClosureWide constant {index} is not a nested function marker")
                })?;
                let nested = func.nested_functions.get(function_index).ok_or_else(|| {
                    format!("MakeClosureWide nested function index {function_index} out of bounds")
                })?;
                if nested.upvalue_descriptors.len() != b {
                    return Err(format!(
                        "MakeClosureWide upvalue count {b} does not match descriptors {}",
                        nested.upvalue_descriptors.len()
                    ));
                }
            }
            ip += 2;
            continue;
        }

        if registers::verify(opcode, a, b, c, imm, num_regs, constants_len)? {
            ip += 1;
            continue;
        }
        if arithmetic::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }
        if control::verify(opcode, ip, a, b, c, imm, num_regs, bytecode.len())? {
            ip += 1;
            continue;
        }
        if memory::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }
        if globals::verify(
            opcode,
            ip,
            a,
            b,
            c,
            imm,
            num_regs,
            constants_len,
            bytecode.len(),
        )? {
            ip += 1;
            continue;
        }
        if calls::verify(opcode, a, b, c, num_regs, upvalues_len)? {
            ip += 1;
            continue;
        }
        if closures::verify(func, opcode, a, b, c, num_regs, constants_len, upvalues_len)? {
            ip += 1;
            continue;
        }
        if arrays::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }

        return Err(format!("unhandled opcode {:?} at {}", opcode, ip));
    }

    Ok(())
}

pub(super) fn verify_call_args(
    base_reg: usize,
    nargs: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    check_reg(base_reg, num_regs, op)?;
    check_call_args(base_reg, nargs, num_regs, op)
}

pub(super) fn verify_const(idx: usize, constants_len: usize, op: &str) -> Result<(), String> {
    check_const_index(idx, constants_len, op)
}

pub(super) fn verify_jump(
    ip: usize,
    imm: i16,
    bytecode_len: usize,
    op: &str,
) -> Result<(), String> {
    check_jump(ip, imm, bytecode_len, op)
}

pub(super) fn verify_reg(reg: usize, num_regs: usize, op: &str) -> Result<(), String> {
    check_reg(reg, num_regs, op)
}

pub(super) fn verify_upval(idx: usize, upvalues_len: usize, op: &str) -> Result<(), String> {
    check_upval_index(idx, upvalues_len, op)
}

pub(super) fn verify_reg_range(
    base: usize,
    count: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    check_reg_range(base, count, num_regs, op)
}
