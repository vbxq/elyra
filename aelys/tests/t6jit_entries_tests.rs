use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, Value};

const JIT_TARGET: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

fn evaluate(source: &str) -> (Value, usize) {
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
    (value, runtime.jit_cache_entries())
}

#[test]
fn three_hot_loops_of_one_function_share_one_compile() {
    let source = r#"
fn pick(mode: int, n: int) -> int {
    let mut s = mode
    if mode == 0 {
        for i in 0..n { s = (s + i * 3) & 0xFFFFFF }
    } else if mode == 1 {
        let mut j = 0
        while j < n {
            s = (s + j * 5) & 0xFFFFFF
            j = j + 1
        }
    } else {
        for k in 0..n { s = (s ^ (k * 7)) & 0xFFFFFF }
    }
    return s
}
pick(0, 30000) + pick(1, 30000) + pick(2, 30000)
"#;
    let n = 30000i64;
    let a = (0..n).fold(0i64, |s, i| (s + i * 3) & 0xFF_FFFF);
    let b = (0..n).fold(1i64, |s, j| (s + j * 5) & 0xFF_FFFF);
    let c = (0..n).fold(2i64, |s, k| (s ^ (k * 7)) & 0xFF_FFFF);
    let (value, entries) = evaluate(source);
    assert_eq!(value, Value::int(a + b + c));
    if JIT_TARGET {
        assert_eq!(entries, 1, "each loop header was compiled separately");
    }
}

#[test]
fn nested_loop_headers_share_one_compile_and_enter_at_either_header() {
    let source = r#"
fn nested(n: int, m: int) -> int {
    let mut s = 0
    for i in 0..n {
        let mut t = i
        for j in 0..m { t = (t * 5 + j) & 0xFFFF }
        s = (s + t) & 0xFFFFFF
    }
    let mut r = 0
    for k in 0..(n * m) { r = (r + k) & 0xFFFFF }
    return s + r
}
nested(40, 600) + nested(3, 20000)
"#;
    let nested = |n: i64, m: i64| {
        let mut s = 0i64;
        for i in 0..n {
            let mut t = i;
            for j in 0..m {
                t = (t * 5 + j) & 0xFFFF;
            }
            s = (s + t) & 0xFF_FFFF;
        }
        let r = (0..n * m).fold(0i64, |r, k| (r + k) & 0xF_FFFF);
        s + r
    };
    let (value, entries) = evaluate(source);
    assert_eq!(value, Value::int(nested(40, 600) + nested(3, 20000)));
    if JIT_TARGET {
        assert_eq!(entries, 1, "each loop header was compiled separately");
    }
}
