mod t5_support;

use t5_support::{run, run_ok};

fn parse_line(call: &str) -> String {
    format!("println(match {call} {{ Some(v) => \"Some({{v}})\", None => \"None\" }})\n")
}

fn values(calls: &[&str]) -> Vec<String> {
    let source: String = calls.iter().map(|call| parse_line(call)).collect();
    run_ok(&source, &[])
}

fn runtime_error(source: &str) -> String {
    let outcome = run(source);
    assert_eq!(
        outcome.code,
        Some(1),
        "{source}\nstdout: {}\nstderr: {}",
        outcome.stdout,
        outcome.stderr
    );
    assert!(
        !outcome.stderr.contains("panicked"),
        "{source}: a host panic leaked: {}",
        outcome.stderr
    );
    assert!(outcome.stderr.contains("stack trace"), "{}", outcome.stderr);
    outcome.stderr
}

#[test]
fn parse_int_rejects_out_of_range_text_without_panicking() {
    assert_eq!(
        values(&[
            "\"99999999999999999\".to_int()",
            "\"140737488355328\".to_int()",
            "\"-140737488355329\".to_int()",
            "\"0xffffffffffffff\".to_int()",
            "\"0x800000000000\".to_int()",
            "\"0b1111111111111111111111111111111111111111111111111\".to_int()",
            "\"0o7777777777777777\".to_int()",
            "\"ffffffffffffff\".to_int_radix(16)",
            "\"-zzzzzzzzzz\".to_int_radix(36)",
            "\"18446744073709551616\".to_int()",
        ]),
        ["None"; 10]
    );
}

#[test]
fn parse_int_accepts_the_exact_bounds() {
    assert_eq!(
        values(&[
            "\"140737488355327\".to_int()",
            "\"-140737488355328\".to_int()",
            "\"0x7fffffffffff\".to_int()",
            "\"-0x800000000000\".to_int()",
            "\"7fffffffffff\".to_int_radix(16)",
            "\"-800000000000\".to_int_radix(16)",
            "\" +42 \".to_int()",
            "\"0b101\".to_int()",
            "\"0o17\".to_int()",
            "\"zz\".to_int_radix(36)",
        ]),
        [
            "Some(140737488355327)",
            "Some(-140737488355328)",
            "Some(140737488355327)",
            "Some(-140737488355328)",
            "Some(140737488355327)",
            "Some(-140737488355328)",
            "Some(42)",
            "Some(5)",
            "Some(15)",
            "Some(1295)",
        ]
    );
}

#[test]
fn parse_int_never_truncates_a_float_text() {
    assert_eq!(
        values(&[
            "\"3.9\".to_int()",
            "\"1e3\".to_int()",
            "\"-2.5\".to_int()",
            "\"nan\".to_int()",
            "\"NaN\".to_int()",
            "\"inf\".to_int()",
            "\"-infinity\".to_int()",
            "\"1.0\".to_int()",
            "\"\".to_int()",
            "\"--1\".to_int()",
            "\"0x\".to_int()",
            "\"0x-1\".to_int()",
        ]),
        ["None"; 12]
    );
}

#[test]
fn as_int_refuses_nan_and_infinities() {
    for value in [
        "0.0 / 0.0",
        "1.0 / 0.0",
        "-1.0 / 0.0",
        "140737488355328.0",
        "1.0e300",
    ] {
        let message = runtime_error(&format!("println(({value}) as int)\n"));
        assert!(message.contains("integer overflow"), "{value}: {message}");
        assert!(
            !message.contains("9223372036854775807"),
            "{value}: the saturated value leaked: {message}"
        );
    }
}

#[test]
fn as_int_truncates_finite_floats_toward_zero() {
    let lines = run_ok(
        "println((-3.9) as int)\nprintln(3.9 as int)\nprintln(140737488355327.5 as int)\nprintln(\" 7 \".to_int().unwrap_or(0))\n",
        &[],
    );
    assert_eq!(lines, ["-3", "3", "140737488355327", "7"]);
}

#[test]
fn a_string_is_never_cast_to_an_int() {
    for text in ["3.9", "1e3", "nan", "99999999999999999"] {
        let outcome = run(&format!("println(\"{text}\" as int)\n"));
        assert_eq!(outcome.code, Some(1), "{text}: {}", outcome.stderr);
        assert!(
            outcome.stderr.contains("error[E0301]") && outcome.stderr.contains("invalid cast"),
            "{text}: {}",
            outcome.stderr
        );
    }
}

#[test]
fn rounding_functions_refuse_nan_infinity_and_overflow() {
    for function in ["floor", "ceil", "round", "trunc"] {
        for value in ["0.0 / 0.0", "1.0 / 0.0", "1.0e300", "-1.0e300"] {
            let message = runtime_error(&format!("println({function}({value}))\n"));
            assert!(
                message.contains(&format!("'{function}'")),
                "{function}({value}): {message}"
            );
        }
    }
    let lines = run_ok(
        "println(floor(-2.5))\nprintln(ceil(-2.5))\nprintln(round(2.5))\nprintln(trunc(-2.9))\n",
        &[],
    );
    assert_eq!(lines, ["-3", "-2", "3", "-2"]);
}

#[test]
fn abs_of_the_smallest_int_is_an_overflow_error() {
    let message = runtime_error("use std::math\nprintln(math::abs(-140737488355328))\n");
    assert!(message.contains("integer overflow"), "{message}");
    let lines = run_ok("use std::math\nprintln(math::abs(-140737488355327))\n", &[]);
    assert_eq!(lines, ["140737488355327"]);
}

#[test]
fn reading_a_64_bit_value_outside_the_int_range_is_an_error() {
    for (fill, function) in [
        (255, "read_u64"),
        (255, "read_u64_be"),
        (127, "read_i64"),
        (127, "read_i64_be"),
        (128, "read_u64"),
    ] {
        let message = runtime_error(&format!(
            "use std::bytes\nlet b = bytes::alloc(8)\nbytes::fill(b, 0, 8, {fill})\nprintln(bytes::{function}(b, 0))\n"
        ));
        assert!(
            message.contains("exceeds the 48-bit int range"),
            "{function} {fill}: {message}"
        );
    }
    let lines = run_ok(
        "use std::bytes\nlet b = bytes::alloc(8)\nbytes::fill(b, 0, 8, 255)\nprintln(bytes::read_i64(b, 0))\nbytes::fill(b, 0, 8, 0)\nbytes::write_u8(b, 0, 9)\nprintln(bytes::read_u64(b, 0))\n",
        &[],
    );
    assert_eq!(lines, ["-1", "9"]);
}
