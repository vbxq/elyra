use std::fs;
use std::path::PathBuf;

use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "t2_fix.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}\n{source}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(&mut vm, source, "t2_fix.aelys", OptimizationLevel::Standard) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn rejected_with(source: &str, code: &str) -> String {
    let message = rejected(source);
    assert!(message.contains(code), "expected {code}, got:\n{message}\nfor:\n{source}");
    assert!(
        !message.contains("__aelys") && !message.contains("AelysTuple"),
        "internal name leaked:\n{message}"
    );
    message
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).expect("write module");
    path
}

#[test]
fn u64_ranges_are_checked_against_the_full_int_range() {
    rejected_with(
        "fn sign(x: u64) -> int { match x { 0 => 0, 1.. => 1 } }\nsign(3)",
        "E0302",
    );
    int_at_every_level(
        "fn sign(x: u64) -> int { match x { 0 => 0, 1.. => 1, _ => 2 } }\nlet a: u64 = 3\nsign(a)",
        1,
    );
    int_at_every_level(
        "fn half(b: u8) -> int { match b { 0..=127 => 0, 128..=255 => 1 } }\nlet b: u8 = 200\nhalf(b)",
        1,
    );
}

#[test]
fn an_unannotated_closure_with_a_non_value_tail_returns_unit() {
    for body in [
        "if k == 0 { 5 }",
        "let z = k",
        "while k < 0 { }",
    ] {
        rejected_with(
            &format!("let f = fn(k: int) {{ {body} }}\nlet x: int = f(1)\nx + 1"),
            "E0301",
        );
        rejected_with(
            &format!("fn apply(g: fn(int) -> int, v: int) -> int {{ g(v) }}\napply(fn(k: int) {{ {body} }}, 1)"),
            "E0301",
        );
    }
    int_at_every_level(
        "let f = fn(k: int) { if k == 0 { 5 } else { 7 } }\nlet g = fn(k: int) { let z = k }\ng(1)\nf(1)",
        7,
    );
}

#[test]
fn a_value_return_that_can_fall_through_is_e0215() {
    for source in [
        "let f = fn(k: int) { if k == 0 { return 5 } }\nlet x: int = f(1)\nx",
        "fn g(k: int) { if k == 0 { return 5 } }\nlet x: int = g(1)\nx",
        "fn g(k: int) { for i in 0..3 { if i == k { return i } } }\ng(7)",
        "let f = fn(k: int) { if k == 0 { 5 } else if k == 1 { 6 } }\nlet x: int = f(1)\nx",
        "fn g(k: int) { if k == 0 { 5 } else if k == 1 { 6 } }\nlet x: int = g(1)\nx",
        "fn outer() -> int {\n let f = fn(k: int) { while k > 0 { return k } }\n f(1)\n}\nouter()",
    ] {
        rejected_with(source, "E0215");
    }
    int_at_every_level(
        "fn g(k: int) { if k == 0 { return 5 }\n k * 2 }\nlet f = fn(k: int) { if k == 0 { return 1 } else { return 2 } }\ng(4) + f(0)",
        9,
    );
}

#[test]
fn else_if_let_chains_work_in_statement_position() {
    let program = r#"
        fn f(a: Option<int>, b: Option<int>, c: bool) -> int {
            let mut r = 0
            if let Some(x) = a { r = x } else if let Some(y) = b { r = y * 10 } else if c { r = 500 }
            if let Some(x) = a { r += x } else if c { r += 1000 } else { r += 7 }
            if c { r += 2 } else if let Some(y) = b { r += y }
            if c { r += 0 } else if let Some(y) = b { r += y } else { r += 3 }
            r
        }
        fn g(a: Option<int>, b: Option<int>, c: bool) -> int {
            if c { 1 } else if let Some(x) = a { x } else if let Some(y) = b { y * 10 } else { 99 }
        }
        let k = fn(a: Option<int>, b: Option<int>) -> int {
            if let Some(x) = a { x } else if let Some(y) = b { y + 100 } else { 0 }
        }
        let low = f(Some(1), None, false) + f(None, Some(2), false) * 10 + f(None, None, true) * 1000
        low + g(None, Some(4), false) * 10000000 + g(None, None, false) * 1000000000 + k(None, Some(5)) * 100000000000
        "#;
    int_at_every_level(
        program,
        5 + 31 * 10 + 1502 * 1000 + 40 * 10_000_000 + 99 * 1_000_000_000 + 105 * 100_000_000_000,
    );
}

