use super::ir::{
    DeoptMap, FunctionIr, IntPredicate, IrInstruction, IrInstructionKind, IrTerminator, IrType,
    SourcePosition, ValueId,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OptimizationReport {
    pub(crate) constants_folded: u64,
    pub(crate) redundant_instructions: u64,
    pub(crate) dead_instructions: u64,
    pub(crate) bounds_checks_eliminated: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Constant {
    Integer(i64),
    Boolean(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Expression {
    Integer(i64),
    Boolean(bool),
    Add(ValueId, ValueId),
    Sub(ValueId, ValueId),
    Multiply(ValueId, ValueId),
    Compare(IntPredicate, ValueId, ValueId),
}

pub(crate) fn optimize_integer_ir(ir: &mut FunctionIr) -> OptimizationReport {
    let mut report = OptimizationReport::default();
    let mut aliases = HashMap::new();

    for block in &mut ir.blocks {
        let mut constants = HashMap::new();
        let mut expressions = HashMap::new();
        let mut retained = Vec::with_capacity(block.instructions.len());
        for mut instruction in block.instructions.drain(..) {
            rewrite_instruction(&mut instruction.kind, &aliases);
            if let Some(constant) = fold(&instruction.kind, &constants) {
                instruction.kind = match constant {
                    Constant::Integer(value) => IrInstructionKind::Iconst(value),
                    Constant::Boolean(value) => IrInstructionKind::Bconst(value),
                };
                report.constants_folded = report.constants_folded.saturating_add(1);
            }

            let Some((result, _)) = instruction.result else {
                retained.push(instruction);
                continue;
            };
            let Some(expression) = expression(&instruction.kind) else {
                retained.push(instruction);
                continue;
            };
            if let Some(existing) = expressions.get(&expression).copied() {
                aliases.insert(result, resolve(existing, &aliases));
                report.redundant_instructions = report.redundant_instructions.saturating_add(1);
                continue;
            }
            expressions.insert(expression, result);
            if let Some(constant) = constant(&instruction.kind) {
                constants.insert(result, constant);
            }
            retained.push(instruction);
        }
        block.instructions = retained;
        rewrite_terminator(&mut block.terminator, &aliases);
    }

    let constants = constant_values(ir);
    for block in &mut ir.blocks {
        block.instructions.retain(|instruction| {
            let IrInstructionKind::BoundsCheck { index, length, .. } = instruction.kind else {
                return true;
            };
            let proven = match (constants.get(&index), constants.get(&length)) {
                (Some(Constant::Integer(index)), Some(Constant::Integer(length))) => {
                    *index >= 0 && index < length
                }
                _ => false,
            };
            if proven {
                report.bounds_checks_eliminated = report.bounds_checks_eliminated.saturating_add(1);
            }
            !proven
        });
    }

    for block in &mut ir.blocks {
        for instruction in &mut block.instructions {
            rewrite_instruction(&mut instruction.kind, &aliases);
        }
        rewrite_terminator(&mut block.terminator, &aliases);
    }
    for map in &mut ir.deopt_maps {
        for (_, value) in &mut map.registers {
            *value = resolve(*value, &aliases);
        }
    }

    loop {
        let used = used_values(ir);
        let mut removed = 0u64;
        for block in &mut ir.blocks {
            block.instructions.retain(|instruction| {
                let removable = instruction
                    .result
                    .is_some_and(|(result, _)| !used.contains(&result))
                    && is_pure(&instruction.kind);
                if removable {
                    removed = removed.saturating_add(1);
                }
                !removable
            });
        }
        report.dead_instructions = report.dead_instructions.saturating_add(removed);
        if removed == 0 {
            break;
        }
    }
    report
}

pub(crate) fn specialize_integer_parameters(ir: &mut FunctionIr, profile: &[Option<i64>]) -> usize {
    if !ir.deopt_maps.is_empty() {
        return 0;
    }
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
    let Some(entry) = ir.blocks.iter_mut().find(|block| block.id == ir.entry) else {
        return 0;
    };
    if profile.len() != entry.parameters.len() {
        return 0;
    }
    let source = SourcePosition {
        bytecode_ip: 0,
        source_line: 0,
    };
    let mut guards = Vec::new();
    for (&(parameter, ty), expected) in entry.parameters.iter().zip(profile) {
        let Some(expected) = expected else {
            continue;
        };
        if ty != IrType::I64 {
            continue;
        }
        let constant = ValueId(next_value);
        let Some(after_constant) = next_value.checked_add(1) else {
            return 0;
        };
        let condition = ValueId(after_constant);
        let Some(after_condition) = after_constant.checked_add(1) else {
            return 0;
        };
        next_value = after_condition;
        guards.push(IrInstruction {
            result: Some((constant, IrType::I64)),
            kind: IrInstructionKind::Iconst(*expected),
            source,
        });
        guards.push(IrInstruction {
            result: Some((condition, IrType::Bool)),
            kind: IrInstructionKind::Icmp {
                predicate: IntPredicate::Equal,
                left: parameter,
                right: constant,
            },
            source,
        });
        guards.push(IrInstruction {
            result: None,
            kind: IrInstructionKind::Guard {
                condition,
                deopt: 0,
            },
            source,
        });
    }
    let specialized = guards.len() / 3;
    if specialized == 0 {
        return 0;
    }
    entry.instructions.splice(0..0, guards);
    if !ir.deopt_maps.iter().any(|map| map.bytecode_ip == 0) {
        ir.deopt_maps.push(DeoptMap {
            bytecode_ip: 0,
            registers: entry
                .parameters
                .iter()
                .enumerate()
                .filter_map(|(register, (value, _))| Some((u16::try_from(register).ok()?, *value)))
                .collect(),
        });
    }
    specialized
}

fn fold(kind: &IrInstructionKind, constants: &HashMap<ValueId, Constant>) -> Option<Constant> {
    let integer = |value| match constants.get(&value) {
        Some(Constant::Integer(value)) => Some(*value),
        _ => None,
    };
    let in_range = |value: i64| {
        (aelys_bytecode::Value::INT_MIN..=aelys_bytecode::Value::INT_MAX)
            .contains(&value)
            .then_some(Constant::Integer(value))
    };
    match *kind {
        IrInstructionKind::Iadd(left, right) => {
            in_range(integer(left)?.checked_add(integer(right)?)?)
        }
        IrInstructionKind::Isub(left, right) => {
            in_range(integer(left)?.checked_sub(integer(right)?)?)
        }
        IrInstructionKind::Imul(left, right) => {
            in_range(integer(left)?.checked_mul(integer(right)?)?)
        }
        IrInstructionKind::Icmp {
            predicate,
            left,
            right,
        } => {
            let left = integer(left)?;
            let right = integer(right)?;
            Some(Constant::Boolean(match predicate {
                IntPredicate::Equal => left == right,
                IntPredicate::NotEqual => left != right,
                IntPredicate::SignedLessThan => left < right,
                IntPredicate::SignedLessThanOrEqual => left <= right,
                IntPredicate::SignedGreaterThan => left > right,
                IntPredicate::SignedGreaterThanOrEqual => left >= right,
            }))
        }
        _ => None,
    }
}

fn constant(kind: &IrInstructionKind) -> Option<Constant> {
    match *kind {
        IrInstructionKind::Iconst(value) => Some(Constant::Integer(value)),
        IrInstructionKind::Bconst(value) => Some(Constant::Boolean(value)),
        _ => None,
    }
}

fn constant_values(ir: &FunctionIr) -> HashMap<ValueId, Constant> {
    ir.blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| Some((instruction.result?.0, constant(&instruction.kind)?)))
        .collect()
}

fn expression(kind: &IrInstructionKind) -> Option<Expression> {
    match *kind {
        IrInstructionKind::Iconst(value) => Some(Expression::Integer(value)),
        IrInstructionKind::Bconst(value) => Some(Expression::Boolean(value)),
        IrInstructionKind::Iadd(left, right) => {
            let (left, right) = ordered(left, right);
            Some(Expression::Add(left, right))
        }
        IrInstructionKind::Isub(left, right) => Some(Expression::Sub(left, right)),
        IrInstructionKind::Imul(left, right) => {
            let (left, right) = ordered(left, right);
            Some(Expression::Multiply(left, right))
        }
        IrInstructionKind::Icmp {
            predicate,
            left,
            right,
        } => Some(Expression::Compare(predicate, left, right)),
        IrInstructionKind::Guard { .. }
        | IrInstructionKind::BoundsCheck { .. }
        | IrInstructionKind::Safepoint { .. } => None,
    }
}

fn ordered(left: ValueId, right: ValueId) -> (ValueId, ValueId) {
    if left.0 <= right.0 {
        (left, right)
    } else {
        (right, left)
    }
}

fn resolve(mut value: ValueId, aliases: &HashMap<ValueId, ValueId>) -> ValueId {
    while let Some(next) = aliases.get(&value).copied() {
        if next == value {
            break;
        }
        value = next;
    }
    value
}

fn rewrite_instruction(kind: &mut IrInstructionKind, aliases: &HashMap<ValueId, ValueId>) {
    match kind {
        IrInstructionKind::Iadd(left, right)
        | IrInstructionKind::Isub(left, right)
        | IrInstructionKind::Imul(left, right) => {
            *left = resolve(*left, aliases);
            *right = resolve(*right, aliases);
        }
        IrInstructionKind::Icmp { left, right, .. } => {
            *left = resolve(*left, aliases);
            *right = resolve(*right, aliases);
        }
        IrInstructionKind::Guard { condition, .. } => {
            *condition = resolve(*condition, aliases);
        }
        IrInstructionKind::BoundsCheck { index, length, .. } => {
            *index = resolve(*index, aliases);
            *length = resolve(*length, aliases);
        }
        IrInstructionKind::Iconst(_)
        | IrInstructionKind::Bconst(_)
        | IrInstructionKind::Safepoint { .. } => {}
    }
}

fn rewrite_terminator(terminator: &mut IrTerminator, aliases: &HashMap<ValueId, ValueId>) {
    match terminator {
        IrTerminator::Jump { arguments, .. } => rewrite_values(arguments, aliases),
        IrTerminator::Branch {
            condition,
            then_arguments,
            else_arguments,
            ..
        } => {
            *condition = resolve(*condition, aliases);
            rewrite_values(then_arguments, aliases);
            rewrite_values(else_arguments, aliases);
        }
        IrTerminator::Return(value) => *value = resolve(*value, aliases),
    }
}

fn rewrite_values(values: &mut [ValueId], aliases: &HashMap<ValueId, ValueId>) {
    for value in values {
        *value = resolve(*value, aliases);
    }
}

fn used_values(ir: &FunctionIr) -> HashSet<ValueId> {
    let mut used = HashSet::new();
    for block in &ir.blocks {
        for instruction in &block.instructions {
            match instruction.kind {
                IrInstructionKind::Iadd(left, right)
                | IrInstructionKind::Isub(left, right)
                | IrInstructionKind::Imul(left, right)
                | IrInstructionKind::Icmp { left, right, .. } => {
                    used.insert(left);
                    used.insert(right);
                }
                IrInstructionKind::Guard { condition, .. } => {
                    used.insert(condition);
                }
                IrInstructionKind::BoundsCheck { index, length, .. } => {
                    used.insert(index);
                    used.insert(length);
                }
                IrInstructionKind::Iconst(_)
                | IrInstructionKind::Bconst(_)
                | IrInstructionKind::Safepoint { .. } => {}
            }
        }
        match &block.terminator {
            IrTerminator::Jump { arguments, .. } => used.extend(arguments.iter().copied()),
            IrTerminator::Branch {
                condition,
                then_arguments,
                else_arguments,
                ..
            } => {
                used.insert(*condition);
                used.extend(then_arguments.iter().copied());
                used.extend(else_arguments.iter().copied());
            }
            IrTerminator::Return(value) => {
                used.insert(*value);
            }
        }
    }
    for map in &ir.deopt_maps {
        used.extend(map.registers.iter().map(|(_, value)| *value));
    }
    used
}

fn is_pure(kind: &IrInstructionKind) -> bool {
    !matches!(
        kind,
        IrInstructionKind::Guard { .. }
            | IrInstructionKind::BoundsCheck { .. }
            | IrInstructionKind::Safepoint { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::ir::{BlockId, DeoptMap, IrBlock, IrInstruction, IrType, SourcePosition};

    fn instruction(result: u32, kind: IrInstructionKind) -> IrInstruction {
        IrInstruction {
            result: Some((ValueId(result), IrType::I64)),
            kind,
            source: SourcePosition {
                bytecode_ip: result,
                source_line: 1,
            },
        }
    }

    #[test]
    fn folds_constants_eliminates_redundancy_and_preserves_deopt_values() {
        let mut ir = FunctionIr {
            name: "optimized".to_string(),
            entry: BlockId(0),
            parameter_types: Vec::new(),
            return_type: IrType::I64,
            blocks: vec![IrBlock {
                id: BlockId(0),
                parameters: Vec::new(),
                instructions: vec![
                    instruction(0, IrInstructionKind::Iconst(19)),
                    instruction(1, IrInstructionKind::Iconst(2)),
                    instruction(2, IrInstructionKind::Iadd(ValueId(0), ValueId(1))),
                    instruction(3, IrInstructionKind::Iadd(ValueId(0), ValueId(1))),
                    instruction(4, IrInstructionKind::Iconst(99)),
                ],
                terminator: IrTerminator::Return(ValueId(3)),
            }],
            deopt_maps: vec![DeoptMap {
                bytecode_ip: 7,
                registers: vec![(0, ValueId(3))],
            }],
        };

        let report = optimize_integer_ir(&mut ir);

        assert_eq!(ir.verify(), Ok(()));
        assert_eq!(report.constants_folded, 2);
        assert_eq!(report.redundant_instructions, 1);
        assert_eq!(report.dead_instructions, 3);
        assert_eq!(ir.blocks[0].instructions.len(), 1);
        assert_eq!(ir.blocks[0].terminator, IrTerminator::Return(ValueId(2)));
        assert_eq!(ir.deopt_maps[0].registers, vec![(0, ValueId(2))]);
    }

    #[test]
    fn eliminates_only_statically_proven_bounds_checks() {
        let mut ir = FunctionIr {
            name: "bounds".to_string(),
            entry: BlockId(0),
            parameter_types: Vec::new(),
            return_type: IrType::I64,
            blocks: vec![IrBlock {
                id: BlockId(0),
                parameters: Vec::new(),
                instructions: vec![
                    instruction(0, IrInstructionKind::Iconst(2)),
                    instruction(1, IrInstructionKind::Iconst(4)),
                    IrInstruction {
                        result: None,
                        kind: IrInstructionKind::BoundsCheck {
                            index: ValueId(0),
                            length: ValueId(1),
                            deopt: 7,
                        },
                        source: SourcePosition {
                            bytecode_ip: 7,
                            source_line: 1,
                        },
                    },
                ],
                terminator: IrTerminator::Return(ValueId(0)),
            }],
            deopt_maps: vec![DeoptMap {
                bytecode_ip: 7,
                registers: vec![(0, ValueId(0))],
            }],
        };

        let mut failing = ir.clone();
        let report = optimize_integer_ir(&mut ir);

        assert_eq!(ir.verify(), Ok(()));
        assert_eq!(report.bounds_checks_eliminated, 1);
        assert!(
            ir.blocks[0]
                .instructions
                .iter()
                .all(|instruction| !matches!(
                    instruction.kind,
                    IrInstructionKind::BoundsCheck { .. }
                ))
        );

        failing.blocks[0]
            .instructions
            .insert(0, instruction(2, IrInstructionKind::Iconst(-1)));
        let check = failing.blocks[0]
            .instructions
            .iter_mut()
            .find(|instruction| matches!(instruction.kind, IrInstructionKind::BoundsCheck { .. }))
            .unwrap();
        let IrInstructionKind::BoundsCheck { index, .. } = &mut check.kind else {
            unreachable!()
        };
        *index = ValueId(2);
        failing.deopt_maps[0].registers = vec![(0, ValueId(2))];

        let report = optimize_integer_ir(&mut failing);

        assert_eq!(failing.verify(), Ok(()));
        assert_eq!(report.bounds_checks_eliminated, 0);
        assert!(failing.blocks[0].instructions.iter().any(|instruction| {
            matches!(instruction.kind, IrInstructionKind::BoundsCheck { .. })
        }));
    }
}
