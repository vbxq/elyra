use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const JIT_TARGET: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

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
            let binary = directory.join(format!("aelys-cli{}", std::env::consts::EXE_SUFFIX));
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

fn run_path(path: &Path, jit: bool, extra: &[&str]) -> Outcome {
    let mut command = Command::new(cli_binary());
    command.arg("run");
    if jit {
        command.arg("-ae.jit=on");
    }
    let output = command
        .args(extra)
        .arg(path)
        .output()
        .expect("aelys-cli should run");
    let name = path.to_str().unwrap_or_default().to_string();
    let clean = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(&name, "<file>");
    Outcome {
        stdout: clean(&output.stdout),
        stderr: clean(&output.stderr),
        code: output.status.code(),
    }
}

fn parity_with(source: &str, extra: &[&str]) -> Outcome {
    let mut file = tempfile::Builder::new()
        .prefix("t6jitfix")
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    let interpreted = run_path(file.path(), false, extra);
    let compiled = run_path(file.path(), true, extra);
    assert_eq!(
        interpreted, compiled,
        "the jit changed the observable behaviour of:\n{source}"
    );
    compiled
}

fn parity(source: &str) -> Outcome {
    parity_with(source, &[])
}

fn first_line(outcome: &Outcome) -> &str {
    outcome.stdout.lines().next().unwrap_or_default()
}

struct Hot {
    value: Value,
    entries: usize,
    exits: u64,
}

fn evaluate(source: &str) -> Hot {
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
    Hot {
        value,
        entries: runtime.jit_cache_entries(),
        exits: runtime.jit_deoptimizations(),
    }
}

fn check(body: &str, expression: &str, expected: i64) -> Hot {
    let outcome = parity(&format!("{body}\nprintln(\"{{{expression}}}\")\n"));
    assert_eq!(outcome.code, Some(0), "{outcome:?}");
    assert_eq!(first_line(&outcome), expected.to_string());
    let hot = evaluate(&format!("{body}\n{expression}\n"));
    assert_eq!(hot.value, Value::int(expected));
    hot
}

const RARE_EXIT: &str = r#"
struct Cell { a: int }
fn work(n: int, mut v: Vec<int>) -> int {
    let mut acc = 0
    for i in 0..n {
        v[i & 1023] = (v[(i * 7) & 1023] + i) & 0xFFFF
        acc = (acc + v[i & 1023]) & 0xFFFFFF
        if (i & 4095) == 4095 {
            let cell = Cell { a: i & 15 }
            acc = acc ^ cell.a
        }
    }
    return acc
}
let mut v: Vec<int> = vec![0; 1024]
"#;

fn rare_exit_reference(n: i64, every: i64) -> i64 {
    let mut v = vec![0i64; 1024];
    let mut acc = 0i64;
    for i in 0..n {
        v[(i & 1023) as usize] = (v[((i * 7) & 1023) as usize] + i) & 0xFFFF;
        acc = (acc + v[(i & 1023) as usize]) & 0xFF_FFFF;
        if (i & every) == every {
            acc ^= i & 15;
        }
    }
    acc
}

#[test]
fn a_loop_leaving_machine_code_rarely_stays_compiled() {
    let hot = check(
        RARE_EXIT,
        "work(2000000, v)",
        rare_exit_reference(2_000_000, 4095),
    );
    if JIT_TARGET {
        let rare_events = 2_000_000 / 4096;
        assert!(
            hot.exits > 400 && hot.exits <= rare_events,
            "every rare event should leave machine code once and the loop must come back: {} exits for {rare_events} events",
            hot.exits
        );
    }
}

#[test]
fn a_loop_leaving_machine_code_on_every_iteration_is_dropped() {
    let body = RARE_EXIT.replace("(i & 4095) == 4095", "(i & 0) == 0");
    let hot = check(&body, "work(200000, v)", rare_exit_reference(200_000, 0));
    if JIT_TARGET {
        assert!(
            hot.exits > 0 && hot.exits < 200,
            "a loop that leaves on every iteration must be given up: {} exits",
            hot.exits
        );
    }
}

const GLOBAL_WRITES: &str = r#"
struct P { a: int }
let mut counter = 0
let mut scale = 0.0
let mut current: Vec<int> = vec![0]
let mut holder = P { a: 0 }
let mut label = "start"
let mut left: Vec<int> = vec![1, 2, 3]
let mut right: Vec<int> = vec![10, 20, 30]

impl P {
    fn touch(self) -> int {
        counter = counter + self.a
        return counter
    }
}

fn store<T>(value: T) -> T {
    counter = counter + 1
    return value
}

fn bump(i: int) -> int {
    counter = counter + i
    counter += 1
    scale = scale + 0.5
    if (i & 1) == 0 { current = left } else { current = right }
    holder = P { a: i & 7 }
    if (i & 2) == 0 { label = "even" } else { label = "odd" }
    return current[1] + holder.a
}

