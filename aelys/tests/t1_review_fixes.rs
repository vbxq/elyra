use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;
use aelys_runtime::Value;

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
                        source_name: "<t1>".to_string(),
                    },
                )
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}"));
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            let outcome = isolate
                .execute(&module, RunOptions::default())
                .unwrap_or_else(|error| panic!("{level:?} {mode:?}: {error}"));
            assert_eq!(
                outcome,
                ExecutionOutcome::Returned(Value::int(expected)),
                "{level:?} {mode:?}\n{source}"
            );
        }
    }
}

fn rejected(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn assert_rejected(source: &str, code: &str) -> String {
    let message = rejected(source);
    assert!(message.contains(code), "expected {code}:\n{message}");
    assert!(
        !message.contains("dynamic"),
        "an internal type name leaked:\n{message}"
    );
    message
}

fn assert_accepted(source: &str) {
    if let Err(error) = Runtime::new().compile(source, CompileOptions::default()) {
        panic!("the source must be accepted:\n{source}\n{error}");
    }
}

const COUNTING: &str = r#"
struct P { x: int, h: int }
struct W { ps: Vec<P> }
let mut log = vec![0]
fn at(i: int) -> int { log[0] = log[0] * 10 + 1; i }
fn rhs(v: int) -> int { log[0] = log[0] * 10 + 2; v }
"#;

fn counting(body: &str) -> String {
    format!("{COUNTING}\n{body}")
}

#[test]
fn compound_assignment_on_a_vec_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut v = vec![100, 50]
v[at(1)] += rhs(7)
v[0] * 1000 + v[1] + log[0] * 100000
"#,
        ),
        100 * 1000 + 57 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_on_an_array_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut a = [100, 50]
a[at(0)] -= rhs(10)
a[0] * 1000 + a[1] + log[0] * 100000
"#,
        ),
        90 * 1000 + 50 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_on_a_field_of_an_indexed_struct_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut v = vec![P { x: 100, h: 0 }, P { x: 50, h: 0 }]
v[at(1)].x -= rhs(10)
v[0].x * 1000 + v[1].x + log[0] * 100000
"#,
        ),
        100 * 1000 + 40 + 12 * 100000,
    );
}

#[test]
fn compound_assignment_through_a_field_path_and_an_index_evaluates_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
let mut w = W { ps: vec![P { x: 0, h: 3 }, P { x: 0, h: 5 }] }
w.ps[at(0)].h *= rhs(2)
w.ps[0].h * 100 + w.ps[1].h + log[0] * 10000
"#,
        ),
        6 * 100 + 5 + 12 * 10000,
    );
}

#[test]
fn compound_assignment_on_nested_indices_evaluates_left_to_right_once() {
    assert_int_everywhere(
        &counting(
            r#"
fn second(i: int) -> int { log[0] = log[0] * 10 + 3; i }
let mut g = [[1, 2], [3, 4]]
g[at(1)][second(0)] += rhs(100)
let mut gv = vec![vec![1, 2], vec![3, 4]]
gv[at(0)][second(1)] *= 3
g[1][0] * 1000 + gv[0][1] + log[0] * 100000
"#,
        ),
        103 * 1000 + 6 + 13213 * 100000,
    );
}

#[test]
fn compound_assignment_with_an_rng_index_hits_one_slot() {
    assert_int_everywhere(
        r#"
struct M { hp: int }
let mut seed = vec![0]
fn rnd(mut s: Vec<int>) -> int { s[0] += 1; s[0] % 2 }
let mut ms = vec![M { hp: 100 }, M { hp: 50 }]
ms[rnd(seed)].hp -= 10
ms[0].hp * 1000 + ms[1].hp
"#,
        100 * 1000 + 40,
    );
}

#[test]
fn compound_assignment_evaluates_the_index_once_in_methods_closures_and_generics() {
    assert_int_everywhere(
        &counting(
            r#"
struct Bag { items: Vec<P> }
impl Bag {
    fn hit(mut self, d: int) { self.items[at(1)].h += d }
}
fn bump<T>(mut xs: Vec<int>, tag: T) { xs[at(0)] += 1 }
let mut b = Bag { items: vec![P { x: 0, h: 0 }, P { x: 0, h: 0 }] }
b.hit(4)
let mut xs = vec![10, 20]
bump(xs, true)
let mut ys = vec![1, 2]
let f = fn() { ys[at(1)] -= 1 }
f()
b.items[1].h * 1000 + xs[0] * 10 + ys[1] + log[0] * 1000000
"#,
        ),
        4 * 1000 + 11 * 10 + 1 + 111 * 1000000,
    );
}

