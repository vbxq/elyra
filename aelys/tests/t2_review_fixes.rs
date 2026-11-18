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
