mod s1_support;

use s1_support::{
    assert_diagnostic, assert_lines_everywhere, compile_error, project_ok, run_ok, runtime_error,
};

fn shown(calls: &[&str]) -> String {
    calls
        .iter()
        .map(|call| {
            format!("println(match {call} {{ Some(v) => \"Some({{v}})\", None => \"None\" }})\n")
        })
        .collect()
}

#[test]
fn to_int_parses_integers_and_answers_none_for_everything_else() {
    assert_lines_everywhere(
        &shown(&[
            "\"42\".to_int()",
            "\" -0x1F \".to_int()",
            "\"+0b101\".to_int()",
            "\"0o17\".to_int()",
            "\"140737488355327\".to_int()",
            "\"-140737488355328\".to_int()",
            "\"140737488355328\".to_int()",
            "\"-140737488355329\".to_int()",
            "\"3.9\".to_int()",
            "\"1e3\".to_int()",
            "\"\".to_int()",
            "\"--1\".to_int()",
            "\"12abc\".to_int()",
        ]),
        &[
            "Some(42)",
            "Some(-31)",
            "Some(5)",
            "Some(15)",
            "Some(140737488355327)",
            "Some(-140737488355328)",
            "None",
            "None",
            "None",
            "None",
            "None",
            "None",
            "None",
        ],
    );
}

#[test]
fn to_int_radix_reads_digits_of_its_base() {
    assert_lines_everywhere(
        &shown(&[
            "\"ff\".to_int_radix(16)",
            "\"-zz\".to_int_radix(36)",
            "\"101\".to_int_radix(2)",
            "\"12\".to_int_radix(2)",
            "\"0xff\".to_int_radix(16)",
            "\"7fffffffffff\".to_int_radix(16)",
            "\"800000000000\".to_int_radix(16)",
        ]),
        &[
            "Some(255)",
            "Some(-1295)",
            "Some(5)",
            "None",
            "None",
            "Some(140737488355327)",
            "None",
        ],
    );
    for radix in ["1", "37", "0", "-2"] {
        let stderr = runtime_error(&format!("println(\"1\".to_int_radix({radix}))\n"));
        assert!(stderr.contains("radix must be 2-36"), "{radix}: {stderr}");
    }
}

#[test]
fn to_float_and_to_bool_parse_or_answer_none() {
    assert_lines_everywhere(
        &shown(&[
            "\"2.5\".to_float()",
            "\" -1e3 \".to_float()",
            "\"7\".to_float()",
            "\"abc\".to_float()",
            "\"\".to_float()",
            "\"1.5x\".to_float()",
            "\"YES\".to_bool()",
            "\" off \".to_bool()",
            "\"1\".to_bool()",
            "\"False\".to_bool()",
            "\"maybe\".to_bool()",
        ]),
        &[
            "Some(2.5)",
            "Some(-1000.0)",
            "Some(7.0)",
            "None",
            "None",
            "None",
            "Some(true)",
            "Some(false)",
            "Some(true)",
            "Some(false)",
            "None",
        ],
    );
}

