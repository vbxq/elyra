use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_opt::OptimizationLevel;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 3] = [JitMode::Off, JitMode::Baseline, JitMode::Tiered];

fn outcome(
    isolate: &mut aelys::Isolate,
    module: Result<aelys::CompiledModule, aelys::AelysError>,
) -> Result<StructuredValue, String> {
    let module = module.map_err(|error| error.to_string())?;
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

fn options(level: OptimizationLevel) -> CompileOptions {
    CompileOptions {
        optimization_level: level,
        ..CompileOptions::default()
    }
}

fn value_at(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let module = runtime.compile(source, options(level));
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    outcome(&mut isolate, module)
}

fn file_value_at(
    entry: &Path,
    level: OptimizationLevel,
    jit: JitMode,
) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let module = isolate.compile_file(entry, options(level));
    outcome(&mut isolate, module)
}

fn expect_everywhere(
    run: impl Fn(OptimizationLevel, JitMode) -> Result<StructuredValue, String>,
    expected: &StructuredValue,
    what: &str,
) {
    for level in LEVELS {
        for jit in JITS {
            let got = run(level, jit);
            assert!(
                got.as_ref() == Ok(expected),
                "{level:?} {jit:?}: expected {expected:?}, got {got:?}\n{what}"
            );
        }
    }
}

fn int_everywhere(source: &str, expected: i64) {
    expect_everywhere(
        |level, jit| value_at(source, level, jit),
        &StructuredValue::Int(expected),
        source,
    );
}

fn string_everywhere(source: &str, expected: &str) {
    expect_everywhere(
        |level, jit| value_at(source, level, jit),
        &StructuredValue::String(expected.to_string()),
        source,
    );
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

// a hang must fail the test, so the compile runs apart and is given up on after a while
fn compiles_within(source: &str, budget: Duration) {
    let owned = source.to_string();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let compiled = Runtime::new()
            .compile(&owned, CompileOptions::default())
            .map(|_| ())
            .map_err(|error| error.to_string());
        let _ = sender.send((compiled, started.elapsed()));
    });
    match receiver.recv_timeout(Duration::from_secs(20)) {
        Ok((Ok(()), elapsed)) => assert!(
            elapsed < budget,
            "compiled in {elapsed:?}, over {budget:?}:\n{source}"
        ),
        Ok((Err(error), _)) => panic!("did not compile: {error}\n{source}"),
        Err(_) => panic!("the compiler did not finish:\n{source}"),
    }
}

const SRC_PRELUDE: &str = r#"
trait Src<T> { fn get(self) -> T; }
struct W<T> { v: T }
impl<T> Src<T> for W<T> { fn get(self) -> T { self.v } }
fn pull<S: Src<T>, T>(s: S) -> T { s.get() }
"#;

fn with_src(body: &str) -> String {
    format!("{SRC_PRELUDE}{body}")
}

