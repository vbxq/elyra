mod common;
use common::*;

const SHOW: &str = r#"
fn show(v: Vec<int>) -> string {
    let mut s = ""
    for x in v { s += "{x}," }
    s
}
"#;

fn with_show(body: &str) -> String {
    format!("{SHOW}\n{body}")
}

#[test]
fn insert_at_front_middle_and_end() {
    assert_aelys_str(
        &with_show(
            "let mut v = vec![1, 2, 3]\nv.insert(0, 9)\nv.insert(2, 8)\nv.insert(5, 7)\nshow(v)",
        ),
        "9,1,8,2,3,7,",
    );
}

#[test]
fn insert_past_the_end_traps_with_the_index() {
    assert_aelys_error_contains(
        "let mut v = vec![1, 2]\nv.insert(3, 0)\n0",
        "index 3 is out of bounds for length 2",
    );
}

#[test]
fn remove_returns_the_element_and_shifts() {
    assert_aelys_str(
        &with_show("let mut v = vec![10, 20, 30, 40]\nlet r = v.remove(1)\n\"{r}|\" + show(v)"),
        "20|10,30,40,",
    );
}

#[test]
fn remove_out_of_bounds_and_negative_trap() {
    assert_aelys_error_contains(
        "let mut v = vec![1]\nlet r = v.remove(1)\nr",
        "index 1 is out of bounds for length 1",
    );
    assert_aelys_error_contains(
        "let mut v = vec![1, 2, 3]\nlet r = v.remove(-1)\nr",
        "index -1 is out of bounds for length 3",
    );
}

#[test]
fn swap_remove_moves_the_last_element_in() {
    assert_aelys_str(
        &with_show("let mut v = vec![1, 2, 3, 4]\nlet r = v.swap_remove(0)\n\"{r}|\" + show(v)"),
        "1|4,2,3,",
    );
}

#[test]
fn clear_truncate_and_resize() {
    assert_aelys_str(
        &with_show(
            "let mut v = vec![1, 2, 3, 4, 5]\nv.truncate(3)\nlet a = show(v)\nv.truncate(10)\nv.resize(5, 0)\nlet b = show(v)\nv.resize(1, 9)\nlet c = show(v)\nv.clear()\na + \"|\" + b + \"|\" + c + \"|{v.len()}{v.is_empty()}\"",
        ),
        "1,2,3,|1,2,3,0,0,|1,|0true",
    );
}

#[test]
fn extend_from_vec_array_and_itself_and_append_empties_source() {
    assert_aelys_str(
        &with_show(
            "let mut v = vec![1]\nlet w = vec![2, 3]\nv.extend(w)\nv.extend([4, 5])\nv.extend(v)\nlet mut src = vec![6]\nv.append(src)\nshow(v) + \"|{src.len()}|{w.len()}\"",
        ),
        "1,2,3,4,5,1,2,3,4,5,6,|0|2",
    );
}

#[test]
fn swap_and_reverse_on_vec_and_array() {
    assert_aelys_str(
        &with_show(
            "let mut v = vec![1, 2, 3, 4]\nv.swap(0, 3)\nv.reverse()\nlet mut a = [7, 8, 9]\na.reverse()\na.swap(0, 1)\nshow(v) + \"|{a[0]}{a[1]}{a[2]}\"",
        ),
        "1,3,2,4,|897",
    );
}

#[test]
fn contains_and_index_of_are_structural_and_not_string_methods() {
    assert_aelys_str(
        r#"
let v = vec!["ab", "cd", "ef"]
let a = [3, 1, 4]
let found = v.index_of("cd")
let missing = v.index_of("zz")
let pos = a.index_of(4)
"{v.contains("ef")} {v.contains("e")} {a.contains(1)} {found.unwrap()} {missing.unwrap_or(-1)} {pos.unwrap()}"
"#,
        "true false true 1 -1 2",
    );
}

#[test]
fn contains_on_a_vec_of_vecs_compares_contents() {
    assert_aelys_bool(
        "let v = vec![vec![1, 2], vec![3]]\nv.contains(vec![3])",
        true,
    );
}

#[test]
fn sort_ints_floats_strings_chars_bools() {
    assert_aelys_str(
        r#"
let mut i = vec![5, -2, 9, 0, -2]
i.sort()
let mut f = vec![2.5, -1.0, 0.5]
f.sort()
let mut s = vec!["pear", "apple", "Zebra", "apples"]
s.sort()
let mut c = vec!['b', 'A', 'a']
c.sort()
let mut b = vec![true, false, true]
b.sort()
"{i[0]},{i[1]},{i[4]}|{f[0]},{f[2]}|" + s.join(",") + "|{c[0]}{c[1]}{c[2]}|{b[0]}"
"#,
        "-2,-2,9|-1.0,2.5|Zebra,apple,apples,pear|Aab|false",
    );
}

#[test]
fn sort_on_a_mutable_fixed_array() {
    assert_aelys_str(
        "let mut a = [3, 1, 2]\na.sort()\n\"{a[0]}{a[1]}{a[2]}\"",
        "123",
    );
}

