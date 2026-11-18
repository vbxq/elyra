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

// a stale binary would prove nothing; T9_AELYS_CLI swaps in another binary, e.g. a frozen baseline
fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("T9_AELYS_CLI") {
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

pub fn run_with(source: &str, flags: &[&str]) -> Outcome {
    let mut file = tempfile::Builder::new()
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    let output = Command::new(cli_binary())
        .arg("run")
        .args(flags)
        .arg(file.path())
        .output()
        .expect("aelys-cli should run");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}

pub fn run(source: &str) -> Outcome {
    run_with(source, &[])
}

pub fn run_ok(source: &str, flags: &[&str]) -> Vec<String> {
    let outcome = run_with(source, flags);
    assert_eq!(
        outcome.code,
        Some(0),
        "flags {flags:?}\nstdout: {}\nstderr: {}",
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
        "the program must be rejected\nstdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    assert!(
        outcome.stdout.is_empty(),
        "a rejected program must print nothing: {}",
        outcome.stdout
    );
    outcome.stderr
}

pub const LEVELS: [&str; 4] = ["-O0", "-O1", "-O2", "-O3"];

pub fn at_every_level(source: &str) -> Vec<Outcome> {
    LEVELS
        .iter()
        .map(|level| run_with(source, &[level]))
        .collect()
}

pub fn assert_every_level(source: &str, lines: &[&str], code: Option<i32>) {
    for (level, outcome) in LEVELS.iter().zip(at_every_level(source)) {
        assert_eq!(
            outcome.lines(),
            lines,
            "{level}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(outcome.code, code, "{level}\nstderr: {}", outcome.stderr);
    }
}

pub fn assert_traps_at_every_level(source: &str, lines: &[&str], message: &str) {
    for (level, outcome) in LEVELS.iter().zip(at_every_level(source)) {
        assert_eq!(
            outcome.lines(),
            lines,
            "{level}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(outcome.code, Some(1), "{level}\nstderr: {}", outcome.stderr);
        assert!(
            outcome.stderr.contains(message),
            "{level}: expected {message:?}\nstderr: {}",
            outcome.stderr
        );
    }
}

pub fn run_project(files: &[(&str, &str)], main: &str, flags: &[&str]) -> Outcome {
    let directory = tempfile::tempdir().expect("temporary project directory");
    for (name, source) in files {
        std::fs::write(directory.path().join(name), source).expect("write project file");
    }
    let output = Command::new(cli_binary())
        .arg("run")
        .args(flags)
        .arg(directory.path().join(main))
        .output()
        .expect("aelys-cli should run");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}
