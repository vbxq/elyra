use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 3] = [JitMode::Off, JitMode::Baseline, JitMode::Tiered];

const CLI_MODES: [&[&str]; 3] = [&["-O0"], &["-O2"], &["-ae.jit=on"]];

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

fn assert_string_everywhere(source: &str, expected: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::String(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => {
                    panic!("{level:?} {jit:?}: expected {expected:?}, got {other:?}\n{source}")
                }
            }
        }
    }
}

fn assert_runtime_error_everywhere(source: &str, needle: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(value) => panic!("{level:?} {jit:?}: expected an error, got {value:?}"),
                Err(text) => {
                    assert!(text.contains(needle), "{level:?} {jit:?}: {text}");
                    assert!(!text.contains("[E0"), "a compile error:\n{text}");
                }
            }
        }
    }
}

fn rejected(source: &str, code: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains(&format!("[{code}]")),
                "expected {code}:\n{message}\n{source}"
            );
            message
        }
    }
}

fn error_code(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => {
            let message = error.to_string();
            let start = message.find("[E").expect("a diagnostic code");
            message[start + 1..start + 6].to_string()
        }
    }
}

// a stale binary would silently prove nothing, so the build runs once per test binary
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

fn cli_stdout(source: &str, flags: &[&str]) -> String {
    let mut file = tempfile::Builder::new()
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    let output = Command::new(cli_binary())
        .args(flags)
        .arg(file.path())
        .output()
        .expect("aelys-cli should run");
    assert!(
        output.status.success(),
        "{flags:?} exited with {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn field_init_shorthand_reads_the_same_named_variable_in_every_literal_form() {
    assert_int_everywhere(
        r#"
struct P { x: int, y: int }
struct W<T> { inner: T, tag: int }
struct Outer { p: P, n: int }
struct One { x: int }
enum E { V { x: int, y: int }, U }
impl P {
    fn swapped(self) -> P { let x = self.y; let y = self.x; P { x, y } }
}
fn mk(x: int, y: int) -> P { P { x, y } }
fn wrap<T>(inner: T, tag: int) -> W<T> { W { inner, tag } }
fn variant(x: int) -> E { E::V { x, y: x + 1 } }
fn score(e: E) -> int { match e { E::V { x, y } => x * 10 + y, E::U => 0 } }
fn sum(p: P) -> int { match p { P {
    x,
    y
} => x + y } }
let mut total = 0
for i in 0..400 {
    let x = i
    let y = 2
    let p = P { x, y }
    let q = P { y, x: 1 }
    let n = 3
    let o = Outer { p: P {
        x,
        y
    }, n }
    let make = fn(x: int) -> P { P { x, y: x } }
    let inner = 5
    let tag = 7
    let w = W::<int> { inner, tag }
    let lone = Outer {
        p,
        n
    }
    let single = P {
        x,
        y: lone.n
    }
    let one = One {
        x
    }
    total += p.x + p.y + q.x + o.p.y + o.n + make(i).y + w.inner + w.tag + wrap(p, 1).inner.x + mk(1, 2).swapped().x + score(variant(1)) + sum(q) + lone.p.x - single.x + one.x - x
}
total
"#,
        3 * (399 * 400 / 2) + 37 * 400,
    );
}

#[test]
fn a_shorthand_naming_an_undefined_variable_is_the_undefined_variable_error_at_its_name() {
    let message = rejected(
        "struct P { x: int, y: int }\nlet x = 1\nlet p = P { x, y }\np.x",
        "E0301",
    );
    assert!(message.contains("undefined variable: y"), "{message}");
    assert!(message.contains(":3:16"), "{message}");
    let message = rejected("enum E { V { a: int } }\nlet e = E::V { a }\n0", "E0301");
    assert!(message.contains("undefined variable: a"), "{message}");
    assert!(message.contains(":2:16"), "{message}");
    let message = rejected(
        "struct P { x: int, y: int }\nlet x = 1\nlet p = P { x, x: 2 }\n0",
        "E0108",
    );
    assert!(
        message.contains("duplicate struct literal field 'x'"),
        "{message}"
    );
}

#[test]
fn return_break_and_continue_are_match_arm_bodies_without_braces() {
    let source = r#"
let mut log = 0
fn first_or(o: Option<int>) -> int {
    let v = match o { Some(x) => x, None => return -1 };
    v * 2
}
fn unit_return(n: int) {
    match n { 0 => return, _ => log += n }
    log += 1000
}
fn scan(v: Vec<int>) -> int {
    let mut t = 0
    'outer: for i in 0..v.len() {
        for j in 0..5 {
            match j {
                1 => continue,
                3 => continue 'outer,
                _ => {}
            }
            match v[i] { 0 => break 'outer, n => t += n * 10 + j }
        }
    }
    t
}
fn find(limit: int) -> int {
    let mut k = 0
    loop {
        k += 1
        match k { n if n * n > limit => break, _ => continue }
    }
    k
}
fn value_loop(limit: int) -> int {
    let mut k = 0
    let r = loop {
        k += 1
        match k * 3 { n if n > limit => break n, _ => continue }
    }
    r
}
fn with_while(v: Vec<int>) -> int {
    let mut i = 0
    let mut t = 0
    while i < v.len() {
        let x = v[i]
        i += 1
        match x { 0 => continue, 9 => break, n => t += n }
    }
    t
}
let lam = fn(o: Option<int>) -> int { match o { Some(n) => n, None => return 77 } }
let mut total = 0
for i in 0..300 {
    unit_return(i % 2)
    total += first_or(Some(3)) + first_or(None) + scan(vec![1, 2, 0, 4]) + find(50) + value_loop(20) + with_while(vec![1, 0, 2, 9, 5]) + lam(None) + lam(Some(1))
}
total * 1000000 + log
"#;
    let per_round = 6 - 1 + 64 + 8 + 21 + 3 + 77 + 1;
    assert_int_everywhere(source, per_round * 300 * 1_000_000 + 150 * 1001);
}

#[test]
fn a_panic_arm_has_type_never_and_a_jump_arm_is_checked_like_its_braced_form() {
    let source = "fn h(n: int) -> int { match n { 0 => panic(\"zero arm\"), _ => n } }\n";
    assert_int_everywhere(&format!("{source}h(4) + h(5)"), 9);
    assert_runtime_error_everywhere(&format!("{source}h(4) + h(0)"), "zero arm");
    for (bare, braced) in [
        ("0 => break", "0 => { break }"),
        ("0 => continue", "0 => { continue }"),
        ("0 => break 'missing", "0 => { break 'missing }"),
        ("0 => return \"s\"", "0 => { return \"s\" }"),
    ] {
        let wrap = |arm: &str| format!("fn f(n: int) -> int {{ match n {{ {arm}, _ => n }} }}\n0");
        let code = error_code(&wrap(bare));
        assert_eq!(code, error_code(&wrap(braced)), "{bare}");
        assert!(code.starts_with("E0"), "{code}");
    }
    rejected(
        "fn f(o: Option<int>) -> string { match o { Some(x) => x, None => return \"n\" } }\n0",
        "E0301",
    );
}

#[test]
fn char_range_patterns_select_the_right_arm_and_nest() {
    assert_int_everywhere(
        r#"
fn kind(c: char) -> int {
    match c {
        '0'..='9' => 1,
        'a'..='z' | 'A'..='Z' => 2,
        '_' => 3,
        ..' ' => 4,
        '\u{80}'.. => 5,
        _ => 0,
    }
}
fn hex(c: char) -> Option<int> {
    match c {
        '0'..='9' => Some((c as int) - ('0' as int)),
        'a'..'g' => Some((c as int) - ('a' as int) + 10),
        _ => None,
    }
}
fn plane(c: char) -> int {
    match c {
        '\0'..='\u{D7FF}' => 1,
        '\u{E000}'..='\u{10FFFF}' => 2,
    }
}
fn nested(o: Option<(char, int)>) -> int {
    match o {
        Some(('a'..='m', n)) => n,
        Some((_, n)) => n * 10,
        None => 0,
    }
}
let mut total = 0
for round in 0..200 {
    total += kind('5') * 10000 + kind('q') * 1000 + kind('Q') * 100 + kind('_') * 10 + kind('!')
    total += kind('\t') * 100 + kind('é') * 10 + kind('9')
    total += hex('f').unwrap_or(-1) * 100 + hex('7').unwrap_or(-1) + hex('g').unwrap_or(-1) * 1000
    total += plane('a') * 10 + plane('\u{E000}')
    total += nested(Some(('c', 4))) + nested(Some(('z', 4))) + nested(None)
    let ch = 'k'
    if let 'a'..='z' = ch { total += 1 }
}
total
"#,
        (12230 + 451 + 507 + 12 + 44 + 1) * 200,
    );
}

#[test]
fn char_range_exhaustiveness_and_reachability_follow_the_integer_range_rules() {
    let wrap = |arms: &str| format!("fn f(c: char) -> int {{ match c {{ {arms} }} }}\n");
    let message = rejected(&wrap("'a'..='z' => 1"), "E0302");
    assert!(message.contains("missing _"), "{message}");
    rejected(
        &wrap("'\\0'..='\\u{D7FF}' => 1, '\\u{E001}'..='\\u{10FFFF}' => 2"),
        "E0302",
    );
    assert_int_everywhere(
        &format!("{}f('b') * 10 + f('z')", wrap("..='m' => 1, 'n'.. => 2")),
        12,
    );
    assert_int_everywhere(
        &format!(
            "{}f('b') * 10 + f('y')",
            wrap("'a'..='m' => 1, 'c'..='x' => 2, _ => 3")
        ),
        13,
    );
    let message = rejected(&wrap("'a'..='z' => 1, 'c' => 2, _ => 3"), "E0356");
    assert!(message.contains("'c'"), "{message}");
    rejected(
        &wrap("'a'..='z' => 1, 'b'..'e' | 'q' => 2, _ => 3"),
        "E0356",
    );
    rejected(
        &wrap("'a'..='m' => 1, 'n'..='z' => 2, 'c'..='x' => 3, _ => 4"),
        "E0356",
    );
    rejected(&wrap("'a' => 1, 'a' => 2, _ => 3"), "E0356");
}

#[test]
fn a_range_on_a_string_float_mixed_or_empty_bound_is_rejected() {
    let wrap = |arms: &str| format!("fn f(c: char) -> int {{ match c {{ {arms}, _ => 0 }} }}\n0");
    for (arms, needle) in [
        ("\"a\"..=\"z\" => 1", "integer or char literal"),
        ("1.0..=2.0 => 1", "integer or char literal"),
        ("'a'..=5 => 1", "char literals"),
        ("1..='9' => 1", "integer literal"),
        ("'z'..='a' => 1", "matches no value"),
        ("'a'..'a' => 1", "matches no value"),
        ("'a'..= => 1", "needs an end"),
    ] {
        let message = rejected(&wrap(arms), "E0108");
        assert!(message.contains(needle), "{arms}: {message}");
    }
    rejected(
        "fn f(n: int) -> int { match n { 'a'..='z' => 1, _ => 2 } }\n0",
        "E0301",
    );
    rejected(
        "fn f(c: char) -> int { match c { 1..=5 => 1, _ => 2 } }\n0",
        "E0301",
    );
}

#[test]
fn a_script_ending_in_an_assignment_yields_unit_at_every_level() {
    let prelude = "struct S { x: int, v: Vec<int> }\nlet mut a = 1\nlet mut v = vec![1, 2]\nlet mut s = S { x: 1, v: vec![5] }\nlet mut m: Map<string, int> = Map::new()\nlet mut arr = [1, 2]\nfn ix() -> int { 1 }\n";
    for tail in [
        "a = 5",
        "a += 2",
        "v[0] = 4",
        "v[0] += 1",
        "v[ix()] += 1",
        "s.x = 3",
        "s.x *= 3",
        "s.v[0] += 2",
        "m[\"k\"] = 1",
        "arr[1] = 9",
        "(a = 4)",
        "if a > 0 { a = 2 } else { a = 3 }",
        "{ a = 7 }",
        "match a { 1 => a = 2, _ => a = 3 }",
    ] {
        for level in LEVELS {
            let mut vm = new_vm().expect("vm");
            let value =
                run_with_vm_and_opt(&mut vm, &format!("{prelude}{tail}"), "p1_tail.aelys", level)
                    .unwrap_or_else(|error| panic!("{tail} {level:?}: {error}"));
            assert!(value.is_unit(), "{tail} {level:?}: {value:?}");
        }
    }
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(
            &mut vm,
            &format!("{prelude}a += 2\na"),
            "p1_tail.aelys",
            level,
        )
        .expect("runs");
        assert_eq!(value.as_int(), Some(3), "{level:?}");
    }
}

