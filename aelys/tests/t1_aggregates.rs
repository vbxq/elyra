mod common;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;
use aelys_runtime::Value;

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

fn assert_int_at_every_level(source: &str, expected: i64) {
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        let mut vm = aelys::new_vm().expect("vm");
        let value = aelys::run_with_vm_and_opt(&mut vm, source, "<t1>", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?}");
    }
}

const SHAPES: &str = r#"
struct P { x: int, y: int }
impl P {
    fn add(mut self, d: int) { self.x += d }
    fn sum(self) -> int { self.x + self.y }
}
struct Inner { c: int, xs: [int; 3] }
struct Outer { inner: Inner, ps: Vec<P>, items: Vec<int>, arr: [int; 3] }
enum Shape { Dot(P), Pair(P, P), Nothing }
"#;

fn with_shapes(body: &str) -> String {
    format!("{SHAPES}\n{body}")
}

#[test]
fn clone_of_a_struct_is_deep() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
let o = Outer { inner: Inner { c: 1, xs: [1, 2, 3] }, ps: vec![P { x: 1, y: 2 }], items: vec![5], arr: [7, 8, 9] }
let mut k = o.clone()
k.inner.c = 100
k.inner.xs[0] = 100
k.ps[0].x = 100
k.items.push(100)
k.arr[2] = 100
o.inner.c * 10000 + o.inner.xs[0] * 1000 + o.ps[0].x * 100 + o.items.len() * 10 + o.arr[2] - 9 +
(k.inner.c + k.inner.xs[0] + k.ps[0].x + k.items[1] + k.arr[2]) * 100000
"#,
        ),
        10000 + 1000 + 100 + 10 + 500 * 100000,
    );
}

#[test]
fn clone_of_vec_array_and_nested_collections_is_deep() {
    assert_int_at_every_level(
        r#"
let g = vec![vec![1, 2], vec![3, 4]]
let mut h = g.clone()
h[0][0] = 50
h[1].push(9)
let a = [[1, 2], [3, 4]]
let mut b = a.clone()
b[1][1] = 70
let f = vec![1.5, 2.5]
let mut f2 = f.clone()
f2[0] = 9.0
let t = vec![true, false]
let mut t2 = t.clone()
t2[1] = true
let tb = if t[1] { 1 } else { 0 }
let ff = if f[0] == 1.5 { 1 } else { 0 }
g[0][0] + g[1].len() * 10 + a[1][1] * 100 + h[0][0] * 1000 + h[1].len() * 100000 + b[1][1] * 1000000 + tb + ff
"#,
        1 + 20 + 400 + 50_000 + 300_000 + 70_000_000 + 1,
    );
}

#[test]
fn clone_of_enum_payloads_and_options_is_deep_and_equal() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
let s = Shape::Pair(P { x: 1, y: 2 }, P { x: 3, y: 4 })
let c = s.clone()
let same = if s == c { 1 } else { 0 }
let o = Some(P { x: 5, y: 6 })
let oc = o.clone()
let opt_same = if o == oc { 10 } else { 0 }
let n = Shape::Nothing
let nc = n.clone()
let unit_same = if n == nc { 100 } else { 0 }
let v = vec![Shape::Dot(P { x: 1, y: 1 }), Shape::Nothing]
let vc = v.clone()
let vec_same = if v == vc { 1000 } else { 0 }
same + opt_same + unit_same + vec_same
"#,
        ),
        1111,
    );
}

#[test]
fn clone_terminates_on_a_cycle_and_keeps_its_shape() {
    assert_int_at_every_level(
        r#"
struct Node { id: int, kids: Vec<Node> }
let mut a = Node { id: 1, kids: vec![] }
a.kids.push(a)
let mut b = a.clone()
b.id = 2
a.id * 100 + b.kids[0].id * 10 + b.kids[0].kids[0].id
"#,
        100 + 20 + 2,
    );
}

#[test]
fn clone_in_a_generic_struct_and_a_function_body() {
    assert_int_at_every_level(
        r#"
struct Box2<T> { items: Vec<T> }
fn dup(b: Box2<int>) -> Box2<int> {
    let mut c = b.clone()
    c.items.push(4)
    c
}
let b = Box2 { items: vec![1, 2] }
let c = dup(b)
b.items.len() * 10 + c.items.len()
"#,
        23,
    );
}

