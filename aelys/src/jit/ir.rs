use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlockId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ValueId(pub(crate) u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IrType {
    Uninitialized,
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
        let definitions = self
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .parameters
                    .iter()
                    .map(|(value, _)| (*value, block.id))
                    .chain(block.instructions.iter().filter_map(|instruction| {
                        instruction.result.map(|(value, _)| (value, block.id))
                    }))
            })
            .collect::<HashMap<_, _>>();
        let dominators = block_dominators(self);
        for block in &self.blocks {
            let mut available = definitions
                .iter()
                .filter_map(|(value, definition)| {
                    (*definition != block.id
                        && dominators
                            .get(&block.id)
                            .is_some_and(|blocks| blocks.contains(definition)))
                    .then_some(*value)
                })
                .chain(block.parameters.iter().map(|(value, _)| *value))
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
        let collections = collection_identities(self);
        let lengths = length_identities(self, &value_types, &collections);
        for block in &self.blocks {
            let mut bounds = HashSet::new();
            for instruction in &block.instructions {
                verify_collection_proof(instruction, &collections, &lengths, &bounds)?;
                if let IrInstructionKind::BoundsCheck { index, length, .. } = instruction.kind {
                    bounds.insert((index, length));
                }
            }
        }
        Ok(())
    }
}

