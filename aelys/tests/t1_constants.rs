use std::fs;
use std::path::PathBuf;

use aelys::{CompileOptions, Runtime};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use tempfile::TempDir;

fn int_at_every_level(source: &str) -> i64 {
    let mut seen = None;
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        let mut vm = aelys::new_vm().expect("vm");
        let value = aelys::run_with_vm_and_opt(&mut vm, source, "<t1>", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}\n{source}"));
        let value = value.as_int().expect("an int result");
        if let Some(previous) = seen {
            assert_eq!(previous, value, "{level:?} disagrees");
        }
        seen = Some(value);
    }
    seen.expect("a level ran")
}

fn rejected(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn write(dir: &TempDir, path: &str, content: &str) -> PathBuf {
    let file = dir.path().join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("parent directories");
    }
    fs::write(&file, content).expect("module file");
    file
}

#[test]
fn constants_are_usable_in_expressions_and_repeat_counts() {
    assert_eq!(
        int_at_every_level(
            r#"
const N: int = 4
const M = N * 2 + 1
const MASK = (1 << 8) - 1
const NEG = -N
const HALF: float = 1.5 * 2.0
const BIG = N > 3
const NAME: string = "do" + "om"
let a = [0; N]
let v = vec![7; M]
let half = if HALF == 3.0 { 1 } else { 0 }
let big = if BIG { 1 } else { 0 }
let name = if NAME == "doom" { 1 } else { 0 }
a.len() + v.len() * 10 + MASK * 100 + NEG * 100000 + half * 1000000 + big * 10000000 + name * 100000000
"#
        ),
        4 + 90 + 25500 - 400000 + 1000000 + 10000000 + 100000000
    );
}

#[test]
fn constants_work_in_types_functions_closures_and_before_their_declaration() {
    assert_eq!(
        int_at_every_level(
            r#"
fn early() -> int { LATE * 2 }
struct Grid { cells: [int; SIDE] }
const SIDE: int = 3
const LATE = SIDE + 1
fn sized() -> int {
    let g = Grid { cells: [0; SIDE] }
    let local: [int; SIDE] = [1; SIDE]
    g.cells.len() + local.len()
}
fn local_constant() -> int {
    const K = SIDE * 5
    let a = [0; K]
    a.len()
}
let before = AFTER
const AFTER: int = 11
let read = fn() { return SIDE + LATE }
early() + sized() * 10 + local_constant() * 100 + before * 10000 + read() * 1000000
"#
        ),
        8 + 60 + 1500 + 110000 + 7000000
    );
}

#[test]
fn constants_take_their_declared_sized_type() {
    assert_eq!(
        int_at_every_level(
            r#"
const SMALL: u8 = 200
const WIDE: i32 = 70000
fn take(x: u8) -> int { x as int }
take(SMALL) + (WIDE as int)
"#
        ),
        70200
    );
}

#[test]
fn a_local_binding_shadows_a_constant() {
    assert_eq!(
        int_at_every_level(
            r#"
const N: int = 3
fn f() -> int {
    let N = 9
    N
}
f() + N * 100
"#
        ),
        309
    );
}

#[test]
fn a_constant_initializer_must_be_a_compile_time_constant() {
    let cases = [
        "fn h() -> int { 3 }\nconst X: int = h()",
        "let y = 2\nconst X = y + 1",
        "const X = [1, 2]",
        "const X = 1 / 0",
        "const X = 10000000000000000 * 1000",
        "fn f() -> int {\n    let n = 2\n    const K = n\n    K\n}",
    ];
    for source in cases {
        let message = rejected(source);
        assert!(message.contains("E0395"), "{source}\n{message}");
        assert!(
            message.contains("compile-time constant")
                || message.contains("cannot be evaluated at compile time"),
            "{message}"
        );
    }
}

#[test]
fn a_constant_cannot_be_assigned() {
    let message = rejected("const X: int = 3\nX = 4");
    assert!(message.contains("immutable"), "{message}");
}

#[test]
fn pub_constants_cross_modules_in_expressions_types_and_repeat_counts() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "cfg.aelys",
        r#"
pub const W: int = 320
pub const H: int = W / 2
pub const TITLE: string = "doom"
const SECRET: int = 7
"#,
    );
    write(
        &dir,
        "lib/grid.aelys",
        r#"
needs cfg
pub const CELLS: int = cfg::W * 2
pub fn cells() -> int {
    let a = [0; cfg::H]
    a.len()
}
"#,
    );
    let main = write(
        &dir,
        "main.aelys",
        r#"
needs cfg
needs lib::grid
const LOCAL: int = cfg::H + 1
struct S { xs: [int; cfg::H] }
let a = [0; cfg::W]
let b = [1; LOCAL]
let s = S { xs: [0; cfg::H] }
let c = [0; H]
let t = if cfg::TITLE == "doom" { 1 } else { 0 }
a.len() + b.len() * 1000 + s.xs.len() * 1000000 + grid::cells() + grid::CELLS * 10 + c.len() * 100000000 + t
"#,
    );
    let value = run_file(&main).expect("the program must run");
    assert_eq!(
        value.as_int(),
        Some(320 + 161_000 + 160_000_000 + 160 + 6400 + 16_000_000_000 + 1)
    );

    let selective = write(
        &dir,
        "selective.aelys",
        "needs cfg::H\nlet a = [0; H]\na.len()\n",
    );
    assert_eq!(run_file(&selective).expect("run").as_int(), Some(160));

    let private = write(&dir, "private.aelys", "needs cfg\ncfg::SECRET\n");
    let error = run_file(&private).expect_err("a private constant is not exported");
    assert!(error.to_string().contains("E0313"), "{error}");
}

#[test]
fn a_module_function_named_like_a_native_wins_in_every_call_position() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir,
        "geo.aelys",
        r#"
pub fn sqrt(t: int) -> int {
    for i in 0..3 { if i == t { return i } }
    t + 100
}
pub fn min(t: int) -> int { t + 200 }
fn local_use() -> int { second(0, sqrt(4), 1) }
fn second(a: int, b: int, c: int) -> int { b }
pub fn through_module() -> int { local_use() }
"#,
    );
    let main = write(
        &dir,
        "main.aelys",
        r#"
needs geo
fn second(a: int, b: int, c: int) -> int { b }
second(0, geo::sqrt(4), 1) + vec![geo::sqrt(4)][0] * 10 + geo::through_module() * 100 + second(0, geo::min(1), 2) * 1000
"#,
    );
    let value = run_file(&main).expect("the program must run");
    assert_eq!(value.as_int(), Some(104 + 1040 + 10400 + 201_000));
}