#[test]
fn a_user_clone_method_wins_over_the_builtin() {
    assert_int_at_every_level(
        r#"
struct S { x: int }
impl S { fn clone(self) -> S { S { x: self.x + 41 } } }
let s = S { x: 1 }
s.clone().x
"#,
        42,
    );
}

#[test]
fn mutable_alias_from_an_immutable_root_is_rejected_at_every_place_form() {
    let cases = [
        ("let a = P { x: 1, y: 2 }\nlet mut b = a\nb.x", "a.clone()"),
        (
            "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\nlet mut it = o.items\nit.len()",
            "o.items.clone()",
        ),
        (
            "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\nlet mut i = o.inner\ni.c",
            "o.inner.clone()",
        ),
        (
            "let v = vec![P { x: 1, y: 2 }]\nlet mut t = v[0]\nt.x",
            "v[0].clone()",
        ),
        (
            "let a = [P { x: 1, y: 2 }]\nlet mut t = a[0]\nt.x",
            "a[0].clone()",
        ),
        (
            "let g = vec![vec![1]]\nlet mut row = g[0]\nrow.len()",
            "g[0].clone()",
        ),
        ("let a = [1, 2]\nlet mut b = a\nb[0]", "a.clone()"),
        (
            "let a = P { x: 1, y: 2 }\nlet mut b = if true { a } else { P { x: 0, y: 0 } }\nb.x",
            "a.clone()",
        ),
        (
            "let a = P { x: 1, y: 2 }\nlet mut b = match 1 { 1 => a, _ => P { x: 0, y: 0 } }\nb.x",
            "a.clone()",
        ),
        (
            "fn f(p: P) -> int {\n    let mut q = p\n    q.x = 9\n    q.x\n}\nf(P { x: 1, y: 1 })",
            "p.clone()",
        ),
        (
            "let ps = vec![P { x: 1, y: 1 }]\nfor p in ps {\n    let mut q = p\n    q.x = 3\n}\n0",
            "p.clone()",
        ),
    ];
    for (body, help) in cases {
        let message = assert_rejected(&with_shapes(body), "E0323");
        assert!(message.contains(help), "{body}\n{message}");
    }
}

#[test]
fn mut_parameters_and_mut_self_receivers_fed_from_immutable_roots_are_rejected() {
    let parameter_cases = [
        "fn h(mut p: P) { p.x = 1 }\nlet a = P { x: 1, y: 1 }\nh(a)",
        "fn h(mut v: Vec<int>) { v.push(1) }\nlet a = vec![1]\nh(a)",
        "fn h(mut v: [int; 3]) { v[0] = 1 }\nlet o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\nh(o.inner.xs)",
        "struct K { n: int }\nimpl K { fn take(self, mut p: P) { p.x = 1 } }\nlet k = K { n: 1 }\nlet a = P { x: 1, y: 1 }\nk.take(a)",
        "fn g(p: P) { h(p) }\nfn h(mut p: P) { p.x = 1 }\ng(P { x: 1, y: 1 })",
    ];
    for body in parameter_cases {
        let message = assert_rejected(&with_shapes(body), "E0323");
        assert!(message.contains("`mut` parameter"), "{body}\n{message}");
    }
    let receiver_cases = [
        "let a = P { x: 1, y: 1 }\na.add(1)",
        "let v = vec![P { x: 1, y: 1 }]\nv[0].add(1)",
        "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![P { x: 1, y: 1 }], items: vec![], arr: [0, 0, 0] }\no.ps[0].add(1)",
        "fn f(p: P) { p.add(1) }\nf(P { x: 1, y: 1 })",
    ];
    for body in receiver_cases {
        assert_rejected(&with_shapes(body), "E0331");
    }
}