#[test]
fn compound_assignment_with_a_pure_index_still_works() {
    assert_int_everywhere(
        r#"
let mut v = vec![1, 2, 3]
let i = 1
v[i + 1] += 10
v[i] *= 4
v[i - 1] %= 1
v[0] * 10000 + v[1] * 100 + v[2]
"#,
        8 * 100 + 13,
    );
}

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_at_top_level() {
    assert_int_everywhere(
        r#"
let top = sqrt(4)
fn sqrt(t: int) -> int { t + 100 }
top
"#,
        104,
    );
}

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_in_every_position() {
    assert_int_everywhere(
        r#"
fn second(a: int, b: int) -> int { b }
fn early() -> int { second(0, sqrt(4)) + sqrt(4) }
fn use_min() -> int { min(5) }
let top = second(0, sqrt(4)) + sqrt(4)
let first = early()
let m = use_min()
fn sqrt(t: int) -> int { t + 100 }
fn min(t: int) -> int { t + 200 }
let f = sqrt
top * 1000000 + first * 1000 + f(1) + m * 1000000000
"#,
        205 * 1000000000 + 208 * 1000000 + 208 * 1000 + 101,
    );
}

#[test]
fn a_user_function_named_like_a_native_wins_before_its_declaration_in_a_method() {
    assert_int_everywhere(
        r#"
struct H { v: int }
impl H { fn go(self) -> int { len(self.v) } }
let r = H { v: 3 }.go()
fn len(x: int) -> int { x * 1000 }
r
"#,
        3000,
    );
}

#[test]
fn a_function_called_before_its_declaration_runs() {
    assert_int_everywhere(
        r#"
let a = later(1)
fn later(t: int) -> int { t + helper() }
fn helper() -> int { 40 }
a
"#,
        41,
    );
}

const ALIAS_SHAPES: &str = r#"
struct In { c: int }
struct S { x: int, inner: In, items: Vec<int> }
fn mk() -> S { S { x: 0, inner: In { c: 0 }, items: vec![] } }
"#;

fn alias_program(body: &str) -> String {
    format!("{ALIAS_SHAPES}\n{body}")
}

#[test]
fn assigning_an_immutable_aggregate_to_a_mutable_binding_is_rejected_everywhere() {
    let cases = [
        "let a = mk()\nlet mut b = mk()\nb = a\nb.x = 99",
        "fn f(a: S) -> int { let mut b = mk(); b = a; b.x = 99; a.x }",
        "let a = mk()\nlet mut g = mk()\nfn run() { g = a; g.x = 99 }",
        "let v = vec![1, 2]\nlet mut w: Vec<int> = vec![]\nw = v\nw.push(3)",
        "let a = mk()\nlet mut i = In { c: 1 }\ni = a.inner",
        "let a = mk()\nlet mut xs = vec![0]\nxs = a.items",
        "let a = [[1, 2], [3, 4]]\nlet mut row = [0, 0]\nrow = a[1]",
        "let a = mk()\nlet mut b = mk()\nlet c = true\nb = if c { a } else { mk() }",
        "let a = mk()\nlet mut b = mk()\nb = match a.x { 0 => a, _ => mk() }",
        "impl S { fn grab(self) -> int { let mut xs = vec![0]; xs = self.items; xs.len() } }",
        "let a = mk()\nlet mut b = mk()\nlet f = fn() { b = a }",
        "fn g<T>(a: Vec<T>) { let mut w: Vec<T> = vec![]; w = a }",
    ];
    for body in cases {
        let message = assert_rejected(&alias_program(body), "E0323");
        assert!(
            message.contains("to the mutable binding"),
            "the message must name the assignment:\n{message}"
        );
    }
}

#[test]
fn assignments_that_do_not_alias_an_immutable_root_are_accepted() {
    let cases = [
        "let a = mk()\nlet mut b = mk()\nb = a.clone()\nb.x = 99",
        "let mut a = mk()\nlet mut b = mk()\nb = a\nb.x = 99",
        "let a = mk()\nlet mut n = 0\nn = a.x",
        "let a = In { c: 1 }\nlet mut s = mk()\ns.inner = a",
        "let a = vec![1]\nlet mut ws = vec![vec![0]]\nws[0] = a",
        "fn f(mut a: S) { let mut b = mk(); b = a; b.x = 1 }",
        "let mut b = mk()\nb = mk()",
    ];
    for body in cases {
        assert_accepted(&alias_program(body));
    }
}

