use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime,
    run_with_config_and_opt,
};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{Value, VmConfig};

const LEVELS: [OptimizationLevel; 4] = [
    OptimizationLevel::None,
    OptimizationLevel::Basic,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn run_at(code: &str, level: OptimizationLevel) -> Value {
    match run_with_config_and_opt(code, "<test>", VmConfig::default(), Vec::new(), level) {
        Ok(value) => value,
        Err(error) => panic!("direct run at {level:?} failed: {error}"),
    }
}

fn run_from_avbc(code: &str, level: OptimizationLevel) -> Value {
    let producer = Runtime::with_jit_mode(JitMode::Off);
    let options = CompileOptions {
        optimization_level: level,
        source_name: "<test>".to_string(),
    };
    let module = match producer.compile(code, options) {
        Ok(module) => module,
        Err(error) => panic!("compile at {level:?} failed: {error}"),
    };
    let avbc = module.avbc().to_vec();

    let consumer = Runtime::with_jit_mode(JitMode::Off);
    let loaded = match consumer.load_avbc(&avbc, "<test>.avbc") {
        Ok(loaded) => loaded,
        Err(error) => panic!("loading the bytecode built at {level:?} failed: {error}"),
    };
    let mut isolate = consumer.new_isolate(IsolateConfig::default());
    match isolate.execute(&loaded, RunOptions::default()) {
        Ok(ExecutionOutcome::Returned(value)) => value,
        Ok(ExecutionOutcome::Exited(status)) => {
            panic!("bytecode built at {level:?} exited with {status}")
        }
        Err(error) => panic!("bytecode run at {level:?} failed: {error}"),
    }
}

fn assert_int_at_every_level(code: &str, expected: i64) {
    for level in LEVELS {
        assert_eq!(
            run_at(code, level).as_int(),
            Some(expected),
            "direct run at {level:?}"
        );
        assert_eq!(
            run_from_avbc(code, level).as_int(),
            Some(expected),
            "bytecode run at {level:?}"
        );
    }
}

#[test]
fn a_let_with_a_computed_initializer_shadows_the_global_it_hides() {
    let code = r#"
        let limit = 7

        fn opaque(n: int) -> int {
            if n > 100 {
                return opaque(n - 1)
            }
            return n * 2
        }

        fn probe(depth: int) -> int {
            if depth > 0 {
                return probe(depth - 1)
            }
            let limit = opaque(1)
            return limit * 10
        }

        probe(0)
    "#;
    assert_int_at_every_level(code, 20);
}

#[test]
fn a_match_arm_binding_shadows_the_global_it_hides() {
    let code = r#"
        struct Holder { v: int }

        let value = 4

        fn pick(depth: int) -> int {
            if depth > 0 {
                return pick(depth - 1)
            }
            let h = Holder { v: 1 }
            match h {
                Holder { v: value } => {
                    value * 2
                }
            }
        }

        pick(0)
    "#;
    assert_int_at_every_level(code, 2);
}

#[test]
fn a_match_arm_binding_shadows_the_local_it_hides() {
    let code = r#"
        struct Holder { v: int }

        fn pick(depth: int) -> int {
            if depth > 0 {
                return pick(depth - 1)
            }
            let value = 4
            let h = Holder { v: 1 }
            match h {
                Holder { v: value } => {
                    value * 2
                }
            }
        }

        pick(0)
    "#;
    assert_int_at_every_level(code, 2);
}

#[test]
fn a_foreach_iterator_shadows_the_global_it_hides() {
    let code = r#"
        let item = 100

        fn probe(depth: int) -> int {
            if depth > 0 {
                return probe(depth - 1)
            }
            let mut total = 0
            for item in [1, 2, 3] {
                total += item
            }
            return total
        }

        probe(0)
    "#;
    assert_int_at_every_level(code, 6);
}

#[test]
fn a_range_loop_iterator_shadows_the_global_it_hides() {
    let code = r#"
        let i = 100

        fn probe(depth: int) -> int {
            if depth > 0 {
                return probe(depth - 1)
            }
            let mut total = 0
            for i in 1..4 {
                total += i
            }
            return total
        }

        probe(0)
    "#;
    assert_int_at_every_level(code, 6);
}

#[test]
fn a_function_parameter_shadows_the_global_it_hides() {
    let code = r#"
        let factor = 42

        fn countdown(factor: int) -> int {
            if factor <= 0 {
                return 0
            }
            return 1 + countdown(factor - 1)
        }

        countdown(10)
    "#;
    assert_int_at_every_level(code, 10);
}

#[test]
fn a_lambda_parameter_shadows_the_global_it_hides() {
    let code = r#"
        let amount = 100

        fn probe(depth: int) -> int {
            if depth > 0 {
                return probe(depth - 1)
            }
            let f = fn(amount: int) -> int { return amount + 1 }
            return f(4)
        }

        probe(0)
    "#;
    assert_int_at_every_level(code, 5);
}

#[test]
fn a_shadowing_binding_stops_shadowing_when_its_scope_ends() {
    let code = r#"
        let base = 3

        fn probe(depth: int) -> int {
            if depth > 0 {
                return probe(depth - 1)
            }
            let mut seen = 0
            for base in [10, 20] {
                seen += base
            }
            return seen + base
        }

        probe(0)
    "#;
    assert_int_at_every_level(code, 33);
}

#[test]
fn a_top_level_let_rebinding_a_global_hides_the_earlier_one() {
    let code = r#"
        fn opaque(n: int) -> int {
            if n > 100 {
                return opaque(n - 1)
            }
            return n * 3
        }

        let a = 1
        let a = opaque(2)
        a
    "#;
    assert_int_at_every_level(code, 6);
}

#[test]
fn a_top_level_let_rebinding_a_constant_global_hides_the_earlier_one() {
    let code = r#"
        let a = 1
        let a = 2
        a
    "#;
    assert_int_at_every_level(code, 2);
}
