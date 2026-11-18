mod common;
use common::*;

use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;

fn timings(source: &str) -> (i64, i64) {
    let mut vm = new_vm().expect("a vm starts");
    let value = run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .expect("the program runs");
    let pointer = value.as_ptr().expect("the program returns a string");
    let heap = vm.heap();
    let object = heap
        .get(aelys_runtime::vm::GcRef::new(pointer))
        .expect("a live string");
    let aelys_runtime::vm::ObjectKind::String(text) = &object.kind else {
        panic!("the program returns a string");
    };
    let mut parts = text
        .as_str()
        .split(' ')
        .map(|part| part.parse::<i64>().unwrap());
    (parts.next().unwrap(), parts.next().unwrap())
}

fn assert_constant_time(field: &str, build: &str, use_site: &str) {
    let source = format!(
        "needs std::time as time\nstruct H {{ f: {field}, n: int }}\nenum E {{ Holds({field}) }}\n{build}\nlet big = make(200000)\nlet small = make(1)\nlet mut h = H {{ f: make(1), n: 0 }}\nfn spin(x: {field}, rounds: int, mut h: H) -> int {{\n    let mut total = 0\n    for i in 0..rounds {{\n        {use_site}\n    }}\n    total\n}}\nlet t0 = time::now_ms()\nlet a = spin(big, 60, h)\nlet t1 = time::now_ms()\nlet b = spin(small, 60, h)\nlet t2 = time::now_ms()\n\"{{t1 - t0}} {{t2 - t1}}\"\n"
    );
    let (big, small) = timings(&source);
    assert!(
        big <= small * 10 + 150,
        "{field} via `{use_site}`: 60 checks of a 200000-element value took {big} ms against {small} ms for one element"
    );
}

const MAP_BUILD: &str = "fn make(n: int) -> Map<int, int> {\n    let mut m: Map<int, int> = Map::new()\n    for i in 0..n { m[i] = i }\n    m\n}";
const VEC_BUILD: &str = "fn make(n: int) -> Vec<string> {\n    let mut v: Vec<string> = vec![]\n    for i in 0..n { v.push(\"s\") }\n    v\n}";
const OPT_MAP_BUILD: &str = "fn make(n: int) -> Option<Map<int, int>> {\n    let mut m: Map<int, int> = Map::new()\n    for i in 0..n { m[i] = i }\n    Some(m)\n}";

#[test]
fn a_container_field_is_checked_in_constant_time() {
    for (field, build) in [
        ("Map<int, int>", MAP_BUILD),
        ("Vec<string>", VEC_BUILD),
        ("Option<Map<int, int>>", OPT_MAP_BUILD),
    ] {
        for use_site in [
            "let k = H { f: x, n: i }\n        total += k.n",
            "h.f = x\n        total += h.n",
            "let e = E::Holds(x)\n        total += i",
        ] {
            if field.starts_with("Option") && use_site.starts_with("h.f") {
                continue;
            }
            assert_constant_time(field, build, use_site);
        }
    }
}

#[test]
fn container_fields_still_hold_their_values() {
    assert_aelys_str(
        r#"
struct H { m: Map<string, int>, v: Vec<int>, a: [float; 2], w: Vec<Vec<string>> }
let mut m: Map<string, int> = Map::new()
m["a"] = 1
let mut h = H { m: m, v: vec![1, 2], a: [0.5, 1.5], w: vec![vec!["x"]] }
let mut m2: Map<string, int> = Map::new()
m2["b"] = 2
h.m = m2
h.v = vec![]
let e: Vec<int> = vec![]
h.v = e
"{h.m.len()} {h.v.len()} {h.a[1]} {h.w.len()}"
"#,
        "1 0 1.5 1",
    );
}
