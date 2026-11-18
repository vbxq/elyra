use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
    new_vm_with_config, run_with_vm_and_opt,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use aelys_runtime::{FrameRequest, FrameSubmission, FrameTarget, JitExecutor, VmConfig};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const OVERFLOW: &str = "integer overflow outside the int range (i64)";
const INLINE_MIN: i64 = -(1 << 47);
const INLINE_MAX: i64 = (1 << 47) - 1;

fn general_loop() -> RunOptions {
    RunOptions {
        max_instructions: Some(1_000_000_000_000),
        ..RunOptions::default()
    }
}

fn modes() -> [(&'static str, JitMode, RunOptions); 4] {
    [
        ("hot loop", JitMode::Off, RunOptions::default()),
        ("general loop", JitMode::Off, general_loop()),
        ("jit", JitMode::Tiered, RunOptions::default()),
        ("jit, general loop", JitMode::Tiered, general_loop()),
    ]
}

fn run(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
    options: RunOptions,
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
        .execute(&module, options)
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => isolate
            .structured_clone(value)
            .map_err(|error| format!("{error:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn error_line(message: &str) -> Option<u32> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse().ok())
}

fn assert_int_everywhere(source: &str, expected: i64) {
    for level in LEVELS {
        for (name, jit, options) in modes() {
            match run(source, level, jit, options) {
                Ok(StructuredValue::Int(got)) => {
                    assert_eq!(got, expected, "{level:?} {name}:\n{source}");
                }
                other => panic!("{level:?} {name}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn assert_error_everywhere(source: &str, needle: &str, line: u32) {
    for level in LEVELS {
        for (name, jit, options) in modes() {
            match run(source, level, jit, options) {
                Ok(value) => panic!("{level:?} {name}: expected an error, got {value:?}\n{source}"),
                Err(text) => {
                    assert!(text.contains(needle), "{level:?} {name}: {text}");
                    assert!(!text.contains("[E0"), "a compile error:\n{text}");
                    assert_eq!(error_line(&text), Some(line), "{level:?} {name}: {text}");
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

fn gc_configs() -> Vec<(&'static str, VmConfig)> {
    vec![
        (
            "stress",
            VmConfig {
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..VmConfig::default()
            },
        ),
        (
            "verify",
            VmConfig {
                gc_verify: true,
                gc_nursery_bytes: 4 * 1024,
                ..VmConfig::default()
            },
        ),
        (
            "verify, stress",
            VmConfig {
                gc_verify: true,
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..VmConfig::default()
            },
        ),
        ("default", VmConfig::default()),
    ]
}

fn assert_int_under_gc_pressure(source: &str, expected: i64) {
    for level in LEVELS {
        for (name, config) in gc_configs() {
            let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
            let value = run_with_vm_and_opt(&mut vm, source, "<r7>", level)
                .unwrap_or_else(|error| panic!("{level:?} {name}: {error}"));
            assert_eq!(value.as_int(), Some(expected), "{level:?} {name}");
            if name != "default" {
                assert!(
                    vm.heap().minor_collection_count() > 20,
                    "{level:?} {name}: only {} minor collections",
                    vm.heap().minor_collection_count()
                );
            }
        }
    }
}

// exits of the fast interpreter loop over a whole run, with the default vm
fn hot_exits(source: &str) -> (i64, u64) {
    let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
    let value = run_with_vm_and_opt(&mut vm, source, "<r7>", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    (
        value.as_int().expect("an int result"),
        vm.execution_stats().hot_exits,
    )
}

fn fixed_int(op: &str, a: i64, b: i64, shift: i64) -> Option<i64> {
    let shift = (shift & 63) as u32;
    let exact = match op {
        "mul_shr" => (i128::from(a) * i128::from(b)) >> shift,
        _ => (i128::from(a) << shift) / i128::from(b),
    };
    i64::try_from(exact).ok()
}

fn fixed_i32(op: &str, a: i64, b: i64, shift: i64) -> i64 {
    let shift = (shift & 63) as u32;
    let exact = match op {
        "mul_shr" => (i128::from(a) * i128::from(b)) >> shift,
        _ => (i128::from(a) << shift) / i128::from(b),
    };
    exact as i64 as i32 as i64
}

#[test]
fn negation_in_hot_code_keeps_int_overflow_and_float_semantics() {
    let n = 3000i64;
    let mut acc = 0i64;
    for i in 0..n {
        let v = i * 7919 - 5_000_000;
        acc = (acc + (-v) % 100_003) % 1_000_000_007;
    }
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let v = i * 7919 - 5000000
        acc = (acc + (-v) % 100003) % 1000000007
    }
    acc
}
run(3000)
",
        acc,
    );
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut flags = 0
    let mut x = 0.0
    let mut nan = 0.0
    for i in 0..n {
        x = 0.0 + (i as float) * 0.0
        nan = x / x
        let z = -x
        let m = -nan
        let back = -(-(i as float) * 2.5)
        if 1.0 / z < 0.0 { flags = flags + 1 }
        if m != m { flags = flags + 1000 }
        if back == (i as float) * 2.5 { flags = flags + 1000000 }
    }
    flags
}
run(40)
",
        40 + 40_000 + 40_000_000,
    );
    assert_error_everywhere(
        "fn run(m: int) -> int {
    let mut s = 0
    for i in 0..10 {
        s = s - i
        let q = -(m + 45 + s)
        s = s + q - q
    }
    s
}
run(-9223372036854775808)
",
        OVERFLOW,
        5,
    );
    assert_int_everywhere(
        "fn run(m: int) -> int {
    let mut s = 0
    for i in 0..10 {
        s = s - i
        let q = -(m + 46 + s)
        s = s + q - q
    }
    s - (m + 1)
}
run(-9223372036854775808)
",
        -45 - (i64::MIN + 1),
    );
    let mut acc = 0i64;
    for i in 0..64i64 {
        let v = INLINE_MIN - i;
        acc = (acc + (-v) % 1_000_000_007 + (-(INLINE_MAX + i)) % 1000) % 1_000_000_007;
    }
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let v = -140737488355328 - i
        let w = 140737488355327 + i
        acc = (acc + (-v) % 1000000007 + (-w) % 1000) % 1000000007
    }
    acc
}
run(64)
",
        acc,
    );
}

#[test]
fn mul_shr_and_shl_div_in_hot_code_match_their_i128_definition() {
    let (n, sh) = (2000i64, 5i64);
    let mut acc = 0i64;
    for i in 0..n {
        let a = i * 977 - 50_000;
        let b = (i % 13) * 4099 + 1;
        let terms = fixed_int("mul_shr", a, b, 16).unwrap()
            + fixed_int("shl_div", a, b, 8).unwrap()
            + fixed_int("mul_shr", a, b, sh).unwrap()
            + fixed_int("shl_div", a, b, sh).unwrap();
        acc = (acc + terms) % 1_000_000_007;
    }
    assert_int_everywhere(
        "fn run(n: int, sh: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let a = i * 977 - 50000
        let b = (i % 13) * 4099 + 1
        acc = (acc + a.mul_shr(b, 16) + a.shl_div(b, 8) + a.mul_shr(b, sh) + a.shl_div(b, sh)) % 1000000007
    }
    acc
}
run(2000, 5)
",
        acc,
    );

    let mut w = 12345i64;
    for _ in 0..500 {
        w = (fixed_i32("mul_shr", w, 70001, 3) + fixed_i32("shl_div", w, 7, 5)) as i32 as i64;
    }
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut w: i32 = 12345
    for i in 0..n {
        w = w.mul_shr(70001, 3) + w.shl_div(7, 5)
    }
    w as int
}
run(500)
",
        w,
    );

    assert_error_everywhere(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let d = 5 - i
        acc = acc + 1000.shl_div(d, 4)
    }
    acc
}
run(10)
",
        "division by zero",
        5,
    );
    assert_error_everywhere(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let a = 1099511627776 + i
        acc = acc + (a - i).mul_shr(a - 1099511627776 + 1073741824, 0) % 7
    }
    acc
}
run(10)
",
        OVERFLOW,
        5,
    );
    let mut acc = 0i64;
    for i in 0..300i64 {
        let a = (1i64 << 40) + i * 977;
        acc = (acc
            + fixed_int("mul_shr", a, 1 << 20, 12).unwrap() % 1_000_003
            + fixed_int("shl_div", a, 3 + i, 9).unwrap() % 1_000_003)
            % 1_000_000_007;
    }
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let a = 1099511627776 + i * 977
        acc = (acc + a.mul_shr(1048576, 12) % 1000003 + a.shl_div(3 + i, 9) % 1000003) % 1000000007
    }
    acc
}
run(300)
",
        acc,
    );
}

