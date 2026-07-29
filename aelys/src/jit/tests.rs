use super::engine::{JitDeoptValue, JitEngine, JitExecution, JitKey, JitTier};
use super::ir::{
    BlockId, DeoptMap, FunctionIr, IntPredicate, IrBlock, IrInstruction, IrInstructionKind,
    IrTerminator, IrType, SourcePosition, ValueId,
};
use super::translate::translate_integer_osr;
use super::translate::{translate_integer_function, translate_optimized_integer_function};
use aelys_bytecode::{Function, OpCode};
use aelys_driver::pipeline::compilation_pipeline_with_opt;
use aelys_opt::OptimizationLevel;
use aelys_runtime::{JitArgument, JitCallResult, JitExecutor, JitFunctionKey, Value};
use aelys_syntax::Source;

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
    let compiled = engine.compile(&key, &ir).expect("IR must compile");
    assert_eq!(compiled.arity(), 2);
    assert_eq!(compiled.execute_i64(&[19, 2]).unwrap(), 42);
    let cached = engine.cached(&key).unwrap().expect("entry must be cached");
    assert!(std::sync::Arc::ptr_eq(&compiled, &cached));
}

#[test]
fn optimized_tier_has_a_distinct_cache_entry() {
    let value = ValueId(0);
    let ir = FunctionIr {
        name: "optimized_constant".to_string(),
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
    let engine = JitEngine::new(4).unwrap();
    let baseline_key = JitKey::new(7, 0, JitTier::Baseline);
    let optimized_key = JitKey::new(7, 0, JitTier::Optimized);
    let baseline = engine.compile(&baseline_key, &ir).unwrap();
    let optimized = engine.compile(&optimized_key, &ir).unwrap();

    assert_eq!(baseline.execute_i64(&[]).unwrap(), 42);
    assert_eq!(optimized.execute_i64(&[]).unwrap(), 42);
    assert!(!std::sync::Arc::ptr_eq(&baseline, &optimized));
    assert_eq!(engine.cache_len().unwrap(), 2);
}

#[test]
fn optimized_translation_inlines_bounded_structural_leaf_calls() {
    let mut leaf = Function::new(Some("increment".to_string()), 1);
    for _ in 0..20 {
        leaf.emit_a(OpCode::AddI, 0, 0, 1, 1);
    }
    leaf.emit_a(OpCode::Return, 0, 0, 0, 1);
    leaf.finalize_bytecode();

    let mut caller = Function::new(Some("caller".to_string()), 1);
    let constant = caller.add_constant_function(leaf);
    caller.emit_a(OpCode::Move, 2, 0, 0, 1);
    caller.emit_b(
        OpCode::LoadK,
        3,
        i16::try_from(constant).expect("test constant index"),
        1,
    );
    caller.emit_a(OpCode::Call, 1, 3, 1, 1);
    caller.emit_a(OpCode::Return, 1, 0, 0, 1);
    caller.finalize_bytecode();

    assert!(translate_integer_function(&caller).is_none());
    let mut ir = translate_optimized_integer_function(&caller)
        .expect("optimized translation must inline the immutable leaf");
    super::optimize::optimize_integer_ir(&mut ir);
    assert_eq!(ir.verify(), Ok(()));

    let engine = JitEngine::new(2).unwrap();
    let compiled = engine
        .compile(&JitKey::new(9, 0, JitTier::Optimized), &ir)
        .unwrap();
    assert_eq!(compiled.execute_i64(&[41]).unwrap(), 61);

    let provider = super::provider::JitProvider::new(2, 1, Some(1)).unwrap();
    let key = JitFunctionKey::root(9);
    assert_eq!(
        provider.try_execute(&key, &caller, &[JitArgument::Integer(41)], 1),
        JitCallResult::Returned(Value::int(61))
    );
    assert_eq!(
        provider.try_execute(&key, &caller, &[JitArgument::Integer(Value::INT_MAX)], 2),
        JitCallResult::Deoptimized {
            bytecode_ip: 0,
            registers: vec![(
                0,
                aelys_runtime::JitDeoptValue::Value(Value::int(Value::INT_MAX))
            )],
        }
    );
    let overflow_provider = super::provider::JitProvider::new(2, 1, Some(1)).unwrap();
    assert_eq!(
        overflow_provider.try_execute(
            &JitFunctionKey::root(10),
            &caller,
            &[JitArgument::Integer(Value::INT_MAX)],
            1
        ),
        JitCallResult::Unsupported
    );
}

#[test]
fn optimized_translation_inlines_compiler_emitted_nested_leaf_calls() {
    let mut pipeline = compilation_pipeline_with_opt(OptimizationLevel::None);
    let root = pipeline
        .compile(Source::new(
            "jit_inline",
            r#"
fn outer(value: int) -> int {
    fn advance(inner: int) -> int {
        let mut result = inner
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        return result
    }
    return advance(value) + 1
}
outer(20)
"#,
        ))
        .unwrap();
    let outer = root
        .nested_functions
        .iter()
        .find(|function| function.name.as_deref() == Some("outer"))
        .expect("compiler must emit outer as a nested function");
    let mut ir = translate_optimized_integer_function(outer)
        .expect("compiler-emitted immutable leaf must inline");
    super::optimize::optimize_integer_ir(&mut ir);
    assert_eq!(ir.verify(), Ok(()));
    let engine = JitEngine::new(2).unwrap();
    let compiled = engine
        .compile(&JitKey::new(10, 0, JitTier::Optimized), &ir)
        .unwrap();
    assert_eq!(compiled.execute_i64(&[20]).unwrap(), 41);
}

#[test]
fn optimized_translation_keeps_tiny_nested_leaf_calls_in_the_interpreter() {
    let mut pipeline = compilation_pipeline_with_opt(OptimizationLevel::None);
    let root = pipeline
        .compile(Source::new(
            "jit_inline_cost",
            r#"
fn outer(value: int) -> int {
    fn increment(inner: int) -> int { return inner + 1 }
    return increment(value)
}
outer(41)
"#,
        ))
        .unwrap();
    let outer = root
        .nested_functions
        .iter()
        .find(|function| function.name.as_deref() == Some("outer"))
        .expect("compiler must emit outer as a nested function");

    assert!(translate_optimized_integer_function(outer).is_none());
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
    let first = engine.compile(&first_key, &ir).unwrap();
    assert_eq!(first.execute_i64(&[]).unwrap(), 42);
    let second = engine.compile(&second_key, &ir).unwrap();
    assert_eq!(second.execute_i64(&[]).unwrap(), 42);
    assert!(engine.cached(&first_key).unwrap().is_none());
    assert!(engine.cached(&second_key).unwrap().is_some());
    assert_eq!(engine.cache_len().unwrap(), 1);
}

#[test]
fn bytecode_cfg_loop_translates_and_executes() {
    let mut function = Function::new(Some("sum_while".to_string()), 1);
    function.num_registers = 5;
    function.emit_b(OpCode::LoadI, 1, 0, 1);
    function.emit_b(OpCode::LoadI, 2, 0, 1);
    let loop_start = function.current_offset();
    function.emit_a(OpCode::LtII, 3, 1, 0, 1);
    let exit = function.emit_jump_if(OpCode::JumpIfNot, 3, 1);
    function.emit_a(OpCode::AddII, 3, 2, 1, 1);
    function.emit_a(OpCode::Move, 2, 3, 0, 1);
    function.emit_b(OpCode::LoadI, 4, 1, 1);
    function.emit_a(OpCode::AddII, 3, 1, 4, 1);
    function.emit_a(OpCode::Move, 1, 3, 0, 1);
    function.emit_jump_back(loop_start, 1);
    function.patch_jump(exit);
    function.emit_a(OpCode::Move, 0, 2, 0, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.emit_a(OpCode::Return0, 0, 0, 0, 1);
    function.finalize_bytecode();

    let ir = translate_integer_function(&function).expect("typed loop must translate");
    assert_eq!(ir.deopt_maps.len(), 1);
    let engine = JitEngine::new(4).unwrap();
    let compiled = engine
        .compile(&JitKey::new(9, 0, JitTier::Baseline), &ir)
        .unwrap();
    assert_eq!(compiled.execute_i64(&[10_000]).unwrap(), 49_995_000);

    let osr = translate_integer_osr(
        &function,
        u32::try_from(loop_start).expect("test loop IP fits u32"),
    )
    .expect("loop header must become an OSR entry");
    let osr_key = JitKey::for_osr(
        9,
        std::sync::Arc::from([0]),
        u32::try_from(loop_start).unwrap(),
    );
    let osr_compiled = engine.compile(&osr_key, &osr).unwrap();
    assert_eq!(osr_compiled.arity(), 5);
    assert_eq!(
        osr_compiled
            .execute_arguments(&[
                JitArgument::Integer(10_000),
                JitArgument::Integer(5_000),
                JitArgument::Integer(100),
                JitArgument::Unused,
                JitArgument::Unused,
            ])
            .unwrap(),
        JitExecution::Returned(37_497_600)
    );
}

#[test]
fn bytecode_translation_rejects_uninitialized_and_out_of_bounds_registers() {
    let mut uninitialized = Function::new(Some("uninitialized".to_string()), 0);
    uninitialized.num_registers = 1;
    uninitialized.emit_a(OpCode::Return, 0, 0, 0, 1);
    uninitialized.finalize_bytecode();
    assert!(translate_integer_function(&uninitialized).is_none());

    let mut out_of_bounds = Function::new(Some("out_of_bounds".to_string()), 0);
    out_of_bounds.num_registers = 1;
    out_of_bounds.emit_a(OpCode::LoadI, 255, 1, 0, 1);
    out_of_bounds.emit_a(OpCode::Return, 0, 0, 0, 1);
    out_of_bounds.finalize_bytecode();
    assert!(translate_integer_function(&out_of_bounds).is_none());
}

#[test]
fn bytecode_collection_loads_carry_verified_dominating_bounds_proofs() {
    for (opcode, expected_type) in [
        (OpCode::ArrayLoadI, IrType::I64Array),
        (OpCode::VecLoadI, IrType::I64Vec),
    ] {
        let mut function = Function::new(Some("collection_load".to_string()), 2);
        function.num_registers = 3;
        function.emit_a(opcode, 2, 0, 1, 1);
        function.emit_a(OpCode::Return, 2, 0, 0, 1);
        function.finalize_bytecode();

        let ir = translate_integer_function(&function).expect("collection load must translate");
        assert_eq!(ir.parameter_types, vec![expected_type, IrType::I64]);
        let kinds = ir
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .instructions
                    .iter()
                    .map(|instruction| &instruction.kind)
            })
            .collect::<Vec<_>>();
        assert!(
            kinds
                .iter()
                .any(|kind| matches!(kind, IrInstructionKind::BoundsCheck { .. }))
        );
        assert!(kinds.iter().any(|kind| matches!(
            kind,
            IrInstructionKind::ArrayLoadIUnchecked { .. }
                | IrInstructionKind::VecLoadIUnchecked { .. }
        )));
        assert!(!kinds.iter().any(|kind| matches!(
            kind,
            IrInstructionKind::ArrayLoadI { .. } | IrInstructionKind::VecLoadI { .. }
        )));
    }
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
                        kind: IrInstructionKind::Icmp {
                            predicate: IntPredicate::Equal,
                            left: input,
                            right: zero,
                        },
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

#[test]
fn compiled_guard_returns_exact_deoptimization_state() {
    let input = ValueId(0);
    let zero = ValueId(1);
    let condition = ValueId(2);
    let ir = FunctionIr {
        name: "guard_exit".to_string(),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64],
        return_type: IrType::I64,
        blocks: vec![IrBlock {
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
                    kind: IrInstructionKind::Icmp {
                        predicate: IntPredicate::Equal,
                        left: input,
                        right: zero,
                    },
                    source: position(1),
                },
                IrInstruction {
                    result: None,
                    kind: IrInstructionKind::Guard {
                        condition,
                        deopt: 4,
                    },
                    source: position(2),
                },
            ],
            terminator: IrTerminator::Return(input),
        }],
        deopt_maps: vec![DeoptMap {
            bytecode_ip: 4,
            registers: vec![(0, input)],
        }],
    };
    let engine = JitEngine::new(4).unwrap();
    let compiled = engine
        .compile(&JitKey::new(11, 0, JitTier::Optimized), &ir)
        .unwrap();

    assert_eq!(compiled.execute(&[0]).unwrap(), JitExecution::Returned(0));
    assert_eq!(
        compiled.execute(&[7]).unwrap(),
        JitExecution::Deoptimized {
            bytecode_ip: 4,
            registers: vec![(0, JitDeoptValue::Integer(7))],
        }
    );
}

