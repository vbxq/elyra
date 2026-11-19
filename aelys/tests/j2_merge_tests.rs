use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitConfig, JitMode, RunOptions, Runtime, Value,
};
use aelys_opt::OptimizationLevel;

const JIT_TARGET: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));
const MASK: i128 = 0xFF_FFFF_FFFF;

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

struct IntType {
    name: &'static str,
    min: i128,
    max: i128,
    span: i128,
    rounds: i128,
}

const TYPES: &[IntType] = &[
    IntType {
        name: "int",
        min: i64::MIN as i128,
        max: i64::MAX as i128,
        span: 30,
        rounds: 120,
    },
    IntType {
        name: "i64",
        min: i64::MIN as i128,
        max: i64::MAX as i128,
        span: 30,
        rounds: 120,
    },
    IntType {
        name: "u64",
        min: 0,
        max: u64::MAX as i128,
        span: 30,
        rounds: 120,
    },
    IntType {
        name: "i32",
        min: i32::MIN as i128,
        max: i32::MAX as i128,
        span: 1500,
        rounds: 12,
    },
    IntType {
        name: "u32",
        min: 0,
        max: u32::MAX as i128,
        span: 1500,
        rounds: 12,
    },
    IntType {
        name: "i16",
        min: i16::MIN as i128,
        max: i16::MAX as i128,
        span: 1500,
        rounds: 12,
    },
    IntType {
        name: "u16",
        min: 0,
        max: u16::MAX as i128,
        span: 1500,
        rounds: 12,
    },
    IntType {
        name: "i8",
        min: i8::MIN as i128,
        max: i8::MAX as i128,
        span: 100,
        rounds: 150,
    },
    IntType {
        name: "u8",
        min: 0,
        max: u8::MAX as i128,
        span: 100,
        rounds: 150,
    },
];

fn literal(value: i128) -> String {
    if value == i64::MIN as i128 {
        "-9223372036854775807 - 1".to_string()
    } else {
        value.to_string()
    }
}

fn forms(signed: bool) -> Vec<(&'static str, char, char, i128, bool)> {
    let mut forms = vec![
        ("for i in a..=b", 'a', 'b', 1, true),
        ("for i in a..b", 'a', 'b', 1, false),
        ("for i in a..=b step 3", 'a', 'b', 3, true),
        ("for i in a..b step k", 'a', 'b', 2, false),
    ];
    if signed {
        forms.extend([
            ("for i in b..=a step -1", 'b', 'a', -1, true),
            ("for i in b..a step -2", 'b', 'a', -2, false),
            ("for i in b..=a step nk", 'b', 'a', -2, true),
        ]);
    }
    forms
}

fn ends_program(ty: &IntType, a: i128, b: i128) -> String {
    let signed = ty.min < 0;
    let mut body = String::new();
    for (header, ..) in forms(signed) {
        body.push_str(&format!(
            "        {header} {{\n            s = (s * 3 + ((i % 7) as int) + 7) & {MASK}\n            c += 1\n        }}\n"
        ));
    }
    let name = ty.name;
    let negated = if signed { ", nk: T" } else { "" };
    let negated_argument = if signed { ", -2" } else { "" };
    let signature = format!("rounds: int, a: T, b: T, k: T{negated}").replace('T', name);
    format!(
        "fn run({signature}) -> int {{\n    let mut s = 0\n    for r in 0..rounds {{\n        let mut c = 0\n{body}        s = (s + c) & {MASK}\n    }}\n    return s\n}}\nrun({}, {}, {}, 2{negated_argument})\n",
        ty.rounds,
        literal(a),
        literal(b),
    )
}

fn ends_reference(ty: &IntType, a: i128, b: i128) -> i64 {
    let signed = ty.min < 0;
    let mut s: i128 = 0;
    for _ in 0..ty.rounds {
        let mut c: i128 = 0;
        for (_, start, end, step, inclusive) in forms(signed) {
            let start = if start == 'a' { a } else { b };
            let end = if end == 'a' { a } else { b };
            let mut i = start;
            loop {
                let inside = match (step > 0, inclusive) {
                    (true, true) => i <= end,
                    (true, false) => i < end,
                    (false, true) => i >= end,
                    (false, false) => i > end,
                };
                if !inside || i < ty.min || i > ty.max {
                    break;
                }
                s = (s * 3 + (i % 7) + 7) & MASK;
                c += 1;
                i += step;
            }
        }
        s = (s + c) & MASK;
    }
    s as i64
}

