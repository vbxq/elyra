mod t9_support;

use t9_support::{assert_every_level, run_with};

fn nested_parens(depth: usize) -> String {
    format!(
        "let x = 1\nlet y = {}x{}\nprintln(y)\n",
        "(".repeat(depth),
        " + 1)".repeat(depth)
    )
}

#[test]
fn eighteen_nested_parentheses_compile() {
    assert_every_level(&nested_parens(18), &["19"], Some(0));
}

#[test]
fn two_hundred_fifty_five_nested_parentheses_compile_at_every_level() {
    assert_every_level(&nested_parens(255), &["256"], Some(0));
}

#[test]
fn nesting_past_the_limit_is_rejected_with_e0105() {
    let outcome = run_with(&nested_parens(300), &[]);
    assert_eq!(outcome.code, Some(1), "{}", outcome.stderr);
    assert!(
        outcome.stderr.contains("E0105") && outcome.stderr.contains("max 256"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn deeply_nested_calls_compile_and_inline() {
    let source = format!(
        "fn f(a: int) -> int {{ a + 1 }}\nfn g(a: int) -> int {{\n    let b = a + 1\n    return b\n}}\nprintln({}0{})\nprintln({}0{})\n",
        "f(".repeat(250),
        ")".repeat(250),
        "g(".repeat(250),
        ")".repeat(250)
    );
    assert_every_level(&source, &["250", "250"], Some(0));
}

#[test]
fn deeply_nested_if_expressions_compile() {
    let source = format!(
        "let x = 1\nlet y = {}x{}\nprintln(y)\n",
        "if x > 0 { ".repeat(200),
        " } else { 0 }".repeat(200)
    );
    assert_every_level(&source, &["1"], Some(0));
}

#[test]
fn deeply_nested_blocks_compile() {
    let source = format!(
        "let mut x = 0\n{}x = 7\n{}println(x)\n",
        "if x == 0 {\n".repeat(250),
        "}\n".repeat(250)
    );
    assert_every_level(&source, &["7"], Some(0));
}

#[test]
fn deeply_nested_array_literals_compile() {
    let source = format!(
        "let y = {}1{}\nprintln(1)\n",
        "[".repeat(250),
        "]".repeat(250)
    );
    assert_every_level(&source, &["1"], Some(0));
}

#[test]
fn a_thousand_term_chain_compiles() {
    let source = format!("let x = 1\nlet y = x{}\nprintln(y)\n", " + x".repeat(1000));
    assert_every_level(&source, &["1001"], Some(0));
}

#[test]
fn unary_chains_past_the_limit_are_rejected_instead_of_overflowing() {
    let source = format!("let x = 5\nlet y = {}x\nprintln(y)\n", "~ ".repeat(1000));
    let outcome = run_with(&source, &[]);
    assert_eq!(outcome.code, Some(1), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("E0105"), "{}", outcome.stderr);
}

#[test]
fn unary_chains_within_the_limit_compile() {
    let source = format!("let x = 5\nlet y = {}x\nprintln(y)\n", "~ ".repeat(250));
    assert_every_level(&source, &["5"], Some(0));
}
