mod t9_support;

use aelys::{CompileOptions, Runtime};
use aelys_common::{AelysError, CompileErrorKind};
use t9_support::{LEVELS, assert_every_level, assert_traps_at_every_level, run_project};

const UNSET: &str = "read before its initialiser ran";

#[test]
fn a_call_through_a_local_named_like_a_top_level_fn_is_never_inlined_as_that_fn() {
    let source = "fn sq(a: int) -> int { a * a }
fn twice(a: int) -> int { a + a }
fn local_shadow() -> int {
    let sq = fn(a: int) -> int { a + 1 }
    sq(5)
}
fn param_shadow(sq: fn(int) -> int) -> int { sq(3) }
fn nested_shadow() -> int {
    fn sq(a: int) -> int { a + 1 }
    sq(5)
}
println(local_shadow())
println(param_shadow(twice))
println(nested_shadow())
{
    let sq = fn(a: int) -> int { a - 1 }
    println(sq(10))
}
";
    assert_every_level(source, &["6", "6", "6", "9"], Some(0));
}

#[test]
fn shadowing_in_lambdas_branches_and_loops_keeps_the_local_callee() {
    let source = "fn sq(a: int) -> int { a * a }
fn twice(a: int) -> int { a + a }
let k = fn(sq: fn(int) -> int) -> int { sq(4) }
println(k(twice))
fn branch() -> int {
    let mut r = 0
    if r == 0 {
        let sq = fn(a: int) -> int { a * 3 }
        r = sq(2)
    }
    r
}
println(branch())
fn looped() -> int {
    let mut total = 0
    let mut i = 0
    while i < 3 {
        let sq = fn(a: int) -> int { a + 100 }
        total = total + sq(i)
        i = i + 1
    }
    total
}
println(looped())
fn before_and_after() -> int {
    let a = sq(2)
    let sq = fn(x: int) -> int { x }
    a + sq(1)
}
println(before_and_after())
println(sq(7))
";
    assert_every_level(source, &["8", "6", "303", "5", "49"], Some(0));
}

#[test]
fn an_unused_int_shift_that_overflows_still_traps() {
    let source = "let s = 63\nlet one = 1\nlet _u = one << s\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "integer overflow");
}

#[test]
fn an_int_shift_that_overflows_still_traps_as_an_unused_inlined_argument() {
    let source = "fn first(a: int, b: int) -> int { a }
fn g(x: int) -> int { first(7, x << 63) }
println(\"start\")
println(g(1))
";
    assert_traps_at_every_level(source, &["start"], "integer overflow");
}

#[test]
fn an_unused_shift_that_folds_in_range_or_never_traps_is_still_removed() {
    let source = "let s = 200
let _a = 1 << 46
let _b = 5 >> s
let c: u8 = 200
let _d = c << 9
let _e = c + c
println(c + c)
println(5 >> s)
";
    assert_every_level(source, &["144", "0"], Some(0));
}

#[test]
fn an_unused_sized_division_by_zero_still_traps() {
    let source = "let a: u8 = 7\nlet z: u8 = 0\nlet _x = a / z\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
    let source = "fn first(a: int, b: i16) -> int { a }
fn g(x: i16, y: i16) -> int { first(1, x % y) }
println(g(3, 0))
";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn sized_division_by_minus_one_wraps_at_every_level() {
    let source = "let m: i32 = -2147483648
let d: i32 = -1
let _x = m / d
println(m / d)
println(m % d)
";
    assert_every_level(source, &["-2147483648", "0"], Some(0));
}

#[test]
fn an_unused_float_to_sized_cast_of_nan_or_inf_still_traps() {
    let source = "let f = 0.0 / 0.0\nlet _x = f as u8\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "cannot convert NaN to u8");
    let source = "fn first(a: int, b: i32) -> int { a }
fn g(f: float) -> int { first(1, f as i32) }
println(g(1.0 / 0.0))
";
    assert_traps_at_every_level(source, &[], "cannot convert inf to i32");
}

#[test]
fn an_unused_int_to_char_cast_outside_the_scalar_range_still_traps() {
    let source = "let n = 55296\nlet _c = n as char\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "invalid char");
}

#[test]
fn an_unused_shl_div_by_zero_still_traps() {
    let source = "let a: i32 = 5\nlet z: i32 = 0\nlet _x = a.shl_div(z, 16)\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn an_unset_global_read_as_an_unused_inlined_argument_still_traps() {
    let source = "fn first(a: int, b: int) -> int { a }
fn g() -> int { first(2, LATE) }
println(g())
let LATE = 5
";
    assert_traps_at_every_level(source, &[], &format!("global `LATE` {UNSET}"));
}

#[test]
fn an_unused_field_read_of_an_unset_global_still_traps() {
    let source = "struct P { x: int }
fn f() -> int {
    let _u = LP.x
    1
}
println(f())
let LP = P { x: 3 }
println(LP.x)
";
    assert_traps_at_every_level(source, &[], &format!("global `LP` {UNSET}"));
    let source = "struct P { x: int }
fn g(a: int, b: int) -> int { a }
fn f() -> int { g(1, LP.x) }
println(f())
let LP = P { x: 3 }
";
    assert_traps_at_every_level(source, &[], &format!("global `LP` {UNSET}"));
}

#[test]
fn an_unused_fixed_index_or_tuple_field_of_an_unset_global_still_traps() {
    let source = "fn f() -> int {
    let _u = LA[1]
    2
}
println(f())
let LA = [1, 2, 3]
println(LA[0])
";
    assert_traps_at_every_level(source, &[], &format!("global `LA` {UNSET}"));
    let source = "fn f() -> int {
    let _u = LT.1
    2
}
println(f())
let LT: (int, int) = (1, 2)
";
    assert_traps_at_every_level(source, &[], &format!("global `LT` {UNSET}"));
}

