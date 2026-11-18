mod t9_support;

use t9_support::run_with;

// expected values computed by python; every int here lies outside the 48-bit inline range unless noted
const MODES: [&[&str]; 5] = [
    &["-O0"],
    &["-O2"],
    &["-O3"],
    &["-ae.jit=on"],
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
        assert_eq!(outcome.lines(), expected, "flags {flags:?}\n{source}");
    }
}

fn assert_trap(source: &str, needle: &str) {
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_ne!(outcome.code, Some(0), "flags {flags:?}: {source}");
        assert!(
            outcome.stderr.contains(needle),
            "flags {flags:?}: {source}\n{}",
            outcome.stderr
        );
    }
}

#[test]
fn boxed_ints_in_vec_array_and_fill() {
    let source = "fn id(x: int) -> int { x }
let big = id(4611686018427387904)
let edge = id(140737488355327)
let mut v: Vec<int> = vec![3, 1, 2]
v.push(big)
v.push(-big)
v.push(edge + 1)
v.insert(0, id(-9223372036854775807) - 1)
println(\"{v}\")
v.sort()
println(\"{v}\")
println(\"{v.contains(140737488355328)} {v.index_of(4611686018427387904)}\")
let last = v.pop()
println(\"{last} {v.len()}\")
let mut a = [edge, edge, edge]
a[1] = a[1] + 1
println(\"{a} {a[1] - a[0]}\")
let mut w: Vec<int> = vec![0; 3]
w.fill(big)
println(\"{w}\")
let mut s = 0
for x in v { s = s + x % 1000 }
println(\"{s}\")
let lit = [9223372036854775807, 1, -9223372036854775807 - 1]
println(\"{lit[0]} {lit[2]}\")
";
    assert_lines(
        source,
        "[-9223372036854775808, 3, 1, 2, 4611686018427387904, -4611686018427387904, 140737488355328]
[-9223372036854775808, -4611686018427387904, 1, 2, 3, 140737488355328, 4611686018427387904]
true Some(6)
Some(4611686018427387904) 6
[140737488355327, 140737488355328, 140737488355327] 1
[4611686018427387904, 4611686018427387904, 4611686018427387904]
-1378
9223372036854775807 -9223372036854775808",
    );
}

#[test]
fn boxed_ints_as_map_keys_and_values() {
    let source = "fn id(x: int) -> int { x }
let mut m: Map<int, int> = Map::new()
let _ = m.insert(id(1 << 62), id(9223372036854775807))
let _ = m.insert(id(1 << 61) * 2, 7)
let _ = m.insert(id(140737488355327), 1)
let _ = m.insert(id(140737488355327) + 1, 2)
println(\"{m.len()} {m.get(4611686018427387904)} {m.get(140737488355328)} {m.get(140737488355327)}\")
println(\"{m.contains_key(-4611686018427387904)} {m.contains_key(4611686018427387904)}\")
let removed = m.remove(4611686018427387904)
println(\"{removed} {m.len()}\")
";
    assert_lines(
        source,
        "3 Some(7) Some(2) Some(1)
false true
Some(7) 2",
    );
}

#[test]
fn boxed_ints_in_struct_fields_enum_payloads_options_and_closures() {
    let source = "fn id(x: int) -> int { x }
struct Acc { total: int, count: int }
enum Num { Small(int), Big(int, int) }
let mut acc = Acc { total: id(140737488355320), count: 0 }
while acc.count < 10 {
    acc.total += 1
    acc.count += 1
}
println(\"{acc.total}\")
let n = Num::Big(acc.total, id(-9000000000000000000))
match n {
    Num::Big(a, b) => println(\"{a - b}\"),
    Num::Small(a) => println(\"{a}\"),
}
let some = Some(id(9007199254740993))
match some {
    Some(x) => println(\"{x + 1}\"),
    None => println(\"none\"),
}
let mut counter = id(140737488355326)
let bump = fn() -> int {
    counter = counter + 1
    counter
}
let first = bump()
let second = bump()
println(\"{first} {second} {counter}\")
let mut g = id(9223372036854775800)
g += 7
println(\"{g}\")
let copy = acc.clone()
println(\"{copy.total == acc.total} {copy.total}\")
";
    assert_lines(
        source,
        "140737488355330
9000140737488355330
9007199254740994
140737488355327 140737488355328 140737488355328
9223372036854775807
true 140737488355330",
    );
}

#[test]
fn boxed_ints_compare_match_print_and_parse_by_value() {
    let source = "fn id(x: int) -> int { x }
let a = id(9007199254740993)
let b = id(9007199254740992)
println(\"{a > b} {a == b} {a != b} {b < 140737488355328} {a == 9007199254740993} {a <= a}\")
let label = match a {
    9007199254740993 => \"exact\",
    _ => \"no\",
}
let band = match id(-140737488355329) {
    -9223372036854775808..=-140737488355329 => \"low\",
    _ => \"high\",
}
let band2 = match id(140737488355328) {
    140737488355328.. => \"boxed\",
    _ => \"inline\",
}
let band3 = match id(9223372036854775807) {
    0..9223372036854775807 => \"below\",
    _ => \"max\",
}
println(\"{label} {band} {band2} {band3}\")
let text = a.to_string()
println(\"{text}\")
let p = \"9223372036854775807\".to_int()
let q = \"-9223372036854775808\".to_int()
let r = \"9223372036854775808\".to_int()
let x = \"7fffffffffffffff\".to_int_radix(16)
println(\"{p} {q} {r} {x}\")
let neg = \"-140737488355329\"
let parsed = neg.to_int().unwrap()
println(\"{parsed - 1}\")
";
    assert_lines(
        source,
        "true false true false true true
exact low boxed max
9007199254740993
Some(9223372036854775807) Some(-9223372036854775808) None Some(9223372036854775807)
-140737488355330",
    );
}

#[test]
fn boxed_ints_cast_to_and_from_float() {
    let source = "fn id(x: int) -> int { x }
fn f(x: float) -> float { x }
println(\"{id(9007199254740993) as float} {id(9223372036854775807) as float} {id(-9223372036854775807 - 1) as float}\")
println(\"{f(9.2e18) as int} {f(-9.2e18) as int} {f(9007199254740992.0) as int}\")
";
    assert_lines(
        source,
        "9007199254740992.0 9223372036854776000.0 -9223372036854776000.0
9200000000000000000 -9200000000000000000 9007199254740992",
    );
    for value in ["9.3e18", "-9.3e18", "9223372036854775808.0"] {
        assert_trap(
            &format!(
                "fn f(x: float) -> float {{ x }}\nlet y = f({value}) as int\nprintln(\"{{y}}\")\n"
            ),
            "integer overflow",
        );
    }
}

#[test]
fn loops_counters_and_globals_cross_the_inline_boundary() {
    let source = "fn id(x: int) -> int { x }
let mut total = 0
for i in id(140737488355325)..id(140737488355330) {
    total = total + (i - 140737488355325)
}
println(\"{total}\")
let mut last = 0
for i in id(9223372036854775805)..=id(9223372036854775807) {
    last = i
}
println(\"{last}\")
let mut k = id(140737488355326)
while k < 140737488355330 { k += 1 }
println(\"{k}\")
";
    assert_lines(source, "10\n9223372036854775807\n140737488355330");
}

#[test]
fn natives_take_and_return_boxed_ints() {
    let source = "fn id(x: int) -> int { x }
let big = id(4611686018427387904)
println(\"{abs(-big)} {min(big, -big)} {max(big, 3)} {clamp(big, 0, 140737488355328)}\")
let ns = time::now_ns()
println(\"{ns > 1700000000000000000} {ns < 9223372036854775807}\")
";
    assert_lines(
        source,
        "4611686018427387904 -4611686018427387904 4611686018427387904 140737488355328
true true",
    );
    assert_trap(
        "fn id(x: int) -> int { x }\nlet y = abs(id(-9223372036854775807) - 1)\nprintln(\"{y}\")\n",
        "integer overflow",
    );
}
