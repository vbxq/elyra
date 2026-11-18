use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "t2_else_if.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn float_at_every_level(source: &str, expected: f64) {
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "t2_else_if.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_float(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(
        &mut vm,
        source,
        "t2_else_if.aelys",
        OptimizationLevel::Standard,
    ) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn else_if_chain_as_function_tail_returns_the_taken_branch() {
    int_at_every_level(
        r#"
        fn f(n: int) -> int { if n == 0 { 10 } else if n < 0 { 20 } else { 30 } }
        f(-1) * 10000 + f(0) * 100 + f(5)
        "#,
        200_000 + 1_000 + 30,
    );
}

#[test]
fn else_if_chain_of_depth_four_in_a_function_tail() {
    int_at_every_level(
        r#"
        fn grade(n: int) -> int {
            if n >= 90 { 4 } else if n >= 80 { 3 } else if n >= 70 { 2 } else if n >= 60 { 1 } else { 0 }
        }
        grade(95) * 10000 + grade(85) * 1000 + grade(75) * 100 + grade(65) * 10 + grade(10)
        "#,
        43210,
    );
}

#[test]
fn multi_line_branches_keep_their_value() {
    int_at_every_level(
        "fn f(c: bool) -> int {\n    if c {\n        5\n    } else if not c {\n        6\n    } else {\n        7\n    }\n}\nlet x = if f(true) == 5 {\n    1\n} else {\n    2\n}\nx * 100 + f(false)\n",
        106,
    );
}

#[test]
fn branch_statements_run_before_the_branch_value() {
    int_at_every_level(
        r#"
        let mut log = 0
        let x = if log == 0 { let y = 5; log = log + 1; y + 1 } else { 2 }
        x * 10 + log
        "#,
        61,
    );
}

#[test]
fn else_if_chain_as_let_initialiser() {
    int_at_every_level(
        r#"
        let n = -3
        let r = if n == 0 { 1 } else if n < 0 { 2 } else { 3 }
        let s = if n > 0 { 10 } else if n == -3 { if n < -5 { 40 } else if n < -1 { 50 } else { 60 } } else { 70 }
        r * 100 + s
        "#,
        250,
    );
}

#[test]
fn else_if_chain_in_match_arm_bodies() {
    int_at_every_level(
        r#"
        fn f(o: Option<int>) -> int {
            match o {
                Some(v) => { if v > 10 { 3 } else if v > 0 { 2 } else { 1 } },
                None => if true { 9 } else if false { 8 } else { 7 },
            }
        }
        f(Some(20)) * 1000 + f(Some(5)) * 100 + f(Some(-1)) * 10 + f(None)
        "#,
        3219,
    );
}

#[test]
fn closure_tails_yield_plain_if_else_and_else_if_values() {
    float_at_every_level(
        r#"
        let f = fn(k: float) -> float { if k == 0.0 { 0.0 } else { 1.0 / k } }
        f(4.0) + f(0.0)
        "#,
        0.25,
    );
    int_at_every_level(
        r#"
        let typed = fn(n: int) -> int { if n > 0 { 1 } else if n < 0 { 2 } else { 3 } }
        let inferred = fn(n: int) { if n > 0 { 10 } else if n < 0 { 20 } else { 30 } }
        typed(-1) * 100 + inferred(0)
        "#,
        230,
    );
}

#[test]
fn nested_closure_tails_keep_their_values() {
    int_at_every_level(
        r#"
        let outer = fn(n: int) -> int {
            let inner = fn(m: int) -> int { if m > 10 { 1 } else { if m > 5 { 2 } else { 3 } } }
            if n == 0 { inner(7) } else if n == 1 { inner(1) } else { inner(n) }
        }
        outer(0) * 100 + outer(1) * 10 + outer(20)
        "#,
        231,
    );
}

#[test]
fn else_if_chain_as_argument_and_return_expression() {
    int_at_every_level(
        r#"
        fn id(x: int) -> int { x }
        fn g(n: int) -> int { return if n == 1 { 11 } else if n == 2 { 22 } else { 33 } }
        id(if g(2) == 22 { 4 } else if g(1) == 11 { 5 } else { 6 }) * 100 + g(3)
        "#,
        433,
    );
}

#[test]
fn else_if_chain_in_a_generic_function_and_a_method() {
    int_at_every_level(
        r#"
        fn pick<T>(c: int, a: T, b: T, d: T) -> T { if c == 0 { a } else if c == 1 { b } else { d } }
        struct Counter { n: int }
        impl Counter {
            fn sign(self) -> int { if self.n > 0 { 1 } else if self.n < 0 { 2 } else { 3 } }
        }
        pick(1, 10, 20, 30) + Counter { n: -4 }.sign()
        "#,
        22,
    );
}

#[test]
fn block_tail_if_inside_a_nested_block() {
    int_at_every_level(
        r#"
        fn f(n: int) -> int {
            let base = 100
            {
                let k = n * 2
                if k > 10 { base + k } else if k > 4 { base } else { 0 }
            }
        }
        f(10) * 1000 + f(3) + f(1)
        "#,
        120_100,
    );
}

#[test]
fn a_non_unit_tail_if_without_else_is_a_type_error() {
    let message = rejected("fn f(n: int) -> int { if n > 0 { 1 } }\nf(1)");
    assert!(message.contains("E0215"), "{message}");
    let message = rejected("fn f(n: int) -> int { if n > 0 { 1 } else if n < 0 { 2 } }\nf(1)");
    assert!(message.contains("E0215"), "{message}");
    let message = rejected("let x: int = if true { 1 }\nx");
    assert!(message.contains("E0101"), "{message}");
}

#[test]
fn a_unit_arm_in_a_valued_match_is_named() {
    let message = rejected("let r = match Some(1) { Some(_) => { let x = 1 }, None => 0 }\nr");
    assert!(message.contains("E0110"), "{message}");
}

#[test]
fn branch_statements_run_in_every_value_position() {
    int_at_every_level(
        r#"
        let mut hits = 0
        fn bump(n: int) -> int { n + 1 }
        let c = true
        let a = if c { hits = hits + 1; 5 } else { 6 }
        fn ret(c: bool) -> int { return if c { let k = 2; k * 3 } else { 0 } }
        let b = bump(if c { hits = hits + 10; 7 } else { 8 })
        let m = match Some(1) { Some(v) => if v == 1 { hits = hits + 100; v } else { 0 }, None => 0 }
        let f = fn(x: int) -> int { if x > 0 { let y = x * 2; y + 1 } else { 0 } }
        let g = if not c { 0 } else if c { hits = hits + 1000; 9 } else { 1 }
        hits * 100000 + a * 10000 + ret(true) * 1000 + b * 100 + m * 10 + f(3) - 7 + g - 9
        "#,
        111_100_000 + 50_000 + 6_000 + 800 + 10,
    );
}
