use std::fs;
use std::path::{Path, PathBuf};

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use aelys_runtime::Value;
use tempfile::TempDir;

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
                        source_name: "<t3a-merge>".to_string(),
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

fn assert_rejected(source: &str, code: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains(code), "expected {code}:\n{message}");
            message
        }
    }
}

fn write(root: &Path, path: &str, content: &str) -> PathBuf {
    let file = root.join(path);
    fs::write(&file, content).expect("write fixture");
    file
}

fn run_module_int(files: &[(&str, &str)], main: &str) -> i64 {
    let dir = TempDir::new().expect("temp dir");
    for (path, content) in files {
        write(dir.path(), path, content);
    }
    let path = write(dir.path(), "main.aelys", main);
    let value = run_file(&path).unwrap_or_else(|error| panic!("main must run:\n{error}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("main must end on an int, got {value:?}"))
}

fn run_avbc(bytes: &[u8]) -> i64 {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime.load_avbc(bytes, "<avbc>").expect("load avbc");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate.execute(&module, RunOptions::default()) {
        Ok(ExecutionOutcome::Returned(value)) => value.as_int().expect("an int result"),
        other => panic!("avbc run: {other:?}"),
    }
}

const SIZED_CONSTANTS: &str = r#"
const A: u8 = 200
const B = A + A
const C: u8 = (200 + 100) >> 1
const D: i32 = 0x7FFFFFFF + 1
const E: u16 = (A as u16) * 400
const F: u32 = (A + A) as u32
const G: bool = (A + A) == 144
const H: i8 = -128 / -1
const I: u8 = 1 << 9
const J: u32 = 1 << 31
const L: u8 = 300.7 as u8
const MASK: u32 = 0xFFFF
fn local() -> u8 {
    const K: u8 = 250 + 10
    K
}
let b: u8 = B
let g = if G { 1 } else { 0 }
let w: u32 = 0x12345
b as int + C as int * 1000 + D as int + E as int + F as int * 10 + g * 7 + H as int + I as int * 100 + J as int + L as int + (w & MASK) as int + local() as int
"#;

const SIZED_CONSTANTS_VALUE: i64 = 144 + 22 * 1000 - 2147483648 + 14464 + 144 * 10 + 7 - 128
    + 2 * 100
    + 2147483648
    + 44
    + 0x2345
    + 4;

#[test]
fn sized_constants_fold_with_the_wrap_of_their_type() {
    assert_int_everywhere(SIZED_CONSTANTS, SIZED_CONSTANTS_VALUE);
}

#[test]
fn a_sized_constant_has_the_value_of_the_same_let() {
    let lets = SIZED_CONSTANTS
        .replace("const ", "let ")
        .replace("let MASK", "const MASK");
    assert_int_everywhere(&lets, SIZED_CONSTANTS_VALUE);
}

#[test]
fn int_constant_shifts_follow_the_runtime_mask_and_overflow_check() {
    assert_int_everywhere(
        "const Z = 1 >> -1\nconst O = 1 << 64\nconst T = 1 << 46\nlet n = -1\nlet z = 1 >> n\nZ * 1000 + O * 100 + z * 10 + T / (1 << 45)",
        102,
    );
    let message = assert_rejected("const B = 1 << 63\nB", "E0395");
    assert!(
        message.contains("1 << 63 overflows the int range"),
        "{message}"
    );
    assert_rejected("const B: u8 = 1 / 0\nB", "E0395");
}

#[test]
fn char_casts_to_sized_types_keep_the_low_bits_of_the_scalar() {
    assert_int_everywhere(
        "let e = '€'\nlet a = 'é'\nlet b: u8 = 65\nlet c = b as char\nlet x: u8 = e as u8\nx as int * 1000000 + (a as u8) as int * 1000 + (e as u16) as int / 1000 + c as int - 65",
        172 * 1000000 + 233 * 1000 + 8,
    );
    assert_int_everywhere(
        "let mut buf: Vec<u8> = vec![]\nbuf.push('A' as u8)\nbuf.push('é' as u8)\n(buf[0] + buf[1]) as int",
        (65 + 233) % 256,
    );
}

#[test]
fn collection_methods_type_their_literals_from_the_element_and_key() {
    assert_int_everywhere(
        r#"
let mut v: Vec<u8> = vec![0; 3]
v.extend(vec![255, 254])
v.push(1)
v.insert(0, 200)
let mut s: Vec<i8> = vec![]
s.push(-1)
s.push(-128)
let has = if s.contains(-1) { 1 } else { 0 }
s.fill(-2)
s.extend([127, 1])
let mut m: Map<i8, i16> = Map::new()
let _ = m.insert(-1, -32768)
let mut n: Map<u8, i8> = Map::new()
n[7] = -1
let low = m[-1] - 1
(v[4] + v[5]) as int * 100000 + has * 10000 + (s[0] + s[1]) as int * 100 + (s[2] + s[3]) as int + low as int * 1000000 + n[7] as int * 7
"#,
        253 * 100000 + 10000 - 4 * 100 - 128 + 32767 * 1000000 - 7,
    );
}

#[test]
fn a_literal_out_of_range_for_a_collection_element_or_key_is_e0301() {
    assert_rejected(
        "let mut v: Vec<u8> = vec![]\nv.extend(vec![256])\n0",
        "E0301",
    );
    assert_rejected(
        "let mut m: Map<u8, u8> = Map::new()\nm[1] = 256\n0",
        "E0301",
    );
    assert_rejected("let mut v: Vec<u8> = vec![]\nv.fill(-1)\n0", "E0301");
}

#[test]
fn place_writes_with_side_effect_indices_wrap_once() {
    assert_int_everywhere(
        r#"
let mut calls = 0
fn idx() -> int {
    calls += 1
    1
}
fn key() -> u8 {
    calls += 1
    1
}
let mut v: Vec<u8> = vec![0, 250]
v[idx()] += 10
struct P { b: u8 }
let mut ps = vec![P { b: 0 }, P { b: 200 }]
ps[idx()].b += 100
let mut m: Map<u8, u8> = Map::new()
let _ = m.insert(1, 255)
m[key()] += 2
let mut a: [i32; 2] = [0x7FFFFFFF, 0]
a[idx()] -= 1
a[0] = a[0] << 1
v[1] as int * 1000000 + ps[1].b as int * 10000 + m[1] as int * 1000 + calls * 100 + a[0] as int + a[1] as int
"#,
        4 * 1000000 + 44 * 10000 + 1000 + 4 * 100 - 2 - 1,
    );
}

#[test]
fn struct_fields_hold_wrapped_sized_values_and_sized_containers() {
    assert_int_everywhere(
        r#"
struct P { b: u8, v: Vec<u8>, m: Map<u8, u8>, a: [i16; 2] }
let mut p = P { b: 250, v: vec![1], m: Map::new(), a: [32767, -1] }
p.b = p.b + 10
p.b += 200
p.v.push(255)
p.v[0] = p.v[0] - 2
let _ = p.m.insert(255, 1)
p.a[0] = p.a[0] + 1
let q = P { b: p.b, v: p.v, m: p.m, a: p.a }
q.b as int * 1000000 + q.v[0] as int * 1000 + (q.v[1] + q.v[1]) as int + q.a[0] as int + q.m[255] as int
"#,
        204 * 1000000 + 255 * 1000 + 254 - 32768 + 1,
    );
}

#[test]
fn a_literal_passed_to_a_method_of_a_sized_generic_receiver_takes_its_type() {
    assert_int_everywhere(
        r#"
struct Stack<T> { items: Vec<T> }
impl<T> Stack<T> {
    fn push(mut self, x: T) { self.items.push(x) }
    fn top(self) -> T { self.items[self.items.len() - 1] }
}
struct Pair<A, B> { a: A, b: B }
impl<A, B> Pair<A, B> {
    fn set_a(mut self, x: A) { self.a = x }
    fn set_b(mut self, x: B) { self.b = x }
}
fn new_stack<T>() -> Stack<T> { Stack { items: vec![] } }
fn id<T>(x: T) -> T { x }
let mut s: Stack<u8> = new_stack()
s.push(200)
s.push(250)
let mut p: Pair<u8, i8> = Pair { a: 0, b: 0 }
p.set_a(250 + 10)
p.set_b(100 + 27)
let plain = id(5)
let mut t = Stack { items: vec![] }
t.push(1000)
(s.top() + s.top()) as int * 10000 + p.a as int * 1000 + (p.b + 1) as int + plain + t.top()
"#,
        244 * 10000 + 4 * 1000 - 128 + 5 + 1000,
    );
    assert_rejected(
        "struct Stack<T> { items: Vec<T> }\nimpl<T> Stack<T> {\n    fn push(mut self, x: T) { self.items.push(x) }\n}\nlet mut s: Stack<u8> = Stack { items: vec![] }\ns.push(256)\n0",
        "E0301",
    );
}

const BITS: &str = r#"pub const MASK: u32 = 0xFFFFFFFF
pub const W: u8 = 200 + 100
pub struct Stack<T> { pub items: Vec<T> }
impl<T> Stack<T> {
    pub fn push(mut self, x: T) { self.items.push(x) }
    pub fn top(self) -> T { self.items[self.items.len() - 1] }
}
pub fn new_stack<T>() -> Stack<T> { Stack { items: vec![] } }
pub fn first<T>(v: Vec<T>) -> T { v[0] }
"#;

#[test]
fn imported_sized_constants_and_generic_templates_keep_the_width() {
    let value = run_module_int(
        &[("bits.aelys", BITS)],
        r#"needs bits
let x: u32 = 0xFFFFFFF0
let mut s: Stack<u8> = new_stack()
s.push(200)
s.push(250)
let v: Vec<u8> = vec![255, 1]
let wrapped = MASK + 1
(x & MASK) as int - 0xFFFFFFF0 + wrapped as int + (bits::W + W) as int * 1000 + (s.top() + s.top()) as int * 10 + (first(v) + 1) as int
"#,
    );
    assert_eq!(value, 88 * 1000 + 244 * 10);
}

#[test]
fn the_binary_format_carries_chars_maps_and_sized_instructions_together() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let source = "let c = 'é'\nlet mut m: Map<u8, char> = Map::new()\nlet _ = m.insert(200, c)\nlet b: u8 = c as u8\nlet k: u8 = b + 223\n(m[k] as int) * 1000 + k as int";
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("compile");
    let bytes = module.avbc().to_vec();
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 5);
    assert_eq!(run_avbc(&bytes), 233 * 1000 + 200);

    let plain =
        "let c = 'é'\nlet mut m: Map<int, char> = Map::new()\nlet _ = m.insert(1, c)\nm[1] as int";
    let module = runtime
        .compile(plain, CompileOptions::default())
        .expect("compile");
    let bytes = module.avbc().to_vec();
    assert!(u16::from_le_bytes([bytes[4], bytes[5]]) < 5);
    assert_eq!(run_avbc(&bytes), 233);
}

#[test]
fn assignments_through_places_type_a_literal_expression_from_the_place() {
    assert_int_everywhere(
        r#"
let mut n: Map<u8, i8> = Map::new()
n[1 + 1] = 100 + 27
let mut v: Vec<u8> = vec![0]
v[0] = 200 + 100
let mut a: [i16; 1] = [0]
a[0] = 32767 + 1
struct P { b: u8 }
let mut p = P { b: 0 }
p.b = 250 + 10
(n[2] + 1) as int * 1000000 + v[0] as int * 1000 + a[0] as int + p.b as int
"#,
        -128 * 1000000 + 44 * 1000 - 32768 + 4,
    );
}

#[test]
fn an_unannotated_global_constant_takes_the_width_of_the_constants_it_names() {
    assert_int_everywhere(
        r#"
fn early() -> u8 { B + 1 }
const A: u8 = 200
const B = A + A
fn g(x: u8) -> u8 { x }
let c = B
let d: u8 = c + 1
early() as int * 1000000 + g(B) as int * 1000 + d as int
"#,
        145 * 1000000 + 144 * 1000 + 145,
    );
}
