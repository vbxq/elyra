use super::ir::{
    BlockId, DeoptMap, FunctionIr, IntPredicate, IrBlock, IrInstruction, IrInstructionKind,
    IrTerminator, IrType, SourcePosition, ValueId,
};
use aelys_bytecode::{Constant, Function, OpCode};
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
    translate_function(function, false, false)
}

pub(crate) fn translate_controlled_integer_function(function: &Function) -> Option<FunctionIr> {
    translate_function(function, false, true)
}

pub(crate) fn translate_optimized_integer_function(function: &Function) -> Option<FunctionIr> {
    translate_function(function, true, false)
}

pub(crate) fn translate_integer_osr(function: &Function, bytecode_ip: u32) -> Option<FunctionIr> {
    let ir = translate_function(function, false, false)?;
    add_osr_entry(ir, bytecode_ip)
}

pub(crate) fn translate_controlled_integer_osr(
    function: &Function,
    bytecode_ip: u32,
) -> Option<FunctionIr> {
    let ir = translate_function(function, false, true)?;
    add_osr_entry(ir, bytecode_ip)
}

fn add_osr_entry(mut ir: FunctionIr, bytecode_ip: u32) -> Option<FunctionIr> {
    let target = ir.blocks.iter().find(|block| {
        block
            .instructions
            .first()
            .is_some_and(|instruction| instruction.source.bytecode_ip == bytecode_ip)
    })?;
    let target_id = target.id;
    let parameter_types = target
        .parameters
        .iter()
        .map(|(_, ty)| *ty)
        .collect::<Vec<_>>();
    let mut next_value = ir
        .blocks
        .iter()
        .flat_map(|block| {
            block.parameters.iter().map(|(value, _)| value.0).chain(
                block
                    .instructions
                    .iter()
                    .filter_map(|instruction| instruction.result.map(|(value, _)| value.0)),
            )
        })
        .max()
        .and_then(|value| value.checked_add(1))
        .unwrap_or(0);
    let parameters = parameter_types
        .iter()
        .map(|ty| Some((next_id(&mut next_value)?, *ty)))
        .collect::<Option<Vec<_>>>()?;
    let arguments = parameters.iter().map(|(value, _)| *value).collect();
    let entry_id = BlockId(
        ir.blocks
            .iter()
            .map(|block| block.id.0)
            .max()
            .and_then(|block| block.checked_add(1))
            .unwrap_or(0),
    );
    ir.entry = entry_id;
    ir.parameter_types = parameter_types;
    ir.blocks.insert(
        0,
        IrBlock {
            id: entry_id,
            parameters,
            instructions: Vec::new(),
            terminator: IrTerminator::Jump {
                target: target_id,
                arguments,
            },
        },
    );
    ir.verify().ok()?;
    Some(ir)
}

