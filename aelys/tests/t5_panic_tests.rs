mod t5_support;

use t5_support::{Outcome, run, run_with};

const PANIC_EXIT: Option<i32> = Some(101);

fn both(source: &str) -> [Outcome; 2] {
    [run(source), run_with(source, &["-ae.jit=on"])]
}

#[test]
fn panic_has_type_never_in_an_int_match_arm() {
    let source = r#"fn pick(n: int) -> int {
    match n {
        0 => 10,
        1 => panic("bad lump index {n}"),
        _ => 30,
    }
}
println(pick(0))
println(pick(2))
println(pick(1))
println("unreachable")
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["10", "30"], "{}", outcome.stderr);
        assert_eq!(outcome.code, PANIC_EXIT, "{}", outcome.stderr);
        assert!(
            outcome.stderr.starts_with("panic: bad lump index 1\n"),
            "{}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains(".aelys:4:"), "{}", outcome.stderr);
        assert!(
            outcome
                .stderr
                .contains("stack trace (most recent call first):"),
            "{}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains("  pick ("), "{}", outcome.stderr);
    }
}

#[test]
fn panic_fits_every_expression_position() {
    let source = r#"fn name(n: int) -> string { if n > 0 { "pos" } else { panic("negative {n}") } }
fn total(v: Vec<int>) -> int {
    let mut t = 0
    for x in v { t = t + if x < 100 { x } else { panic("too big {x}") } }
    t
}
fn check(flag: bool) { if flag == false { panic("flag unset") } }
check(true)
println(name(1))
println(total(vec![1, 2, 3]))
fn inv(k: float) -> float { if k == 0.0 { panic("zero") } else { 1.0 / k } }
println(inv(4.0))
println(total(vec![1, 200]))
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["pos", "6", "0.25"], "{}", outcome.stderr);
        assert_eq!(outcome.code, PANIC_EXIT);
        assert!(
            outcome.stderr.starts_with("panic: too big 200\n"),
            "{}",
            outcome.stderr
        );
    }
}

#[test]
fn panic_never_unwinds_through_question_mark() {
    let source = r#"fn inner(n: int) -> Result<int, string> {
    if n == 0 { panic("inner gave up") }
    Ok(n)
}
fn outer(n: int) -> Result<int, string> { let v = inner(n)?; Ok(v + 1) }
println(match outer(1) { Ok(v) => v, Err(_) => -1 })
println(match outer(0) { Ok(v) => v, Err(_) => -1 })
println("unreachable")
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["2"], "{}", outcome.stderr);
        assert_eq!(outcome.code, PANIC_EXIT);
        assert!(outcome.stderr.starts_with("panic: inner gave up\n"));
        assert!(outcome.stderr.contains("  inner ("), "{}", outcome.stderr);
        assert!(outcome.stderr.contains("  outer ("), "{}", outcome.stderr);
    }
}

#[test]
fn assert_passes_silently_and_fails_with_its_message() {
    let source = r#"fn half(n: int) -> int {
    assert(n % 2 == 0, "odd input {n}")
    n / 2
}
println(half(8))
println(half(7))
println("unreachable")
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["4"], "{}", outcome.stderr);
        assert_eq!(outcome.code, PANIC_EXIT);
        assert!(
            outcome
                .stderr
                .starts_with("panic: assertion failed: odd input 7\n"),
            "{}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains(".aelys:2:"), "{}", outcome.stderr);
        assert!(outcome.stderr.contains("  half ("), "{}", outcome.stderr);
    }
}

#[test]
fn assert_in_a_hot_loop_under_the_jit() {
    let source = r#"fn advance(i: int) -> int {
    assert(i < 150000, "limit reached at {i}")
    i + 1
}
let mut acc = 0
let mut i = 0
while i < 149000 { acc = acc + advance(i); i = i + 1 }
println(acc)
while true { acc = advance(i); i = i + 1 }
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["11100574500"], "{}", outcome.stderr);
        assert_eq!(outcome.code, PANIC_EXIT);
        assert!(
            outcome
                .stderr
                .starts_with("panic: assertion failed: limit reached at 150000\n"),
            "{}",
            outcome.stderr
        );
    }
}