#[test]
fn a_parse_result_is_an_option_for_question_mark_and_unwrap_or() {
    assert_lines_everywhere(
        "fn sum(a: string, b: string) -> Option<int> {
    let x = a.to_int()?
    let y = b.to_int()?
    Some(x + y)
}
println(sum(\"40\", \"2\").unwrap_or(-1))
println(sum(\"40\", \"two\").unwrap_or(-1))
let words = vec![\"3\", \"x\", \"4\"]
let mut total = 0
for w in words { total += w.to_int().unwrap_or(0) }
println(total)
",
        &["42", "-1", "7"],
    );
}

#[test]
fn integers_format_in_every_radix() {
    assert_lines_everywhere(
        "let n = 255
println(n.to_hex())
println(n.to_binary())
println(n.to_octal())
println(n.to_radix(36))
println(0.to_hex())
println(0.to_radix(2))
println((-255).to_hex())
println((-5).to_binary())
println((-8).to_octal())
println(140737488355327.to_hex())
println((-140737488355328).to_hex())
let b: u8 = 200
let w: i32 = -17
println(b.to_hex() + \" \" + w.to_radix(16))
println(255.to_radix(16) == 255.to_hex())
",
        &[
            "ff",
            "11111111",
            "377",
            "73",
            "0",
            "0",
            "-ff",
            "-101",
            "-10",
            "7fffffffffff",
            "-800000000000",
            "c8 -11",
            "true",
        ],
    );
    for radix in ["1", "37"] {
        let stderr = runtime_error(&format!("println(10.to_radix({radix}))\n"));
        assert!(stderr.contains("radix must be 2-36"), "{radix}: {stderr}");
    }
}

#[test]
fn numbers_convert_with_as_and_to_string() {
    assert_lines_everywhere(
        "let f = 3.99
println(f as int)
println((-3.99) as int)
println(7 as float)
println((1 + 2).to_string() + 2.5.to_string())
println(true.to_string())
",
        &["3", "-3", "7.0", "32.5", "true"],
    );
}

#[test]
fn chars_and_code_points_convert_with_as() {
    assert_lines_everywhere(
        "println('A' as int)
println('é' as int)
println(65 as char)
println(0x1F600 as char)
println(1114111 as char == '\\u{10FFFF}')
fn code(n: int) -> char { n as char }
println(code(97).to_string() + (98 as char).to_string())
println(('a' as int + 1) as char)
",
        &["65", "233", "A", "😀", "true", "ab", "b"],
    );
}

#[test]
fn an_invalid_code_point_traps() {
    for value in [
        "1114112",
        "55296",
        "57343",
        "(0 - 1)",
        "140737488355327",
        "inv(56000)",
    ] {
        let stderr = runtime_error(&format!(
            "fn inv(n: int) -> int {{ n }}\nprintln({value} as char)\n"
        ));
        assert!(
            stderr.contains("is not a Unicode scalar value"),
            "{value}: {stderr}"
        );
    }
    assert_eq!(
        run_ok(
            "println(55295 as char == '\\u{D7FF}')\nprintln(57344 as int)\n",
            &[]
        ),
        ["true", "57344"]
    );
}

#[test]
fn a_qualified_convert_call_names_its_replacement() {
    for (call, help) in [
        ("convert::parse_int(\"1\")", "write `s.to_int()`"),
        (
            "convert::parse_int_radix(\"1\", 2)",
            "write `s.to_int_radix(r)`",
        ),
        ("convert::parse_float(\"1\")", "write `s.to_float()`"),
        ("convert::parse_bool(\"1\")", "write `s.to_bool()`"),
        ("convert::to_string(1)", "write `x.to_string()`"),
        ("convert::to_hex(1)", "write `n.to_hex()`"),
        ("convert::to_radix(1, 2)", "write `n.to_radix(r)`"),
        ("convert::to_int(1.5)", "write `x as int`"),
        ("convert::to_float(1)", "write `x as float`"),
        ("convert::chr(65)", "write `n as char`"),
        ("convert::ord(\"A\")", "write `c as int`"),
        ("convert::to_bool(1)", "compare explicitly"),
        ("convert::type_of(1)", "every value has a static type"),
        ("convert::is_int(1)", "every value has a static type"),
    ] {
        let stderr = compile_error(&format!("let x = {call}\n"));
        assert_diagnostic(&stderr, "E0449", help);
    }
}

#[test]
fn a_bare_former_convert_call_names_its_replacement() {
    for (call, help) in [
        ("parse_int(\"1\")", "write `s.to_int()`"),
        ("to_int(1.5)", "write `x as int`"),
        ("to_string(3)", "write `x.to_string()`"),
        ("type_of(3)", "every value has a static type"),
        ("is_function(3)", "every value has a static type"),
    ] {
        let stderr = compile_error(&format!(
            "fn f() -> int {{\n    let x = {call}\n    0\n}}\nf()\n"
        ));
        assert_diagnostic(&stderr, "E0449", help);
    }
}

#[test]
fn a_declared_function_with_a_former_name_is_called() {
    assert_lines_everywhere(
        "fn to_int(x: int) -> int { x + 1 }
fn type_of(x: int) -> string { \"mine\" }
println(to_int(41))
println(type_of(0))
",
        &["42", "mine"],
    );
    let files = [
        (
            "convert.aelys",
            "pub fn parse_int(s: string) -> int { s.len() }\n",
        ),
        (
            "main.aelys",
            "use convert\nuse convert::{parse_int}\nprintln(convert::parse_int(\"abc\") + parse_int(\"de\"))\n",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["5"]);
}
