use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_opt::OptimizationLevel;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 3] = [JitMode::Off, JitMode::Baseline, JitMode::Tiered];

fn value_at(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let module = runtime
        .compile(
            source,
            CompileOptions {
                optimization_level: level,
                ..CompileOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => isolate
            .structured_clone(value)
            .map_err(|error| format!("{error:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn int_everywhere(source: &str, expected: i64) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::Int(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => panic!("{level:?} {jit:?}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn string_everywhere(source: &str, expected: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::String(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => panic!("{level:?} {jit:?}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn runtime_error_everywhere(source: &str, needle: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(value) => {
                    panic!("{level:?} {jit:?}: expected a runtime error, got {value:?}\n{source}")
                }
                Err(text) => {
                    assert!(
                        text.contains(needle),
                        "{level:?} {jit:?}: expected '{needle}', got:\n{text}"
                    );
                    assert!(!text.contains("[E0"), "a compile error:\n{text}");
                }
            }
        }
    }
}

fn rejected(source: &str, code: &str) -> String {
    for level in LEVELS {
        match value_at(source, level, JitMode::Off) {
            Ok(value) => panic!("{level:?}: expected {code}, got {value:?}\n{source}"),
            Err(text) => assert!(
                text.contains(code),
                "{level:?}: expected {code}, got:\n{text}"
            ),
        }
    }
    value_at(source, OptimizationLevel::None, JitMode::Off).unwrap_err()
}

#[test]
fn a_closure_capturing_a_binding_of_a_non_last_match_arm_keeps_its_value() {
    int_everywhere(
        r#"
fn mk(k: int) -> fn() -> int {
    match k {
        n if n > 0 => fn() -> int { n * 2 },
        _ => fn() -> int { 100 },
    }
}
let f = mk(21)
let g = mk(5)
let junk = vec![1, 2, 3, 4, 5, 6, 7, 8]
f() * 1000 + g()
"#,
        42_010,
    );
}

#[test]
fn closures_from_match_arms_stored_in_a_vec_are_called_after_the_match() {
    int_everywhere(
        r#"
let mut cbs: Vec<fn() -> int> = vec![]
let mut i = 0
while i < 4 {
    let o: Option<int> = if i % 2 == 0 { Some(i * 10) } else { None }
    match o {
        Some(v) => cbs.push(fn() -> int { v + 1 }),
        None => cbs.push(fn() -> int { 0 - 1 }),
    }
    i = i + 1
}
let mut t = 0
for f in cbs { t = t * 100 + f() }
t
"#,
        992_099,
    );
}

#[test]
fn a_closure_capturing_an_if_let_binding_keeps_its_value() {
    int_everywhere(
        r#"
let mut cbs: Vec<fn() -> int> = vec![]
let mut i = 0
while i < 4 {
    let o: Option<int> = Some(i * 10)
    if let Some(v) = o { cbs.push(fn() -> int { v + 1 }) } else { cbs.push(fn() -> int { 0 }) }
    i = i + 1
}
let mut t = 0
for f in cbs { t = t * 100 + f() }
t
"#,
        1_112_131,
    );
}

#[test]
fn a_closure_capturing_a_while_let_binding_keeps_its_value() {
    int_everywhere(
        r#"
let mut cbs: Vec<fn() -> int> = vec![]
let src: Vec<Option<int>> = vec![Some(1), Some(2), Some(3), None]
let mut k = 0
while let Some(v) = src[k] {
    cbs.push(fn() -> int { v * 7 })
    k = k + 1
}
let mut t = 0
for f in cbs { t = t * 100 + f() }
t
"#,
        71_421,
    );
}

#[test]
fn a_closure_created_in_a_failing_guard_keeps_its_value() {
    int_everywhere(
        r#"
let mut cbs: Vec<fn() -> int> = vec![]
let mut i = 0
while i < 3 {
    let o: Option<int> = Some(i)
    match o {
        Some(v) if { cbs.push(fn() -> int { v * 3 }); v > 100 } => cbs.push(fn() -> int { 0 }),
        _ => { let q = i + 50; cbs.push(fn() -> int { q }) },
    }
    i = i + 1
}
let mut t = 0
for f in cbs { t = t * 100 + f() }
t
"#,
        5_003_510_652,
    );
}

#[test]
fn closures_from_match_arms_in_a_function_a_method_and_a_generic_function() {
    int_everywhere(
        r#"
enum E { A(int), B(int, int), C }
struct H { base: int }
impl H {
    fn make(self, e: E) -> fn() -> int {
        match e {
            E::A(x) => fn() -> int { x },
            E::B(x, y) => fn() -> int { x * 10 + y },
            E::C => fn() -> int { 0 },
        }
    }
}
fn pick<T>(t: T, e: E) -> fn() -> int {
    match e {
        E::B(x, y) => fn() -> int { x + y },
        E::A(x) => fn() -> int { x * 3 },
        E::C => fn() -> int { 9 },
    }
}
let h = H { base: 1 }
let a = h.make(E::B(4, 2))
let b = h.make(E::A(7))
let c = pick(1, E::B(5, 6))
let d = pick("s", E::A(8))
let pad = vec![100, 200, 300]
a() * 1000000 + b() * 10000 + c() * 100 + d()
"#,
        42_071_124,
    );
}

#[test]
fn early_exits_close_captured_locals_of_the_scopes_they_leave() {
    int_everywhere(
        r#"
let mut cbs: Vec<fn() -> int> = vec![]
let r = 'outer: loop {
    let mut j = 0
    while j < 3 {
        let v = 40 + j
        cbs.push(fn() -> int { v + 1 })
        if j == 1 { break 'outer 5 }
        j = j + 1
    }
}
for i in 0..4 {
    let w = i * 10
    cbs.push(fn() -> int { w })
    if i == 1 { continue }
    if i == 2 { break }
}
fn mk(k: int) -> fn() -> int {
    if k > 0 {
        let n = k * 3
        let f = fn() -> int { n }
        return f
    }
    let g = fn() -> int { 0 }
    g
}
cbs.push(mk(7))
let pad = vec![1, 2, 3]
let mut t = 0
for f in cbs { t = t * 100 + f() }
t + r
"#,
        414_200_102_026,
    );
}

#[test]
fn the_left_operand_is_read_before_a_call_on_the_right_writes_it() {
    int_everywhere(
        r#"
fn main() -> int {
    let mut x = 10
    let f = fn() -> int { x = 100; 1 }
    x = x + f()
    let mut y = 10
    let g = fn() -> int { y = 100; 1 }
    y += g()
    let mut z = 10
    let h = fn() -> int { z = 100; 1 }
    z = z - h()
    x * 1000000 + y * 1000 + z
}
main()
"#,
        11_011_009,
    );
}

#[test]
fn every_operator_reads_its_left_local_before_the_right_call() {
    int_everywhere(
        r#"
fn main() -> int {
    let mut x = 10
    let f = fn() -> int { x = 100; 1 }
    let a = x + f()
    let mut z = 10
    let g = fn() -> int { z = 100; 2 }
    let b = z * g()
    let mut q = 10
    let h = fn() -> int { q = 100; 1 }
    let c = q < h() + 50
    let mut m = 7
    let k = fn() -> int { m = 0; 1 }
    let d = m == 7 + k() - 1
    a * 10000 + b * 10 + (if c { 1 } else { 0 }) + (if d { 1000 } else { 0 })
}
main()
"#,
        111_201,
    );
}

#[test]
fn compound_assignments_read_the_place_before_the_right_call_in_every_form() {
    int_everywhere(
        r#"
struct S { a: int }
let mut gx = 10
fn fg() -> int { gx = 100; 1 }
gx = gx + fg()
let mut gy = 10
fn fy() -> int { gy = 100; 1 }
gy += fy()
let mut v = vec![10, 20]
fn fv() -> int { v[0] = 100; 1 }
v[0] = v[0] + fv()
let mut w = vec![10, 20]
fn fw() -> int { w[0] = 100; 1 }
w[0] += fw()
let mut s = S { a: 10 }
fn fsa() -> int { s.a = 100; 1 }
s.a = s.a + fsa()
let mut t = S { a: 10 }
fn ft() -> int { t.a = 100; 1 }
t.a += ft()
fn inner() -> int {
    let mut u = 10
    let f = fn() -> int { u = 100; 1 }
    let h = fn() -> int { u = u + f(); u }
    h()
}
gx + gy * 100 + v[0] * 10000 + w[0] * 1000000 + s.a * 100000000 + t.a * 10000000000 + inner() * 1000000000000
"#,
        11_111_111_111_111,
    );
}

#[test]
fn an_index_and_its_container_are_read_before_a_later_call() {
    int_everywhere(
        r#"
fn main() -> int {
    let mut v = vec![1, 2]
    let f = fn() -> int { v = vec![7, 8]; 0 }
    let a = v[f()]
    let mut i = 0
    let h = fn() -> int { i = 1; 5 }
    let mut u = vec![10, 20]
    u[i] = h()
    let mut j = 0
    let k = fn() -> int { j = 1; 0 }
    let arr = [3, 4]
    let b = arr[j] + k()
    a * 1000 + u[0] * 10 + u[1] / 10 + b * 100000
}
main()
"#,
        301_052,
    );
}

#[test]
fn operand_order_is_left_to_right_for_calls_on_both_sides() {
    string_everywhere(
        r#"
let mut log = ""
fn p(n: int) -> int { log = log + "{n}"; n }
let mut w = vec![0, 0, 0]
w[p(1)] = p(2)
w[p(1)] += p(2)
let r = p(3) + p(4) * p(5)
let m = vec![vec![1, 2], vec![3, 4]]
let z = m[p(1)][p(0)] + m[p(0)][p(1)]
log
"#,
        "12123451001",
    );
}

#[test]
fn constant_folding_leaves_an_overflowing_index_to_the_runtime_trap() {
    for source in [
        "let a = [1, 2, 3, 4]\na[(140737488355327 * 2) & 255]",
        "let a = [1, 2, 3, 4]\na[(140737488355327 + 1) & 3]",
        "let a = [1, 2, 3, 4]\na[(-140737488355327 - 2) & 3]",
        "let a = vec![1, 2, 3, 4]\na[(140737488355327 * 2) & 255]",
        "let a = [1, 2, 3, 4]\na[-(-140737488355327 - 1) & 3]",
    ] {
        runtime_error_everywhere(source, "48-bit");
    }
    let message = rejected(
        "let a = [1, 2, 3, 4]\na[(70368744177663 * 2) & 255]",
        "E0321",
    );
    assert!(message.contains("254"), "{message}");
}

#[test]
fn a_question_mark_binding_has_the_payload_type_before_a_match() {
    for source in [
        r#"enum Tok { A(string), B }
fn get(t: Tok) -> Result<Tok, string> { Ok(t) }
fn name(t: Tok) -> Result<string, string> {
    let s = get(t)?
    match s {
        Tok::A(n) => Ok(n),
        Tok::B => Ok("b"),
    }
}
name(Tok::A("x")).unwrap()"#,
        r#"enum Tok { A(string), B }
struct Sp { tok: Tok }
fn get(t: Sp) -> Result<Sp, string> { Ok(t) }
fn name(t: Sp) -> Result<string, string> {
    let s = get(t)?
    match s.tok {
        Tok::A(n) => Ok(n),
        Tok::B => Ok("b"),
    }
}
name(Sp { tok: Tok::A("x") }).unwrap()"#,
        r#"enum Tok { A(string), B }
fn get(t: Tok) -> Option<Tok> { Some(t) }
fn name(t: Tok) -> Option<string> {
    match get(t)? {
        Tok::A(n) => Some(n),
        Tok::B => Some("b"),
    }
}
name(Tok::A("x")).unwrap()"#,
        r#"enum Stmt { Eval(int), Skip }
fn parse(n: int) -> Result<Stmt, string> { if n > 0 { Ok(Stmt::Eval(n)) } else { Err("neg") } }
fn twice(n: int) -> Result<int, string> { Ok(n * 2) }
fn run(n: int) -> Result<string, string> {
    let stmt = parse(n)?
    match stmt {
        Stmt::Eval(e) => Ok("x{twice(e)? - 6}".substr(0, 1)),
        Stmt::Skip => Ok("skip"),
    }
}
run(3).unwrap()"#,
    ] {
        string_everywhere(source, "x");
    }
}

#[test]
fn if_let_and_while_let_on_a_question_mark_value_match_user_enums() {
    int_everywhere(
        r#"
enum Tok { A(int), B }
struct W { t: Tok }
fn g(n: int) -> Result<Option<Tok>, string> { if n > 0 { Ok(Some(Tok::A(n))) } else { Ok(None) } }
fn h(n: int) -> Option<W> { Some(W { t: Tok::A(n) }) }
fn run(n: int) -> Result<int, string> {
    let mut total = 0
    if let Some(t) = g(n)? {
        match t { Tok::A(k) => { total = total + k }, Tok::B => { total = total + 1000 } }
    }
    while let Some(Tok::A(k)) = g(n - total)? {
        total = total + k
    }
    Ok(total)
}
fn run2(n: int) -> Option<int> {
    match h(n)?.t { Tok::A(k) => Some(k * 2), Tok::B => None }
}
run(5).unwrap() * 100 + run2(4).unwrap()
"#,
        508,
    );
}

#[test]
fn an_undefined_name_in_a_question_mark_inside_a_format_string_points_at_the_string() {
    let message = rejected(
        "fn twice(n: int) -> Result<int, string> { Ok(n * 2) }\nfn run(n: int) -> Result<string, string> {\n    Ok(\"{twice(zz)?}\")\n}\nrun(3).unwrap()",
        "E0301",
    );
    assert!(message.contains(":3:"), "{message}");
}

#[test]
fn a_trait_argument_named_only_in_a_bound_is_deduced_from_the_impl() {
    int_everywhere(
        r#"
trait Collection<T> {
    fn take(mut self) -> Option<T>;
    fn size(self) -> int;
}
struct Stack<T> { items: Vec<T> }
impl<T> Collection<T> for Stack<T> {
    fn take(mut self) -> Option<T> { self.items.pop() }
    fn size(self) -> int { self.items.len() }
}
fn drain_all<C: Collection<T>, T>(mut c: C) -> Vec<T> {
    let mut out: Vec<T> = vec![]
    while c.size() > 0 {
        out.push(c.take().unwrap())
    }
    out
}
fn count<C: Collection<T>, T>(c: C) -> int { c.size() }
let mut s = Stack { items: vec![1, 2, 3] }
let d = drain_all(s)
let n = count(Stack { items: vec!["a", "b"] })
d[0] * 1000 + d[2] * 100 + d.len() * 10 + n
"#,
        3_132,
    );
    int_everywhere(
        r#"
trait Source<T> { fn first(self) -> T; }
struct Ints { v: Vec<int> }
struct Strs { v: Vec<string> }
impl Source<int> for Ints { fn first(self) -> int { self.v[0] } }
impl Source<string> for Strs { fn first(self) -> string { self.v[0] } }
fn head<S: Source<T>, T>(s: S) -> T { s.first() }
fn twice<T, S: Source<T>>(s: S) -> Vec<T> { let a = s.first(); vec![a, a] }
let a = head(Ints { v: vec![5, 6] })
let b = head(Strs { v: vec!["xyz"] })
let c = twice(Ints { v: vec![7] })
a * 100 + c[1] * 10 + b.len()
"#,
        573,
    );
}

#[test]
fn an_undeducible_generic_parameter_is_named_in_the_diagnostic() {
    let message = rejected("fn count<C, T>(c: C) -> int { 1 }\ncount(3)", "E0343");
    assert!(message.contains("'T'"), "{message}");
}

#[test]
fn user_methods_named_like_option_methods_are_reachable_on_user_types() {
    int_everywhere(
        r#"
struct P { n: int }
impl P {
    fn expect(self, want: int) -> bool { self.n == want }
    fn unwrap(self) -> int { self.n * 2 }
    fn unwrap_or(self, d: int) -> int { self.n + d }
    fn is_some(self) -> bool { true }
    fn map(self, k: int) -> int { self.n * k }
    fn ok(self) -> int { 7 }
}
enum Maybe { Yes(int), No }
impl Maybe {
    fn unwrap(self) -> int { match self { Maybe::Yes(v) => v, Maybe::No => 0 } }
    fn is_none(self) -> bool { match self { Maybe::Yes(_) => false, Maybe::No => true } }
}
struct Wrap<T> { v: T }
impl<T> Wrap<T> { fn unwrap(self) -> T { self.v } }
trait Fallback { fn unwrap_or(self, d: int) -> int; }
struct Q { n: int }
impl Fallback for Q { fn unwrap_or(self, d: int) -> int { self.n + d } }
fn via<T: Fallback>(t: T) -> int { t.unwrap_or(100) }
let p = P { n: 3 }
let flags = (if p.expect(3) { 1 } else { 0 }) + (if p.is_some() { 2 } else { 0 }) + (if Maybe::No.is_none() { 4 } else { 0 })
let o: Option<int> = Some(8)
p.unwrap() * 10000000 + p.unwrap_or(4) * 1000000 + p.map(3) * 10000 + p.ok() * 1000 + Maybe::Yes(4).unwrap() * 100 + Wrap { v: 5 }.unwrap() * 10 + flags + via(Q { n: 1 }) - 101 + o.unwrap() - 8 + Q { n: 1 }.unwrap_or(1) - 2
"#,
        67_097_457,
    );
}

#[test]
fn a_destructuring_let_naming_a_constant_is_a_refutable_pattern() {
    for source in [
        "const LIMIT = 5\nlet (LIMIT, b) = (7, 8)\nb",
        "const LIMIT = 5\nlet t = (5, 8)\nlet (LIMIT, b) = t\nb",
        "const LIMIT = 5\nfn f() -> int {\n    let (a, (LIMIT, b)) = (1, (7, 8))\n    b\n}\nf()",
        "const LIMIT = 5\nstruct P { x: int, y: int }\nfn f(p: P) -> int {\n    let P { x: LIMIT, y } = p\n    y\n}\nf(P { x: 5, y: 2 })",
        "const FLAG = true\nlet mut (FLAG, n) = (true, 1)\nn",
    ] {
        let message = rejected(source, "E0397");
        assert!(message.contains("refutable pattern"), "{message}");
    }
    int_everywhere(
        "const LIMIT = 5\nfn g() -> int {\n    let LIMIT = 9\n    let (LIMIT, b) = (7, 8)\n    LIMIT + b\n}\nlet c = match (5, 1) { (LIMIT, b) => b, _ => 0 }\ng() * 10 + c",
        151,
    );
}

#[test]
fn a_trait_impl_for_a_built_in_type_is_a_named_diagnostic() {
    for ty in [
        "int", "float", "bool", "string", "char", "u8", "i32", "u64", "f32",
    ] {
        let source = format!(
            "trait Ordered {{ fn rank(self) -> int; }}\nimpl Ordered for {ty} {{ fn rank(self) -> int {{ 1 }} }}\n1"
        );
        let message = rejected(&source, "E0112");
        assert!(
            message.contains(&format!("built-in type '{ty}'")) && message.contains(":2:18"),
            "{message}"
        );
    }
}
