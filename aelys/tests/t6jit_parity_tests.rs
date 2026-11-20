use aelys::{
    AelysError, CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime,
    Value,
};
use aelys_common::error::RuntimeErrorKind;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "aelys-cli"])
                .status()
                .expect("cargo build should run");
            assert!(status.success(), "building aelys-cli failed");
            let mut directory = std::env::current_exe().expect("test executable path");
            directory.pop();
            if directory.ends_with("deps") {
                directory.pop();
            }
            let binary = directory.join(format!("elyra{}", std::env::consts::EXE_SUFFIX));
            assert!(binary.is_file(), "missing aelys-cli at {binary:?}");
            binary
        })
        .clone()
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn run_cli(source: &str, jit: bool) -> Outcome {
    let mut file = tempfile::Builder::new()
        .prefix("t6jit")
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    let mut command = Command::new(cli_binary());
    command.arg("run");
    if jit {
        command.arg("-ae.jit=on");
    }
    let output = command
        .arg(file.path())
        .output()
        .expect("aelys-cli should run");
    let name = file.path().to_str().unwrap_or_default().to_string();
    let clean = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(&name, "<file>");
    Outcome {
        stdout: clean(&output.stdout),
        stderr: clean(&output.stderr),
        code: output.status.code(),
    }
}

fn parity(source: &str) -> Outcome {
    let interpreted = run_cli(source, false);
    let compiled = run_cli(source, true);
    assert_eq!(
        interpreted, compiled,
        "the jit changed the observable behaviour of:\n{source}"
    );
    compiled
}

