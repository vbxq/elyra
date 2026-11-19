mod common;
use common::*;

#[test]
fn split_returns_a_vec_of_strings() {
    assert_aelys_str(
        r#"
let parts = "a,b,,c".split(",")
let empty = "".split(",")
let chars = "héy".split("")
let multi = "1::2::3".split("::")
"{parts.len()}:" + parts.join("|") + " {empty.len()}:" + empty[0] + " {chars.len()}:" + chars[1] + " " + multi.join("+")
"#,
        "4:a|b||c 1: 3:é 1+2+3",
    );
}

#[test]
fn lines_and_bytes() {
    assert_aelys_str(
        r#"
let ls = "one\ntwo\r\nthree\n".lines()
let bs = "Aé".to_bytes()
"{ls.len()} " + ls.join(",") + " {bs.len()} {bs[0]} {bs[1]} {bs[2]}"
"#,
        "3 one,two,three 3 65 195 169",
    );
}

#[test]
fn split_results_are_independent_strings() {
    assert_aelys_str(
        r#"
let mut parts = "x y z".split(" ")
parts[1] += "!"
parts.sort()
parts.join("")
"#,
        "xy!z",
    );
}

#[test]
fn string_ordering_is_lexicographic_by_scalar() {
    assert_aelys_str(
        r#""{"abc" < "abd"} {"ab" < "abc"} {"b" > "abc"} {"Z" < "a"} {"é" > "z"} {"" < "a"} {"abc" <= "abc"} {"abc" >= "abd"}""#,
        "true true true true true true true false",
    );
}

#[test]
fn appending_in_place_keeps_earlier_values_intact() {
    assert_aelys_str(
        r#"
let base = "ab"
let left = base + "c"
let right = base + "d"
let mut s = ""
for i in 0..5 { s += "x" }
let snapshot = s
s += "y"
let mut t = snapshot
t += "z"
base + " " + left + " " + right + " " + snapshot + " " + s + " " + t + " " + (s + s)
"#,
        "ab abc abd xxxxx xxxxxy xxxxxz xxxxxyxxxxxy",
    );
}

#[test]
fn a_long_append_loop_builds_the_right_string() {
    assert_aelys_int(
        r#"
let mut acc = ""
for i in 0..200000 { acc += "x" }
let mut digits = ""
for i in 0..1000 { digits += "{i % 10}" }
acc.len() + digits.len() * 1000 + (digits[999] as int - '0' as int)
"#,
        200000 + 1000 * 1000 + 9,
    );
}

#[test]
fn appended_strings_survive_collection_and_compare_equal() {
    assert_aelys_bool(
        r#"
let mut kept: Vec<string> = vec![]
let mut s = ""
for i in 0..3000 {
    s += "ab"
    if i % 1000 == 0 { kept.push(s) }
}
kept[0] == "ab" && kept[1].len() == 2002 && kept[2].len() == 4002 && s.len() == 6000
"#,
        true,
    );
}

#[test]
fn appended_strings_work_as_map_keys() {
    assert_aelys_int(
        r#"
let mut m: Map<string, int> = Map::new()
let mut k = "k"
k += "ey"
m[k] = 3
m["key"]
"#,
        3,
    );
}

#[test]
fn string_join_on_a_string_receiver_is_unchanged() {
    assert_aelys_str("\"a\\nb\".join(\",\")", "a,b");
}
