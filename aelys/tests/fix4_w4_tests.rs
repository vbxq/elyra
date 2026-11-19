mod t9_support;

use t9_support::run_with;

// every expected value below was computed by python, independently of the VM
const MODES: [&[&str]; 6] = [
    &["-O0"],
    &["-O2"],
    &["-O3"],
    &["-ae.jit=on"],
    &["-ae.jit-sync=on"],
    &["-ae.gc-stress=on", "-ae.gc-verify=on"],
];

fn assert_lines(source: &str, expected: &str) {
    let expected: Vec<&str> = expected.lines().collect();
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(0),
            "flags {flags:?}\nstderr: {}",
            outcome.stderr
        );
        assert_eq!(
            outcome.lines(),
            expected,
            "flags {flags:?}\nstdout: {}",
            outcome.stdout
        );
    }
}

fn assert_runtime_error(source: &str, message: &str) {
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(1),
            "flags {flags:?}: {source}\nstderr: {}",
            outcome.stderr
        );
        assert!(
            outcome.stderr.contains(message),
            "flags {flags:?}: {source}\nwanted: {message}\nstderr: {}",
            outcome.stderr
        );
    }
}

fn assert_refused(source: &str, code: &str, message: &str) {
    let outcome = run_with(source, &[]);
    assert_ne!(outcome.code, Some(0), "{source}");
    assert!(
        outcome.stderr.contains(code) && outcome.stderr.contains(message),
        "{source}\nwanted: {code} {message}\nstderr: {}",
        outcome.stderr
    );
}

