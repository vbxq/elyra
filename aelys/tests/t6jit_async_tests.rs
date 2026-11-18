use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitConfig, JitMode, RunOptions, Runtime, Value,
};
use aelys_runtime::parse_vm_args;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

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

fn run_cli(path: &Path, flags: &[&str]) -> (String, String, Option<i32>) {
    let output = Command::new(cli_binary())
        .arg("run")
        .args(flags)
        .arg(path)
        .output()
        .expect("aelys-cli should run");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

enum Mode {
    Off,
    Sync,
    Async,
}

fn runtime(mode: &Mode) -> Runtime {
    match mode {
        Mode::Off => Runtime::with_jit_mode(JitMode::Off),
        Mode::Sync => Runtime::with_jit_mode(JitMode::Tiered),
        Mode::Async => Runtime::with_jit_config(
            JitMode::Tiered,
            JitConfig::default().with_async_compilation(true),
        )
        .expect("async jit configuration"),
    }
}

fn execute(runtime: &Runtime, source: &str) -> Value {
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("program compiles");
    execute_module(runtime, &module)
}

fn execute_module(runtime: &Runtime, module: &aelys::CompiledModule) -> Value {
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(module, RunOptions::default())
        .expect("program runs")
    {
        ExecutionOutcome::Returned(value) => value,
        outcome => panic!("expected a returned value, got {outcome:?}"),
    }
}

// a value read through a global function, a vec and a global, all replaced after the compile was requested
const SWAPPED_BEFORE_INSTALL: &str = r#"
needs std::time
fn one() -> int { return 1 }
fn two() -> int { return 2 }
let mut pick = one
let mut v: Vec<int> = vec![1, 2, 3]
let mut g = 10
fn work(k: int) -> int {
    let mut acc = 0
    let mut i = 0
    while i < 8 {
        acc = acc + pick() * v[i % 3] + g
        i = i + 1
    }
    return acc + k
}
let mut total = 0
for k in 0..1000 { total = total + work(k) }
pick = two
for j in 0..5000 { v.push(j) }
v[0] = 100
v[1] = 200
v[2] = 300
g = 7
time::sleep_us(300000)
for k in 0..1000 { total = total + work(k) }
total
"#;

#[test]
fn state_changed_between_snapshot_and_install_is_seen_by_the_installed_code() {
    for mode in [Mode::Off, Mode::Sync, Mode::Async] {
        let runtime = runtime(&mode);
        let value = execute(&runtime, SWAPPED_BEFORE_INSTALL);
        assert_eq!(value, Value::int(594_500 + 3_555_500));
        if JIT_TARGET && !matches!(mode, Mode::Off) {
            assert!(runtime.jit_cache_entries() >= 1);
        }
    }
}

#[test]
fn a_module_global_written_between_snapshot_and_install_is_read_live() {
    let directory = tempfile::tempdir().expect("temporary directory");
    std::fs::write(
        directory.path().join("counter.aelys"),
        "pub let mut count = 0\npub fn set(value: int) { count = value }\n",
    )
    .expect("write module");
    let main = directory.path().join("main.aelys");
    std::fs::write(
        &main,
        r#"needs std::time
needs counter
fn work(k: int) -> int {
    let mut acc = 0
    let mut i = 0
    while i < 4 {
        acc = acc + counter::count + i
        i = i + 1
    }
    return acc + k
}
let mut total = 0
for k in 0..1000 { total = total + work(k) }
counter::set(50)
time::sleep_us(300000)
for k in 0..1000 { total = total + work(k) }
println("{total}")
"#,
    )
    .expect("write main");
    for flags in [
        &[][..],
        &["-ae.jit=on"][..],
        &["-ae.jit=on", "-ae.jit-sync=on"][..],
        &["-ae.jit=on", "-ae.gc-stress=on", "-ae.gc-verify=on"][..],
    ] {
        let (stdout, stderr, code) = run_cli(&main, flags);
        assert_eq!(code, Some(0), "{flags:?}: {stderr}");
        assert_eq!(stdout, "1211000\n", "{flags:?}");
    }
}

fn long_loop_reference(n: i64) -> i64 {
    let mut s = 0i64;
    for i in 0..n {
        s = (s * 31 + i) & 0xFF_FFFF;
        if i % 3 == 0 {
            s ^= i & 0xFF;
        }
    }
    s
}

#[test]
fn async_tiering_matches_the_interpreter_on_a_loop_recompiled_while_it_runs() {
    let source = r#"
fn long(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = (s * 31 + i) & 0xFFFFFF
        if i % 3 == 0 { s = s ^ (i & 0xFF) }
    }
    return s
}
fn short(n: int) -> int {
    let mut s = 0
    for i in 0..n { s = (s + i * i) & 0xFFFFFF }
    return s
}
let mut t = 0
for k in 0..12000 { t = (t + short(k & 31)) & 0xFFFFFF }
long(3000000) + t
"#;
    let short = |n: i64| (0..n).fold(0i64, |s, i| (s + i * i) & 0xFF_FFFF);
    let t = (0..12000i64).fold(0i64, |t, k| (t + short(k & 31)) & 0xFF_FFFF);
    let expected = long_loop_reference(3_000_000) + t;
    for mode in [Mode::Off, Mode::Sync, Mode::Async] {
        let runtime = runtime(&mode);
        assert_eq!(execute(&runtime, source), Value::int(expected));
    }
}

