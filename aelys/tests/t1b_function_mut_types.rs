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
                        source_name: "<t1b>".to_string(),
                    },
                )
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}\n{source}"));
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            let outcome = isolate
                .execute(&module, RunOptions::default())
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}\n{source}"));
            assert_eq!(
                outcome,
                ExecutionOutcome::Returned(Value::int(expected)),
                "{level:?} {mode:?}\n{source}"
            );
        }
    }
}

fn assert_rejected(source: &str, code: &str) -> String {
    let message = match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains(code),
        "expected {code}:\n{source}\n{message}"
    );
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

const SHAPES: &str = r#"
struct S { v: int }
struct W { items: Vec<S>, s: S }
fn touch(mut s: S) -> int { s.v = s.v + 10; s.v }
fn read_only(s: S) -> int { s.v }
"#;

fn program(body: &str) -> String {
    format!("{SHAPES}\n{body}")
}

#[test]
fn an_unannotated_function_parameter_called_with_an_immutable_parameter_runs() {
    assert_int_everywhere(
        "struct Counter { v: int }
impl Counter { fn peek(self) -> int { return self.v } }
fn getter(c: Counter) -> int { return c.peek() }
fn apply(f, c: Counter) -> int { return f(c) }
apply(getter, Counter { v: 7 })",
        7,
    );
    assert_int_everywhere(
        &program(
            "fn apply(f, s: S) -> int { return f(s) }
let a = S { v: 4 }
apply(read_only, a) + apply(fn(x: S) -> int { return x.v * 10 }, a)",
        ),
        44,
    );
}

#[test]
fn a_function_value_bound_to_a_mut_function_keeps_its_mut_parameter() {
    let cases = [
        "let a = S { v: 1 }\nlet h = touch\nh(a)",
        "let a = S { v: 1 }\nlet h = touch\nlet h2 = h\nh2(a)",
        "let a = S { v: 1 }\nlet mut h = touch\nh(a)",
        "let a = S { v: 1 }\nlet read_only = touch\nread_only(a)",
        "let h = touch\nfn run(a: S) -> int { h(a) }",
        "let a = S { v: 1 }\nlet h = touch\nlet c = fn() -> int { h(a) }",
        "let a = S { v: 1 }\nlet g = fn(mut q: S) -> int { q.v = 99; 0 }\ng(a)",
        "let w = W { items: vec![S { v: 1 }], s: S { v: 2 } }\nlet h = touch\nh(w.items[0])",
        "let w = W { items: vec![S { v: 1 }], s: S { v: 2 } }\nlet h = touch\nh(w.s)",
    ];
    for body in cases {
        let message = assert_rejected(&program(body), "E0323");
        assert!(
            message.contains("`mut` parameter 1 of a function value of type `fn(mut S) -> int`"),
            "{body}:\n{message}"
        );
    }
}

#[test]
fn a_call_through_a_mut_function_type_checks_only_its_mut_positions() {
    let rejected = [
        (
            "fn apply(f: fn(mut S) -> int, s: S) -> int { f(s) }",
            "`s`",
            "`fn(mut S) -> int`",
        ),
        (
            "fn apply(f: fn(mut S) -> int, s: S) -> int { f(s) }\nlet mut a = S { v: 1 }\napply(touch, a)",
            "`s`",
            "`fn(mut S) -> int`",
        ),
        (
            "fn apply(f: fn(int, mut S) -> int, s: S) -> int { f(1, s) }",
            "parameter 2",
            "`fn(int, mut S) -> int`",
        ),
        (
            "fn go(w: W, f: fn(mut S) -> int) -> int { f(w.items[0]) }",
            "`w.items[0]`",
            "immutable binding `w`",
        ),
        (
            "fn go<T>(f: fn(mut S) -> T, s: S) -> T { f(s) }",
            "`s`",
            "`fn(mut S) -> T`",
        ),
        (
            "impl S { fn run(self, f: fn(mut S) -> int) -> int { f(self) } }",
            "`self`",
            "`fn(mut S) -> int`",
        ),
        (
            "let a = S { v: 1 }\nlet g = fn(f: fn(mut S) -> int) -> int { f(a) }",
            "`a`",
            "`fn(mut S) -> int`",
        ),
    ];
    for (body, place, detail) in rejected {
        let message = assert_rejected(&program(body), "E0323");
        assert!(
            message.contains(place) && message.contains(detail),
            "{body}:\n{message}"
        );
    }
    let accepted = [
        "fn apply(f: fn(int, mut S) -> int, mut s: S) -> int { f(1, s) }",
        "fn apply(f: fn(mut S, S) -> int, mut a: S, b: S) -> int { f(a, b) }",
        "fn apply(f: fn(S) -> int, s: S) -> int { f(s) }",
        "fn apply(f: fn(mut S) -> int, s: S) -> int { f(s.clone()) }",
        "fn go(mut w: W, f: fn(mut S) -> int) -> int { f(w.items[0]) }",
    ];
    for body in accepted {
        assert_accepted(&program(body));
    }
}

#[test]
fn a_plain_function_value_accepts_an_immutable_root() {
    assert_int_everywhere(
        &program(
            "fn apply(f: fn(S) -> int, s: S) -> int { return f(s) }
let a = S { v: 3 }
let h = read_only
apply(read_only, a) * 100 + h(a) * 10 + a.v",
        ),
        333,
    );
}