#[test]
fn an_unset_global_read_inside_a_closure_still_traps() {
    let source = "fn first(a: int, b: int) -> int { a }
let k = fn() -> int {
    let _u = LATE
    first(3, LATE)
}
println(k())
let LATE = 4
";
    assert_traps_at_every_level(source, &[], &format!("global `LATE` {UNSET}"));
}

#[test]
fn a_global_bound_before_any_code_runs_is_read_normally() {
    let source = "struct P { x: int }
let G = 5
let LP = P { x: 4 }
fn first(a: int, b: int) -> int { a }
fn f() -> int {
    let _u = G
    let _v = LP.x
    first(G * 2, LP.x)
}
println(f())
let LATE = 6
println(first(LATE, G))
";
    assert_every_level(source, &["10", "6"], Some(0));
}

#[test]
fn a_module_global_read_before_its_binding_traps_at_every_level() {
    let module = "fn first(a: int, b: int) -> int { a }
pub fn get() -> int {
    let _u = scale
    first(1, scale)
}
pub let early = get()
pub let mut scale = 3
";
    for level in LEVELS {
        let outcome = run_project(
            &[
                ("early.aelys", module),
                ("main.aelys", "needs early\nprintln(early::early)\n"),
            ],
            "main.aelys",
            &[level],
        );
        assert_eq!(outcome.code, Some(1), "{level}\n{}", outcome.stderr);
        assert!(outcome.stdout.is_empty(), "{level}: {}", outcome.stdout);
        assert!(
            outcome.stderr.contains(&format!("global `scale` {UNSET}")),
            "{level}: {}",
            outcome.stderr
        );
    }
}

#[test]
fn a_closure_capturing_the_binding_of_an_unfailing_last_arm_keeps_its_value() {
    let source = "fn mk(i: int) -> fn() -> int {
    match i + 1 {
        n => (fn() -> int { n * 2 }),
    }
}
let mut total = 0
for i in 0..30000 {
    let f = mk(i)
    let g = mk(i + 1)
    total = total + f() + g() - g()
}
println(total)
";
    let expected = (0..30000i64).map(|i| 2 * (i + 1)).sum::<i64>().to_string();
    assert_eq!(expected, "900030000");
    assert_every_level(source, &[expected.as_str()], Some(0));
}

fn on_a_default_thread<T: Send + 'static>(task: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(task)
        .expect("spawn a 2 MiB thread")
        .join()
        .expect("compilation must not overflow a 2 MiB thread")
}

fn compile_on_a_default_thread(source: String) -> Result<(), AelysError> {
    on_a_default_thread(move || {
        Runtime::new()
            .compile(
                &source,
                CompileOptions {
                    optimization_level: aelys_opt::OptimizationLevel::Aggressive,
                    ..CompileOptions::default()
                },
            )
            .map(|_| ())
    })
}

fn is_depth_limit(error: &AelysError) -> bool {
    matches!(
        error,
        AelysError::Compile(error)
            if matches!(error.kind, CompileErrorKind::RecursionDepthExceeded { max: 256 })
    )
}

#[test]
fn programs_inside_the_nesting_limits_compile_in_process_on_a_two_mib_thread() {
    let parens = format!(
        "let x = 1\nlet y = {}x{}\ny\n",
        "(".repeat(255),
        " + 1)".repeat(255)
    );
    compile_on_a_default_thread(parens).expect("255 parentheses compile");
    let calls = format!(
        "fn f(a: int) -> int {{ a + 1 }}\n{}0{}\n",
        "f(".repeat(200),
        ")".repeat(200)
    );
    compile_on_a_default_thread(calls).expect("200 nested calls compile");
    let chain = format!("let t = true\n{}\n", vec!["t"; 1000].join(" and "));
    compile_on_a_default_thread(chain).expect("a 1000-term and chain compiles");
    let blocks = format!(
        "let mut x = 0\n{}x = 7\n{}x\n",
        "if x == 0 {\n".repeat(250),
        "}\n".repeat(250)
    );
    let value = on_a_default_thread(move || aelys::run(&blocks, "blocks.aelys"))
        .expect("250 nested ifs run");
    assert_eq!(value.as_int(), Some(7));
}

#[test]
fn nesting_past_the_limit_is_e0105_in_process_on_a_two_mib_thread() {
    let ifs = format!(
        "let mut x = 0\n{}x = 7\n{}x\n",
        "if x == 0 {\n".repeat(260),
        "}\n".repeat(260)
    );
    let error = compile_on_a_default_thread(ifs).expect_err("260 nested ifs are rejected");
    assert!(is_depth_limit(&error), "{error}");
    let pattern = format!(
        "let v = Some(1)\nmatch v {{\n    {}1{} => 0,\n    _ => 1,\n}}\n",
        "Some(".repeat(200_000),
        ")".repeat(200_000)
    );
    let error =
        compile_on_a_default_thread(pattern).expect_err("a 200000-deep pattern is rejected");
    assert!(is_depth_limit(&error), "{error}");
}
