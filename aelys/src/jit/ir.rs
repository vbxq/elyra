use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlockId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ValueId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IrType {
    I64,
    Bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    pub(crate) bytecode_ip: u32,
    pub(crate) source_line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeoptMap {
    pub(crate) bytecode_ip: u32,
    pub(crate) registers: Vec<(u16, ValueId)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum IrInstructionKind {
    Iconst(i64),
    Bconst(bool),
    Iadd(ValueId, ValueId),
    Isub(ValueId, ValueId),
    Imul(ValueId, ValueId),
    IcmpEq(ValueId, ValueId),
    Guard { condition: ValueId, deopt: u32 },
    Safepoint { deopt: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IrInstruction {
    pub(crate) result: Option<(ValueId, IrType)>,
    pub(crate) kind: IrInstructionKind,
    pub(crate) source: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum IrTerminator {
    Jump {
        target: BlockId,
        arguments: Vec<ValueId>,
    },
    Branch {
        condition: ValueId,
        then_target: BlockId,
        then_arguments: Vec<ValueId>,
        else_target: BlockId,
        else_arguments: Vec<ValueId>,
    },
    Return(ValueId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IrBlock {
    pub(crate) id: BlockId,
    pub(crate) parameters: Vec<(ValueId, IrType)>,
    pub(crate) instructions: Vec<IrInstruction>,
    pub(crate) terminator: IrTerminator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FunctionIr {
    pub(crate) name: String,
    pub(crate) entry: BlockId,
    pub(crate) parameter_types: Vec<IrType>,
    pub(crate) return_type: IrType,
    pub(crate) blocks: Vec<IrBlock>,
    pub(crate) deopt_maps: Vec<DeoptMap>,
}

impl FunctionIr {
    pub(crate) fn verify(&self) -> Result<(), IrError> {
        let block_by_id = self
            .blocks
            .iter()
            .map(|block| (block.id, block))
            .collect::<HashMap<_, _>>();
        if block_by_id.len() != self.blocks.len() || !block_by_id.contains_key(&self.entry) {
            return Err(IrError::InvalidBlockGraph);
        }
        let entry = block_by_id[&self.entry];
        let entry_types = entry
            .parameters
            .iter()
            .map(|(_, ty)| *ty)
            .collect::<Vec<_>>();
        if entry_types != self.parameter_types {
            return Err(IrError::EntrySignatureMismatch);
        }

        let mut value_types = HashMap::new();
        for block in &self.blocks {
            for &(value, ty) in &block.parameters {
                if value_types.insert(value, ty).is_some() {
                    return Err(IrError::DuplicateValue(value));
                }
            }
            for instruction in &block.instructions {
                if let Some((value, ty)) = instruction.result
                    && value_types.insert(value, ty).is_some()
                {
                    return Err(IrError::DuplicateValue(value));
                }
            }
        }

        let deopt_by_id = self
            .deopt_maps
            .iter()
            .map(|map| (map.bytecode_ip, map))
            .collect::<HashMap<_, _>>();
        if deopt_by_id.len() != self.deopt_maps.len() {
            return Err(IrError::DuplicateDeoptMap);
        }
        for map in &self.deopt_maps {
            for &(_, value) in &map.registers {
                require_value(&value_types, value)?;
            }
        }
        for block in &self.blocks {
            let mut available = block
                .parameters
                .iter()
                .map(|(value, _)| *value)
                .collect::<HashSet<_>>();
            for instruction in &block.instructions {
                verify_instruction(instruction, &value_types, &available, &deopt_by_id)?;
                if let Some((result, _)) = instruction.result {
                    available.insert(result);
                }
            }
            verify_terminator(
                block,
                &block_by_id,
                &value_types,
                &available,
                self.return_type,
            )?;
        }
        Ok(())
    }
}

fn verify_instruction(
    instruction: &IrInstruction,
    values: &HashMap<ValueId, IrType>,
    available: &HashSet<ValueId>,
    deopts: &HashMap<u32, &DeoptMap>,
) -> Result<(), IrError> {
    match instruction.kind {
        IrInstructionKind::Iconst(_) => require_result(instruction, IrType::I64),
        IrInstructionKind::Bconst(_) => require_result(instruction, IrType::Bool),
        IrInstructionKind::Iadd(left, right)
        | IrInstructionKind::Isub(left, right)
        | IrInstructionKind::Imul(left, right) => {
            require_available(available, left)?;
            require_available(available, right)?;
            require_type(values, left, IrType::I64)?;
            require_type(values, right, IrType::I64)?;
            require_result(instruction, IrType::I64)
        }
        IrInstructionKind::IcmpEq(left, right) => {
            require_available(available, left)?;
            require_available(available, right)?;
            let left_type = require_value(values, left)?;
            let right_type = require_value(values, right)?;
            if left_type != right_type {
                return Err(IrError::TypeMismatch);
            }
            require_result(instruction, IrType::Bool)
        }
        IrInstructionKind::Guard { condition, deopt } => {
            require_available(available, condition)?;
            require_type(values, condition, IrType::Bool)?;
            require_no_result(instruction)?;
            require_deopt(deopts, available, deopt)
        }
        IrInstructionKind::Safepoint { deopt } => {
            require_no_result(instruction)?;
            require_deopt(deopts, available, deopt)
        }
    }
}

fn verify_terminator(
    block: &IrBlock,
    blocks: &HashMap<BlockId, &IrBlock>,
    values: &HashMap<ValueId, IrType>,
    available: &HashSet<ValueId>,
    return_type: IrType,
) -> Result<(), IrError> {
    match &block.terminator {
        IrTerminator::Jump { target, arguments } => {
            verify_edge(*target, arguments, blocks, values, available)
        }
        IrTerminator::Branch {
            condition,
            then_target,
            then_arguments,
            else_target,
            else_arguments,
        } => {
            require_available(available, *condition)?;
            require_type(values, *condition, IrType::Bool)?;
            verify_edge(*then_target, then_arguments, blocks, values, available)?;
            verify_edge(*else_target, else_arguments, blocks, values, available)
        }
        IrTerminator::Return(value) => {
            require_available(available, *value)?;
            require_type(values, *value, return_type)
        }
    }
}

fn verify_edge(
    target: BlockId,
    arguments: &[ValueId],
    blocks: &HashMap<BlockId, &IrBlock>,
    values: &HashMap<ValueId, IrType>,
    available: &HashSet<ValueId>,
) -> Result<(), IrError> {
    let Some(target) = blocks.get(&target) else {
        return Err(IrError::UnknownBlock);
    };
    if arguments.len() != target.parameters.len() {
        return Err(IrError::BlockArgumentMismatch);
    }
    for (&argument, &(_, expected)) in arguments.iter().zip(&target.parameters) {
        require_available(available, argument)?;
        require_type(values, argument, expected)?;
    }
    Ok(())
}

fn require_result(instruction: &IrInstruction, expected: IrType) -> Result<(), IrError> {
    match instruction.result {
        Some((_, actual)) if actual == expected => Ok(()),
        _ => Err(IrError::TypeMismatch),
    }
}

fn require_no_result(instruction: &IrInstruction) -> Result<(), IrError> {
    if instruction.result.is_none() {
        Ok(())
    } else {
        Err(IrError::UnexpectedResult)
    }
}

fn require_deopt(
    deopts: &HashMap<u32, &DeoptMap>,
    available: &HashSet<ValueId>,
    deopt: u32,
) -> Result<(), IrError> {
    let Some(map) = deopts.get(&deopt) else {
        return Err(IrError::UnknownDeoptMap(deopt));
    };
    for &(_, value) in &map.registers {
        require_available(available, value)?;
    }
    Ok(())
}

fn require_available(available: &HashSet<ValueId>, value: ValueId) -> Result<(), IrError> {
    if available.contains(&value) {
        Ok(())
    } else {
        Err(IrError::ValueOutOfScope(value))
    }
}

fn require_type(
    values: &HashMap<ValueId, IrType>,
    value: ValueId,
    expected: IrType,
) -> Result<(), IrError> {
    if require_value(values, value)? == expected {
        Ok(())
    } else {
        Err(IrError::TypeMismatch)
    }
}

fn require_value(values: &HashMap<ValueId, IrType>, value: ValueId) -> Result<IrType, IrError> {
    values
        .get(&value)
        .copied()
        .ok_or(IrError::UnknownValue(value))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IrError {
    InvalidBlockGraph,
    EntrySignatureMismatch,
    DuplicateValue(ValueId),
    UnknownValue(ValueId),
    UnknownBlock,
    UnknownDeoptMap(u32),
    DuplicateDeoptMap,
    ValueOutOfScope(ValueId),
    TypeMismatch,
    UnexpectedResult,
    BlockArgumentMismatch,
}
