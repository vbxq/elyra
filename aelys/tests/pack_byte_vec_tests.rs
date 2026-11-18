mod s1_support;

use s1_support::{compile_error, run_ok};

const MODES: [&[&str]; 5] = [
    &["-O0"],
    &["-O2"],
    &["-ae.jit=on"],
    &["-O2", "-ae.jit=on"],
    &["-ae.gc-stress=on", "-ae.gc-verify=on"],
];

fn assert_everywhere(source: &str, expected: &[&str]) {
    for flags in MODES {
        assert_eq!(run_ok(source, flags), expected, "flags {flags:?}\n{source}");
    }
}

// expected lines computed by python on bytearray/list equivalents of each step
#[test]
fn every_vec_u8_operation_on_packed_storage_gives_the_expected_bytes() {
    let source = r#"let mut v: Vec<u8> = vec![0; 8]
v[0] = 255
v[1] = 1
let one: u8 = 1
v[2] = v[0] + one
v.push(7)
println(v)
println(v.len())
println(v.pop())
v.insert(0, 9)
println(v)
println(v.remove(0))
println(v.swap_remove(1))
println(v)
let mut w = v.clone()
w[0] = 5
println("{v[0]} {w[0]} {v == w} {v == v.clone()}")
println(v[0..3])
v.sort()
println(v)
v.fill(3)
println(v)
v.extend(vec![4, 5])
println(v)
v.reverse()
println(v)
println("{v.contains(4)} {v.contains(200)} {v.index_of(3)} {v.first()} {v.last()}")
v.copy_within(0, 2, 4)
println(v)
v.swap(0, v.len() - 1)
println(v)
v.resize(12, 254)
println(v)
v.truncate(3)
println(v)
let mut total = 0
for b in v { total = total + (b as int) }
println(total)
let mut more: Vec<u8> = vec![1, 2]
v.append(more)
println("{v} {more.len()}")
v.set(1, 200)
println(v)
println("{v.to_utf8()} {"hé".to_bytes()} {"hé".to_bytes().to_utf8()}")
v.clear()
println("{v.is_empty()} {v}")
"#;
    assert_everywhere(
        source,
        &[
            "[255, 1, 0, 0, 0, 0, 0, 0, 7]",
            "9",
            "Some(7)",
            "[9, 255, 1, 0, 0, 0, 0, 0, 0]",
            "9",
            "1",
            "[255, 0, 0, 0, 0, 0, 0]",
            "255 5 false true",
            "[255, 0, 0]",
            "[0, 0, 0, 0, 0, 0, 255]",
            "[3, 3, 3, 3, 3, 3, 3]",
            "[3, 3, 3, 3, 3, 3, 3, 4, 5]",
            "[5, 4, 3, 3, 3, 3, 3, 3, 3]",
            "true false Some(2) Some(5) Some(3)",
            "[5, 4, 3, 3, 5, 4, 3, 3, 3]",
            "[3, 4, 3, 3, 5, 4, 3, 3, 5]",
            "[3, 4, 3, 3, 5, 4, 3, 3, 5, 254, 254, 254]",
            "[3, 4, 3]",
            "10",
            "[3, 4, 3, 1, 2] 0",
            "[3, 200, 3, 1, 2]",
            "None [104, 195, 169] Some(\"hé\")",
            "true []",
        ],
    );
}

#[test]
fn a_byte_holds_0_to_255_and_u8_arithmetic_wraps_before_the_store() {
    let source = r#"let mut v: Vec<u8> = vec![0; 6]
let a: u8 = 200
let one: u8 = 1
let zero: u8 = 0
v[0] = a + a
v[1] = zero - one
v[2] = 300 as u8
v[3] = (0 - 1) as u8
v[4] = 255
v[5] = 0
println(v)
println("{v[1] as int + 1} {v.read_u8(1)} {v.read_i8(1)}")
let mut big: int = 0
for i in 0..6 { big = big * 256 + (v[i] as int) }
println(big)
"#;
    assert_everywhere(
        source,
        &[
            "[144, 255, 44, 255, 255, 0]",
            "256 255 -1",
            "159425646034688",
        ],
    );
    let wide = compile_error("let mut v: Vec<u8> = vec![0; 2]\nv[0] = 256\n");
    assert!(
        wide.contains("E0301") && wide.contains("256 does not fit in u8"),
        "{wide}"
    );
    let pushed = compile_error("let mut v: Vec<u8> = vec![0; 2]\nv.push(256)\n");
    assert!(pushed.contains("E0301"), "{pushed}");
    let negative = compile_error("let v: Vec<u8> = vec![1, -1]\n");
    assert!(negative.contains("E0301"), "{negative}");
    let int_value = compile_error("let mut v: Vec<u8> = vec![0; 2]\nlet x: int = 5\nv[0] = x\n");
    assert!(int_value.contains("error[E0"), "{int_value}");
}

