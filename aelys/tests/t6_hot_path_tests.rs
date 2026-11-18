mod common;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_backend::Compiler;
use aelys_bytecode::asm::disassemble;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::Value;
use aelys_sema::TypeInference;
use aelys_syntax::Source;
use common::{assert_aelys_error_contains, assert_aelys_int, run_aelys};

fn assembly_of(source: &str) -> String {
    let src = Source::new("<t6>", source);
    let tokens = Lexer::with_source(src.clone()).scan().expect("lexes");
    let ast = Parser::new_rust_collections(tokens, src.clone())
        .parse()
        .expect("parses");
    let typed = TypeInference::infer_program(ast, src.clone()).expect("types");
    let mut optimizer = Optimizer::new(OptimizationLevel::Standard);
    let optimized = optimizer.optimize(typed);
    let (function, _globals) = Compiler::new(None, src)
        .compile_typed(&optimized)
        .expect("compiles");
    disassemble(&function)
}

fn body_of(assembly: &str, name: &str) -> String {
    let marker = format!(".name \"{name}\"");
    let start = assembly
        .find(&marker)
        .unwrap_or_else(|| panic!("function {name} is in the assembly:\n{assembly}"));
    let rest = &assembly[start..];
    let end = rest.find("; !== FUN").unwrap_or(rest.len());
    rest[..end].to_string()
}

fn count(body: &str, mnemonic: &str) -> usize {
    body.lines()
        .filter(|line| {
            line.split_whitespace()
                .nth(1)
                .is_some_and(|word| word == mnemonic)
        })
        .count()
}

fn run_with_jit(source: &str, mode: JitMode) -> Value {
    let runtime = Runtime::with_jit_mode(mode);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate.execute(&module, RunOptions::default()) {
        Ok(ExecutionOutcome::Returned(value)) => value,
        other => panic!("expected a returned value, got {other:?}"),
    }
}

#[test]
fn a_discarded_local_assignment_writes_the_variable_without_a_copy() {
    let body = body_of(
        &assembly_of(
            r#"
fn advance(n: int) -> int {
    let mut acc = 0
    let mut x = 12345
    for i in 0..n {
        acc = (acc + i) ^ x
        x = x + acc
    }
    return acc ^ x
}
advance(3)
"#,
        ),
        "advance",
    );
    assert_eq!(
        count(&body, "Move"),
        1,
        "only the loop bound copy remains:\n{body}"
    );
}

#[test]
fn a_discarded_element_store_leaves_no_copy_of_the_stored_value() {
    let body = body_of(
        &assembly_of(
            r#"
fn fill(mut v: Vec<int>, n: int) {
    for i in 0..n {
        v[i] = i * 3
    }
}
let mut v: Vec<int> = vec![0; 4]
fill(v, 4)
"#,
        ),
        "fill",
    );
    assert_eq!(
        count(&body, "Move"),
        1,
        "only the loop bound copy remains:\n{body}"
    );
}

#[test]
fn a_small_literal_shift_or_mask_is_an_immediate_operand() {
    let body = body_of(
        &assembly_of(
            r#"
fn mix(x: int, y: int) -> int {
    return ((x + y) >> 3) & 127 | 64 ^ 5
}
mix(1, 2)
"#,
        ),
        "mix",
    );
    assert_eq!(count(&body, "ShrIImm"), 1, "{body}");
    assert_eq!(count(&body, "AndIImm"), 1, "{body}");
    assert_eq!(
        count(&body, "LoadI"),
        0,
        "no literal needs its own register:\n{body}"
    );
}

