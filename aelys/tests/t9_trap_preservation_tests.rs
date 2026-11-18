mod t9_support;

use t9_support::{assert_every_level, assert_traps_at_every_level};

#[test]
fn an_unused_division_by_zero_still_traps() {
    let source = "let a = 7\nlet b = 0\nlet _x = a / b\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn an_unused_modulo_by_zero_still_traps() {
    let source = "let a = 7\nlet b = 0\nlet _x = a % b\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn an_unused_division_by_a_call_returning_zero_still_traps() {
    let source = "fn z() -> int { 0 }\nlet a = 7\nlet _x = a / z()\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn an_unused_out_of_bounds_vec_index_still_traps() {
    let source = "let v = vec![1, 2, 3]\nlet i = 10\nlet _x = v[i]\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "index out of bounds");
}

#[test]
fn an_unused_out_of_bounds_constant_index_still_traps() {
    let source = "fn f(v: Vec<int>) -> int {\n    let _x = v[10]\n    return 1\n}\nprintln(f(vec![1, 2, 3]))\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "index out of bounds");
}

#[test]
fn an_unused_out_of_bounds_fixed_array_index_still_traps() {
    let source = "let a: [int; 3] = [1, 2, 3]\nlet i = 5\nlet _x = a[i]\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "index out of bounds");
}

#[test]
fn an_unused_overflowing_multiplication_still_traps() {
    let source = "let a = 140737488355327\nlet b = 2\nlet _x = a * b\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "integer overflow");
}

#[test]
fn an_unused_overflowing_addition_of_constants_still_traps() {
    let source = "let _x = 140737488355327 + 1\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "integer overflow");
}

#[test]
fn an_unused_overflowing_negation_still_traps() {
    let source = "let a = -140737488355328\nlet _x = -a\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "integer overflow");
}

#[test]
fn an_unused_float_to_int_cast_that_overflows_still_traps() {
    let source = "let f = 1e300\nlet _x = f as int\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "integer overflow");
}

#[test]
fn an_unused_slice_out_of_bounds_still_traps() {
    let source = "fn id(x: int) -> int { x }\nlet a = vec![1, 2, 3]\nlet s = a[id(2)..id(9)]\nprintln(\"NO TRAP\")\n";
    assert_traps_at_every_level(source, &[], "index out of bounds");
}

#[test]
fn an_unused_trap_inside_a_function_body_still_traps() {
    let source = r#"fn f(v: Vec<int>, d: int) -> int {
    let _q = 100 / d
    let _e = v[d]
    return 1
}
println(f(vec![1, 2], 1))
println(f(vec![1, 2], 0))
"#;
    assert_traps_at_every_level(source, &["1"], "division by zero");
}

#[test]
fn an_unused_trap_in_a_loop_body_traps_on_the_failing_iteration() {
    let source = r#"let v = vec![1, 2, 3]
for i in 0..5 {
    let _x = v[i]
    println(i)
}
"#;
    assert_traps_at_every_level(source, &["0", "1", "2"], "index out of bounds");
}

#[test]
fn an_unused_trap_in_a_match_arm_block_still_traps() {
    let source = r#"fn f(k: int, d: int) -> int {
    match k {
        0 => {
            let _x = 10 / d
            1
        }
        _ => 2,
    }
}
println(f(1, 0))
println(f(0, 0))
"#;
    assert_traps_at_every_level(source, &["2"], "division by zero");
}

#[test]
fn an_unused_global_constant_that_traps_still_traps() {
    let source = "let K = 10 / 0\nprintln(\"after\")\n";
    assert_traps_at_every_level(source, &[], "division by zero");
}

#[test]
fn provably_safe_unused_expressions_are_still_removed_without_changing_output() {
    let source = r#"let v = vec![1, 2, 3]
let a: [int; 3] = [4, 5, 6]
let x = 9
let _p = a[2]
let _q = x / 3
let _r = x % 4
let _s = 2 * 3 + 1
let _t = 1.5 / 0.0
println("ok {v[0]} {x}")
"#;
    assert_every_level(source, &["ok 1 9"], Some(0));
}
