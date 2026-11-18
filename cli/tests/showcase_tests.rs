use std::path::{Path, PathBuf};
use std::process::Command;

fn showcase_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate always has a workspace parent")
        .join("examples/showcase")
        .join(name)
}

fn run_showcase(name: &str, input: Option<&str>, flags: &[&str]) {
    let dir = showcase_dir(name);
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect("expected.txt");
    let mut command = Command::new(env!("CARGO_BIN_EXE_aelys-cli"));
    command.args(flags).arg(dir.join(format!("{name}.aelys")));
    if let Some(input) = input {
        command.arg(dir.join(input));
    }
    let output = command.output().expect("run aelys-cli");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{name} {flags:?} exited with {:?}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        stderr.is_empty(),
        "{name} {flags:?} wrote to stderr:\n{stderr}"
    );
    assert_eq!(
        stdout, expected,
        "{name} {flags:?} stdout differs from its oracle"
    );
}

const MODES: &[&[&str]] = &[&["-O0"], &["-O2"], &["-ae.jit=on"]];

#[test]
fn calc_matches_its_oracle() {
    for flags in MODES {
        run_showcase("calc", None, flags);
    }
}

#[test]
fn container_matches_its_oracle() {
    for flags in MODES {
        run_showcase("container", None, flags);
    }
}

#[test]
fn todo_matches_its_oracle() {
    for flags in MODES {
        run_showcase("todo", Some("commands.txt"), flags);
    }
}

#[test]
fn wordfreq_matches_its_oracle() {
    for flags in MODES {
        run_showcase("wordfreq", Some("text.txt"), flags);
    }
}

#[test]
fn a_script_ending_in_a_unit_statement_prints_nothing_after_its_output() {
    let dir = std::env::temp_dir().join("aelys_cli_showcase_trailing_unit");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("unit_tail.aelys");
    std::fs::write(&path, "println(\"hi\")\nfor i in 0..2 { println(i) }\n").expect("write script");
    let output = Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg(&path)
        .output()
        .expect("run aelys-cli");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "hi\n0\n1\n");
}
