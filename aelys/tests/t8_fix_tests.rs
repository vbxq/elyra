use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use std::fmt::Write as _;
use std::path::PathBuf;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 4] = [
    OptimizationLevel::None,
    OptimizationLevel::Basic,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 2] = [JitMode::Off, JitMode::Tiered];

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
            .map_err(|e| format!("{e:?}")),
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

fn assert_runtime_error_everywhere(source: &str, needles: &[&str]) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(value) => {
                    panic!("{level:?} {jit:?}: expected a runtime error, got {value:?}\n{source}")
                }
                Err(text) => {
                    for needle in needles {
                        assert!(
                            text.contains(needle),
                            "{level:?} {jit:?}: expected '{needle}', got:\n{text}"
                        );
                    }
                    assert!(
                        !text.contains("[E0"),
                        "must not be a compile error:\n{text}"
                    );
                }
            }
        }
    }
}

fn assert_rejected_with(source: &str, code: &str) {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected with {code}:\n{source}"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains(code), "expected {code}, got:\n{text}");
        }
    }
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write source");
    path
}

const UNINIT: &str = "read before its initialiser ran";

#[test]
fn a_hoisted_function_reading_an_immutable_int_global_before_its_let_is_a_runtime_error() {
    assert_runtime_error_everywhere(
        "let a = f()\nlet b = 10\nfn f() -> int { b + 1 }\na",
        &["global `b` read before its initialiser ran", ":3:"],
    );
}

#[test]
fn every_global_shape_read_too_early_is_the_same_runtime_error() {
    let cases = [
        (
            "let a = f()\nlet mut b = 10\nfn f() -> int { b * 3 + 1 }\na",
            "b",
        ),
        (
            "let a = f()\nlet mut b = 2.5\nfn f() -> float { b * 2.0 }\n1",
            "b",
        ),
        (
            "let a = f()\nlet mut b = true\nfn f() -> int { if b { 1 } else { 2 } }\na",
            "b",
        ),
        (
            "let a = f()\nlet b = \"hello\"\nfn f() -> int { b.len() }\na",
            "b",
        ),
        (
            "struct P { x: int }\nlet a = f()\nlet p = P { x: 5 }\nfn f() -> int { p.x + 1 }\na",
            "p",
        ),
        (
            "let a = f()\nlet v = vec![1, 2, 3]\nfn f() -> int { v[1] }\na",
            "v",
        ),
        (
            "let a = g(2)\nlet k = 7\nlet add = fn(x: int) -> int { x + k }\nfn g(x: int) -> int { add(x) }\na",
            "add",
        ),
        (
            "struct P { x: int }\nlet a = P { x: 1 }.get()\nlet base = 40\nimpl P {\n    fn get(self) -> int { self.x + base }\n}\na",
            "base",
        ),
        (
            "let a = bump()\nlet mut c = 5\nfn bump() -> int {\n    c += 1\n    c\n}\na",
            "c",
        ),
        (
            "let a = f()\nlet b = 10\n@inline\nfn f() -> int { b + 1 }\na",
            "b",
        ),
        (
            "let a = outer()\nlet b = 3\nfn inner() -> int { b }\nfn outer() -> int { inner() * 2 }\na",
            "b",
        ),
        (
            "let a = h(4)\nlet b = 3\nfn h<T>(x: T) -> int { b }\na",
            "b",
        ),
    ];
    for (source, name) in cases {
        let needle = format!("global `{name}` {UNINIT}");
        assert_runtime_error_everywhere(source, &[&needle]);
    }
}

#[test]
fn a_global_read_after_its_let_ran_still_works_and_still_folds() {
    assert_int_everywhere("let b = 10\nlet a = f()\nfn f() -> int { b + 1 }\na", 11);
    assert_int_everywhere(
        "let mut counter = 100\nlet a = bump()\nfn bump() -> int {\n    counter += 1\n    counter\n}\na * 1000 + counter",
        101101,
    );
    assert_int_everywhere(
        "let a = 1\nlet b = 10\nfn f() -> int { b + a }\nprintln(\"x\")\nlet c = 5\nfn g() -> int { c * 2 }\nf() + g()",
        21,
    );
    assert_int_everywhere(
        "let x = even(10)\nfn even(n: int) -> bool { if n == 0 { true } else { odd(n - 1) } }\nfn odd(n: int) -> bool { if n == 0 { false } else { even(n - 1) } }\nif x { 1 } else { 0 }",
        1,
    );
}