#[test]
fn global_writes_from_hot_code_reach_every_reader() {
    let n = 3000i64;
    let g: i64 = (0..n).sum();
    let expected = g * 1000 + format!("n{}", n - 1).len() as i64 + g % 7;
    let source = "let mut g = 0
let mut name = \"start\"
fn bump(n: int) {
    for i in 0..n {
        g = g + i
        name = \"n{i}\"
    }
}
fn read() -> int { g }
bump(3000)
read() * 1000 + name.len() + g % 7
";
    assert_int_everywhere(source, expected);
    assert_int_under_gc_pressure(source, expected);

    let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
    let value =
        run_with_vm_and_opt(&mut vm, source, "<r7>", OptimizationLevel::Standard).expect("runs");
    assert_eq!(value.as_int(), Some(expected));
    assert_eq!(vm.get_global("g").and_then(|value| value.as_int()), Some(g));
    assert!(vm.global_names().iter().any(|name| name == "name"));
}

#[test]
fn a_global_written_in_hot_code_before_its_initialiser_reads_as_uninitialised() {
    assert_error_everywhere(
        "fn set(n: int) {
    for i in 0..n {
        later = later + i
    }
}
set(3)
let mut later = 5
later
",
        "global `later` read before its initialiser ran",
        3,
    );
    assert_int_everywhere(
        "fn set(n: int) {
    for i in 0..n {
        later = i
    }
}
set(3)
let mut later = 5
later
",
        5,
    );
}