fn hot_loop(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = (s + bump(i)) & 0xFFFFFFF
        counter += 2
        s = s + store(i & 3) + holder.touch() * 0
    }
    return s
}
"#;

fn global_writes_reference(n: i64) -> (i64, i64) {
    let mut counter = 0i64;
    let mut s = 0i64;
    for i in 0..n {
        counter += i + 1;
        let current = if i & 1 == 0 { 2 } else { 20 };
        let holder = i & 7;
        s = (s + current + holder) & 0xFFF_FFFF;
        counter += 2;
        counter += 1;
        s += i & 3;
        counter += holder;
    }
    (s, counter)
}

#[test]
fn global_writes_run_in_machine_code_with_the_interpreter_semantics() {
    let n = 30_000;
    let (s, counter) = global_writes_reference(n);
    let expected = s * 1000 + counter % 1000;
    let hot = check(
        GLOBAL_WRITES,
        &format!("hot_loop({n}) * 1000 + counter % 1000"),
        expected,
    );
    let outcome = parity(&format!(
        "{GLOBAL_WRITES}\nlet r = hot_loop({n})\nprintln(\"{{r}} {{counter}} {{scale == {}.0}} {{label}} {{current[0]}} {{holder.a}}\")\n",
        n / 2
    ));
    let last = n - 1;
    let label = if last & 2 == 0 { "even" } else { "odd" };
    let current = if last & 1 == 0 { 1 } else { 10 };
    assert_eq!(
        first_line(&outcome),
        format!("{s} {counter} true {label} {current} {}", last & 7)
    );
    if JIT_TARGET {
        assert!(hot.entries >= 2, "{} entries", hot.entries);
    }
}

#[test]
fn a_top_level_loop_writing_globals_never_leaves_machine_code() {
    let body = r#"
let mut total = 0
let mut parity_flag = false
for i in 0..200000 {
    total = (total + i * 3) & 0xFFFFFF
    total += 1
    parity_flag = (i & 1) == 0
}
"#;
    let mut total = 0i64;
    for i in 0..200_000i64 {
        total = (total + i * 3) & 0xFF_FFFF;
        total += 1;
    }
    let hot = check(body, "total", total);
    if JIT_TARGET {
        assert_eq!(hot.exits, 0, "the loop left machine code");
        assert!(hot.entries >= 1);
    }
}

#[test]
fn a_global_function_swapped_by_machine_code_is_called_fresh() {
    let body = r#"
fn one() -> int { return 1 }
fn two() -> int { return 2 }
let mut pick = one
fn run(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = s + pick()
        if (i & 1) == 0 { pick = two } else { pick = one }
    }
    return s
}
"#;
    check(body, "run(100000)", 150_000);
}

#[test]
fn a_hot_function_calling_a_native_reads_user_globals_correctly() {
    let body = r#"
use std::math
let mut g = 2
fn f(x: int) -> int { math::abs(x) + g }
let mut s = 0
for i in 0..40000 { s = s + f(i) }
"#;
    let hot = check(body, "s", 800_060_000);
    let helper = r#"
use std::math
let mut g = 2
fn h() -> int { g }
fn f(x: int) -> int { math::abs(x) + h() }
let mut s = 0
for i in 0..40000 { s = s + f(i) }
"#;
    check(helper, "s", 800_060_000);
    let changing = r#"
use std::math
let mut g = 2
fn f(x: int) -> int { math::abs(0 - x) + g }
let mut s = 0
for i in 0..40000 {
    g = i & 3
    s = s + f(i)
}
"#;
    let expected = (0..40_000i64).map(|i| i + (i & 3)).sum::<i64>();
    check(changing, "s", expected);
    if JIT_TARGET {
        assert_eq!(hot.exits, 0, "the native call left machine code");
    }
}

#[test]
fn a_native_error_in_machine_code_reports_the_interpreter_stack() {
    let source = r#"
use std::bytes
fn read_at(buf: int, i: int) -> int { return bytes::read_u8(buf, i) }
fn sum(buf: int, n: int) -> int {
    let mut s = 0
    for i in 0..n { s = s + read_at(buf, i & 15) }
    s = s + read_at(buf, 99)
    return s
}
let buf = bytes::alloc(16)
println("{sum(buf, 50000)}")
"#;
    let outcome = parity(source);
    assert_ne!(outcome.code, Some(0));
    assert!(
        outcome.stderr.contains("read_u8"),
        "missing native error: {}",
        outcome.stderr
    );
}

