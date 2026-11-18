mod t9_support;

use t9_support::{assert_lines_everywhere, compile_error, run_project};

#[test]
fn a_lambda_is_the_tail_expression_of_functions_blocks_arms_and_lambdas() {
    let source = "struct Base { k: int }
impl Base {
    fn adder(self) -> fn(int) -> int {
        fn(x: int) -> int { x + self.k }
    }
}
fn adder(n: int) -> fn(int) -> int {
    fn(x: int) -> int { x + n }
}
fn constant() -> fn() -> int {
    fn() -> int { 0 }
}
fn pick(o: Option<int>) -> fn() -> int {
    match o {
        Some(v) => fn() -> int { v * 2 },
        None => fn() -> int { -1 },
    }
}
fn branch(c: bool) -> fn(int) -> int {
    if c {
        fn(x: int) -> int { x * 10 }
    } else {
        fn(x: int) -> int { x - 10 }
    }
}
fn generic<T>(seed: T) -> fn() -> T {
    fn() -> T { seed }
}
fn curried() -> fn(int) -> fn(int) -> int {
    fn(a: int) -> fn(int) -> int {
        fn(b: int) -> int { a * 100 + b }
    }
}
let block_value = {
    let m = 3
    fn(x: int) -> int { x * m }
}
println(adder(2)(5))
println(constant()())
println(pick(Some(4))())
println(pick(None)())
println(branch(true)(3))
println(branch(false)(3))
println(generic(true)())
println(curried()(4)(2))
println(block_value(7))
println(Base { k: 40 }.adder()(2))
";
    assert_lines_everywhere(
        source,
        &["7", "0", "8", "-1", "30", "-7", "true", "402", "21", "42"],
    );
}

#[test]
fn a_named_fn_statement_is_still_a_declaration() {
    let source = "fn outer() -> int {
    fn inner(x: int) -> int { x + 1 }
    inner(1)
}
pub fn exported() -> int { 5 }
println(outer())
println(exported())
";
    assert_lines_everywhere(source, &["2", "5"]);
}

#[test]
fn from_is_an_ordinary_identifier_outside_needs() {
    let source = "struct Span { from: int, to: int }
struct Wrap { v: int }
impl From<int> for Wrap {
    fn from(v: int) -> Wrap { Wrap { v: v * 2 } }
}
struct Cursor { at: int }
impl Cursor {
    fn from(self) -> int { self.at }
}
fn span(from: int, to: int) -> int { to - from }
fn lift(x: int) -> Result<int, Wrap> {
    let r: Result<int, int> = Err(x)
    let v = r?
    Ok(v)
}
let from = 3
let s = Span { from: 2, to: 9 }
let shifted = fn(from: int) -> int { from + 1 }
let mut total = 0
for from in 0..3 {
    total += from
}
let picked = match Some(8) {
    Some(from) => from,
    None => 0,
}
println(span(3, 10))
println(from)
println(s.from + s.to)
println(Cursor { at: 2 }.from())
println(shifted(from))
println(total)
println(picked)
println(Wrap::from(5).v)
println(match lift(4) { Ok(_) => 0, Err(w) => w.v })
";
    assert_lines_everywhere(source, &["7", "3", "11", "2", "4", "3", "8", "10", "8"]);
}

#[test]
fn needs_from_still_imports_symbols() {
    let files = [
        (
            "util.aelys",
            "pub fn twice(x: int) -> int { x * 2 }\npub fn thrice(x: int) -> int { x * 3 }\n",
        ),
        ("other.aelys", "pub fn half(x: int) -> int { x / 2 }\n"),
        (
            "main.aelys",
            "needs half from other\nneeds thrice, twice from util\nlet from = 1\nprintln(twice(4) + thrice(from) + half(10))\n",
        ),
    ];
    let outcome = run_project(&files, "main.aelys", &[]);
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    assert_eq!(outcome.lines(), ["16"]);
}

#[test]
fn for_loops_destructure_tuple_and_struct_patterns() {
    let source = "struct P { x: int, y: int }
fn sum_keys(v: Vec<(int, string)>) -> int {
    let mut s = 0
    for (k, _) in v {
        s += k
    }
    s
}
fn firsts<T>(v: Vec<(T, int)>) -> Vec<T> {
    let mut out: Vec<T> = vec![]
    for (t, _) in v {
        out.push(t)
    }
    out
}
let pairs: Vec<(int, string)> = vec![(1, \"a\"), (2, \"b\"), (3, \"c\")]
let mut line = \"\"
for (k, name) in pairs {
    line = line + \"{k}{name} \"
}
println(line)
println(sum_keys(pairs))
let ps = vec![P { x: 1, y: 2 }, P { x: 3, y: 4 }]
let mut t = 0
for P { x, y } in ps {
    t += x * 10 + y
}
println(t)
let nested: Vec<((int, int), int)> = vec![((1, 2), 3), ((4, 5), 6)]
let mut u = 0
for ((a, b), c) in &nested {
    u += a * 100 + b * 10 + c
}
println(u)
let mut fs: Vec<fn() -> int> = vec![]
for (k, _) in pairs {
    fs.push(fn() -> int { k * 7 })
}
println(fs[0]() + fs[1]() + fs[2]())
let mut seen = 0
'outer: for (k, _) in pairs {
    for (j, _) in pairs {
        if j == 2 { continue 'outer }
        if k == 3 { break 'outer }
        seen += k * 10 + j
    }
}
println(seen)
println(firsts(vec![(true, 1), (false, 2)]).len())
let arr = [(1, 1), (2, 4)]
let mut sq = 0
for (_, s) in arr {
    sq += s
}
println(sq)
";
    assert_lines_everywhere(
        source,
        &["1a 2b 3c ", "6", "46", "579", "42", "32", "2", "5"],
    );
}

#[test]
fn a_refutable_for_pattern_is_rejected_by_name() {
    let stderr =
        compile_error("let v = vec![Some(1), None]\nfor Some(x) in v {\n    println(x)\n}\n");
    assert!(stderr.contains("error[E0108]"), "{stderr}");
    assert!(stderr.contains("refutable pattern in `for`"), "{stderr}");
    let stderr = compile_error(
        "enum E { A(int), B }\nlet v = vec![(E::A(1), 2)]\nfor (E::A(x), y) in v {\n    println(x + y)\n}\n",
    );
    assert!(stderr.contains("error[E0108]"), "{stderr}");
}

#[test]
fn for_pattern_bindings_are_immutable() {
    let stderr = compile_error(
        "struct P { x: int }\nlet mut ps = vec![(P { x: 1 }, 2)]\nfor (p, _) in ps {\n    p.x = 5\n}\n",
    );
    assert!(stderr.contains("error[E0330]"), "{stderr}");
}