// a packed element costs one byte of heap, a word-sized slot costs eight
#[test]
fn every_way_of_making_a_vec_u8_stores_one_byte_per_element() {
    let source = r#"use std::sys
use std::fs
fn cost(before: int, n: int) -> string {
    sys::gc_collect()
    let grown = sys::gc_heap_bytes() - before
    if grown < 4 * n + 64 { "packed" } else { "wide {grown} for {n}" }
}
fn base() -> int {
    sys::gc_collect()
    sys::gc_heap_bytes()
}
let n = 400000
let path = "{sys::env("PACK_DIR").unwrap()}/bytes.bin"
let mut at = base()
let repeat: Vec<u8> = vec![7; n]
println("repeat {cost(at, n)}")
at = base()
let mut pushed: Vec<u8> = vec![]
for i in 0..100000 { pushed.push((i % 256) as u8) }
println("push {cost(at, 100000)}")
at = base()
let literal: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64]
println("literal {cost(at, 64)}")
let text = "ab".repeat(n / 2)
at = base()
let bytes = text.to_bytes()
println("to_bytes {cost(at, n)}")
fs::write_bytes(path, &repeat).unwrap()
at = base()
let read = fs::read_bytes(path).unwrap()
println("read_bytes {cost(at, n)}")
at = base()
let copy = repeat.clone()
println("clone {cost(at, n)}")
at = base()
let slice = repeat[0..n]
println("slice {cost(at, n)}")
at = base()
let mapped = repeat.iter().map(fn(b: u8) -> u8 { return b + 1 }).collect()
println("map {cost(at, n)}")
at = base()
let mut extended: Vec<u8> = vec![]
extended.extend(repeat)
println("extend {cost(at, n)}")
let mut table: Map<int, u8> = Map::new()
for i in 0..20000 { let _ = table.insert(i, (i % 256) as u8) }
at = base()
let values = table.values()
println("map values {cost(at, 20000)}")
println("{repeat[n - 1]} {pushed[99999]} {literal[63]} {bytes[1]} {read[5]} {copy[3]} {slice[2]} {mapped[0]} {extended[n - 1]} {values[19999]}")
"#;
    let directory = tempfile::tempdir().expect("scratch directory");
    unsafe { std::env::set_var("PACK_DIR", directory.path()) };
    assert_everywhere(
        source,
        &[
            "repeat packed",
            "push packed",
            "literal packed",
            "to_bytes packed",
            "read_bytes packed",
            "clone packed",
            "slice packed",
            "map packed",
            "extend packed",
            "map values packed",
            "7 159 64 98 7 7 7 8 7 31",
        ],
    );
}

#[test]
fn byte_vectors_and_int_vectors_live_side_by_side() {
    let source = r#"let bytes: Vec<u8> = vec![1, 128, 255]
let mut ints: Vec<int> = vec![300, 1099511627776, 9223372036854775807]
ints.push(bytes[2] as int * 1000)
let mut back: Vec<u8> = vec![0; 4]
for i in 0..4 { back[i] = ints[i] as u8 }
let widened = bytes.iter().map(fn(b: u8) -> int { return b as int * 1000000 }).collect()
println(ints)
println(back)
println(widened)
let mut sum: int = 0
for i in 0..3 { sum = sum + ints[i] / 1099511627776 + bytes[i] as int }
println(sum)
let mut grid: Vec<Vec<u8>> = vec![vec![0; 2]; 3]
grid[1][0] = 9
let rows: Vec<int> = vec![grid[0][0] as int, grid[1][0] as int, grid[2][0] as int]
println("{grid} {rows}")
"#;
    assert_everywhere(
        source,
        &[
            "[300, 1099511627776, 9223372036854775807, 255000]",
            "[44, 0, 255, 24]",
            "[1000000, 128000000, 255000000]",
            "8388992",
            "[[0, 0], [9, 0], [0, 0]] [0, 9, 0]",
        ],
    );
}

#[test]
fn a_clone_of_packed_bytes_is_independent_at_every_depth() {
    let source = r#"struct Frame { pixels: Vec<u8>, rows: Vec<Vec<u8>> }
let mut a = Frame { pixels: vec![1, 2, 3], rows: vec![vec![4, 5], vec![6]] }
let mut b = a.clone()
b.pixels[0] = 100
b.rows[0][1] = 200
b.rows[1].push(7)
a.pixels.push(9)
println("{a.pixels} {a.rows}")
println("{b.pixels} {b.rows}")
let c = a.pixels
let mut d = c.clone()
d.fill(0)
println("{c} {d} {c == a.pixels}")
"#;
    assert_everywhere(
        source,
        &[
            "[1, 2, 3, 9] [[4, 5], [6]]",
            "[100, 2, 3] [[4, 200], [6, 7]]",
            "[1, 2, 3, 9] [0, 0, 0, 0] true",
        ],
    );
}