fn evaluate(source: &str) -> (Value, Runtime) {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("program compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .expect("program runs");
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    (value, runtime)
}

fn evaluate_error(source: &str) -> (RuntimeErrorKind, Runtime) {
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("program compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let error = isolate
        .execute(&module, RunOptions::default())
        .expect_err("program must trap");
    let AelysError::Runtime(error) = error else {
        panic!("expected a runtime error, got {error:?}");
    };
    (error.kind, runtime)
}

fn printed(body: &str, expression: &str) -> String {
    format!("{body}\nprintln(\"{{{expression}}}\")\n")
}

fn returned(body: &str, expression: &str) -> String {
    format!("{body}\n{expression}\n")
}

fn check_int(body: &str, expression: &str, expected: i64) {
    let outcome = parity(&printed(body, expression));
    assert_eq!(outcome.code, Some(0), "{outcome:?}");
    assert_eq!(
        outcome.stdout.lines().next(),
        Some(expected.to_string().as_str())
    );
    let (value, runtime) = evaluate(&returned(body, expression));
    assert_eq!(value, Value::int(expected));
    assert!(
        runtime.jit_cache_entries() >= 1,
        "the program never reached machine code"
    );
}

const BITWISE: &str = r#"
fn mix(n: int) -> int {
    let mut a = 12345
    let mut b = 777
    for i in 0..n {
        a = (a ^ (i << 3)) & 0xFFFFFF
        b = (b | (a >> 2)) ^ (~i & 0xFFFF)
        a = a + (b % 97) - (i / 7)
    }
    return a * 1000 + (b & 0xFFFF)
}
"#;

fn bitwise_reference(n: i64) -> i64 {
    let (mut a, mut b) = (12345i64, 777i64);
    for i in 0..n {
        a = (a ^ (i << 3)) & 0xFF_FFFF;
        b = (b | (a >> 2)) ^ (!i & 0xFFFF);
        a = a + (b % 97) - (i / 7);
    }
    a * 1000 + (b & 0xFFFF)
}

#[test]
fn bitwise_division_and_remainder_match_the_interpreter_in_a_loop() {
    check_int(BITWISE, "mix(100000)", bitwise_reference(100_000));
}

const PIXELS: &str = r#"
fn run(frames: int) -> int {
    let mut buf: Vec<int> = vec![0; 6400]
    let mut arr: [int; 6400] = [0; 6400]
    let mut colormap: Vec<int> = vec![0; 256]
    let mut texture: [int; 128] = [0; 128]
    for i in 0..256 { colormap[i] = (i * 7 + 3) & 255 }
    for i in 0..128 { texture[i] = (i * 37 + 11) & 255 }
    let mut acc = 0
    for f in 0..frames {
        for x in 0..80 {
            let mut frac = (x * (f + 1) * 977) & 0xFFFFFF
            let stp = 0x10000 + ((x + f) & 255) * 256
            for y in 0..80 {
                let c = colormap[texture[(frac >> 16) & 127]]
                buf[y * 80 + x] = c
                arr[y * 80 + x] = c ^ f
                frac = frac + stp
            }
        }
        acc = (acc + buf[(f * 131) % 6400] + arr[(f * 7919) % 6400]) & 0xFFFFFFF
    }
    let mut s = 0
    for i in 0..6400 { s = (s + buf[i] * ((i & 7) + 1) + arr[i]) & 0xFFFFFFF }
    return (acc & 0xFFFF) * 1000000000 + s
}
"#;

fn pixels_reference(frames: i64) -> i64 {
    let mut buf = vec![0i64; 6400];
    let mut arr = vec![0i64; 6400];
    let colormap = (0..256).map(|i| (i * 7 + 3) & 255).collect::<Vec<i64>>();
    let texture = (0..128).map(|i| (i * 37 + 11) & 255).collect::<Vec<i64>>();
    let mut acc = 0i64;
    for f in 0..frames {
        for x in 0..80i64 {
            let mut frac = (x * (f + 1) * 977) & 0xFF_FFFF;
            let stp = 0x10000 + ((x + f) & 255) * 256;
            for y in 0..80i64 {
                let c = colormap[texture[((frac >> 16) & 127) as usize] as usize];
                buf[(y * 80 + x) as usize] = c;
                arr[(y * 80 + x) as usize] = c ^ f;
                frac += stp;
            }
        }
        acc = (acc + buf[((f * 131) % 6400) as usize] + arr[((f * 7919) % 6400) as usize])
            & 0xFFF_FFFF;
    }
    let mut s = 0i64;
    for i in 0..6400usize {
        s = (s + buf[i] * ((i as i64 & 7) + 1) + arr[i]) & 0xFFF_FFFF;
    }
    (acc & 0xFFFF) * 1_000_000_000 + s
}

#[test]
fn vec_and_array_loads_and_stores_match_the_interpreter() {
    check_int(PIXELS, "run(12)", pixels_reference(12));
}

const STRUCTS: &str = r#"
struct P { a: int, b: int, c: int, d: int }
struct W { p: P, n: int }

fn single(n: int) -> int {
    let mut p = P { a: 1, b: 2, c: 3, d: 4 }
    for i in 0..n {
        p.a = (p.a + i) & 0xFFFFF
        p.b = (p.b ^ p.a) & 0xFFFF
        p.c = (p.c + p.b) & 0xFFFFFF
        p.d = (p.d + p.c + p.a) & 0xFFFFFFF
    }
    return p.a ^ p.b ^ p.c ^ p.d
}

fn many(rounds: int) -> int {
    let mut v: Vec<P> = vec![]
    for j in 0..100 {
        v.push(P { a: j, b: j * 2, c: j * 3, d: 0 })
    }
    for r in 0..rounds {
        for j in 0..100 {
            let mut s = v[j]
            s.a = (s.a + r) & 0xFFFF
            s.d = (s.d + s.a * s.b + s.c) & 0xFFFFFFF
            v[j] = s
        }
    }
    let mut s = 0
    for j in 0..100 {
        s = (s + v[j].d + v[j].a) & 0xFFFFFFF
    }
    return s
}

fn nested(n: int) -> int {
    let mut w = W { p: P { a: 0, b: 1, c: 2, d: 3 }, n: 0 }
    for i in 0..n {
        let mut q = w.p
        q.b = (q.b + i) & 0xFFFF
        w.n = (w.n + q.b) & 0xFFFFFF
    }
    return w.n + w.p.b
}
"#;

fn structs_reference() -> i64 {
    let (mut a, mut b, mut c, mut d) = (1i64, 2i64, 3i64, 4i64);
    for i in 0..50_000i64 {
        a = (a + i) & 0xF_FFFF;
        b = (b ^ a) & 0xFFFF;
        c = (c + b) & 0xFF_FFFF;
        d = (d + c + a) & 0xFFF_FFFF;
    }
    let single = a ^ b ^ c ^ d;
    let mut v = (0..100i64)
        .map(|j| [j, j * 2, j * 3, 0])
        .collect::<Vec<_>>();
    for r in 0..300i64 {
        for s in v.iter_mut() {
            s[0] = (s[0] + r) & 0xFFFF;
            s[3] = (s[3] + s[0] * s[1] + s[2]) & 0xFFF_FFFF;
        }
    }
    let many = v.iter().fold(0i64, |s, p| (s + p[3] + p[0]) & 0xFFF_FFFF);
    let (mut pb, mut wn) = (1i64, 0i64);
    for i in 0..30_000i64 {
        pb = (pb + i) & 0xFFFF;
        wn = (wn + pb) & 0xFF_FFFF;
    }
    (single % 1000) * 1_000_000 + (many % 1000) * 1000 + ((wn + pb) % 1000)
}

#[test]
fn struct_fields_read_and_written_by_offset_match_the_interpreter() {
    check_int(
        STRUCTS,
        "(single(50000) % 1000) * 1000000 + (many(300) % 1000) * 1000 + nested(30000) % 1000",
        structs_reference(),
    );
}

const CALLS: &str = r#"
fn fib(n: int) -> int {
    if n < 2 { return n }
    return fib(n - 1) + fib(n - 2)
}
fn f3(x: int) -> int { return (x & 1023) + 1 }
fn f2(x: int) -> int { return f3(x) ^ 3 }
fn f1(x: int) -> int { return f2(x) + 5 }
fn chain(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        acc = (acc + f1(i)) & 0xFFFFFFF
    }
    return acc
}
"#;

fn calls_reference() -> i64 {
    fn fib(n: i64) -> i64 {
        if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
    }
    let mut acc = 0i64;
    for i in 0..60_000i64 {
        acc = (acc + ((((i & 1023) + 1) ^ 3) + 5)) & 0xFFF_FFFF;
    }
    fib(22) * 1000 + acc
}

#[test]
fn calls_between_compiled_functions_match_the_interpreter() {
    check_int(CALLS, "fib(22) * 1000 + chain(60000)", calls_reference());
}

const MIXED_CALLS: &str = r#"
fn label(i: int) -> int {
    let text = "n{i}"
    return text.len()
}
fn plain(i: int) -> int { return i * 3 }
fn run(n: int) -> int {
    let mut total = 0
    for i in 0..n {
        if i % 1000 == 7 {
            total = total + label(i)
        } else {
            total = total + plain(i)
        }
    }
    return total
}
"#;

fn mixed_reference(n: i64) -> i64 {
    (0..n)
        .map(|i| {
            if i % 1000 == 7 {
                1 + i.to_string().len() as i64
            } else {
                i * 3
            }
        })
        .sum()
}

#[test]
fn a_call_the_jit_cannot_compile_resumes_in_the_interpreter() {
    check_int(MIXED_CALLS, "run(50000)", mixed_reference(50_000));
}

const FLOATS: &str = r#"
fn wave(n: int) -> float {
    let mut v: Vec<float> = vec![0.0; 64]
    let mut x = 0.5
    let mut y = 1.25
    for i in 0..n {
        x = x * 1.0001 + 0.25
        y = y / 1.5 + x
        v[i % 64] = v[i % 64] + y - x
        if x > 1000.0 { x = x - 999.0 }
    }
    let mut s = 0.0
    for i in 0..64 { s = s + v[i] }
    return s + x + y
}
"#;

fn floats_reference(n: i64) -> f64 {
    let mut v = [0.0f64; 64];
    let (mut x, mut y) = (0.5f64, 1.25f64);
    for i in 0..n {
        x = x * 1.0001 + 0.25;
        y = y / 1.5 + x;
        let slot = (i % 64) as usize;
        v[slot] = v[slot] + y - x;
        if x > 1000.0 {
            x -= 999.0;
        }
    }
    let mut s = 0.0f64;
    for item in v {
        s += item;
    }
    s + x + y
}

#[test]
fn float_arithmetic_and_float_vectors_match_the_interpreter() {
    let (value, runtime) = evaluate(&returned(FLOATS, "wave(40000)"));
    assert_eq!(
        value.as_float().map(f64::to_bits),
        Some(floats_reference(40_000).to_bits())
    );
    assert!(runtime.jit_cache_entries() >= 1);
    let outcome = parity(&printed(FLOATS, "wave(40000)"));
    assert_eq!(outcome.code, Some(0));
}

const BOOLS: &str = r#"
fn sieve(n: int) -> int {
    let mut composite: Vec<bool> = vec![false; 20000]
    let mut count = 0
    for i in 2..n {
        if composite[i] == false {
            count = count + 1
            let mut j = i * i
            while j < n {
                composite[j] = true
                j = j + i
            }
        }
    }
    return count
}
"#;

#[test]
fn bool_vectors_and_while_loops_match_the_interpreter() {
    check_int(BOOLS, "sieve(20000)", 2262);
}

fn assert_trap(source: &str, message: &str, location: &str, kind: RuntimeErrorKind) {
    let outcome = parity(source);
    assert_eq!(outcome.code, Some(1), "{outcome:?}");
    assert!(
        outcome.stderr.contains(message),
        "missing {message:?} in {}",
        outcome.stderr
    );
    assert!(
        outcome.stderr.contains(location),
        "missing {location:?} in {}",
        outcome.stderr
    );
    let (error, runtime) = evaluate_error(&source.replace("println(", "let _ = ("));
    assert_eq!(
        std::mem::discriminant(&error),
        std::mem::discriminant(&kind)
    );
    assert!(
        runtime.jit_cache_entries() >= 1,
        "the trap never ran in machine code"
    );
    assert!(
        runtime.jit_deoptimizations() >= 1,
        "machine code never handed the trapping instruction back to the interpreter"
    );
}

#[test]
fn an_out_of_bounds_index_in_a_compiled_callee_reports_the_interpreter_trap() {
    let source = r#"fn get(v: Vec<int>, i: int) -> int {
    return v[i]
}
fn run(n: int) -> int {
    let mut v: Vec<int> = vec![0; 100]
    let mut s = 0
    for i in 0..n {
        s = s + get(v, i / 190)
    }
    return s
}
println("{run(20000)}")
"#;
    assert_trap(
        source,
        "index 100 is out of bounds for length 100",
        "get (<file>:2)\n  run (<file>:8)",
        RuntimeErrorKind::IndexOutOfBounds {
            index: 0,
            length: 0,
        },
    );
}

