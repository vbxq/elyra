use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_common::error::RuntimeErrorKind;
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use std::path::PathBuf;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 4] = [
    OptimizationLevel::None,
    OptimizationLevel::Basic,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn value_at(source: &str, level: OptimizationLevel) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
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
        match value_at(source, level) {
            Ok(StructuredValue::Int(got)) => {
                assert_eq!(got, expected, "{level:?}:\n{source}");
            }
            other => panic!("{level:?}: expected {expected}, got {other:?}\n{source}"),
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

fn assert_runtime_error(source: &str, needle: &str) {
    match value_at(source, OptimizationLevel::Standard) {
        Ok(value) => panic!("expected a runtime error, got {value:?}"),
        Err(text) => {
            assert!(text.contains(needle), "expected '{needle}', got:\n{text}");
            assert!(
                !text.contains("E0321"),
                "must not be a compile error:\n{text}"
            );
        }
    }
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write source");
    path
}

fn aelys_path(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[test]
fn a_global_vec_initialised_empty_and_filled_later_takes_a_constant_index() {
    assert_int_everywhere(
        r#"
let mut rows: Vec<Vec<int>> = vec![]
fn setup() {
    rows = vec![vec![1, 2], vec![3, 4]]
}
fn first() -> int {
    let r = rows[0]
    r[1]
}
setup()
first()
"#,
        2,
    );
}

#[test]
fn a_global_vec_grown_by_push_in_a_function_takes_a_constant_index() {
    assert_int_everywhere(
        r#"
let mut v = vec![1]
fn grow() { v.push(9) }
grow()
v[1]
"#,
        9,
    );
}

#[test]
fn a_constant_index_store_and_slice_into_a_refilled_global_vec_are_accepted() {
    assert_int_everywhere(
        r#"
let mut v: Vec<int> = vec![]
fn fill() {
    v = vec![5, 6, 7]
}
fill()
v[2] = 70
let s = v[0..2]
v[2] + s[1] + s.len()
"#,
        78,
    );
}

#[test]
fn a_vec_grown_through_a_mut_parameter_takes_a_constant_index() {
    assert_int_everywhere(
        r#"
fn grow(mut v: Vec<int>) { v.push(7) }
let mut v: Vec<int> = vec![]
grow(v)
v[0]
"#,
        7,
    );
    assert_rejected_with(
        "fn grow(mut v: Vec<int>) { v.push(7) }\nlet v: Vec<int> = vec![]\ngrow(v)\nv[0]",
        "E0323",
    );
}

#[test]
fn a_local_vec_grown_by_a_closure_takes_a_constant_index() {
    assert_int_everywhere(
        r#"
fn f() -> int {
    let mut v: Vec<int> = vec![]
    let g = fn() { v.push(5) }
    g()
    v[0]
}
f()
"#,
        5,
    );
}

#[test]
fn a_method_body_reads_a_refilled_global_vec_with_a_constant_index() {
    assert_int_everywhere(
        r#"
let mut table: Vec<int> = vec![]
struct Reader { k: int }
impl Reader {
    fn head(self) -> int { table[0] + self.k }
}
fn pick<T>(value: T) -> int { table[1] }
table = vec![10, 20]
let r = Reader { k: 1 }
r.head() + pick(3)
"#,
        31,
    );
}

#[test]
fn provably_fixed_lengths_still_reject_a_constant_index() {
    assert_rejected_with("[1, 2][2]", "E0321");
    assert_rejected_with("vec![1, 2][2]", "E0321");
    assert_rejected_with("let a = [1, 2]\na[2]", "E0321");
    assert_rejected_with("let mut a = [1, 2]\na[2]", "E0321");
    assert_rejected_with(
        "fn f() -> int {\n    let a = [7; 2]\n    a[2]\n}\nf()",
        "E0321",
    );
}

#[test]
fn a_vec_binding_out_of_range_traps_at_run_time() {
    assert_runtime_error("let v = vec![1, 2]\nv[2]", "out of bounds");
    assert_runtime_error(
        "let mut rows: Vec<int> = vec![]\nfn f() -> int { rows[0] }\nf()",
        "out of bounds",
    );
}

#[test]
fn a_unit_function_with_an_early_return_ends_on_an_assignment() {
    assert_int_everywhere(
        r#"
let mut g = 0
fn set(v: int) {
    if v == 0 { return () }
    g = v + 1
}
set(1)
set(0)
g
"#,
        2,
    );
}

#[test]
fn every_assignment_form_is_a_unit_tail_after_an_early_return() {
    assert_int_everywhere(
        r#"
let mut g = 0
struct B { n: int }
impl B {
    fn set(mut self, v: int) {
        if v == 0 { return () }
        self.n = v
    }
}
fn bump(v: int) {
    if v == 0 { return () }
    g += v
}
fn put(mut v: Vec<int>, x: int) {
    if x < 0 { return () }
    v[0] = x
}
let mut b = B { n: 1 }
b.set(9)
bump(3)
let mut v = vec![1, 2]
put(v, 8)
let mut h = 0
let f = fn(x: int) {
    if x == 0 { return () }
    h = x * 2
}
f(5)
b.n * 1000 + g * 100 + v[0] * 10 + h
"#,
        9390,
    );
}

#[test]
fn an_annotated_function_still_returns_its_trailing_assignment() {
    assert_int_everywhere(
        r#"
let mut g = 0
fn set(v: int) -> int {
    g = v + 1
}
set(4)
"#,
        5,
    );
}

#[test]
fn an_unannotated_trailing_assignment_returns_unit() {
    assert_rejected_with(
        r#"
let mut g = 0
fn set(v: int) {
    g = v + 1
}
let r: int = set(1)
r
"#,
        "E0301",
    );
}

#[test]
fn a_bare_return_before_a_closing_brace_returns_from_a_unit_function() {
    assert_int_everywhere(
        r#"
let mut g = 0
fn f(x: int) {
    if x < 0 { return }
    g = g + x
}
fn loop_until(n: int) {
    let mut i = 0
    while true {
        if i == n { return }
        g = g + 100
        i = i + 1
    }
}
fn arm(o: Option<int>) {
    match o {
        Some(v) => {
            g = g + v * 1000
            return
        }
        None => { return }
    }
}
fn tail() {
    g = g + 10000
    return
}
let lam = fn(x: int) {
    if x > 0 { return }
    g = -1
}
f(-5)
f(3)
loop_until(2)
arm(Some(4))
arm(None)
tail()
lam(1)
g
"#,
        14203,
    );
}

#[test]
fn a_bare_return_in_a_function_returning_int_is_a_type_error_not_a_parse_error() {
    assert_rejected_with("fn f() -> int { return }\nf()", "E0301");
}

#[test]
fn a_top_level_let_calls_a_function_declared_further_down() {
    assert_int_everywhere(
        r#"
let tab = make()
fn make() -> Vec<int> { vec![1, 2] }
tab[1]
"#,
        2,
    );
    assert_int_everywhere(
        r#"
let x = a()
fn a() -> int { b() + 1 }
fn b() -> int { 40 }
x
"#,
        41,
    );
}

#[test]
fn a_top_level_let_calls_an_associated_function_of_a_later_impl() {
    assert_int_everywhere(
        r#"
let p = Point::origin()
struct Point { x: int, y: int }
impl Point {
    fn origin() -> Point { Point { x: 3, y: 4 } }
}
p.x + p.y
"#,
        7,
    );
}

#[test]
fn a_module_top_level_let_calls_a_function_declared_further_down_the_module() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "lib.aelys",
        "pub let tab = make()\nfn make() -> Vec<int> { vec![1, 2] }\npub let mut counter = 0\npub fn bump() { counter += 1 }\n",
    );
    let main = write(
        &dir,
        "main.aelys",
        "needs lib\nlib::bump()\nlib::tab[1] * 10\n",
    );
    let value = run_file(&main).expect("the module initialises before main runs");
    assert_eq!(value.as_int(), Some(20));
}

#[test]
fn an_undefined_variable_hint_stays_outside_the_quoted_name() {
    let kind = RuntimeErrorKind::UndefinedVariable("make\n\nhint: did you mean 'max'?".to_string());
    assert_eq!(
        kind.message(),
        "undefined variable 'make'\n   = hint: did you mean 'max'?"
    );
    assert_eq!(
        RuntimeErrorKind::UndefinedVariable("make".to_string()).message(),
        "undefined variable 'make'"
    );
}

#[test]
fn write_bytes_writes_every_byte_of_a_vec_unchanged() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("out.bin");
    let source = format!(
        r#"
needs std::fs
let b: Vec<u8> = vec![200, 0, 255, 128]
let ok = match fs::write_bytes("{path}", &b) {{
    Ok(_) => 1,
    Err(_) => 0,
}}
ok
"#,
        path = aelys_path(&path)
    );
    assert_int_everywhere(&source, 1);
    assert_eq!(std::fs::read(&path).expect("file"), vec![200, 0, 255, 128]);
}

