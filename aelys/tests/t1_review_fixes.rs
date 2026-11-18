use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;
use aelys_runtime::Value;

fn assert_int_everywhere(source: &str, expected: i64) {
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        for mode in [JitMode::Off, JitMode::Tiered] {
            let runtime = Runtime::with_jit_mode(mode);
            let module = runtime
                .compile(
                    source,
                    CompileOptions {
                        optimization_level: level,
                        source_name: "<t1>".to_string(),
                    },
                )
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}"));
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            let outcome = isolate
                .execute(&module, RunOptions::default())
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}"));
            assert_eq!(
                outcome,
                ExecutionOutcome::Returned(Value::int(expected)),
                "{level:?} {mode:?}\n{source}"
            );
        }
    }
}

fn rejected(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn assert_rejected(source: &str, code: &str) -> String {
    let message = rejected(source);
    assert!(message.contains(code), "expected {code}:\n{message}");
    assert!(
        !message.contains("dynamic"),
        "an internal type name leaked:\n{message}"
    );
    message
}

fn assert_accepted(source: &str) {
    if let Err(error) = Runtime::new().compile(source, CompileOptions::default()) {
        panic!("the source must be accepted:\n{source}\n{error}");
    }
}

const COUNTING: &str = r#"
struct P { x: int, h: int }
struct W { ps: Vec<P> }
let mut log = vec![0]
fn at(i: int) -> int { log[0] = log[0] * 10 + 1; i }
fn rhs(v: int) -> int { log[0] = log[0] * 10 + 2; v }
"#;

fn counting(body: &str) -> String {
    format!("{COUNTING}\n{body}")
}

#[test]
fn compound_assignment_on_a_vec_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut v = vec![100, 50]
v[at(1)] += rhs(7)
v[0] * 1000 + v[1] + log[0] * 100000
"#,
        ),
        100 * 1000 + 57 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_on_an_array_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut a = [100, 50]
a[at(0)] -= rhs(10)
a[0] * 1000 + a[1] + log[0] * 100000
"#,
        ),
        90 * 1000 + 50 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_on_a_field_of_an_indexed_struct_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut v = vec![P { x: 100, h: 0 }, P { x: 50, h: 0 }]
v[at(1)].x -= rhs(10)
v[0].x * 1000 + v[1].x + log[0] * 100000
"#,
        ),
        100 * 1000 + 40 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_through_a_field_path_and_an_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut w = W { ps: vec![P { x: 0, h: 3 }, P { x: 0, h: 5 }] }
w.ps[at(0)].h *= rhs(2)
w.ps[0].h * 100 + w.ps[1].h + log[0] * 10000
"#,
        ),
        6 * 100 + 5 + 12 * 10000,
    );
}

#[test]
fn compound_assignment_on_nested_indices_evaluates_left_to_right_once() {
    assert_int_everywhere(
        &counting(
            r#"
fn second(i: int) -> int { log[0] = log[0] * 10 + 3; i }
let mut g = [[1, 2], [3, 4]]
g[at(1)][second(0)] += rhs(100)
let mut gv = vec![vec![1, 2], vec![3, 4]]
gv[at(0)][second(1)] *= 3
g[1][0] * 1000 + gv[0][1] + log[0] * 100000
"#,
        ),
        103 * 1000 + 6 + 13213 * 100000,
    );
}

#[test]
fn compound_assignment_with_an_rng_index_hits_one_slot() {
    assert_int_everywhere(
        r#"
struct M { hp: int }
let mut seed = vec![0]
fn rnd(mut s: Vec<int>) -> int { s[0] += 1; s[0] % 2 }
let mut ms = vec![M { hp: 100 }, M { hp: 50 }]
ms[rnd(seed)].hp -= 10
ms[0].hp * 1000 + ms[1].hp
"#,
        100 * 1000 + 40,
    );
}

#[test]
fn compound_assignment_evaluates_the_index_once_in_methods_closures_and_generics() {
    assert_int_everywhere(
        &counting(
            r#"
struct Bag { items: Vec<P> }
impl Bag {
    fn hit(mut self, d: int) { self.items[at(1)].h += d }
}
fn bump<T>(mut xs: Vec<int>, tag: T) { xs[at(0)] += 1 }
let mut b = Bag { items: vec![P { x: 0, h: 0 }, P { x: 0, h: 0 }] }
b.hit(4)
let mut xs = vec![10, 20]
bump(xs, true)
let mut ys = vec![1, 2]
let f = fn() { ys[at(1)] -= 1 }
f()
b.items[1].h * 1000 + xs[0] * 10 + ys[1] + log[0] * 1000000
"#,
        ),
        4 * 1000 + 11 * 10 + 1 + 111 * 1000000,
    );
}

#[test]
fn compound_assignment_with_a_pure_index_still_works() {
    assert_int_everywhere(
        r#"
let mut v = vec![1, 2, 3]
let i = 1
v[i + 1] += 10
v[i] *= 4
v[i - 1] %= 1
v[0] * 10000 + v[1] * 100 + v[2]
"#,
        8 * 100 + 13,
    );
}