#[test]
fn compiled_bounds_check_deoptimizes_only_out_of_range_values() {
    let index = ValueId(0);
    let length = ValueId(1);
    let ir = FunctionIr {
        name: "bounds_exit".to_string(),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64],
        return_type: IrType::I64,
        blocks: vec![IrBlock {
            id: BlockId(0),
            parameters: vec![(index, IrType::I64)],
            instructions: vec![
                IrInstruction {
                    result: Some((length, IrType::I64)),
                    kind: IrInstructionKind::Iconst(4),
                    source: position(0),
                },
                IrInstruction {
                    result: None,
                    kind: IrInstructionKind::BoundsCheck {
                        index,
                        length,
                        deopt: 3,
                    },
                    source: position(1),
                },
            ],
            terminator: IrTerminator::Return(index),
        }],
        deopt_maps: vec![DeoptMap {
            bytecode_ip: 3,
            registers: vec![(0, index)],
        }],
    };
    let engine = JitEngine::new(4).unwrap();
    let compiled = engine
        .compile(&JitKey::new(12, 0, JitTier::Optimized), &ir)
        .unwrap();

    assert_eq!(compiled.execute(&[2]).unwrap(), JitExecution::Returned(2));
    assert_eq!(
        compiled.execute(&[-1]).unwrap(),
        JitExecution::Deoptimized {
            bytecode_ip: 3,
            registers: vec![(0, JitDeoptValue::Integer(-1))],
        }
    );
    assert_eq!(
        compiled.execute(&[4]).unwrap(),
        JitExecution::Deoptimized {
            bytecode_ip: 3,
            registers: vec![(0, JitDeoptValue::Integer(4))],
        }
    );
}

