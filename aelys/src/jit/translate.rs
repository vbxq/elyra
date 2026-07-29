use super::ir::{
    BlockId, DeoptMap, FunctionIr, IntPredicate, IrBlock, IrInstruction, IrInstructionKind,
    IrTerminator, IrType, SourcePosition, ValueId,
};
use aelys_bytecode::{Function, OpCode};
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Clone, Copy)]
struct DecodedInstruction {
    ip: usize,
    next: usize,
    opcode: OpCode,
    a: usize,
    b: usize,
    c: usize,
    target: Option<usize>,
}

pub(crate) fn translate_integer_function(function: &Function) -> Option<FunctionIr> {
    let register_count = usize::try_from(function.num_registers).ok()?;
    if register_count < usize::from(function.arity) || register_count > usize::from(u16::MAX) + 1 {
        return None;
    }
    let decoded = decode(function)?;
    let parameter_types =
        infer_parameter_types(usize::from(function.arity), register_count, &decoded)?;
    let (leaders, instruction_by_ip) = leaders(function, &decoded)?;
    let block_types = infer_block_types(
        register_count,
        &parameter_types,
        &leaders,
        &decoded,
        &instruction_by_ip,
    )?;
    let reachable = leaders
        .iter()
        .copied()
        .filter(|leader| block_types.contains_key(leader))
        .collect::<Vec<_>>();
    let mut next_value = 0u32;
    let mut next_block = 0u32;
    let entry = BlockId(next_block);
    next_block = next_block.checked_add(1)?;
    let block_ids = reachable
        .iter()
        .map(|leader| {
            let id = BlockId(next_block);
            next_block = next_block.checked_add(1)?;
            Some((*leader, id))
        })
        .collect::<Option<HashMap<_, _>>>()?;

    let mut entry_parameters = Vec::with_capacity(usize::from(function.arity));
    let mut entry_registers = Vec::with_capacity(register_count);
    for &ty in &parameter_types {
        let value = next_id(&mut next_value)?;
        entry_parameters.push((value, ty));
        entry_registers.push(value);
    }
    for _ in parameter_types.len()..register_count {
        entry_registers.push(ValueId(u32::MAX));
    }
    let mut entry_instructions = Vec::new();
    for register in entry_registers
        .iter_mut()
        .take(register_count)
        .skip(usize::from(function.arity))
    {
        let value = next_id(&mut next_value)?;
        entry_instructions.push(IrInstruction {
            result: Some((value, IrType::I64)),
            kind: IrInstructionKind::Iconst(0),
            source: position(function, 0),
        });
        *register = value;
    }
    let first = *block_ids.get(&0)?;
    let mut blocks = vec![IrBlock {
        id: entry,
        parameters: entry_parameters,
        instructions: entry_instructions,
        terminator: IrTerminator::Jump {
            target: first,
            arguments: entry_registers,
        },
    }];

    let mut deopt_maps = Vec::new();
    for &leader in &reachable {
        let leader_index = leaders.binary_search(&leader).ok()?;
        let end = leaders
            .get(leader_index + 1)
            .copied()
            .unwrap_or_else(|| function.bytecode.len());
        let input_types = block_types.get(&leader)?;
        let mut parameters = Vec::with_capacity(register_count);
        let mut registers = Vec::with_capacity(register_count);
        for &ty in input_types {
            let value = next_id(&mut next_value)?;
            let ty = ty.unwrap_or(IrType::I64);
            parameters.push((value, ty));
            registers.push(value);
        }
        let mut instructions = Vec::new();
        let mut terminator = None;
        let mut cursor = leader;
        while cursor < end {
            let instruction = *decoded.get(*instruction_by_ip.get(&cursor)?)?;
            let source = position(function, instruction.ip);
            match instruction.opcode {
                OpCode::Move => {
                    let value = *registers.get(instruction.b)?;
                    *registers.get_mut(instruction.a)? = value;
                }
                OpCode::LoadI => {
                    let word = function.bytecode.as_slice()[instruction.ip];
                    let immediate = u16::try_from(word & 0xffff).ok()?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        IrInstructionKind::Iconst(i64::from(i16::from_ne_bytes(
                            immediate.to_ne_bytes(),
                        ))),
                        source,
                    )?;
                }
                OpCode::LoadBool => {
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::Bool,
                        IrInstructionKind::Bconst(instruction.b != 0),
                        source,
                    )?;
                }
                OpCode::ArrayLen => {
                    let array = *registers.get(instruction.b)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        IrInstructionKind::ArrayLen(array),
                        source,
                    )?;
                }
                OpCode::ArrayLoadI => {
                    let array = *registers.get(instruction.b)?;
                    let index = *registers.get(instruction.c)?;
                    let bytecode_ip = push_deopt_map(instruction.ip, &registers, &mut deopt_maps)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        IrInstructionKind::ArrayLoadI {
                            array,
                            index,
                            deopt: bytecode_ip,
                        },
                        source,
                    )?;
                }
                OpCode::AddI | OpCode::SubI => {
                    let immediate = next_id(&mut next_value)?;
                    instructions.push(IrInstruction {
                        result: Some((immediate, IrType::I64)),
                        kind: IrInstructionKind::Iconst(i64::try_from(instruction.c).ok()?),
                        source,
                    });
                    let left = *registers.get(instruction.b)?;
                    let kind = if instruction.opcode == OpCode::AddI {
                        IrInstructionKind::Iadd(left, immediate)
                    } else {
                        IrInstructionKind::Isub(left, immediate)
                    };
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        kind,
                        source,
                    )?;
                }
                OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::AddII
                | OpCode::SubII
                | OpCode::MulII => {
                    let left = *registers.get(instruction.b)?;
                    let right = *registers.get(instruction.c)?;
                    let kind = match instruction.opcode {
                        OpCode::Add | OpCode::AddII => IrInstructionKind::Iadd(left, right),
                        OpCode::Sub | OpCode::SubII => IrInstructionKind::Isub(left, right),
                        OpCode::Mul | OpCode::MulII => IrInstructionKind::Imul(left, right),
                        _ => return None,
                    };
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        kind,
                        source,
                    )?;
                }
                OpCode::LtII
                | OpCode::LeII
                | OpCode::GtII
                | OpCode::GeII
                | OpCode::EqII
                | OpCode::NeII => {
                    let predicate = predicate(instruction.opcode)?;
                    let left = *registers.get(instruction.b)?;
                    let right = *registers.get(instruction.c)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::Bool,
                        IrInstructionKind::Icmp {
                            predicate,
                            left,
                            right,
                        },
                        source,
                    )?;
                }
                OpCode::Jump | OpCode::JumpLong => {
                    record_backedge(
                        function,
                        instruction,
                        &registers,
                        &mut instructions,
                        &mut deopt_maps,
                    )?;
                    terminator = Some(IrTerminator::Jump {
                        target: *block_ids.get(&instruction.target?)?,
                        arguments: registers.clone(),
                    });
                    break;
                }
                OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
                    record_backedge(
                        function,
                        instruction,
                        &registers,
                        &mut instructions,
                        &mut deopt_maps,
                    )?;
                    let target = *block_ids.get(&instruction.target?)?;
                    let fallthrough = *block_ids.get(&instruction.next)?;
                    let jump_on_true =
                        matches!(instruction.opcode, OpCode::JumpIf | OpCode::JumpIfLong);
                    let (then_target, else_target) = if jump_on_true {
                        (target, fallthrough)
                    } else {
                        (fallthrough, target)
                    };
                    terminator = Some(IrTerminator::Branch {
                        condition: *registers.get(instruction.a)?,
                        then_target,
                        then_arguments: registers.clone(),
                        else_target,
                        else_arguments: registers.clone(),
                    });
                    break;
                }
                OpCode::Return => {
                    terminator = Some(IrTerminator::Return(*registers.get(instruction.a)?));
                    break;
                }
                _ => return None,
            }
            cursor = instruction.next;
        }
        if terminator.is_none() {
            terminator = Some(IrTerminator::Jump {
                target: *block_ids.get(&end)?,
                arguments: registers,
            });
        }
        blocks.push(IrBlock {
            id: *block_ids.get(&leader)?,
            parameters,
            instructions,
            terminator: terminator?,
        });
    }

    let ir = FunctionIr {
        name: function
            .name
            .clone()
            .unwrap_or_else(|| "anonymous".to_string()),
        entry,
        parameter_types,
        return_type: IrType::I64,
        blocks,
        deopt_maps,
    };
    ir.verify().ok()?;
    Some(ir)
}