#[test]
fn sharing_through_mutable_roots_is_allowed_and_visible() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
fn grow(mut v: Vec<int>) { v.push(1) }
fn set(mut p: P) { p.x = 40 }
let mut a = P { x: 1, y: 2 }
let mut b = a
b.x = 7
let mut items = vec![0]
grow(items)
let mut c = P { x: 0, y: 0 }
set(c)
let fresh = P { x: 3, y: 3 }
let mut copy = fresh.clone()
copy.x = 99
let n = fresh.x
let mut m = n
m = m + 1
a.x * 1000 + items.len() * 100 + c.x + fresh.x * 100000 + m * 1000000
"#,
        ),
        7000 + 200 + 40 + 300_000 + 4_000_000,
    );
}

#[test]
fn repeat_literals_give_every_slot_its_own_aggregate() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
let mut g = [[0; 4]; 3]
g[1][2] = 5
let mut vg = vec![vec![0; 4]; 3]
vg[1][2] = 5
vg[2].push(1)
let mut ps = [P { x: 0, y: 0 }; 3]
ps[0].x = 10
let mut vs = vec![P { x: 0, y: 0 }; 3]
vs[1].x += 7
let proto = P { x: 4, y: 0 }
let mut qs = vec![proto; 2]
qs[0].x = 99
let mut os = vec![Some(P { x: 1, y: 1 }); 2]
let gsum = g[0][2] + g[1][2] * 10 + g[2][2] * 100
let vgsum = vg[0][2] + vg[1][2] * 10 + vg[2][2] * 100 + vg[0].len() * 1000 + vg[2].len() * 10000
let pssum = ps[0].x + ps[1].x + ps[2].x
let vssum = vs[0].x + vs[1].x * 10 + vs[2].x * 100
gsum + vgsum * 100 + pssum * 10000000 + vssum * 1000000000 + proto.x * 100000000000 + qs[1].x * 1000000000000 + os.len()
"#,
        ),
        50 + (50 + 4000 + 50000) * 100
            + 10 * 10_000_000
            + 70 * 1_000_000_000
            + 4 * 100_000_000_000
            + 4 * 1_000_000_000_000
            + 2,
    );
}

#[test]
fn repeat_literals_in_a_function_and_a_generic_function() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
fn table(n: int) -> Vec<P> { vec![P { x: 0, y: 0 }; n] }
fn fill<T>(v: T, n: int) -> Vec<T> { vec![v; n] }
let mut t = table(3)
t[2].x = 8
let mut f = fill(P { x: 1, y: 0 }, 2)
f[0].x = 5
t[0].x + t[1].x + t[2].x * 10 + f[1].x * 100 + f[0].x * 1000
"#,
        ),
        80 + 100 + 5000,
    );
}

#[test]
fn places_are_mutable_when_their_root_is() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
struct World { outers: Vec<Outer> }
let mut v = vec![P { x: 1, y: 1 }, P { x: 2, y: 2 }]
v[0].x = 10
v[1].x += 5
v[1].add(100)
let mut a = [P { x: 1, y: 1 }]
a[0].y = 20
a[0].y -= 1
let mut o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![P { x: 0, y: 0 }], items: vec![], arr: [0, 0, 0] }
o.ps[0].x = 3
o.ps[0].x *= 2
o.inner.xs[1] = 4
o.inner.c += 1
o.items.push(6)
o.arr[2] = 7
o.ps[0].add(1)
let mut w = World { outers: vec![o.clone()] }
w.outers[0].inner.xs[0] = 9
w.outers[0].ps[0].y = 11
w.outers[0].items.push(12)
v[0].x + v[1].x + a[0].y + o.ps[0].x + o.inner.xs[1] + o.inner.c + o.items[0] + o.arr[2] +
w.outers[0].inner.xs[0] + w.outers[0].ps[0].y + w.outers[0].items.len() * 1000
"#,
        ),
        10 + 107 + 19 + 7 + 4 + 2 + 6 + 7 + 9 + 11 + 2000,
    );
}

