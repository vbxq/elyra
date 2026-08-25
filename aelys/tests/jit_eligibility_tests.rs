use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;

const JIT_TARGET: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

const DRIVER: &str = r#"
let mut total = 0
let mut index = 0
while index < 12000 {
    total = outer(index)
    index = index + 1
}
total
"#;

const SCALAR: &str = r#"
fn outer(value: int) -> int {
    return value + 1
}
"#;

const NESTED_STRUCT: &str = r#"
struct P { x: int }
fn outer(value: int) -> int {
    fn helper(y: int) -> int {
        let p = P { x: y }
        return p.x
    }
    return value + 1
}
"#;

const NESTED_ENUM: &str = r#"
enum E { A(int), B }
fn outer(value: int) -> int {
    fn helper(y: int) -> int {
        let e = E::A(y)
        return match e { E::A(n) => n, E::B => 0 }
    }
    return value + 1
}
"#;

const OWN_ENUM: &str = r#"
enum E { A(int), B }
fn outer(value: int) -> int {
    let e = E::A(value)
    return match e { E::A(n) => n, E::B => 0 }
}
"#;

struct Outcome {
    value: i64,
    cache_entries: usize,
}

fn run_hot(prelude: &str) -> Outcome {
    let source = format!("{prelude}{DRIVER}");
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime
        .compile(&source, options)
        .expect("jit eligibility workload must compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .expect("jit eligibility workload must run");
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("jit eligibility workload must return a value, got {outcome:?}");
    };
    Outcome {
        value: value
            .as_int()
            .expect("jit eligibility workload must return an integer"),
        cache_entries: runtime.jit_cache_entries(),
    }
}

fn run_once(source: &str) -> i64 {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("workload must compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .expect("workload must load and run");
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("workload must return a value, got {outcome:?}");
    };
    value.as_int().expect("workload must return an integer")
}

#[test]
fn a_jit_eligible_scalar_function_is_still_translated() {
    let outcome = run_hot(SCALAR);
    assert_eq!(outcome.value, 12000);
    if JIT_TARGET {
        assert!(
            outcome.cache_entries > 0,
            "scalar function must still reach the jit cache, got {} entries",
            outcome.cache_entries
        );
    } else {
        assert_eq!(outcome.cache_entries, 0);
    }
}

#[test]
fn a_function_using_enum_opcodes_never_enters_the_jit_cache() {
    let outcome = run_hot(OWN_ENUM);
    assert_eq!(outcome.value, 11999);
    assert_eq!(
        outcome.cache_entries, 0,
        "an enum-using function must stay interpreted"
    );
    if JIT_TARGET {
        let control = run_hot(SCALAR);
        assert!(
            control.cache_entries > 0,
            "the harness must be able to observe a cache entry at all"
        );
    }
}

#[test]
fn a_caller_of_a_struct_using_nested_function_stays_eligible() {
    let outcome = run_hot(NESTED_STRUCT);
    assert_eq!(outcome.value, 12000);
    if JIT_TARGET {
        assert!(
            outcome.cache_entries > 0,
            "a nested struct user must not disqualify its caller, got {} entries",
            outcome.cache_entries
        );
    } else {
        assert_eq!(outcome.cache_entries, 0);
    }
}

#[test]
fn a_caller_of_an_enum_using_nested_function_stays_eligible() {
    let outcome = run_hot(NESTED_ENUM);
    assert_eq!(outcome.value, 12000);
    if JIT_TARGET {
        assert!(
            outcome.cache_entries > 0,
            "a nested enum user must not disqualify its caller, got {} entries",
            outcome.cache_entries
        );
    } else {
        assert_eq!(outcome.cache_entries, 0);
    }
}

#[test]
fn a_lambda_that_builds_a_struct_carries_its_own_eligibility_flag() {
    let value = run_once(
        "struct P { x: int }\nlet build = fn(y: int) -> int { let p = P { x: y }\n p.x }\nbuild(7)\n",
    );
    assert_eq!(value, 7);
}

#[test]
fn a_lambda_that_builds_an_enum_carries_its_own_eligibility_flag() {
    let value = run_once(
        "enum E { A(int), B }\nlet build = fn(y: int) -> int { let e = E::A(y)\n match e { E::A(n) => n, E::B => 0 } }\nbuild(5)\n",
    );
    assert_eq!(value, 5);
}

#[test]
fn an_untyped_function_that_builds_a_struct_carries_its_own_eligibility_flag() {
    let value = run_once(
        "struct P { x: int }\nfn build(y) { let p = P { x: y }\n return p.x }\nbuild(9)\n",
    );
    assert_eq!(value, 9);
}