fn translate_function(
    function: &Function,
    inline_leaf_calls: bool,
    controlled: bool,
) -> Option<FunctionIr> {
    let register_count = usize::try_from(function.num_registers).ok()?;
    if register_count < usize::from(function.arity) || register_count > usize::from(u16::MAX) + 1 {
        return None;
    }
    if function.jit_unsupported_struct
        || contains_schema_opcode(function)
        || contains_sum_opcode(function)
    {
        return None;
    }
    let decoded = decode(function)?;
    let parameter_types =
        infer_parameter_types(usize::from(function.arity), register_count, &decoded)?;
    let (leaders, instruction_by_ip) = leaders(function, &decoded)?;
    let block_types = infer_block_types(
        function,
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
    let mut return_type = None;
    for &leader in &reachable {
        let leader_index = leaders.binary_search(&leader).ok()?;
        let end = leaders
            .get(leader_index + 1)
            .copied()
            .unwrap_or_else(|| function.bytecode.len());
        let input_types = block_types.get(&leader)?;
        let mut parameters = Vec::with_capacity(register_count);
        let mut registers = Vec::with_capacity(register_count);
        let mut register_types = Vec::with_capacity(register_count);
        for &ty in input_types {
            let value = next_id(&mut next_value)?;
            let ty = ty.unwrap_or(IrType::Uninitialized);
            parameters.push((value, ty));
            registers.push(value);
            register_types.push(ty);
        }
        let mut instructions = Vec::new();
        let mut nested_functions = vec![None; register_count];
        let mut global_indices = vec![None; register_count];
        let mut terminator = None;
        let mut cursor = leader;
        while cursor < end {
            let instruction = *decoded.get(*instruction_by_ip.get(&cursor)?)?;
            let source = position(function, instruction.ip);
            if controlled {
                instructions.push(IrInstruction {
                    result: None,
                    kind: IrInstructionKind::JitPoll,
                    source,
                });
            }
            if matches!(
                instruction.opcode,
                OpCode::LoadI
                    | OpCode::LoadBool
                    | OpCode::LoadK
                    | OpCode::LoadKWide
                    | OpCode::Neg
                    | OpCode::ArrayLen
                    | OpCode::VecLen
                    | OpCode::ArrayLoadI
                    | OpCode::VecLoadI
                    | OpCode::AddI
                    | OpCode::SubI
                    | OpCode::Add
                    | OpCode::Sub
                    | OpCode::Mul
                    | OpCode::AddII
                    | OpCode::SubII
                    | OpCode::MulII
                    | OpCode::AddFF
                    | OpCode::SubFF
                    | OpCode::MulFF
                    | OpCode::DivFF
                    | OpCode::AddFFG
                    | OpCode::SubFFG
                    | OpCode::MulFFG
                    | OpCode::DivFFG
                    | OpCode::Lt
                    | OpCode::Le
                    | OpCode::Gt
                    | OpCode::Ge
                    | OpCode::Eq
                    | OpCode::Ne
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
                    | OpCode::LtFFG
                    | OpCode::LeFFG
                    | OpCode::GtFFG
                    | OpCode::GeFFG
                    | OpCode::EqFFG
                    | OpCode::NeFFG
            ) {
                *nested_functions.get_mut(instruction.a)? = None;
            }
            match instruction.opcode {
                OpCode::GetGlobalIdx | OpCode::GetGlobalIdxWide => {
                    let index = global_index(function, instruction)?;
                    *global_indices.get_mut(instruction.a)? =
                        is_native_global(function, index).then_some(index);
                    *register_types.get_mut(instruction.a)? = IrType::Uninitialized;
                    *nested_functions.get_mut(instruction.a)? = None;
                }
                OpCode::Move => {
                    let value = *registers.get(instruction.b)?;
                    *registers.get_mut(instruction.a)? = value;
                    *nested_functions.get_mut(instruction.a)? =
                        *nested_functions.get(instruction.b)?;
                }
                OpCode::LoadK | OpCode::LoadKWide if inline_leaf_calls => {
                    let index = constant_index(function, instruction)?;
                    match function.constants.get(index)? {
                        Constant::NestedFunction(index) => {
                            let index = usize::try_from(*index).ok()?;
                            function.nested_functions.get(index)?;
                            *nested_functions.get_mut(instruction.a)? = Some(index);
                        }
                        constant => {
                            let (ty, kind) = constant_ir(constant)?;
                            emit_value(
                                &mut registers,
                                &mut instructions,
                                &mut next_value,
                                instruction.a,
                                ty,
                                kind,
                                source,
                            )?;
                        }
                    }
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
                OpCode::LoadK | OpCode::LoadKWide => {
                    let constant = function
                        .constants
                        .get(constant_index(function, instruction)?)?;
                    let (ty, kind) = constant_ir(constant)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        ty,
                        kind,
                        source,
                    )?;
                }
                OpCode::ArrayLen | OpCode::VecLen => {
                    let collection = *registers.get(instruction.b)?;
                    let kind = if instruction.opcode == OpCode::ArrayLen {
                        IrInstructionKind::ArrayLen(collection)
                    } else {
                        IrInstructionKind::VecLen(collection)
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
                OpCode::ArrayLoadI | OpCode::VecLoadI => {
                    let collection = *registers.get(instruction.b)?;
                    let index = *registers.get(instruction.c)?;
                    let bytecode_ip = push_deopt_map(instruction.ip, &registers, &mut deopt_maps)?;
                    let length = next_id(&mut next_value)?;
                    let (length_kind, load_kind) = if instruction.opcode == OpCode::ArrayLoadI {
                        (
                            IrInstructionKind::ArrayLen(collection),
                            IrInstructionKind::ArrayLoadIUnchecked {
                                array: collection,
                                index,
                            },
                        )
                    } else {
                        (
                            IrInstructionKind::VecLen(collection),
                            IrInstructionKind::VecLoadIUnchecked {
                                vector: collection,
                                index,
                            },
                        )
                    };
                    instructions.push(IrInstruction {
                        result: Some((length, IrType::I64)),
                        kind: length_kind,
                        source,
                    });
                    instructions.push(IrInstruction {
                        result: None,
                        kind: IrInstructionKind::BoundsCheck {
                            index,
                            length,
                            deopt: bytecode_ip,
                        },
                        source,
                    });
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::I64,
                        load_kind,
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
                    let result_type =
                        if matches!(instruction.opcode, OpCode::Add | OpCode::Sub | OpCode::Mul)
                            && (*register_types.get(instruction.b)? == IrType::F64
                                || *register_types.get(instruction.c)? == IrType::F64)
                        {
                            IrType::F64
                        } else {
                            IrType::I64
                        };
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
                        result_type,
                        kind,
                        source,
                    )?;
                }
                OpCode::AddFF
                | OpCode::SubFF
                | OpCode::MulFF
                | OpCode::DivFF
                | OpCode::AddFFG
                | OpCode::SubFFG
                | OpCode::MulFFG
                | OpCode::DivFFG => {
                    let left = *registers.get(instruction.b)?;
                    let right = *registers.get(instruction.c)?;
                    let kind = match instruction.opcode {
                        OpCode::AddFF | OpCode::AddFFG => IrInstructionKind::Iadd(left, right),
                        OpCode::SubFF | OpCode::SubFFG => IrInstructionKind::Isub(left, right),
                        OpCode::MulFF | OpCode::MulFFG => IrInstructionKind::Imul(left, right),
                        OpCode::DivFF | OpCode::DivFFG => IrInstructionKind::Fdiv(left, right),
                        _ => return None,
                    };
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::F64,
                        kind,
                        source,
                    )?;
                }
                OpCode::Neg => {
                    let value = *registers.get(instruction.b)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        IrType::F64,
                        IrInstructionKind::Fneg(value),
                        source,
                    )?;
                }
                OpCode::Lt
                | OpCode::Le
                | OpCode::Gt
                | OpCode::Ge
                | OpCode::Eq
                | OpCode::Ne
                | OpCode::LtII
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
                OpCode::LtFF
                | OpCode::LeFF
                | OpCode::GtFF
                | OpCode::GeFF
                | OpCode::EqFF
                | OpCode::NeFF
                | OpCode::LtFFG
                | OpCode::LeFFG
                | OpCode::GtFFG
                | OpCode::GeFFG
                | OpCode::EqFFG
                | OpCode::NeFFG => {
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
                    let ty = match *register_types.get(instruction.a)? {
                        IrType::Uninitialized => IrType::I64,
                        ty => ty,
                    };
                    if !matches!(ty, IrType::I64 | IrType::F64 | IrType::Bool) {
                        return None;
                    }
                    if let Some(existing) = return_type {
                        if existing != ty {
                            return None;
                        }
                    } else {
                        return_type = Some(ty);
                    }
                    terminator = Some(IrTerminator::Return(*registers.get(instruction.a)?));
                    break;
                }
                OpCode::CallGlobal => {
                    if instruction.c > 32 {
                        return None;
                    }
                    let argument_start = instruction.a.checked_add(1)?;
                    let argument_end = argument_start.checked_add(instruction.c)?;
                    let arguments = registers.get(argument_start..argument_end)?.to_vec();
                    let global_index = u32::try_from(instruction.b).ok()?;
                    if !is_native_global(function, global_index) {
                        return None;
                    }
                    let result_type =
                        infer_call_result_type(function, &decoded, instruction.ip, instruction.a)?;
                    emit_value(
                        &mut registers,
                        &mut instructions,
                        &mut next_value,
                        instruction.a,
                        result_type,
                        IrInstructionKind::NativeCall {
                            global_index,
                            arguments,
                        },
                        source,
                    )?;
                    *register_types.get_mut(instruction.a)? = result_type;
                    *nested_functions.get_mut(instruction.a)? = None;
                }
                OpCode::Call | OpCode::CallCached => {
                    if let Some(global_index) = global_indices.get(instruction.b)?.as_ref().copied()
                    {
                        if instruction.c > 32 {
                            return None;
                        }
                        let argument_start = call_argument_start(instruction)?;
                        let argument_end = argument_start.checked_add(instruction.c)?;
                        let arguments = registers.get(argument_start..argument_end)?.to_vec();
                        let result_type = infer_call_result_type(
                            function,
                            &decoded,
                            instruction.ip,
                            instruction.a,
                        )?;
                        emit_value(
                            &mut registers,
                            &mut instructions,
                            &mut next_value,
                            instruction.a,
                            result_type,
                            IrInstructionKind::NativeCall {
                                global_index,
                                arguments,
                            },
                            source,
                        )?;
                        *register_types.get_mut(instruction.a)? = result_type;
                        *global_indices.get_mut(instruction.a)? = None;
                        *nested_functions.get_mut(instruction.a)? = None;
                    } else if inline_leaf_calls {
                        let nested = (*nested_functions.get(instruction.b)?)?;
                        let callee = function.nested_functions.get(nested)?;
                        if usize::from(callee.arity) != instruction.c {
                            return None;
                        }
                        let argument_start = call_argument_start(instruction)?;
                        let argument_end = argument_start.checked_add(instruction.c)?;
                        let arguments = registers.get(argument_start..argument_end)?;
                        let result = inline_integer_leaf(
                            callee,
                            arguments,
                            &mut instructions,
                            &mut next_value,
                            source,
                        )?;
                        *registers.get_mut(instruction.a)? = result;
                        *nested_functions.get_mut(instruction.a)? = None;
                    } else {
                        return None;
                    }
                }
                _ => return None,
            }
            if instruction.opcode == OpCode::Move {
                let ty = *register_types.get(instruction.b)?;
                *register_types.get_mut(instruction.a)? = ty;
                *global_indices.get_mut(instruction.a)? = *global_indices.get(instruction.b)?;
            } else if let Some(ty) = output_type(function, instruction, &register_types) {
                *register_types.get_mut(instruction.a)? = ty;
            }
            if !matches!(
                instruction.opcode,
                OpCode::GetGlobalIdx | OpCode::GetGlobalIdxWide | OpCode::Move
            ) {
                *global_indices.get_mut(instruction.a)? = None;
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
        return_type: return_type.unwrap_or(IrType::I64),
        blocks,
        deopt_maps,
    };
    ir.verify().ok()?;
    Some(ir)
}

// eligibility is per function, so a nested function's schema opcodes must not
fn contains_schema_opcode(function: &Function) -> bool {
    function.bytecode.as_slice().iter().any(|word| {
        matches!(
            OpCode::from_u8((word >> 24) as u8),
            Some(
                OpCode::StructNew
                    | OpCode::StructLoad
                    | OpCode::StructStore
                    | OpCode::EnumNew
                    | OpCode::EnumTest
                    | OpCode::EnumLoad
            )
        )
    })
}

fn contains_sum_opcode(function: &Function) -> bool {
    function.bytecode.iter().any(|word| {
        let opcode = OpCode::from_u8((word >> 24) as u8);
        let inner = if opcode == Some(OpCode::Wide) {
            OpCode::from_u8(((word >> 16) & 0xff) as u8)
        } else {
            opcode
        };
        matches!(
            inner,
            Some(
                OpCode::LoadUnit
                    | OpCode::LoadNone
                    | OpCode::MakeSum
                    | OpCode::SumTest
                    | OpCode::SumPayload
                    | OpCode::MatchFail
            )
        )
    })
}

fn inline_integer_leaf(
    function: &Function,
    arguments: &[ValueId],
    instructions: &mut Vec<IrInstruction>,
    next_value: &mut u32,
    source: SourcePosition,
) -> Option<ValueId> {
    const INLINE_INSTRUCTION_BUDGET: usize = 64;
    const INLINE_MINIMUM_INSTRUCTIONS: usize = 20;
    const INLINE_REGISTER_BUDGET: usize = 64;

    let register_count = usize::try_from(function.num_registers).ok()?;
    if arguments.len() != usize::from(function.arity)
        || register_count < arguments.len()
        || register_count > INLINE_REGISTER_BUDGET
    {
        return None;
    }
    let decoded = decode(function)?;
    if decoded.len() < INLINE_MINIMUM_INSTRUCTIONS || decoded.len() > INLINE_INSTRUCTION_BUDGET {
        return None;
    }
    let mut registers = arguments.to_vec();
    for _ in arguments.len()..register_count {
        let value = next_id(next_value)?;
        instructions.push(IrInstruction {
            result: Some((value, IrType::I64)),
            kind: IrInstructionKind::Iconst(0),
            source,
        });
        registers.push(value);
    }
    for instruction in decoded {
        match instruction.opcode {
            OpCode::Move => {
                *registers.get_mut(instruction.a)? = *registers.get(instruction.b)?;
            }
            OpCode::LoadI => {
                let word = function.bytecode.as_slice()[instruction.ip];
                let immediate = u16::try_from(word & 0xffff).ok()?;
                emit_value(
                    &mut registers,
                    instructions,
                    next_value,
                    instruction.a,
                    IrType::I64,
                    IrInstructionKind::Iconst(i64::from(i16::from_ne_bytes(
                        immediate.to_ne_bytes(),
                    ))),
                    source,
                )?;
            }
            OpCode::LoadK => {
                let Constant::Int(value) = function
                    .constants
                    .get(constant_index(function, instruction)?)?
                else {
                    return None;
                };
                emit_value(
                    &mut registers,
                    instructions,
                    next_value,
                    instruction.a,
                    IrType::I64,
                    IrInstructionKind::Iconst(*value),
                    source,
                )?;
            }
            OpCode::AddI | OpCode::SubI => {
                let immediate = next_id(next_value)?;
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
                    instructions,
                    next_value,
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
                    instructions,
                    next_value,
                    instruction.a,
                    IrType::I64,
                    kind,
                    source,
                )?;
            }
            OpCode::Return => {
                return registers.get(instruction.a).copied();
            }
            _ => return None,
        }
    }
    None
}

fn constant_index(function: &Function, instruction: DecodedInstruction) -> Option<usize> {
    let word = *function.bytecode.as_slice().get(instruction.ip)?;
    if instruction.opcode == OpCode::LoadKWide {
        usize::try_from(*function.bytecode.as_slice().get(instruction.ip + 1)?).ok()
    } else {
        usize::try_from(word & 0xffff).ok()
    }
}

fn constant_ir(constant: &Constant) -> Option<(IrType, IrInstructionKind)> {
    match constant {
        Constant::Bool(value) => Some((IrType::Bool, IrInstructionKind::Bconst(*value))),
        Constant::Int(value) => Some((IrType::I64, IrInstructionKind::Iconst(*value))),
        Constant::Float(bits) => Some((
            IrType::F64,
            IrInstructionKind::Iconst(i64::from_ne_bytes(bits.to_ne_bytes())),
        )),
        Constant::Null | Constant::String(_) | Constant::NestedFunction(_) => None,
    }
}

fn output_type(
    function: &Function,
    instruction: DecodedInstruction,
    register_types: &[IrType],
) -> Option<IrType> {
    Some(match instruction.opcode {
        OpCode::GetGlobalIdx | OpCode::GetGlobalIdxWide => IrType::Uninitialized,
        OpCode::LoadI => IrType::I64,
        OpCode::LoadBool => IrType::Bool,
        OpCode::LoadK | OpCode::LoadKWide => {
            constant_ir(
                function
                    .constants
                    .get(constant_index(function, instruction)?)?,
            )?
            .0
        }
        OpCode::ArrayLen
        | OpCode::VecLen
        | OpCode::ArrayLoadI
        | OpCode::VecLoadI
        | OpCode::AddI
        | OpCode::SubI
        | OpCode::AddII
        | OpCode::SubII
        | OpCode::MulII => IrType::I64,
        OpCode::Add | OpCode::Sub | OpCode::Mul => {
            if register_types.get(instruction.b) == Some(&IrType::F64)
                || register_types.get(instruction.c) == Some(&IrType::F64)
            {
                IrType::F64
            } else {
                IrType::I64
            }
        }
        OpCode::AddFF
        | OpCode::SubFF
        | OpCode::MulFF
        | OpCode::DivFF
        | OpCode::AddFFG
        | OpCode::SubFFG
        | OpCode::MulFFG
        | OpCode::DivFFG
        | OpCode::Neg => IrType::F64,
        OpCode::Lt
        | OpCode::Le
        | OpCode::Gt
        | OpCode::Ge
        | OpCode::Eq
        | OpCode::Ne
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
        | OpCode::LtFFG
        | OpCode::LeFFG
        | OpCode::GtFFG
        | OpCode::GeFFG
        | OpCode::EqFFG
        | OpCode::NeFFG => IrType::Bool,
        OpCode::Call | OpCode::CallCached if register_types.get(instruction.a).is_some() => {
            IrType::I64
        }
        OpCode::CallGlobal => match register_types.get(instruction.a)? {
            IrType::F64 => IrType::F64,
            IrType::Bool => IrType::Bool,
            IrType::I64 => IrType::I64,
            IrType::Uninitialized => return None,
            _ => return None,
        },
        _ => return None,
    })
}

fn infer_call_result_type(
    function: &Function,
    decoded: &[DecodedInstruction],
    call_ip: usize,
    destination: usize,
) -> Option<IrType> {
    let start = decoded
        .iter()
        .position(|instruction| instruction.ip == call_ip)?
        .checked_add(1)?;
    let mut tracked = destination;
    let uses = |instruction: DecodedInstruction, register: usize| {
        instruction.b == register || instruction.c == register
    };

    for (relative_index, instruction) in decoded.iter().skip(start).copied().enumerate() {
        let instruction_index = start.checked_add(relative_index)?;
        let required = match instruction.opcode {
            OpCode::Move => {
                if instruction.a == tracked {
                    if instruction.b == tracked {
                        continue;
                    }
                    return None;
                }
                if instruction.b == tracked {
                    tracked = instruction.a;
                }
                continue;
            }
            OpCode::AddI | OpCode::SubI => (instruction.b == tracked).then_some(IrType::I64),
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::AddII
            | OpCode::SubII
            | OpCode::MulII
            | OpCode::Lt
            | OpCode::Le
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::Eq
            | OpCode::Ne
            | OpCode::LtII
            | OpCode::LeII
            | OpCode::GtII
            | OpCode::GeII
            | OpCode::EqII
            | OpCode::NeII => {
                if !uses(instruction, tracked) {
                    None
                } else {
                    let other = if instruction.b == tracked {
                        instruction.c
                    } else {
                        instruction.b
                    };
                    Some(
                        if infer_register_type_before(
                            function,
                            decoded,
                            start,
                            instruction_index,
                            other,
                        ) == Some(IrType::F64)
                        {
                            IrType::F64
                        } else {
                            IrType::I64
                        },
                    )
                }
            }
            OpCode::AddFF
            | OpCode::SubFF
            | OpCode::MulFF
            | OpCode::DivFF
            | OpCode::AddFFG
            | OpCode::SubFFG
            | OpCode::MulFFG
            | OpCode::DivFFG
            | OpCode::LtFF
            | OpCode::LeFF
            | OpCode::GtFF
            | OpCode::GeFF
            | OpCode::EqFF
            | OpCode::NeFF
            | OpCode::LtFFG
            | OpCode::LeFFG
            | OpCode::GtFFG
            | OpCode::GeFFG
            | OpCode::EqFFG
            | OpCode::NeFFG => uses(instruction, tracked).then_some(IrType::F64),
            OpCode::Neg => (instruction.b == tracked).then_some(IrType::F64),
            OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
                (instruction.a == tracked).then_some(IrType::Bool)
            }
            OpCode::Return => return None,
            OpCode::Call | OpCode::CallCached | OpCode::CallGlobal => {
                let function_register = match instruction.opcode {
                    OpCode::CallGlobal | OpCode::CallCached => instruction.a,
                    OpCode::Call => instruction.b,
                    _ => unreachable!(),
                };
                let argument_start = function_register.checked_add(1)?;
                let argument_end = argument_start.checked_add(instruction.c)?;
                if (argument_start..argument_end).contains(&tracked) {
                    return None;
                }
                if instruction.a == tracked {
                    return None;
                }
                continue;
            }
            OpCode::Jump | OpCode::JumpLong => return None,
            _ => {
                if instruction.a == tracked {
                    return None;
                }
                continue;
            }
        };
        if required.is_some() {
            return required;
        }
        if instruction.a == tracked {
            return None;
        }
    }
    None
}

