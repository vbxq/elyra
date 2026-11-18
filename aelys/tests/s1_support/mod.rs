#![allow(dead_code)]

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

impl Outcome {
    pub fn lines(&self) -> Vec<&str> {
        self.stdout.lines().filter(|line| *line != "()").collect()
    }
}

// S1_AELYS_CLI swaps in another binary, e.g. the frozen baseline the witnesses must fail on
fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("S1_AELYS_CLI") {
                return PathBuf::from(path);
            }
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

fn outcome(output: std::process::Output) -> Outcome {
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}

pub fn run_with(source: &str, flags: &[&str]) -> Outcome {
    let mut file = tempfile::Builder::new()
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    outcome(
        Command::new(cli_binary())
            .arg("run")
            .args(flags)
            .arg(file.path())
            .output()
            .expect("aelys-cli should run"),
    )
}

pub fn run(source: &str) -> Outcome {
    run_with(source, &[])
}

pub fn run_ok(source: &str, flags: &[&str]) -> Vec<String> {
    let outcome = run_with(source, flags);
    assert_eq!(
        outcome.code,
        Some(0),
        "flags {flags:?}\nsource:\n{source}\nstdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    outcome.lines().into_iter().map(str::to_string).collect()
}

pub fn assert_lines_everywhere(source: &str, expected: &[&str]) {
    for flags in [
        &["-O0"][..],
        &["-O1"][..],
        &["-O2"][..],
        &["-O3"][..],
        &["-ae.jit=on"][..],
    ] {
        assert_eq!(run_ok(source, flags), expected, "flags {flags:?}");
    }
}

pub fn compile_error(source: &str) -> String {
    let outcome = run(source);
    assert_eq!(
        outcome.code,
        Some(1),
        "the program must be rejected\nsource:\n{source}\nstdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    assert!(
        outcome.stdout.is_empty(),
        "a rejected program must print nothing: {}",
        outcome.stdout
    );
    assert!(
        !outcome.stderr.contains("stack trace"),
        "the program must be rejected before it runs: {}",
        outcome.stderr
    );
    outcome.stderr
}

pub fn runtime_error(source: &str) -> String {
    let outcome = run(source);
    assert_eq!(
        outcome.code,
        Some(1),
        "the program must trap\nsource:\n{source}\nstdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains("stack trace"),
        "the program must trap at run time, not be rejected: {}",
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("panicked"),
        "a host panic leaked: {}",
        outcome.stderr
    );
    outcome.stderr
}

pub fn run_project(files: &[(&str, &str)], main: &str) -> Outcome {
    let directory = tempfile::tempdir().expect("temporary project directory");
    for (name, source) in files {
        let path = directory.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create project directory");
        }
        std::fs::write(path, source).expect("write project file");
    }
    outcome(
        Command::new(cli_binary())
            .arg("run")
            .arg(directory.path().join(main))
            .output()
            .expect("aelys-cli should run"),
    )
}

pub fn project_ok(files: &[(&str, &str)], main: &str) -> Vec<String> {
    let outcome = run_project(files, main);
    assert_eq!(
        outcome.code,
        Some(0),
        "stdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    outcome.lines().into_iter().map(str::to_string).collect()
}

pub fn assert_diagnostic(stderr: &str, code: &str, help: &str) {
    assert!(
        stderr.contains(&format!("error[{code}]")),
        "expected {code}: {stderr}"
    );
    assert!(
        stderr.contains(&format!("= help: {help}")),
        "expected the help line {help:?}: {stderr}"
    );
}
