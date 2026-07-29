use crate::bytecode::decode_a;
use crate::bytecode::{OpCode, WideRegisterOperands};

pub(super) fn required_registers(bytecode: &[u32]) -> usize {
    let mut max_reg: usize = 0;
    let mut used = false;
    let mut ip = 0;

    while ip < bytecode.len() {
        let instr = bytecode[ip];
        let (op, a, b, c) = decode_a(instr);
        let imm_bits = u16::try_from(instr & 0xFFFF).expect("immediate occupies two bytes");
        let imm = i16::from_ne_bytes(imm_bits.to_ne_bytes());

        match op {
            OpCode::Move => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None)
            }
            OpCode::AddGlobalI => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None)
            }
            OpCode::LoadI
            | OpCode::LoadNull
            | OpCode::LoadBool
            | OpCode::LoadK
            | OpCode::LoadKWide
            | OpCode::GetGlobalIdxWide
            | OpCode::SetGlobalIdxWide => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Eq
            | OpCode::Ne
            | OpCode::Lt
            | OpCode::Le
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::AddII
            | OpCode::SubII
            | OpCode::MulII
            | OpCode::DivII
            | OpCode::ModII
            | OpCode::AddFF
            | OpCode::SubFF
            | OpCode::MulFF
            | OpCode::DivFF
            | OpCode::ModFF
            | OpCode::LtII
            | OpCode::LeII
            | OpCode::GtII
            | OpCode::GeII
            | OpCode::EqII
            | OpCode::NeII
            | OpCode::LtFF
            | OpCode::LeFF
            | OpCode::GtFF
            | OpCode::GeFF
            | OpCode::EqFF
            | OpCode::NeFF
            | OpCode::AddIIG
            | OpCode::SubIIG
            | OpCode::MulIIG
            | OpCode::DivIIG
            | OpCode::ModIIG
            | OpCode::AddFFG
            | OpCode::SubFFG
            | OpCode::MulFFG
            | OpCode::DivFFG
            | OpCode::ModFFG
            | OpCode::LtIIG
            | OpCode::LeIIG
            | OpCode::GtIIG
            | OpCode::GeIIG
            | OpCode::EqIIG
            | OpCode::NeIIG
            | OpCode::LtFFG
            | OpCode::LeFFG
            | OpCode::GtFFG
            | OpCode::GeFFG
            | OpCode::EqFFG
            | OpCode::NeFFG
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::ShlII
            | OpCode::ShrII
            | OpCode::AndII
            | OpCode::OrII
            | OpCode::XorII => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            OpCode::ShlIImm
            | OpCode::ShrIImm
            | OpCode::AndIImm
            | OpCode::OrIImm
            | OpCode::XorIImm => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            OpCode::Neg | OpCode::Not | OpCode::BitNot | OpCode::NotI => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            OpCode::Jump => {
                let _ = imm;
            }
            OpCode::JumpIf | OpCode::JumpIfNot => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::Call => {
                let nargs = c as usize;
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
                if nargs > 0 {
                    update_max_reg(&mut max_reg, &mut used, (b as usize) + nargs, None, None);
                }
            }
            OpCode::CallWide => {
                let Some(first) = bytecode.get(ip + 1) else {
                    break;
                };
                let Some(second) = bytecode.get(ip + 2) else {
                    break;
                };
                let dest = (first >> 16) as usize;
                let func = (first & 0xffff) as usize;
                let nargs = (second >> 16) as usize;
                update_max_reg(&mut max_reg, &mut used, dest, Some(func), None);
                if nargs > 0 {
                    update_max_reg(&mut max_reg, &mut used, func + nargs, None, None);
                }
            }
            OpCode::ArrayLitWide | OpCode::VecLitWide => {
                let Some(first) = bytecode.get(ip + 1) else {
                    break;
                };
                let Some(second) = bytecode.get(ip + 2) else {
                    break;
                };
                let dest = (first >> 16) as usize;
                let start = (first & 0xffff) as usize;
                let count = (second >> 16) as usize;
                update_max_reg(&mut max_reg, &mut used, dest, Some(start), None);
                if count > 0 {
                    update_max_reg(&mut max_reg, &mut used, start + count - 1, None, None);
                }
            }
            OpCode::JumpIfWideLong | OpCode::JumpIfNotWideLong => {
                let Some(register_word) = bytecode.get(ip + 1) else {
                    break;
                };
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    (register_word >> 16) as usize,
                    None,
                    None,
                );
            }
            OpCode::Return => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::Return0 => {}
            OpCode::GetGlobal | OpCode::SetGlobal => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::MakeClosure
            | OpCode::MakeClosureWide
            | OpCode::GetUpval
            | OpCode::CloseUpvals => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::SetUpval => {
                update_max_reg(&mut max_reg, &mut used, b as usize, None, None);
            }
            OpCode::MakeClosureRegisterWide => {
                if let Some(operands) = bytecode.get(ip + 1) {
                    update_max_reg(
                        &mut max_reg,
                        &mut used,
                        (operands >> 16) as usize,
                        None,
                        None,
                    );
                }
            }
            OpCode::LoopWideLong => {
                if let Some(operands) = bytecode.get(ip + 1) {
                    let register = (operands >> 16) as usize;
                    update_max_reg(
                        &mut max_reg,
                        &mut used,
                        register,
                        register.checked_add(1),
                        register.checked_add(2),
                    );
                }
            }
            OpCode::ForLoopI
            | OpCode::ForLoopIInc
            | OpCode::ForLoopILong
            | OpCode::ForLoopIIncLong => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some((a as usize) + 1),
                    Some((a as usize) + 2),
                );
            }
            OpCode::AddI | OpCode::SubI => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            OpCode::LtImm | OpCode::LeImm | OpCode::GtImm | OpCode::GeImm => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            OpCode::WhileLoopLt => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some((a as usize) + 1),
                    None,
                );
            }
            OpCode::LtIImm | OpCode::LeIImm | OpCode::GtIImm | OpCode::GeIImm => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            OpCode::GetGlobalIdx | OpCode::SetGlobalIdx => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::CallGlobal => {
                let nargs = c as usize;
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
                if nargs > 0 {
                    update_max_reg(&mut max_reg, &mut used, (a as usize) + nargs, None, None);
                }
            }
            OpCode::CallCached => {
                let nargs = c as usize;
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
                if nargs > 0 {
                    update_max_reg(&mut max_reg, &mut used, (a as usize) + nargs, None, None);
                }
            }
            OpCode::CallUpval | OpCode::TailCallUpval => {
                let nargs = c as usize;
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
                if nargs > 0 {
                    update_max_reg(&mut max_reg, &mut used, (a as usize) + nargs, None, None);
                }
            }

            // Array operations - dest, count
            OpCode::ArrayNewI | OpCode::ArrayNewF | OpCode::ArrayNewB | OpCode::ArrayNewP => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            // Array literal - dest, start, count (uses regs start..start+count)
            OpCode::ArrayLit | OpCode::VecLit => {
                let count = c as usize;
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
                if count > 0 {
                    update_max_reg(
                        &mut max_reg,
                        &mut used,
                        (b as usize) + count - 1,
                        None,
                        None,
                    );
                }
            }
            // Array load/get/store - all use 3 registers
            OpCode::ArrayLoadI
            | OpCode::ArrayLoadF
            | OpCode::ArrayLoadB
            | OpCode::ArrayLoadP
            | OpCode::ArrayGetI
            | OpCode::ArrayGetF
            | OpCode::ArrayGetB
            | OpCode::ArrayGetP
            | OpCode::ArrayStoreI
            | OpCode::ArrayStoreF
            | OpCode::ArrayStoreB
            | OpCode::ArrayStoreP => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            // Array/Vec length - dest, arr
            OpCode::ArrayLen | OpCode::VecLen | OpCode::VecCap => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }

            // Vec operations - dest, cap
            OpCode::VecNewI | OpCode::VecNewF | OpCode::VecNewB | OpCode::VecNewP => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            // Vec push - vec, val
            OpCode::VecPushI | OpCode::VecPushF | OpCode::VecPushB | OpCode::VecPushP => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            // Vec pop - dest, vec
            OpCode::VecPopI | OpCode::VecPopF | OpCode::VecPopB | OpCode::VecPopP => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            // Vec reserve - vec, cap
            OpCode::VecReserve => {
                update_max_reg(&mut max_reg, &mut used, a as usize, Some(b as usize), None);
            }
            // Vec load - dest, vec, idx (3 registers)
            OpCode::VecLoadI | OpCode::VecLoadF | OpCode::VecLoadB | OpCode::VecLoadP => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            // Vec get (safe) - dest, vec, idx (3 registers)
            OpCode::VecGetI | OpCode::VecGetF | OpCode::VecGetB | OpCode::VecGetP => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            // Vec store - vec, idx, val (3 registers)
            OpCode::VecStoreI | OpCode::VecStoreF | OpCode::VecStoreB | OpCode::VecStoreP => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            // String load char - dest, string, index (3 registers)
            OpCode::StringLoadChar => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize,
                    Some(b as usize),
                    Some(c as usize),
                );
            }
            // String for loop - uses consecutive regs [char_result(a), byte_offset(a+1), string_ptr(a+2)]
            OpCode::StringForLoop | OpCode::StringForLoopLong => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize + 2,
                    Some(a as usize + 1),
                    Some(a as usize),
                );
            }
            // Vec/Array for loop - uses consecutive regs [element(a), index(a+1), collection_ptr(a+2)]
            OpCode::VecForLoop
            | OpCode::ArrayForLoop
            | OpCode::VecForLoopLong
            | OpCode::ArrayForLoopLong => {
                update_max_reg(
                    &mut max_reg,
                    &mut used,
                    a as usize + 2,
                    Some(a as usize + 1),
                    Some(a as usize),
                );
            }
            OpCode::JumpLong => {}
            OpCode::JumpIfLong | OpCode::JumpIfNotLong => {
                update_max_reg(&mut max_reg, &mut used, a as usize, None, None);
            }
            OpCode::Wide => {
                let Some(first) = bytecode.get(ip + 1) else {
                    break;
                };
                let Some(second) = bytecode.get(ip + 2) else {
                    break;
                };
                let wide_a = (first >> 16) as usize;
                let wide_b = (first & 0xffff) as usize;
                let wide_c = (second >> 16) as usize;
                match OpCode::from_u8(a).and_then(OpCode::wide_register_operands) {
                    Some(WideRegisterOperands::A) => {
                        update_max_reg(&mut max_reg, &mut used, wide_a, None, None)
                    }
                    Some(WideRegisterOperands::A2) => {
                        update_max_reg(&mut max_reg, &mut used, wide_a, wide_a.checked_add(1), None)
                    }
                    Some(WideRegisterOperands::B) => {
                        update_max_reg(&mut max_reg, &mut used, wide_b, None, None)
                    }
                    Some(WideRegisterOperands::Ab) => {
                        update_max_reg(&mut max_reg, &mut used, wide_a, Some(wide_b), None)
                    }
                    Some(WideRegisterOperands::Abc) => {
                        update_max_reg(&mut max_reg, &mut used, wide_a, Some(wide_b), Some(wide_c))
                    }
                    None => {}
                }
            }
        }
        ip += 1 + op.extension_words();
    }

    if used { max_reg + 1 } else { 0 }
}

fn update_max_reg(
    max_reg: &mut usize,
    used: &mut bool,
    a: usize,
    b: Option<usize>,
    c: Option<usize>,
) {
    *used = true;
    *max_reg = (*max_reg).max(a);
    if let Some(b) = b {
        *max_reg = (*max_reg).max(b);
    }
    if let Some(c) = c {
        *max_reg = (*max_reg).max(c);
    }
}
