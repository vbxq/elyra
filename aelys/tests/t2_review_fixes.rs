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