#[test]
fn sort_by_is_stable_and_uses_the_comparator() {
    assert_aelys_str(
        r#"
struct P { key: int, tag: string }
let mut v = vec![P { key: 2, tag: "a" }, P { key: 1, tag: "b" }, P { key: 2, tag: "c" }, P { key: 1, tag: "d" }, P { key: 0, tag: "e" }]
v.sort_by(fn(x: P, y: P) -> int { x.key - y.key })
let mut out = ""
for p in v { out += p.tag }
out
"#,
        "ebdac",
    );
}

#[test]
fn sort_by_descending_on_a_large_vec_matches_sort() {
    assert_aelys_bool(
        r#"
let mut v: Vec<int> = vec![]
let mut x = 12345
for i in 0..2000 {
    x = (x * 1103 + 12345) % 65536
    v.push(x)
}
let mut w = vec![]
w.extend(v)
v.sort_by(fn(a: int, b: int) -> int { b - a })
w.sort()
w.reverse()
v == w
"#,
        true,
    );
}

#[test]
fn sort_by_in_a_function_with_a_capturing_comparator() {
    assert_aelys_str(
        r#"
fn order(v: Vec<int>, sign: int) -> string {
    let mut w = vec![]
    w.extend(v)
    w.sort_by(fn(a: int, b: int) -> int { sign * (a - b) })
    let mut s = ""
    for x in w { s += "{x}" }
    s
}
order(vec![3, 1, 2], 1) + "|" + order(vec![3, 1, 2], -1)
"#,
        "123|321",
    );
}

#[test]
fn first_last_set_fill_join() {
    assert_aelys_str(
        r#"
let mut v = vec![4, 5, 6]
let e: Vec<int> = vec![]
v.set(1, 50)
let a = "{v.first().unwrap()} {v.last().unwrap()} {v[1]} {e.first().unwrap_or(-1)} {e.last().unwrap_or(-1)}"
v.fill(7)
let mut arr = [0, 0]
arr.fill(3)
let words = vec!["x", "y", "z"]
a + " {v[0]}{v[2]} {arr[1]} " + words.join(", ")
"#,
        "4 6 50 -1 -1 77 3 x, y, z",
    );
}

#[test]
fn set_out_of_bounds_traps() {
    assert_aelys_error_contains(
        "let mut v = vec![1]\nv.set(4, 2)\n0",
        "index 4 is out of bounds for length 1",
    );
}

#[test]
fn mutating_methods_need_a_mutable_binding() {
    for call in [
        "v.insert(0, 1)",
        "v.sort()",
        "v.clear()",
        "v.reverse()",
        "v.fill(0)",
    ] {
        let err = run_aelys_err(&format!("let v = vec![1, 2]\n{call}\n0"));
        assert!(err.contains("E0318"), "{call}: {err}");
    }
}

#[test]
fn growing_methods_are_rejected_on_fixed_arrays() {
    let err = run_aelys_err("let mut a = [1, 2]\na.insert(0, 3)\n0");
    assert!(
        err.contains("E0311") && err.contains("requires a vector"),
        "{err}"
    );
}

#[test]
fn sort_rejects_elements_without_an_order() {
    let err = run_aelys_err("struct P { x: int }\nlet mut v = vec![P { x: 1 }]\nv.sort()\n0");
    assert!(err.contains("E0471") && err.contains("sort_by"), "{err}");
}

#[test]
fn fill_and_resize_with_an_aggregate_are_rejected() {
    let err =
        run_aelys_err("struct P { x: int }\nlet mut v = vec![P { x: 1 }]\nv.fill(P { x: 2 })\n0");
    assert!(err.contains("E0472"), "{err}");
    let err = run_aelys_err("let mut v = vec![vec![1]]\nv.resize(3, vec![0])\n0");
    assert!(err.contains("E0472"), "{err}");
}

#[test]
fn join_requires_strings() {
    let err = run_aelys_err("let v = vec![1, 2]\nlet s = v.join(\",\")\n0");
    assert!(err.contains("E0301"), "{err}");
}

#[test]
fn vec_methods_inside_a_generic_function() {
    assert_aelys_str(
        r#"
fn move_front<T>(v: Vec<T>) -> T {
    let mut w = vec![]
    w.extend(v)
    let x = w.remove(w.len() - 1)
    w.insert(0, x)
    w.first().unwrap()
}
"{move_front(vec![1, 2, 3])}" + move_front(vec!["a", "b"])
"#,
        "3b",
    );
}

#[test]
fn collected_elements_survive_collection_while_removing() {
    assert_aelys_int(
        r#"
let mut v: Vec<string> = vec![]
for i in 0..20000 { v.push("item{i}") }
let mut total = 0
while v.len() > 0 {
    let s = v.swap_remove(0)
    total += s.len()
}
total
"#,
        (0..20000)
            .map(|i: i32| format!("item{i}").len() as i64)
            .sum(),
    );
}