#[test]
fn nested_destructuring_let_resolves_every_binding() {
    int_at_every_level(
        r#"
        struct P { x: int, y: int }
        let (t, P { x, .. }) = ((1, 2), P { x: 7, y: 8 })
        let ((e, f), (g, h)) = ((4, 8), (7, 1))
        let (a, (b, (c, d))) = (1, (2, (3, 4)))
        let mut (m, (n, _)) = (10, (20, 30))
        m += n
        let pair = ((5, 6), (7, 9))
        let ((_, six), (_, nine)) = pair
        let z = pair.1
        let w = z.1
        t.0 + x + e + f + g + h + a + b + c + d + m + six + nine + w
        "#,
        1 + 7 + 4 + 8 + 7 + 1 + 1 + 2 + 3 + 4 + 30 + 6 + 9 + 9,
    );
    int_at_every_level(
        r#"
        struct Q { p: (int, int), s: string }
        fn f(k: int) -> int {
            let ((a, b), Q { p: (c, d), .. }) = ((k, k * 2), Q { p: (k + 1, k + 2), s: "q" })
            a + b + c + d
        }
        let g = fn(k: int) -> int {
            let ((a, b), (c, d)) = ((k, 1), (2, k))
            a * b + c * d
        }
        f(1) * 100 + g(3)
        "#,
        (1 + 2 + 2 + 3) * 100 + 3 + 6,
    );
}

#[test]
fn internal_tuple_names_are_neither_visible_nor_writable() {
    for (access, message) in [
        ("t.2", "tuple (int, int) has no field 2"),
        ("t.3 + 1", "tuple (int, int) has no field 3"),
        ("t.x", "tuple (int, int) has no field x"),
    ] {
        let error = rejected_with(&format!("let t = (1, 2)\n{access}"), "E0363");
        assert!(error.contains(message), "{error}");
    }
    let nested = rejected_with("let t = ((1, \"a\"), 2)\nt.0.5", "E0363");
    assert!(nested.contains("tuple (int, string) has no field 5"), "{nested}");
    for source in [
        "let x: AelysTuple2<int, int> = (1, 2)\nx.1",
        "struct AelysTuple2 { a: int }\nlet t = (1, 2)\nt.0",
        "let t = (1, 2)\nmatch t { AelysTuple2 { .. } => 1 }",
        "let AelysTuple3 = 4\nAelysTuple3",
        "fn f(t: AelysTuple2<int, int>) -> int { t.0 }\nf((1, 2))",
        "enum AelysTupleKind { A }\n1",
    ] {
        let error = rejected(source);
        assert!(
            error.contains("E0394") && error.contains("reserved name") && !error.contains("<prelude>"),
            "{error}\nfor:\n{source}"
        );
    }
    for source in [
        "let t = (1, 2)\nmatch t { (a, b, c) => a }",
        "let (a, b, c) = (1, 2)\na",
        "fn f(t: (int, int, int)) -> int { match t { (a, b) => a } }\nf((1, 2, 3))",
    ] {
        let error = rejected_with(source, "E0301");
        assert!(error.contains("tuple pattern has"), "{error}\nfor:\n{source}");
    }
}

