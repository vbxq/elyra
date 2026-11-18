use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 2] = [OptimizationLevel::None, OptimizationLevel::Standard];

const JITS: [JitMode; 2] = [JitMode::Off, JitMode::Tiered];

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
            .map_err(|e| format!("{e:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn assert_int_everywhere(source: &str, expected: i64) {
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

fn assert_rejected_with(source: &str, code: &str) {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected with {code}:\n{source}"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains(code), "expected {code}, got:\n{text}");
        }
    }
}

// debug sema frames are tens of kilobytes per nesting level; the cli main thread has 8 MiB, a test thread 2 MiB
fn on_main_sized_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the body must not panic");
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write source");
    path
}

const LIST: &str = r#"
enum L { Nil, Cons(int, L) }
fn build(n: int) -> L {
    let mut l = L::Nil
    for i in 0..n { l = L::Cons(i, l) }
    l
}
fn total(l: L) -> int {
    let mut acc = 0
    let mut cur = l
    let mut go = true
    while go {
        match cur {
            L::Cons(h, t) => {
                acc = acc + h
                cur = t
            }
            L::Nil => { go = false }
        }
    }
    acc
}
fn depth(l: L) -> int {
    match l {
        L::Cons(_, t) => 1 + depth(t),
        L::Nil => 0,
    }
}
fn top_two(l: L) -> int {
    match l {
        L::Cons(a, L::Cons(b, _)) => a * 1000 + b,
        L::Cons(a, L::Nil) => a,
        L::Nil => -1,
    }
}
"#;

#[test]
fn a_recursive_enum_list_one_and_two_deep_matches_back_through_its_payload() {
    assert_int_everywhere(&format!("{LIST}top_two(build(1))"), 0);
    assert_int_everywhere(&format!("{LIST}top_two(build(2))"), 1000);
    assert_int_everywhere(&format!("{LIST}top_two(build(3))"), 2001);
    assert_int_everywhere(&format!("{LIST}top_two(build(0))"), -1);
}

#[test]
fn a_recursive_enum_list_one_hundred_deep_is_built_and_walked_recursively() {
    assert_int_everywhere(
        &format!("{LIST}depth(build(100)) * 1000 + total(build(100))"),
        104950,
    );
}

#[test]
fn the_reviewed_one_hundred_deep_list_reproducer_reads_its_head() {
    assert_int_everywhere(
        "enum L { Nil, Cons(int, L) }\nlet mut l = L::Nil\nfor i in 0..100 { l = L::Cons(i, l) }\nmatch l { L::Cons(h, _) => h, L::Nil => 0 }",
        99,
    );
}

#[test]
fn a_recursive_enum_list_one_hundred_thousand_deep_survives_collection_and_sums() {
    assert_int_everywhere(&format!("{LIST}total(build(100000))"), 4_999_950_000);
}

#[test]
fn a_recursive_enum_stored_in_a_struct_field_and_a_global_keeps_its_depth() {
    assert_int_everywhere(
        &format!(
            "{LIST}struct Holder {{ list: L, tag: int }}\nlet mut h = Holder {{ list: build(200), tag: 1 }}\nh.list = L::Cons(500, h.list)\nh.tag = depth(h.list)\nh.tag * 1000 + top_two(h.list) % 1000"
        ),
        201_199,
    );
}

#[test]
fn two_mutually_recursive_enums_nest_two_hundred_levels() {
    assert_int_everywhere(
        r#"
enum A { End, Next(int, B) }
enum B { Stop, More(int, A) }
fn build(n: int) -> A {
    let mut a = A::End
    let mut i = 0
    while i < n {
        a = A::Next(i, B::More(i * 2, a))
        i = i + 1
    }
    a
}
fn sum_a(a: A) -> int {
    match a {
        A::Next(x, b) => x + sum_b(b),
        A::End => 0,
    }
}
fn sum_b(b: B) -> int {
    match b {
        B::More(y, a) => y + sum_a(a),
        B::Stop => 0,
    }
}
sum_a(build(200))
"#,
        59700,
    );
}

#[test]
fn a_struct_recursive_through_option_is_walked_with_while_let() {
    assert_int_everywhere(
        r#"
struct Node { v: int, next: Option<Node> }
fn walk(head: Option<Node>) -> int {
    let mut acc = 0
    let mut cur = head
    while let Some(n) = cur {
        acc = acc + n.v
        cur = n.next
    }
    acc
}
let mut head: Option<Node> = None
for i in 0..300 { head = Some(Node { v: i, next: head }) }
walk(head)
"#,
        44850,
    );
}