fn block_dominators(ir: &FunctionIr) -> HashMap<BlockId, HashSet<BlockId>> {
    let mut predecessors = ir
        .blocks
        .iter()
        .map(|block| (block.id, HashSet::new()))
        .collect::<HashMap<_, _>>();
    for block in &ir.blocks {
        match &block.terminator {
            IrTerminator::Jump { target, .. } => {
                predecessors.entry(*target).or_default().insert(block.id);
            }
            IrTerminator::Branch {
                then_target,
                else_target,
                ..
            } => {
                predecessors
                    .entry(*then_target)
                    .or_default()
                    .insert(block.id);
                predecessors
                    .entry(*else_target)
                    .or_default()
                    .insert(block.id);
            }
            IrTerminator::Return(_) => {}
        }
    }
    let all = ir
        .blocks
        .iter()
        .map(|block| block.id)
        .collect::<HashSet<_>>();
    let mut dominators = ir
        .blocks
        .iter()
        .map(|block| {
            let blocks = if block.id == ir.entry {
                HashSet::from([ir.entry])
            } else {
                all.clone()
            };
            (block.id, blocks)
        })
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for block in &ir.blocks {
            if block.id == ir.entry {
                continue;
            }
            let mut next = predecessors
                .get(&block.id)
                .into_iter()
                .flatten()
                .filter_map(|predecessor| dominators.get(predecessor).cloned())
                .reduce(|left, right| left.intersection(&right).copied().collect())
                .unwrap_or_default();
            next.insert(block.id);
            if dominators.get(&block.id) != Some(&next) {
                dominators.insert(block.id, next);
                changed = true;
            }
        }
        if !changed {
            return dominators;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CollectionIdentity {
    pub(super) origin: ValueId,
    pub(super) ty: IrType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProofState {
    Unknown,
    Known(CollectionIdentity),
    Invalid,
}

pub(super) fn collection_identities(ir: &FunctionIr) -> HashMap<ValueId, CollectionIdentity> {
    let incoming = incoming_arguments(ir);
    let mut states = HashMap::new();
    for block in &ir.blocks {
        for &(value, ty) in &block.parameters {
            if !matches!(ty, IrType::I64Array | IrType::I64Vec) {
                continue;
            }
            let state = if block.id == ir.entry {
                ProofState::Known(CollectionIdentity { origin: value, ty })
            } else {
                ProofState::Unknown
            };
            states.insert(value, state);
        }
    }
    propagate_parameter_proofs(ir, &incoming, &mut states);
    states
        .into_iter()
        .filter_map(|(value, state)| match state {
            ProofState::Known(identity) => Some((value, identity)),
            ProofState::Unknown | ProofState::Invalid => None,
        })
        .collect()
}

fn length_identities(
    ir: &FunctionIr,
    value_types: &HashMap<ValueId, IrType>,
    collections: &HashMap<ValueId, CollectionIdentity>,
) -> HashMap<ValueId, CollectionIdentity> {
    let incoming = incoming_arguments(ir);
    let mut states = HashMap::new();
    for (&value, &ty) in value_types {
        if ty == IrType::I64 {
            states.insert(value, ProofState::Invalid);
        }
    }
    for block in &ir.blocks {
        if block.id != ir.entry {
            for &(value, ty) in &block.parameters {
                if ty == IrType::I64 {
                    states.insert(value, ProofState::Unknown);
                }
            }
        }
        for instruction in &block.instructions {
            let Some((result, IrType::I64)) = instruction.result else {
                continue;
            };
            let collection = match instruction.kind {
                IrInstructionKind::ArrayLen(array) => collections.get(&array).copied(),
                IrInstructionKind::VecLen(vector) => collections.get(&vector).copied(),
                _ => None,
            };
            if let Some(collection) = collection {
                states.insert(result, ProofState::Known(collection));
            }
        }
    }
    propagate_parameter_proofs(ir, &incoming, &mut states);
    states
        .into_iter()
        .filter_map(|(value, state)| match state {
            ProofState::Known(identity) => Some((value, identity)),
            ProofState::Unknown | ProofState::Invalid => None,
        })
        .collect()
}

fn propagate_parameter_proofs(
    ir: &FunctionIr,
    incoming: &HashMap<BlockId, Vec<Vec<ValueId>>>,
    states: &mut HashMap<ValueId, ProofState>,
) {
    loop {
        let mut changed = false;
        for block in &ir.blocks {
            if block.id == ir.entry {
                continue;
            }
            let Some(edges) = incoming.get(&block.id) else {
                continue;
            };
            for (index, &(parameter, _)) in block.parameters.iter().enumerate() {
                if !states.contains_key(&parameter) {
                    continue;
                }
                let next = merge_proofs(
                    edges
                        .iter()
                        .filter_map(|arguments| arguments.get(index))
                        .map(|argument| {
                            states.get(argument).copied().unwrap_or(ProofState::Invalid)
                        }),
                );
                if states.get(&parameter).copied() != Some(next) {
                    states.insert(parameter, next);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn merge_proofs(states: impl Iterator<Item = ProofState>) -> ProofState {
    let mut known = None;
    let mut saw_edge = false;
    for state in states {
        saw_edge = true;
        match state {
            ProofState::Unknown => {}
            ProofState::Invalid => return ProofState::Invalid,
            ProofState::Known(identity) => match known {
                Some(existing) if existing != identity => return ProofState::Invalid,
                Some(_) => {}
                None => known = Some(identity),
            },
        }
    }
    if !saw_edge {
        ProofState::Invalid
    } else {
        known.map_or(ProofState::Unknown, ProofState::Known)
    }
}

fn incoming_arguments(ir: &FunctionIr) -> HashMap<BlockId, Vec<Vec<ValueId>>> {
    let mut incoming = HashMap::<BlockId, Vec<Vec<ValueId>>>::new();
    for block in &ir.blocks {
        match &block.terminator {
            IrTerminator::Jump { target, arguments } => {
                incoming.entry(*target).or_default().push(arguments.clone());
            }
            IrTerminator::Branch {
                then_target,
                then_arguments,
                else_target,
                else_arguments,
                ..
            } => {
                incoming
                    .entry(*then_target)
                    .or_default()
                    .push(then_arguments.clone());
                incoming
                    .entry(*else_target)
                    .or_default()
                    .push(else_arguments.clone());
            }
            IrTerminator::Return(_) => {}
        }
    }
    incoming
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
            if left_type == IrType::Uninitialized || left_type != right_type {
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
    collections: &HashMap<ValueId, CollectionIdentity>,
    lengths: &HashMap<ValueId, CollectionIdentity>,
    bounds: &HashSet<(ValueId, ValueId)>,
) -> Result<(), IrError> {
    let (collection, index, ty) = match instruction.kind {
        IrInstructionKind::ArrayLoadIUnchecked { array, index } => (array, index, IrType::I64Array),
        IrInstructionKind::VecLoadIUnchecked { vector, index } => (vector, index, IrType::I64Vec),
        _ => return Ok(()),
    };
    let collection = collections.get(&collection).copied();
    let proven = lengths.iter().any(|(length, identity)| {
        collection == Some(*identity) && identity.ty == ty && bounds.contains(&(index, *length))
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
        if expected != IrType::Uninitialized {
            require_type(values, argument, expected)?;
        }
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