#[test]
fn closures_in_a_loop_share_outer_variables_and_keep_their_iteration_locals() {
    let shared = r#"
        fn shared_while() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            let mut x = 42
            let mut j = 0
            while j < 2 {
                fs.push(fn() -> int { x })
                x += 1
                j += 1
            }
            x += 100
            fs[0]() * 1000 + fs[1]()
        }
        fn shared_for_break() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            let mut x = 1
            for i in 0..10 {
                fs.push(fn() -> int { x })
                if i == 2 { break }
                if i == 0 { x += 1; continue }
                x += 10
            }
            x += 100
            fs[0]() + fs[1]() + fs[2]()
        }
        fn writer() -> int {
            let mut total = 0
            let mut adders: Vec<fn(int) -> int> = vec![]
            let mut k = 0
            loop {
                if k == 3 { break }
                adders.push(fn(v: int) -> int { total += v; total })
                k += 1
            }
            adders[0](1)
            adders[1](10)
            adders[2](100)
            total
        }
        fn nested() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            let mut x = 0
            for a in 0..2 {
                let mut b = 0
                while b < 2 {
                    fs.push(fn() -> int { x * 100 + a * 10 + b })
                    b += 1
                }
                x += 1
            }
            fs[0]() + fs[3]()
        }
    "#;
    for (tail, expected) in [
        ("shared_while()", 144_144),
        ("shared_for_break()", 336),
        ("writer()", 111),
        ("nested()", 202 + 212),
    ] {
        int_at_every_level(&format!("{shared}\n{tail}"), expected);
    }
    let per_iteration = r#"
        fn ranges() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            for i in 0..4 {
                let k = i * 10
                if i == 1 { fs.push(fn() -> int { k + i }); continue }
                fs.push(fn() -> int { k + i })
            }
            fs[0]() + fs[1]() * 100 + fs[2]() * 10000 + fs[3]() * 1000000
        }
        fn elements() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            for v in vec![3, 5, 7] {
                fs.push(fn() -> int { v })
            }
            let mut n = 0
            while n < 2 {
                let m = n + 1
                fs.push(fn() -> int { m })
                n += 1
            }
            fs[0]() + fs[1]() * 10 + fs[2]() * 100 + fs[3]() * 1000 + fs[4]() * 10000
        }
        ranges() * 100000 + elements()
    "#;
    int_at_every_level(
        per_iteration,
        (11 * 100 + 22 * 10000 + 33 * 1_000_000) * 100_000 + 3 + 50 + 700 + 1000 + 20000,
    );
}

#[test]
fn a_statement_match_discards_its_arm_values() {
    int_at_every_level(
        r#"
        fn pick(o: Option<int>) -> int {
            match o {
                Some(a) => { if a > 0 { return a } },
                None => { return -1 },
            }
            0
        }
        fn walk(v: Vec<Option<int>>) -> int {
            for x in v {
                match x { Some(a) => { if a > 2 { return a } }, None => { return 0 } }
            }
            99
        }
        fn mixed(o: Result<int, string>) -> int {
            let mut r = 0
            match o { Ok(a) => r = a, Err(_) => { } }
            match o { Ok(a) => a, Err(_) => { r += 1000 } }
            match o { Ok(a) => { r += a }, Err(_) => println("e") }
            if let Ok(a) = o { a } else { r += 1 }
            r
        }
        let low = pick(Some(5)) + pick(None) + pick(Some(-2)) + walk(vec![Some(1), Some(4)]) * 10
        low + walk(vec![Some(1), None]) + walk(vec![Some(1)]) * 100 + mixed(Ok(3)) * 100000 + mixed(Err("x")) * 1000000
        "#,
        4 + 40 + 9900 + 6 * 100_000 + 1001 * 1_000_000,
    );
    rejected_with(
        "fn f(o: Option<int>) -> int { let mut v: Vec<int> = vec![1]\n match o { Some(a) => v.pop(), None => { } }\n 0 }\nf(None)",
        "E0304",
    );
}

#[test]
fn an_assignment_tail_makes_a_unit_function() {
    int_at_every_level(
        r#"
        let mut count = 0
        let mut name = "a"
        fn bump(c: bool) {
            if c { count = count + 1 } else { name = "b" }
        }
        fn bump2(c: bool) {
            if c { count += 10 } else { println("side") }
        }
        let f = fn(k: int) { if k > 0 { count += k } else { name = "neg" } }
        bump(true)
        bump(false)
        bump2(true)
        bump2(false)
        f(100)
        f(-1)
        let tag = if name == "neg" { 1000 } else { 0 }
        count + tag
        "#,
        1111,
    );
    rejected_with(
        "let mut count = 0\nlet g = fn(k: int) { count = k }\nlet x: int = g(3)\nx",
        "E0301",
    );
    rejected_with(
        "fn h(mut v: Vec<int>, c: bool) { if c { v.push(1) } else { v.pop() } }\nh(vec![1], true)",
        "E0301",
    );
    int_at_every_level("let mut z = 0\nfn set(v: int) -> int { z = v }\nset(41) + 1", 42);
}