#[test]
fn a_generic_recursive_tree_inserts_five_hundred_keys_and_a_degenerate_chain() {
    assert_int_everywhere(
        r#"
enum Tree<T> { Leaf, Node(Tree<T>, T, Tree<T>) }
fn insert(t: Tree<int>, x: int) -> Tree<int> {
    match t {
        Tree::Leaf => Tree::Node(Tree::Leaf, x, Tree::Leaf),
        Tree::Node(l, v, r) => if x < v { Tree::Node(insert(l, x), v, r) } else { Tree::Node(l, v, insert(r, x)) },
    }
}
fn sum(t: Tree<int>) -> int {
    match t {
        Tree::Leaf => 0,
        Tree::Node(l, v, r) => sum(l) + v + sum(r),
    }
}
fn height(t: Tree<int>) -> int {
    match t {
        Tree::Leaf => 0,
        Tree::Node(l, _, r) => {
            let a = height(l)
            let b = height(r)
            if a > b { a + 1 } else { b + 1 }
        }
    }
}
let mut t: Tree<int> = Tree::Leaf
let mut k = 7
for _ in 0..500 {
    k = (k * 31 + 11) % 1009
    t = insert(t, k)
}
let mut chain: Tree<int> = Tree::Leaf
for i in 0..150 { chain = insert(chain, i) }
sum(t) * 1000 + height(chain)
"#,
        256_144_150,
    );
}

#[test]
fn an_enum_recursive_through_a_vec_nests_one_hundred_twenty_levels() {
    assert_int_everywhere(
        r#"
enum T { Leaf(int), Branch(Vec<T>) }
fn sum(t: T) -> int {
    match t {
        T::Leaf(v) => v,
        T::Branch(children) => {
            let mut acc = 0
            for c in children { acc = acc + sum(c) }
            acc
        }
    }
}
let mut t = T::Leaf(1)
for i in 0..120 { t = T::Branch(vec![t, T::Leaf(i)]) }
sum(t)
"#,
        7141,
    );
}

#[test]
fn a_recursive_enum_declared_in_a_module_is_built_there_and_matched_in_the_importer() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "lists.aelys",
        "pub enum L { Nil, Cons(int, L) }\npub fn build(n: int) -> L {\n    let mut l = L::Nil\n    for i in 0..n { l = L::Cons(i, l) }\n    l\n}\n",
    );
    let main = write(
        &dir,
        "main.aelys",
        "needs lists\nfn count(l: L) -> int {\n    match l {\n        L::Cons(_, t) => 1 + count(t),\n        L::Nil => 0,\n    }\n}\nlet l = L::Cons(-1, lists::build(150))\nlet head = match l { L::Cons(h, L::Cons(n, _)) => h * 1000 + n, _ => 0 }\ncount(l) * 100000 + head\n",
    );
    let value = run_file(&main).expect("a module-built recursive enum must run");
    assert_eq!(value.as_int(), Some(151 * 100_000 - 1000 + 149));
}

fn chain_program(calls: usize) -> String {
    format!(
        "struct C {{ n: int }}\nimpl C {{\n    fn inc(self) -> C {{ C {{ n: self.n + 1 }} }}\n}}\nlet c = C {{ n: 0 }}\nlet r = c{}.n\nr",
        ".inc()".repeat(calls)
    )
}

fn assert_compiles_quickly(source: &str, bound: Duration) {
    let started = Instant::now();
    let compiled = Runtime::new().compile(
        source,
        CompileOptions {
            optimization_level: OptimizationLevel::Standard,
            ..CompileOptions::default()
        },
    );
    let elapsed = started.elapsed();
    if let Err(error) = compiled {
        panic!("the chain must compile: {error}\n{source}");
    }
    assert!(
        elapsed < bound,
        "compiling took {elapsed:?}, bound {bound:?}:\n{source}"
    );
}

#[test]
fn the_reviewed_eleven_call_method_chain_compiles_quickly_and_counts_eleven() {
    on_main_sized_stack(|| {
        let source = chain_program(11);
        assert_compiles_quickly(&source, Duration::from_secs(2));
        assert_int_everywhere(&source, 11);
    });
}

#[test]
fn a_ninety_nine_call_method_chain_compiles_in_linear_time_and_counts_ninety_nine() {
    on_main_sized_stack(|| {
        let source = chain_program(99);
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 99);
    });
}

#[test]
fn a_two_hundred_call_method_chain_is_the_depth_limit_diagnostic_not_an_exhaustion() {
    on_main_sized_stack(|| {
        let source = chain_program(200);
        let started = Instant::now();
        assert_rejected_with(&source, "E0368");
        assert!(started.elapsed() < Duration::from_secs(5));
    });
}