#[test]
fn packed_bytes_round_trip_through_a_file() {
    let directory = tempfile::tempdir().expect("scratch directory");
    let path = directory.path().join("all.bin");
    let source = format!(
        r#"use std::fs
let mut v: Vec<u8> = vec![0; 256]
for i in 0..256 {{ v[i] = (255 - i) as u8 }}
fs::write_bytes("{path}", &v).unwrap()
let r = fs::read_bytes("{path}").unwrap()
let mut same = r.len() == 256
for i in 0..256 {{ same = same && r[i] == v[i] }}
println("{{same}} {{r[0]}} {{r[255]}} {{r == v}}")
"#,
        path = path.display()
    );
    assert_everywhere(&source, &["true 255 0 true"]);
    let written = std::fs::read(&path).expect("written file");
    assert_eq!(written, (0..=255u8).rev().collect::<Vec<_>>());
}

// checksums computed by python over the same byte arithmetic
#[test]
fn hot_loops_read_and_write_packed_bytes() {
    let source = r#"fn fill(mut v: Vec<u8>, seed: int) {
    let mut x = seed
    for i in 0..v.len() {
        x = (x * 1103515245 + 12345) % 2147483648
        v[i] = (x / 65536) as u8
    }
}
fn checksum(v: Vec<u8>) -> int {
    let mut h = 0
    for i in 0..v.len() { h = (h * 31 + v[i] as int) % 1000000007 }
    h
}
fn blend(mut dst: Vec<u8>, src: Vec<u8>) {
    let mut i = 0
    while i < dst.len() {
        dst[i] = src[i] / 2 + dst[i] / 2
        i += 1
    }
}
let mut a: Vec<u8> = vec![0; 50000]
let mut b: Vec<u8> = vec![0; 50000]
fill(a, 1)
fill(b, 2)
println(checksum(a))
blend(a, b)
println(checksum(a))
let mut table: Vec<u8> = vec![0; 256]
for i in 0..256 { table[i] = (i * 7 % 256) as u8 }
let mut mapped = 0
for i in 0..a.len() { mapped = mapped + table[a[i] as int] as int }
println(mapped)
"#;
    assert_everywhere(source, &["526450", "779315111", "6410209"]);
}

#[test]
fn sixty_four_bit_reads_and_writes_cover_the_whole_int_range() {
    let source = r#"let mut v: Vec<u8> = vec![0; 16]
let low = -9223372036854775807 - 1
v.write_i64_le(0, 9223372036854775807)
v.write_i64_be(8, low)
println("{v.read_i64_le(0)} {v.read_i64_be(8)} {v.read_u64_le(0)}")
println(v)
v.write_u64_be(0, 9223372036854775807)
println("{v.read_u64_be(0)} {v.read_i64_be(0) == 9223372036854775807}")
v.write_i64_le(0, -1)
println(v.read_i64_le(0))
"#;
    assert_everywhere(
        source,
        &[
            "9223372036854775807 -9223372036854775808 9223372036854775807",
            "[255, 255, 255, 255, 255, 255, 255, 127, 128, 0, 0, 0, 0, 0, 0, 0]",
            "9223372036854775807 true",
            "-1",
        ],
    );
    let trap = s1_support::runtime_error(
        "let mut v: Vec<u8> = vec![0; 8]\nv.write_i64_le(0, -1)\nprintln(v.read_u64_le(0))\n",
    );
    assert!(
        trap.contains("read_u64_le: value 18446744073709551615 does not fit in int"),
        "{trap}"
    );
}

#[test]
fn byte_vectors_in_generic_functions_closures_and_methods() {
    let source = r#"fn first_of<T>(v: Vec<T>) -> T { v[0] }
fn put<T>(mut v: Vec<T>, at: int, x: T) { v[at] = x }
struct Screen { pixels: Vec<u8> }
impl Screen {
    fn plot(mut self, at: int, colour: u8) { self.pixels[at] = colour }
    fn sum(self) -> int {
        let mut s = 0
        for p in self.pixels { s += p as int }
        s
    }
}
let mut v: Vec<u8> = vec![5; 4]
let c: u8 = 250
put(v, 1, c)
println("{first_of(v)} {v}")
let mut s = Screen { pixels: vec![0; 4] }
s.plot(2, 255)
s.pixels[0] += 3
println("{s.pixels} {s.sum()}")
let bump = fn(i: int) { v[i] = v[i] + 1 }
bump(1)
bump(3)
println(v)
"#;
    assert_everywhere(
        source,
        &["5 [5, 250, 5, 5]", "[3, 0, 255, 0] 258", "[5, 251, 5, 6]"],
    );
}