#[test]
fn a_module_initialiser_calling_a_function_that_reads_a_later_module_global_is_a_runtime_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "lib.aelys",
        "pub let tab = make()\npub let mut scale = 3\nfn make() -> Vec<int> {\n    vec![scale * 2, scale * 3]\n}\n",
    );
    let main = write(&dir, "main.aelys", "use lib\nlib::tab[0] + lib::tab[1]\n");
    let error = run_file(&main).expect_err("the module reads `scale` before it is bound");
    let text = error.to_string();
    assert!(text.contains(&format!("global `scale` {UNINIT}")), "{text}");

    write(
        &dir,
        "lib2.aelys",
        "pub let mut scale = 3\npub let tab = make()\nfn make() -> Vec<int> {\n    vec![scale * 2, scale * 3]\n}\n",
    );
    let main = write(
        &dir,
        "main2.aelys",
        "use lib2\nlib2::tab[0] + lib2::tab[1]\n",
    );
    let value = run_file(&main).expect("bound in order");
    assert_eq!(value.as_int(), Some(15));
}

#[test]
fn a_unit_function_ending_on_an_assignment_returns_unit_at_run_time() {
    assert_int_everywhere(
        r#"
let mut g = 0
let mut count = 0
fn set(v: int) { g = v }
fn setc(v: int) { g += v }
let bump = fn() { count = count + 1 }
struct B { v: int }
impl B {
    fn put(mut self, x: int) { self.v = x }
}
fn idx(mut v: Vec<int>) { v[0] = 42 }
fn pick(c: bool) {
    if c { g = 1 } else { g = 2 }
}
fn pick2(c: bool) {
    match c { true => g = 3, false => g = 4 }
}
fn early(v: int) {
    if v == 0 { return }
    g = v
}
fn hold<T>(x: T) { g = 9 }
let mut b = B { v: 1 }
let mut bits = 0
if set(3) == () { bits += 1 }
if setc(3) == () { bits += 2 }
if bump() == () { bits += 4 }
if b.put(9) == () { bits += 8 }
if idx(vec![1]) == () { bits += 16 }
if Some(set(4)) == Some(()) { bits += 32 }
if vec![set(1), set(2)] == vec![(), ()] { bits += 64 }
if pick(true) == () { bits += 128 }
let units: Vec<unit> = vec![set(5)]
if units[0] == () { bits += 256 }
if pick2(false) == () { bits += 512 }
if early(7) == () { bits += 1024 }
if hold(1) == () { bits += 2048 }
let lam = fn(x: int) { if x > 0 { g = x } else { g = 0 - x } }
if lam(3) == () { bits += 4096 }
bits * 100 + g + count
"#,
        819_104,
    );
}

#[test]
fn an_annotated_function_ending_on_an_assignment_still_returns_the_value() {
    assert_int_everywhere(
        "let mut g = 0\nfn set(v: int) -> int {\n    g = v + 1\n}\nlet h = fn(v: int) -> int { g = v * 2 }\nset(4) * 100 + h(3)",
        506,
    );
}

#[test]
fn a_value_return_followed_by_a_trailing_assignment_falls_through_without_a_value() {
    assert_rejected_with(
        "let mut g = 0\nfn f(c: bool) {\n    if c { return 7 }\n    g = 5\n}\nf(true)",
        "E0215",
    );
    assert_rejected_with(
        "let mut g = 0\nlet f = fn(c: bool) {\n    if c { return 7 }\n    g = 5\n}\nf(true)",
        "E0215",
    );
    assert_int_everywhere(
        "let mut g = 0\nfn f(c: bool) {\n    if c { return 7 }\n    g = 5\n    g + 1\n}\nf(true) * 10 + f(false)",
        76,
    );
}

#[test]
fn a_match_tail_with_a_bare_return_arm_and_an_assignment_arm_is_a_unit_tail() {
    assert_int_everywhere(
        r#"
let mut g = 0
fn c(x: int) {
    match x { 0 => { return }, _ => { g = 77 } }
}
c(0)
let first = g
c(1)
first * 1000 + g
"#,
        77,
    );
}

fn wide_frame(locals: usize, keep_param_live: bool) -> (String, i64) {
    let mut source = String::from(
        "fn hog(a: int) -> int {\n    a\n}\nfn add2(a: int, b: int) -> int {\n    let t = hog(a) + hog(b)\n    t\n}\nfn f(n: int) -> int {\n",
    );
    for i in 0..locals {
        writeln!(source, "    let l{i} = n + {i}").unwrap();
    }
    let last = locals - 1;
    writeln!(
        source,
        "    let r = add2(hog(l3), hog(n)) + hog(l{last}) * 2"
    )
    .unwrap();
    source.push_str("    let c = fn(z: int) -> int { hog(z) + l7 }\n");
    source.push_str("    let q = c(hog(1))\n");
    source.push_str("    let s = \"{hog(2)}:{l100}\"\n");
    source.push_str("    let mut acc = 0\n");
    for chunk in (0..locals).collect::<Vec<_>>().chunks(10) {
        let terms: Vec<String> = chunk.iter().map(|i| format!("l{i}")).collect();
        writeln!(source, "    acc += {}", terms.join(" + ")).unwrap();
    }
    if keep_param_live {
        source.push_str("    acc + r + q + s.len() + n\n}\nf(1)\n");
    } else {
        source.push_str("    acc + r + q + s.len()\n}\nf(1)\n");
    }
    let n = 1i64;
    let count = i64::try_from(locals).unwrap();
    let sum: i64 = (0..count).map(|i| n + i).sum();
    let r = (n + 3) + n + (n + count - 1) * 2;
    let q = n + (n + 7);
    let s_len = format!("{}:{}", 2, n + 100).len() as i64;
    let extra = if keep_param_live { n } else { 0 };
    (source, sum + r + q + s_len + extra)
}

