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
            "convert::parse_int(\"99999999999999999999\")",
            "convert::parse_int(\"9223372036854775808\")",
            "convert::parse_int(\"-9223372036854775809\")",
            "convert::parse_int(\"0x10000000000000000\")",
            "convert::parse_int(\"0x8000000000000000\")",
            "convert::parse_int(\"0b1111111111111111111111111111111111111111111111111111111111111111\")",
            "convert::parse_int(\"0o2000000000000000000000\")",
            "convert::parse_int_radix(\"8000000000000000\", 16)",
            "convert::parse_int_radix(\"-zzzzzzzzzzzzzz\", 36)",
            "convert::parse_int(\"18446744073709551616\")",
        ]),
        ["None"; 10]
    );
}

#[test]
fn parse_int_accepts_the_exact_bounds() {
    assert_eq!(
        values(&[
            "convert::parse_int(\"9223372036854775807\")",
            "convert::parse_int(\"-9223372036854775808\")",
            "convert::parse_int(\"0x7fffffffffffffff\")",
            "convert::parse_int(\"-0x8000000000000000\")",
            "convert::parse_int_radix(\"7fffffffffffffff\", 16)",
            "convert::parse_int_radix(\"-8000000000000000\", 16)",
            "convert::parse_int(\" +42 \")",
            "convert::parse_int(\"0b101\")",
            "convert::parse_int(\"0o17\")",
            "convert::parse_int_radix(\"zz\", 36)",
        ]),
        [
            "Some(9223372036854775807)",
            "Some(-9223372036854775808)",
            "Some(9223372036854775807)",
            "Some(-9223372036854775808)",
            "Some(9223372036854775807)",
            "Some(-9223372036854775808)",
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
            "convert::parse_int(\"3.9\")",
            "convert::parse_int(\"1e3\")",
            "convert::parse_int(\"-2.5\")",
            "convert::parse_int(\"nan\")",
            "convert::parse_int(\"NaN\")",
            "convert::parse_int(\"inf\")",
            "convert::parse_int(\"-infinity\")",
            "convert::parse_int(\"1.0\")",
            "convert::parse_int(\"\")",
            "convert::parse_int(\"--1\")",
            "convert::parse_int(\"0x\")",
            "convert::parse_int(\"0x-1\")",
        ]),
        ["None"; 12]
    );
}

#[test]
fn to_int_refuses_nan_and_infinities() {
    for (value, shown) in [
        ("0.0 / 0.0", "NaN"),
        ("1.0 / 0.0", "inf"),
        ("-1.0 / 0.0", "-inf"),
        ("9223372036854775808.0", "9223372036854775808"),
        ("1.0e300", "1000000000"),
    ] {
        let message = runtime_error(&format!("println(convert::to_int({value}))\n"));
        assert!(message.contains("convert.to_int"), "{value}: {message}");
        assert!(message.contains(shown), "{value}: {message}");
        assert!(
            !message.contains("9223372036854775807"),
            "{value}: the saturated value leaked: {message}"
        );
    }
}

#[test]
fn to_int_truncates_finite_floats_toward_zero() {
    let lines = run_ok(
        "println(convert::to_int(-3.9))\nprintln(convert::to_int(3.9))\nprintln(convert::to_int(140737488355327.5))\nprintln(convert::to_int(\" 7 \"))\n",
        &[],
    );
    assert_eq!(lines, ["-3", "3", "140737488355327", "7"]);
}

#[test]
fn to_int_on_a_float_text_is_an_error_not_a_truncation() {
    for text in ["3.9", "1e3", "nan", "99999999999999999999"] {
        let message = runtime_error(&format!("println(convert::to_int(\"{text}\"))\n"));
        assert!(message.contains("convert.to_int"), "{text}: {message}");
        assert!(message.contains("is not an integer"), "{text}: {message}");
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
    let message = runtime_error("needs std::math\nprintln(math::abs(-9223372036854775808))\n");
    assert!(message.contains("integer overflow"), "{message}");
    let lines = run_ok(
        "needs std::math\nprintln(math::abs(-9223372036854775807))\n",
        &[],
    );
    assert_eq!(lines, ["9223372036854775807"]);
}

#[test]
fn reading_a_64_bit_value_outside_the_int_range_is_an_error() {
    for (fill, function) in [(255, "read_u64"), (255, "read_u64_be"), (128, "read_u64")] {
        let message = runtime_error(&format!(
            "needs std::bytes\nlet b = bytes::alloc(8)\nbytes::fill(b, 0, 8, {fill})\nprintln(bytes::{function}(b, 0))\n"
        ));
        assert!(
            message.contains("exceeds the int range"),
            "{function} {fill}: {message}"
        );
    }
    for function in ["read_i64", "read_i64_be"] {
        let lines = run_ok(
            &format!(
                "needs std::bytes\nlet b = bytes::alloc(8)\nbytes::fill(b, 0, 8, 127)\nprintln(bytes::{function}(b, 0))\n"
            ),
            &[],
        );
        assert_eq!(lines, ["9187201950435737471"], "{function}");
    }
    let lines = run_ok(
        "needs std::bytes\nlet b = bytes::alloc(8)\nbytes::fill(b, 0, 8, 255)\nprintln(bytes::read_i64(b, 0))\nbytes::fill(b, 0, 8, 0)\nbytes::write_u8(b, 0, 9)\nprintln(bytes::read_u64(b, 0))\n",
        &[],
    );
    assert_eq!(lines, ["-1", "9"]);
}