#[test]
fn an_unannotated_function_parameter_adopts_the_mut_parameter_it_is_passed() {
    let message = assert_rejected(
        &program(
            "fn apply(f, s: S) -> int { return f(s) }
let a = S { v: 1 }
apply(touch, a)",
        ),
        "E0323",
    );
    assert!(
        message.contains("cannot pass `s` to the `mut` parameter 1")
            && message.contains("`fn(mut S) -> int`"),
        "{message}"
    );
    assert_int_everywhere(
        &program(
            "fn apply(f, mut s: S) -> int { return f(s) }
let mut a = S { v: 1 }
let r = apply(touch, a)
r * 100 + a.v",
        ),
        1111,
    );
}

#[test]
fn mut_lambdas_stored_in_fields_and_vecs_mutate_a_mutable_root() {
    assert_int_everywhere(
        &program(
            "struct Holder { f: fn(mut S) -> int }
let mut a = S { v: 1 }
let t = Holder { f: fn(mut s: S) -> int { s.v = s.v * 3; s.v } }
let g = t.f
let x = g(a)
let y = t.f(a)
let fs = vec![touch, fn(mut s: S) -> int { s.v = s.v + 100; s.v }]
let z = fs[1](a)
let k = fs[0]
let w = k(a)
x * 1000000 + y * 10000 + w + z * 0 + a.v * 0",
        ),
        3 * 1_000_000 + 9 * 10_000 + 119,
    );
    let rejected = [
        "struct Holder { f: fn(mut S) -> int }\nlet a = S { v: 1 }\nlet t = Holder { f: touch }\nt.f(a)",
        "struct Holder { f: fn(mut S) -> int }\nlet a = S { v: 1 }\nlet t = Holder { f: touch }\n(t.f)(a)",
        "struct Holder { f: fn(mut S) -> int }\nlet a = S { v: 1 }\nlet t = Holder { f: touch }\nlet g = t.f\ng(a)",
        "let a = S { v: 1 }\nlet fs = vec![touch]\nfs[0](a)",
        "let a = S { v: 1 }\nlet fs = [touch, touch]\nlet k = fs[1]\nk(a)",
        "let a = vec![S { v: 1 }]\nfn pick() -> fn(mut S) -> int { touch }\npick()(a[0])",
    ];
    for body in rejected {
        let message = assert_rejected(&program(body), "E0323");
        assert!(message.contains("`fn(mut S) -> int`"), "{body}:\n{message}");
    }
}

#[test]
fn a_generic_higher_order_function_maps_every_element() {
    assert_int_everywhere(
        &program(
            "fn map_all<T>(v: Vec<T>, f: fn(T) -> T) -> Vec<T> {
    let mut out = vec![]
    for x in v { out.push(f(x)) }
    return out
}
let r = map_all(vec![1, 2, 3], fn(x: int) -> int { return x * 2 })
let src = vec![S { v: 1 }, S { v: 2 }]
let q = map_all(src, fn(s: S) -> S { return S { v: s.v + 5 } })
r[0] + r[1] + r[2] + q[0].v * 100 + q[1].v * 1000 + src[0].v * 10000",
        ),
        2 + 4 + 6 + 600 + 7000 + 10000,
    );
    let message = assert_rejected(
        &program(
            "fn map_all<T>(v: Vec<T>, f: fn(T) -> T) -> Vec<T> {
    let mut out = vec![]
    for x in v { out.push(f(x)) }
    return out
}
let q = map_all(vec![S { v: 1 }], fn(mut s: S) -> S { s.v = 0; s })",
        ),
        "E0301",
    );
    assert!(message.contains("fn(mut S) -> S"), "{message}");
}

#[test]
fn a_mut_mismatch_between_function_types_is_a_named_diagnostic() {
    let cases = [
        (
            "fn apply(f: fn(S) -> int, s: S) -> int { f(s) }\nlet mut a = S { v: 1 }\napply(touch, a)",
            "expected fn(S) -> int, found fn(mut S) -> int",
        ),
        (
            "let h: fn(mut S) -> int = read_only",
            "expected fn(mut S) -> int, found fn(S) -> int",
        ),
        (
            "struct Holder { f: fn(S) -> int }\nlet t = Holder { f: touch }",
            "fn(mut S) -> int",
        ),
        ("let mut h = read_only\nh = touch", "fn(mut S) -> int"),
        ("fn pick() -> fn(S) -> int { touch }", "fn(mut S) -> int"),
        (
            "fn apply(f, mut s: S) -> int { f(s) }\nlet mut a = S { v: 1 }\nlet x = apply(read_only, a)\nlet y = apply(touch, a)",
            "fn(mut S) -> int",
        ),
        ("let fs = vec![read_only, touch]", "fn(mut S) -> int"),
    ];
    for (body, detail) in cases {
        let message = assert_rejected(&program(body), "E0301");
        assert!(message.contains(detail), "{body}:\n{message}");
        assert!(
            message.contains("a `mut` parameter is part of a function's type"),
            "{body}:\n{message}"
        );
    }
}

#[test]
fn mut_on_a_scalar_parameter_is_not_part_of_the_function_type() {
    assert_int_everywhere(
        "fn dbl(mut x: int) -> int { x = x * 2; x }
fn inc(x: int) -> int { x + 1 }
fn apply(f, x: int) -> int { f(x) }
let h: fn(int) -> int = dbl
let k: fn(mut int) -> int = inc
let n = 5
apply(inc, 1) + apply(dbl, 3) + h(n) + k(0) + n * 100",
        2 + 6 + 10 + 1 + 500,
    );
}