const FOR_AT_THE_ENDS: &str = r#"fn show(c: int, s: string) { println(c.to_string() + s) }
let lo: u64 = 9223372036854775808
let hi: u64 = 9223372036854775810
let mut c = 0
let mut s = ""
for i in lo..hi { c += 1; s = s + "," + i.to_string() }
show(c, s)
let a: u64 = 18446744073709551610
let b: u64 = 18446744073709551615
c = 0
s = ""
for i in a..=b step 2 { c += 1; s = s + "," + i.to_string() }
show(c, s)
let a2: u64 = 18446744073709551613
c = 0
s = ""
for i in a2..=b { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
s = ""
for i in a2..b { c += 1; s = s + "," + i.to_string() }
show(c, s)
let m = -9223372036854775807 - 1
c = 0
s = ""
for i in m..m + 3 { c += 1; s = s + "," + i.to_string() }
show(c, s)
let mt: i64 = -9223372036854775807 - 1
c = 0
s = ""
for i in mt..mt + 6 step 2 { c += 1; s = s + "," + i.to_string() }
show(c, s)
let x = 9223372036854775807
c = 0
s = ""
for i in x - 2..=x { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
s = ""
for i in x - 6..=x step 3 { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
s = ""
for i in m + 4..=m step -2 { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
s = ""
for i in m + 2..m step -1 { c += 1; s = s + "," + i.to_string() }
show(c, s)
let p: i8 = 125
let q: i8 = 127
c = 0
s = ""
for i in p..=q { c += 1; s = s + "," + i.to_string() }
show(c, s)
let r: i8 = -126
let t: i8 = -128
c = 0
s = ""
for i in r..=t step -1 { c += 1; s = s + "," + i.to_string() }
show(c, s)
let z: u8 = 0
let w: u8 = 255
c = 0
for i in z..=w { c += 1 }
show(c, "")
let u: u32 = 4294967285
let v: u32 = 4294967295
c = 0
s = ""
for i in u..=v step 5 { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
for i in x..x { c += 1 }
for i in x..=x - 1 { c += 1 }
for i in b..b { c += 1 }
for i in m..m { c += 1 }
for i in m..=m { c += 1 }
for i in b..=b { c += 1 }
show(c, "")
let back = -3
c = 0
s = ""
for i in m + 6..m step back { c += 1; s = s + "," + i.to_string() }
show(c, s)
c = 0
s = ""
for i in a..=b {
    if i == 18446744073709551611 { continue }
    if i == 18446744073709551614 { break }
    c += 1
    s = s + "," + i.to_string()
}
show(c, s)
"#;

const FOR_AT_THE_ENDS_EXPECTED: &str = "2,9223372036854775808,9223372036854775809
3,18446744073709551610,18446744073709551612,18446744073709551614
3,18446744073709551613,18446744073709551614,18446744073709551615
2,18446744073709551613,18446744073709551614
3,-9223372036854775808,-9223372036854775807,-9223372036854775806
3,-9223372036854775808,-9223372036854775806,-9223372036854775804
3,9223372036854775805,9223372036854775806,9223372036854775807
3,9223372036854775801,9223372036854775804,9223372036854775807
3,-9223372036854775804,-9223372036854775806,-9223372036854775808
2,-9223372036854775806,-9223372036854775807
3,125,126,127
3,-126,-127,-128
256
3,4294967285,4294967290,4294967295
2
2,-9223372036854775802,-9223372036854775805
3,18446744073709551610,18446744073709551612,18446744073709551613";

#[test]
fn for_loops_reach_both_ends_of_every_integer_type() {
    assert_lines(FOR_AT_THE_ENDS, FOR_AT_THE_ENDS_EXPECTED);
}

#[test]
fn for_loops_at_the_ends_stay_correct_in_hot_compiled_functions() {
    let source = r#"fn up(lo: u64, hi: u64) -> int {
    let mut c = 0
    for i in lo..=hi { c += 1 }
    c
}
fn low(lo: i64, n: i64) -> int {
    let mut c = 0
    for i in lo..lo + n step 2 { c += 1 }
    c
}
fn top(hi: int) -> int {
    let mut c = 0
    for i in hi - 4..=hi { c += 1 }
    for i in hi..hi { c += 1 }
    c
}
fn down(lo: int) -> int {
    let mut c = 0
    for i in lo + 5..=lo step -1 { c += 1 }
    c
}
let mut total = 0
for k in 0..400 {
    total += up(18446744073709551612, 18446744073709551615)
    total += low(-9223372036854775807 - 1, 6)
    total += top(9223372036854775807)
    total += down(-9223372036854775807 - 1)
    total += up(5, 7)
}
println(total)
"#;
    // per round: 4 + 3 + 5 + 6 + 3 = 21
    assert_lines(source, "8400");
}

#[test]
fn host_buffers_sized_by_a_program_int_report_out_of_memory() {
    for source in [
        "let mut v = vec![1]\nv.resize(100000000000, 0)\nprintln(v.len())\n",
        "let mut v = vec![1.5]\nv.resize(100000000000, 2.5)\nprintln(v.len())\n",
        "let mut v = vec![true]\nv.resize(100000000000, false)\nprintln(v.len())\n",
        "println(\" \".repeat(100000000000).len())\n",
        "println(\"ab\".repeat(9223372036854775807).len())\n",
        "println(\"ab\".pad_left(100000000000, \"x\").len())\n",
        "println(\"ab\".pad_right(100000000000, \"x\").len())\n",
        "println(\"ab\".pad_left(100000000000, \"\u{e9}\").len())\n",
    ] {
        for flags in [&["-O0"][..], &["-O3", "-ae.jit-sync=on"][..]] {
            let outcome = run_with(source, flags);
            assert_eq!(
                outcome.code,
                Some(1),
                "flags {flags:?}: {source}\nstderr: {}",
                outcome.stderr
            );
            assert!(
                outcome.stderr.contains("out of memory: requested"),
                "flags {flags:?}: {source}\nstderr: {}",
                outcome.stderr
            );
        }
    }
}

#[test]
fn bounded_resize_repeat_and_pad_keep_their_results() {
    let source = r#"let mut v = vec![1]
v.resize(600, 3)
println(v.len())
println(v[599] + v[0])
let mut w = vec![1, 2, 3]
w.resize(1, 0)
println(w)
println("xy".repeat(3))
println("xy".repeat(0).len())
println("ab".pad_left(5, "0"))
println("ab".pad_right(5, "é"))
println("abcdef".pad_left(2, "0"))
println("ab".pad_left(-3, "0"))
"#;
    assert_lines(source, "600\n4\n[1]\nxyxyxy\n0\n000ab\nabééé\nabcdef\nab");
}

#[test]
fn u64_values_above_the_int_maximum_are_match_patterns() {
    let source = r#"fn k(x: u64) -> int {
    match x {
        18446744073709551615 => 1,
        9223372036854775808..=18446744073709551614 => 2,
        _ => 3,
    }
}
fn full(x: u64) -> int { match x { 0..=18446744073709551615 => 7 } }
fn halves(x: u64) -> int { match x { 0..9223372036854775808 => 0, 9223372036854775808.. => 1 } }
const H: u64 = 9223372036854775808
fn named(x: u64) -> int { match x { H => 1, _ => 2 } }
fn whole(x: int) -> int { match x { -9223372036854775808..=9223372036854775807 => 4 } }
let a: u64 = 18446744073709551615
let b: u64 = 9223372036854775808
let c: u64 = 9223372036854775807
println(k(a))
println(k(b))
println(k(c))
println(full(c))
println(halves(c))
println(halves(b))
println(named(b))
println(named(a))
println(whole(-1))
"#;
    assert_lines(source, "1\n2\n3\n7\n0\n1\n1\n2\n4");
}

#[test]
fn a_u64_pattern_outside_the_scrutinee_type_is_e0301() {
    assert_refused(
        "fn m(x: int) -> int { match x { 18446744073709551615 => 1, _ => 2 } }\nprintln(m(5))\n",
        "E0301",
        "integer literal 18446744073709551615 does not fit in int",
    );
    assert_refused(
        "fn m(x: u64) -> int { match x { 0..9223372036854775808 => 1, 9223372036854775808..18446744073709551615 => 2 } }\nprintln(m(5))\n",
        "E0302",
        "missing 18446744073709551615",
    );
}

#[test]
fn sized_collection_literals_type_plain_literals_by_the_sized_element() {
    let source = r#"let a: i64 = 5
let b: i32 = -7
let c: u64 = 18446744073709551615
let d: u8 = 200
let v: Vec<i64> = vec![a, 0]
let w = vec![0, b, 3]
let x = [c, 1]
let y = [d, 0, 55]
let z: Vec<u64> = vec![1, 9223372036854775808]
let arr: [i32; 2] = [a as i32, 0]
println(v)
println(w)
println(x)
println(y)
println(z)
println(arr)
println(w[1] - 2147483647 - 1)
"#;
    assert_lines(
        source,
        "[5, 0]\n[0, -7, 3]\n[18446744073709551615, 1]\n[200, 0, 55]\n[1, 9223372036854775808]\n[5, 0]\n2147483641",
    );
    assert_refused(
        "let d: u8 = 200\nlet y = vec![d, 300]\nprintln(y)\n",
        "E0301",
        "integer literal 300 does not fit in u8",
    );
}

#[test]
fn integer_methods_apply_to_the_result_of_a_generic_call() {
    let source = r#"fn gid<T>(x: T) -> T { x }
let b: u8 = 200
let y: i32 = 9
let u: u64 = 18446744073709551615
let r = gid(255)
println(r.to_hex())
println(gid(255).to_binary())
println(gid(255).to_octal())
println(gid(255).to_radix(3))
println(gid(255).mul_shr(3, 1))
println(gid(255).shl_div(3, 1))
println(gid(b).to_hex())
println(gid(y).mul_shr(3, 1))
println(gid(u).to_hex())
println(gid(255).to_hex().len())
"#;
    assert_lines(
        source,
        "ff\n11111111\n377\n100110\n382\n170\nc8\n13\nffffffffffffffff\n2",
    );
    assert_refused(
        "fn gid<T>(x: T) -> T { x }\nprintln(gid(\"x\").to_hex())\n",
        "E0371",
        "no field or method 'to_hex' on a value of type string",
    );
}

#[test]
fn call_syntax_names_the_method_when_a_field_shares_its_name() {
    let source = r#"struct G { ready: bool }
impl G { fn ready(self) -> bool { !self.ready } }
struct B<T> { v: T, f: fn(int) -> int }
impl<T> B<T> { fn v(self) -> T { self.v } }
trait Lo { fn lo(self) -> int; }
struct S { lo: int, hi: int }
impl Lo for S { fn lo(self) -> int { self.lo * 10 } }
fn dbl(x: int) -> int { x * 2 }
fn gid<T>(x: T) -> T { x }
let g = G { ready: true }
println(g.ready())
println(g.ready)
let b = B { v: 7, f: dbl }
println(b.v() + 1)
println(b.v)
println(b.f(4))
let s = S { lo: 3, hi: 4 }
println(s.lo())
println(s.lo)
println(gid(s).lo())
println(gid(S { lo: 5, hi: 0 }).lo)
println(gid(b).v())
"#;
    assert_lines(source, "false\ntrue\n8\n7\n8\n30\n3\n30\n5\n7");
}

#[test]
fn slice_bounds_take_any_integer_type() {
    let source = r#"let v = vec![1, 2, 3, 4]
let a = [5, 6, 7]
let i: u64 = 1
let j: u64 = 3
let k: u8 = 2
let m: i32 = 0
println(v[i..j])
println(v[i..=j])
println(a[m..k])
println(v[..j])
println(v[k..])
println(v[i..3])
"#;
    assert_lines(
        source,
        "[2, 3]\n[2, 3, 4]\n[5, 6]\n[1, 2, 3]\n[3, 4]\n[2, 3]",
    );
    assert_runtime_error(
        "let v = vec![1, 2, 3, 4]\nlet big: u64 = 18446744073709551615\nprintln(v[1..big])\n",
        "index 18446744073709551615 is out of bounds for length 4",
    );
}

#[test]
fn index_errors_report_the_real_index_and_length() {
    let cases = [
        (
            "let v = vec![1, 2, 3]\nlet i: u64 = 18446744073709551615\nprintln(v[i])\n",
            "index 18446744073709551615 is out of bounds for length 3",
        ),
        (
            "let v = [1, 2, 3]\nlet i = -2\nprintln(v[i])\n",
            "index -2 is out of bounds for length 3",
        ),
        (
            "let mut v = vec![1.5, 2.5]\nlet i: u64 = 9223372036854775808\nv[i] = 4.5\n",
            "index 9223372036854775808 is out of bounds for length 2",
        ),
        (
            "let mut v = [true, false, true, true]\nlet i = -7\nv[i] = false\n",
            "index -7 is out of bounds for length 4",
        ),
        (
            "let v = vec![\"a\", \"b\"]\nlet i: i64 = -9223372036854775807 - 1\nprintln(v[i])\n",
            "index -9223372036854775808 is out of bounds for length 2",
        ),
        (
            "let s = \"h\u{e9}llo\"\nlet i: u64 = 18446744073709551615\nprintln(s[i])\n",
            "index 18446744073709551615 is out of bounds for length 5",
        ),
        (
            "fn get(v: Vec<int>, i: int) -> int { v[i] }\nlet v = vec![4, 5, 6]\nlet mut s = 0\nfor k in 0..3000 { s += get(v, k % 3) }\nprintln(get(v, -1))\n",
            "index -1 is out of bounds for length 3",
        ),
    ];
    for (source, message) in cases {
        assert_runtime_error(source, message);
    }
}

#[test]
fn a_u64_cast_to_char_reports_the_invalid_scalar() {
    assert_runtime_error(
        "let x: u64 = 18446744073709551615\nprintln(x as char)\n",
        "invalid char: 18446744073709551615 is not a Unicode scalar value",
    );
    assert_runtime_error(
        "let x = -1\nprintln(x as char)\n",
        "invalid char: -1 is not a Unicode scalar value",
    );
}

#[test]
fn a_missing_integer_map_key_is_displayed_unquoted() {
    assert_runtime_error(
        "let m: Map<int, int> = Map::new()\nprintln(m[140737488355328])\n",
        "map key not found: 140737488355328\n",
    );
    assert_runtime_error(
        "let m: Map<u64, int> = Map::new()\nlet k: u64 = 18446744073709551615\nprintln(m[k])\n",
        "map key not found: 18446744073709551615\n",
    );
    assert_runtime_error(
        "let m: Map<string, int> = Map::new()\nprintln(m[\"140737488355328\"])\n",
        "map key not found: \"140737488355328\"",
    );
}

#[test]
fn a_non_bool_operand_of_not_names_the_operator() {
    let outcome = run_with("println(!5)\n", &[]);
    assert!(
        outcome.stderr.contains("E0301")
            && outcome.stderr.contains("operand of !")
            && !outcome.stderr.contains("if condition"),
        "{}",
        outcome.stderr
    );
}