#[test]
fn assignment_through_a_mutable_binding_shares_with_a_mutable_source() {
    assert_int_everywhere(
        &alias_program(
            r#"
let mut a = mk()
let mut b = mk()
b = a
b.x = 7
let c = mk()
let mut d = mk()
d = c.clone()
d.x = 9
a.x * 100 + c.x * 10 + d.x
"#,
        ),
        7 * 100 + 9,
    );
}

const FN_VALUE_SHAPES: &str = r#"
struct S { x: int }
struct Holder { f: fn(mut S) -> int }
fn touch(mut s: S) -> int { s.x = 99; 0 }
fn read(s: S) -> int { s.x }
"#;

fn fn_value_program(body: &str) -> String {
    format!("{FN_VALUE_SHAPES}\n{body}")
}

#[test]
fn a_mut_parameter_reached_through_a_function_value_is_guarded() {
    let cases = [
        "let a = S { x: 1 }\nlet h = touch\nh(a)",
        "let a = S { x: 1 }\nlet h = touch\nlet h2 = h\nh2(a)",
        "let a = S { x: 1 }\nlet g = fn(mut q: S) -> int { q.x = 99; 0 }\ng(a)",
        "let a = S { x: 1 }\nlet read = touch\nread(a)",
        "let h = touch\nfn run(a: S) -> int { h(a) }",
        "let a = S { x: 1 }\nlet h = touch\nlet c = fn() -> int { h(a) }",
        "fn apply(f: fn(mut S) -> int, s: S) -> int { f(s) }",
        "let a = S { x: 1 }\nlet mut h = touch\nh(a)",
        "let a = S { x: 1 }\nlet t = Holder { f: touch }\nt.f(a)",
        "let a = S { x: 1 }\nlet t = Holder { f: touch }\n(t.f)(a)",
        "let a = vec![S { x: 1 }]\nfn pick() -> fn(mut S) -> int { touch }\npick()(a[0])",
    ];
    for body in cases {
        let message = assert_rejected(&fn_value_program(body), "E0323");
        assert!(message.contains("fn(mut S) -> int"), "{body}:\n{message}");
    }
}

#[test]
fn calls_through_function_values_that_cannot_alias_are_accepted() {
    let cases = [
        "let a = S { x: 1 }\nlet f = fn(s: S) -> int { s.x }\nf(a)",
        "let a = S { x: 1 }\nlet h = read\nh(a)",
        "let mut a = S { x: 1 }\nlet h = touch\nh(a)",
        "fn apply(f: fn(mut S) -> int, mut s: S) -> int { f(s) }\nlet mut a = S { x: 1 }\napply(touch, a)",
        "fn apply(f: fn(int) -> int, n: int) -> int { f(n) }",
        "fn apply(f: fn(S) -> int, s: S) -> int { f(s) }\nlet a = S { x: 1 }\napply(read, a)",
        "let a = S { x: 1 }\nlet b = a.clone()\nfn apply(f: fn(mut S) -> int, mut s: S) -> int { f(s) }\napply(touch, a.clone())",
    ];
    for body in cases {
        assert_accepted(&fn_value_program(body));
    }
}

#[test]
fn a_mut_parameter_reached_through_a_function_value_mutates_a_mutable_root() {
    assert_int_everywhere(
        &fn_value_program(
            r#"
fn apply(f: fn(mut S) -> int, mut s: S) -> int { f(s) }
let mut a = S { x: 1 }
let mut b = S { x: 2 }
let h = touch
let _ = h(a)
let _ = apply(touch, b)
let k = S { x: 5 }
let c = k.clone()
let mut d = c.clone()
let _ = apply(touch, d)
a.x * 10000 + b.x * 100 + k.x + c.x * 0 + d.x * 0
"#,
        ),
        99 * 10000 + 99 * 100 + 5,
    );
}

#[test]
fn a_reserved_double_underscore_name_cannot_be_declared() {
    let cases = [
        "struct P { x: int }\nfn __clone(p: P) -> P { p }",
        "let __clone = 5",
        "let mut __tostring = 5",
        "const __N = 3",
        "fn f(__clone: int) -> int { __clone }",
        "let g = fn(__clone: int) -> int { __clone }",
        "fn f() -> int { let __clone = 1\n__clone }",
        "for __i in 0..3 { }",
        "let r = match 3 { __x => __x }",
        "struct __P { x: int }",
        "struct P { __x: int }",
        "enum __E { A }",
        "enum E { __A }",
        "trait __T { fn m(self) -> int }",
        "fn id<__T>(x: __T) -> __T { x }",
        "struct S { x: int }\nimpl S { fn __clone(self) -> S { self } }",
        "needs std::math as __m",
    ];
    for body in cases {
        let message = assert_rejected(body, "E0114");
        assert!(message.contains("reserved"), "{message}");
    }
}

