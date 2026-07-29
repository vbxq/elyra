use super::engine::{JitEngine, JitKey, JitTier};
use super::ir::{
    BlockId, DeoptMap, FunctionIr, IrBlock, IrInstruction, IrInstructionKind, IrTerminator, IrType,
    SourcePosition, ValueId,
};

fn position(ip: u32) -> SourcePosition {
    SourcePosition {
        bytecode_ip: ip,
        source_line: 1,
    }
}

#[test]
fn cranelift_executes_verified_ssa_and_reuses_cache_entry() {
    let left = ValueId(0);
    let right = ValueId(1);
    let sum = ValueId(2);
    let two = ValueId(3);
    let result = ValueId(4);
    let ir = FunctionIr {
        name: "add_then_double".to_string(),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64, IrType::I64],
        return_type: IrType::I64,
        blocks: vec![IrBlock {
            id: BlockId(0),
            parameters: vec![(left, IrType::I64), (right, IrType::I64)],
            instructions: vec![
                IrInstruction {
                    result: Some((sum, IrType::I64)),
                    kind: IrInstructionKind::Iadd(left, right),
                    source: position(0),
                },
                IrInstruction {
                    result: Some((two, IrType::I64)),
                    kind: IrInstructionKind::Iconst(2),
                    source: position(1),
                },
                IrInstruction {
                    result: Some((result, IrType::I64)),
                    kind: IrInstructionKind::Imul(sum, two),
                    source: position(2),
                },
            ],
            terminator: IrTerminator::Return(result),
        }],
        deopt_maps: Vec::new(),
    };
    let engine = JitEngine::new(4).expect("native JIT must initialize");
    let key = JitKey::new(7, 0, JitTier::Baseline);
    let compiled = engine.compile(key, &ir).expect("IR must compile");
    assert_eq!(compiled.arity(), 2);
    assert_eq!(compiled.execute_i64(&[19, 2]).unwrap(), 42);
    let cached = engine.cached(key).unwrap().expect("entry must be cached");
    assert!(std::sync::Arc::ptr_eq(&compiled, &cached));
}

#[test]
fn lru_evicts_the_oldest_cache_entry() {
    let value = ValueId(0);
    let ir = FunctionIr {
        name: "constant".to_string(),
        entry: BlockId(0),
        parameter_types: Vec::new(),
        return_type: IrType::I64,
        blocks: vec![IrBlock {
            id: BlockId(0),
            parameters: Vec::new(),
            instructions: vec![IrInstruction {
                result: Some((value, IrType::I64)),
                kind: IrInstructionKind::Iconst(42),
                source: position(0),
            }],
            terminator: IrTerminator::Return(value),
        }],
        deopt_maps: Vec::new(),
    };
    let engine = JitEngine::new(1).unwrap();
    let first_key = JitKey::new(1, 0, JitTier::Baseline);
    let second_key = JitKey::new(2, 0, JitTier::Baseline);
    let first = engine.compile(first_key, &ir).unwrap();
    assert_eq!(first.execute_i64(&[]).unwrap(), 42);
    let second = engine.compile(second_key, &ir).unwrap();
    assert_eq!(second.execute_i64(&[]).unwrap(), 42);
    assert!(engine.cached(first_key).unwrap().is_none());
    assert!(engine.cached(second_key).unwrap().is_some());
    assert_eq!(engine.cache_len().unwrap(), 1);
}

#[test]
fn verifier_covers_branches_guards_safepoints_and_deopt_maps() {
    let input = ValueId(0);
    let zero = ValueId(1);
    let condition = ValueId(2);
    let output = ValueId(3);
    let ir = FunctionIr {
        name: "guarded".to_string(),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64],
        return_type: IrType::I64,
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                parameters: vec![(input, IrType::I64)],
                instructions: vec![
                    IrInstruction {
                        result: Some((zero, IrType::I64)),
                        kind: IrInstructionKind::Iconst(0),
                        source: position(0),
                    },
                    IrInstruction {
                        result: Some((condition, IrType::Bool)),
                        kind: IrInstructionKind::IcmpEq(input, zero),
                        source: position(1),
                    },
                    IrInstruction {
                        result: None,
                        kind: IrInstructionKind::Safepoint { deopt: 4 },
                        source: position(2),
                    },
                    IrInstruction {
                        result: None,
                        kind: IrInstructionKind::Guard {
                            condition,
                            deopt: 4,
                        },
                        source: position(3),
                    },
                ],
                terminator: IrTerminator::Branch {
                    condition,
                    then_target: BlockId(1),
                    then_arguments: vec![zero],
                    else_target: BlockId(1),
                    else_arguments: vec![input],
                },
            },
            IrBlock {
                id: BlockId(1),
                parameters: vec![(output, IrType::I64)],
                instructions: Vec::new(),
                terminator: IrTerminator::Return(output),
            },
        ],
        deopt_maps: vec![DeoptMap {
            bytecode_ip: 4,
            registers: vec![(0, input)],
        }],
    };
    assert_eq!(ir.verify(), Ok(()));

    let mut out_of_scope = ir.clone();
    out_of_scope.blocks[0].instructions.swap(0, 1);
    assert_eq!(
        out_of_scope.verify(),
        Err(super::ir::IrError::ValueOutOfScope(zero))
    );

    let mut duplicate_deopt = ir;
    duplicate_deopt
        .deopt_maps
        .push(duplicate_deopt.deopt_maps[0].clone());
    assert_eq!(
        duplicate_deopt.verify(),
        Err(super::ir::IrError::DuplicateDeoptMap)
    );
}