fn call_argument_start(instruction: DecodedInstruction) -> Option<usize> {
    instruction.a.checked_add(1)
}

fn infer_register_type_before(
    function: &Function,
    decoded: &[DecodedInstruction],
    start: usize,
    end: usize,
    target: usize,
) -> Option<IrType> {
    let register_count = decoded
        .iter()
        .take(end)
        .flat_map(|instruction| [instruction.a, instruction.b, instruction.c])
        .max()
        .unwrap_or(target)
        .max(target)
        .checked_add(1)?;
    let mut types = vec![None; register_count];
    for instruction in decoded.get(start..end)?.iter().copied() {
        let ty = match instruction.opcode {
            OpCode::Move => types.get(instruction.b).copied().flatten(),
            OpCode::LoadI => Some(IrType::I64),
            OpCode::LoadBool => Some(IrType::Bool),
            OpCode::LoadK | OpCode::LoadKWide => constant_ir(
                function
                    .constants
                    .get(constant_index(function, instruction)?)?,
            )
            .map(|(ty, _)| ty),
            OpCode::AddFF
            | OpCode::SubFF
            | OpCode::MulFF
            | OpCode::DivFF
            | OpCode::AddFFG
            | OpCode::SubFFG
            | OpCode::MulFFG
            | OpCode::DivFFG
            | OpCode::Neg => Some(IrType::F64),
            OpCode::Lt
            | OpCode::Le
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::Eq
            | OpCode::Ne
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
            | OpCode::LtFFG
            | OpCode::LeFFG
            | OpCode::GtFFG
            | OpCode::GeFFG
            | OpCode::EqFFG
            | OpCode::NeFFG => Some(IrType::Bool),
            OpCode::ArrayLen | OpCode::ArrayLoadI | OpCode::VecLen | OpCode::VecLoadI => {
                Some(IrType::I64)
            }
            _ => None,
        };
        if instruction.a < types.len() {
            types[instruction.a] = ty;
        }
    }
    types.get(target).copied().flatten()
}

