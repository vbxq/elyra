mod t5_support;

use t5_support::compile_error;

const R: &str = "fn r() -> Result<int, string> { Ok(1) }\n";
const O: &str = "fn o() -> Option<int> { Some(1) }\n";

fn help_line(message: &str) -> &str {
    message
        .lines()
        .find(|line| line.trim_start().starts_with("= help:"))
        .unwrap_or_else(|| panic!("no help line in: {message}"))
}

#[test]
fn unused_result_names_every_repair() {
    for body in ["r()\nprintln(\"after\")\n", "fn f() { r() }\nf()\n"] {
        let message = compile_error(&format!("{R}{body}"));
        assert!(
            message.contains("error[E0303]: unused Result value"),
            "{message}"
        );
        let help = help_line(&message);
        for repair in ["use the value", "'?'", "match", "let _ = "] {
            assert!(help.contains(repair), "{repair}: {help}");
        }
    }
}

#[test]
fn unused_option_names_every_repair() {
    let message = compile_error(&format!("{O}o()\nprintln(\"after\")\n"));
    assert!(
        message.contains("error[E0304]: unused Option value"),
        "{message}"
    );
    let help = help_line(&message);
    for repair in ["use the value", "'?'", "match", "let _ = "] {
        assert!(help.contains(repair), "{repair}: {help}");
    }
}

#[test]
fn a_forgotten_question_mark_in_an_annotation_suggests_extracting() {
    let message = compile_error(&format!("{R}let x: int = r()\nprintln(x)\n"));
    assert!(message.contains("error[E0301]"), "{message}");
    let help = help_line(&message);
    assert!(help.contains("Result<int, string>"), "{help}");
    for repair in ["'?'", "match", "unwrap"] {
        assert!(help.contains(repair), "{repair}: {help}");
    }
}

#[test]
fn a_forgotten_question_mark_before_arithmetic_suggests_extracting() {
    for body in [
        "fn g() -> int { let x = r(); x + 1 }\nprintln(g())\n",
        "fn g() -> int { let x = r(); 1 + x }\nprintln(g())\n",
    ] {
        let message = compile_error(&format!("{R}{body}"));
        assert!(message.contains("error[E0301]"), "{message}");
        let help = help_line(&message);
        assert!(help.contains("'?'"), "{help}");
        assert!(!help.contains("Ok("), "{help}");
    }
    let message = compile_error(&format!(
        "{O}fn g() -> int {{ let x = o(); x * 2 }}\nprintln(g())\n"
    ));
    assert!(help_line(&message).contains("Option<int>"), "{message}");
}

#[test]
fn returning_a_plain_value_suggests_the_constructor() {
    let message = compile_error("fn f() -> Result<int, string> { 5 }\nlet _ = f()\n");
    assert!(message.contains("error[E0301]"), "{message}");
    assert!(help_line(&message).contains("Ok(...)"), "{message}");
    let message = compile_error("fn f() -> Option<string> { return \"x\" }\nlet _ = f()\n");
    assert!(help_line(&message).contains("Some(...)"), "{message}");
}

#[test]
fn a_result_used_as_a_condition_suggests_the_test_methods() {
    let message = compile_error(&format!("{R}if r() {{ println(1) }}\n"));
    assert!(help_line(&message).contains("is_ok()"), "{message}");
    let message = compile_error(&format!("{O}while o() {{ println(1) }}\n"));
    assert!(help_line(&message).contains("is_some()"), "{message}");
}

#[test]
fn unrelated_mismatches_get_no_sum_help() {
    let message = compile_error("fn f() -> int { \"text\" }\nprintln(f())\n");
    assert!(message.contains("error[E0301]"), "{message}");
    assert!(!message.contains("= help:"), "{message}");
}

#[test]
fn unconvertible_error_type_is_spelled_in_source_types() {
    let message = compile_error(
        "enum E1 { A }\nenum E2 { B }\nfn r() -> Result<int, E1> { Err(E1::A) }\nfn f() -> Result<float, E2> { let x = r()?; Ok(1.5) }\nlet _ = f()\n",
    );
    assert!(message.contains("error[E0374]"), "{message}");
    assert!(
        message.contains(
            "cannot propagate Result<int, E1> with '?' from a function returning Result<float, E2>"
        ),
        "{message}"
    );
    assert!(!message.contains("i64"), "{message}");
    assert!(!message.contains("f64"), "{message}");
}
