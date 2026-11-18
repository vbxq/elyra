use std::fs;
use std::path::{Path, PathBuf};

use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime,
    new_vm_with_config, run_with_vm_and_opt,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use aelys_runtime::{Value, VmConfig};
use tempfile::TempDir;

fn assert_int_everywhere(source: &str, expected: i64) {
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        for mode in [JitMode::Off, JitMode::Tiered] {
            let runtime = Runtime::with_jit_mode(mode);
            let module = runtime
                .compile(
                    source,
                    CompileOptions {
                        optimization_level: level,
                        source_name: "<t2-merge>".to_string(),
                    },
                )
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}\n{source}"));
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            let outcome = isolate
                .execute(&module, RunOptions::default())
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}\n{source}"));
            assert_eq!(
                outcome,
                ExecutionOutcome::Returned(Value::int(expected)),
                "{level:?} {mode:?}\n{source}"
            );
        }
    }
}

fn assert_rejected(source: &str, code: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains(code),
                "expected {code}:\n{message}\n{source}"
            );
            message
        }
    }
}

fn write(root: &Path, path: &str, content: &str) -> PathBuf {
    let file = root.join(path);
    fs::write(&file, content).expect("write fixture");
    file
}

fn run_module_int(files: &[(&str, &str)], main: &str) -> i64 {
    let dir = TempDir::new().expect("temp dir");
    for (path, content) in files {
        write(dir.path(), path, content);
    }
    let path = write(dir.path(), "main.aelys", main);
    let value = run_file(&path).unwrap_or_else(|error| panic!("main must run:\n{error}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("main must end on an int, got {value:?}"))
}

#[test]
fn char_literals_and_labels_share_the_quote() {
    assert_int_everywhere(
        r#"
let a = 'a'
let nl = '\n'
let e = '\u{e9}'
let mut k = 0
'outer: loop {
    k += 1
    break 'outer
}
'rows: for i in 0..3 {
    for j in 0..3 {
        if j == 1 { continue 'rows }
        k += 10
    }
}
(a as int) + (nl as int) + (e as int) + k
"#,
        97 + 10 + 233 + 31,
    );
    assert_int_everywhere(
        "let n = 'outer: loop { break 'outer 7 }\nlet q = '\\''\nn + (q as int)",
        7 + 39,
    );
    assert_rejected("let c = 'a\n0", "E0102");
    let message = assert_rejected("let c = 'ab'\n0", "exactly one character");
    assert!(!message.contains("loop label"), "{message}");
}

#[test]
fn a_char_match_needs_a_wildcard_and_is_matched_by_value() {
    let message = assert_rejected(
        "fn f(c: char) -> int { match c { 'a' => 1, 'b' => 2 } }\nf('a')",
        "E0302",
    );
    assert!(message.contains("missing _"), "{message}");
    assert_int_everywhere(
        "fn f(c: char) -> int { match c { 'a' => 1, 'b' | 'c' => 2, _ => 3 } }\nf('a') * 100 + f('c') * 10 + f('z')",
        123,
    );
    assert_int_everywhere(
        "fn f(o: Option<char>) -> int { match o { Some('x') => 1, Some(_) => 2, None => 3 } }\nf(Some('x')) * 100 + f(Some('y')) * 10 + f(None)",
        123,
    );
    assert_rejected(
        "fn f(c: char) -> int { match c { 'a' => 1, 'a' => 2, _ => 3 } }\nf('a')",
        "E0356",
    );
}

#[test]
fn sized_range_patterns_use_the_bounds_of_their_wrapping_type() {
    assert_int_everywhere(
        "fn h(x: u8) -> int { match x { 0..=127 => 0, 128..=255 => 1 } }\nlet a: u8 = 250\nlet b: u8 = a + 10\nh(a) * 10 + h(b)",
        10,
    );
    assert_int_everywhere(
        "fn s(x: i8) -> int { match x { -128..=-1 => 0, 0..=127 => 1 } }\nlet a: i8 = 127\nlet b: i8 = a + 1\ns(a) * 10 + s(b)",
        10,
    );
    assert_int_everywhere(
        "fn w(x: u16) -> int { match x { 0 => 0, 1..=65535 => 1 } }\nlet z: u16 = 65535\nw(z + 1) * 10 + w(z)",
        1,
    );
    let message = assert_rejected(
        "fn h(x: u8) -> int { match x { 0..=127 => 0, 128..=256 => 1 } }\nh(2)",
        "E0301",
    );
    assert!(message.contains("256 does not fit in u8"), "{message}");
    let message = assert_rejected(
        "fn s(x: i8) -> int { match x { -129..=0 => 0, _ => 1 } }\nlet v: i8 = 1\ns(v)",
        "E0301",
    );
    assert!(message.contains("-129 does not fit in i8"), "{message}");
    assert_rejected(
        "fn h(x: u8) -> int { match x { 0..=127 => 0, 128..=254 => 1 } }\nlet v: u8 = 1\nh(v)",
        "E0302",
    );
    assert_rejected(
        "fn h(x: int) -> int { match x { 0..=127 => 0, 128.. => 1 } }\nh(1)",
        "E0302",
    );
    assert_rejected(
        "fn h(x: u64) -> int { match x { 0..=9223372036854775807 => 0 } }\nh(1)",
        "E0302",
    );
}

#[test]
fn a_constant_named_in_a_pattern_matches_its_value() {
    assert_int_everywhere(
        "const LIMIT = 10\nfn f(x: int) -> int { match x { LIMIT => 1, _ => 0 } }\nf(10) * 10 + f(3)",
        10,
    );
    assert_int_everywhere(
        "const LIMIT = 10\nfn f(x: int) -> int { if let LIMIT = x { 1 } else { 0 } }\nf(10) * 10 + f(3)",
        10,
    );
    assert_int_everywhere(
        "const TOP: u8 = 200\nfn f(x: u8) -> int { match x { TOP => 1, 0..=199 => 2, _ => 3 } }\nlet a: u8 = 200\nlet b: u8 = 7\nlet c: u8 = 255\nf(a) * 100 + f(b) * 10 + f(c)",
        123,
    );
    assert_int_everywhere(
        "const NAME = \"bob\"\nconst ON = true\nfn f(o: Option<string>, b: bool) -> int {\n    let x = match o { Some(NAME) => 1, Some(_) => 2, None => 3 }\n    let y = match b { ON => 10, _ => 20 }\n    x + y\n}\nf(Some(\"bob\"), true) * 100 + f(Some(\"al\"), false)",
        11 * 100 + 22,
    );
    assert_int_everywhere(
        "const LIMIT = 10\nfn f(LIMIT: int) -> int { match 4 { LIMIT => LIMIT } }\nf(1)",
        4,
    );
    assert_int_everywhere(
        "const LIMIT = 10\nfn f(x: int) -> int {\n    let mut n = 0\n    let mut v = x\n    while let LIMIT = v { n += 1\n v = 0 }\n    n\n}\nf(10) * 10 + f(3)",
        10,
    );
    assert_rejected(
        "const LIMIT = 10\nenum E { A, B }\nfn f(e: E) -> int { match e { LIMIT => 1, _ => 0 } }\nf(E::A)",
        "E0301",
    );
    assert_rejected(
        "const LIMIT = 10\nfn f(x: int) -> int { match x { LIMIT => 1 } }\nf(1)",
        "E0302",
    );
    assert_rejected(
        "const LIMIT = 300\nfn f(x: u8) -> int { match x { LIMIT => 1, _ => 0 } }\nlet v: u8 = 1\nf(v)",
        "E0301",
    );
    assert_rejected(
        "const HALF = 0.5\nfn f(x: int) -> int { match x { HALF => 1, _ => 0 } }\nf(1)",
        "E0301",
    );
}

#[test]
fn a_statement_match_keeps_discarding_its_arms_under_literal_unification() {
    assert_int_everywhere(
        "fn f(k: int) -> int {\n    match k { 0 => 5, _ => 6 }\n    7\n}\nf(0) + f(1)",
        14,
    );
    assert_int_everywhere(
        "fn f(k: u8) -> int {\n    let mut r: u8 = 0\n    match k { 0 => r = 200, _ => {} }\n    r as int\n}\nlet z: u8 = 0\nf(z)",
        200,
    );
    assert_int_everywhere(
        "fn pick(c: bool) -> u8 { match c { true => 200, false => 100 } }\nlet a = pick(true)\nlet b = pick(true)\n(a + b) as int",
        144,
    );
}

#[test]
fn assignment_tails_make_unit_functions_in_every_place_form() {
    for body in [
        "n = n + 1",
        "(n = n + 1)",
        "v[0] = 1",
        "v[g()] += 1",
        "s.x = 2",
        "s.x += 3",
    ] {
        let program = format!(
            "struct S {{ x: int }}\nfn g() -> int {{ 0 }}\nfn f(mut n: int, mut v: Vec<int>, mut s: S) {{ {body} }}\nlet r = f(1, vec![0], S {{ x: 0 }})\nlet k: int = r\nk"
        );
        assert_rejected(&program, "E0301");
    }
    assert_int_everywhere(
        "fn g() -> int { 0 }\nfn f(mut v: Vec<int>) { v[g()] += 1 }\nlet mut v = vec![5]\nf(v)\nv[0]",
        6,
    );
}

#[test]
fn reserved_names_hold_in_destructuring_and_tuples() {
    let message = assert_rejected("let (__a, b) = (1, 2)\nb", "E0114");
    assert!(message.contains("reserved for the compiler"), "{message}");
    assert_rejected(
        "struct P { x: int }\nlet P { x: __x } = P { x: 1 }\n0",
        "E0114",
    );
    let message = assert_rejected("let AelysTuple2 = 3\n0", "E0394");
    assert!(
        message.contains("cannot be written: names starting with `AelysTuple`"),
        "{message}"
    );
    assert_int_everywhere("let (a, b) = (1, 2)\nlet P = 3\na + b + P", 6);
}

#[test]
fn a_value_block_or_loop_cannot_alias_an_immutable_aggregate() {
    for init in [
        "{ println(0)\n a }",
        "if c { println(1)\n a } else { a }",
        "loop { break a }",
        "'l: loop { loop { break 'l a } }",
        "match 1 { 1 => { println(2)\n a }, _ => vec![0] }",
    ] {
        let program = format!(
            "fn f(c: bool) -> int {{\n    let a = vec![1, 2]\n    let mut b = {init}\n    b.push(3)\n    a.len()\n}}\nf(true)"
        );
        assert_rejected(&program, "E0323");
    }
    assert_int_everywhere(
        "fn f(c: bool) -> int {\n    let a = vec![1, 2]\n    let mut b = loop { break a.clone() }\n    b.push(3)\n    a.len() * 10 + b.len()\n}\nf(true)",
        23,
    );
    assert_int_everywhere(
        "fn f(c: bool) -> int {\n    let mut a = vec![1, 2]\n    let mut b = { println(0)\n a }\n    b.push(3)\n    a.len()\n}\nf(true)",
        3,
    );
}

#[test]
fn closures_captured_in_a_loop_left_by_break_survive_collection() {
    let source = r#"
struct P { x: int }
fn churn(k: int) -> int {
    let mut s = 0
    for j in 0..k {
        let p = P { x: j }
        s = s + p.x
    }
    s
}
let mut fs: Vec<fn() -> int> = vec![]
let mut i = 0
while true {
    let cell = P { x: i * 10 }
    let v = vec![i, i + 1]
    fs.push(fn() -> int { cell.x + v[1] })
    churn(50)
    i += 1
    if i == 6 { break }
    if i % 2 == 0 { continue }
}
'outer: for k in 0..4 {
    let q = P { x: k }
    fs.push(fn() -> int { q.x * 1000 })
    churn(50)
    if k == 2 { break 'outer }
}
churn(200)
let mut total = 0
for f in fs { total = total + f() }
total
"#;
    let expected = (0..6).map(|i| i * 10 + i + 1).sum::<i64>() + 3000;
    for config in [
        VmConfig {
            gc_stress: true,
            gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
            ..VmConfig::default()
        },
        VmConfig {
            gc_verify: true,
            gc_nursery_bytes: 4 * 1024,
            ..VmConfig::default()
        },
    ] {
        let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "<t2-merge>", OptimizationLevel::Standard)
            .unwrap_or_else(|error| panic!("program failed: {error}"));
        assert_eq!(value.as_int(), Some(expected));
    }
}