fn deduced_in_generic_callers() -> Vec<(String, StructuredValue)> {
    let int = StructuredValue::Int;
    let string = |text: &str| StructuredValue::String(text.to_string());
    vec![
        (
            with_src("fn outer<T>(w: W<T>) -> T { pull(w) }\nouter(W { v: 5 })"),
            int(5),
        ),
        (
            r#"
trait Src<T> { fn get(self) -> T; }
struct W<T> { v: T }
impl<T> Src<T> for W<T> { fn get(self) -> T { self.v } }
fn pull<S: Src<U>, U>(s: S) -> U { s.get() }
fn outer<T>(w: W<T>) -> T { pull(w) }
outer(W { v: 5 })
"#
            .to_string(),
            int(5),
        ),
        (
            with_src(
                r#"
fn a<T>(w: W<T>) -> T { pull(w) }
fn b<T>(w: W<T>) -> T { a(w) }
fn c<U>(w: W<U>) -> U { b(w) }
"{c(W { v: 7 })}{c(W { v: "s" })}"
"#,
            ),
            string("7s"),
        ),
        (
            with_src(
                r#"
fn lengths<T>(w: W<Vec<T>>) -> int {
    let v = pull(w)
    v.len()
}
fn unwrap2<T>(w: W<W<T>>) -> T {
    let inner = pull(w)
    pull(inner)
}
lengths(W { v: vec![1, 2, 3] }) * 100 + unwrap2(W { v: W { v: 9 } })
"#,
            ),
            int(309),
        ),
        (
            with_src(
                r#"
fn ping<T>(w: W<T>, n: int) -> T { if n <= 0 { pull(w) } else { pong(w, n - 1) } }
fn pong<T>(w: W<T>, n: int) -> T { if n <= 0 { pull(w) } else { ping(w, n - 1) } }
ping(W { v: 11 }, 5)
"#,
            ),
            int(11),
        ),
        (
            r#"
trait Src<T> { fn get(self) -> T; }
struct P<A, B> { a: A, b: B }
impl<A, B> Src<B> for P<A, B> { fn get(self) -> B { self.b } }
fn pull<S: Src<T>, T>(s: S) -> T { s.get() }
fn outer<B, A>(p: P<B, A>) -> A { pull(p) }
outer(P { a: 1, b: "two" })
"#
            .to_string(),
            string("two"),
        ),
        (
            r#"
trait Src<T> { fn get(self) -> T; }
struct Box2<T> { v: T, n: int }
impl<T> Src<int> for Box2<T> { fn get(self) -> int { self.n } }
fn pull<S: Src<T>, T>(s: S) -> T { s.get() }
fn outer<T>(b: Box2<T>) -> int { pull(b) + 1 }
outer(Box2 { v: "x", n: 41 })
"#
            .to_string(),
            int(42),
        ),
        (
            with_src("fn outer<T>(w: W<T>) -> T { pull::<W<T>, T>(w) }\nouter(W { v: 5 })"),
            int(5),
        ),
        (
            with_src(
                r#"
fn outer<T>(w: W<T>) -> T {
    let f = fn(x: W<T>) -> T { pull(x) }
    f(w)
}
outer(W { v: 6 })
"#,
            ),
            int(6),
        ),
        (
            r#"
trait Queue<T> { fn take(mut self) -> Option<T>; }
struct Stack<T> { data: Vec<T> }
impl<T> Queue<T> for Stack<T> {
    fn take(mut self) -> Option<T> { self.data.pop() }
}
fn drain<Q: Queue<T>, T>(mut q: Q) -> Vec<T> {
    let mut out: Vec<T> = vec![]
    while let Some(x) = q.take() { out.push(x) }
    out
}
fn all<T>(mut s: Stack<T>) -> Vec<T> { drain(s) }
let v = all(Stack { data: vec![1, 2, 3] })
v[0] * 100 + v[1] * 10 + v[2]
"#
            .to_string(),
            int(321),
        ),
    ]
}

#[test]
fn a_bound_deduced_call_inside_a_generic_caller_runs() {
    for (source, expected) in deduced_in_generic_callers() {
        expect_everywhere(
            |level, jit| value_at(&source, level, jit),
            &expected,
            &source,
        );
    }
}

#[test]
fn a_bound_deduced_call_inside_a_generic_caller_compiles_quickly() {
    for (source, _) in deduced_in_generic_callers() {
        compiles_within(&source, Duration::from_secs(2));
    }
}

struct ModuleDir {
    root: tempfile::TempDir,
}