#[test]
fn natives_that_collect_keep_machine_code_references_alive() {
    let source = r#"
use std::string
struct Box { label: string, n: int }
fn run(n: int) -> int {
    let keep: Vec<int> = vec![5, 6, 7]
    let boxed = Box { label: "kept", n: 3 }
    let mut s = 0
    for i in 0..n {
        let text = i.to_string()
        s = (s + string::len(text) + keep[1] + boxed.n + string::len(boxed.label)) & 0xFFFFFFF
    }
    return s
}
println("{run(120000)}")
"#;
    let expected = (0..120_000i64)
        .map(|i| i.to_string().len() as i64 + 6 + 3 + 4)
        .fold(0i64, |s, x| (s + x) & 0xFFF_FFFF);
    let outcome = parity_with(source, &["-ae.max-heap=4M"]);
    assert_eq!(outcome.code, Some(0), "{outcome:?}");
    assert_eq!(first_line(&outcome), expected.to_string());
}

#[test]
fn a_call_before_an_unsupported_operation_no_longer_blocks_compilation() {
    let body = r#"
struct Cell { a: int }
fn triple(i: int) -> int { return i * 3 }
let mut op = triple
fn f(n: int) -> int {
    let mut acc = 0
    for i in 0..n { acc = (acc + op(i)) & 0xFFFFFF }
    let cell = Cell { a: acc }
    return cell.a
}
"#;
    let expected = (0..300_000i64).fold(0i64, |acc, i| (acc + i * 3) & 0xFF_FFFF);
    let hot = check(body, "f(300000)", expected);
    if JIT_TARGET {
        assert!(
            hot.entries >= 2,
            "the loop calling triple was not compiled: {} entries",
            hot.entries
        );
    }
}

#[test]
fn module_globals_written_by_machine_code_across_modules_stay_coherent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    std::fs::write(
        directory.path().join("tally.aelys"),
        r#"
pub let mut hits = 0
pub let mut last = 0
pub fn hit(x: int) -> int {
    hits = hits + x
    last = x
    return hits
}
pub fn total() -> int { return hits + last }
"#,
    )
    .expect("write module");
    let main = directory.path().join("main.aelys");
    std::fs::write(
        &main,
        r#"
use tally
let mut mine = 0
fn drive(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = (s + tally::hit(i & 7)) & 0xFFFFFF
        mine = mine + 1
    }
    return s
}
let s = drive(100000)
println("{s} {tally::total()} {mine}")
"#,
    )
    .expect("write main");
    let interpreted = run_path(&main, false, &[]);
    let compiled = run_path(&main, true, &[]);
    assert_eq!(interpreted, compiled);
    let mut hits = 0i64;
    let mut s = 0i64;
    for i in 0..100_000i64 {
        hits += i & 7;
        s = (s + hits) & 0xFF_FFFF;
    }
    assert_eq!(
        first_line(&compiled),
        format!("{s} {} 100000", hits + (99_999 & 7))
    );
}

#[test]
fn code_hot_enough_for_the_optimizing_tier_keeps_its_results() {
    let body = r#"
let mut calls = 0
fn mix(a: int, b: int) -> int { return (a * 31 + b) & 0xFFFFF }
fn row(n: int, seed: int) -> int {
    calls = calls + 1
    let mut acc = seed
    for i in 0..n { acc = mix(acc, i) }
    return acc
}
fn drive(rows: int) -> int {
    let mut total = 0
    for r in 0..rows { total = (total + row(2000, r)) & 0xFFFFFFF }
    return total * 10000 + calls % 10000
}
"#;
    let mix = |a: i64, b: i64| (a * 31 + b) & 0xF_FFFF;
    let mut total = 0i64;
    for r in 0..2500i64 {
        let mut acc = r;
        for i in 0..2000i64 {
            acc = mix(acc, i);
        }
        total = (total + acc) & 0xFFF_FFFF;
    }
    check(body, "drive(2500)", total * 10000 + 2500);
}

#[test]
fn a_global_written_by_machine_code_is_seen_by_the_interpreted_script() {
    let source = r#"
struct Box { n: int }
let mut hits = 0
fn gw(x: int) -> int {
    if x % 7 == 3 { hits = hits + 1 }
    return x + hits
}
fn step(x: int) -> int {
    let mut acc = 0
    for i in 0..5 { acc = (acc + gw(x + i)) & 0xFFFFF }
    return acc
}
let mut total = 0
let mut seen = 0
for it in 0..4000 {
    let b = Box { n: it }
    total = (total + step(b.n)) & 0xFFFFFF
    seen = seen + hits
}
println("{hits} {total} {seen}")
"#;
    let (mut hits, mut total, mut seen) = (0i64, 0i64, 0i64);
    for it in 0..4000i64 {
        let mut acc = 0i64;
        for i in 0..5 {
            let x = it + i;
            if x % 7 == 3 {
                hits += 1;
            }
            acc = (acc + x + hits) & 0xF_FFFF;
        }
        total = (total + acc) & 0xFF_FFFF;
        seen += hits;
    }
    let outcome = parity(source);
    assert_eq!(first_line(&outcome), format!("{hits} {total} {seen}"));
}
