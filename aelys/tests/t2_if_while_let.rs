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
        let value = run_with_vm_and_opt(&mut vm, source, "t2_let.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(&mut vm, source, "t2_let.aelys", OptimizationLevel::Standard) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn if_let_takes_the_matching_branch_and_binds() {
    int_at_every_level(
        r#"
        let mut total = 0
        let a: Option<int> = Some(7)
        let b: Option<int> = None
        if let Some(v) = a { total = total + v } else { total = total + 1000 }
        if let Some(v) = b { total = total + v } else { total = total + 100 }
        if let Some(v) = b { total = total + v }
        total
        "#,
        107,
    );
}

#[test]
fn if_let_is_an_expression() {
    int_at_every_level(
        r#"
        fn f(o: Option<int>) -> int { if let Some(v) = o { v * 2 } else { -1 } }
        let w = if let Some(v) = Some(4) { v + 1 } else { 0 }
        f(Some(10)) * 100 + f(None) + w * 10000
        "#,
        50_000 + 2_000 - 1,
    );
}

#[test]
fn else_if_let_chains_and_nested_enum_patterns() {
    int_at_every_level(
        r#"
        enum Shape { Circle(int), Rect(int, int), Empty }
        fn area(s: Shape) -> int {
            if let Shape::Circle(r) = s { r * r * 3 }
            else if let Shape::Rect(w, h) = s { w * h }
            else { 0 }
        }
        area(Shape::Circle(2)) * 10000 + area(Shape::Rect(3, 4)) * 10 + area(Shape::Empty)
        "#,
        120_000 + 120,
    );
}

#[test]
fn if_let_bindings_do_not_leak() {
    let message = rejected(
        r#"
        let o: Option<int> = Some(1)
        if let Some(inner) = o { inner } else { 0 }
        inner
        "#,
    );
    assert!(message.contains("inner"), "{message}");
}

#[test]
fn while_let_drains_until_the_pattern_fails() {
    int_at_every_level(
        r#"
        let mut stack = vec![1, 2, 3, 4]
        let mut sum = 0
        while let Some(top) = stack.pop() {
            if top == 3 { continue }
            sum = sum * 10 + top
        }
        sum
        "#,
        421,
    );
}

#[test]
fn while_let_supports_break_and_labels() {
    int_at_every_level(
        r#"
        let mut stack = vec![5, 6, 7, 8, 9]
        let mut seen = 0
        'drain: while let Some(top) = stack.pop() {
            for k in 0..3 {
                if top == 6 { break 'drain }
                if k == 1 { continue 'drain }
                seen = seen + 1
            }
        }
        seen * 100 + stack.len()
        "#,
        301,
    );
}

#[test]
fn while_let_over_a_counter_function() {
    int_at_every_level(
        r#"
        let mut n = 0
        fn next(n: int) -> Option<int> { if n < 5 { Some(n + 1) } else { None } }
        let mut steps = 0
        while let Some(v) = next(n) { n = v; steps = steps + 1 }
        steps * 10 + n
        "#,
        55,
    );
}

#[test]
fn if_let_in_a_closure_body() {
    int_at_every_level(
        r#"
        let get = fn(o: Option<int>) -> int { if let Some(x) = o { x } else { 0 } }
        get(Some(9)) + get(None)
        "#,
        9,
    );
}