impl ModuleDir {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().expect("temporary module directory");
        for (name, text) in files {
            std::fs::write(root.path().join(name), text).expect("write module file");
        }
        ModuleDir { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

const VALUED_LIB: &str = r#"
pub struct Item { pub n: int }
pub trait Valued {
    fn value(self) -> int;
    fn label(self) -> string { "worth {self.value()}" }
}
impl Valued for Item { fn value(self) -> int { self.n } }
"#;

fn module_string_everywhere(files: &[(&str, &str)], expected: &str) {
    let dir = ModuleDir::new(files);
    let entry = dir.path("main.aelys");
    expect_everywhere(
        |level, jit| file_value_at(&entry, level, jit),
        &StructuredValue::String(expected.to_string()),
        files.last().map(|(_, text)| *text).unwrap_or_default(),
    );
}

#[test]
fn a_default_method_of_an_imported_trait_is_reachable_through_a_whole_module_import() {
    module_string_everywhere(
        &[
            ("lib.aelys", VALUED_LIB),
            (
                "main.aelys",
                "needs lib\nlet it = Item { n: 4 }\n\"{it.value()} {it.label()}\"\n",
            ),
        ],
        "4 worth 4",
    );
}

#[test]
fn a_default_method_of_an_imported_trait_is_reachable_through_a_selective_import() {
    module_string_everywhere(
        &[
            ("lib.aelys", VALUED_LIB),
            (
                "main.aelys",
                "needs Item, Valued from lib\nlet it = Item { n: 4 }\nit.label()\n",
            ),
        ],
        "worth 4",
    );
}

#[test]
fn an_importer_implementing_an_imported_trait_adopts_its_default_method() {
    module_string_everywhere(
        &[
            ("lib.aelys", VALUED_LIB),
            (
                "main.aelys",
                r#"
needs Valued from lib
struct Mine { k: int }
impl Valued for Mine { fn value(self) -> int { self.k * 2 } }
let m = Mine { k: 5 }
m.label()
"#,
            ),
        ],
        "worth 10",
    );
}

#[test]
fn an_imported_default_method_is_reachable_through_a_bound_and_a_closure() {
    module_string_everywhere(
        &[
            ("lib.aelys", VALUED_LIB),
            (
                "main.aelys",
                r#"
needs lib
fn show<T: Valued>(t: T) -> string { t.label() }
let f = fn(i: Item) -> string { i.label() }
"{show(Item { n: 3 })}|{f(Item { n: 6 })}"
"#,
            ),
        ],
        "worth 3|worth 6",
    );
}

#[test]
fn default_methods_cross_three_modules_and_an_override_wins() {
    module_string_everywhere(
        &[
            (
                "shapes.aelys",
                r#"
pub trait Shape {
    fn area(self) -> int;
    fn double(self) -> int { self.area() * 2 }
    fn describe(self, prefix: string) -> string { "{prefix}:{self.double()}" }
}
"#,
            ),
            (
                "kinds.aelys",
                r#"
needs Shape from shapes
pub struct Sq { pub s: int }
impl Shape for Sq { fn area(self) -> int { self.s * self.s } }
pub struct Odd { pub v: int }
impl Shape for Odd {
    fn area(self) -> int { self.v }
    fn double(self) -> int { 1000 }
}
"#,
            ),
            (
                "main.aelys",
                r#"
needs shapes
needs kinds
fn total<T: Shape>(xs: Vec<T>) -> int {
    let mut t = 0
    for x in xs { t += x.double() }
    t
}
let q = Sq { s: 3 }
let o = Odd { v: 7 }
"{q.describe("sq")} {o.describe("odd")} {total(vec![Sq { s: 1 }, Sq { s: 2 }])}"
"#,
            ),
        ],
        "sq:18 odd:1000 10",
    );
}

#[test]
fn a_missing_method_is_named_a_method_and_a_missing_field_a_field() {
    let method = rejected("struct P { x: int }\nlet p = P { x: 1 }\np.nope()", "E0363");
    assert!(
        method.contains("unknown method 'nope' on struct P"),
        "{method}"
    );
    let field = rejected("struct P { x: int }\nlet p = P { x: 1 }\np.nope", "E0363");
    assert!(
        field.contains("unknown field 'nope' on struct P"),
        "{field}"
    );
}

#[test]
fn a_branch_joined_binding_matches_a_nested_user_variant() {
    int_everywhere(
        r#"
enum Item { Key(int), Coin(int) }
fn pick(v: Vec<Item>, i: int) -> int {
    let k = if i < v.len() { Some(v[i]) } else { None }
    match k {
        Some(Item::Key(n)) => n,
        Some(_) => 1,
        None => 0,
    }
}
pick(vec![Item::Key(7)], 0) * 100 + pick(vec![Item::Coin(2)], 0) * 10 + pick(vec![], 0)
"#,
        710,
    );
    int_everywhere(
        r#"
enum Item { Key(int), Coin(int) }
fn pick(v: Vec<Item>, i: int) -> int {
    let k = if i < v.len() { Some(v[i]) } else { None }
    match (i, k) {
        (0, Some(Item::Key(n))) => n,
        (_, Some(_)) => 1,
        (_, None) => 0,
    }
}
pick(vec![Item::Key(7)], 0)
"#,
        7,
    );
}

#[test]
fn a_match_joined_binding_from_a_generic_method_matches_a_user_variant() {
    int_everywhere(
        r#"
enum Ev { Tick(int), Quit }
trait Queue<T> { fn take(mut self) -> Option<T>; }
struct Stack<T> { data: Vec<T> }
impl<T> Queue<T> for Stack<T> {
    fn take(mut self) -> Option<T> { self.data.pop() }
}
let mut q: Stack<Ev> = Stack { data: vec![Ev::Quit, Ev::Tick(4)] }
let ev = match q.take() { Some(e) => e, None => Ev::Quit }
match ev {
    Ev::Tick(t) => t,
    Ev::Quit => 0,
}
"#,
        4,
    );
}

#[test]
fn joined_bindings_match_user_variants_in_a_method_a_generic_function_and_a_closure() {
    int_everywhere(
        r#"
enum Item { Key(int), Coin(int) }
enum Tree<T> { Leaf(T), Node(int) }
struct Bag { v: Vec<Item> }
impl Bag {
    fn pick(self, i: int) -> int {
        let k = match i { 0 => Some(self.v[0]), _ => None }
        match k { Some(Item::Coin(n)) => n * 10, Some(Item::Key(n)) => n, None => 0 }
    }
}
fn gen<T>(v: Vec<Tree<T>>, c: bool) -> int {
    let t = if c { Some(v[0]) } else { None }
    match t { Some(Tree::Node(n)) => n, Some(Tree::Leaf(_)) => 1, None => 0 }
}
let b = Bag { v: vec![Item::Coin(4)] }
let f = fn(i: int) -> int {
    let k = if i == 0 { Some(Item::Key(3)) } else { None }
    match k { Some(Item::Key(n)) => n, _ => 0 }
}
let tv: Vec<Tree<int>> = vec![Tree::Node(7)]
b.pick(0) * 1000 + gen(tv, true) * 100 + gen(vec![Tree::Leaf("x")], true) * 10 + f(0)
"#,
        40_713,
    );
}

#[test]
fn question_mark_on_a_function_value_returning_the_generic_error_is_the_identity() {
    int_everywhere(
        r#"
fn twice<T>(f: fn(int) -> Result<int, T>, x: int) -> Result<int, T> {
    let a = f(x)?
    f(a)
}
fn inc(y: int) -> Result<int, string> { if y > 10 { Err("big {y}") } else { Ok(y + 4) } }
let first = match twice(inc, 1) { Ok(v) => v, Err(_) => -1 }
let second = match twice(inc, 8) { Ok(_) => -1, Err(e) => e.len() }
first * 100 + second
"#,
        906,
    );
    int_everywhere(
        r#"
fn twice<T>(f: fn(int) -> Result<int, T>, x: int) -> Result<int, T> {
    let a = f(x)?
    f(a)
}
1
"#,
        1,
    );
}

#[test]
fn question_mark_on_function_values_holding_generic_types_in_locals_options_and_methods() {
    int_everywhere(
        r#"
fn opt<T>(f: fn(int) -> Option<T>, x: int) -> Option<T> {
    let a = f(x)?
    Some(a)
}
fn half(n: int) -> Option<int> { if n % 2 == 0 { Some(n / 2) } else { None } }
fn twice<T>(f: fn(int) -> Result<int, T>, x: int) -> Result<int, T> {
    let g = f
    let a = g(x)?
    g(a)
}
fn inc(y: int) -> Result<int, string> { if y > 10 { Err("big {y}") } else { Ok(y + 4) } }
let a = match opt(half, 8) { Some(v) => v, None => -1 }
let b = match opt(half, 7) { Some(v) => v, None => -1 }
let c = match twice(inc, 2) { Ok(v) => v, Err(_) => -1 }
a * 10000 + (b + 1) * 1000 + c
"#,
        40_010,
    );
    int_everywhere(
        r#"
struct Runner<T> { f: fn(int) -> Result<int, T> }
impl<T> Runner<T> {
    fn run(self, x: int) -> Result<int, T> {
        let f = self.f
        let a = f(x)?
        f(a + 1)
    }
}
fn dbl(y: int) -> Result<int, string> { if y > 10 { Err("big") } else { Ok(y * 2) } }
let r = Runner { f: dbl }
let v = match r.run(2) { Ok(v) => v, Err(_) => -1 }
let w = match r.run(5) { Ok(v) => v, Err(e) => e.len() }
v * 100 + w
"#,
        1003,
    );
}

#[test]
fn an_unresolved_generic_body_names_the_type_parameter_not_the_function() {
    let message = rejected(
        "fn grab<T>(x: T) -> T {\n    let e = None\n    x\n}\ngrab(3)",
        "E0343",
    );
    assert!(message.contains("generic parameter 'T'"), "{message}");
    assert!(!message.contains("'grab'"), "{message}");
}

#[test]
fn a_nested_member_of_a_value_typed_after_solving_resolves() {
    int_everywhere(
        r#"
trait Prio { fn key(self) -> int; }
trait Queue<T> { fn take(mut self) -> Option<T>; }
struct Stack<T> { data: Vec<T> }
impl<T: Prio> Queue<T> for Stack<T> {
    fn take(mut self) -> Option<T> { self.data.pop() }
}
struct J { p: int }
impl Prio for J { fn key(self) -> int { self.p } }
struct Boosted<T> { inner: T, boost: int }
impl<T: Prio> Prio for Boosted<T> { fn key(self) -> int { self.inner.key() - self.boost } }
fn drain<Q: Queue<T>, T>(mut q: Q) -> Vec<T> {
    let mut out: Vec<T> = vec![]
    while let Some(x) = q.take() { out.push(x) }
    out
}
let mut s: Stack<Boosted<J>> = Stack { data: vec![Boosted { inner: J { p: 1 }, boost: 5 }] }
let order = drain(s)
let mut t = 0
for b in order { t = b.inner.p * 100 + b.boost * 10 + b.inner.key() }
t
"#,
        151,
    );
    int_everywhere(
        r#"
struct J { p: int }
impl J { fn twice(self) -> int { self.p * 2 } }
struct B<T> { inner: T, boost: int }
fn id<T>(x: T) -> Vec<T> { vec![x] }
fn one<T>(x: T) -> T { x }
let order = id(B { inner: B { inner: J { p: 3 }, boost: 1 }, boost: 5 })
let mut t = 0
for b in order { t = b.inner.inner.p * 100 + b.inner.inner.twice() * 10 + b.inner.boost }
let c = one(B { inner: J { p: 4 }, boost: 0 })
t * 10 + c.inner.p
"#,
        3614,
    );
}

#[test]
fn a_nested_member_typed_after_solving_formats_through_its_display() {
    string_everywhere(
        r#"
struct J { p: int }
impl Display for J { fn to_display(self) -> string { "J{self.p}" } }
struct Boosted<T> { inner: T, boost: int }
fn id<T>(x: T) -> Vec<T> { vec![x] }
let order = id(Boosted { inner: J { p: 3 }, boost: 5 })
let mut out = ""
for b in order { out = "{b.inner}/{b.inner.to_string()}/{b.boost}" }
out
"#,
        "J3/J3/5",
    );
}

#[test]
fn to_string_on_a_type_with_display_calls_its_display() {
    string_everywhere(
        r#"
struct J { p: int }
impl Display for J { fn to_display(self) -> string { "J{self.p}" } }
enum E { A, B(int) }
impl Display for E { fn to_display(self) -> string { match self { E::A => "a", E::B(n) => "b{n}" } } }
let j = J { p: 4 }
"{j.to_string()} {E::B(2).to_string()} {E::A.to_string()}"
"#,
        "J4 b2 a",
    );
}

#[test]
fn to_string_in_a_generic_body_dispatches_on_the_specialized_type() {
    string_everywhere(
        r#"
struct A { v: int }
impl A { fn to_string(self) -> string { "A<{self.v}>" } }
struct D { v: int }
impl Display for D { fn to_display(self) -> string { "D<{self.v}>" } }
struct Holder<T> { t: T }
impl<T> Holder<T> { fn show(self) -> string { self.t.to_string() } }
fn show<T>(t: T) -> string { t.to_string() }
let parts = vec![
    Holder { t: A { v: 1 } }.show(),
    Holder { t: D { v: 2 } }.show(),
    Holder { t: 5 }.show(),
    Holder { t: "s" }.show(),
    show(A { v: 3 }),
    show(D { v: 4 }),
    show((1, "a")),
    show(2.5),
]
parts.join(",")
"#,
        "A<1>,D<2>,5,s,A<3>,D<4>,(1, \"a\"),2.5",
    );
}

#[test]
fn to_string_in_a_generic_body_on_a_type_without_display_is_refused() {
    let held = rejected(
        "struct P { v: int }\nstruct Holder<T> { t: T }\nimpl<T> Holder<T> { fn show(self) -> string { self.t.to_string() } }\nHolder { t: P { v: 2 } }.show()",
        "E0338",
    );
    assert!(
        held.contains("'Display' is not implemented for P"),
        "{held}"
    );
    let free = rejected(
        "struct P { v: int }\nfn show<T>(t: T) -> string { t.to_string() }\nshow(P { v: 2 })",
        "E0338",
    );
    assert!(
        free.contains("'Display' is not implemented for P"),
        "{free}"
    );
    let nested = rejected(
        "struct P { v: int }\nstruct B<T> { inner: T }\nfn id<T>(x: T) -> Vec<T> { vec![x] }\nlet order = id(B { inner: P { v: 3 } })\nlet mut s = \"\"\nfor b in order { s = b.inner.to_string() }\ns",
        "E0338",
    );
    assert!(
        nested.contains("'Display' is not implemented for P"),
        "{nested}"
    );
    let direct = rejected(
        "struct P { v: int }\nlet p = P { v: 3 }\np.to_string()",
        "E0338",
    );
    assert_eq!(direct.matches("E0338").count(), 1, "{direct}");
}

#[test]
fn to_string_on_a_tuple_keeps_the_built_in_rendering() {
    string_everywhere("let t = (1, \"a\")\nt.to_string()", "(1, \"a\")");
}

fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "aelys-cli"])
                .status()
                .expect("cargo build should run");
            assert!(status.success(), "building aelys-cli failed");
            let mut directory = std::env::current_exe().expect("test executable path");
            directory.pop();
            if directory.ends_with("deps") {
                directory.pop();
            }
            let binary = directory.join(format!("aelys-cli{}", std::env::consts::EXE_SUFFIX));
            assert!(binary.is_file(), "missing aelys-cli at {binary:?}");
            binary
        })
        .clone()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/w3b")
        .join(name)
}

fn program_prints_its_oracle(program: &str) {
    let directory = fixture(program);
    let expected = std::fs::read_to_string(directory.join("expected.txt")).expect("oracle output");
    for flags in [&["-O0"][..], &["-O2"], &["-ae.jit=on"]] {
        let output = Command::new(cli_binary())
            .arg("run")
            .args(flags)
            .arg(directory.join("main.aelys"))
            .output()
            .expect("aelys-cli should run");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.trim_end() == expected.trim_end(),
            "{program} {flags:?} exited with {:?}\nstdout:\n{stdout}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn the_inventory_program_with_an_imported_default_method_prints_its_oracle() {
    program_prints_its_oracle("inventory");
}

#[test]
fn the_adventure_program_with_a_match_joined_binding_prints_its_oracle() {
    program_prints_its_oracle("adventure");
}

#[test]
fn the_heap_program_with_unannotated_bound_deduction_prints_its_oracle() {
    program_prints_its_oracle("heap");
}
