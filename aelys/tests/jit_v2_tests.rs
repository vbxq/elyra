use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitConfig, JitConfigError, JitMode,
    RunOptions, Runtime, Value,
};

fn assert_send_sync<T: Send + Sync>() {}

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