#[test]
fn osr_enters_in_place_tested_for_loops_at_both_integer_limits() {
    for ty in TYPES {
        for (a, b) in [(ty.min, ty.min + ty.span), (ty.max - ty.span, ty.max)] {
            let source = ends_program(ty, a, b);
            let expected = ends_reference(ty, a, b);
            let inline = ty.max <= u32::MAX as i128;
            for mode in [Mode::Off, Mode::Sync, Mode::Async] {
                for level in [OptimizationLevel::None, OptimizationLevel::Standard] {
                    let runtime = runtime(mode);
                    let value = execute(&runtime, &source, level);
                    assert_eq!(
                        value,
                        Value::int(expected),
                        "{} {a}..{b} {mode:?} {level:?}\n{source}",
                        ty.name
                    );
                    if JIT_TARGET && inline && matches!(mode, Mode::Sync) {
                        assert!(
                            runtime.jit_osr_executions() >= 1,
                            "{} {a}..{b} {level:?}: no compiled loop ran",
                            ty.name
                        );
                    }
                }
            }
        }
    }
}

const REUSED_COUNTER_LOOPS: &[&str] = &[
    "for j in 0..n",
    "for j in 0..=n - 1",
    "for j in n - 1..=0 step -1",
    "for j in n..0 step -1",
    "for j in 0..n step k",
    "for j in n - 1..=0 step nk",
];

fn reused_program(header: &str, method: bool) -> String {
    let body = format!(
        "    let k = 1\n    let nk = -1\n    let mut s = 0\n    {{\n        let a = v\n        for i in 0..n {{ s = (s + a[i & 63]) & 0xFFFFFF }}\n    }}\n    {header} {{ s = (s ^ w[j & 63]) + 1 }}\n    return s"
    );
    let signature = "n: int, v: Vec<int>, w: Vec<int>";
    let head = "let w: Vec<int> = vec![5; 64]\n";
    if method {
        format!(
            "{head}struct R {{ q: int }}\nimpl R {{\n fn work(self, {signature}) -> int {{\n{body}\n }}\n}}\nlet r = R {{ q: 0 }}\nr.work(200000, vec![3; 64], w)\n"
        )
    } else {
        format!("{head}fn work({signature}) -> int {{\n{body}\n}}\nwork(200000, vec![3; 64], w)\n")
    }
}

fn reused_reference() -> i64 {
    let n = 200_000i64;
    let mut s = 0i64;
    for _ in 0..n {
        s = (s + 3) & 0xFF_FFFF;
    }
    for _ in 0..n {
        s = (s ^ 5) + 1;
    }
    s
}

#[test]
fn a_reused_counter_register_stays_unboxed_in_every_in_place_tested_loop_shape() {
    let mut refused = Vec::new();
    for header in REUSED_COUNTER_LOOPS {
        for method in [false, true] {
            let source = reused_program(header, method);
            for mode in [Mode::Off, Mode::Sync, Mode::Async] {
                let runtime = runtime(mode);
                let value = execute(&runtime, &source, OptimizationLevel::Standard);
                assert_eq!(
                    value,
                    Value::int(reused_reference()),
                    "{header} method={method} {mode:?}"
                );
                if JIT_TARGET
                    && matches!(mode, Mode::Sync)
                    && (runtime.jit_osr_executions() < 1 || runtime.jit_deoptimizations() >= 4)
                {
                    refused.push(format!(
                        "{header} method={method}: {} osr runs, {} exits",
                        runtime.jit_osr_executions(),
                        runtime.jit_deoptimizations()
                    ));
                }
            }
        }
    }
    assert!(refused.is_empty(), "{refused:#?}");
}