#[test]
fn functions_with_more_than_255_live_registers_compile_and_run() {
    for locals in [240, 250, 252, 253, 254, 255, 256, 260, 300] {
        for keep in [false, true] {
            let (source, expected) = wide_frame(locals, keep);
            assert_int_everywhere(&source, expected);
        }
    }
}

#[test]
fn the_reviewers_252_live_register_function_prints_32401() {
    let (source, expected) = wide_frame(252, false);
    assert_eq!(expected, 32401);
    assert_int_everywhere(&source, 32401);
}

#[test]
fn a_cached_closure_call_with_a_destination_above_255_passes_its_argument_above_dest() {
    let mut source = String::from("fn hog(a: int) -> int {\n    a\n}\nfn f(n: int) -> int {\n");
    for i in 0..255 {
        writeln!(source, "    let l{i} = n + {i}").unwrap();
    }
    source.push_str(
        "    let c = fn(z: int) -> int { z + l7 }\n    let q = c(hog(1))\n    let mut acc = 0\n",
    );
    for chunk in (0..255).collect::<Vec<_>>().chunks(10) {
        let terms: Vec<String> = chunk.iter().map(|i| format!("l{i}")).collect();
        writeln!(source, "    acc += {}", terms.join(" + ")).unwrap();
    }
    source.push_str("    acc + q\n}\nf(1)\n");
    let sum: i64 = (0..255).map(|i| 1 + i).sum();
    assert_int_everywhere(&source, sum + 1 + 8);
}

#[test]
fn a_closure_created_in_a_register_above_255_keeps_every_lower_register() {
    let mut source = String::from("fn f(n: int) -> int {\n");
    for i in 0..300 {
        writeln!(source, "    let l{i} = n + {i}").unwrap();
    }
    source.push_str(
        "    let c = fn(z: int) -> int { z + l7 + n }\n    let q = c(l299)\n    let mut acc = n\n",
    );
    for chunk in (0..300).collect::<Vec<_>>().chunks(10) {
        let terms: Vec<String> = chunk.iter().map(|i| format!("l{i}")).collect();
        writeln!(source, "    acc += {}", terms.join(" + ")).unwrap();
    }
    source.push_str("    acc * 1000 + q\n}\nf(1)\n");
    let sum: i64 = 1 + (0..300).map(|i| 1 + i).sum::<i64>();
    assert_int_everywhere(&source, sum * 1000 + 300 + 8 + 1);
}

#[test]
fn the_binary_write_snippet_of_the_stdlib_doc_runs_from_a_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out = dir.path().join("out.bin");
    let main = write(
        &dir,
        "main.aelys",
        &format!(
            "use std::fs\nuse std::bytes as bytes\nlet buffer = bytes::alloc(2)\nbytes::write_u8(buffer, 0, 0xC8)\nbytes::write_u8(buffer, 1, 0xFF)\nlet _ = fs::write_file_bytes(\"{}\", buffer)\n7\n",
            out.display().to_string().replace('\\', "/")
        ),
    );
    let value = run_file(&main).expect("the documented import form compiles from a file");
    assert_eq!(value.as_int(), Some(7));
    assert_eq!(std::fs::read(&out).expect("file"), vec![0xC8, 0xFF]);
}

#[test]
fn a_register_above_255_in_an_operand_with_no_wide_form_is_refused_at_its_line() {
    use aelys_bytecode::{Function, OpCode, Register};
    let mut function = Function::new(None, 0);
    function.emit_register_abc(
        OpCode::CallGlobal,
        Register::new(300),
        Register::new(1),
        Register::new(0),
        42,
    );
    assert_eq!(function.current_offset(), 0, "nothing truncated is emitted");
    assert_eq!(
        function.own_wide_operand_fault(),
        Some((OpCode::CallGlobal, 42))
    );
    function.emit_register_abc(
        OpCode::Move,
        Register::new(300),
        Register::new(1),
        Register::new(0),
        43,
    );
    assert_eq!(
        function.own_wide_operand_fault(),
        Some((OpCode::CallGlobal, 42))
    );
}