#[test]
fn compiled_integer_array_load_is_bounded_and_preserves_reference_on_deopt() {
    let array = ValueId(0);
    let index = ValueId(1);
    let length = ValueId(2);
    let loaded = ValueId(3);
    let ir = FunctionIr {
        name: "array_load".to_string(),
        entry: BlockId(0),
        parameter_types: vec![IrType::I64Array, IrType::I64],
        return_type: IrType::I64,
        blocks: vec![IrBlock {
            id: BlockId(0),
            parameters: vec![(array, IrType::I64Array), (index, IrType::I64)],
            instructions: vec![
                IrInstruction {
                    result: Some((length, IrType::I64)),
                    kind: IrInstructionKind::ArrayLen(array),
                    source: position(7),
                },
                IrInstruction {
                    result: None,
                    kind: IrInstructionKind::BoundsCheck {
                        index,
                        length,
                        deopt: 7,
                    },
                    source: position(7),
                },
                IrInstruction {
                    result: Some((loaded, IrType::I64)),
                    kind: IrInstructionKind::ArrayLoadIUnchecked { array, index },
                    source: position(7),
                },
            ],
            terminator: IrTerminator::Return(loaded),
        }],
        deopt_maps: vec![DeoptMap {
            bytecode_ip: 7,
            registers: vec![(0, array), (1, index)],
        }],
    };
    let mut unproven = ir.clone();
    unproven.blocks[0].instructions.drain(0..2);
    assert_eq!(
        unproven.verify(),
        Err(super::ir::IrError::MissingBoundsProof)
    );
    let engine = JitEngine::new(4).unwrap();
    let compiled = engine
        .compile(&JitKey::new(13, 0, JitTier::Baseline), &ir)
        .unwrap();
    let elements = [19, 42];

    assert_eq!(
        compiled
            .execute_arguments(&[
                JitArgument::IntegerArray(&elements),
                JitArgument::Integer(1),
            ])
            .unwrap(),
        JitExecution::Returned(42)
    );
    assert_eq!(
        compiled
            .execute_arguments(&[
                JitArgument::IntegerArray(&elements),
                JitArgument::Integer(2),
            ])
            .unwrap(),
        JitExecution::Deoptimized {
            bytecode_ip: 7,
            registers: vec![
                (0, JitDeoptValue::Argument(0)),
                (1, JitDeoptValue::Integer(2)),
            ],
        }
    );
}