#[test]
fn step_is_a_keyword_only_in_a_for_header() {
    int_at_every_level(
        r#"
        fn step(k: int) -> int { k + 1 }
        let step_size = 3
        let mut s = 0
        for i in 1..step(4) { s += 100000 }
        for i in 0..10 step step_size { s += i }
        let stride = step(1)
        for i in 0..=10 step stride { s += i * 100 }
        for i in 0..step(2) step 2 { s += 10000000 }
        struct T { step: int }
        let t = T { step: 7 }
        fn local() -> int { let step = 1
            step }
        s + t.step + local()
        "#,
        20_000_000 + 400_000 + 3000 + 18 + 7 + 1,
    );
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir, "lib.aelys", "pub fn step() -> int { 7 }\n");
    let main = write(&dir, "main.aelys", "needs lib\nlib::step() * 6\n");
    let value = aelys_driver::run_file(&main).expect("lib::step() is callable");
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn a_brace_inside_parentheses_or_brackets_restores_newline_statements() {
    int_at_every_level(
        r#"
fn id(x: int) -> int { x }
fn apply(f: fn(int) -> int) -> int { f(3) }
let k = 1
let a = id(if k == 0 {
    let t = 5
    t
} else {
    let u = 7
    u + 1
})
let v = vec![
    match k {
        0 => 1,
        _ => {
            let q = 10
            q + 1
        }
    },
    2
]
let mut calls = 0
let r = apply(fn(x: int) -> int {
    calls += 1
    x * 2
})
let t = (
    {
        let w = 4
        w * w
    },
    9
)
let b = id({
    let g = id
    (5)
})
let arr = [
    1,
    2
]
a * 1000000 + v[0] * 10000 + r * 1000 + t.0 * 10 + calls + b + arr[1]
        "#,
        8 * 1_000_000 + 11 * 10_000 + 6 * 1000 + 16 * 10 + 1 + 5 + 2,
    );
    int_at_every_level(
        r#"
let mut launched = 0
fn launch() -> int { launched += 1
    launched }
fn run(x: unit) -> int { 3 }
let r = run({
    let handler = launch
    ()
})
r * 10 + launched
        "#,
        30,
    );
}

#[test]
fn a_quote_name_quote_is_not_a_label() {
    let error = rejected("let t = 'a'\n1");
    assert!(error.contains("E0002"), "{error}");
    int_at_every_level(
        "let mut n = 0\n'outer: for i in 0..3 { for j in 0..3 { if j == 1 { continue 'outer }\n n += 1 } }\nn",
        3,
    );
}

const GEO_MODULE: &str = r#"
pub struct Seg { pub a: (int, int), pub b: ((int, string), bool) }
pub enum Shape { Pt((int, int)), Box { pub corner: (int, int), pub size: int }, Nothing }
pub fn seg() -> Seg { Seg { a: (1, 2), b: ((3, "x"), true) } }
pub fn mk(k: int) -> Shape {
    if k == 0 { Shape::Pt((4, 5)) } else if k == 1 { Shape::Box { corner: (6, 7), size: 8 } } else { Shape::Nothing }
}
pub fn find(n: int) -> Option<(int, string)> { if n > 0 { Some((n, "f")) } else { None } }
pub fn segs() -> Vec<Seg> { vec![seg(), Seg { a: (9, 9), b: ((0, ""), false) }] }
pub fn one() -> int { 1 }
"#;

fn run_with_geo(main: &str) -> Result<aelys::Value, String> {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir, "geo.aelys", GEO_MODULE);
    let path = write(&dir, "main.aelys", main);
    aelys_driver::run_file(&path).map_err(|error| error.to_string())
}