#[test]
fn imported_globals_written_in_hot_code_on_both_sides_stay_live() {
    let dir = TempDir::new().expect("temp dir");
    write(
        &dir,
        "cnt.aelys",
        "pub let mut counter = 0
pub let mut label = \"none\"
pub fn bump(n: int) {
    for i in 0..n {
        counter = counter + 1
        label = \"l{i}\"
    }
}
pub fn peek() -> int { counter }
",
    );
    let main = write(
        &dir,
        "main.aelys",
        "use cnt
fn go() -> int {
    let mut s = 0
    for k in 0..12 {
        cnt::bump(k)
        s = s * 3 + cnt::peek() + counter
        for j in 0..3 {
            counter = counter + j
        }
    }
    s * 100 + label.len()
}
go()
",
    );
    let mut s = 0i64;
    let mut counter = 0i64;
    let mut label = String::from("none");
    for k in 0..12 {
        counter += k;
        if k > 0 {
            label = format!("l{}", k - 1);
        }
        s = s * 3 + 2 * counter;
        counter += 3;
    }
    let value = run_file(&main).expect("runs");
    assert_eq!(value.as_int(), Some(s * 100 + label.len() as i64));
}

#[test]
fn a_plain_call_into_a_reused_register_runs_in_hot_code() {
    let leaf = |x: i64| -> i64 { x.abs() * 2 + 1 };
    let probe = |name: i64, n: i64| -> i64 {
        let k = leaf(name);
        let mut s = leaf(k);
        let mut t = 0;
        while t < n {
            t += s % 7 + 1;
            s += 1;
        }
        s + t
    };
    let mut acc = 0i64;
    for i in 0..2000 {
        acc = (acc + probe(i - 1000, 3)) % 1_000_000_007;
    }
    let source = "fn leaf(x: int) -> int {
    if x < 0 { return leaf(0 - x) }
    x * 2 + 1
}
fn probe(name: int, n: int) -> int {
    let k = leaf(name)
    let mut s = leaf(k)
    let mut t = 0
    while t < n {
        t = t + s % 7 + 1
        s = s + 1
    }
    s + t
}
fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        acc = (acc + probe(i - 1000, 3)) % 1000000007
    }
    acc
}
run(2000)
";
    assert_int_everywhere(source, acc);
    let (value, exits) = hot_exits(source);
    assert_eq!(value, acc);
    assert!(exits < 100, "{exits} exits for 2000 iterations");
}