fn decode(function: &Function) -> Option<Vec<DecodedInstruction>> {
    let words = function.bytecode.as_slice();
    let mut decoded = Vec::new();
    let mut ip = 0usize;
    while ip < words.len() {
        let word = words[ip];
        let opcode = u8::try_from(word >> 24).ok().and_then(OpCode::from_u8)?;
        let length = opcode.extension_words().checked_add(1)?;
        let next = ip.checked_add(length)?;
        if next > words.len() {
            return None;
        }
        let a = usize::from(u8::try_from((word >> 16) & 0xff).ok()?);
        let b = usize::from(u8::try_from((word >> 8) & 0xff).ok()?);
        let c = usize::from(u8::try_from(word & 0xff).ok()?);
        let target = match opcode {
            OpCode::Jump | OpCode::JumpIf | OpCode::JumpIfNot => {
                let immediate = u16::try_from(word & 0xffff).ok()?;
                offset_target(next, i32::from(i16::from_ne_bytes(immediate.to_ne_bytes())))
            }
            OpCode::JumpLong | OpCode::JumpIfLong | OpCode::JumpIfNotLong => {
                let offset = i32::from_ne_bytes(words.get(ip + 1)?.to_ne_bytes());
                offset_target(next, offset)
            }
            _ => None,
        };
        decoded.push(DecodedInstruction {
            ip,
            next,
            opcode,
            a,
            b,
            c,
            target,
        });
        ip = next;
    }
    Some(decoded)
}

