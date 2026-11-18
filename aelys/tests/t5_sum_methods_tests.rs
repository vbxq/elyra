mod t5_support;

use t5_support::{assert_lines_everywhere, compile_error};

const HELPERS: &str = r#"
fn r(n: int) -> Result<int, string> { if n > 0 { Ok(n) } else { Err("bad {n}") } }
fn o(n: int) -> Option<int> { if n > 0 { Some(n) } else { None } }
"#;

#[test]
fn is_ok_is_err_is_some_is_none() {
    let source = format!(
        r#"{HELPERS}
println("{{r(1).is_ok()}} {{r(0).is_ok()}} {{r(1).is_err()}} {{r(0).is_err()}}")
println("{{o(1).is_some()}} {{o(0).is_some()}} {{o(1).is_none()}} {{o(0).is_none()}}")
if r(5).is_ok() && o(0).is_none() {{ println("both") }}
let mut n = 3
while o(n).is_some() {{ n = n - 1 }}
println(n)
"#
    );
    assert_lines_everywhere(
        &source,
        &[
            "true false false true",
            "true false false true",
            "both",
            "0",
        ],
    );
}

#[test]
fn ok_or_converts_an_option_into_a_result() {
    let source = format!(
        r#"{HELPERS}
println(match o(3).ok_or("missing") {{ Ok(v) => "ok {{v}}", Err(e) => e }})
println(match o(0).ok_or("missing") {{ Ok(v) => "ok {{v}}", Err(e) => e }})
fn load(n: int) -> Result<string, string> {{ let v = o(n).ok_or("no lump {{n}}")?; Ok("lump {{v * 10}}") }}
println(match load(4) {{ Ok(t) => t, Err(e) => e }})
println(match load(0) {{ Ok(t) => t, Err(e) => e }})
enum E {{ Absent(int) }}
println(match o(0).ok_or(E::Absent(9)) {{ Ok(v) => v, Err(e) => match e {{ E::Absent(k) => k }} }})
"#
    );
    assert_lines_everywhere(&source, &["ok 3", "missing", "lump 40", "no lump 0", "9"]);
}

#[test]
fn methods_on_receivers_inferred_later() {
    let source = format!(
        r#"{HELPERS}
let failed = fn(x) {{ x.is_err() }}
let empty = fn(x) {{ x.is_none() }}
let lift = fn(x) {{ x.ok_or(7) }}
println(failed(r(0)))
println(failed(r(2)))
println(empty(o(0)))
println(match lift(o(2)) {{ Ok(v) => v, Err(e) => e }})
println(match lift(o(0)) {{ Ok(v) => v, Err(e) => e }})
"#
    );
    assert_lines_everywhere(&source, &["true", "false", "true", "2", "7"]);
}

#[test]
fn methods_reject_the_other_family() {
    for (source, method, receiver) in [
        ("println(o(1).is_ok())", "is_ok", "Option<int>"),
        ("println(o(1).is_err())", "is_err", "Option<int>"),
        ("println(r(1).is_some())", "is_some", "Result<int, string>"),
        ("println(r(1).is_none())", "is_none", "Result<int, string>"),
        ("println(r(1).ok_or(3))", "ok_or", "Result<int, string>"),
    ] {
        let message = compile_error(&format!("{HELPERS}{source}\n"));
        assert!(message.contains("error[E0309]"), "{source}: {message}");
        assert!(
            message.contains(&format!("method '{method}' is not available on {receiver}")),
            "{source}: {message}"
        );
    }
}

#[test]
fn ok_or_result_must_still_be_used() {
    let message = compile_error(&format!("{HELPERS}o(1).ok_or(\"x\")\n"));
    assert!(message.contains("error[E0303]"), "{message}");
}