#[test]
fn the_cli_prints_nothing_for_a_script_ending_in_an_assignment_or_a_unit_value() {
    for tail in ["a += 2", "a = 5", "v[0] = 9", "s.x = 4", "println(a)", "()"] {
        let source = format!(
            "struct S {{ x: int }}\nlet mut a = 1\nlet mut v = vec![1]\nlet mut s = S {{ x: 1 }}\nprintln(\"start\")\n{tail}\n"
        );
        let expected = if tail == "println(a)" {
            "start\n1\n"
        } else {
            "start\n"
        };
        for flags in CLI_MODES {
            assert_eq!(cli_stdout(&source, flags), expected, "{tail} {flags:?}");
        }
    }
    for flags in CLI_MODES {
        assert_eq!(
            cli_stdout("let a = 2\na * 21\n", flags),
            "42\n",
            "{flags:?}"
        );
    }
}

#[test]
fn aggregates_of_displayable_elements_render_in_debug_form_in_every_position() {
    assert_string_everywhere(
        r#"
fn show<T: Display>(x: T) -> string { "<{x}>" }
let v = vec![1, 2, 3]
let a = [4, 5]
let s = vec!["a", "b\"q"]
let t = (1, "x")
let t3 = (1, 'c', true, 2.5)
let some = Some(3)
let none: Option<int> = None
let ss = Some("s")
let ok: Result<int, string> = Ok(2)
let err: Result<int, string> = Err("bad")
let nested = vec![vec![1], vec![2, 3]]
let chars = vec!['a', '\n']
let mut m: Map<string, Vec<int>> = Map::new()
let _ = m.insert("b", vec![2])
let _ = m.insert("a", vec![0])
let mut im: Map<int, string> = Map::new()
let _ = im.insert(2, "x")
let deep = Some(vec![(1, "a")])
let b: u8 = 200
let bytes = vec![b]
let empty: Vec<int> = vec![]
let mut out = "{v}|{a}|{s}|{t}|{t3}|{some}|{none}|{ss}|{ok}|{err}|{nested}|{chars}|{m}|{im}|{deep}|{bytes}|{empty}"
out = out + "|" + show(vec![1]) + show((1, 2)) + (vec![true]).to_string() + __tostring(Some(1.5)) + [(1, 'z')].to_string()
let word = "s"
let ch = 'c'
out + "|{word}|{ch}"
"#,
        concat!(
            r#"[1, 2, 3]|[4, 5]|["a", "b\"q"]|(1, "x")|(1, 'c', true, 2.5)|Some(3)|None|Some("s")|"#,
            r#"Ok(2)|Err("bad")|[[1], [2, 3]]|['a', '\n']|{"b": [2], "a": [0]}|{2: "x"}|"#,
            r#"Some([(1, "a")])|[200]|[]|<[1]><(1, 2)>[true]Some(1.5)[(1, 'z')]|s|c"#
        ),
    );
}