fn many_hot_functions(count: usize) -> (String, i64) {
    let mut source = String::new();
    let mut expected = 0i64;
    for index in 0..count {
        source.push_str(&format!(
            "fn f{index}(n: int) -> int {{\n    let mut s = {index}\n    for i in 0..n {{ s = (s * 3 + i + {index}) & 0xFFFF }}\n    return s\n}}\n"
        ));
    }
    source.push_str("let mut total = 0\nfor k in 0..1001 {\n");
    for index in 0..count {
        source.push_str(&format!("    total = (total + f{index}(4)) & 0xFFFFFF\n"));
    }
    source.push_str("}\n");
    for _ in 0..1001 {
        for index in 0..count as i64 {
            let mut s = index;
            for i in 0..4 {
                s = (s * 3 + i + index) & 0xFFFF;
            }
            expected = (expected + s) & 0xFF_FFFF;
        }
    }
    (source, expected)
}

#[test]
fn dropping_the_vm_with_compiles_in_flight_neither_hangs_nor_crashes() {
    let (body, expected) = many_hot_functions(90);
    let source = format!("{body}total\n");
    let runtime = runtime(&Mode::Async);
    assert_eq!(execute(&runtime, &source), Value::int(expected));
    assert_eq!(execute(&runtime, &source), Value::int(expected));
    let started = Instant::now();
    drop(runtime);
    assert!(started.elapsed() < Duration::from_secs(20));

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("many.aelys");
    std::fs::write(&path, format!("{body}println(\"{{total}}\")\n")).expect("write program");
    let (stdout, stderr, code) = run_cli(&path, &["-ae.jit=on"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, format!("{expected}\n"));
}

static INJECTED: AtomicUsize = AtomicUsize::new(0);

// compiles requested near the end of a run finish on the worker after execute returned
fn settled_count() -> usize {
    let mut count = INJECTED.load(Ordering::SeqCst);
    let mut stable = 0;
    while stable < 10 {
        std::thread::sleep(Duration::from_millis(100));
        let now = INJECTED.load(Ordering::SeqCst);
        stable = if now == count { stable + 1 } else { 0 };
        count = now;
    }
    count
}

#[test]
fn a_panic_in_the_compiler_leaves_the_function_interpreted_and_the_program_running() {
    if !JIT_TARGET {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .unwrap_or_default();
        if message == "injected frame compiler fault" {
            INJECTED.fetch_add(1, Ordering::SeqCst);
        } else {
            previous(info);
        }
    }));
    let source = r#"
fn hot(n: int) -> int {
    let mut s = 0
    for i in 0..n { s = (s + i * 7) & 0xFFFFF }
    return s
}
fn other(n: int) -> int { return n * 2 }
let mut t = 0
for k in 0..3000 { t = (t + hot(k & 15) + other(k)) & 0xFFFFFF }
let mut u = 0
for i in 0..30000 { u = (u + i) & 0xFFFFF }
t + hot(40000) + u
"#;
    let hot = |n: i64| (0..n).fold(0i64, |s, i| (s + i * 7) & 0xF_FFFF);
    let t = (0..3000i64).fold(0i64, |t, k| (t + hot(k & 15) + k * 2) & 0xFF_FFFF);
    let u = (0..30000i64).fold(0i64, |u, i| (u + i) & 0xF_FFFF);
    let expected = Value::int(t + hot(40000) + u);
    for asynchronous in [false, true] {
        INJECTED.store(0, Ordering::SeqCst);
        let runtime = Runtime::with_jit_config(
            JitMode::Tiered,
            JitConfig::default()
                .with_async_compilation(asynchronous)
                .with_compile_panic_for("hot"),
        )
        .expect("jit configuration");
        let module = runtime
            .compile(source, CompileOptions::default())
            .expect("program compiles");
        assert_eq!(execute_module(&runtime, &module), expected);
        let first = settled_count();
        assert!(first >= 1, "the injected fault never fired");
        assert_eq!(execute_module(&runtime, &module), expected);
        assert_eq!(
            settled_count(),
            first,
            "a function whose compile panicked was compiled again (async {asynchronous})"
        );
        assert!(
            runtime.jit_cache_entries() >= 1,
            "other code stopped compiling"
        );
    }
}

#[test]
fn the_sync_switch_is_parsed_and_validated() {
    let parsed = parse_vm_args(&["-ae.jit=on".to_string(), "-ae.jit-sync=on".to_string()])
        .expect("valid arguments");
    assert!(parsed.jit);
    assert!(parsed.jit_sync);
    let parsed = parse_vm_args(&["-ae.jit=on".to_string()]).expect("valid arguments");
    assert!(!parsed.jit_sync);
    assert!(parse_vm_args(&["-ae.jit-sync=sometimes".to_string()]).is_err());
}
