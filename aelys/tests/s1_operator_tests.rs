mod s1_support;

use s1_support::{assert_diagnostic, assert_lines_everywhere, compile_error, run_ok};

#[test]
fn and_binds_tighter_than_or_and_bang_tighter_than_both() {
    assert_lines_everywhere(
        "println(true || false && false)
println((true || false) && false)
println(!true || true)
println(!(true || true))
println(!false && false)
println(false && false || true)
println(1 < 2 && 3 > 4 || 5 == 5)
println(!false == true)
println(!!true)
",
        &[
            "true", "false", "true", "false", "false", "true", "true", "true", "true",
        ],
    );
}

#[test]
fn the_right_operand_runs_only_when_it_decides_the_result() {
    assert_lines_everywhere(
        "let mut calls = 0
fn hit(v: bool) -> bool {
    calls += 1
    v
}
let a = false && hit(true)
let b = true || hit(false)
println(\"{a} {b} {calls}\")
let c = true && hit(false)
let d = false || hit(true)
println(\"{c} {d} {calls}\")
let e = hit(false) && hit(true) || hit(true)
println(\"{e} {calls}\")
fn inside(x: int) -> bool {
    x > 0 && hit(x > 5) || !hit(false)
}
println(\"{inside(0)} {calls}\")
println(\"{inside(9)} {calls}\")
let lazy = fn(v: bool) -> bool { v || hit(true) }
println(\"{lazy(true)} {calls}\")
",
        &[
            "false true 0",
            "false true 2",
            "true 4",
            "true 5",
            "true 6",
            "true 6",
        ],
    );
}

#[test]
fn bang_negates_calls_fields_elements_methods_and_groups() {
    assert_lines_everywhere(
        "struct Flag { on: bool }
impl Flag {
    fn get(self) -> bool { self.on }
}
fn yes() -> bool { true }
let f = Flag { on: true }
let v = vec![false, true]
let s = \"abc\"
println(!yes())
println(!f.on)
println(!f.get())
println(!v[0])
println(!s.contains(\"z\"))
println(!(yes() && f.on))
println(!Flag { on: false }.on)
let mut n = 0
while !(n >= 3) { n += 1 }
println(n)
if !v[1] { println(\"no\") } else { println(\"yes\") }
",
        &[
            "false", "false", "false", "true", "true", "false", "true", "3", "yes",
        ],
    );
}

#[test]
fn not_equal_and_the_vec_macro_keep_their_bang() {
    assert_lines_everywhere(
        "let a = 1
let b = 2
println(a != b)
println(a!=a)
println(!(a != b))
let v = vec![1, 2, 3]
println(v.len() != 2)
println(vec![true][0] != false)
",
        &["true", "false", "false", "true", "true"],
    );
}

#[test]
fn the_old_words_are_ordinary_names() {
    assert_lines_everywhere(
        "let and = 1
let or = 2
let mut not = 3
not += 1
struct Gate { and: int, or: int }
fn not_of(x: bool) -> bool { !x }
let g = Gate { and: 10, or: 20 }
println(and + or + not)
println(g.and + g.or)
println(not_of(false))
let needs = 7
println(needs)
",
        &["7", "30", "true", "7"],
    );
}

#[test]
fn a_function_named_not_is_still_called_without_a_space() {
    assert_lines_everywhere(
        "fn not(x: int) -> int { x * 2 }
println(not(21))
",
        &["42"],
    );
}

#[test]
fn the_words_inside_strings_and_comments_are_text() {
    assert_eq!(
        run_ok(
            "// a and b or not c
let t = \"a and b or not c\"
println(t)
/* not and or */
println(\"{t.len()}\")
",
            &[]
        ),
        ["a and b or not c", "16"]
    );
}

#[test]
fn and_as_an_operator_names_the_replacement() {
    for source in [
        "let a = true\nlet b = false\nprintln(a and b)\n",
        "fn f(a: bool, b: bool) -> bool {\n    a and b\n}\nprintln(f(true, true))\n",
        "let x = 3\nif x > 1 and x < 5 { println(x) }\n",
        "let mut i = 0\nwhile i < 3 and true { i += 1 }\n",
    ] {
        let stderr = compile_error(source);
        assert_diagnostic(&stderr, "E0117", "write `a && b`");
        assert!(stderr.contains("`and` is not an operator"), "{stderr}");
    }
}

#[test]
fn or_as_an_operator_names_the_replacement() {
    for source in [
        "let a = true\nprintln(a or false)\n",
        "let f = fn(a: bool) -> bool { a or false }\nprintln(f(true))\n",
        "let x = 3\nif x < 1 or x > 2 { println(x) }\n",
    ] {
        let stderr = compile_error(source);
        assert_diagnostic(&stderr, "E0117", "write `a || b`");
        assert!(stderr.contains("`or` is not an operator"), "{stderr}");
    }
}

#[test]
fn not_as_an_operator_names_the_replacement() {
    for source in [
        "let a = true\nprintln(not a)\n",
        "let a = true\nif not a { println(1) }\n",
        "println(not (1 == 2))\n",
        "fn f(v: Vec<bool>) -> bool { not v[0] }\nprintln(f(vec![true]))\n",
        "println(not true)\n",
    ] {
        let stderr = compile_error(source);
        assert_diagnostic(&stderr, "E0117", "write `!x`");
        assert!(stderr.contains("`not` is not an operator"), "{stderr}");
    }
}