#[test]
fn write_chunk_writes_the_raw_bytes_to_an_open_handle() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("out.bin");
    let source = format!(
        r#"
needs std::fs
let b: Vec<u8> = vec![0xC8, 0x80, 0x0A]
let f = fs::open("{path}", "w").unwrap()
let first = fs::write_chunk(f, &b).unwrap()
let second = fs::write_chunk(f, &b).unwrap()
fs::close(f).unwrap()
first + second
"#,
        path = aelys_path(&path)
    );
    assert_int_everywhere(&source, 6);
    assert_eq!(
        std::fs::read(&path).expect("file"),
        vec![0xC8, 0x80, 0x0A, 0xC8, 0x80, 0x0A]
    );
}

#[test]
fn binary_writes_report_a_bad_path_or_handle_as_an_error_value() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("no_such_dir").join("out.bin");
    let source = format!(
        r#"
needs std::fs
let b: Vec<u8> = vec![1]
let bad_path = fs::write_bytes("{missing}", &b).is_err()
let bad_handle = fs::write_chunk(987654, &b).is_err()
let mut n = 0
if bad_path {{ n = n + 10 }}
if bad_handle {{ n = n + 100 }}
n
"#,
        missing = aelys_path(&missing)
    );
    assert_int_everywhere(&source, 110);
}

