use aelys::{
    CompileOptions, ExecutionOutcome, InterruptHandle, IsolateConfig, RunOptions, Runtime,
    StructuredCloneError, StructuredValue,
};
use aelys_common::error::{AelysError, RuntimeErrorKind};
use aelys_opt::OptimizationLevel;
use std::sync::Arc;
use std::time::Instant;

fn assert_send<T: Send>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn public_runtime_types_have_the_required_thread_contracts() {
    assert_send_sync::<Runtime>();
    assert_send_sync::<aelys::CompiledModule>();
    assert_send::<aelys::Isolate>();
}

#[test]
fn one_compiled_module_runs_in_parallel_isolates() {
    let runtime = Runtime::new();
    let module = Arc::new(
        runtime
            .compile("40 + 2", CompileOptions::default())
            .unwrap(),
    );
    let mut threads = Vec::new();
    for _ in 0..100 {
        let runtime = runtime.clone();
        let module = Arc::clone(&module);
        threads.push(std::thread::spawn(move || {
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            isolate.execute(&module, RunOptions::default()).unwrap()
        }));
    }
    for thread in threads {
        let ExecutionOutcome::Returned(value) = thread.join().unwrap() else {
            panic!("unexpected exit")
        };
        assert_eq!(value.as_int(), Some(42));
    }
}

#[test]
fn optimization_levels_preserve_values_and_random_streams() {
    let runtime = Runtime::new();
    let source = "[sys.random_int(0, 1000000), sys.random_int(0, 1000000), (40 + 2) * 3]";
    let mut expected = None;

    for optimization_level in [
        OptimizationLevel::None,
        OptimizationLevel::Basic,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        let module = runtime
            .compile(
                source,
                CompileOptions {
                    optimization_level,
                    ..CompileOptions::default()
                },
            )
            .unwrap();
        let mut config = IsolateConfig::default();
        config.random_seed = Some(0xA3E1_5EED);
        let mut isolate = runtime.new_isolate(config);
        let ExecutionOutcome::Returned(value) =
            isolate.execute(&module, RunOptions::default()).unwrap()
        else {
            panic!("random differential workload exited");
        };
        let cloned = isolate.structured_clone(value).unwrap();
        if let Some(expected) = &expected {
            assert_eq!(&cloned, expected);
        } else {
            expected = Some(cloned);
        }
    }
}

#[test]
fn raizen_soak_replays_globals_allocations_and_gc_deterministically() {
    let runtime = Runtime::with_jit_mode(aelys::JitMode::Tiered);
    let module = runtime
        .compile(
            r#"
let mut epoch = 0
fn task_step(id: int, tick: int) -> int {
    return (id * 17 + tick * 31 + epoch) % 997
}
let mut sum = 0
for tick in 0..500 {
    epoch = tick
    let noise = sys.random_int(0, 31)
    for id in 0..64 {
        let transient = Vec[id, tick, noise, epoch]
        sum = sum + task_step(transient[0], transient[1])
    }
}
[sum, sys.random_state()]
"#,
            CompileOptions::default(),
        )
        .unwrap();
    let mut expected = None;

    for _ in 0..2 {
        let mut config = IsolateConfig::default()
            .with_max_heap_bytes(1024 * 1024)
            .unwrap();
        config.random_seed = Some(0x5EED_2022);
        let mut isolate = runtime.new_isolate(config);
        let ExecutionOutcome::Returned(value) = isolate
            .execute(
                &module,
                RunOptions {
                    report: true,
                    ..RunOptions::default()
                },
            )
            .unwrap()
        else {
            panic!("soak workload unexpectedly exited");
        };
        let result = isolate.structured_clone(value).unwrap();
        let report = isolate.last_report().unwrap();
        assert!(report.allocations >= 32_000);
        assert!(report.collections > 0);
        assert!(report.gc_pause_max_ns < 2_000_000);
        if let Some(expected) = &expected {
            assert_eq!(&result, expected);
        } else {
            expected = Some(result);
        }
    }
}

