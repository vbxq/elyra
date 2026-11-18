use aelys::{new_vm_with_config, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{VM, VmConfig, parse_vm_args};

fn config(stress: bool, nursery: u64) -> VmConfig {
    VmConfig {
        gc_stress: stress,
        gc_nursery_bytes: nursery,
        ..VmConfig::default()
    }
}

fn run_int_with(config: VmConfig, source: &str) -> (i64, VM) {
    let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
    let value = run_with_vm_and_opt(&mut vm, source, "<t6gc>", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("program failed: {error}"));
    (value.as_int().expect("program must return an int"), vm)
}

fn configs() -> Vec<(&'static str, VmConfig)> {
    vec![
        ("stress", config(true, VmConfig::MIN_GC_NURSERY_BYTES)),
        ("small nursery", config(false, 4 * 1024)),
        (
            "verify",
            VmConfig {
                gc_verify: true,
                ..config(false, 4 * 1024)
            },
        ),
        ("default", VmConfig::default()),
    ]
}

fn assert_program(source: &str, expected: i64) {
    for (name, config) in configs() {
        let (value, vm) = run_int_with(config, source);
        assert_eq!(value, expected, "{name}: wrong value");
        if name != "default" {
            assert!(
                vm.heap().minor_collection_count() > 10,
                "{name}: the program must run many minor collections, ran {}",
                vm.heap().minor_collection_count()
            );
        }
        if name == "stress" {
            assert!(
                vm.heap().major_collection_count() > 2,
                "{name}: the program must complete several incremental majors, ran {}",
                vm.heap().major_collection_count()
            );
        }
    }
}

const FIELDS: &str = r#"
struct Inner { v: int }
struct Outer { a: Inner, b: Inner, tag: int }
fn main_value() -> int {
    let n = 120
    let mut keep: Vec<Outer> = vec![]
    for i in 0..n { keep.push(Outer { a: Inner { v: i }, b: Inner { v: i * 2 }, tag: i }) }
    let mut total = 0
    let mut round = 0
    while round < 600 {
        let mut o = keep[round % n]
        o.a = Inner { v: round * 3 }
        let mut p = keep[(round * 7) % n]
        p.b = Inner { v: round + 1000 }
        let mut s = 0
        for j in 0..n { s = s + keep[j].a.v + keep[j].b.v + keep[j].tag }
        total = total + s
        round = round + 1
    }
    return total
}
main_value()
"#;

fn fields_oracle() -> i64 {
    let n = 120usize;
    let mut a: Vec<i64> = (0..n as i64).collect();
    let mut b: Vec<i64> = (0..n as i64).map(|i| i * 2).collect();
    let tag: Vec<i64> = (0..n as i64).collect();
    let mut total = 0;
    for round in 0..600usize {
        a[round % n] = round as i64 * 3;
        b[(round * 7) % n] = round as i64 + 1000;
        total += (0..n).map(|j| a[j] + b[j] + tag[j]).sum::<i64>();
    }
    total
}

#[test]
fn struct_fields_replaced_in_old_objects_keep_their_values_across_collections() {
    assert_program(FIELDS, fields_oracle());
}

const VEC_SLOTS: &str = r#"
struct P { x: int, y: int }
fn main_value() -> int {
    let n = 200
    let mut v: Vec<P> = vec![]
    for k in 0..n { v.push(P { x: k, y: 1 }) }
    let mut arr = [P { x: 0, y: 0 }, P { x: 1, y: 0 }, P { x: 2, y: 0 }, P { x: 3, y: 0 }]
    let mut total = 0
    let mut i = 0
    while i < 3000 {
        v[(i * 13) % n] = P { x: i, y: 2 }
        arr[i % 4] = P { x: i * 5, y: 3 }
        if i % 50 == 0 {
            let mut s = 0
            for k in 0..n { s = s + v[k].x * 3 + v[k].y }
            for k in 0..4 { s = s + arr[k].x + arr[k].y }
            total = total + s
        }
        i = i + 1
    }
    return total
}
main_value()
"#;

fn vec_slots_oracle() -> i64 {
    let n = 200usize;
    let mut v: Vec<(i64, i64)> = (0..n as i64).map(|k| (k, 1)).collect();
    let mut arr: Vec<(i64, i64)> = (0..4).map(|k| (k, 0)).collect();
    let mut total = 0;
    for i in 0..3000usize {
        v[(i * 13) % n] = (i as i64, 2);
        arr[i % 4] = (i as i64 * 5, 3);
        if i % 50 == 0 {
            let mut s: i64 = v.iter().map(|(x, y)| x * 3 + y).sum();
            s += arr.iter().map(|(x, y)| x + y).sum::<i64>();
            total += s;
        }
    }
    total
}

#[test]
fn vec_and_array_slots_holding_young_structs_survive_minor_and_major_collections() {
    assert_program(VEC_SLOTS, vec_slots_oracle());
}

const ENUM_PAYLOADS: &str = r#"
struct Inner { v: int }
enum Shape { Circle(Inner), Pair(Inner, Inner), Empty }
fn score(s: Shape) -> int {
    return match s {
        Shape::Circle(inner) => inner.v,
        Shape::Pair(x, y) => x.v * 10 + y.v,
        Shape::Empty => 7,
    }
}
fn main_value() -> int {
    let n = 90
    let mut shapes: Vec<Shape> = vec![]
    for k in 0..n { shapes.push(Shape::Circle(Inner { v: k })) }
    let mut total = 0
    let mut i = 0
    while i < 2500 {
        let k = (i * 11) % n
        if i % 3 == 0 {
            shapes[k] = Shape::Pair(Inner { v: i }, Inner { v: i + 1 })
        } else if i % 3 == 1 {
            shapes[k] = Shape::Circle(Inner { v: i * 2 })
        } else {
            shapes[k] = Shape::Empty
        }
        if i % 40 == 0 {
            for j in 0..n { total = total + score(shapes[j]) }
        }
        i = i + 1
    }
    return total
}
main_value()
"#;

fn enum_payloads_oracle() -> i64 {
    let n = 90usize;
    let mut shapes: Vec<i64> = (0..n as i64).collect();
    let mut total = 0;
    for i in 0..2500i64 {
        let k = (i as usize * 11) % n;
        shapes[k] = match i % 3 {
            0 => i * 10 + i + 1,
            1 => i * 2,
            _ => 7,
        };
        if i % 40 == 0 {
            total += shapes.iter().sum::<i64>();
        }
    }
    total
}

#[test]
fn enum_payloads_stored_in_old_vec_slots_survive_collections() {
    assert_program(ENUM_PAYLOADS, enum_payloads_oracle());
}

const CLOSURES: &str = r#"
struct Inner { v: int }
fn make(seed: int) -> fn() -> int {
    let mut held = Inner { v: seed }
    return fn() -> int {
        held = Inner { v: held.v + seed + 1 }
        return held.v
    }
}
fn main_value() -> int {
    let n = 40
    let mut fs: Vec<fn() -> int> = vec![]
    for k in 0..n { fs.push(make(k)) }
    let mut total = 0
    let mut i = 0
    while i < 3000 {
        let f = fs[(i * 7) % n]
        total = total + f()
        if i % 500 == 0 {
            fs[i % n] = make(i)
        }
        i = i + 1
    }
    return total
}
main_value()
"#;

fn closures_oracle() -> i64 {
    let n = 40usize;
    let mut held: Vec<(i64, i64)> = (0..n as i64).map(|k| (k, k)).collect();
    let mut total = 0;
    for i in 0..3000usize {
        let slot = &mut held[(i * 7) % n];
        slot.0 = slot.0 + slot.1 + 1;
        total += slot.0;
        if i % 500 == 0 {
            held[i % n] = (i as i64, i as i64);
        }
    }
    total
}

#[test]
fn closure_upvalues_replaced_with_young_structs_survive_collections() {
    assert_program(CLOSURES, closures_oracle());
}

const GRAPH: &str = r#"
struct Node { id: int, kids: Vec<Node> }
fn next(s: int) -> int {
    return (s * 75 + 74) % 65537
}
fn main_value() -> int {
    let mut all: Vec<Node> = vec![]
    for k in 0..30 { all.push(Node { id: k, kids: vec![] }) }
    let mut next_id = 30
    let mut seed = 99
    let mut total = 0
    let mut st = 0
    while st < 1500 {
        seed = next(seed)
        let op = seed % 4
        seed = next(seed)
        let a = seed % 30
        seed = next(seed)
        let b = seed % 30
        if op == 0 {
            all[a] = Node { id: next_id, kids: vec![] }
            next_id = next_id + 1
        } else if op == 1 {
            let mut owner = all[a]
            let mut ks = owner.kids
            ks.push(all[b])
        } else if op == 2 {
            let mut owner = all[a]
            let mut ks = owner.kids
            if ks.len() > 0 {
                ks[b % ks.len()] = all[b]
            } else {
                ks.push(Node { id: next_id, kids: vec![] })
                next_id = next_id + 1
            }
        } else {
            let mut owner = all[a]
            let mut ks = owner.kids
            ks.push(Node { id: next_id, kids: vec![all[b]] })
            next_id = next_id + 1
        }
        if st % 25 == 0 {
            for k in 0..30 {
                let node = all[k]
                total = total + node.id * 31
                let ks = node.kids
                for j in 0..ks.len() {
                    let kid = ks[j]
                    total = total + kid.id * (j + 1)
                    let gk = kid.kids
                    if gk.len() > 0 { total = total + gk[0].id }
                }
            }
        }
        st = st + 1
    }
    return total
}
main_value()
"#;

fn graph_oracle() -> i64 {
    fn next(s: i64) -> i64 {
        (s * 75 + 74) % 65537
    }
    let mut ids: Vec<i64> = Vec::new();
    let mut kids: Vec<Vec<usize>> = Vec::new();
    let new_node =
        |id: i64, children: Vec<usize>, ids: &mut Vec<i64>, kids: &mut Vec<Vec<usize>>| {
            ids.push(id);
            kids.push(children);
            ids.len() - 1
        };
    let mut all: Vec<usize> = Vec::new();
    for k in 0..30 {
        all.push(new_node(k, Vec::new(), &mut ids, &mut kids));
    }
    let mut next_id = 30;
    let mut seed = 99;
    let mut total = 0;
    for step in 0..1500 {
        seed = next(seed);
        let op = seed % 4;
        seed = next(seed);
        let a = (seed % 30) as usize;
        seed = next(seed);
        let b = (seed % 30) as usize;
        match op {
            0 => {
                all[a] = new_node(next_id, Vec::new(), &mut ids, &mut kids);
                next_id += 1;
            }
            1 => {
                let target = all[b];
                kids[all[a]].push(target);
            }
            2 => {
                let owner = all[a];
                if !kids[owner].is_empty() {
                    let len = kids[owner].len();
                    kids[owner][b % len] = all[b];
                } else {
                    let fresh = new_node(next_id, Vec::new(), &mut ids, &mut kids);
                    next_id += 1;
                    kids[owner].push(fresh);
                }
            }
            _ => {
                let fresh = new_node(next_id, vec![all[b]], &mut ids, &mut kids);
                next_id += 1;
                kids[all[a]].push(fresh);
            }
        }
        if step % 25 == 0 {
            for &node in &all {
                total += ids[node] * 31;
                for (j, kid) in kids[node].iter().enumerate() {
                    total += ids[*kid] * (j as i64 + 1);
                    if let Some(first) = kids[*kid].first() {
                        total += ids[*first];
                    }
                }
            }
        }
    }
    total
}

#[test]
fn random_struct_graph_with_shared_nodes_and_cycles_matches_the_oracle() {
    assert_program(GRAPH, graph_oracle());
}

const BUG1_SMALL: &str = r#"
struct P { x: int, y: int }
fn main_value() -> int {
    let n = 30000
    let mut v: Vec<P> = vec![]
    for k in 0..n { v.push(P { x: k, y: 1 }) }
    let mut i = 0
    while i < 120000 {
        v[i % n] = P { x: i, y: 2 }
        i = i + 1
    }
    let mut s = 0
    for k in 0..n { s = s + v[k].y + v[k].x }
    return s
}
main_value()
"#;

#[test]
fn large_vec_rewritten_during_incremental_marking_completes_with_bounded_heap() {
    let n = 30000i64;
    let iterations = 120000i64;
    let expected: i64 = (0..n)
        .map(|k| {
            let last = iterations - 1 - ((iterations - 1 - k) % n);
            last + 2
        })
        .sum();
    for (name, config) in [
        ("default", VmConfig::default()),
        ("small nursery", config(false, 16 * 1024)),
        ("stress", config(true, VmConfig::MIN_GC_NURSERY_BYTES)),
    ] {
        let (value, vm) = run_int_with(config, BUG1_SMALL);
        assert_eq!(value, expected, "{name}");
        assert!(
            vm.heap().major_collection_count() > 0 || vm.heap().major_collection_active(),
            "{name}: no major started"
        );
        assert!(
            vm.heap().peak_bytes() < 32 * 1024 * 1024,
            "{name}: heap peaked at {} bytes",
            vm.heap().peak_bytes()
        );
        if name == "stress" {
            assert!(
                vm.heap().major_collection_count() > 5,
                "{name}: majors did not complete"
            );
        }
    }
}

#[test]
fn gc_statistics_are_readable_from_the_program() {
    let source = r#"
use std::sys
struct P { x: int, y: int }
let mut v: Vec<P> = vec![]
for k in 0..5000 { v.push(P { x: k, y: k }) }
let mut i = 0
while i < 40000 {
    v[i % 5000] = P { x: i, y: 1 }
    i = i + 1
}
let minor = sys::gc_minor_collections()
let pauses = sys::gc_pause_count()
let max_us = sys::gc_max_pause_us()
let window = sys::gc_window_max_pause_us()
let window_after = sys::gc_window_max_pause_us()
let p99 = sys::gc_p99_pause_us()
let total = sys::gc_total_pause_us()
let line = sys::gc_stats()
let mut ok = 0
if minor > 0 { ok = ok + 1 }
if pauses >= minor { ok = ok + 10 }
if max_us >= p99 { ok = ok + 100 }
if total >= max_us { ok = ok + 1000 }
if window <= max_us { ok = ok + 10000 }
if window_after == 0 { ok = ok + 100000 }
if sys::gc_allocations() > 45000 { ok = ok + 1000000 }
if sys::gc_live_objects() > 5000 { ok = ok + 10000000 }
if sys::gc_heap_bytes() > 0 { ok = ok + 100000000 }
if line.contains("minor=") { ok = ok + 1000000000 }
ok
"#;
    let (value, vm) = run_int_with(config(false, 16 * 1024), source);
    assert_eq!(value, 1_111_111_111);
    let stats = vm.gc_pause_stats();
    assert!(stats.count() >= vm.heap().minor_collection_count());
    assert!(stats.max_nanos() >= stats.percentile_nanos(99.0));
    let report = vm.gc_stats_report();
    assert!(report.contains("max_pause_us="), "{report}");
    assert!(report.contains("p99_pause_us="), "{report}");
    assert!(report.contains("pause histogram"), "{report}");
}

#[test]
fn gc_collect_from_the_program_reclaims_unreachable_objects() {
    let source = r#"
use std::sys
struct P { x: int, y: int }
fn check() -> int {
    sys::gc_collect()
    let base = sys::gc_live_objects()
    let mut sink = 0
    for k in 0..3000 {
        let p = P { x: k, y: k }
        sink = sink + p.x
    }
    let peak = sys::gc_live_objects()
    sys::gc_collect()
    let after = sys::gc_live_objects()
    if sink != 4498500 { return 0 - 1 }
    if after > base + 10 { return 0 - 2 }
    return peak - after
}
check()
"#;
    let (value, _vm) = run_int_with(VmConfig::default(), source);
    assert!(value >= 2990, "only {value} objects were reclaimed");
}

#[test]
fn gc_vm_flags_parse_and_reject_bad_values() {
    let args = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
    let parsed = parse_vm_args(&args(&[
        "-ae.gc-stats=on",
        "--ae-gc-nursery=64K",
        "-ae.gc-stress=off",
        "-ae.gc-verify=on",
    ]))
    .expect("valid flags");
    assert!(parsed.config.gc_verify);
    assert!(!VmConfig::default().gc_verify);
    assert!(parse_vm_args(&args(&["-ae.gc-verify=sometimes"])).is_err());
    assert!(parsed.config.gc_stats);
    assert_eq!(parsed.config.gc_nursery_bytes, 64 * 1024);
    assert!(!parsed.config.gc_stress);
    assert!(parse_vm_args(&args(&["-ae.gc-stats=maybe"])).is_err());
    assert!(parse_vm_args(&args(&["-ae.gc-nursery=10"])).is_err());
    assert!(!VmConfig::default().gc_stats);
}

const MOVED_TO_LOCALS: &str = r#"
struct Inner { v: int }
struct Holder { inner: Inner }
fn main_value() -> int {
    let n = 64
    let mut hs: Vec<Holder> = vec![]
    for k in 0..n { hs.push(Holder { inner: Inner { v: k } }) }
    let mut total = 0
    let mut i = 0
    while i < 4000 {
        let mut h = hs[(i * 5) % n]
        let taken = h.inner
        h.inner = Inner { v: i + 100000 }
        let mut pad = 0
        for j in 0..6 {
            let junk = Inner { v: j }
            pad = pad + junk.v
        }
        let again = taken
        total = total + again.v + pad
        let mut back = hs[(i * 3) % n]
        back.inner = taken
        i = i + 1
    }
    for k in 0..n { total = total + hs[k].inner.v }
    return total
}
main_value()
"#;

fn moved_to_locals_oracle() -> i64 {
    let n = 64usize;
    let mut inner: Vec<i64> = (0..n as i64).collect();
    let mut total = 0;
    for i in 0..4000usize {
        let taken = inner[(i * 5) % n];
        inner[(i * 5) % n] = i as i64 + 100000;
        total += taken + 15;
        inner[(i * 3) % n] = taken;
    }
    total + inner.iter().sum::<i64>()
}

#[test]
fn objects_moved_from_the_heap_into_locals_during_marking_survive_the_major() {
    assert_program(MOVED_TO_LOCALS, moved_to_locals_oracle());
}

const ROUNDS_OF_FRESH_VECS: &str = r#"
struct P { x: int, y: int }
let mut t = 0
for r in 0..10 {
    let mut v: Vec<P> = vec![]
    for i in 0..100000 { v.push(P { x: 1, y: 2 }) }
    t += v.len()
}
t
"#;

#[test]
fn rounds_of_fresh_large_vecs_complete_with_a_bounded_heap() {
    let (value, vm) = run_int_with(VmConfig::default(), ROUNDS_OF_FRESH_VECS);
    assert_eq!(value, 1_000_000);
    assert!(
        vm.heap().major_collection_count() > 0 || vm.heap().major_collection_active(),
        "no major started"
    );
    assert!(
        vm.heap().peak_bytes() < 40 * 1024 * 1024,
        "heap peaked at {} bytes",
        vm.heap().peak_bytes()
    );
}