#[test]
fn clone_and_repeat_copies_still_reach_the_builtin() {
    assert_int_everywhere(
        r#"
struct P { x: int }
let a = P { x: 1 }
let mut b = a.clone()
b.x = 99
let mut v = [P { x: 1 }; 2]
v[0].x = 5
a.x * 100 + v[1].x * 10 + __tostring(7).len()
"#,
        100 + 10 + 1,
    );
}

#[test]
fn match_and_for_bindings_from_a_mutable_root_may_be_bound_mutably() {
    assert_int_everywhere(
        r#"
struct S { x: int, items: Vec<int> }
fn mk() -> S { S { x: 1, items: vec![] } }
let mut a = S { x: 1, items: vec![5] }
match a { S { items, .. } => { let mut x = items; x.push(1) } }
let mut v = vec![mk(), mk()]
for s in v { let mut m = s; m.x = 7 }
let mut w = vec![mk()]
match w[0] { S { items, .. } => { let mut deep = items; deep.push(3) } }
fn in_fn(mut p: S) -> int {
    match p { S { items, .. } => { let mut k = items; k.push(2) } }
    p.items.len()
}
a.items.len() * 1000 + (v[0].x + v[1].x) * 10 + w[0].items.len() + in_fn(mk()) * 100
"#,
        2 * 1000 + 14 * 10 + 1 + 100,
    );
}

#[test]
fn match_and_for_bindings_from_an_immutable_root_name_that_root() {
    let cases = [
        (
            "struct S { x: int, items: Vec<int> }\nlet a = S { x: 1, items: vec![5] }\nmatch a { S { items, .. } => { let mut x = items; x.push(1) } }",
            "immutable binding `a`",
        ),
        (
            "struct S { x: int }\nlet v = vec![S { x: 1 }]\nfor s in v { let mut m = s; m.x = 7 }",
            "immutable binding `v`",
        ),
        (
            "struct S { x: int, items: Vec<int> }\nfn f(a: S) { match a { S { items, .. } => { let mut a = S { x: 0, items: vec![] }; let mut x = items; x.push(1) } } }",
            "immutable binding `a`",
        ),
        (
            "struct S { x: int }\nlet v = vec![S { x: 1 }]\nfor s in v { let f = fn() { let mut m = s; m.x = 7 } }",
            "immutable binding `v`",
        ),
    ];
    for (body, root) in cases {
        let message = assert_rejected(body, "E0323");
        assert!(message.contains(root), "expected {root}:\n{message}");
    }
}

#[test]
fn a_constant_that_cannot_be_evaluated_names_the_cause() {
    let cases = [
        ("const A = 1 / 0", "division by zero"),
        ("const A = 5 % 0", "remainder by zero"),
        (
            "const A = 9223372036854775807 + 1",
            "overflows the int range",
        ),
        (
            "const A = 10000000000 * 10000000000",
            "overflows the int range",
        ),
        ("const B = 1 << 63", "overflows the int range"),
        ("const B = 2 << 62", "2 << 62 overflows the int range"),
        (
            "const A = -9223372036854775807 - 1\nconst B = -A",
            "overflows the int range",
        ),
        ("const D = 0\nconst A = (2 + 3) / D", "division by zero"),
        ("fn f() -> int { const K = 4 % 0\nK }", "remainder by zero"),
    ];
    for (body, cause) in cases {
        let message = assert_rejected(body, "E0395");
        assert!(message.contains(cause), "expected {cause}:\n{message}");
        assert!(
            !message.contains("not a compile-time constant"),
            "{message}"
        );
    }
    let message = assert_rejected("fn f() -> int { 3 }\nconst A = f()", "E0395");
    assert!(message.contains("not a compile-time constant"), "{message}");
}

#[test]
fn a_constant_declared_twice_in_one_scope_is_rejected() {
    for body in [
        "const A = 1\nconst A = 2",
        "const A = 1\nprintln(A)\nconst A = 2",
        "fn f() -> int { const K = 1\nconst K = 2\nK }",
    ] {
        assert_rejected(body, "E0396");
    }
    assert_int_everywhere(
        r#"
const A = 1
fn f() -> int { const A = 10
A }
fn g() -> int { const B = 2
B }
fn h() -> int { const B = 3
B }
A * 1000 + f() * 100 + g() * 10 + h()
"#,
        1000 + 1000 + 20 + 3,
    );
}

