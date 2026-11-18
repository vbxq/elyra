mod common;
use common::*;

#[test]
fn insert_get_contains_remove_len() {
    assert_aelys_str(
        r#"
let mut m: Map<string, int> = Map::new()
let first = m.insert("a", 1)
let second = m.insert("a", 2)
let b = m.insert("b", 3)
let got = m.get("a")
let none = m.get("zz")
let removed = m.remove("b")
let again = m.remove("b")
"{first.unwrap_or(-1)} {second.unwrap()} {b.unwrap_or(-1)} {got.unwrap()} {none.unwrap_or(-1)} {removed.unwrap()} {again.unwrap_or(-1)} {m.len()} {m.contains_key("a")} {m.contains_key("b")} {m.is_empty()}"
"#,
        "-1 1 -1 2 -1 3 -1 1 true false false",
    );
}

#[test]
fn index_read_write_and_compound_assignment() {
    assert_aelys_str(
        r#"
let mut m = Map::new()
m[10] = "ten"
m[20] = "twenty"
m[10] = m[10] + "!"
let mut n: Map<int, int> = Map::new()
n[1] = 5
n[1] += 4
n[1] *= 2
m[10] + " {n[1]} {m.len()}"
"#,
        "ten! 18 2",
    );
}

#[test]
fn reading_a_missing_key_traps_with_the_key() {
    assert_aelys_error_contains(
        "let mut m: Map<string, int> = Map::new()\nm[\"a\"] = 1\nm[\"nope\"]",
        "map key not found: \"nope\"",
    );
    assert_aelys_error_contains(
        "let m: Map<int, int> = Map::new()\nm[7]",
        "map key not found: 7",
    );
}

#[test]
fn iteration_follows_insertion_order_across_removals() {
    assert_aelys_str(
        r#"
let mut m: Map<string, int> = Map::new()
m["c"] = 1
m["a"] = 2
m["b"] = 3
m["a"] = 20
let _ = m.remove("c")
m["d"] = 4
m["c"] = 5
let mut out = ""
for k in m.keys() { out += k }
for v in m.values() { out += "{v}" }
out
"#,
        "abdc20345",
    );
}

#[test]
fn many_removals_compact_without_reordering() {
    assert_aelys_str(
        r#"
let mut m: Map<int, int> = Map::new()
for i in 0..1000 { m[i] = i * 2 }
for i in 0..1000 {
    if i % 10 != 3 { let _ = m.remove(i) }
}
let ks = m.keys()
"{m.len()} {ks[0]} {ks[1]} {ks[99]} {m[993]}"
"#,
        "100 3 13 993 1986",
    );
}

#[test]
fn key_types_int_char_bool_sized_and_string() {
    assert_aelys_str(
        r#"
let mut c: Map<char, int> = Map::new()
for ch in "abracadabra" {
    if c.contains_key(ch) { c[ch] += 1 } else { c[ch] = 1 }
}
let mut b: Map<bool, string> = Map::new()
b[true] = "yes"
b[false] = "no"
let mut s: Map<u8, int> = Map::new()
s[255] = 1
s[0] = 2
"{c['a']}{c['b']}{c['r']}{c['c']}{c['d']} {c.len()} " + b[true] + b[false] + " {s[255]}{s[0]}"
"#,
        "52211 5 yesno 12",
    );
}

#[test]
fn invalid_key_types_are_rejected_with_a_named_error() {
    for program in [
        "let m: Map<float, int> = Map::new()\n0",
        "let mut m = Map::new()\nm[1.5] = 2\n0",
        "struct P { x: int }\nlet m: Map<P, int> = Map::new()\n0",
        "let m: Map<Vec<int>, int> = Map::new()\n0",
    ] {
        let err = run_aelys_err(program);
        assert!(
            err.contains("E0470") && err.contains("cannot be a Map key"),
            "{program}: {err}"
        );
    }
}

#[test]
fn equality_is_structural_and_order_insensitive() {
    assert_aelys_str(
        r#"
let mut a: Map<string, Vec<int>> = Map::new()
a["x"] = vec![1]
a["y"] = vec![2, 3]
let mut b: Map<string, Vec<int>> = Map::new()
b["y"] = vec![2, 3]
b["x"] = vec![1]
let mut c: Map<string, Vec<int>> = Map::new()
c["x"] = vec![1]
c["y"] = vec![2, 4]
"{a == b} {a == c} {a != c}"
"#,
        "true false true",
    );
}

#[test]
fn display_renders_entries_in_insertion_order() {
    assert_aelys_str(
        "let mut m: Map<string, int> = Map::new()\nm[\"b\"] = 2\nm[\"a\"] = 1\nlet e: Map<int, bool> = Map::new()\nto_string(m) + \" \" + to_string(e)",
        "{b: 2, a: 1} {}",
    );
}

#[test]
fn maps_have_reference_semantics() {
    assert_aelys_int(
        r#"
fn add(mut m: Map<string, int>) { m["k"] = 7 }
let mut m: Map<string, int> = Map::new()
add(m)
struct Holder { table: Map<string, int> }
let h = Holder { table: m }
m["j"] = 1
h.table["k"] + h.table.len()
"#,
        9,
    );
}

#[test]
fn clear_then_reuse() {
    assert_aelys_str(
        "let mut m: Map<int, int> = Map::new()\nm[1] = 1\nm.clear()\nm[2] = 2\n\"{m.len()} {m.contains_key(1)} {m[2]}\"",
        "1 false 2",
    );
}

#[test]
fn a_map_cannot_be_iterated_directly() {
    let err = run_aelys_err("let m: Map<int, int> = Map::new()\nfor k in m { }\n0");
    assert!(err.contains("E0365") && err.contains("keys()"), "{err}");
}

#[test]
fn mutation_needs_a_mutable_binding() {
    let err = run_aelys_err("let m: Map<int, int> = Map::new()\nm.clear()\n0");
    assert!(err.contains("E0318"), "{err}");
    let err = run_aelys_err("let m: Map<int, int> = Map::new()\nm[1] = 2\n0");
    assert!(err.contains("E0318"), "{err}");
}

#[test]
fn values_survive_garbage_collection() {
    assert_aelys_int(
        r#"
let mut m: Map<string, string> = Map::new()
for i in 0..30000 { m["key{i}"] = "value{i}" }
let mut total = 0
for i in 0..30000 { total += m["key{i}"].len() }
let _ = m.remove("key5")
total + m.len()
"#,
        (0..30000)
            .map(|i: i64| format!("value{i}").len() as i64)
            .sum::<i64>()
            + 29999,
    );
}

#[test]
fn generic_function_over_a_map() {
    assert_aelys_int(
        r#"
fn count<K>(m: Map<K, int>, k: K) -> int {
    match m.get(k) {
        Some(n) => n,
        None => 0,
    }
}
let mut m: Map<string, int> = Map::new()
m["a"] = 4
count(m, "a") * 10 + count(m, "b")
"#,
        40,
    );
}