fn global_index(function: &Function, instruction: DecodedInstruction) -> Option<u32> {
    let word = *function.bytecode.as_slice().get(instruction.ip)?;
    match instruction.opcode {
        OpCode::GetGlobalIdx => Some(word & 0xffff),
        OpCode::GetGlobalIdxWide => function
            .bytecode
            .as_slice()
            .get(instruction.ip.checked_add(1)?)
            .copied(),
        _ => None,
    }
}

fn is_native_global(function: &Function, index: u32) -> bool {
    usize::try_from(index)
        .ok()
        .and_then(|index| function.global_layout.names().get(index))
        .is_some_and(|name| name.contains("::"))
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
    function: &Function,
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
            transfer_types(function, instruction, &mut types)?;
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
            (Some(IrType::Uninitialized), Some(right)) => Some(right),
            (Some(left), Some(IrType::Uninitialized)) => Some(left),
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

fn transfer_types(
    function: &Function,
    instruction: DecodedInstruction,
    registers: &mut [Option<IrType>],
) -> Option<()> {
    let destination = instruction.a;
    match instruction.opcode {
        OpCode::Move => {
            let value = (*registers.get(instruction.b)?)?;
            *registers.get_mut(destination)? = Some(value);
        }
        OpCode::GetGlobalIdx | OpCode::GetGlobalIdxWide => {
            *registers.get_mut(destination)? = Some(IrType::Uninitialized);
        }
        OpCode::LoadI => *registers.get_mut(destination)? = Some(IrType::I64),
        OpCode::LoadBool => *registers.get_mut(destination)? = Some(IrType::Bool),
        OpCode::LoadK | OpCode::LoadKWide => {
            let index = constant_index(function, instruction)?;
            match function.constants.get(index)? {
                Constant::NestedFunction(_) => *registers.get_mut(destination)? = None,
                Constant::Bool(_) => *registers.get_mut(destination)? = Some(IrType::Bool),
                Constant::Int(_) => *registers.get_mut(destination)? = Some(IrType::I64),
                Constant::Float(_) => *registers.get_mut(destination)? = Some(IrType::F64),
                Constant::Null | Constant::String(_) => return None,
            }
        }
        OpCode::ArrayLen => {
            require_register_type(registers, instruction.b, IrType::I64Array)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::ArrayLoadI => {
            require_register_type(registers, instruction.b, IrType::I64Array)?;
            require_inferred_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::VecLen => {
            require_register_type(registers, instruction.b, IrType::I64Vec)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::VecLoadI => {
            require_register_type(registers, instruction.b, IrType::I64Vec)?;
            require_inferred_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::AddI | OpCode::SubI => {
            require_inferred_register_type(registers, instruction.b, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::Add | OpCode::Sub | OpCode::Mul => {
            let ty = if *registers.get(instruction.b)? == Some(IrType::F64)
                || *registers.get(instruction.c)? == Some(IrType::F64)
            {
                IrType::F64
            } else {
                IrType::I64
            };
            require_inferred_register_type(registers, instruction.b, ty)?;
            require_inferred_register_type(registers, instruction.c, ty)?;
            *registers.get_mut(destination)? = Some(ty);
        }
        OpCode::AddII | OpCode::SubII | OpCode::MulII => {
            require_inferred_register_type(registers, instruction.b, IrType::I64)?;
            require_inferred_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::I64);
        }
        OpCode::AddFF
        | OpCode::SubFF
        | OpCode::MulFF
        | OpCode::DivFF
        | OpCode::AddFFG
        | OpCode::SubFFG
        | OpCode::MulFFG
        | OpCode::DivFFG => {
            require_inferred_register_type(registers, instruction.b, IrType::F64)?;
            require_inferred_register_type(registers, instruction.c, IrType::F64)?;
            *registers.get_mut(destination)? = Some(IrType::F64);
        }
        OpCode::Neg => {
            require_inferred_register_type(registers, instruction.b, IrType::F64)?;
            *registers.get_mut(destination)? = Some(IrType::F64);
        }
        OpCode::Lt
        | OpCode::Le
        | OpCode::Gt
        | OpCode::Ge
        | OpCode::Eq
        | OpCode::Ne
        | OpCode::LtII
        | OpCode::LeII
        | OpCode::GtII
        | OpCode::GeII
        | OpCode::EqII
        | OpCode::NeII => {
            require_inferred_register_type(registers, instruction.b, IrType::I64)?;
            require_inferred_register_type(registers, instruction.c, IrType::I64)?;
            *registers.get_mut(destination)? = Some(IrType::Bool);
        }
        OpCode::LtFF
        | OpCode::LeFF
        | OpCode::GtFF
        | OpCode::GeFF
        | OpCode::EqFF
        | OpCode::NeFF
        | OpCode::LtFFG
        | OpCode::LeFFG
        | OpCode::GtFFG
        | OpCode::GeFFG
        | OpCode::EqFFG
        | OpCode::NeFFG => {
            require_inferred_register_type(registers, instruction.b, IrType::F64)?;
            require_inferred_register_type(registers, instruction.c, IrType::F64)?;
            *registers.get_mut(destination)? = Some(IrType::Bool);
        }
        OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
            require_inferred_register_type(registers, instruction.a, IrType::Bool)?;
        }
        OpCode::Return => {
            if !matches!(
                registers.get(instruction.a)?,
                Some(IrType::I64 | IrType::F64 | IrType::Bool | IrType::Uninitialized)
            ) {
                return None;
            }
        }
        OpCode::CallGlobal => {
            *registers.get_mut(destination)? = Some(IrType::Uninitialized);
        }
        OpCode::Call | OpCode::CallCached => {
            *registers.get_mut(destination)? = Some(IrType::Uninitialized);
        }
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
        OpCode::Lt | OpCode::LtII | OpCode::LtFF | OpCode::LtFFG => {
            Some(IntPredicate::SignedLessThan)
        }
        OpCode::Le | OpCode::LeII | OpCode::LeFF | OpCode::LeFFG => {
            Some(IntPredicate::SignedLessThanOrEqual)
        }
        OpCode::Gt | OpCode::GtII | OpCode::GtFF | OpCode::GtFFG => {
            Some(IntPredicate::SignedGreaterThan)
        }
        OpCode::Ge | OpCode::GeII | OpCode::GeFF | OpCode::GeFFG => {
            Some(IntPredicate::SignedGreaterThanOrEqual)
        }
        OpCode::Eq | OpCode::EqII | OpCode::EqFF | OpCode::EqFFG => Some(IntPredicate::Equal),
        OpCode::Ne | OpCode::NeII | OpCode::NeFF | OpCode::NeFFG => Some(IntPredicate::NotEqual),
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
            | OpCode::Lt
            | OpCode::Le
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::Eq
            | OpCode::Ne
            | OpCode::LtII
            | OpCode::LeII
            | OpCode::GtII
            | OpCode::GeII
            | OpCode::EqII
            | OpCode::NeII => {
                require(instruction.b, IrType::I64)?;
                require(instruction.c, IrType::I64)?;
            }
            OpCode::AddFF
            | OpCode::SubFF
            | OpCode::MulFF
            | OpCode::DivFF
            | OpCode::AddFFG
            | OpCode::SubFFG
            | OpCode::MulFFG
            | OpCode::DivFFG
            | OpCode::LtFF
            | OpCode::LeFF
            | OpCode::GtFF
            | OpCode::GeFF
            | OpCode::EqFF
            | OpCode::NeFF
            | OpCode::LtFFG
            | OpCode::LeFFG
            | OpCode::GtFFG
            | OpCode::GeFFG
            | OpCode::EqFFG
            | OpCode::NeFFG => {
                require(instruction.b, IrType::F64)?;
                require(instruction.c, IrType::F64)?;
            }
            OpCode::Neg => require(instruction.b, IrType::F64)?,
            OpCode::ArrayLen => require(instruction.b, IrType::I64Array)?,
            OpCode::ArrayLoadI => {
                require(instruction.b, IrType::I64Array)?;
                require(instruction.c, IrType::I64)?;
            }
            OpCode::VecLen => require(instruction.b, IrType::I64Vec)?,
            OpCode::VecLoadI => {
                require(instruction.b, IrType::I64Vec)?;
                require(instruction.c, IrType::I64)?;
            }
            OpCode::JumpIf | OpCode::JumpIfLong | OpCode::JumpIfNot | OpCode::JumpIfNotLong => {
                require(instruction.a, IrType::Bool)?;
            }
            OpCode::Return => {
                if let Some(parameter) = *origins.get(instruction.a)? {
                    match types[parameter] {
                        Some(IrType::I64 | IrType::F64 | IrType::Bool) | None => {}
                        Some(_) => return None,
                    }
                }
            }
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
                    | OpCode::AddFF
                    | OpCode::SubFF
                    | OpCode::MulFF
                    | OpCode::DivFF
                    | OpCode::AddFFG
                    | OpCode::SubFFG
                    | OpCode::MulFFG
                    | OpCode::DivFFG
                    | OpCode::Neg
                    | OpCode::Lt
                    | OpCode::Le
                    | OpCode::Gt
                    | OpCode::Ge
                    | OpCode::Eq
                    | OpCode::Ne
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
                    | OpCode::LtFFG
                    | OpCode::LeFFG
                    | OpCode::GtFFG
                    | OpCode::GeFFG
                    | OpCode::EqFFG
                    | OpCode::NeFFG
                    | OpCode::ArrayLen
                    | OpCode::ArrayLoadI
                    | OpCode::VecLen
                    | OpCode::VecLoadI
                    | OpCode::Call
                    | OpCode::CallCached
                    | OpCode::CallGlobal
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

fn require_inferred_register_type(
    registers: &mut [Option<IrType>],
    register: usize,
    expected: IrType,
) -> Option<()> {
    let value = registers.get_mut(register)?;
    match *value {
        Some(existing) if existing != expected && existing != IrType::Uninitialized => None,
        _ => {
            *value = Some(expected);
            Some(())
        }
    }
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

#[cfg(test)]
mod sum_opcode_tests {
    use super::contains_sum_opcode;
    use aelys_bytecode::{Function, OpCode, Register};

    #[test]
    fn every_sum_opcode_blocks_translation_in_compact_and_wide_forms() {
        let opcodes = [
            OpCode::LoadUnit,
            OpCode::LoadNone,
            OpCode::MakeSum,
            OpCode::SumTest,
            OpCode::SumPayload,
            OpCode::MatchFail,
        ];

        let mut compact = Function::new(Some("sum-compact".to_string()), 0);
        for opcode in opcodes {
            compact.emit_a(opcode, 0, 0, 0, 1);
        }
        compact.finalize_bytecode();
        assert!(contains_sum_opcode(&compact));

        let mut wide = Function::new(Some("sum-wide".to_string()), 0);
        for opcode in [
            OpCode::LoadUnit,
            OpCode::LoadNone,
            OpCode::MakeSum,
            OpCode::SumTest,
            OpCode::SumPayload,
        ] {
            wide.emit_wide_abc(
                opcode,
                Register::new(256),
                Register::new(257),
                Register::new(258),
                1,
            );
        }
        wide.finalize_bytecode();
        assert!(contains_sum_opcode(&wide));
    }
}