#[test]
fn tuples_cross_module_boundaries_in_fields_and_payloads() {
    let value = run_with_geo(
        r#"
needs geo
let s = geo::seg()
let v = geo::segs()
let a = match geo::mk(0) { Shape::Pt((x, y)) => x * 10 + y, _ => 0 }
let b = match geo::mk(1) { Shape::Box { corner, size } => corner.0 + corner.1 + size, _ => 0 }
let c = match geo::mk(0) { Shape::Pt(t) => t.0, Shape::Box { corner: (p, q), .. } => p, Shape::Nothing => 0 }
let d = match geo::find(3) { Some((n, w)) => n, None => 0 }
let (p, (q, r)) = (s.a.1, s.b.0)
s.a.0 + s.a.1 + s.b.0.0 + a + b + c + d + v[1].a.0 + p + q
"#,
    )
    .expect("imported tuple fields and payloads");
    assert_eq!(value.as_int(), Some(3 + 3 + 45 + 21 + 4 + 3 + 9 + 2 + 3));

    let untouched = run_with_geo("needs geo\ngeo::one() + 41\n").expect("a module with tuple fields loads");
    assert_eq!(untouched.as_int(), Some(42));

    for (main, expected) in [
        ("needs geo\nlet s = geo::seg()\ns.a.5\n", "tuple (int, int) has no field 5"),
        ("needs geo\nlet s = geo::seg()\nlet y: int = s.b\ny\n", "found ((int, string), bool)"),
        (
            "needs geo\nmatch geo::mk(0) { Shape::Pt((a, b, c)) => 1, _ => 0 }\n",
            "tuple pattern has 3 elements",
        ),
    ] {
        let error = run_with_geo(main).expect_err("rejected");
        assert!(error.contains(expected), "{error}\nfor:\n{main}");
        assert!(!error.contains("__aelys") && !error.contains("AelysTuple"), "{error}");
    }
}

#[test]
fn an_undefined_name_in_a_destructured_tuple_is_reported_first() {
    let error = rejected("let (p, (q, r)) = (1, zz.b)\np");
    assert!(error.contains("undefined variable: zz"), "{error}");
}

#[test]
fn low_severity_diagnostics_and_receivers() {
    for source in [
        "let mut v = vec![1, 2, 3]\nwhile let Some(x) = v.pop() {\n if x == 2 { break 99 }\n}\nv.len()",
        "let mut v = vec![1, 2]\n'a: while let Some(x) = v.pop() { loop { break 'a 5 } }\nv.len()",
    ] {
        let error = rejected_with(source, "E0391");
        assert!(!error.contains("if/else"), "{error}");
    }
    let mixed = rejected_with("let c = true\nlet r = loop { if c { break 1 }\n break \"a\" }\nr", "E0301");
    assert!(mixed.contains("`break` values of one loop"), "{mixed}");
    for source in [
        "fn f() -> (int, Option<int>) { (1, None) }\nlet Some(q) = f().1\nq",
        "enum C { R, G }\nlet C::R = C::G\n1",
    ] {
        let error = rejected_with(source, "E0108");
        assert!(error.contains("refutable pattern"), "{error}");
    }
    int_at_every_level("let Foo = 3\nFoo + 1", 4);
    int_at_every_level(
        r#"
        struct P { a: int, b: string }
        impl P { fn get(self) -> int { self.a } }
        fn probe(k: int) -> int {
            let n = (if k == 0 { P { a: 1, b: "x" } } else { P { a: 2, b: "y" } }).get()
            n
        }
        fn length(k: int) -> int { (if k == 0 { vec![1, 2] } else { vec![3] }).len() }
        probe(0) * 1000 + probe(1) * 100 + length(0) * 10 + length(1)
        "#,
        1221,
    );
    int_at_every_level(
        "let mut v = vec![1, 2, 3]\nlet mut s = 0\nwhile let Some(x) = v.pop() { if x == 1 { break }\n s += x }\ns",
        5,
    );
}

#[test]
fn a_break_compiled_before_the_capturing_closure_still_closes_the_iteration() {
    int_at_every_level(
        r#"
        fn labeled_continue() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            let mut i = 0
            'o: while i < 2 {
                i += 1
                let x = i
                for j in 0..3 {
                    if j == 1 { continue 'o }
                    fs.push(fn() -> int { x * 10 + j })
                }
            }
            let p = 9
            fs[0]() * 100 + fs[1]() + p * 0
        }
        fn labeled_break_mut() -> int {
            let mut fs: Vec<fn() -> int> = vec![]
            let mut i = 0
            'outer: while i < 2 {
                i += 1
                let mut x = i * 100
                for j in 0..3 {
                    if j == 1 { continue 'outer }
                    fs.push(fn() -> int { x })
                    x += 5
                }
            }
            let a = 777
            fs[0]() * 1000 + fs[1]() + a * 0
        }
        labeled_continue() * 1000000 + labeled_break_mut()
        "#,
        1020 * 1_000_000 + 105_205,
    );
}
