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
                        source_name: "<t3b-merge>".to_string(),
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

fn assert_rejected(source: &str, code: &str) {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains(code), "expected {code}:\n{message}");
            assert!(
                !message.contains("dynamic"),
                "an internal type name leaked:\n{message}"
            );
        }
    }
}

#[test]
fn string_push_writes_back_through_a_mutable_root_place() {
    assert_int_everywhere(
        r#"
struct P { name: string, tags: Vec<string> }
impl P { fn mark(mut self) { self.name.push('!') } }
let mut p = P { name: "n", tags: vec!["a", "b"] }
p.name.push('2')
p.mark()
let mut v = vec!["x", "y"]
v[1].push('?')
let mut m: Map<int, string> = Map::new()
m[1] = "q"
m[1].push('r')
let mut ps = vec![P { name: "a", tags: vec![] }]
ps[0].name.push('b')
(ps[0]).name.push('c')
let mut grid = [["", ""], ["", ""]]
grid[1][0].push('g')
let f = fn() { p.tags[1].push('c') }
f()
f()
fn tag<T>(mut q: P, x: T) -> string {
    q.tags[0].push('z')
    q.tags[0]
}
let tagged = tag(P { name: "", tags: vec!["y"] }, 0)
let mut ok = 0
if p.name == "n2!" { ok += 1 }
if v[0] == "x" && v[1] == "y?" { ok += 1 }
if m[1] == "qr" { ok += 1 }
if ps[0].name == "abc" { ok += 1 }
if grid[1][0] == "g" && grid[0][0] == "" { ok += 1 }
if p.tags[1] == "bcc" && p.tags[0] == "a" { ok += 1 }
if tagged == "yz" { ok += 1 }
ok
"#,
        7,
    );
}

#[test]
fn string_push_on_an_element_evaluates_each_index_once() {
    assert_int_everywhere(
        r#"
struct P { tags: Vec<string> }
let mut calls = 0
let next = fn() -> int {
    calls += 1
    calls - 1
}
let mut v = vec!["a", "b", "c"]
v[next()].push('1')
let mut p = P { tags: vec!["x", "y"] }
p.tags[next() - 1].push('2')
let mut rows = vec![vec!["", ""], vec!["", ""]]
rows[next() - 1][next() - 2].push('3')
let mut ok = 0
if v[0] == "a1" && v[1] == "b" { ok += 1 }
if p.tags[0] == "x2" && p.tags[1] == "y" { ok += 1 }
if rows[1][1] == "3" && rows[0][0] == "" { ok += 1 }
ok * 10 + calls
"#,
        34,
    );
}

#[test]
fn mutating_methods_follow_the_root_of_the_place() {
    assert_int_everywhere(
        r#"
struct H { items: Vec<int>, m: Map<string, int> }
impl H {
    fn add(mut self, k: string) {
        let _ = self.m.insert(k, 1)
        self.items.push(3)
    }
}
let mut h = H { items: vec![1], m: Map::new() }
h.items.insert(0, 9)
h.items.push(4)
h.add("z")
let _ = h.m.insert("a", 2)
let mut vv = vec![vec![1], vec![2]]
vv[1].push(7)
vv[0].clear()
h.items.len() * 100 + h.m.len() * 10 + vv[1].len() + vv[0].len()
"#,
        422,
    );
}

#[test]
fn mutating_methods_on_an_immutable_root_are_rejected() {
    let header = "struct P { name: string, tags: Vec<string>, m: Map<string, int> }\n";
    for body in [
        "let p = P { name: \"n\", tags: vec![], m: Map::new() }\np.name.push('a')",
        "let p = P { name: \"n\", tags: vec![\"a\"], m: Map::new() }\np.tags[0].push('a')",
        "let p = P { name: \"n\", tags: vec![], m: Map::new() }\np.tags.push(\"a\")",
        "let p = P { name: \"n\", tags: vec![], m: Map::new() }\nlet _ = p.m.insert(\"a\", 1)",
        "let v = vec![\"a\"]\nv[0].push('a')",
        "let vv = vec![vec![1]]\nvv[0].insert(0, 2)",
        "fn f(p: P) { p.name.push('a') }",
        "fn f(p: P) { p.tags.clear() }",
        "impl P { fn g(self) { self.name.push('a') } }",
        "impl P { fn g(self) { let _ = self.m.remove(\"a\") } }",
    ] {
        assert_rejected(&format!("{header}{body}\n0"), "E0318");
    }
}

