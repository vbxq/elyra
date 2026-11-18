mod t9_support;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, Value};
use t9_support::run_with;

const MODES: [&[&str]; 4] = [
    &["-ae.jit=on"],
    &["-ae.jit=on", "-ae.jit-sync=on"],
    &["-O3", "-ae.jit=on", "-ae.jit-sync=on"],
    &[
        "-ae.jit=on",
        "-ae.jit-sync=on",
        "-ae.gc-stress=on",
        "-ae.gc-verify=on",
    ],
];

// a byte read as an 8-byte int slot would mix neighbouring bytes into every value
const BYTE_LOOPS: &str = r#"fn fill(mut v: Vec<u8>, seed: int) {
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
fn mixed(bytes: Vec<u8>, mut ints: Vec<int>) -> int {
    let mut s = 0
    for i in 0..bytes.len() {
        ints[i] = bytes[i] as int * 1000
        s = (s + ints[i] + bytes[i] as int) % 1000000007
    }
    s
}
fn each(v: Vec<u8>) -> int {
    let mut s = 0
    for b in v { s = (s * 7 + b as int) % 1000003 }
    s
}
fn grow(n: int) -> int {
    let mut v: Vec<u8> = vec![]
    for i in 0..n { v.push((i * 13 % 256) as u8) }
    checksum(v)
}
let mut a: Vec<u8> = vec![0; 50000]
fill(a, 1)
let mut ints: Vec<int> = vec![0; 50000]
let total = checksum(a) + mixed(a, ints) + each(a) + grow(30000) + ints[49999]
"#;

fn expected() -> i64 {
    let mut x = 1i64;
    let a: Vec<i64> = (0..50000)
        .map(|_| {
            x = (x * 1103515245 + 12345) % 2147483648;
            (x / 65536) % 256
        })
        .collect();
    let checksum = |v: &[i64]| v.iter().fold(0i64, |h, b| (h * 31 + b) % 1000000007);
    let mixed = a.iter().fold(0i64, |s, b| (s + b * 1000 + b) % 1000000007);
    let each = a.iter().fold(0i64, |s, b| (s * 7 + b) % 1000003);
    let grown: Vec<i64> = (0..30000).map(|i| i * 13 % 256).collect();
    checksum(&a) + mixed + each + checksum(&grown) + a[49999] * 1000
}

#[test]
fn compiled_loops_over_packed_bytes_compute_the_interpreter_values() {
    let want = expected();
    for flags in MODES {
        let outcome = run_with(&format!("{BYTE_LOOPS}println(total)\n"), flags);
        assert_eq!(outcome.code, Some(0), "{flags:?}\n{}", outcome.stderr);
        assert_eq!(outcome.lines(), vec![want.to_string().as_str()], "{flags:?}");
    }
    let interpreted = run_with(&format!("{BYTE_LOOPS}println(total)\n"), &["-ae.jit=off"]);
    assert_eq!(interpreted.lines(), vec![want.to_string().as_str()]);
}

#[test]
fn the_byte_loops_reach_machine_code() {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(&format!("{BYTE_LOOPS}total\n"), CompileOptions::default())
        .expect("program compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) = isolate
        .execute(&module, RunOptions::default())
        .expect("program runs")
    else {
        panic!("expected a value");
    };
    assert_eq!(value, Value::int(expected()));
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert!(
            runtime.jit_cache_entries() >= 1,
            "the program never reached machine code"
        );
    }
}