#[test]
fn assigning_an_option_or_result_to_any_place_uses_the_value() {
    assert_int_everywhere(
        r#"
struct N { id: int, next: Option<int> }
struct L { nodes: Vec<N> }
impl N {
    fn link(mut self, v: int) { self.next = Some(v) }
}
let mut a = N { id: 1, next: None }
a.next = Some(3)
let mut l = L { nodes: vec![N { id: 2, next: None }] }
l.nodes[0].next = Some(9)
let mut opts: Vec<Option<int>> = vec![None]
opts[0] = Some(5)
let mut arr: [Option<int>; 2] = [None, None]
arr[1] = Some(4)
let mut g: Option<int> = None
fn setg() { g = Some(7) }
setg()
let mut rs: Vec<Result<int, string>> = vec![Ok(1)]
rs[0] = Err("x")
let mut b = N { id: 3, next: None }
b.link(20)
let mut c = N { id: 4, next: None }
let set_c = fn(v: int) { c.next = Some(v) }
set_c(30)
let total = a.next.unwrap() + l.nodes[0].next.unwrap() + opts[0].unwrap() + arr[1].unwrap()
total + g.unwrap() + rs.len() + b.next.unwrap() + c.next.unwrap()
"#,
        3 + 9 + 5 + 4 + 7 + 1 + 20 + 30,
    );
}

#[test]
fn an_ignored_option_or_result_is_still_rejected() {
    for body in [
        "fn f() -> Option<int> { Some(1) }\nf()",
        "let v = vec![Some(1)]\nv[0]",
        "struct N { next: Option<int> }\nlet a = N { next: None }\na.next",
        "fn r() -> Result<int, string> { Ok(1) }\nfn g() { r() }\ng()",
    ] {
        let message = rejected(body);
        assert!(
            message.contains("E0304") || message.contains("E0303"),
            "{body}\n{message}"
        );
    }
}

#[test]
fn increment_and_decrement_work_on_every_place_and_evaluate_the_index_once() {
    assert_int_everywhere(
        &counting(
            r#"
struct Bag { items: Vec<P> }
impl Bag {
    fn bump(mut self) { self.items[at(0)].h++ }
}
fn dec<T>(mut xs: Vec<int>, tag: T) { xs[at(1)]-- }
let mut s = P { x: 5, h: 0 }
s.x++
s.x--
s.x--
let mut a = [1, 2, 3]
a[0]++
a[2]--
let mut w = W { ps: vec![P { x: 0, h: 10 }, P { x: 0, h: 20 }] }
w.ps[at(1)].h++
w.ps[at(0)].x--
let mut g = vec![vec![1, 2], vec![3, 4]]
g[at(1)][at(0)]++
let mut b = Bag { items: vec![P { x: 0, h: 7 }] }
b.bump()
let mut xs = vec![10, 20]
dec(xs, 1)
let mut ys = vec![0]
let f = fn() { ys[at(0)]++ }
f()
let mut n = 1
n++
let checks = s.x * 1000000 + a[0] * 100000 + a[2] * 10000 + w.ps[1].h * 100 + g[1][0] * 10 + n
checks + (w.ps[0].x + b.items[0].h + xs[1] + ys[0]) * 1000000000 + log[0] * 0
"#,
        ),
        4 * 1000000
            + 2 * 100000
            + 2 * 10000
            + 21 * 100
            + 4 * 10
            + 2
            + (-1 + 8 + 19 + 1) * 1000000000,
    );
    assert_int_everywhere(
        &counting(
            r#"
let mut w = W { ps: vec![P { x: 0, h: 10 }, P { x: 0, h: 20 }] }
w.ps[at(1)].h++
let mut g = vec![vec![1, 2], vec![3, 4]]
g[at(1)][at(0)]--
log[0]
"#,
        ),
        111,
    );
}

#[test]
fn increment_on_an_immutable_place_or_a_temporary_is_rejected() {
    assert_rejected("struct S { v: int }\nlet s = S { v: 1 }\ns.v++", "E0330");
    assert_rejected("let a = [1, 2]\na[0]++", "E0318");
    assert_rejected("fn f() -> int { 1 }\nf()++", "E0104");
}