#[test]
fn step_stays_contextual_beside_sized_for_bounds() {
    assert_int_everywhere(
        "fn step(x: int) -> int { x * 2 }\nlet lo: u8 = 250\nlet mut s = 0\nfor i in lo..255 step 2 { s = s + (i as int) }\ns + step(1)",
        250 + 252 + 254 + 2,
    );
    assert_int_everywhere(
        "struct W { step: int }\nlet w = W { step: 3 }\nlet mut s = 0\nfor i in 0..10 step w.step { s = s + i }\ns",
        18,
    );
}

const PAIRS: &str = r#"
pub struct Stack<T> { pub items: Vec<T> }
impl<T> Stack<T> {
    pub fn pushed(self, v: T) -> Stack<T> {
        let mut out: Vec<T> = vec![]
        for x in self.items { out.push(x) }
        out.push(v)
        Stack { items: out }
    }
    pub fn top(self) -> T { self.items[self.items.len() - 1] }
    pub fn size(self) -> int { self.items.len() }
}
pub fn new_stack<T>() -> Stack<T> { Stack { items: vec![] } }
pub fn first<T>(p: (T, int)) -> T { p.0 }
pub struct Seg { pub a: (int, int), pub b: ((int, string), bool) }
pub fn seg() -> Seg { Seg { a: (1, 2), b: ((3, "xy"), true) } }
pub fn find(n: int) -> Option<(int, string)> { if n > 0 { Some((n, "f")) } else { None } }
fn pick<T>(x: T) -> T { x }
pub fn twice<T>(x: T) -> (T, T) {
    let r = loop { break pick(x) }
    let q = { let z = pick(r)
 z }
    (r, q)
}
"#;