#[test]
fn vec_and_array_element_access_in_hot_code_keeps_bounds_and_values() {
    let n = 4000i64;
    let mut v = vec![0i64; 257];
    let mut arr = [0i64; 64];
    let mut acc = 0i64;
    for i in 0..n {
        let k = ((i * 37) % 257) as usize;
        v[k] = v[k] * 3 + i - 2000;
        let j = (i % 64) as usize;
        arr[j] += v[k] % 1000;
        acc = (acc + v[k] % 1_000_003 + arr[j]) % 1_000_000_007;
    }
    assert_int_everywhere(
        "fn run(n: int) -> int {
    let mut v: Vec<int> = vec![0; 257]
    let mut arr = [0; 64]
    let mut acc = 0
    for i in 0..n {
        let k = (i * 37) % 257
        v[k] = v[k] * 3 + i - 2000
        let j = i % 64
        arr[j] = arr[j] + v[k] % 1000
        acc = (acc + v[k] % 1000003 + arr[j]) % 1000000007
    }
    acc
}
run(4000)
",
        acc,
    );
    for body in [
        "        acc = acc + v[i - 3]\n",
        "        v[10 - i] = i\n",
        "        acc = acc + v[i + 3]\n",
    ] {
        let source = format!(
            "fn run(n: int) -> int {{
    let mut v: Vec<int> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    let mut acc = 0
    for i in 0..n {{
{body}    }}
    acc + v[0]
}}
run(20)
"
        );
        assert_error_everywhere(&source, "index", 5);
    }
}

#[test]
fn natives_that_succeed_with_unit_return_one_shared_ok_and_copy_without_a_temporary() {
    let source = "use std::bytes
let buf = bytes::alloc(64)
let mut frame: Vec<int> = vec![0; 64]
let mut oks = 0
let mut sum = 0
let mut tags = 0
for round in 0..300 {
TAG    for i in 0..64 { frame[i] = (i * 7 + round) % 256 }
    let first = bytes::copy_from_vec(buf, 0, frame)
    let again = bytes::copy_from_vec(buf, 0, frame)
    if first == again { oks = oks + 1 }
    match first {
        Ok(_) => { oks = oks + 1 }
        Err(e) => { oks = oks - 1000 }
    }
    sum = (sum + bytes::read_u8(buf, round % 64)) % 1000000007
}
let bad = bytes::copy_from_vec(buf, 60, frame)
let high = bytes::copy_from_vec(buf, 0, vec![1, 300])
let errs = match bad { Ok(_) => 0, Err(e) => 1 } + match high { Ok(_) => 0, Err(e) => e.len() }
oks * 1000000 + sum * 100 + errs + tags * 0
";
    let mut sum = 0i64;
    for round in 0..300i64 {
        let i = round % 64;
        sum = (sum + (i * 7 + round) % 256) % 1_000_000_007;
    }
    let high = "copy_from_vec: element 1 is 300, not a byte in 0..=255";
    let expected = 600 * 1_000_000 + sum * 100 + 1 + high.len() as i64;
    let tagged = source.replace(
        "TAG",
        "    let junk: Vec<int> = vec![round; 64]\n    tags = tags + junk[3]\n",
    );
    assert_int_under_gc_pressure(&tagged, expected);

    let source = source.replace("TAG", "");
    let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
    let before = vm.heap().allocation_count();
    let value =
        run_with_vm_and_opt(&mut vm, &source, "<r7>", OptimizationLevel::Standard).expect("runs");
    assert_eq!(value.as_int(), Some(expected));
    let allocated = vm.heap().allocation_count() - before;
    assert!(
        allocated < 300,
        "{allocated} objects for 600 successful copies"
    );
}

#[test]
fn neg_fixed_point_global_stores_and_plain_calls_stay_in_the_fast_loop() {
    let neg = "fn run(n: int) -> int {
    let mut t = 0
    for i in 0..n { t = t + -i }
    t
}
run(20000)
";
    let (value, exits) = hot_exits(neg);
    assert_eq!(value, -(0..20000i64).sum::<i64>());
    assert!(exits < 50, "neg: {exits} exits");

    let fixed = "fn run(n: int) -> int {
    let mut t = 0
    for i in 0..n { t = t + i.mul_shr(3, 4) + (i + 1).shl_div(5, 2) }
    t
}
run(20000)
";
    let (value, exits) = hot_exits(fixed);
    let expected: i64 = (0..20000i64)
        .map(|i| {
            fixed_int("mul_shr", i, 3, 4).unwrap() + fixed_int("shl_div", i + 1, 5, 2).unwrap()
        })
        .sum();
    assert_eq!(value, expected);
    assert!(exits < 50, "fixed: {exits} exits");

    let global = "let mut g = 0