#[test]
fn places_are_mutable_through_mut_self_mut_params_closures_and_generics() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
struct Bag<T> { items: Vec<T>, count: int }
impl<T> Bag<T> {
    fn put(mut self, v: T) { self.items.push(v); self.count += 1 }
}
impl Outer {
    fn grow(mut self, n: int) { self.items.push(n); self.ps[0].x += n; self.arr[1] = n; self.inner.xs[2] = n }
}
fn bump(mut ps: Vec<P>) { ps[0].x += 1; ps[0].add(10); ps.push(P { x: 0, y: 0 }) }
fn push_all<T>(mut v: Vec<T>, x: T) { v.push(x) }
fn work() -> int {
    let mut o = Outer { inner: Inner { c: 0, xs: [0, 0, 0] }, ps: vec![P { x: 1, y: 0 }], items: vec![], arr: [0, 0, 0] }
    o.grow(5)
    let mut b = Bag { items: vec![1], count: 0 }
    b.put(2)
    let mut ps = vec![P { x: 0, y: 0 }]
    bump(ps)
    let mut ints = vec![1]
    push_all(ints, 2)
    let mut cells = vec![P { x: 0, y: 0 }]
    let set = fn(k: int) { cells[0].x = k }
    set(6)
    o.items[0] + o.ps[0].x + o.arr[1] + o.inner.xs[2] + b.items.len() * 10 + b.count * 100 + ps[0].x * 1000 + ps.len() * 100000 + ints.len() * 1000000 + cells[0].x * 10000000
}
work()
"#,
        ),
        5 + 6 + 5 + 5 + 20 + 100 + 11_000 + 200_000 + 2_000_000 + 60_000_000,
    );
}

#[test]
fn places_through_immutable_roots_are_rejected_without_leaking_internal_types() {
    let cases = [
        ("let v = vec![P { x: 1, y: 1 }]\nv[0].x = 2", "E0330"),
        ("let v = vec![P { x: 1, y: 1 }]\nv[0].x += 2", "E0330"),
        ("let a = [P { x: 1, y: 1 }]\na[0].x = 2", "E0330"),
        (
            "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\no.items.push(1)",
            "E0318",
        ),
        (
            "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\no.arr[1] = 3",
            "E0318",
        ),
        (
            "let o = Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] }\no.inner.xs[1] = 3",
            "E0318",
        ),
        ("fn g(o: Outer) { o.ps[0].x = 1 }", "E0330"),
        ("fn g(o: Outer) { o.items.push(1) }", "E0318"),
        (
            "impl Outer { fn bad(self) { self.items.push(1) } }",
            "E0318",
        ),
        (
            "fn mk() -> Outer { Outer { inner: Inner { c: 1, xs: [0, 0, 0] }, ps: vec![], items: vec![], arr: [0, 0, 0] } }\nmk().items.push(1)",
            "E0318",
        ),
    ];
    for (body, code) in cases {
        assert_rejected(&with_shapes(body), code);
    }
}

#[test]
fn an_if_expression_yielding_a_struct_can_be_mutated() {
    assert_int_at_every_level(
        &with_shapes(
            r#"
fn pick(c: bool) -> int {
    let mut b = if c { P { x: 1, y: 0 } } else { P { x: 2, y: 0 } }
    b.x = 9
    b.x
}
let mut a = P { x: 1, y: 1 }
let mut b = if true { a } else { P { x: 2, y: 2 } }
b.x = 30
pick(true) + pick(false) + a.x
"#,
        ),
        48,
    );
}

#[test]
fn clone_and_repeat_survive_an_avbc_round_trip_and_the_jit() {
    let source = r#"
struct P { x: int }
fn run() -> int {
    let mut total = 0
    for round in 0..200 {
        let mut ps = vec![P { x: 0 }; 4]
        ps[3].x = round
        let c = ps.clone()
        ps[3].x = 0
        total += c[3].x + ps[0].x + c.len()
    }
    total
}
run()
"#;
    let expected = (0..200).sum::<i64>() + 4 * 200;
    let module = Runtime::new()
        .compile(source, CompileOptions::default())
        .expect("compile");
    let avbc = module.avbc().to_vec();
    for mode in [JitMode::Off, JitMode::Baseline, JitMode::Tiered] {
        let consumer = Runtime::with_jit_mode(mode);
        let loaded = consumer.load_avbc(&avbc, "t1.avbc").expect("load");
        let mut isolate = consumer.new_isolate(IsolateConfig::default());
        assert_eq!(
            isolate
                .execute(&loaded, RunOptions::default())
                .expect("run"),
            ExecutionOutcome::Returned(Value::int(expected)),
            "{mode:?}"
        );
    }
}