#[test]
fn println_and_placeholders_render_aggregates_through_the_cli() {
    let source = r#"
println(vec![1, 2])
println((1, "x"))
println(Some(vec!["a"]))
println("v={}", [1, 2])
let e: Result<int, string> = Err("no")
print(e)
println("")
let mut m: Map<string, int> = Map::new()
let _ = m.insert("z", 1)
let _ = m.insert("a", 2)
eprintln(m)
println(m)
"#;
    for flags in CLI_MODES {
        assert_eq!(
            cli_stdout(source, flags),
            "[1, 2]\n(1, \"x\")\nSome([\"a\"])\nv=[1, 2]\nErr(\"no\")\n{\"z\": 1, \"a\": 2}\n",
            "{flags:?}"
        );
    }
}

#[test]
fn an_aggregate_holding_a_struct_or_a_closure_is_rejected_in_every_rendering_position() {
    let types = "struct P { x: int }\nimpl Display for P { fn to_display(self) -> string { \"P\" } }\nstruct Q { x: int }\n";
    for line in [
        "println(vec![P { x: 1 }])",
        "println(\"{vec![P { x: 1 }]}\")",
        "println(\"{}\", Some(P { x: 1 }))",
        "println((1, P { x: 1 }))",
        "println(vec![Q { x: 1 }])",
        "let r: Result<P, string> = Err(\"e\")\nprintln(\"{r}\")",
        "let f = fn(x: int) -> int { x }\nprintln(vec![f])",
        "let s = vec![P { x: 1 }].to_string()",
    ] {
        let message = rejected(&format!("{types}{line}\n0"), "E0338");
        assert!(message.contains("render each element"), "{line}: {message}");
    }
    assert_string_everywhere(&format!("{types}let p = P {{ x: 2 }}\n\"{{p}}\""), "P");
}
