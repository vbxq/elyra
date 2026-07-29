use aelys::{
    AelysError, CompileOptions, ExecutionOutcome, IsolateConfig, JitConfig, JitConfigError,
    JitMode, RunOptions, Runtime, Value,
};
use aelys_common::error::RuntimeErrorKind;
use aelys_opt::OptimizationLevel;

fn assert_send_sync<T: Send + Sync>() {}

const NESTED_INTEGER_LOOP: &str = r#"
fn compute(limit: int) -> int {
    let mut i = 0
    let mut sum = 0
    while i < limit {
        sum = sum + i
        i = i + 1
    }
    return sum
}

compute(10000)
"#;

const NESTED_INCREMENT: &str = "fn increment(value: int) -> int { return value + 1 } increment(41)";

#[test]
fn baseline_jit_executes_and_shares_machine_code_between_isolates() {
    assert_send_sync::<Runtime>();
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let module = runtime
        .compile(
            "let left = 19; let right = 2; (left + right) * 2",
            CompileOptions::default(),
        )
        .unwrap();

    let mut first = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        first.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);

    let mut second = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        second.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn off_mode_never_populates_the_jit_cache() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile("40 + 2", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 0);
}

#[test]
fn jit_cache_limit_is_validated_and_configurable() {
    assert_eq!(
        JitConfig::default().with_max_cache_entries(0),
        Err(JitConfigError::EmptyCache)
    );
    let config = JitConfig::default().with_max_cache_entries(1).unwrap();
    assert_eq!(config.max_cache_entries(), 1);
    let runtime = Runtime::with_jit_config(JitMode::Baseline, config).unwrap();
    assert_eq!(runtime.jit_cache_entries(), 0);
}

#[test]
fn tiered_mode_compiles_at_the_call_threshold_and_shares_the_result() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile("40 + 2", CompileOptions::default())
        .unwrap();
    let mut first = runtime.new_isolate(IsolateConfig::default());
    for _ in 0..999 {
        assert_eq!(
            first.execute(&module, RunOptions::default()).unwrap(),
            ExecutionOutcome::Returned(Value::int(42))
        );
    }
    assert_eq!(runtime.jit_cache_entries(), 0);
    assert_eq!(
        first.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);

    let mut second = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        second.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn tiered_mode_promotes_to_an_optimized_cache_entry() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile("40 + 2", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    for _ in 0..10_000 {
        assert_eq!(
            isolate.execute(&module, RunOptions::default()).unwrap(),
            ExecutionOutcome::Returned(Value::int(42))
        );
    }
    assert_eq!(runtime.jit_cache_entries(), 2);
}

#[test]
fn profiled_numeric_guard_deoptimizes_to_the_exact_interpreter_state() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn increment(value: int) -> int { return value + 1 }
let mut result = 0
for index in 0..10000 {
    result = increment(41)
}
increment(42)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(43))
    );
    assert_eq!(runtime.jit_cache_entries(), 2);
    assert_eq!(runtime.jit_deoptimizations(), 1);
}

#[test]
fn baseline_jit_compiles_nested_cfg_and_shares_it_between_isolates() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let module = runtime
        .compile(NESTED_INTEGER_LOOP, CompileOptions::default())
        .unwrap();

    let mut first = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        first.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);

    let mut second = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        second.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn execution_control_keeps_nested_calls_in_the_interpreter() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let module = runtime
        .compile(NESTED_INTEGER_LOOP, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let options = RunOptions {
        max_instructions: Some(1_000_000),
        ..RunOptions::default()
    };

    assert_eq!(
        isolate.execute(&module, options).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
    assert_eq!(runtime.jit_cache_entries(), 0);
}

#[test]
fn nested_tiered_jit_uses_the_per_isolate_call_threshold() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime.compile(NESTED_INCREMENT, options).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    for _ in 0..999 {
        assert_eq!(
            isolate.execute(&module, RunOptions::default()).unwrap(),
            ExecutionOutcome::Returned(Value::int(42))
        );
    }
    assert_eq!(runtime.jit_cache_entries(), 0);
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn tiered_jit_compiles_at_the_backedge_threshold() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(NESTED_INTEGER_LOOP, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_osr_executions(), 1);
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
}

#[test]
fn tiered_jit_enters_machine_code_from_the_hot_loop_header() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(
            r#"
fn compute(limit: int) -> int {
    let mut index = 0
    let mut total = 0
    while index < limit {
        total = total + index
        index = index + 1
    }
    return total
}
compute(100000) + 7
"#,
            CompileOptions::default(),
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(4_999_950_007))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_osr_executions(), 1);
}

