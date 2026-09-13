mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use aelys_driver::run_file;
use common::*;
use tempfile::TempDir;

// a callglobal/callcached/callupval hands the callee a frame based at dst+1 and lets it use its own

#[test]
fn field_write_from_zero_arg_global_keeps_the_target() {
    let code = r#"
fn zero() -> int {
    1
}

struct Box { n: int, m: int }

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    b.n = zero()
    b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}

#[test]
fn nested_field_write_from_zero_arg_global_keeps_the_target() {
    let code = r#"
fn zero() -> int {
    1
}

struct Box { n: int, m: int }
struct Outer { b: Box, k: int }

fn probe() -> int {
    let mut o = Outer { b: Box { n: 0, m: 0 }, k: 9 }
    let keep = 5
    o.b.n = zero()
    o.b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}

#[test]
fn field_write_from_zero_arg_cached_call_keeps_the_target() {
    let code = r#"
fn zero() -> int {
    1
}

struct Box { n: int, m: int }

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    let f = zero
    b.n = f()
    b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}

#[test]
fn field_write_from_zero_arg_lambda_keeps_the_target() {
    let code = r#"
struct Box { n: int, m: int }

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    let g = fn() -> int { 1 }
    b.n = g()
    b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}

#[test]
fn field_write_from_zero_arg_associated_function_keeps_the_target() {
    let code = r#"
struct Box { n: int, m: int }

struct Maker { v: int }
impl Maker {
    fn make() -> int { 1 }
}

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    b.n = Maker::make()
    b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}

fn write_file(dir: &TempDir, path: &str, content: &str) -> PathBuf {
    let file_path = dir.path().join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    let mut file = File::create(&file_path).expect("create file");
    write!(file, "{}", content).expect("write file");
    file_path
}

#[test]
fn field_write_from_zero_arg_module_call_keeps_the_target() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_file(&dir, "callwin.aelys", "pub fn zero() -> int { 1 }\n");
    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs callwin

struct Box { n: int, m: int }

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    b.n = callwin::zero()
    b.n + keep
}

probe()
"#,
    );

    let result = run_file(&main_path).expect("module program should run");
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn field_write_from_one_arg_global_keeps_the_target() {
    let code = r#"
fn one(x: int) -> int { x }

struct Box { n: int, m: int }

fn probe() -> int {
    let mut b = Box { n: 0, m: 0 }
    let keep = 5
    b.n = one(1)
    b.n + keep
}

probe()
"#;
    assert_aelys_int(code, 6);
}
