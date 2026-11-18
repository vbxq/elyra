use std::fs;
use std::path::PathBuf;

use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "t2_fix.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}\n{source}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(&mut vm, source, "t2_fix.aelys", OptimizationLevel::Standard) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn rejected_with(source: &str, code: &str) -> String {
    let message = rejected(source);
    assert!(message.contains(code), "expected {code}, got:\n{message}\nfor:\n{source}");
    assert!(
        !message.contains("__aelys") && !message.contains("AelysTuple"),
        "internal name leaked:\n{message}"
    );
    message
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).expect("write module");
    path
}

#[test]
fn u64_ranges_are_checked_against_the_full_int_range() {
    rejected_with(
        "fn sign(x: u64) -> int { match x { 0 => 0, 1.. => 1 } }\nsign(3)",
        "E0302",
    );
    int_at_every_level(
        "fn sign(x: u64) -> int { match x { 0 => 0, 1.. => 1, _ => 2 } }\nlet a: u64 = 3\nsign(a)",
        1,
    );
    int_at_every_level(
        "fn half(b: u8) -> int { match b { 0..=127 => 0, 128..=255 => 1 } }\nlet b: u8 = 200\nhalf(b)",
        1,
    );
}

#[test]
fn an_unannotated_closure_with_a_non_value_tail_returns_unit() {
    for body in [
        "if k == 0 { 5 }",
        "let z = k",
        "while k < 0 { }",
    ] {
        rejected_with(
            &format!("let f = fn(k: int) {{ {body} }}\nlet x: int = f(1)\nx + 1"),
            "E0301",
        );
        rejected_with(
            &format!("fn apply(g: fn(int) -> int, v: int) -> int {{ g(v) }}\napply(fn(k: int) {{ {body} }}, 1)"),
            "E0301",
        );
    }
    int_at_every_level(
        "let f = fn(k: int) { if k == 0 { 5 } else { 7 } }\nlet g = fn(k: int) { let z = k }\ng(1)\nf(1)",
        7,
    );
}

#[test]
fn a_value_return_that_can_fall_through_is_e0215() {
    for source in [
        "let f = fn(k: int) { if k == 0 { return 5 } }\nlet x: int = f(1)\nx",
        "fn g(k: int) { if k == 0 { return 5 } }\nlet x: int = g(1)\nx",
        "fn g(k: int) { for i in 0..3 { if i == k { return i } } }\ng(7)",
        "let f = fn(k: int) { if k == 0 { 5 } else if k == 1 { 6 } }\nlet x: int = f(1)\nx",
        "fn g(k: int) { if k == 0 { 5 } else if k == 1 { 6 } }\nlet x: int = g(1)\nx",
        "fn outer() -> int {\n let f = fn(k: int) { while k > 0 { return k } }\n f(1)\n}\nouter()",
    ] {
        rejected_with(source, "E0215");
    }
    int_at_every_level(
        "fn g(k: int) { if k == 0 { return 5 }\n k * 2 }\nlet f = fn(k: int) { if k == 0 { return 1 } else { return 2 } }\ng(4) + f(0)",
        9,
    );
}

#[test]
fn else_if_let_chains_work_in_statement_position() {
    let program = r#"
        fn f(a: Option<int>, b: Option<int>, c: bool) -> int {
            let mut r = 0
            if let Some(x) = a { r = x } else if let Some(y) = b { r = y * 10 } else if c { r = 500 }
            if let Some(x) = a { r += x } else if c { r += 1000 } else { r += 7 }
            if c { r += 2 } else if let Some(y) = b { r += y }
            if c { r += 0 } else if let Some(y) = b { r += y } else { r += 3 }
            r
        }
        fn g(a: Option<int>, b: Option<int>, c: bool) -> int {
            if c { 1 } else if let Some(x) = a { x } else if let Some(y) = b { y * 10 } else { 99 }
        }
        let k = fn(a: Option<int>, b: Option<int>) -> int {
            if let Some(x) = a { x } else if let Some(y) = b { y + 100 } else { 0 }
        }
        let low = f(Some(1), None, false) + f(None, Some(2), false) * 10 + f(None, None, true) * 1000
        low + g(None, Some(4), false) * 10000000 + g(None, None, false) * 1000000000 + k(None, Some(5)) * 100000000000
        "#;
    int_at_every_level(
        program,
        5 + 31 * 10 + 1502 * 1000 + 40 * 10_000_000 + 99 * 1_000_000_000 + 105 * 100_000_000_000,
    );
}

#[test]
fn nested_destructuring_let_resolves_every_binding() {
    int_at_every_level(
        r#"
        struct P { x: int, y: int }
        let (t, P { x, .. }) = ((1, 2), P { x: 7, y: 8 })
        let ((e, f), (g, h)) = ((4, 8), (7, 1))
        let (a, (b, (c, d))) = (1, (2, (3, 4)))
        let mut (m, (n, _)) = (10, (20, 30))
        m += n
        let pair = ((5, 6), (7, 9))
        let ((_, six), (_, nine)) = pair
        let z = pair.1
        let w = z.1
        t.0 + x + e + f + g + h + a + b + c + d + m + six + nine + w
        "#,
        1 + 7 + 4 + 8 + 7 + 1 + 1 + 2 + 3 + 4 + 30 + 6 + 9 + 9,
    );
    int_at_every_level(
        r#"
        struct Q { p: (int, int), s: string }
        fn f(k: int) -> int {
            let ((a, b), Q { p: (c, d), .. }) = ((k, k * 2), Q { p: (k + 1, k + 2), s: "q" })
            a + b + c + d
        }
        let g = fn(k: int) -> int {
            let ((a, b), (c, d)) = ((k, 1), (2, k))
            a * b + c * d
        }
        f(1) * 100 + g(3)
        "#,
        (1 + 2 + 2 + 3) * 100 + 3 + 6,
    );
}