#[test]
fn instruction_budget_and_interrupt_are_structured_errors() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("while true {}", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let error = isolate
        .execute(
            &module,
            RunOptions {
                max_instructions: Some(10),
                ..RunOptions::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AelysError::Runtime(ref error)
            if matches!(error.kind, RuntimeErrorKind::InstructionBudgetExceeded { limit: 10 })
    ));

    let interrupt = InterruptHandle::new();
    interrupt.interrupt();
    let error = isolate
        .execute(
            &module,
            RunOptions {
                interrupt: Some(interrupt),
                safepoint_interval: 1,
                ..RunOptions::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AelysError::Runtime(ref error) if matches!(error.kind, RuntimeErrorKind::Interrupted)
    ));

    let error = isolate
        .execute(
            &module,
            RunOptions {
                deadline: Some(Instant::now()),
                safepoint_interval: 1,
                ..RunOptions::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AelysError::Runtime(ref error) if matches!(error.kind, RuntimeErrorKind::DeadlineExceeded)
    ));

    let recovery = runtime.compile("42", CompileOptions::default()).unwrap();
    assert!(matches!(
        isolate.execute(&recovery, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(value) if value.as_int() == Some(42)
    ));
}

#[test]
fn sys_exit_is_an_execution_outcome() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("sys.exit(7)", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert!(matches!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Exited(7)
    ));
}

#[test]
fn compiled_modules_are_avbc_v2_and_v1_is_rejected() {
    let runtime = Runtime::new();
    let module = runtime.compile("42", CompileOptions::default()).unwrap();
    assert_eq!(&module.avbc()[4..6], &2u16.to_le_bytes());

    let mut legacy = module.avbc().to_vec();
    legacy[4..6].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(
        aelys_bytecode::asm::deserialize(&legacy),
        Err(aelys_bytecode::asm::BinaryError::UnsupportedVersion(1))
    ));
}

#[test]
fn compiled_string_constants_are_structural_and_materialized_per_isolate() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("\"isolate local\"", CompileOptions::default())
        .unwrap();
    let function = aelys_bytecode::asm::deserialize(module.avbc()).unwrap();
    assert!(matches!(
        function.constants.as_slice(),
        [aelys_bytecode::Constant::String(value)] if value == "isolate local"
    ));

    let mut first = runtime.new_isolate(IsolateConfig::default());
    let mut second = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(first_value) =
        first.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("unexpected exit")
    };
    let ExecutionOutcome::Returned(second_value) =
        second.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("unexpected exit")
    };
    assert_eq!(first.value_to_string(first_value), "isolate local");
    assert_eq!(second.value_to_string(second_value), "isolate local");
}

#[test]
fn structured_clone_copies_values_without_sharing_heap_handles() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("[1, 2, 3]", CompileOptions::default())
        .unwrap();
    let mut source = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) = source.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("unexpected exit")
    };
    let cloned = source.structured_clone(value).unwrap();
    assert_eq!(
        cloned,
        StructuredValue::Array(vec![
            StructuredValue::Int(1),
            StructuredValue::Int(2),
            StructuredValue::Int(3),
        ])
    );

    let mut target = runtime.new_isolate(IsolateConfig::default());
    let imported = target.import_clone(&cloned).unwrap();
    assert_eq!(target.structured_clone(imported).unwrap(), cloned);
    assert_ne!(value.as_ptr(), imported.as_ptr());
}

#[test]
fn structured_clone_rejects_functions() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("fn f() { return 1 }\nf", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("unexpected exit")
    };
    assert!(matches!(
        isolate.structured_clone(value),
        Err(StructuredCloneError::Unsupported("function"))
    ));
}

#[test]
fn execution_report_tracks_run_position_allocations_and_initial_seed() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "\"report allocation\"",
            CompileOptions {
                source_name: "report.aelys".to_string(),
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let mut config = IsolateConfig::default();
    config.random_seed = Some(123_456);
    let mut isolate = runtime.new_isolate(config);
    isolate
        .execute(
            &module,
            RunOptions {
                report: true,
                ..RunOptions::default()
            },
        )
        .unwrap();
    let report = isolate.last_report().unwrap();
    assert!(report.instructions > 0);
    assert!(report.allocations >= 2);
    assert_eq!(report.function.as_deref(), Some("<main>"));
    assert!(report.instruction_pointer.is_some());
    assert_eq!(report.source, "report.aelys");
    assert_eq!(report.random_seed, 123_456);
}
