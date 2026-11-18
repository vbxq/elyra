mod common;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_runtime::Value;
use common::assert_aelys_int;

fn execute(
    source: &str,
    mode: JitMode,
    options: RunOptions,
    config: IsolateConfig,
) -> Result<Value, String> {
    let runtime = Runtime::with_jit_mode(mode);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("compiles");
    let mut isolate = runtime.new_isolate(config);
    match isolate.execute(&module, options) {
        Ok(ExecutionOutcome::Returned(value)) => Ok(value),
        Ok(other) => panic!("expected a returned value, got {other:?}"),
        Err(error) => Err(error.to_string()),
    }
}

fn error_line(message: &str) -> Option<u32> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse().ok())
}

fn old_loop() -> RunOptions {
    RunOptions {
        max_instructions: Some(1_000_000_000_000),
        ..RunOptions::default()
    }
}

fn assert_error_line_in_every_mode(source: &str, message: &str, line: u32) {
    let modes = [
        ("hot loop", JitMode::Off, RunOptions::default()),
        ("general loop", JitMode::Off, old_loop()),
        ("jit", JitMode::Tiered, RunOptions::default()),
        ("jit, general loop", JitMode::Tiered, old_loop()),
    ];
    for (name, mode, options) in modes {
        let error = execute(source, mode, options, IsolateConfig::default())
            .expect_err("the program fails at run time");
        assert!(error.contains(message), "{name}: {error}");
        assert_eq!(error_line(&error), Some(line), "{name}: {error}");
    }
}

fn assert_int_in_every_mode(source: &str, expected: i64) {
    let modes = [
        ("hot loop", JitMode::Off, RunOptions::default()),
        ("general loop", JitMode::Off, old_loop()),
        ("jit", JitMode::Tiered, RunOptions::default()),
    ];
    for (name, mode, options) in modes {
        let value = execute(source, mode, options, IsolateConfig::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{name}");
    }
}

const OVERFLOW: &str = "integer overflow";

#[test]
fn a_failing_cast_after_a_loop_reports_its_own_line() {
    assert_error_line_in_every_mode(
        r#"
let mut G = 0
fn bump(x: int) -> int {
    G = G + x
    return G
}
fn main_body() -> int {
    let f = 1e19
    let a = bump(1)
    let mut w = 0
    while w < 3 {
        w = w + 1
        G = G + w
    }
    let b = f as int
    return a + b
}
main_body()
"#,
        OVERFLOW,
        15,
    );
}

#[test]
fn a_failing_cast_inside_a_closure_reports_its_own_line() {
    assert_error_line_in_every_mode(
        r#"
fn run() -> int {
    let mut k = 0
    let scale = 2e18
    let f = fn(x: int) -> int {
        k = k + x
        return (scale * (k as float)) as int
    }
    let mut s = 0
    for i in 0..10 { s = s + f(i) }
    return s
}
run()
"#,
        OVERFLOW,
        7,
    );
}

#[test]
fn a_failing_cast_in_a_method_called_from_a_loop_reports_its_own_line() {
    assert_error_line_in_every_mode(
        r#"
struct Acc { total: int }
impl Acc {
    fn add(mut self, x: int) -> int {
        self.total = self.total + x
        let f = 4e18 * (self.total as float)
        return f as int
    }
}
fn run() -> int {
    let mut a = Acc { total: 0 }
    let mut s = 0
    for i in 0..10 { s = s + a.add(i) }
    return s
}
run()
"#,
        OVERFLOW,
        7,
    );
}

#[test]
fn an_unwrapped_none_reports_its_own_line() {
    assert_error_line_in_every_mode(
        r#"
fn pick(i: int) -> Option<int> {
    if i < 5 { return Some(i) }
    return None
}
fn run() -> int {
    let mut s = 0
    for i in 0..10 {
        s = s + i
        let o = pick(i)
        s = s + o.unwrap()
    }
    return s
}
run()
"#,
        "Option",
        11,
    );
}

#[test]
fn closures_read_and_write_open_and_closed_captures() {
    assert_int_in_every_mode(
        r#"
fn make() -> fn(int) -> int {
    let mut hidden = 10
    let mut label = "a"
    return fn(x: int) -> int {
        hidden = hidden + x
        label = "{label}b"
        return hidden + label.len()
    }
}
fn run(n: int) -> int {
    let mut k = 1
    let add = fn(a: int) -> int {
        k = k + 1
        return a + k
    }
    let later = make()
    let mut s = 0
    for i in 0..n {
        s = add(s) & 0xFFFFFF
        s = s + later(i)
        k = k + 1
    }
    return s + k
}
run(1000)
"#,
        17_186_057,
    );
}

#[test]
fn function_values_and_closures_called_from_a_loop_return_to_the_right_frame() {
    assert_int_in_every_mode(
        r#"
fn twice(x: int) -> int { return x * 2 }
fn run(n: int) -> int {
    let f = twice
    let mut base = 3
    let g = fn(x: int) -> int { return f(x) + base }
    let mut s = 0
    for i in 0..n {
        s = (s + g(i) + f(i)) & 0xFFFFF
        base = base + 1
    }
    return s
}
run(5000)
"#,
        636_516,
    );
}

#[test]
fn options_matched_in_a_loop_keep_their_payloads() {
    assert_int_in_every_mode(
        r#"
fn half(i: int) -> Option<int> {
    if i % 3 == 0 { return None }
    return Some(i / 2)
}
fn run(n: int) -> int {
    let mut s = 0
    let mut misses = 0
    for i in 0..n {
        let o = half(i)
        s = s + match o { Some(v) => v, None => 0 }
        misses = misses + match o { None => 1, Some(_) => 0 }
        let p: Option<int> = Some(i)
        s = s + match p { Some(v) => v & 1, None => 1000 }
    }
    return s * 10000 + misses
}
run(3000)
"#,
        15_010_001_000,
    );
}

#[test]
fn a_pushed_vector_starts_at_one_slot_and_doubles() {
    assert_aelys_int(
        r#"
let mut v: Vec<int> = vec![]
let mut code = 0
for i in 0..9 {
    v.push(i)
    code = code * 32 + v.capacity()
}
code
"#,
        1_172_668_948_752,
    );
}

#[test]
fn many_one_element_vectors_fit_a_small_heap() {
    let source = r#"
let mut outer: Vec<Vec<int>> = vec![]
for i in 0..100000 {
    let mut v: Vec<int> = vec![]
    v.push(i)
    outer.push(v)
}
let mut s = 0
for v in outer { s = s + v[0] + v.capacity() }
s
"#;
    let config = IsolateConfig::default()
        .with_max_heap_bytes(6 * 1024 * 1024)
        .expect("the budget is legal");
    let value = execute(source, JitMode::Off, RunOptions::default(), config)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value.as_int(), Some(4_999_950_000 + 100_000));
}

#[test]
fn a_loop_entered_with_a_dead_bool_register_is_replaced_on_the_stack() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(
            r#"
fn compute(limit: int) -> int {
    let mut total = 0
    if limit > 5 { total = 1 }
    let mut index = 0
    while index < limit {
        total = total + index * 2
        index = index + 1
    }
    return total
}
compute(100000) + 7
"#,
            CompileOptions::default(),
        )
        .expect("compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(9_999_900_008))
    );
    assert_eq!(runtime.jit_cache_entries(), 1);
    assert_eq!(runtime.jit_osr_executions(), 1);
}
