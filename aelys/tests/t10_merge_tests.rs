mod s1_support;

use s1_support::{assert_diagnostic, compile_error, run_ok, runtime_error};

// expected strings computed by python (int(), format(), and a radix-36 loop)
const MODES: [&[&str]; 6] = [
    &["-O0"],
    &["-O1"],
    &["-O2"],
    &["-O3"],
    &["-ae.jit=on"],
    &["-ae.gc-stress=on", "-ae.gc-verify=on"],
];

fn assert_everywhere(source: &str, expected: &[&str]) {
    for flags in MODES {
        assert_eq!(run_ok(source, flags), expected, "flags {flags:?}\n{source}");
    }
}

fn shown(calls: &[&str]) -> String {
    calls
        .iter()
        .map(|call| {
            format!("println(match {call} {{ Some(v) => \"Some({{v}})\", None => \"None\" }})\n")
        })
        .collect()
}

#[test]
fn string_to_int_accepts_the_whole_i64_range() {
    assert_everywhere(
        &shown(&[
            "\"9223372036854775807\".to_int()",
            "\"9223372036854775808\".to_int()",
            "\"-9223372036854775808\".to_int()",
            "\"-9223372036854775809\".to_int()",
            "\"140737488355328\".to_int()",
            "\"-140737488355329\".to_int()",
            "\"0x7fffffffffffffff\".to_int()",
            "\"0x8000000000000000\".to_int()",
            "\"-8000000000000000\".to_int_radix(16)",
            "\"1y2p0ij32e8e7\".to_int_radix(36)",
            "\"1y2p0ij32e8e8\".to_int_radix(36)",
        ]),
        &[
            "Some(9223372036854775807)",
            "None",
            "Some(-9223372036854775808)",
            "None",
            "Some(140737488355328)",
            "Some(-140737488355329)",
            "Some(9223372036854775807)",
            "None",
            "Some(-9223372036854775808)",
            "Some(9223372036854775807)",
            "None",
        ],
    );
}

#[test]
fn a_parsed_wide_value_is_a_working_int() {
    assert_everywhere(
        "let big = \"140737488355328\".to_int().unwrap()
let max = \"9223372036854775807\".to_int().unwrap()
println(big + 1)
println(max - big)
println(big == 140737488355328)
println(\"-9223372036854775808\".to_int().unwrap() < 0 - max)
",
        &["140737488355329", "9223231299366420479", "true", "true"],
    );
    let stderr =
        runtime_error("let max = \"9223372036854775807\".to_int().unwrap()\nprintln(max + 1)\n");
    assert!(stderr.contains("integer overflow"), "{stderr}");
}

#[test]
fn int_formatting_methods_cover_the_i64_edges() {
    assert_everywhere(
        "fn id(x: int) -> int { x }
let min = id(-9223372036854775807) - 1
let max = id(9223372036854775807)
println(min.to_hex())
println(min.to_binary())
println(min.to_octal())
println(min.to_radix(36))
println(max.to_hex())
println(max.to_radix(36))
println((-9223372036854775807 - 1).to_hex())
println(id(140737488355328).to_hex())
",
        &[
            "-8000000000000000",
            "-1000000000000000000000000000000000000000000000000000000000000000",
            "-1000000000000000000000",
            "-1y2p0ij32e8e8",
            "7fffffffffffffff",
            "1y2p0ij32e8e7",
            "-8000000000000000",
            "800000000000",
        ],
    );
}

#[test]
fn int_formatting_methods_read_u64_values_above_the_int_range() {
    assert_everywhere(
        "let u: u64 = 18446744073709551615
let h: u64 = 9223372036854775808
let m: i64 = -9223372036854775808
println(u.to_hex())
println(u.to_binary())
println(u.to_octal())
println(u.to_radix(36))
println(h.to_hex())
println(m.to_hex())
",
        &[
            "ffffffffffffffff",
            "1111111111111111111111111111111111111111111111111111111111111111",
            "1777777777777777777777",
            "3w5e11264sgsf",
            "8000000000000000",
            "-8000000000000000",
        ],
    );
}

#[test]
fn as_int_follows_the_i64_range() {
    assert_everywhere(
        "fn f(x: float) -> float { x }
println(f(140737488355328.0) as int)
println(f(-9223372036854775808.0) as int)
println(f(9.2e18) as int)
",
        &[
            "140737488355328",
            "-9223372036854775808",
            "9200000000000000000",
        ],
    );
    for value in ["9223372036854775808.0", "-9.3e18"] {
        let stderr = runtime_error(&format!(
            "fn f(x: float) -> float {{ x }}\nprintln(f({value}) as int)\n"
        ));
        assert!(stderr.contains("integer overflow"), "{value}: {stderr}");
    }
}

#[test]
fn the_convert_module_stays_removed_with_wide_arguments() {
    for (call, help) in [
        (
            "convert::parse_int(\"9223372036854775807\")",
            "write `s.to_int()`",
        ),
        (
            "convert::to_hex(-9223372036854775807 - 1)",
            "write `n.to_hex()`",
        ),
        ("convert::to_int(9.2e18)", "write `x as int`"),
        (
            "convert::is_int(9223372036854775807)",
            "every value has a static type",
        ),
        ("parse_int(\"9223372036854775807\")", "write `s.to_int()`"),
    ] {
        let stderr = compile_error(&format!("let x = {call}\n"));
        assert_diagnostic(&stderr, "E0449", help);
    }
    let stderr = compile_error("use std::convert\nprintln(1)\n");
    assert!(stderr.contains("error[E0449]"), "{stderr}");
}
