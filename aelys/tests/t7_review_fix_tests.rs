use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use aelys_driver::run_file;

fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for (path, content) in files {
        write(dir.path(), path, content);
    }
    dir
}

fn write(root: &Path, path: &str, content: &str) -> PathBuf {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("parent dirs");
    }
    fs::write(&file, content).expect("write fixture");
    file
}

fn run_int(dir: &TempDir, main: &str, source: &str) -> i64 {
    let path = write(dir.path(), main, source);
    let value = run_file(&path).unwrap_or_else(|error| panic!("{main} must run:\n{error}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("{main} must end on an int, got {value:?}"))
}

fn run_err(dir: &TempDir, main: &str, source: &str) -> String {
    let path = write(dir.path(), main, source);
    run_file(&path)
        .map(|value| panic!("{main} must be refused, got {value:?}"))
        .unwrap_err()
        .to_string()
}

#[test]
fn a_module_private_function_named_like_a_builtin_never_replaces_the_builtin() {
    let dir = tree(&[(
        "geo.aelys",
        "fn abs(x: int) -> int { if x < 0 { return 0 - x }\n return x }\n\
         fn max(a: int, b: int) -> int { if a > b { return a }\n return b }\n\
         pub fn dist(a: int, b: int) -> int { abs(a - b) }\n\
         pub fn big(a: int, b: int) -> int { max(a, b) }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs geo\n\
             let a = dist(3, 10)\n\
             let b = big(3, 10)\n\
             let c = if math::abs(-2.5) == 2.5 { 1 } else { 0 }\n\
             let d = if math::max(1.5, 2.5) == 2.5 { 1 } else { 0 }\n\
             let e = if abs(-4.5) == 4.5 { 1 } else { 0 }\n\
             a * 10000 + b * 100 + c * 10 + d + e * 1000\n"
        ),
        72_011,
    );
}

#[test]
fn a_builtin_named_function_defined_after_its_use_is_the_module_own() {
    let dir = tree(&[(
        "late.aelys",
        "pub fn dist(a: int, b: int) -> int { abs(a - b) }\n\
         pub fn via_closure(a: int) -> int { let f = fn(x: int) -> int { return abs(x) }\n f(a) }\n\
         pub struct W { pub n: int }\n\
         impl W { pub fn mag(self) -> int { abs(self.n) } }\n\
         fn abs(x: int) -> int { if x < 0 { return 1000 - x }\n return x }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs late\n\
             let w = W { n: -4 }\n\
             let k = if math::abs(-2.5) == 2.5 { 1 } else { 0 }\n\
             dist(3, 10) * 1000000 + via_closure(-2) * 100 + w.mag() + k * 100000000\n"
        ),
        100_000_000 + 1007 * 1_000_000 + 1002 * 100 + 1004,
    );
}

#[test]
fn a_module_private_string_helper_never_replaces_the_string_builtin() {
    let dir = tree(&[(
        "mstr.aelys",
        "fn trim(s: string) -> string { \"T\" }\npub fn tt() -> string { trim(\" a \") }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs mstr\n\
             let a = if tt() == \"T\" { 1 } else { 0 }\n\
             let b = if string::trim(\"  b  \") == \"b\" { 1 } else { 0 }\n\
             a * 10 + b\n"
        ),
        11,
    );
}

#[test]
fn an_entry_file_function_named_like_a_builtin_never_replaces_it_in_a_module() {
    let dir = tree(&[(
        "user.aelys",
        "pub fn lo() -> int { if math::min(1.5, 0.5) == 0.5 { 1 } else { 0 } }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs user\nlet a = min(3, 4)\nfn min(a: int, b: int) -> int { 77 }\na * 10 + user::lo()\n"
        ),
        771,
    );
}

#[test]
fn a_user_module_named_like_a_builtin_module_is_the_one_called() {
    let dir = tree(&[
        (
            "math.aelys",
            "pub fn sqrt(x: float) -> float { 42.0 }\npub fn mine() -> int { 7 }\n",
        ),
        ("time.aelys", "pub fn now() -> int { 5 }\n"),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "m1.aelys",
            "needs math\nlet s = if math::sqrt(16.0) == 42.0 { 1 } else { 0 }\nmath::mine() * 10 + s\n"
        ),
        71,
    );
    assert_eq!(run_int(&dir, "m7.aelys", "needs time\ntime::now()\n"), 5);
}
