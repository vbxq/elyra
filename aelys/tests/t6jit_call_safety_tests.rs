use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "aelys-cli"])
                .status()
                .expect("cargo build should run");
            assert!(status.success(), "building aelys-cli failed");
            let mut directory = std::env::current_exe().expect("test executable path");
            directory.pop();
            if directory.ends_with("deps") {
                directory.pop();
            }
            let binary = directory.join(format!("aelys-cli{}", std::env::consts::EXE_SUFFIX));
            assert!(binary.is_file(), "missing aelys-cli at {binary:?}");
            binary
        })
        .clone()
}

fn run(main: &Path, jit: bool) -> (Option<i32>, String, String) {
    let mut command = Command::new(cli_binary());
    command.arg("run");
    if jit {
        command.arg("-ae.jit=on");
    }
    let output = command
        .arg(main)
        .current_dir(main.parent().expect("source directory"))
        .output()
        .expect("aelys-cli should run");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn program(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (name, text) in files {
        std::fs::write(directory.path().join(name), text).expect("write source");
    }
    let main = directory.path().join(files[0].0);
    (directory, main)
}

fn assert_prints(files: &[(&str, &str)], expected: &str) {
    let (_directory, main) = program(files);
    let interpreted = run(&main, false);
    let compiled = run(&main, true);
    assert_eq!(
        interpreted, compiled,
        "the jit changed the program's behaviour"
    );
    assert_eq!(compiled.0, Some(0), "{compiled:?}");
    assert_eq!(compiled.1.lines().next(), Some(expected), "{compiled:?}");
}

const UTILS: &str = r#"pub fn twice(x: int) -> int {
    let mut s = 0
    for k in 0..2 { s = s + x }
    return s
}
"#;

fn expected_with(extra: i64) -> String {
    let mut acc = 0i64;
    for i in 0..3000i64 {
        acc = (acc + 2 * i + 1 + extra) & 0xFF_FFFF;
    }
    format!("{}", acc * 100_000 + 12345)
}

#[test]
fn a_hot_function_calling_an_imported_function_keeps_its_caller_frame() {
    let main = r#"use utils
fn hot(x: int) -> int {
    return utils::twice(x) + 1
}
fn run(n: int) -> int {
    let mut keep = 12345
    let mut acc = 0
    for i in 0..n {
        acc = (acc + hot(i)) & 0xFFFFFF
    }
    return acc * 100000 + keep
}
println("{run(3000)}")
"#;
    assert_prints(
        &[("main.aelys", main), ("utils.aelys", UTILS)],
        &expected_with(0),
    );
}

#[test]
fn a_hot_function_mixing_a_native_and_an_imported_call_keeps_its_caller_frame() {
    let main = r#"use utils
use std::math
fn hot(x: int) -> int {
    let a = math::abs(0 - 1)
    return utils::twice(x) + a
}
fn run(n: int) -> int {
    let mut keep = 12345
    let mut acc = 0
    for i in 0..n {
        acc = (acc + hot(i) + 1 - 1) & 0xFFFFFF
    }
    return acc * 100000 + keep
}
println("{run(3000)}")
"#;
    assert_prints(
        &[("main.aelys", main), ("utils.aelys", UTILS)],
        &expected_with(0),
    );
}

#[test]
fn a_compiled_loop_calling_an_uncompiled_function_value_keeps_its_live_locals() {
    let main = r#"fn label(i: int) -> int {
    let text = "n{i}"
    return text.len()
}
fn run(n: int) -> int {
    let f = label
    let mut keep = 777
    let mut a = 1
    let mut b = 2
    for i in 0..n {
        let c = f(i)
        a = (a + c + keep) & 0xFFFFFF
        b = (b ^ a) + 1
    }
    return a * 1000 + b % 1000 + keep
}
println("{run(30000)}")
"#;
    let (keep, mut a, mut b) = (777i64, 1i64, 2i64);
    for i in 0..30_000i64 {
        let c = 1 + i.to_string().len() as i64;
        a = (a + c + keep) & 0xFF_FFFF;
        b = (b ^ a) + 1;
    }
    assert_prints(
        &[("main.aelys", main)],
        &format!("{}", a * 1000 + b % 1000 + keep),
    );
}
