use super::ir::{
    BlockId, FunctionIr, IrBlock, IrInstruction, IrInstructionKind, IrTerminator, IrType,
    SourcePosition, ValueId,
};
use aelys_bytecode::{Function, OpCode};

pub(crate) fn translate_integer_function(function: &Function) -> Option<FunctionIr> {
    let register_count = usize::try_from(function.num_registers).ok()?;
    let mut registers = vec![None; register_count];
    let mut parameters = Vec::with_capacity(usize::from(function.arity));
    let mut next_value = 0u32;
    for register in 0..usize::from(function.arity) {
        let value = next_id(&mut next_value)?;
        registers.get_mut(register)?.replace(value);
        parameters.push((value, IrType::I64));
    }

    let mut instructions = Vec::new();
    let mut terminator = None;
    for (ip, &word) in function.bytecode.as_slice().iter().enumerate() {
        let opcode = u8::try_from(word >> 24).ok().and_then(OpCode::from_u8)?;
        let a = usize::from(u8::try_from((word >> 16) & 0xff).ok()?);
        let b = usize::from(u8::try_from((word >> 8) & 0xff).ok()?);
        let c = usize::from(u8::try_from(word & 0xff).ok()?);
        let position = SourcePosition {
            bytecode_ip: u32::try_from(ip).ok()?,
            source_line: source_line(function, ip),
        };
        match opcode {
            OpCode::Move => {
                let value = *registers.get(b)?.as_ref()?;
                *registers.get_mut(a)? = Some(value);
            }
            OpCode::LoadI => {
                let immediate = u16::try_from(word & 0xffff).ok()?;
                let immediate = i16::from_ne_bytes(immediate.to_ne_bytes());
                emit_value(
                    &mut registers,
                    &mut instructions,
                    &mut next_value,
                    a,
                    IrType::I64,
                    IrInstructionKind::Iconst(i64::from(immediate)),
                    position,
                )?;
            }
            OpCode::AddI | OpCode::SubI => {
                let left = *registers.get(b)?.as_ref()?;
                let immediate = next_id(&mut next_value)?;
                instructions.push(IrInstruction {
                    result: Some((immediate, IrType::I64)),
                    kind: IrInstructionKind::Iconst(i64::try_from(c).ok()?),
                    source: position,
                });
                let kind = if opcode == OpCode::AddI {
                    IrInstructionKind::Iadd(left, immediate)
                } else {
                    IrInstructionKind::Isub(left, immediate)
                };
                emit_value(
                    &mut registers,
                    &mut instructions,
                    &mut next_value,
                    a,
                    IrType::I64,
                    kind,
                    position,
                )?;
            }
            OpCode::AddII | OpCode::SubII | OpCode::MulII => {
                let left = *registers.get(b)?.as_ref()?;
                let right = *registers.get(c)?.as_ref()?;
                let kind = match opcode {
                    OpCode::AddII => IrInstructionKind::Iadd(left, right),
                    OpCode::SubII => IrInstructionKind::Isub(left, right),
                    OpCode::MulII => IrInstructionKind::Imul(left, right),
                    _ => return None,
                };
                emit_value(
                    &mut registers,
                    &mut instructions,
                    &mut next_value,
                    a,
                    IrType::I64,
                    kind,
                    position,
                )?;
            }
            OpCode::Return => {
                terminator = Some(IrTerminator::Return(*registers.get(a)?.as_ref()?));
                break;
            }
            _ => return None,
        }
    }

    Some(FunctionIr {
        name: function
            .name
            .clone()
            .unwrap_or_else(|| "anonymous".to_string()),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64; usize::from(function.arity)],
        return_type: IrType::I64,
        blocks: vec![IrBlock {
            id: BlockId(0),
            parameters,
            instructions,
            terminator: terminator?,
        }],
        deopt_maps: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn emit_value(
    registers: &mut [Option<ValueId>],
    instructions: &mut Vec<IrInstruction>,
    next_value: &mut u32,
    destination: usize,
    ty: IrType,
    kind: IrInstructionKind,
    source: SourcePosition,
) -> Option<()> {
    let value = next_id(next_value)?;
    instructions.push(IrInstruction {
        result: Some((value, ty)),
        kind,
        source,
    });
    *registers.get_mut(destination)? = Some(value);
    Some(())
}

fn next_id(next: &mut u32) -> Option<ValueId> {
    let value = ValueId(*next);
    *next = next.checked_add(1)?;
    Some(value)
}

fn source_line(function: &Function, ip: usize) -> u32 {
    function
        .lines
        .iter()
        .rev()
        .find(|(offset, _)| usize::from(*offset) <= ip)
        .map(|(_, line)| *line)
        .unwrap_or(0)
}