#[test]
fn a_negative_array_index_in_a_hot_loop_reports_the_interpreter_trap() {
    let source = r#"fn run(n: int) -> int {
    let mut a: [int; 16] = [0; 16]
    let mut s = 0
    for i in 0..n {
        let k = 15 - i / 1700
        a[k + 1 - 1] = i
        s = s + a[k]
    }
    return s
}
println("{run(30000)}")
"#;
    assert_trap(
        source,
        "index -1 is out of bounds for length 16",
        "run (<file>:6)",
        RuntimeErrorKind::IndexOutOfBounds {
            index: 0,
            length: 0,
        },
    );
}

#[test]
fn integer_overflow_in_a_hot_loop_reports_the_interpreter_trap() {
    let source = r#"fn run(n: int) -> int {
    let mut x = 1
    for i in 0..n {
        x = (x * 3 + i) & 0xFFFFFF
        if i == 40000 { x = (x + 1000000) * 100000000000000 }
    }
    return x
}
println("{run(50000)}")
"#;
    assert_trap(
        source,
        "integer overflow",
        "run (<file>:5)",
        RuntimeErrorKind::IntegerOverflow,
    );
}

#[test]
fn division_by_zero_in_a_compiled_function_reports_the_interpreter_trap() {
    let source = r#"fn ratio(a: int, b: int) -> int {
    let mut q = 0
    for k in 0..2 { q = q + a / b }
    return q
}
fn run(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = s + ratio(i, 4000 - i)
    }
    return s
}
println("{run(5000)}")
"#;
    assert_trap(
        source,
        "division by zero",
        "ratio (<file>:3)\n  run (<file>:9)",
        RuntimeErrorKind::DivisionByZero,
    );
}

