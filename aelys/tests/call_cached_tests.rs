mod common;
use aelys_bytecode::OpCode;
use aelys_driver::pipeline::compilation_pipeline_with_opt;
use aelys_opt::OptimizationLevel;
use aelys_syntax::Source;
use common::*;

#[test]
fn call_cached_works_for_local_function() {
    let code = r#"
let double = fn(x) { return x * 2 }
double(21)
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn call_cached_works_for_stdlib() {
    let code = r#"
let math_abs = math.abs
math_abs(-5)
"#;
    assert_aelys_int(code, 5);
}

#[test]
fn call_cached_works_with_multiple_args() {
    let code = r#"
let add = fn(a, b) { return a + b }
add(10, 32)
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn call_cached_chain_local() {
    let code = r#"
fn make_mult(n) {
    return fn(x) { return x * n }
}
let double = make_mult(2)
let triple = make_mult(3)
double(10) + triple(10)
"#;
    assert_aelys_int(code, 50);
}

#[test]
fn call_cached_with_closure() {
    let code = r#"
fn make_counter(start) {
    let mut count = start
    return fn() { count = count + 1; return count }
}
let c = make_counter(0)
c()
c()
c()
"#;
    assert_aelys_int(code, 3);
}

#[test]
fn call_cached_zero_argument_is_emitted() {
    let mut pipeline = compilation_pipeline_with_opt(OptimizationLevel::None);
    let source = Source::new(
        "test",
        "fn run() { let f = fn() { return 7 }\n f() }\nrun()",
    );
    let (function, _) = pipeline.compile(source).expect("compile failed");
    fn contains_call_cached(function: &aelys_runtime::Function) -> bool {
        function
            .bytecode
            .iter()
            .any(|instruction| (instruction >> 24) as u8 == OpCode::CallCached as u8)
            || function
                .nested_functions
                .iter()
                .any(contains_call_cached)
    }
    assert!(contains_call_cached(&function));
}