fn run(n: int) -> int {
    for i in 0..n { g = g + i }
    g
}
run(20000)
";
    let (value, exits) = hot_exits(global);
    assert_eq!(value, (0..20000i64).sum::<i64>());
    assert!(exits < 50, "global: {exits} exits");

    let stored = "let mut g = 0
fn run(n: int) -> int {
    let mut t = 0
    for i in 0..n {
        g = i * 2
        t = t + g
    }
    t
}
run(20000)
";
    let (value, exits) = hot_exits(stored);
    assert_eq!(value, (0..20000i64).map(|i| i * 2).sum::<i64>());
    assert!(exits < 50, "stored: {exits} exits");
}

#[test]
fn a_global_accumulated_in_hot_code_keeps_its_overflow_and_uninitialised_errors() {
    assert_error_everywhere(
        "let mut g = 9223372036854775000
fn run(n: int) -> int {
    for i in 0..n {
        g = g + i
    }
    g
}
run(100)
",
        OVERFLOW,
        4,
    );
    assert_int_everywhere(
        "let mut g = 9223372036854775000
fn run(n: int) -> int {
    for i in 0..n {
        g = g + i
    }
    g
}
run(26)
",
        9_223_372_036_854_775_000 + (0..26).sum::<i64>(),
    );
    assert_int_everywhere(
        "let mut g = 140737488355000
fn run(n: int) -> int {
    for i in 0..n {
        g = g + i
    }
    g
}
run(100)
",
        140_737_488_355_000 + (0..100).sum::<i64>(),
    );
    assert_error_everywhere(
        "fn run(n: int) -> int {
    for i in 0..n {
        g = g + i
    }
    g
}
let first = run(5)
let mut g = 0
first
",
        "global `g` read before its initialiser ran",
        3,
    );
}

// a compiler that accepts every request and never delivers, so the vm keeps interpreting with the jit active
#[derive(Default)]
struct NeverDelivers {
    calls: AtomicU64,
    loops: AtomicU64,
}

impl JitExecutor for NeverDelivers {
    fn submit_frame(&self, request: FrameRequest<'_>) -> FrameSubmission {
        match request.target {
            FrameTarget::Call => self.calls.fetch_add(1, Ordering::Relaxed),
            FrameTarget::Loop(_) => self.loops.fetch_add(1, Ordering::Relaxed),
        };
        FrameSubmission::Queued
    }

    fn frame_call_threshold(&self) -> u64 {
        100
    }
}

#[test]
fn hot_code_with_the_jit_active_counts_back_edges_and_calls_without_leaving() {
    let source = "fn step(x: int) -> int { (x * 7 + 3) % 1000003 }
fn run(n: int) -> int {
    let mut acc = 1
    let mut i = 0
    while i < n {
        acc = step(acc + i)
        i = i + 1
    }
    for j in 0..n {
        acc = (acc + j) % 1000003
    }
    acc
}
run(30000)
";
    let mut acc = 1i64;
    for i in 0..30000i64 {
        acc = ((acc + i) * 7 + 3) % 1_000_003;
    }
    for j in 0..30000i64 {
        acc = (acc + j) % 1_000_003;
    }
    let jit = Arc::new(NeverDelivers::default());
    let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
    vm.configure_jit(Some(Arc::clone(&jit) as Arc<dyn JitExecutor>));
    let value = run_with_vm_and_opt(&mut vm, source, "<r7>", OptimizationLevel::None)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value.as_int(), Some(acc));
    let exits = vm.execution_stats().hot_exits;
    assert!(
        exits < 200,
        "{exits} exits for 60000 back-edges and 30000 calls"
    );
    assert_eq!(
        jit.calls.load(Ordering::Relaxed),
        1,
        "one call compile requested"
    );
    assert_eq!(jit.loops.load(Ordering::Relaxed), 2, "one compile per loop");
}