#[test]
fn binary_writes_are_typed_by_the_native_signature_table() {
    assert_rejected_with(
        "needs std::fs\nlet t = \"text\"\nlet r = fs::write_bytes(\"x.bin\", &t)\n0",
        "E0301",
    );
    assert_rejected_with(
        "needs std::fs\nlet v: Vec<u8> = vec![1]\nlet r: Result<int, string> = fs::write_bytes(\"x.bin\", &v)\n0",
        "E0301",
    );
    assert_rejected_with(
        "needs std::fs\nlet v: Vec<int> = vec![1]\nlet r = fs::write_chunk(1, &v)\n0",
        "E0301",
    );
}

#[test]
fn a_trailing_assignment_of_an_option_is_not_an_unused_option() {
    assert_int_everywhere(
        r#"
let mut shared: Option<int> = Some(42)
fn clear(v: int) {
    if v == 0 { return () }
    shared = None
}
fn reset() {
    shared = Some(7)
}
let mut seen = 0
let mark = fn(x: int) {
    if x == 0 { return }
    seen = x
}
clear(1)
let a = match shared {
    Some(v) => v,
    None => 100,
}
reset()
mark(3)
a + shared.unwrap() + seen
"#,
        110,
    );
}

#[test]
fn storing_an_option_or_result_is_not_discarding_it() {
    assert_int_everywhere(
        r#"
let mut shared: Option<int> = Some(42)
let mut outcome: Result<int, string> = Ok(1)
struct Slot { v: Option<int> }
fn f() {
    shared = None
    outcome = Err("no")
    let mut s = Slot { v: Some(1) }
    s.v = None
    let mut list: Vec<Option<int>> = vec![Some(1)]
    list[0] = Some(9)
    shared = list[0]
}
f()
let bad = match outcome {
    Ok(_) => 0,
    Err(_) => 1000,
}
shared.unwrap() + bad
"#,
        1009,
    );
}