#[test]
fn a_wide_literal_operand_is_loaded_once_before_its_loop() {
    let source = r#"
fn mask(n: int) -> int {
    let mut acc = 0
    let mut x = 12345
    for i in 0..n {
        x = (x * 1000 + i) & 268435455
        acc = (acc + x) & 268435455
    }
    return acc
}
mask(5000)
"#;
    let body = body_of(&assembly_of(source), "mask");
    assert_eq!(count(&body, "LoadK"), 1, "{body}");
    assert_eq!(
        count(&body, "LoadI"),
        5,
        "1000 joins the loop set-up outside the loop:\n{body}"
    );
    assert_aelys_int(source, {
        let (mut acc, mut x) = (0i64, 12345i64);
        for i in 0..5000 {
            x = (x * 1000 + i) & 268435455;
            acc = (acc + x) & 268435455;
        }
        acc
    });
}

#[test]
fn retargeted_assignments_compute_the_same_values() {
    assert_aelys_int(
        r#"
fn twice(v: int) -> int { return v * 2 }
fn run() -> int {
    let mut x = 7
    let mut y = 3
    let v = vec![5, 6, 7]
    x = (y * 2) + x
    y = x - y
    x = x + twice(x)
    y = -y
    x = v[2] + x
    let t = x
    x = y
    y = t
    x = (if y > 0 { y } else { 1 }) + x
    return x * 1000 + y
}
run()
"#,
        36 * 1000 + 46,
    );
}

#[test]
fn integer_overflow_in_a_hot_loop_still_traps() {
    assert_aelys_error_contains(
        r#"
fn grow() -> int {
    let mut x = 1
    for i in 0..100 { x = x * 3 }
    return x
}
grow()
"#,
        "overflow",
    );
    assert_aelys_error_contains(
        r#"
fn grow() -> int {
    let mut x = 140737488355000
    for i in 0..1000 { x = x + 1 }
    return x
}
grow()
"#,
        "overflow",
    );
}

#[test]
fn division_by_zero_in_a_hot_loop_still_traps() {
    assert_aelys_error_contains(
        r#"
fn divide(n: int) -> int {
    let mut acc = 0
    for i in 0..n { acc = acc + 100 / (3 - i) }
    return acc
}
divide(10)
"#,
        "division by zero",
    );
}

#[test]
fn shifts_beyond_the_fast_range_keep_their_values() {
    assert_aelys_int(
        r#"
fn shifts(a: int) -> int {
    let mut s = 0
    for k in 40..56 { s = s + (a >> k) }
    return s
}
shifts(0 - 1099511627776)
"#,
        -16,
    );
}

#[test]
fn an_out_of_bounds_index_in_a_hot_loop_still_reports_it() {
    assert_aelys_error_contains(
        r#"
fn walk(v: Vec<int>) -> int {
    let mut s = 0
    for i in 0..10 { s = s + v[i] }
    return s
}
walk(vec![1, 2, 3])
"#,
        "out of bounds",
    );
}

#[test]
fn float_vectors_round_trip_through_the_hot_loop() {
    let value = run_aelys(
        r#"
fn run() -> float {
    let mut v: Vec<float> = vec![0.0; 8]
    for i in 0..8 { v[i] = 0.5 * 3.0 }
    let mut s = 0.0
    for i in 0..8 { s = s + v[i] }
    return s
}
run()
"#,
    );
    assert_eq!(value.as_float(), Some(12.0));
}

#[test]
fn deep_recursion_through_the_hot_call_path_still_overflows_the_stack() {
    assert_aelys_error_contains(
        r#"
fn down(n: int) -> int { return down(n + 1) + 1 }
down(0)
"#,
        "stack overflow",
    );
}

#[test]
fn recursive_calls_return_their_values_to_the_right_frame() {
    assert_aelys_int(
        r#"
fn fib(n: int) -> int {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}
fn f3(x: int) -> int { return (x & 1023) + 1 }
fn f2(x: int) -> int { return f3(x) ^ 3 }
fn f1(x: int) -> int { return f2(x) + 5 }
fn chain(n: int) -> int {
    let mut acc = 0
    for i in 0..n { acc = (acc + f1(i)) & 268435455 }
    return acc
}
fib(20) * 1000000 + chain(3000)
"#,
        6765 * 1_000_000
            + (0..3000i64).fold(0, |acc, i| (acc + ((((i & 1023) + 1) ^ 3) + 5)) & 268435455),
    );
}

