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
        let value = run_with_vm_and_opt(&mut vm, source, "t2_tuple.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(
        &mut vm,
        source,
        "t2_tuple.aelys",
        OptimizationLevel::Standard,
    ) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn tuple_return_field_access_and_destructuring() {
    int_at_every_level(
        r#"
        fn divmod(a: int, b: int) -> (int, int) { (a / b, a % b) }
        let t = divmod(17, 5)
        let (q, r) = divmod(23, 4)
        t.0 * 1000 + t.1 * 100 + q * 10 + r
        "#,
        3253,
    );
}

#[test]
fn heterogeneous_and_nested_tuples() {
    int_at_every_level(
        r#"
        let t = (1, (2.5, "zz"), true)
        let ((a, b), c) = ((1, 2), 3)
        let n = if t.2 { t.1.1.len() } else { 0 }
        n * 1000 + a * 100 + b * 10 + c + t.0
        "#,
        2124,
    );
}

#[test]
fn tuple_patterns_in_match_are_checked_for_exhaustiveness() {
    int_at_every_level(
        r#"
        fn f(t: (int, bool)) -> int { match t { (0, _) => 1, (n, true) => n * 10, (_, false) => 2 } }
        f((0, false)) * 10000 + f((5, true)) * 10 + f((5, false))
        "#,
        10_502,
    );
    let message = rejected(
        "fn f(t: (bool, bool)) -> int { match t { (true, _) => 1, (_, true) => 2 } }\nf((true, true))",
    );
    assert!(message.contains("missing (false, false)"), "{message}");
}

#[test]
fn tuples_in_vectors_struct_fields_and_enum_payloads() {
    int_at_every_level(
        r#"
        struct Seg { a: (int, int), b: (int, int) }
        enum Ev { Move((int, int)), Stop }
        let v: Vec<(int, bool)> = vec![(1, true), (2, false), (3, true)]
        let mut kept = 0
        for item in v { if item.1 { kept = kept + item.0 } }
        let s = Seg { a: (1, 2), b: (4, 6) }
        let (x0, y0) = s.a
        let dx = s.b.0 - x0
        let dy = s.b.1 - y0
        let e = Ev::Move((7, 8))
        let m = match e { Ev::Move((p, q)) => p + q, Ev::Stop => 0 }
        kept * 10000 + dx * 1000 + dy * 100 + m
        "#,
        43_415,
    );
}

#[test]
fn tuple_equality_is_structural() {
    int_at_every_level(
        r#"
        let a = (1, "x")
        let b = (1, "x")
        let c = (2, "x")
        let mut score = 0
        if a == b { score = score + 1 }
        if a != c { score = score + 10 }
        if (1, (2, 3)) == (1, (2, 3)) { score = score + 100 }
        score
        "#,
        111,
    );
}

#[test]
fn generic_functions_and_closures_over_tuples() {
    int_at_every_level(
        r#"
        fn swap<A, B>(t: (A, B)) -> (B, A) { (t.1, t.0) }
        let (x, y) = swap((true, 9))
        let mk = fn(n: int) -> (int, int) { if n > 0 { (n, 1) } else if n < 0 { (0 - n, -1) } else { (0, 0) } }
        let (m, sg) = mk(-4)
        let f = fn(p: (int, int)) -> int { let (a, b) = p
            a * b }
        (if y { 1 } else { 0 }) * 10000 + x * 1000 + m * 100 + sg * 10 + f((2, 3))
        "#,
        19_396,
    );
}

#[test]
fn destructuring_let_with_mut_and_wildcards() {
    int_at_every_level(
        r#"
        let mut (a, b) = (1, 2)
        a = a + 10
        let (_, keep, _) = (7, 8, 9)
        a * 100 + b * 10 + keep
        "#,
        1128,
    );
}

#[test]
fn struct_destructuring_let() {
    int_at_every_level(
        r#"
        struct Point { x: int, y: int }
        struct Line { head: Point, tail: Point }
        let p = Point { x: 3, y: 4 }
        let Point { x, y } = p
        let l = Line { head: Point { x: 1, y: 2 }, tail: Point { x: 5, y: 9 } }
        let Line { head: Point { x: fx, .. }, tail } = l
        x * 1000 + y * 100 + fx * 10 + tail.y
        "#,
        3419,
    );
}

#[test]
fn tuple_misuse_is_rejected_with_named_diagnostics() {
    let message = rejected("let mut t = (1, 2)\nt.0 = 5\nt.0");
    assert!(message.contains("E0393"), "{message}");
    let message = rejected("let (a, b) = (1, 2, 3)\na");
    assert!(message.contains("E0301"), "{message}");
    let message = rejected("let (a, 0) = (1, 2)\na");
    assert!(
        message.contains("E0108") && message.contains("refutable"),
        "{message}"
    );
    let message = rejected("let t: (int, string) = (1, 2)\n0");
    assert!(message.contains("E0301"), "{message}");
    let message = rejected("struct Q { a: int }\nlet Q { a, b } = Q { a: 1 }\na");
    assert!(message.contains("E0363"), "{message}");
    let message = rejected("struct Q { a: int }\nstruct R { a: int }\nlet Q { a } = R { a: 1 }\na");
    assert!(message.contains("E0301"), "{message}");
}
