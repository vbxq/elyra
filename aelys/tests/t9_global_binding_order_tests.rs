mod t9_support;

use t9_support::{LEVELS, assert_every_level, run_project};

const UTIL: &str = "pub fn twice(x: int) -> int { x * 2 }\npub let base = 1000\n";

fn assert_project_every_level(main: &str, lines: &[&str]) {
    for level in LEVELS {
        let outcome = run_project(
            &[("util.aelys", UTIL), ("main.aelys", main)],
            "main.aelys",
            &[level],
        );
        assert_eq!(
            outcome.lines(),
            lines,
            "{level}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(outcome.code, Some(0), "{level}\nstderr: {}", outcome.stderr);
    }
}

#[test]
fn a_top_level_let_does_not_reach_reads_of_the_import_it_shadows_before_it() {
    assert_project_every_level(
        "use util\nprintln(base)\nlet base = 7\nprintln(base)\nprintln(util::base)\n",
        &["1000", "7", "1000"],
    );
}

#[test]
fn an_initializer_read_before_the_shadowing_let_keeps_the_imported_value() {
    assert_project_every_level(
        "use util\nlet b2 = base * 2\nlet base = 7\nprintln(b2)\nprintln(base)\n",
        &["2000", "7"],
    );
}

#[test]
fn a_function_called_before_the_shadowing_let_reads_the_import() {
    assert_project_every_level(
        "use util\nfn early() -> int { base }\nprintln(early())\nlet base = 7\nprintln(early())\nprintln(base + 1)\n",
        &["1000", "7", "8"],
    );
}

#[test]
fn a_constant_read_only_after_its_binding_still_propagates_the_same_value() {
    assert_every_level(
        "let K = 6 * 7\nfn get() -> int { K }\nprintln(K)\nprintln(get())\n",
        &["42", "42"],
        Some(0),
    );
}