#[test]
fn a_global_written_under_one_layout_is_read_under_another() {
    assert_aelys_int(
        r#"
let mut counter = 0
let mut scale = 2
fn bump(k: int) { counter = counter + k * scale }
fn read() -> int { return counter }
fn rescale(s: int) { scale = s }
fn run() -> int {
    let mut seen = 0
    for i in 0..50 {
        bump(i)
        seen = seen + read()
        if i == 25 { rescale(3) }
    }
    return seen * 10 + scale
}
run()
"#,
        {
            let (mut counter, mut scale, mut seen) = (0i64, 2i64, 0i64);
            for i in 0..50 {
                counter += i * scale;
                seen += counter;
                if i == 25 {
                    scale = 3;
                }
            }
            seen * 10 + scale
        },
    );
}

#[test]
fn a_global_written_inside_a_callee_loop_is_visible_to_the_caller() {
    assert_aelys_int(
        r#"
let mut total = 0
fn add_all(n: int) {
    for i in 0..n { total = total + i }
}
fn run() -> int {
    add_all(100)
    let first = total
    add_all(10)
    return first * 1000 + total
}
run()
"#,
        4950 * 1000 + 4995,
    );
}

#[test]
fn pushes_grow_a_vector_without_losing_elements() {
    assert_aelys_int(
        r#"
fn run() -> int {
    let mut v: Vec<int> = vec![]
    for i in 0..100000 { v.push(i & 255) }
    let mut s = 0
    for i in 0..v.len() { s = s + v[i] }
    return s * 10 + v.len() % 7
}
run()
"#,
        (0..100000i64).map(|i| i & 255).sum::<i64>() * 10 + 100000 % 7,
    );
}

#[test]
fn objects_allocated_under_collection_pressure_keep_their_fields() {
    assert_aelys_int(
        r#"
struct Node { label: string, next: Vec<int>, weight: int }
fn run() -> int {
    let mut nodes: Vec<Node> = vec![]
    for i in 0..20000 {
        let parts: Vec<int> = vec![i, i + 1]
        nodes.push(Node { label: "n", next: parts, weight: i })
    }
    let mut s = 0
    for i in 0..nodes.len() {
        let n = nodes[i]
        s = s + n.next[1] + n.weight + n.label.len()
    }
    return s
}
run()
"#,
        (0..20000i64).map(|i| (i + 1) + i + 1).sum(),
    );
}

#[test]
fn the_jit_and_the_interpreter_agree_on_the_hot_benches() {
    let programs = [
        r#"
fn run(n: int) -> int {
    let mut acc = 0
    let mut x = 12345
    for i in 0..n {
        acc = (acc + i) ^ (x >> 3)
        x = (x + acc + 7) & 268435455
        acc = acc & 268435455
    }
    return acc ^ x
}
run(30000)
"#,
        r#"
struct Sq { s: int }
trait Shape { fn area(self) -> int; }
impl Shape for Sq { fn area(self) -> int { return self.s * self.s } }
fn total<T: Shape>(x: T) -> int { return x.area() }
fn run(n: int) -> int {
    let mut sq = Sq { s: 1 }
    let mut acc = 0
    for i in 0..n {
        sq.s = i & 255
        acc = (acc + total(sq)) & 268435455
    }
    return acc
}
run(30000)
"#,
        r#"
fn compute(limit: int) -> int {
    let mut i = 0
    let mut sum = 0
    while i < limit {
        sum = sum + i
        i = i + 1
    }
    return sum
}
compute(30000)
"#,
    ];
    for program in programs {
        let off = run_with_jit(program, JitMode::Off);
        let tiered = run_with_jit(program, JitMode::Tiered);
        assert_eq!(off.as_int(), tiered.as_int(), "{program}");
        assert!(off.as_int().is_some(), "{program}");
    }
}
