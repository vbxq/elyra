use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitConfig, JitMode, RunOptions, Runtime, Value,
};
use aelys_opt::OptimizationLevel;
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

#[derive(Clone, Copy, Debug)]
enum Mode {
    Off,
    Sync,
    Async,
}

fn runtime(mode: Mode) -> Runtime {
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

fn execute(runtime: &Runtime, source: &str, level: OptimizationLevel) -> Value {
    let options = CompileOptions {
        optimization_level: level,
        ..CompileOptions::default()
    };
    let module = runtime.compile(source, options).expect("program compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .expect("program runs")
    {
        ExecutionOutcome::Returned(value) => value,
        outcome => panic!("expected a returned value, got {outcome:?}"),
    }
}

const COUNT: i64 = 30_000;
const TAIL_ROUNDS: i64 = 1_000;

struct Form {
    name: &'static str,
    // {hook} is replaced by a pause that lets a background compile land while the loop still runs
    phase: &'static str,
    tail: &'static str,
    value: i64,
}

// each loop body starts with an instruction that defines no register, so the osr entry reaches the next loop through code that writes nothing
const FORMS: &[Form] = &[
    Form {
        name: "vec repeat",
        phase: "let mut w: Vec<int> = vec![k; n]",
        tail: "w.len() + w[n - 1]",
        value: COUNT + 7,
    },
    Form {
        name: "array repeat",
        phase: "let w = [7; 30000]",
        tail: "w.len() + w[29999]",
        value: COUNT + 7,
    },
    Form {
        name: "range store",
        phase: "let mut w: Vec<int> = vec![0; 8]\n for i in 0..n { w[z] = k\n {hook:i == 10100} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
    Form {
        name: "inclusive store",
        phase: "let mut w: Vec<int> = vec![0; 8]\n let last = n - 1\n for i in 0..=last { w[z] = k\n {hook:i == 10100} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
    Form {
        name: "step store",
        phase: "let mut w: Vec<int> = vec![0; 8]\n for i in 0..n step 2 { w[z] = k\n {hook:i == 20200} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
    Form {
        name: "downward store",
        phase: "let mut w: Vec<int> = vec![0; 8]\n for i in n..0 step -1 { w[z] = k\n {hook:i == 15000} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
    Form {
        name: "vec for-each",
        phase: "let source = src(n)\n let mut w: Vec<int> = vec![0; 8]\n let mut c = 0\n for x in source { w[z] = x\n c = c + 1\n {hook:c == 10100} }",
        tail: "w.len() + w[0] + c",
        value: 15 + COUNT,
    },
    Form {
        name: "array for-each",
        phase: "let source = asrc()\n let mut w: Vec<int> = vec![0; 8]\n let mut c = 0\n for x in source { w[z] = x\n c = c + 1\n {hook:c == 10100} }",
        tail: "w.len() + w[0] + c",
        value: 15 + COUNT,
    },
    Form {
        name: "while store",
        phase: "let mut w: Vec<int> = vec![0; 8]\n let mut i = 0\n while i < n { w[z] = k\n i = i + 1\n {hook:i == 10100} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
    Form {
        name: "struct field store",
        phase: "let mut p = P { x: 0, y: 0 }\n for i in 0..n { p.x = k\n {hook:i == 10100} }",
        tail: "p.x + 8",
        value: 15,
    },
    Form {
        name: "array store",
        phase: "let mut w = [0; 8]\n for i in 0..n { w[z] = k\n {hook:i == 10100} }",
        tail: "w.len() + w[0]",
        value: 15,
    },
];

#[derive(Clone, Copy, Debug)]
enum Context {
    Function,
    Method,
    Closure,
    Generic,
}

const CONTEXTS: [Context; 4] = [
    Context::Function,
    Context::Method,
    Context::Closure,
    Context::Generic,
];

fn with_hooks(phase: &str, pause: bool) -> String {
    let mut out = String::new();
    let mut rest = phase;
    while let Some(start) = rest.find("{hook:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 6..];
        let end = after.find('}').expect("hook end");
        if pause {
            out.push_str(&format!(
                "if {} {{ time::sleep_us(1000000) }}",
                &after[..end]
            ));
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn program(form: &Form, context: Context, pause: bool) -> String {
    let body = format!(
        "    {}\n    let mut s = 0\n    for i in 0..m {{\n        let f = i as float\n        s = s + (f * 2.0) as int + i\n    }}\n    return s * 1000000 + {}",
        with_hooks(form.phase, pause),
        form.tail
    );
    let head = "use std::time\nstruct P { x: int, y: int }\nfn src(n: int) -> Vec<int> { vec![7; n] }\nfn asrc() -> [int; 30000] { [7; 30000] }\n";
    let args = format!("{COUNT}, {TAIL_ROUNDS}, 7, 0");
    match context {
        Context::Function => format!(
            "{head}fn run(n: int, m: int, k: int, z: int) -> int {{\n{body}\n}}\nrun({args})\n"
        ),
        Context::Method => format!(
            "{head}struct R {{ q: int }}\nimpl R {{\n fn run(self, n: int, m: int, k: int, z: int) -> int {{\n{body}\n}}\n}}\nlet r = R {{ q: 0 }}\nr.run({args})\n"
        ),
        Context::Closure => format!(
            "{head}let run = fn(n: int, m: int, k: int, z: int) -> int {{\n{body}\n}}\nrun({args})\n"
        ),
        Context::Generic => format!(
            "{head}fn run<T>(tag: T, n: int, m: int, k: int, z: int) -> int {{\n{body}\n}}\nrun(true, {args})\n"
        ),
    }
}

fn expected(form: &Form) -> i64 {
    let tail: i64 = (0..TAIL_ROUNDS).map(|i| 3 * i).sum();
    tail * 1_000_000 + form.value
}

// returns how many runs had compiled code by the end; a background compile may still be queued on a loaded machine
fn check_forms(
    mode: Mode,
    levels: &[OptimizationLevel],
    contexts: &[Context],
    pause: bool,
) -> usize {
    let mut compiled = 0;
    for form in FORMS {
        for &context in contexts {
            let source = program(form, context, pause);
            for level in levels {
                let runtime = runtime(mode);
                let value = execute(&runtime, &source, *level);
                assert_eq!(
                    value,
                    Value::int(expected(form)),
                    "{} in {context:?}, {mode:?}, {level:?}",
                    form.name
                );
                if runtime.jit_cache_entries() >= 1 {
                    compiled += 1;
                } else if JIT_TARGET && matches!(mode, Mode::Sync) {
                    panic!("{} in {context:?}, {level:?}: nothing compiled", form.name);
                }
            }
        }
    }
    compiled
}

#[test]
fn interpreter_reference_for_the_osr_forms() {
    check_forms(Mode::Off, &[OptimizationLevel::None], &CONTEXTS, false);
}

#[test]
fn osr_entry_through_a_body_that_writes_nothing_keeps_the_next_loop_registers_sync() {
    check_forms(
        Mode::Sync,
        &[OptimizationLevel::None, OptimizationLevel::Standard],
        &CONTEXTS,
        false,
    );
}

#[test]
fn osr_entry_through_a_body_that_writes_nothing_keeps_the_next_loop_registers_async() {
    let compiled = check_forms(
        Mode::Async,
        &[OptimizationLevel::Standard],
        &[Context::Function, Context::Method],
        true,
    );
    if JIT_TARGET {
        assert!(compiled >= 1, "no background compile landed");
    }
}

fn array_fill_reference(rounds: i64) -> i64 {
    let n = 1_000_000i64;
    let mut v = vec![0i64; n as usize];
    let mut total = 0i64;
    for r in 0..rounds {
        for i in 0..n {
            v[i as usize] = (i * (r + 3)) & 0xFFFF;
        }
        let s: i64 = v.iter().sum();
        total = (total + s) & 0xF_FFFF_FFFF;
    }
    total
}

const ARRAY_FILL: &str = r#"fn run(rounds: int) -> int {
    let n = 1000000
    let mut v: Vec<int> = vec![0; n]
    let mut total = 0
    for r in 0..rounds {
        for i in 0..n {
            v[i] = (i * (r + 3)) & 0xFFFF
        }
        let mut s = 0
        for i in 0..n {
            s = s + v[i]
        }
        total = (total + s) & 0xFFFFFFFFF
    }
    return total
}
println("checksum={run(3)}")
"#;

const FINESINE: &str = r#"use std::math as math
fn make() -> Vec<int> {
    let mut t: Vec<int> = vec![0; 10240]
    for i in 0..10240 {
        let a = (i as float + 0.5) * 2.0 * math::PI / 8192.0
        t[i] = (math::sin(a) * 65536.0) as int
    }
    t
}
let t = make()
println("{t[100] + t[10239]}")
"#;

fn finesine_reference() -> i64 {
    let entry = |i: i64| {
        let a = (i as f64 + 0.5) * 2.0 * std::f64::consts::PI / 8192.0;
        (a.sin() * 65536.0) as i64
    };
    entry(100) + entry(10239)
}

const CLI_MODES: &[&[&str]] = &[
    &[],
    &["-ae.jit=on"],
    &["-ae.jit=on", "-ae.jit-sync=on"],
    &["-O0", "-ae.jit=on"],
    &["-O0", "-ae.jit=on", "-ae.jit-sync=on"],
];

#[test]
fn array_fill_checksum_survives_the_jit() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("array_fill.aelys");
    std::fs::write(&path, ARRAY_FILL).expect("write program");
    let want = format!("checksum={}\n", array_fill_reference(3));
    for flags in CLI_MODES {
        let (stdout, stderr, code) = run_cli(&path, flags);
        assert_eq!(code, Some(0), "{flags:?}: {stderr}");
        assert_eq!(stdout, want, "{flags:?}");
    }
}

#[test]
fn compiled_for_loop_stops_at_its_end_after_an_osr_entry() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("finesine.aelys");
    std::fs::write(&path, FINESINE).expect("write program");
    let want = format!("{}\n", finesine_reference());
    let mut modes = CLI_MODES.to_vec();
    modes.push(&[
        "-ae.jit=on",
        "-ae.jit-sync=on",
        "-ae.gc-stress=on",
        "-ae.gc-verify=on",
    ]);
    for flags in modes {
        let (stdout, stderr, code) = run_cli(&path, flags);
        assert_eq!(code, Some(0), "{flags:?}: {stderr}");
        assert_eq!(stdout, want, "{flags:?}");
    }
}

#[test]
fn osr_forms_under_gc_stress_and_verify() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for form in FORMS
        .iter()
        .filter(|form| matches!(form.name, "vec repeat" | "vec for-each" | "array store"))
    {
        let source = program(form, Context::Function, false)
            .replace(&format!("run({COUNT}"), &format!("println(\"{{run({COUNT}"))
            .replace(", 7, 0)\n", ", 7, 0)}\")\n");
        let path = directory.path().join("form.aelys");
        std::fs::write(&path, source).expect("write program");
        let (stdout, stderr, code) = run_cli(
            &path,
            &[
                "-ae.jit=on",
                "-ae.jit-sync=on",
                "-ae.gc-stress=on",
                "-ae.gc-verify=on",
            ],
        );
        assert_eq!(code, Some(0), "{}: {stderr}", form.name);
        assert_eq!(stdout, format!("{}\n", expected(form)), "{}", form.name);
    }
}

struct Reuse {
    kind: &'static str,
    param: &'static str,
    read: &'static str,
    argument: &'static str,
    element: i64,
}

// the first loop reads a local that holds a reference, and the second loop takes that local's register for its counter
const REUSES: &[Reuse] = &[
    Reuse {
        kind: "vec",
        param: "Vec<int>",
        read: "a[i & 63]",
        argument: "vec![3; 64]",
        element: 3,
    },
    Reuse {
        kind: "array",
        param: "[int; 64]",
        read: "a[i & 63]",
        argument: "[3; 64]",
        element: 3,
    },
    Reuse {
        kind: "struct",
        param: "P",
        read: "a.x",
        argument: "P { x: 3, y: 4 }",
        element: 3,
    },
    Reuse {
        kind: "string",
        param: "string",
        read: "a.len()",
        argument: "\"abc\"",
        element: 3,
    },
];

fn reuse_program(reuse: &Reuse, method: bool) -> String {
    let body = format!(
        "    let mut s = 0\n    {{\n        let a = v\n        for i in 0..n {{ s = (s + {}) & 0xFFFFFF }}\n    }}\n    for j in 0..n {{ s = (s ^ w[j & 63]) + 1 }}\n    return s",
        reuse.read
    );
    let signature = format!("n: int, v: {}, w: Vec<int>", reuse.param);
    let head = "struct P { x: int, y: int }\nlet w: Vec<int> = vec![5; 64]\n";
    if method {
        format!(
            "{head}struct R {{ q: int }}\nimpl R {{\n fn work(self, {signature}) -> int {{\n{body}\n }}\n}}\nlet r = R {{ q: 0 }}\nr.work(200000, {}, w)\n",
            reuse.argument
        )
    } else {
        format!(
            "{head}fn work({signature}) -> int {{\n{body}\n}}\nwork(200000, {}, w)\n",
            reuse.argument
        )
    }
}

fn reuse_reference(element: i64) -> i64 {
    let n = 200_000i64;
    let mut s = 0i64;
    for _ in 0..n {
        s = (s + element) & 0xFF_FFFF;
    }
    for _ in 0..n {
        s = (s ^ 5) + 1;
    }
    s
}

#[test]
fn a_counter_register_holding_a_reference_at_an_earlier_loop_does_not_refuse_that_loop() {
    for reuse in REUSES {
        for method in [false, true] {
            let source = reuse_program(reuse, method);
            for mode in [Mode::Off, Mode::Sync] {
                let runtime = runtime(mode);
                let value = execute(&runtime, &source, OptimizationLevel::Standard);
                assert_eq!(
                    value,
                    Value::int(reuse_reference(reuse.element)),
                    "{} method={method} {mode:?}",
                    reuse.kind
                );
                if JIT_TARGET && matches!(mode, Mode::Sync) {
                    assert!(
                        runtime.jit_osr_executions() >= 1,
                        "{} method={method}: the compiled loops never returned",
                        reuse.kind
                    );
                    assert!(
                        runtime.jit_deoptimizations() < 4,
                        "{} method={method}: {} exits",
                        reuse.kind,
                        runtime.jit_deoptimizations()
                    );
                }
            }
        }
    }
}
