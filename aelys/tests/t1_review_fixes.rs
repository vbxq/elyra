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

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_at_top_level() {
    assert_int_everywhere(
        r#"
let top = sqrt(4)
fn sqrt(t: int) -> int { t + 100 }
top
"#,
        104,
    );
}

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_in_every_position() {
    assert_int_everywhere(
        r#"
fn second(a: int, b: int) -> int { b }
fn early() -> int { second(0, sqrt(4)) + sqrt(4) }
fn use_min() -> int { min(5) }
let top = second(0, sqrt(4)) + sqrt(4)
let first = early()
let m = use_min()
fn sqrt(t: int) -> int { t + 100 }
fn min(t: int) -> int { t + 200 }
let f = sqrt
top * 1000000 + first * 1000 + f(1) + m * 1000000000
"#,
        205 * 1000000000 + 208 * 1000000 + 208 * 1000 + 101,
    );
}

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_in_a_method() {
    assert_int_everywhere(
        r#"
struct H { v: int }
impl H { fn go(self) -> int { len(self.v) } }
let r = H { v: 3 }.go()
fn len(x: int) -> int { x * 1000 }
r
"#,
        3000,
    );
}

#[test]
fn a_function_called_before_its_declaration_runs() {
    assert_int_everywhere(
        r#"
let a = later(1)
fn later(t: int) -> int { t + helper() }
fn helper() -> int { 40 }
a
"#,
        41,
    );
}

const ALIAS_SHAPES: &str = r#"
struct In { c: int }
struct S { x: int, inner: In, items: Vec<int> }
fn mk() -> S { S { x: 0, inner: In { c: 0 }, items: vec![] } }
"#;

fn alias_program(body: &str) -> String {
    format!("{ALIAS_SHAPES}\n{body}")
}

#[test]
fn assigning_an_immutable_aggregate_to_a_mutable_binding_is_rejected_everywhere() {
    let cases = [
        "let a = mk()\nlet mut b = mk()\nb = a\nb.x = 99",
        "fn f(a: S) -> int { let mut b = mk(); b = a; b.x = 99; a.x }",
        "let a = mk()\nlet mut g = mk()\nfn run() { g = a; g.x = 99 }",
        "let v = vec![1, 2]\nlet mut w: Vec<int> = vec![]\nw = v\nw.push(3)",
        "let a = mk()\nlet mut i = In { c: 1 }\ni = a.inner",
        "let a = mk()\nlet mut xs = vec![0]\nxs = a.items",
        "let a = [[1, 2], [3, 4]]\nlet mut row = [0, 0]\nrow = a[1]",
        "let a = mk()\nlet mut b = mk()\nlet c = true\nb = if c { a } else { mk() }",
        "let a = mk()\nlet mut b = mk()\nb = match a.x { 0 => a, _ => mk() }",
        "impl S { fn grab(self) -> int { let mut xs = vec![0]; xs = self.items; xs.len() } }",
        "let a = mk()\nlet mut b = mk()\nlet f = fn() { b = a }",
        "fn g<T>(a: Vec<T>) { let mut w: Vec<T> = vec![]; w = a }",
    ];
    for body in cases {
        let message = assert_rejected(&alias_program(body), "E0323");
        assert!(
            message.contains("to the mutable binding"),
            "the message must name the assignment:\n{message}"
        );
    }
}

#[test]
fn assignments_that_do_not_alias_an_immutable_root_are_accepted() {
    let cases = [
        "let a = mk()\nlet mut b = mk()\nb = a.clone()\nb.x = 99",
        "let mut a = mk()\nlet mut b = mk()\nb = a\nb.x = 99",
        "let a = mk()\nlet mut n = 0\nn = a.x",
        "let a = In { c: 1 }\nlet mut s = mk()\ns.inner = a",
        "let a = vec![1]\nlet mut ws = vec![vec![0]]\nws[0] = a",
        "fn f(mut a: S) { let mut b = mk(); b = a; b.x = 1 }",
        "let mut b = mk()\nb = mk()",
    ];
    for body in cases {
        assert_accepted(&alias_program(body));
    }
}

#[test]
fn assignment_through_a_mutable_binding_shares_with_a_mutable_source() {
    assert_int_everywhere(
        &alias_program(
            r#"
let mut a = mk()
let mut b = mk()
b = a
b.x = 7
let c = mk()
let mut d = mk()
d = c.clone()
d.x = 9
a.x * 100 + c.x * 10 + d.x
"#,
        ),
        7 * 100 + 9,
    );
}
