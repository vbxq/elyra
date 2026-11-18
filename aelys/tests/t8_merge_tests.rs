use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use std::fmt::Write as _;
use std::path::PathBuf;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 3] = [JitMode::Off, JitMode::Baseline, JitMode::Tiered];

const UNINIT: &str = "read before its initialiser ran";

fn value_at(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let module = runtime
        .compile(
            source,
            CompileOptions {
                optimization_level: level,
                ..CompileOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => isolate
            .structured_clone(value)
            .map_err(|error| format!("{error:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn assert_int_everywhere(source: &str, expected: i64) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::Int(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => panic!("{level:?} {jit:?}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn assert_runtime_error_everywhere(source: &str, needle: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(value) => {
                    panic!("{level:?} {jit:?}: expected a runtime error, got {value:?}\n{source}")
                }
                Err(text) => {
                    assert!(
                        text.contains(needle),
                        "{level:?} {jit:?}: expected '{needle}', got:\n{text}"
                    );
                    assert!(!text.contains("[E0"), "a compile error:\n{text}");
                }
            }
        }
    }
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write source");
    path
}

#[test]
fn a_function_reading_a_global_is_bound_before_top_level_code_and_reads_it_once_set() {
    assert_int_everywhere(
        "let b = 10\nlet a = f()\nlet mut c = 0\nc = g()\nfn f() -> int { b + 1 }\nfn g() -> int { a * 2 + b }\nstruct S { v: int }\nimpl S { fn get(self) -> int { self.v + b } }\nlet s = S { v: 5 }.get()\na * 10000 + c * 100 + s",
        11 * 10000 + 32 * 100 + 15,
    );
    assert_runtime_error_everywhere(
        "let a = S { v: 1 }.get()\nlet b = 10\nstruct S { v: int }\nimpl S { fn get(self) -> int { self.v + b } }\na",
        &format!("global `b` {UNINIT}"),
    );
}

#[test]
fn unit_tails_cover_value_blocks_match_arms_and_hoisted_compound_assignments() {
    assert_int_everywhere(
        r#"
let mut g = 0
let mut calls = 0
fn ix() -> int {
    calls += 1
    1
}
fn blk() { { g = 5 } }
fn arms(c: int) {
    match c { 0 => g = 1, _ => { g = c } }
}
fn idx(mut v: Vec<int>) {
    if v.len() == 0 { return () }
    v[ix()] += 10
}
let lam = fn() { { g += 100 } }
let mut v = vec![1, 2]
let mut bits = 0
if blk() == () { bits += 1 }
if arms(7) == () { bits += 2 }
if idx(v) == () { bits += 4 }
if lam() == () { bits += 8 }
bits * 100000 + g * 100 + v[1] * 10 + calls
"#,
        15 * 100000 + 107 * 100 + 12 * 10 + 1,
    );
}

#[test]
fn a_char_global_and_the_uninitialised_sentinel_stay_distinct() {
    assert_int_everywhere(
        r#"
let z = '\0'
let one = '\u{1}'
let x = 'x'
fn gz() -> char { z }
fn g1() -> char { one }
fn gx() -> char { x }
let mut bits = 0
if gz() == '\0' { bits += 1 }
if g1() as int == 1 { bits += 2 }
if gx() == 'x' { bits += 4 }
if z as int == 0 { bits += 8 }
bits
"#,
        15,
    );
    assert_runtime_error_everywhere(
        "let a = g()\nlet z = '\\0'\nfn g() -> char { z }\n1",
        &format!("global `z` {UNINIT}"),
    );
    assert_runtime_error_everywhere(
        "let a = g()\nlet one = '\\u{1}'\nfn g() -> int { one as int }\n1",
        &format!("global `one` {UNINIT}"),
    );
}

#[test]
fn constants_fold_into_functions_bound_before_top_level_code() {
    assert_int_everywhere(
        "let a = f()\nconst K: int = 7\nconst W: u8 = 200 + 100\nfn f() -> int { K + 1 + W as int }\na",
        52,
    );
}

#[test]
fn module_variables_read_too_early_are_the_uninitialised_error_and_live_reads_work() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "early.aelys",
        "pub fn get<T>(x: T) -> int { scale }\npub let first = get(1)\npub let mut scale = 3\n",
    );
    let main = write(&dir, "main.aelys", "use early\nearly::first\n");
    let text = run_file(&main)
        .expect_err("the module reads `scale` before it is bound")
        .to_string();
    assert!(text.contains(&format!("global `scale` {UNINIT}")), "{text}");

    write(
        &dir,
        "live.aelys",
        "pub let mut scale = 3\npub let mut letter = '\\0'\npub fn bump() { scale += 1 }\npub fn get<T>(x: T) -> int { scale }\n",
    );
    let main = write(
        &dir,
        "main2.aelys",
        "use live\nuse live::{scale, letter, bump}\nfn early() -> int { scale * 100 + live::scale }\nlet a = early()\nbump()\nlet b = early()\nlet c = if letter == '\\0' { 1 } else { 0 }\na * 1000000 + b * 100 + live::get(1) * 10 + c\n",
    );
    let value = run_file(&main).expect("module bound before main runs");
    assert_eq!(value.as_int(), Some(303 * 1000000 + 404 * 100 + 4 * 10 + 1));
}