fn leaders(
    function: &Function,
    decoded: &[DecodedInstruction],
) -> Option<(Vec<usize>, HashMap<usize, usize>)> {
    let instruction_by_ip = decoded
        .iter()
        .enumerate()
        .map(|(index, instruction)| (instruction.ip, index))
        .collect::<HashMap<_, _>>();
    let mut leaders = BTreeSet::from([0]);
    for instruction in decoded {
        if let Some(target) = instruction.target {
            if !instruction_by_ip.contains_key(&target) {
                return None;
            }
            leaders.insert(target);
            if instruction.next < function.bytecode.len() {
                leaders.insert(instruction.next);
            }
        } else if matches!(instruction.opcode, OpCode::Return | OpCode::Return0)
            && instruction.next < function.bytecode.len()
        {
            leaders.insert(instruction.next);
        }
    }
    Some((leaders.into_iter().collect(), instruction_by_ip))
}

fn infer_block_types(
    register_count: usize,
    parameter_types: &[IrType],
    leaders: &[usize],
    decoded: &[DecodedInstruction],
    instruction_by_ip: &HashMap<usize, usize>,
) -> Option<HashMap<usize, Vec<Option<IrType>>>> {
    let mut entry_types = vec![None; register_count];
    entry_types
        .iter_mut()
        .zip(parameter_types)
        .for_each(|(slot, ty)| *slot = Some(*ty));
    let mut inputs = HashMap::from([(0usize, entry_types)]);
    let mut queue = VecDeque::from([0usize]);
    while let Some(leader) = queue.pop_front() {
        let leader_index = leaders.binary_search(&leader).ok()?;
        let end = leaders.get(leader_index + 1).copied().unwrap_or(usize::MAX);
        let mut types = inputs.get(&leader)?.clone();
        let mut cursor = leader;
        let successors = loop {
            let instruction = *decoded.get(*instruction_by_ip.get(&cursor)?)?;
            transfer_types(instruction, &mut types)?;
            if let Some(successors) = successors(instruction, end) {
                break successors;
            }
            cursor = instruction.next;
            if cursor >= end {
                break vec![end];
            }
        };
        for successor in successors {
            if successor == usize::MAX {
                continue;
            }
            if let Some(existing) = inputs.get_mut(&successor) {
                if merge_types(existing, &types)? {
                    queue.push_back(successor);
                }
            } else {
                inputs.insert(successor, types.clone());
                queue.push_back(successor);
            }
        }
    }
    Some(inputs)
}