#[test]
fn user_methods_named_like_builtin_methods_chain_without_reinference() {
    on_main_sized_stack(|| {
        let names = [
            "len", "push", "filter", "get", "trim", "fold", "collect", "contains", "clone",
            "is_empty",
        ];
        let methods = names
            .iter()
            .map(|name| format!("    fn {name}(self) -> S {{ S {{ n: self.n + 1 }} }}\n"))
            .collect::<String>();
        let calls = (0..48)
            .map(|index| format!(".{}()", names[index % names.len()]))
            .collect::<String>();
        let source =
            format!("struct S {{ n: int }}\nimpl S {{\n{methods}}}\nS {{ n: 0 }}{calls}.n");
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 48);
    });
}

#[test]
fn a_mixed_method_and_field_chain_compiles_quickly() {
    on_main_sized_stack(|| {
        let calls = ".b().c".repeat(45);
        let source = format!(
            "struct A {{ n: int }}\nstruct B {{ c: A }}\nimpl A {{\n    fn b(self) -> B {{ B {{ c: A {{ n: self.n + 1 }} }} }}\n}}\nlet a = A {{ n: 0 }}\na{calls}.n"
        );
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 45);
    });
}

#[test]
fn a_long_field_chain_through_nested_structs_reads_the_innermost_value() {
    on_main_sized_stack(|| {
        let depth = 24;
        let mut source = String::from("struct S0 { n: int }\nlet s0 = S0 { n: 77 }\n");
        for level in 1..=depth {
            source.push_str(&format!(
                "struct S{level} {{ inner: S{prev} }}\nlet s{level} = S{level} {{ inner: s{prev} }}\n",
                prev = level - 1
            ));
        }
        source.push_str(&format!("s{depth}{}.n", ".inner".repeat(depth)));
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 77);
    });
}

#[test]
fn method_chains_in_a_function_a_closure_and_a_generic_struct_compile_quickly() {
    on_main_sized_stack(|| {
        let calls = ".inc()".repeat(40);
        let source = format!(
            "struct G<T> {{ n: int, tag: T }}\nimpl<T> G<T> {{\n    fn inc(self) -> G<T> {{ G {{ n: self.n + 1, tag: self.tag }} }}\n}}\nfn in_fn(g: G<bool>) -> int {{ g{calls}.n }}\nlet in_closure = fn(g: G<string>) -> int {{ g{calls}.n }}\nin_fn(G {{ n: 0, tag: true }}) * 100 + in_closure(G {{ n: 1, tag: \"x\" }})"
        );
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 4041);
    });
}

#[test]
fn string_and_option_method_chains_compile_quickly() {
    on_main_sized_stack(|| {
        let source = format!(
            "let s = \"  ab  \"{}\nlet o = Some(1){}\ns.len() * 1000 + o.unwrap_or(0)",
            ".trim()".repeat(60),
            ".map(fn(x: int) -> int {{ x + 1 }})".repeat(30)
        );
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 2031);
    });
}

#[test]
fn a_long_collection_pipeline_compiles_quickly() {
    on_main_sized_stack(|| {
        let source = format!(
            "let v = vec![1, 2, 3]\nlet w: Vec<int> = v.iter(){}.collect()\nw[0] * 100 + w.len()",
            ".map(fn(x: int) -> int {{ x + 1 }}).filter(fn(x: int) -> bool {{ x > 1 }})".repeat(30)
        );
        assert_compiles_quickly(&source, Duration::from_secs(5));
        assert_int_everywhere(&source, 3103);
    });
}

#[test]
fn an_option_reassigned_at_the_end_of_a_loop_body_is_read_by_the_next_iteration() {
    assert_int_everywhere(
        "fn f() -> int {\n    let mut k = 0\n    let mut o: Option<int> = Some(5)\n    while let Some(v) = o {\n        k = k + v\n        o = if v > 1 { Some(v - 1) } else { None }\n    }\n    k\n}\nf()",
        15,
    );
    assert_int_everywhere(
        "fn f() -> int {\n    let mut k = 0\n    let mut o: Option<int> = Some(1)\n    for i in 0..4 {\n        k = k + o.unwrap_or(0)\n        o = Some(i * 10)\n    }\n    k\n}\nf()",
        31,
    );
    assert_int_everywhere(
        "fn f() -> int {\n    let mut k = 0\n    let mut r: Result<int, string> = Ok(2)\n    loop {\n        if k > 5 { break }\n        k = k + r.unwrap_or(0)\n        r = Ok(k)\n    }\n    k\n}\nf()",
        8,
    );
}

#[test]
fn an_option_assigned_in_a_loop_and_never_read_is_still_unused() {
    assert_rejected_with(
        "fn f() -> int {\n    let mut o: Option<int> = Some(1)\n    for i in 0..2 { o = Some(i) }\n    0\n}\nf()",
        "E0304",
    );
    assert_rejected_with(
        "fn f() -> int {\n    let mut o: Option<int> = Some(1)\n    let mut i = 0\n    while i < 2 {\n        let t = o\n        t.unwrap_or(0)\n        i = i + 1\n    }\n    o = None\n    i\n}\nf()",
        "E0304",
    );
}
