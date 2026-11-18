mod t5_support;

use t5_support::{assert_lines_everywhere, compile_error};

const HELPERS: &str = r#"
fn r(n: int) -> Result<int, string> { if n > 0 { Ok(n) } else { Err("bad {n}") } }
fn o(n: int) -> Option<int> { if n > 0 { Some(n) } else { None } }
fn s(n: int) -> Result<string, string> { if n > 0 { Ok("s{n}") } else { Err("no s") } }
"#;

fn with_helpers(body: &str) -> String {
    format!("{HELPERS}{body}")
}

#[test]
fn int_operand_inside_a_unit_function() {
    let source = with_helpers(
        r#"
fn save(n: int) -> Result<unit, string> { let x = r(n)?; println("saved {x}"); Ok(()) }
fn show(x: Result<unit, string>) -> string { match x { Ok(_) => "ok", Err(e) => e } }
println(show(save(3)))
println(show(save(0)))
"#,
    );
    assert_lines_everywhere(&source, &["saved 3", "ok", "bad 0"]);
}

#[test]
fn string_operand_inside_a_vec_function() {
    let source = with_helpers(
        r#"
fn lens(n: int) -> Result<Vec<int>, string> { let t = s(n)?; Ok(vec![t.len(), 7]) }
fn show(x: Result<Vec<int>, string>) -> string { match x { Ok(v) => "{v[0]},{v[1]}", Err(e) => e } }
println(show(lens(12)))
println(show(lens(-1)))
"#,
    );
    assert_lines_everywhere(&source, &["3,7", "no s"]);
}

#[test]
fn option_operand_inside_an_option_of_another_type() {
    let source = with_helpers(
        r#"
fn label(n: int) -> Option<string> { let v = o(n)?; Some("v{v}") }
fn flag(n: int) -> Option<bool> { let v = o(n)?; Some(v > 2) }
println(match label(4) { Some(t) => t, None => "none" })
println(match label(0) { Some(t) => t, None => "none" })
println(match flag(3) { Some(b) => "{b}", None => "none" })
println(match flag(-3) { Some(b) => "{b}", None => "none" })
"#,
    );
    assert_lines_everywhere(&source, &["v4", "none", "true", "none"]);
}

#[test]
fn generic_operand_success_type() {
    let source = with_helpers(
        r#"
fn present<T>(x: Result<T, string>) -> Result<bool, string> { let _v = x?; Ok(true) }
fn some_len<T>(x: Option<T>) -> Option<int> { let _v = x?; Some(1) }
println(match present(r(1)) { Ok(b) => "{b}", Err(e) => e })
println(match present(s(0)) { Ok(b) => "{b}", Err(e) => e })
println(match some_len(o(2)) { Some(n) => "{n}", None => "none" })
println(match some_len(o(0)) { Some(n) => "{n}", None => "none" })
"#,
    );
    assert_lines_everywhere(&source, &["true", "no s", "1", "none"]);
}

#[test]
fn method_loop_and_match_arm_contexts() {
    let source = with_helpers(
        r#"
struct W { k: int }
impl W {
    fn m(self, n: int) -> Result<string, string> { let v = r(n)?; Ok("m{v + self.k}") }
}
fn in_loop(n: int) -> Result<string, string> {
    let mut acc = 0
    for i in 0..3 { acc = acc + r(n - i)? }
    Ok("acc {acc}")
}
fn in_arm(n: int) -> Option<bool> {
    let z = match n { 0 => o(n)?, _ => o(n)? + 1 }
    Some(z > 2)
}
let w = W { k: 10 }
println(match w.m(5) { Ok(t) => t, Err(e) => e })
println(match w.m(-5) { Ok(t) => t, Err(e) => e })
println(match in_loop(5) { Ok(t) => t, Err(e) => e })
println(match in_loop(1) { Ok(t) => t, Err(e) => e })
println(match in_arm(3) { Some(b) => "{b}", None => "none" })
println(match in_arm(-3) { Some(b) => "{b}", None => "none" })
"#,
    );
    assert_lines_everywhere(
        &source,
        &["m15", "bad -5", "acc 12", "bad 0", "true", "none"],
    );
}

#[test]
fn closure_contexts_typed_and_inferred() {
    let source = with_helpers(
        r#"
fn typed(n: int) -> int {
    let f = fn(k: int) -> Result<bool, string> { let v = r(k)?; Ok(v > 1) }
    match f(n) { Ok(b) => if b { 1 } else { 2 }, Err(e) => e.len() }
}
let inferred = fn(k: int) { let v = r(k)?; Ok(v > 1) }
let opt = fn(k: int) { let v = o(k)?; Some("x{v}") }
let param = fn(x) -> Result<int, string> { let v = x?; Ok(v + 1) }
println(typed(5))
println(typed(1))
println(typed(0))
println(match inferred(5) { Ok(b) => "{b}", Err(e) => e })
println(match inferred(0) { Ok(b) => "{b}", Err(e) => e })
println(match opt(5) { Some(t) => t, None => "none" })
println(match opt(0) { Some(t) => t, None => "none" })
println(match param(r(4)) { Ok(v) => "{v}", Err(e) => e })
println(match param(r(-4)) { Ok(v) => "{v}", Err(e) => e })
"#,
    );
    assert_lines_everywhere(
        &source,
        &["1", "2", "5", "true", "bad 0", "x5", "none", "5", "bad -4"],
    );
}