#[test]
fn panic_and_assert_are_type_checked() {
    for (source, needle) in [
        ("panic(3)\n", "expected string, found int"),
        ("assert(1, \"x\")\n", "expected bool, found int"),
        ("assert(true)\n", "wrong number of arguments"),
    ] {
        let outcome = run(source);
        assert_eq!(outcome.code, Some(1), "{source}: {}", outcome.stderr);
        assert!(
            outcome.stderr.contains(needle),
            "{source}: {}",
            outcome.stderr
        );
    }
}

#[test]
fn user_declarations_shadow_the_builtins() {
    let source = r#"fn panic(msg: string) -> int { msg.len() }
fn run() -> int {
    let assert = fn(a: bool, b: string) -> int { b.len() + 10 }
    println(assert(false, "yes"))
    panic("four")
}
println(run())
"#;
    let outcome = run(source);
    assert_eq!(outcome.code, Some(0), "{}", outcome.stderr);
    assert_eq!(outcome.lines(), ["13", "4"]);
}

#[test]
fn unwrap_on_err_prints_the_string_payload() {
    let source = r#"fn r() -> Result<int, string> { Err("disk on fire") }
println("before")
let v = r().unwrap()
println("after {v}")
"#;
    for outcome in both(source) {
        assert_eq!(outcome.lines(), ["before"]);
        assert_eq!(outcome.code, Some(1));
        assert!(
            outcome
                .stderr
                .contains("Result::unwrap failed on Err: disk on fire\n"),
            "{}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains(".aelys:3:"), "{}", outcome.stderr);
    }
}

#[test]
fn expect_on_err_prints_the_message_and_the_payload() {
    let source = r#"fn r() -> Result<int, string> { Err("short read") }
fn load() -> int { r().expect("loading WAD") }
println(load())
"#;
    for outcome in both(source) {
        assert_eq!(outcome.code, Some(1));
        assert!(
            outcome.stderr.contains("loading WAD: short read\n"),
            "{}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains("  load ("), "{}", outcome.stderr);
    }
}

#[test]
fn unwrap_renders_user_enum_struct_and_collection_payloads() {
    let cases = [
        (
            "enum WadError { Missing(string), Short { got: int, want: int }, Denied }\nfn r() -> Result<int, WadError> { Err(WadError::Short { got: 3, want: 4 }) }\nlet _ = r().unwrap()\n",
            "Result::unwrap failed on Err: WadError::Short { got: 3, want: 4 }\n",
        ),
        (
            "enum WadError { Missing(string), Denied }\nfn r() -> Result<int, WadError> { Err(WadError::Missing(\"a.wad\")) }\nlet _ = r().expect(\"open\")\n",
            "open: WadError::Missing(\"a.wad\")\n",
        ),
        (
            "enum WadError { Missing(string), Denied }\nfn r() -> Result<int, WadError> { Err(WadError::Denied) }\nlet _ = r().unwrap()\n",
            "Result::unwrap failed on Err: WadError::Denied\n",
        ),
        (
            "struct P { x: int, y: int }\nfn r() -> Result<int, P> { Err(P { x: 1, y: 2 }) }\nlet _ = r().unwrap()\n",
            "Result::unwrap failed on Err: P { x: 1, y: 2 }\n",
        ),
        (
            "fn r() -> Result<int, Vec<int>> { Err(vec![5, 6]) }\nlet _ = r().unwrap()\n",
            "Result::unwrap failed on Err: vec![5, 6]\n",
        ),
        (
            "fn r() -> Result<string, int> { Err(42) }\nlet _ = r().unwrap()\n",
            "Result::unwrap failed on Err: 42\n",
        ),
    ];
    for (source, expected) in cases {
        for outcome in both(source) {
            assert_eq!(outcome.code, Some(1), "{source}");
            assert!(
                outcome.stderr.contains(expected),
                "{source}\nexpected {expected:?} in {}",
                outcome.stderr
            );
        }
    }
}

#[test]
fn option_unwrap_and_expect_keep_their_messages() {
    let outcome = run("fn o() -> Option<int> { None }\nlet _ = o().unwrap()\n");
    assert_eq!(outcome.code, Some(1));
    assert!(
        outcome
            .stderr
            .contains("Option::unwrap failed: value was not the success variant"),
        "{}",
        outcome.stderr
    );
    let outcome =
        run("fn o() -> Option<int> { None }\nlet _ = o().expect(\"config port missing\")\n");
    assert_eq!(outcome.code, Some(1));
    assert!(
        outcome.stderr.contains("error: config port missing\n"),
        "{}",
        outcome.stderr
    );
}