#[test]
fn tiered_jit_aggregates_backedges_across_calls() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn compute() -> int {
    let mut i = 0
    while i < 100 {
        i = i + 1
    }
    return i
}
compute()
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    for _ in 0..99 {
        assert_eq!(
            isolate.execute(&module, RunOptions::default()).unwrap(),
            ExecutionOutcome::Returned(Value::int(100))
        );
    }
    assert_eq!(runtime.jit_cache_entries(), 0);
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(100))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn baseline_jit_executes_the_generic_call_fallback() {
    let mut source = String::new();
    for index in 0..256 {
        source.push_str(&format!("let padding_{index} = {index}\n"));
    }
    source.push_str(NESTED_INTEGER_LOOP);
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let module = runtime.compile(&source, CompileOptions::default()).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(49_995_000))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn nested_function_paths_do_not_collide_in_the_shared_cache() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn add_one(value: int) -> int { return value + 1 }
fn add_two(value: int) -> int { return value + 2 }
add_one(39) + add_two(0)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 2);
}

#[test]
fn jit_arithmetic_overflow_falls_back_to_the_structured_interpreter_error() {
    for expression in ["(value + 1) - 1", "value * value"] {
        let source = format!(
            "fn overflow(value: int) -> int {{ return {expression} }}\noverflow(140737488355327)"
        );
        let runtime = Runtime::with_jit_mode(JitMode::Baseline);
        let options = CompileOptions {
            optimization_level: OptimizationLevel::None,
            ..CompileOptions::default()
        };
        let module = runtime.compile(&source, options).unwrap();
        let mut isolate = runtime.new_isolate(IsolateConfig::default());

        let error = isolate.execute(&module, RunOptions::default()).unwrap_err();
        let AelysError::Runtime(error) = error else {
            panic!("expected a runtime overflow error");
        };
        assert!(matches!(error.kind, RuntimeErrorKind::IntegerOverflow));
        assert_eq!(runtime.jit_cache_entries(), 1);
    }
}

#[test]
fn baseline_jit_reads_borrowed_integer_arrays() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn read(values: Array<Int>, index: int) -> int {
    return values.len() + values[index]
}
let values = Array[19, 40]
let mut result = 0
for index in 0..1000 {
    result = read(values, 1)
}
result
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn array_jit_deoptimization_restores_the_original_reference() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn read(values: Array<Int>, index: int) -> int {
    return values[index]
}
let values = Array[19, 42]
for index in 0..1000 {
    read(values, 1)
}
read(values, 2)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let error = isolate.execute(&module, RunOptions::default()).unwrap_err();
    let AelysError::Runtime(error) = error else {
        panic!("expected an array bounds error");
    };
    assert!(
        matches!(
            error.kind,
            RuntimeErrorKind::IndexOutOfBounds {
                index: 2,
                length: 2
            }
        ),
        "{:?}",
        error.kind
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_deoptimizations(), 1);
}

#[test]
fn vec_jit_reads_length_and_deoptimizes_out_of_bounds() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn read(values: Vec<Int>, index: int) -> int {
    return values.len() + values[index]
}
let values = Vec[19, 40]
for index in 0..1000 {
    read(values, 1)
}
read(values, 2)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let error = isolate.execute(&module, RunOptions::default()).unwrap_err();
    let AelysError::Runtime(error) = error else {
        panic!("expected a vec bounds error");
    };
    assert!(
        matches!(
            error.kind,
            RuntimeErrorKind::IndexOutOfBounds {
                index: 2,
                length: 2
            }
        ),
        "{:?}",
        error.kind
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_deoptimizations(), 1);
}

#[test]
fn baseline_jit_executes_integer_array_loops() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn sum(values: Array<Int>) -> int {
    let mut index = 0
    let mut total = 0
    while index < values.len() {
        total = total + values[index]
        index = index + 1
    }
    return total
}
let values = Array[1, 2, 3, 4, 5, 6, 7, 8]
sum(values)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(36))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
}

#[test]
fn tiered_collection_calls_compile_optimized_code() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
            r#"
fn read(values: Array<Int>) -> int { return values[1] }
let values = Array[19, 42]
let mut result = 0
for index in 0..10000 {
    result = read(values)
}
result
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(42))
    );
    assert_eq!(runtime.jit_cache_entries(), 2);
    assert_eq!(runtime.jit_deoptimizations(), 0);
}

#[test]
fn tiered_jit_inlines_hot_immutable_nested_leaf_calls() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(
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
        return result
    }
    return advance(value) + 1
}
outer(20)
"#,
            options,
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    for _ in 0..9_999 {
        assert_eq!(
            isolate.execute(&module, RunOptions::default()).unwrap(),
            ExecutionOutcome::Returned(Value::int(33))
        );
    }
    assert_eq!(runtime.jit_cache_entries(), 0);
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(33))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_deoptimizations(), 0);
}