#[test]
fn tuples_cross_modules_through_generic_templates_and_nested_schemas() {
    let main = r#"
use pairs
let s: pairs::Stack<(int, string)> = pairs::new_stack()
let s2 = s.pushed((4, "four")).pushed((5, "five!"))
let (n, word) = s2.top()
let g = pairs::seg()
let ((c, t), flag) = g.b
let f = match pairs::find(9) { Some((k, w)) => k + w.len(), None => 0 }
let h = pairs::first((11, 0))
let (t1, t2) = pairs::twice(100)
n * 1000000 + word.len() * 100000 + s2.size() * 10000 + c * 1000 + t.len() * 100 + f + h + t1 + t2 + (if flag { 0 } else { 1 })
"#;
    assert_eq!(
        run_module_int(&[("pairs.aelys", PAIRS)], main),
        5 * 1_000_000 + 5 * 100_000 + 2 * 10_000 + 3 * 1000 + 2 * 100 + 10 + 11 + 200
    );
}

#[test]
fn the_showcase_and_audit_if_reproducers_give_their_values() {
    assert_int_everywhere(
        "let s = if true { println(\"side effect\"); 5 } else { 6 }\ns",
        5,
    );
    assert_int_everywhere(
        "fn st(flag: bool, n: int) -> int {\n    let s = if flag {\n        n + 1\n    } else {\n        n - 1\n    }\n    s\n}\nst(true, 3)",
        4,
    );
    assert_int_everywhere(
        "enum S { L(int), E(int) }\nfn st(flag: bool, n: int) -> Result<S, string> {\n    let s = if flag {\n        S::L(n)\n    } else {\n        S::E(n)\n    }\n    Ok(s)\n}\nmatch st(true, 3) { Ok(s) => match s { S::L(v) => v, S::E(v) => -v }, Err(_) => 0 }",
        3,
    );
    assert_int_everywhere(
        "fn f(c: int) -> int {\n    if c == 0 { 10 } else if c == 1 { 20 } else { 30 }\n}\nf(0) + f(1) * 100 + f(2) * 10000",
        10 + 2000 + 300000,
    );
    assert_int_everywhere(
        "fn g(c: int) -> int {\n    match c {\n        0 => {\n            if c > 1 { 2 } else { 3 }\n        }\n        _ => 1,\n    }\n}\ng(0)",
        3,
    );
    assert_int_everywhere(
        "fn f(n: int) -> int { if n == 0 { 10 } else if n < 0 { 20 } else { 30 } }\nf(-1)",
        20,
    );
    assert_int_everywhere(
        "fn f(c: bool) -> u8 {\n    if c {\n        let z = 1\n        255\n    } else if z() { 0 } else { 1 }\n}\nfn z() -> bool { false }\n(f(true) + f(false)) as int",
        0,
    );
}

#[test]
fn a_sized_context_types_the_literals_of_value_blocks_loops_and_block_arms() {
    assert_int_everywhere(
        r#"
fn t(c: bool) -> int {
    let x: u8 = if c {
        let q = 1
        200
    } else { 100 }
    let y: u8 = { 200 }
    let z: u8 = loop { break 200 }
    let w: u8 = match c { true => { let k = 2
 250 }, false => 1 }
    (x + y) as int * 1000000 + (z + w) as int
}
fn g(c: bool) -> u8 {
    match c {
        true => {
            let k = 0
            255
        }
        false => 1
    }
}
fn h(c: bool) -> u8 {
    let a: u8 = 250
    if c { let k = 1
 a } else { 10 }
}
let mut k: u8 = 0
k = 'x: loop { loop { break 'x 255 } }
t(true) + ((g(true) + g(false)) as int) * 10 + ((h(false) + h(true)) as int) * 100 + ((k + 1) as int)
"#,
        144000194 + 400,
    );
    let message = assert_rejected("let v: u8 = loop { break 256 }\n0", "E0301");
    assert!(message.contains("256 does not fit in u8"), "{message}");
    assert_rejected("let v: i8 = { let q = 1\n 128 }\n0", "E0301");
}
