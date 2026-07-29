use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlockId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ValueId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IrType {
    I64,
    Bool,
    I64Array,
    I64Vec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IntPredicate {
    Equal,
    NotEqual,
    SignedLessThan,
    SignedLessThanOrEqual,
    SignedGreaterThan,
    SignedGreaterThanOrEqual,
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
    Icmp {
        predicate: IntPredicate,
        left: ValueId,
        right: ValueId,
    },
    Guard {
        condition: ValueId,
        deopt: u32,
    },
    BoundsCheck {
        index: ValueId,
        length: ValueId,
        deopt: u32,
    },
    ArrayLen(ValueId),
    ArrayLoadI {
        array: ValueId,
        index: ValueId,
        deopt: u32,
    },
    ArrayLoadIUnchecked {
        array: ValueId,
        index: ValueId,
    },
    VecLen(ValueId),
    VecLoadI {
        vector: ValueId,
        index: ValueId,
        deopt: u32,
    },
    VecLoadIUnchecked {
        vector: ValueId,
        index: ValueId,
    },
    Safepoint {
        deopt: u32,
    },
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
            let mut lengths = HashMap::new();
            let mut bounds = HashSet::new();
            for instruction in &block.instructions {
                verify_instruction(instruction, &value_types, &available, &deopt_by_id)?;
                verify_collection_proof(instruction, &lengths, &bounds)?;
                if let Some((result, _)) = instruction.result {
                    match instruction.kind {
                        IrInstructionKind::ArrayLen(array) => {
                            lengths.insert(result, (array, IrType::I64Array));
                        }
                        IrInstructionKind::VecLen(vector) => {
                            lengths.insert(result, (vector, IrType::I64Vec));
                        }
                        _ => {}
                    }
                }
                if let IrInstructionKind::BoundsCheck { index, length, .. } = instruction.kind {
                    bounds.insert((index, length));
                }
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
        IrInstructionKind::Icmp { left, right, .. } => {
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
        IrInstructionKind::BoundsCheck {
            index,
            length,
            deopt,
        } => {
            require_available(available, index)?;
            require_available(available, length)?;
            require_type(values, index, IrType::I64)?;
            require_type(values, length, IrType::I64)?;
            require_no_result(instruction)?;
            require_deopt(deopts, available, deopt)
        }
        IrInstructionKind::ArrayLen(array) => {
            require_available(available, array)?;
            require_type(values, array, IrType::I64Array)?;
            require_result(instruction, IrType::I64)
        }
        IrInstructionKind::ArrayLoadI {
            array,
            index,
            deopt,
        } => {
            require_available(available, array)?;
            require_available(available, index)?;
            require_type(values, array, IrType::I64Array)?;
            require_type(values, index, IrType::I64)?;
            require_result(instruction, IrType::I64)?;
            require_deopt(deopts, available, deopt)
        }
        IrInstructionKind::ArrayLoadIUnchecked { array, index } => {
            require_available(available, array)?;
            require_available(available, index)?;
            require_type(values, array, IrType::I64Array)?;
            require_type(values, index, IrType::I64)?;
            require_result(instruction, IrType::I64)
        }
        IrInstructionKind::VecLen(vector) => {
            require_available(available, vector)?;
            require_type(values, vector, IrType::I64Vec)?;
            require_result(instruction, IrType::I64)
        }
        IrInstructionKind::VecLoadI {
            vector,
            index,
            deopt,
        } => {
            require_available(available, vector)?;
            require_available(available, index)?;
            require_type(values, vector, IrType::I64Vec)?;
            require_type(values, index, IrType::I64)?;
            require_result(instruction, IrType::I64)?;
            require_deopt(deopts, available, deopt)
        }
        IrInstructionKind::VecLoadIUnchecked { vector, index } => {
            require_available(available, vector)?;
            require_available(available, index)?;
            require_type(values, vector, IrType::I64Vec)?;
            require_type(values, index, IrType::I64)?;
            require_result(instruction, IrType::I64)
        }
        IrInstructionKind::Safepoint { deopt } => {
            require_no_result(instruction)?;
            require_deopt(deopts, available, deopt)
        }
    }
}

fn verify_collection_proof(
    instruction: &IrInstruction,
    lengths: &HashMap<ValueId, (ValueId, IrType)>,
    bounds: &HashSet<(ValueId, ValueId)>,
) -> Result<(), IrError> {
    let (collection, index, ty) = match instruction.kind {
        IrInstructionKind::ArrayLoadIUnchecked { array, index } => (array, index, IrType::I64Array),
        IrInstructionKind::VecLoadIUnchecked { vector, index } => (vector, index, IrType::I64Vec),
        _ => return Ok(()),
    };
    let proven = lengths.iter().any(|(length, &(source, source_type))| {
        source == collection && source_type == ty && bounds.contains(&(index, *length))
    });
    if proven {
        Ok(())
    } else {
        Err(IrError::MissingBoundsProof)
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
    MissingBoundsProof,
}
