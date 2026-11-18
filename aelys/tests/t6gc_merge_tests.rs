use aelys::{new_vm_with_config, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{VM, VmConfig};
use std::collections::HashMap;

const M: i64 = 1_000_000_007;

const PRELUDE: &str = r#"
struct P { x: int }
fn mk(i: int) -> Option<int> { Some(i) }
fn churn(k: int) -> int {
    let mut s = 0
    for j in 0..k {
        let p = P { x: j }
        s = s + p.x
    }
    s
}
fn weigh(v: Vec<Option<int>>) -> int {
    let mut s = 0
    for k in 0..v.len() { s = (s + v[k].unwrap() * (k + 1)) % 1000000007 }
    s
}
fn weigh_p(v: Vec<P>) -> int {
    let mut s = 0
    for k in 0..v.len() { s = (s + v[k].x * (k + 1)) % 1000000007 }
    s
}
fn hash(s: string) -> int {
    let mut h = 7
    for c in s.chars() { h = (h * 31 + (c as int)) % 1000000007 }
    h
}
"#;

fn configs() -> Vec<(&'static str, VmConfig)> {
    vec![
        (
            "stress",
            VmConfig {
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..VmConfig::default()
            },
        ),
        (
            "small nursery",
            VmConfig {
                gc_nursery_bytes: 4 * 1024,
                ..VmConfig::default()
            },
        ),
        (
            "verify",
            VmConfig {
                gc_verify: true,
                gc_nursery_bytes: 4 * 1024,
                ..VmConfig::default()
            },
        ),
        ("default", VmConfig::default()),
    ]
}

fn run_with(config: VmConfig, source: &str) -> (i64, VM) {
    let program = format!("{PRELUDE}\n{source}");
    let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
    let value = run_with_vm_and_opt(
        &mut vm,
        &program,
        "<t6gc-merge>",
        OptimizationLevel::Standard,
    )
    .unwrap_or_else(|error| panic!("program failed: {error}"));
    (value.as_int().expect("program must return an int"), vm)
}

fn assert_program(source: &str, expected: i64) {
    for (name, config) in configs() {
        let (value, vm) = run_with(config, source);
        assert_eq!(value, expected, "{name}: wrong value");
        if name != "default" {
            assert!(
                vm.heap().minor_collection_count() > 20,
                "{name}: the program must run many minor collections, ran {}",
                vm.heap().minor_collection_count()
            );
        }
    }
}

fn weigh(v: &[i64]) -> i64 {
    v.iter()
        .enumerate()
        .fold(0, |s, (k, x)| (s + x * (k as i64 + 1)).rem_euclid(M))
}

fn hash(s: &str) -> i64 {
    s.chars().fold(7, |h, c| (h * 31 + c as i64) % M)
}

fn seeded(n: i64) -> Vec<i64> {
    (0..n).collect()
}

#[test]
fn vec_insert_moves_a_young_element_of_an_old_vec_to_a_new_index() {
    let source = r#"
let filler = Some(-1)
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    v.push(mk(r * 7))
    v.insert(r % v.len(), filler)
    let _ = churn(60)
    total = (total + weigh(v)) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut total = 0;
    for r in 0..300i64 {
        v.push(r * 7);
        let at = (r as usize) % v.len();
        v.insert(at, -1);
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_remove_moves_a_young_element_of_an_old_vec_down() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    v.push(mk(r * 7))
    let _ = v.remove(r % (v.len() - 1))
    let _ = churn(60)
    total = (total + weigh(v)) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut total = 0;
    for r in 0..300i64 {
        v.push(r * 7);
        let at = (r as usize) % (v.len() - 1);
        v.remove(at);
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_swap_remove_moves_the_young_last_element_into_an_old_vec_slot() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    v.push(mk(r * 7))
    let _ = v.swap_remove(r % (v.len() - 1))
    let _ = churn(60)
    total = (total + weigh(v)) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut total = 0;
    for r in 0..300i64 {
        v.push(r * 7);
        let at = (r as usize) % (v.len() - 1);
        v.swap_remove(at);
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_and_array_swap_move_a_young_element_to_another_slot() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let mut a = [Some(1), Some(2), Some(3), Some(4)]
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    v.push(mk(r * 7))
    v.swap(r % (v.len() - 1), v.len() - 1)
    a[3] = mk(r)
    a.swap(3, r % 3)
    let _ = churn(60)
    let mut s = weigh(v)
    for k in 0..4 { s = s + a[k].unwrap() * (k + 1) }
    total = (total + s) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut a = [1i64, 2, 3, 4];
    let mut total = 0;
    for r in 0..300i64 {
        v.push(r * 7);
        let last = v.len() - 1;
        v.swap((r as usize) % last, last);
        a[3] = r;
        a.swap(3, (r as usize) % 3);
        let s = weigh(&v)
            + a.iter()
                .enumerate()
                .map(|(k, x)| x * (k as i64 + 1))
                .sum::<i64>();
        total = (total + s) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_reverse_moves_a_young_element_to_the_front_of_an_old_vec() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    v.push(mk(r * 7))
    v.reverse()
    let _ = churn(60)
    total = (total + weigh(v)) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut total = 0;
    for r in 0..300i64 {
        v.push(r * 7);
        v.reverse();
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_sort_moves_young_strings_inside_an_old_vec() {
    let source = r#"
let mut v: Vec<string> = vec![]
for i in 0..40 { v.push("{10000 + (i * 7919) % 90000}") }
let _ = churn(300)
let mut total = 0
for r in 0..200 {
    v.push("{10000 + (r * 104729 + 13) % 90000}")
    v.sort()
    let _ = churn(60)
    total = (total + hash(v.join(","))) % 1000000007
}
total
"#;
    let mut v: Vec<String> = (0..40i64)
        .map(|i| format!("{}", 10000 + (i * 7919) % 90000))
        .collect();
    let mut total = 0;
    for r in 0..200i64 {
        v.push(format!("{}", 10000 + (r * 104729 + 13) % 90000));
        v.sort();
        total = (total + hash(&v.join(","))) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_sort_by_moves_young_elements_inside_an_old_vec() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..40 { v.push(Some((i * 7919) % 1000)) }
let _ = churn(300)
let mut total = 0
for r in 0..150 {
    v.push(mk((r * 104729 + 13) % 1000))
    v.sort_by(fn(a: Option<int>, b: Option<int>) -> int { a.unwrap() - b.unwrap() })
    let _ = churn(60)
    total = (total + weigh(v)) % 1000000007
}
total
"#;
    let mut v: Vec<i64> = (0..40i64).map(|i| (i * 7919) % 1000).collect();
    let mut total = 0;
    for r in 0..150i64 {
        v.push((r * 104729 + 13) % 1000);
        v.sort();
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_extend_append_and_truncate_store_young_elements_into_an_old_vec() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..50 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..200 {
    let mut src: Vec<Option<int>> = vec![]
    src.push(mk(r))
    src.push(mk(r + 1000))
    v.extend(src)
    let mut more: Vec<Option<int>> = vec![]
    more.push(mk(r + 2000))
    more.push(mk(r + 3000))
    v.append(more)
    if v.len() > 160 { v.truncate(70) }
    let _ = churn(60)
    total = (total + weigh(v) + more.len()) % 1000000007
}
total
"#;
    let mut v = seeded(50);
    let mut total = 0;
    for r in 0..200i64 {
        v.extend([r, r + 1000]);
        v.extend([r + 2000, r + 3000]);
        if v.len() > 160 {
            v.truncate(70);
        }
        total = (total + weigh(&v)) % M;
    }
    assert_program(source, total);
}

#[test]
fn vec_resize_fill_and_set_store_fresh_young_structs_into_an_old_vec() {
    let source = r#"
let mut v: Vec<P> = vec![]
for i in 0..60 { v.push(P { x: i }) }
let mut o: Vec<Option<int>> = vec![]
for i in 0..30 { o.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..200 {
    v.resize(v.len() + 2, P { x: r })
    v.set(r % v.len(), P { x: r * 3 })
    o.set(r % 30, mk(r * 5))
    if r % 25 == 24 { v.fill(P { x: r }) }
    if v.len() > 150 { v.resize(60, P { x: 0 }) }
    let _ = churn(60)
    total = (total + weigh_p(v) + weigh(o)) % 1000000007
}
total
"#;
    let mut v = seeded(60);
    let mut o = seeded(30);
    let mut total = 0;
    for r in 0..200i64 {
        v.extend([r, r]);
        let at = (r as usize) % v.len();
        v[at] = r * 3;
        o[(r as usize) % 30] = r * 5;
        if r % 25 == 24 {
            v.fill(r);
        }
        if v.len() > 150 {
            v.truncate(60);
        }
        total = (total + weigh(&v) + weigh(&o)) % M;
    }
    assert_program(source, total);
}

#[test]
fn map_insert_index_remove_and_clear_with_young_values_in_an_old_map() {
    let source = r#"
let mut m: Map<int, P> = Map::new()
for i in 0..40 { m[i] = P { x: i } }
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    let _ = m.insert(r % 97, P { x: r * 5 })
    m[(r * 7) % 89] = P { x: r + 1 }
    if r % 5 == 0 { let _ = m.remove((r * 3) % 97) }
    if r % 100 == 99 { m.clear() }
    let _ = churn(60)
    let mut s = 0
    for k in m.keys() { s = (s + k * 31 + m[k].x) % 1000000007 }
    total = (total + s) % 1000000007
}
total
"#;
    let mut m: HashMap<i64, i64> = (0..40).map(|i| (i, i)).collect();
    let mut total = 0;
    for r in 0..300i64 {
        m.insert(r % 97, r * 5);
        m.insert((r * 7) % 89, r + 1);
        if r % 5 == 0 {
            m.remove(&((r * 3) % 97));
        }
        if r % 100 == 99 {
            m.clear();
        }
        let s = m.iter().fold(0, |s, (k, x)| (s + k * 31 + x) % M);
        total = (total + s) % M;
    }
    assert_program(source, total);
}

#[test]
fn map_keeps_young_string_keys_stored_into_an_old_map() {
    let source = r#"
let mut m: Map<string, int> = Map::new()
m["seed"] = 1
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    let _ = m.insert("k{r % 150}", r)
    let _ = churn(60)
    let mut s = 0
    for k in m.keys() { s = (s + hash(k) + m[k]) % 1000000007 }
    total = (total + s) % 1000000007
}
total
"#;
    let mut m: HashMap<String, i64> = HashMap::from([("seed".to_string(), 1)]);
    let mut total = 0;
    for r in 0..300i64 {
        m.insert(format!("k{}", r % 150), r);
        let s = m.iter().fold(0, |s, (k, x)| (s + hash(k) + x) % M);
        total = (total + s) % M;
    }
    assert_program(source, total);
}

#[test]
fn deep_clone_of_an_old_graph_keeps_every_copied_child() {
    let source = r#"
struct Holder { items: Vec<P>, tag: Option<int>, name: string }
fn items_for(i: int) -> Vec<P> {
    let mut items: Vec<P> = vec![]
    for j in 0..8 { items.push(P { x: i * 10 + j }) }
    items
}
fn weigh_h(v: Vec<Holder>) -> int {
    let mut s = 0
    for k in 0..v.len() {
        s = (s + weigh_p(v[k].items) * (k + 1) + v[k].tag.unwrap() + hash(v[k].name)) % 1000000007
    }
    s
}
let mut base: Vec<Holder> = vec![]
for i in 0..60 { base.push(Holder { items: items_for(i), tag: Some(i), name: "h{i}" }) }
let _ = churn(300)
let mut total = 0
for r in 0..40 {
    let mut c = base.clone()
    c[r % 60].items[r % 8] = P { x: 0 - r }
    c[(r * 7) % 60].tag = mk(r * 11)
    let _ = churn(30)
    total = (total + weigh_h(c) * 3 + weigh_h(base)) % 1000000007
}
total
"#;
    #[derive(Clone)]
    struct Holder {
        items: Vec<i64>,
        tag: i64,
        name: String,
    }
    let weigh_h = |v: &[Holder]| {
        v.iter().enumerate().fold(0, |s, (k, h)| {
            (s + weigh(&h.items) * (k as i64 + 1) + h.tag + hash(&h.name)) % M
        })
    };
    let base: Vec<Holder> = (0..60i64)
        .map(|i| Holder {
            items: (0..8).map(|j| i * 10 + j).collect(),
            tag: i,
            name: format!("h{i}"),
        })
        .collect();
    let mut total = 0;
    for r in 0..40i64 {
        let mut c = base.clone();
        c[(r % 60) as usize].items[(r % 8) as usize] = -r;
        c[((r * 7) % 60) as usize].tag = r * 11;
        total = (total + weigh_h(&c) * 3 + weigh_h(&base)) % M;
    }
    assert_program(source, total);
}

#[test]
fn string_building_through_fields_elements_and_split_keeps_young_strings() {
    let source = r#"
struct Named { name: string, n: int }
let mut people: Vec<Named> = vec![]
for i in 0..30 { people.push(Named { name: "p{i}", n: i }) }
let mut words: Vec<string> = vec![]
for i in 0..30 { words.push("w{i}") }
let mut joined: Vec<string> = vec![]
for i in 0..1500 { joined.push("{i * 37}") }
let text = joined.join(",")
let _ = churn(300)
let mut total = 0
for r in 0..200 {
    let k = r % 30
    people[k].name.push('a')
    words[k].push('b')
    people[(k * 7) % 30].name += "{r % 10}"
    if r % 50 == 49 { people[k].name = "p{k}" }
    let _ = churn(40)
    let mut s = 0
    for i in 0..30 { s = (s + hash(people[i].name) * (i + 1) + hash(words[i])) % 1000000007 }
    total = (total + s) % 1000000007
}
let parts = text.split(",")
let mut t = 0
for i in 0..parts.len() { t = (t + hash(parts[i]) * (i + 1)) % 1000000007 }
(total + t) % 1000000007
"#;
    let mut people: Vec<String> = (0..30).map(|i| format!("p{i}")).collect();
    let mut words: Vec<String> = (0..30).map(|i| format!("w{i}")).collect();
    let mut total = 0;
    for r in 0..200usize {
        let k = r % 30;
        people[k].push('a');
        words[k].push('b');
        people[(k * 7) % 30].push_str(&format!("{}", r % 10));
        if r % 50 == 49 {
            people[k] = format!("p{k}");
        }
        let s = (0..30).fold(0, |s, i| {
            (s + hash(&people[i]) * (i as i64 + 1) + hash(&words[i])) % M
        });
        total = (total + s) % M;
    }
    let t = (0..1500i64).fold(0, |t, i| (t + hash(&format!("{}", i * 37)) * (i + 1)) % M);
    assert_program(source, (total + t) % M);
}

#[test]
fn nested_place_writes_methods_and_closures_store_young_structs_into_old_objects() {
    let source = r#"
struct Inner { v: int }
struct Mid { a: Vec<Inner>, b: Inner }
struct Outer { m: Mid, items: Vec<Mid> }
impl Outer {
    fn bump(mut self, r: int) { self.m.b = Inner { v: r * 9 } }
}
fn mid_for(i: int) -> Mid {
    let mut a: Vec<Inner> = vec![]
    for j in 0..4 { a.push(Inner { v: i + j }) }
    Mid { a: a, b: Inner { v: i } }
}
fn counter(seed: int) -> fn() -> int {
    let mut held = Inner { v: seed }
    return fn() -> int {
        held = Inner { v: held.v + 1 }
        return held.v
    }
}
let mut items: Vec<Mid> = vec![]
for i in 0..20 { items.push(mid_for(i)) }
let mut w = Outer { m: mid_for(100), items: items }
let tick = counter(5)
let _ = churn(300)
let mut total = 0
for r in 0..300 {
    w.m.a[r % 4] = Inner { v: r }
    w.items[r % 20].b = Inner { v: r * 2 }
    w.items[(r * 3) % 20].a[r % 4] = Inner { v: r * 4 }
    w.items[(r * 5) % 20].a[(r + 1) % 4].v += 3
    w.bump(r)
    let t = tick()
    let _ = churn(60)
    let mut s = w.m.b.v + t
    for j in 0..4 { s = s + w.m.a[j].v * (j + 1) }
    for i in 0..20 {
        s = s + w.items[i].b.v * (i + 1)
        for j in 0..4 { s = s + w.items[i].a[j].v * (j + 2) }
    }
    total = (total + s) % 1000000007
}
total
"#;
    let mut top: Vec<i64> = (0..4).map(|j| 100 + j).collect();
    let mut b: Vec<i64> = (0..20).collect();
    let mut a: Vec<Vec<i64>> = (0..20i64)
        .map(|i| (0..4).map(|j| i + j).collect())
        .collect();
    let mut held = 5i64;
    let mut total = 0;
    for r in 0..300i64 {
        let ru = r as usize;
        top[ru % 4] = r;
        b[ru % 20] = r * 2;
        a[(ru * 3) % 20][ru % 4] = r * 4;
        a[(ru * 5) % 20][(ru + 1) % 4] += 3;
        let mb = r * 9;
        held += 1;
        let mut s = mb + held;
        for (j, x) in top.iter().enumerate() {
            s += x * (j as i64 + 1);
        }
        for (i, row) in a.iter().enumerate() {
            s += b[i] * (i as i64 + 1);
            for (j, x) in row.iter().enumerate() {
                s += x * (j as i64 + 2);
            }
        }
        total = (total + s) % M;
    }
    assert_program(source, total);
}

#[test]
fn a_large_old_vec_reordered_while_a_major_marks_it_keeps_every_element() {
    let source = r#"
let mut v: Vec<Option<int>> = vec![]
for i in 0..6000 { v.push(Some(i)) }
let _ = churn(300)
let mut total = 0
for r in 0..12 {
    v.reverse()
    let _ = churn(200)
    v.swap(r, 5999 - r)
    let _ = v.remove(r * 400)
    v.insert(r * 300, mk(r))
    let _ = churn(200)
    total = (total + weigh(v)) % 1000000007
}
v.sort_by(fn(a: Option<int>, b: Option<int>) -> int { b.unwrap() - a.unwrap() })
let _ = churn(300)
(total + weigh(v)) % 1000000007
"#;
    let mut v = seeded(6000);
    let mut total = 0;
    for r in 0..12i64 {
        v.reverse();
        v.swap(r as usize, (5999 - r) as usize);
        v.remove((r * 400) as usize);
        v.insert((r * 300) as usize, r);
        total = (total + weigh(&v)) % M;
    }
    v.sort_by(|a, b| b.cmp(a));
    assert_program(source, (total + weigh(&v)) % M);
}

#[test]
fn interned_strings_freed_by_minor_and_major_sweeps_leave_the_intern_table() {
    let mut heap = aelys_bytecode::Heap::new();
    let kept = heap.intern_string("kept");
    for i in 0..500 {
        heap.intern_string(&format!("young{i}"));
    }
    assert_eq!(heap.interned_count(), 501);
    heap.mark_young([kept]);
    heap.sweep_young();
    assert_eq!(
        heap.interned_count(),
        1,
        "a minor sweep left dead interned strings"
    );
    for i in 0..300 {
        heap.intern_string(&format!("old{i}"));
    }
    heap.mark_young([kept]);
    heap.sweep_young();
    heap.mark(kept);
    heap.sweep();
    assert_eq!(
        heap.interned_count(),
        1,
        "a major sweep left dead interned strings"
    );
    assert_eq!(heap.find_interned_string("kept"), Some(kept));
}

#[test]
fn string_buffer_bytes_are_charged_at_allocation_and_returned_by_the_sweep() {
    let mut heap = aelys_bytecode::Heap::new();
    let base = heap.bytes_allocated();
    let big = "x".repeat(1 << 20);
    let kept = heap.alloc_string(&big);
    for _ in 0..20 {
        heap.alloc_string(&big);
    }
    assert!(
        heap.bytes_allocated() >= base + 21 * (1 << 20),
        "string buffers were not charged: {}",
        heap.bytes_allocated()
    );
    heap.mark_young([kept]);
    heap.sweep_young();
    let after = heap.bytes_allocated() - base;
    assert!(
        ((1 << 20)..(1 << 20) + 4096).contains(&after),
        "freed string buffers were not returned: {after} bytes still charged"
    );
}
