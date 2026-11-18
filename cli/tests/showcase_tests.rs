use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn showcase_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate always has a workspace parent")
        .join("examples/showcase")
        .join(name)
}

fn run_program(name: &str, input: Option<&Path>, flags: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aelys-cli"));
    command
        .args(flags)
        .arg(showcase_dir(name).join(format!("{name}.aelys")));
    if let Some(input) = input {
        command.arg(input);
    }
    command.output().expect("run aelys-cli")
}

fn run_oracle(name: &str, input: Option<&Path>) -> String {
    let mut command = Command::new("python3");
    command.arg("-I").arg(showcase_dir(name).join("oracle.py"));
    if let Some(input) = input {
        command.arg(input);
    }
    let output = command.output().expect("run python3 oracle");
    assert!(
        output.status.success(),
        "{name} oracle failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 oracle output")
}

const MODES: &[&[&str]] = &[&["-O0"], &["-O2"], &["-ae.jit=on"]];

fn check_showcase(name: &str, input: Option<&str>, expected_file: &str) -> String {
    let dir = showcase_dir(name);
    let input = input.map(|file| dir.join(file));
    let expected = std::fs::read_to_string(dir.join(expected_file)).expect("expected output file");
    assert_eq!(
        run_oracle(name, input.as_deref()),
        expected,
        "{name}: {expected_file} is not what its python oracle prints"
    );
    for flags in MODES {
        let output = run_program(name, input.as_deref(), flags);
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
    expected
}

fn answer_to(transcript: &str, line: &str) -> String {
    let prompt = format!("> {line}");
    let mut lines = transcript.lines();
    while let Some(current) = lines.next() {
        if current == prompt {
            return lines
                .next()
                .expect("an answer after the prompt")
                .to_string();
        }
    }
    panic!("the calc transcript has no prompt {prompt:?}");
}

#[test]
fn calc_matches_its_oracle() {
    let transcript = check_showcase("calc", Some("script.txt"), "expected.txt");
    let cases = [
        ("1 + 2 * 3", "7.0"),
        ("2 ^ 3 ^ 2", "512.0"),
        ("-2 ^ 2", "-4.0"),
        ("1 / (3 - 3)", "error: division by zero"),
        ("let q = 7 / (2 - 2)", "error: division by zero"),
        ("q", "error: unknown variable 'q'"),
        ("(1 + 2", "error: unexpected end of input"),
        ("((2 * 3) + 1", "error: unexpected end of input"),
        ("1 + 2)", "error: column 6: unexpected ')'"),
        ("(4))", "error: column 4: unexpected ')'"),
        ("4 + * 5", "error: column 5: unexpected '*'"),
        ("3 + $ 4", "error: column 5: unexpected character '$'"),
        ("1.2.3 + 1", "error: column 1: malformed number '1.2.3'"),
        ("max(1)", "error: max() takes 2 arguments, got 1"),
        ("sqrt(-4)", "error: sqrt() is undefined for -4.0"),
        ("floor(2.5)", "error: unknown function 'floor'"),
    ];
    for (line, answer) in cases {
        assert_eq!(
            answer_to(&transcript, line),
            answer,
            "calc answer to {line}"
        );
    }
}

#[test]
fn container_matches_its_oracle() {
    let output = check_showcase("container", None, "expected.txt");
    assert!(output.contains("drained ints: [-3, 0, 7, 7, 19, 42, 56, 100]\n"));
    assert!(output.contains("ring keeps 3 of 5, dropped 2, oldest 3\n"));
    assert!(output.contains("log drained: [panic(p9), reboot(p2)], empty = true\n"));
}

#[test]
fn todo_matches_its_oracle() {
    let output = check_showcase("todo", Some("commands.txt"), "expected.txt");
    for line in [
        "line 8: task #2 is already done",
        "line 9: priority must be a number, got 'high'",
        "line 16: no task #9",
        "line 17: unknown command 'frobnicate'",
        "line 18: 'done' needs a task id",
        "line 19: 'add' needs a priority and a text",
        "line 37: nothing to undo",
        "line 38: task id must be a number, got 'abc'",
        "finished with 8 errors",
    ] {
        assert!(
            output.lines().any(|l| l == line),
            "todo output lacks {line:?}"
        );
    }
}

#[test]
fn wordfreq_matches_its_oracle() {
    let output = check_showcase("wordfreq", Some("text.txt"), "expected.txt");
    assert!(
        output.starts_with("words: 267 total, 186 distinct\nlongest: neighbourhood (13 letters)\n")
    );
}

#[test]
fn wordfreq_counts_unicode_words_and_letters() {
    let output = check_showcase("wordfreq", Some("unicode.txt"), "expected_unicode.txt");
    assert!(
        output.starts_with("words: 57 total, 44 distinct\nlongest: smörgåsbord (11 letters)\n")
    );
    for line in [
        "  1. été       3",
        "  2. москва    3",
        "  3. привет    3",
        "letters: 243 total, 64 distinct, accented vowels: àåèéëïöúûüý",
    ] {
        assert!(
            output.lines().any(|l| l == line),
            "wordfreq unicode output lacks {line:?}"
        );
    }
}

#[test]
fn wordfreq_reports_an_empty_text() {
    let output = check_showcase("wordfreq", Some("empty.txt"), "expected_empty.txt");
    assert_eq!(
        output,
        "words: 0 total, 0 distinct\nlongest: none\ntop 10 words:\n\
         letters: 0 total, 0 distinct, accented vowels: none\nvowels (accents folded):\n\
         \x20 a    0\n  e    0\n  i    0\n  o    0\n  u    0\n  y    0\n"
    );
}

#[test]
fn showcases_reading_a_file_report_usage_and_missing_files() {
    for name in ["calc", "todo", "wordfreq"] {
        let output = run_program(name, None, &[]);
        assert_eq!(output.status.code(), Some(2), "{name} without argument");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with(&format!("usage: {name} <")),
            "{name} usage line"
        );

        let missing = showcase_dir(name).join("no-such-input.txt");
        let output = run_program(name, Some(&missing), &[]);
        assert_eq!(output.status.code(), Some(1), "{name} with a missing file");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with(&format!("{name}: ")),
            "{name} missing file message"
        );
    }
}

#[test]
fn a_script_ending_in_a_unit_statement_prints_nothing_after_its_output() {
    let dir = std::env::temp_dir().join("aelys_cli_showcase_trailing_unit");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cases = [
        (
            "loop_tail",
            "println(\"hi\")\nfor i in 0..2 { println(i) }\n",
            "hi\n0\n1\n",
        ),
        ("call_tail", "fn f() { println(\"x\") }\nf()\n", "x\n"),
        ("unit_literal_tail", "println(\"y\")\n()\n", "y\n"),
        (
            "if_let_tail",
            "if let Some(v) = Some(4) { println(v) }\n",
            "4\n",
        ),
        ("value_tail", "let x = 3\nx + 1\n", "4\n"),
    ];
    for (name, source, expected) in cases {
        let path = dir.join(format!("{name}.aelys"));
        std::fs::write(&path, source).expect("write script");
        for flags in MODES {
            let output = Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
                .args(*flags)
                .arg(&path)
                .output()
                .expect("run aelys-cli");
            assert!(output.status.success(), "{name} {flags:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                expected,
                "{name} {flags:?}"
            );
        }
    }
}