#[test]
fn question_mark_in_a_discarded_binding_still_propagates() {
    let source = with_helpers(
        r#"
fn same(x: Result<string, string>) -> Result<string, string> { let _v = x?; Ok("kept") }
fn other(x: Result<string, string>) -> Result<int, string> { let _ = x?; Ok(1) }
fn statement(x: Result<int, string>) -> Result<int, string> { x?; Ok(2) }
println(match same(s(0)) { Ok(t) => t, Err(e) => e })
println(match same(s(1)) { Ok(t) => t, Err(e) => e })
println(match other(s(0)) { Ok(v) => "{v}", Err(e) => e })
println(match statement(r(0)) { Ok(v) => "{v}", Err(e) => e })
println(match statement(r(1)) { Ok(v) => "{v}", Err(e) => e })
"#,
    );
    assert_lines_everywhere(&source, &["no s", "kept", "no s", "bad 0", "2"]);
}

#[test]
fn from_conversion_with_a_different_success_type() {
    let source = r#"
struct ParseErr { line: int }
struct AppErr { line: int }
impl From<ParseErr> for AppErr {
    fn from(source: ParseErr) -> AppErr { AppErr { line: source.line + 100 } }
}
fn parse(n: int) -> Result<int, ParseErr> { if n > 0 { Ok(n) } else { Err(ParseErr { line: 3 }) } }
fn run(n: int) -> Result<string, AppErr> { let v = parse(n)?; Ok("parsed {v}") }
println(match run(7) { Ok(t) => t, Err(e) => "line {e.line}" })
println(match run(0) { Ok(t) => t, Err(e) => "line {e.line}" })
"#;
    assert_lines_everywhere(source, &["parsed 7", "line 103"]);
}

#[test]
fn a_success_type_mismatch_is_reported_where_the_value_is_used() {
    let message = compile_error(&with_helpers(
        "fn wrong() -> Result<string, string> { let v = r(1)?; let t: string = v; Ok(t) }\nlet _ = wrong()\n",
    ));
    assert!(message.contains("error[E0301]"), "{message}");
    assert!(message.contains("expected string, found int"), "{message}");
}

#[test]
fn question_mark_at_script_top_level_is_e0305() {
    let message = compile_error(&with_helpers("let x = r(0)?\nprintln(x)\n"));
    assert!(message.contains("error[E0305]"), "{message}");
    assert!(message.contains("top level of a script"), "{message}");
    assert!(!message.contains("dynamic"), "{message}");
    let message = compile_error(&with_helpers("let y = o(0)?\nprintln(y)\n"));
    assert!(message.contains("error[E0305]"), "{message}");
    assert!(!message.contains("dynamic"), "{message}");
}

#[test]
fn question_mark_in_a_plain_function_is_e0305_with_a_repair() {
    let message = compile_error(&with_helpers(
        "fn f() -> int { let x = r(1)?; x }\nprintln(f())\n",
    ));
    assert!(message.contains("error[E0305]"), "{message}");
    assert!(message.contains("help:"), "{message}");
}

#[test]
fn question_mark_on_a_plain_value_is_e0306() {
    let message =
        compile_error("fn f() -> Result<int, string> { let x = 5?; Ok(x) }\nlet _ = f()\n");
    assert!(message.contains("error[E0306]"), "{message}");
    assert!(
        message.contains("the operand of '?' must be an Option or a Result, found int"),
        "{message}"
    );
}

#[test]
fn optimizer_keeps_a_question_mark_whose_value_is_unused() {
    let source = with_helpers(
        r#"
fn same(x: Result<string, string>) -> Result<string, string> { let _v = x?; Ok("kept") }
fn arm(x: Option<int>) -> Option<int> { let _v = match 1 { _ => { let y = x?; y } }; Some(5) }
println(match same(s(0)) { Ok(t) => t, Err(e) => e })
println(match same(s(1)) { Ok(t) => t, Err(e) => e })
println(match arm(o(0)) { Some(v) => "{v}", None => "none" })
println(match arm(o(1)) { Some(v) => "{v}", None => "none" })
"#,
    );
    assert_lines_everywhere(&source, &["no s", "kept", "none", "5"]);
}