fn merge_types(existing: &mut [Option<IrType>], incoming: &[Option<IrType>]) -> Option<bool> {
    if existing.len() != incoming.len() {
        return None;
    }
    let mut changed = false;
    for (existing, incoming) in existing.iter_mut().zip(incoming) {
        let merged = match (*existing, *incoming) {
            (Some(left), Some(right)) if left == right => Some(left),
            (Some(_), Some(_)) => return None,
            _ => None,
        };
        if *existing != merged {
            *existing = merged;
            changed = true;
        }
    }
    Some(changed)
}

fn transfer_types(instruction: DecodedInstruction, registers: &mut [Option<IrType>]) -> Option<()> {
    let destination = instruction.a;
    match instruction.opcode {
        OpCode::Move => {
            let value = (*registers.get(instruction.b)?)?;
            *registers.get_mut(destination)? = Some(value);
        }
        OpCode::LoadI => *registers.get_mut(destination)? = Some(IrType::I64),
        OpCode::LoadBool => *registers.get_mut(destination)? = Some(IrType::Bool),
        OpCode::ArrayLen => {
            require_register_type(registers, instruction.b, IrType::I64Array)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::ArrayLoadI => {
            require_register_type(registers, instruction.b, IrType::I64Array)?;
            require_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::AddI | OpCode::SubI => {
            require_register_type(registers, instruction.b, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::AddII | OpCode::SubII | OpCode::MulII => {
            require_register_type(registers, instruction.b, IrType::I64)?;
            require_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::LtII | OpCode::LeII | OpCode::GtII | OpCode::GeII | OpCode::EqII | OpCode::NeII => {
            require_register_type(registers, instruction.b, IrType::I64)?;
            require_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::Bool);
        }
        OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
            require_register_type(registers, instruction.a, IrType::Bool)?;
        }
        OpCode::Return => require_register_type(registers, instruction.a, IrType::I64)?,
        OpCode::Jump | OpCode::JumpLong => {}
        _ => return None,
    }
    Some(())
}

fn successors(instruction: DecodedInstruction, block_end: usize) -> Option<Vec<usize>> {
    match instruction.opcode {
        OpCode::Jump | OpCode::JumpLong => Some(vec![instruction.target?]),
        OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
            Some(vec![instruction.target?, instruction.next])
        }
        OpCode::Return | OpCode::Return0 => Some(Vec::new()),
        _ if instruction.next >= block_end => Some(vec![block_end]),
        _ => None,
    }
}

fn predicate(opcode: OpCode) -> Option<IntPredicate> {
    match opcode {
        OpCode::LtII => Some(IntPredicate::SignedLessThan),
        OpCode::LeII => Some(IntPredicate::SignedLessThanOrEqual),
        OpCode::GtII => Some(IntPredicate::SignedGreaterThan),
        OpCode::GeII => Some(IntPredicate::SignedGreaterThanOrEqual),
        OpCode::EqII => Some(IntPredicate::Equal),
        OpCode::NeII => Some(IntPredicate::NotEqual),
        _ => None,
    }
}

fn record_backedge(
    function: &Function,
    instruction: DecodedInstruction,
    registers: &[ValueId],
    instructions: &mut Vec<IrInstruction>,
    deopt_maps: &mut Vec<DeoptMap>,
) -> Option<()> {
    if instruction.target? >= instruction.ip {
        return Some(());
    }
    let bytecode_ip = push_deopt_map(instruction.ip, registers, deopt_maps)?;
    instructions.push(IrInstruction {
        result: None,
        kind: IrInstructionKind::Safepoint { deopt: bytecode_ip },
        source: position(function, instruction.ip),
    });
    Some(())
}

fn push_deopt_map(ip: usize, registers: &[ValueId], deopt_maps: &mut Vec<DeoptMap>) -> Option<u32> {
    let bytecode_ip = u32::try_from(ip).ok()?;
    deopt_maps.push(DeoptMap {
        bytecode_ip,
        registers: registers
            .iter()
            .enumerate()
            .map(|(register, value)| Some((u16::try_from(register).ok()?, *value)))
            .collect::<Option<Vec<_>>>()?,
    });
    Some(bytecode_ip)
}

fn infer_parameter_types(
    arity: usize,
    register_count: usize,
    decoded: &[DecodedInstruction],
) -> Option<Vec<IrType>> {
    let mut types = vec![None; arity];
    let mut origins = vec![None; register_count];
    for (index, origin) in origins.iter_mut().take(arity).enumerate() {
        *origin = Some(index);
    }
    for instruction in decoded {
        let mut require = |register: usize, ty: IrType| -> Option<()> {
            let Some(parameter) = *origins.get(register)? else {
                return Some(());
            };
            match types[parameter] {
                Some(existing) if existing != ty => None,
                _ => {
                    types[parameter] = Some(ty);
                    Some(())
                }
            }
        };
        match instruction.opcode {
            OpCode::Move => {}
            OpCode::AddI | OpCode::SubI => require(instruction.b, IrType::I64)?,
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::AddII
            | OpCode::SubII
            | OpCode::MulII
            | OpCode::LtII
            | OpCode::LeII
            | OpCode::GtII
            | OpCode::GeII
            | OpCode::EqII
            | OpCode::NeII => {
                require(instruction.b, IrType::I64)?;
                require(instruction.c, IrType::I64)?;
            }
            OpCode::ArrayLen => require(instruction.b, IrType::I64Array)?,
            OpCode::ArrayLoadI => {
                require(instruction.b, IrType::I64Array)?;
                require(instruction.c, IrType::I64)?;
            }
            OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
                require(instruction.a, IrType::Bool)?;
            }
            OpCode::Return => require(instruction.a, IrType::I64)?,
            _ => {}
        }
        if instruction.a < register_count {
            if instruction.opcode == OpCode::Move {
                origins[instruction.a] = *origins.get(instruction.b)?;
            } else if matches!(
                instruction.opcode,
                OpCode::LoadI
                    | OpCode::LoadBool
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::AddI
                    | OpCode::SubI
                    | OpCode::AddII
                    | OpCode::SubII
                    | OpCode::MulII
                    | OpCode::LtII
                    | OpCode::LeII
                    | OpCode::GtII
                    | OpCode::GeII
                    | OpCode::EqII
                    | OpCode::NeII
                    | OpCode::ArrayLen
                    | OpCode::ArrayLoadI
            ) {
                origins[instruction.a] = None;
            }
        }
    }
    Some(
        types
            .into_iter()
            .map(|ty| ty.unwrap_or(IrType::I64))
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_value(
    registers: &mut [ValueId],
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
    *registers.get_mut(destination)? = value;
    Some(())
}

fn require_register_type(
    registers: &[Option<IrType>],
    register: usize,
    expected: IrType,
) -> Option<()> {
    (*registers.get(register)? == Some(expected)).then_some(())
}

fn next_id(next: &mut u32) -> Option<ValueId> {
    let value = ValueId(*next);
    *next = next.checked_add(1)?;
    Some(value)
}

fn offset_target(next: usize, offset: i32) -> Option<usize> {
    if offset >= 0 {
        next.checked_add(usize::try_from(offset).ok()?)
    } else {
        next.checked_sub(usize::try_from(offset.unsigned_abs()).ok()?)
    }
}

fn position(function: &Function, ip: usize) -> SourcePosition {
    SourcePosition {
        bytecode_ip: u32::try_from(ip).unwrap_or(u32::MAX),
        source_line: function
            .lines
            .iter()
            .rev()
            .find(|(offset, _)| usize::from(*offset) <= ip)
            .map(|(_, line)| *line)
            .unwrap_or(0),
    }
}
