mod common;
use common::*;

#[test]
fn literals_and_escapes() {
    assert_aelys_str(
        r#"
let a = 'a'
let nl = '\n'
let e = '\u{e9}'
let q = '\''
let bs = '\\'
let z = '\0'
"{a as int} {nl as int} {e as int} {q as int} {bs as int} {z as int} {e}"
"#,
        "97 10 233 39 92 0 é",
    );
}

#[test]
fn malformed_literals_are_lexical_errors() {
    for (program, needle) in [
        ("let c = ''\n0", "exactly one character"),
        ("let c = 'ab'\n0", "exactly one character"),
        ("let c = '1\n0", "missing closing quote"),
        ("let c = '\\u{110000}'\n0", "unicode escape"),
        ("let c = '\\u{d800}'\n0", "unicode escape"),
        ("let c = '\\q'\n0", "E0005"),
    ] {
        let err = run_aelys_err(program);
        assert!(err.contains(needle), "{program}: {err}");
        if needle != "E0005" {
            assert!(err.contains("E0008"), "{program}: {err}");
        }
    }
}

#[test]
fn unicode_escape_in_strings() {
    assert_aelys_str(r#""caf\u{e9} \u{1F600}""#, "café 😀");
}

#[test]
fn int_char_casts_round_trip_and_trap_on_invalid_scalars() {
    assert_aelys_str(
        "let c = 65 as char\nlet d = (c as int + 1) as char\n\"{c}{d}{0x1F600 as char}\"",
        "AB😀",
    );
    assert_aelys_error_contains(
        "let n = 0xD800\nlet c = n as char\nc as int",
        "invalid char: 55296 is not a Unicode scalar value",
    );
    assert_aelys_error_contains(
        "let n = -1\nlet c = n as char\nc as int",
        "invalid char: -1 is not a Unicode scalar value",
    );
}

#[test]
fn comparison_operators_follow_scalar_order() {
    assert_aelys_str(
        "\"{'a' < 'b'} {'Z' < 'a'} {'é' > 'z'} {'a' <= 'a'} {'b' >= 'c'} {'x' == 'x'} {'x' != 'y'}\"",
        "true true true true false true true",
    );
}

#[test]
fn char_methods() {
    assert_aelys_str(
        r#""{'7'.is_digit()} {'x'.is_digit()} {'é'.is_alpha()} {'3'.is_alpha()} {'\t'.is_whitespace()} {'q'.to_upper()} {'Q'.to_lower()} {'é'.to_upper()} {'ß'.to_upper()} {'_'.is_alphanumeric()}""#,
        "true false true false true Q q É ß false",
    );
}

#[test]
fn for_each_over_a_string_yields_chars() {
    assert_aelys_str(
        r#"
let mut digits = 0
let mut out = ""
for c in "a1é2" {
    if c.is_digit() { digits += 1 } else { out.push(c.to_upper()) }
}
out + " {digits}"
"#,
        "AÉ 2",
    );
}

#[test]
fn string_index_yields_a_char() {
    assert_aelys_str(
        "let s = \"héllo\"\nlet c = s[1]\nlet code = c as int\n\"{c} {code} {s[4] == 'o'}\"",
        "é 233 true",
    );
    assert_aelys_error_contains(
        "let s = \"abc\"\nlet i = -1\nlet c = s[i]\nc as int",
        "index -1 is out of bounds for length 3",
    );
}

#[test]
fn chars_returns_a_vec_of_char() {
    assert_aelys_str(
        "let cs = \"héllo\".chars()\n\"{cs.len()} {cs[1]} {cs.contains('l')} {cs.index_of('o').unwrap()}\"",
        "5 é true 4",
    );
}

#[test]
fn match_on_char_patterns() {
    assert_aelys_str(
        r#"
fn kind(c: char) -> string {
    match c {
        'a' | 'e' | 'i' | 'o' | 'u' => "v",
        ' ' => "_",
        _ => "c",
    }
}
let mut out = ""
for c in "hi you" { out += kind(c) }
out
"#,
        "cv_cvv",
    );
}

#[test]
fn string_building_with_push_and_from_char() {
    assert_aelys_str(
        r#"
let mut s = string::from_char('x')
s.push('y')
s.push('\u{e9}')
let snapshot = s
s.push('!')
s + " " + snapshot
"#,
        "xyé! xyé",
    );
}

#[test]
fn push_needs_a_mutable_string_variable() {
    let err = run_aelys_err("let s = \"a\"\ns.push('b')\n0");
    assert!(err.contains("E0203") || err.contains("immutable"), "{err}");
}

#[test]
fn display_and_type_of_char() {
    assert_aelys_str("let c = 'k'\nc.to_string() + \" {c}\"", "k k");
}

#[test]
fn chars_compare_equal_inside_collections() {
    assert_aelys_bool("vec!['a', 'b'] == \"ab\".chars()", true);
}

#[test]
fn char_and_string_do_not_mix() {
    let err = run_aelys_err("let c = 'a'\nlet s: string = c\n0");
    assert!(err.contains("E0301"), "{err}");
    let err = run_aelys_err("let b = 'a' == \"a\"\n0");
    assert!(err.contains("E0301"), "{err}");
}

#[test]
fn a_quote_before_a_longer_name_lexes_as_a_label_not_a_char() {
    assert_aelys_int("'outer: while true { break 'outer }\n0", 0);
    assert_aelys_int("let mut i = 0\n'scan: for j in 0..3 { i += j }\ni", 3);
    let err = run_aelys_err("while true { break 'done }\n0");
    assert!(err.contains("E0390"), "{err}");
    for program in ["let c = 'a\n0", "let c = '_x\n0"] {
        let err = run_aelys_err(program);
        assert!(err.contains("E0102"), "{program}: {err}");
    }
    let err = run_aelys_err("let c = 'ab'\n0");
    assert!(err.contains("exactly one character"), "{err}");
}

#[test]
fn a_single_identifier_character_between_quotes_stays_a_char() {
    assert_aelys_str(
        r#"
let a = 'a'
let u = '_'
let e = 'é'
let z = 'Z'
let s = match a { 'a' => "A", _ => "?" }
"{a}{u}{e}{z}{s}{a as int}"
"#,
        "a_éZA97",
    );
}