#[test]
fn map_clone_is_deep() {
    assert_int_everywhere(
        r#"
let mut m: Map<string, Vec<int>> = Map::new()
let _ = m.insert("a", vec![1])
let mut c = m.clone()
c["a"].push(2)
let _ = c.insert("b", vec![])
let mut nested: Map<int, Map<int, int>> = Map::new()
nested[0] = Map::new()
let mut copy = nested.clone()
copy[0][5] = 5
m["a"].len() * 1000 + c["a"].len() * 100 + m.len() * 10 + nested[0].len() + copy[0].len()
"#,
        1211,
    );
}

#[test]
fn a_repeated_map_gives_every_slot_its_own_map() {
    assert_int_everywhere(
        r#"
let mut m: Map<string, Vec<int>> = Map::new()
let _ = m.insert("a", vec![1])
let mut ms = vec![m; 3]
let _ = ms[0].insert("z", vec![])
ms[1]["a"].push(9)
let mut fixed = [Map::new(); 2]
fixed[0][1] = 1
let _: Map<int, int> = fixed[1]
ms[0].len() * 10000 + ms[2].len() * 1000 + ms[1]["a"].len() * 100 + ms[2]["a"].len() * 10 + fixed[1].len()
"#,
        21210,
    );
}

#[test]
fn a_mutable_alias_of_an_immutable_map_is_rejected() {
    for body in [
        "let m: Map<int, int> = Map::new()\nlet mut m2 = m",
        "let m: Map<int, int> = Map::new()\nlet mut q: Map<int, int> = Map::new()\nq = m",
        "struct H { m: Map<int, int> }\nlet h = H { m: Map::new() }\nlet mut m2 = h.m",
    ] {
        assert_rejected(&format!("{body}\n0"), "E0323");
    }
}

#[test]
fn fill_and_resize_copy_an_aggregate_into_every_slot() {
    assert_int_everywhere(
        r#"
struct S { x: int }
let mut v = vec![S { x: 0 }; 2]
v.fill(S { x: 5 })
v[0].x = 7
v.resize(4, S { x: 1 })
v[2].x = 8
let mut maps: Vec<Map<int, int>> = vec![]
maps.resize(2, Map::new())
maps[0][1] = 1
let mut arr = [vec![0], vec![0]]
arr.fill(vec![3])
arr[0].push(4)
let mut opts = vec![None, None]
opts.fill(Some(S { x: 1 }))
let mut first = opts[0].unwrap()
first.x = 6
let second = match opts[1] {
    Some(s) => s.x,
    None => 0,
}
let mut ok = 0
if v[0].x == 7 && v[1].x == 5 { ok += 1 }
if v[2].x == 8 && v[3].x == 1 { ok += 1 }
if maps[0].len() == 1 && maps[1].len() == 0 { ok += 1 }
if arr[0].len() == 2 && arr[1].len() == 1 { ok += 1 }
if second == 1 { ok += 1 }
ok
"#,
        5,
    );
}

#[test]
fn generic_fill_and_resize_copy_an_aggregate_into_every_slot() {
    assert_int_everywhere(
        r#"
struct P { k: int }
fn fill_it<T>(mut v: Vec<T>, x: T) { v.fill(x) }
fn fill_fixed<T>(mut a: [T; 2], x: T) { a.fill(x) }
fn grow<T>(mut v: Vec<T>, x: T) { v.resize(3, x) }
let mut ps = vec![P { k: 0 }, P { k: 0 }]
fill_it(ps, P { k: 2 })
ps[0].k = 3
let mut fixed = [vec![0], vec![0]]
fill_fixed(fixed, vec![1, 1])
fixed[0].push(1)
let mut ms: Vec<Map<int, int>> = vec![Map::new()]
grow(ms, Map::new())
let one = 1
let two = 2
ms[one][0] = 0
let mut ok = 0
if ps[0].k == 3 && ps[1].k == 2 { ok += 1 }
if fixed[0].len() == 3 && fixed[1].len() == 2 { ok += 1 }
if ms.len() == 3 && ms[one].len() == 1 && ms[two].len() == 0 { ok += 1 }
ok
"#,
        3,
    );
}

#[test]
fn compound_assignment_on_a_map_evaluates_the_key_once_everywhere() {
    assert_int_everywhere(
        r#"
let mut calls = 0
let mut m: Map<int, int> = Map::new()
m[0] = 100
m[1] = 200
let next = fn() -> int {
    calls += 1
    calls - 1
}
m[next()] += 5
let mut names: Map<string, string> = Map::new()
names["a"] = "x"
let key = fn() -> string {
    calls += 1
    "a"
}
names[key()] += "y"
let mut ok = 0
if names["a"] == "xy" { ok += 1 }
calls * 10000 + m[0] * 10 + ok
"#,
        21051,
    );
}