#[test]
fn remainder_by_zero_in_an_on_stack_replaced_loop_reports_the_interpreter_trap() {
    let source = r#"fn run(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = s + i % (30000 - i)
    }
    return s
}
println("{run(40000)}")
"#;
    assert_trap(
        source,
        "division by zero",
        "run (<file>:4)",
        RuntimeErrorKind::DivisionByZero,
    );
}

#[test]
fn deep_recursion_through_compiled_calls_reports_the_interpreter_stack_overflow() {
    let source = r#"fn down(n: int) -> int {
    if n == 0 { return 0 }
    return down(n - 1) + 1
}
fn run() -> int {
    let mut s = 0
    for i in 0..2000 { s = s + down(10) }
    return s + down(5000)
}
println("{run()}")
"#;
    let outcome = parity(source);
    assert_eq!(outcome.code, Some(1), "{outcome:?}");
    assert!(
        outcome.stderr.contains("down (<file>:3)"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn a_global_rebound_between_calls_is_seen_by_compiled_code() {
    let body = r#"
let mut factor = 3
fn scaled(i: int) -> int { return i * factor }
fn run(n: int) -> int {
    let mut s = 0
    for i in 0..n { s = s + scaled(i) }
    return s
}
fn both() -> int {
    let first = run(20000)
    factor = 5
    return first * 10 + run(20000) % 10
}
"#;
    let first: i64 = (0..20_000i64).map(|i| i * 3).sum();
    let second: i64 = (0..20_000i64).map(|i| i * 5).sum();
    check_int(body, "both()", first * 10 + second % 10);
}