#[test]
fn a_function_with_a_native_import_calls_the_callers_latest_functions_under_the_jit() {
    assert_int_everywhere(
        r#"
use std::math
let mut sink = 0
fn helper(x: int) -> int {
    sink = x
    x * 2
}
fn f(x: int) -> int { math::abs(x) + helper(x) }
let mut acc = 0
for i in 0..5 {
    acc += f(0 - i)
}
acc * 100 + sink
"#,
        (0..5).map(|i: i64| i - 2 * i).sum::<i64>() * 100 - 4,
    );
}

#[test]
fn new_call_sites_keep_live_registers_above_their_window() {
    assert_int_everywhere(
        r#"
fn hog(a: int) -> int {
    let x1 = a + 1000
    let x2 = x1 * 3
    let x3 = x2 + x1
    x3 - x2 - 1000
}
struct P { v: Vec<int> }
fn f(n: int) -> int {
    let keep = n * 7
    let mut v = vec![n + 2, n, n + 1]
    v.sort_by(fn(a: int, b: int) -> int { hog(b) - a })
    let after = hog(keep)
    let p = P { v: vec![keep] }
    let mut q = p.clone()
    q.v.push(hog(n))
    let mut m: Map<int, int> = Map::new()
    m[hog(n)] = keep
    v[0] * 100000 + q.v[1] * 1000 + m[n] * 10 + after + keep
}
f(1) + f(3)
"#,
        3 * 100000 + 1000 + 70 + 14 + (5 * 100000 + 3000 + 210 + 42),
    );
}

fn wide_frame_program(locals: usize) -> (String, i64) {
    let mut source = String::from("fn hog(a: int) -> int {\n    a\n}\nfn f(n: int) -> int {\n");
    for i in 0..locals {
        writeln!(source, "    let l{i} = n + {i}").unwrap();
    }
    let last = locals - 1;
    writeln!(source, "    let a: u8 = (l{last} as u8) + 250").unwrap();
    source.push_str("    let b: i32 = (l3 as i32) << 8\n");
    source.push_str("    let c = b.mul_shr(b, 16)\n");
    source.push_str("    let neg: i8 = -(l5 as i8)\n");
    source.push_str("    let ch = 'a'\n");
    writeln!(source, "    let t = (l{}, hog(l{}))", last - 2, last - 3).unwrap();
    source.push_str("    let (p, q) = t\n");
    source.push_str("    let mut v = vec![3, 1, l1]\n");
    source.push_str("    v.sort_by(fn(x: int, y: int) -> int { x - y })\n");
    source.push_str("    let w = v.clone()\n");
    source.push_str("    let mut acc = 0\n");
    for chunk in (0..locals).collect::<Vec<_>>().chunks(10) {
        let terms: Vec<String> = chunk.iter().map(|i| format!("l{i}")).collect();
        writeln!(source, "    acc += {}", terms.join(" + ")).unwrap();
    }
    source.push_str(
        "    acc + a as int + c as int + neg as int + ch as int + p + q + w[2]\n}\nf(1)\n",
    );
    let n = 1i64;
    let count = i64::try_from(locals).unwrap();
    let sum: i64 = (0..count).map(|i| n + i).sum();
    let a = (n + count - 1 + 250) % 256;
    let b = (n + 3) << 8;
    let c = (b * b) >> 16;
    let neg = -(n + 5);
    let p = n + count - 3;
    let q = n + count - 4;
    let mut v = [3, 1, n + 1];
    v.sort_unstable();
    (source, sum + a + c + neg + 97 + p + q + v[2])
}

#[test]
fn sized_fixed_char_tuple_and_collection_operands_above_r255_are_never_truncated() {
    for locals in [250, 300] {
        let (source, expected) = wide_frame_program(locals);
        assert_int_everywhere(&source, expected);
    }
}
