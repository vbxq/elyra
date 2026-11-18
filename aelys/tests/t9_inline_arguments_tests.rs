mod t9_support;

use t9_support::{assert_every_level, assert_traps_at_every_level};

const TICK: &str = r#"let mut n = 0
fn tick() -> int {
    n = n + 1
    println("tick {n}")
    n
}
fn tr(label: string, value: int) -> int {
    println(label)
    value
}
"#;

fn with_tick(body: &str) -> String {
    format!("{TICK}{body}")
}

#[test]
fn an_argument_used_twice_is_evaluated_once() {
    let source = with_tick("fn sq(x: int) -> int { x * x }\nprintln(sq(tick()))\nprintln(n)\n");
    assert_every_level(&source, &["tick 1", "1", "1"], Some(0));
}

#[test]
fn an_argument_of_an_unused_parameter_is_still_evaluated() {
    let source = with_tick(
        "fn second(x: int, y: int) -> int { y }\nprintln(second(tick(), 5))\nprintln(n)\n",
    );
    assert_every_level(&source, &["tick 1", "5", "1"], Some(0));
}

#[test]
fn arguments_are_evaluated_left_to_right_before_the_body() {
    let source = with_tick(
        "fn sub(a: int, b: int) -> int { b - a }\nprintln(sub(tr(\"a\", 1), tr(\"b\", 10)))\n",
    );
    assert_every_level(&source, &["a", "b", "9"], Some(0));
}

#[test]
fn a_parameter_read_in_reverse_order_keeps_the_argument_order() {
    let source = with_tick(
        "fn rev(a: int, b: int, c: int) -> int { c * 100 + b * 10 + a }\nprintln(rev(tr(\"a\", 1), tr(\"b\", 2), tr(\"c\", 3)))\n",
    );
    assert_every_level(&source, &["a", "b", "c", "321"], Some(0));
}

#[test]
fn a_variable_read_by_one_argument_and_written_by_the_next_keeps_its_old_value() {
    let source = r#"let mut g = 1
fn bump() -> int {
    g = g + 10
    g
}
fn sub(a: int, b: int) -> int { a - b }
fn keep(a: int, b: int) -> int { a }
println(sub(g, bump()))
println(keep(g, bump()))
println(sub(bump(), g))
"#;
    assert_every_level(source, &["-10", "11", "0"], Some(0));
}

#[test]
fn a_local_read_by_one_argument_and_assigned_by_the_next_keeps_its_old_value() {
    let source = r#"fn keep(a: int, b: int) -> int { a + a }
fn run() -> int {
    let mut x = 1
    let r = keep(x, x = 5)
    return r * 10 + x
}
println(run())
"#;
    assert_every_level(source, &["25"], Some(0));
}

#[test]
fn nested_inlined_calls_evaluate_each_argument_once() {
    let source = with_tick(
        "fn sq(x: int) -> int { x * x }\nfn add(a: int, b: int) -> int { a + b }\nprintln(sq(add(tick(), sq(tick()))))\nprintln(n)\n",
    );
    assert_every_level(&source, &["tick 1", "tick 2", "25", "2"], Some(0));
}

#[test]
fn inline_always_evaluates_its_argument_once() {
    let source = with_tick(
        "@inline_always\nfn twice(x: int) -> int { x + x }\nprintln(twice(tick()))\nprintln(n)\n",
    );
    assert_every_level(&source, &["tick 1", "2", "1"], Some(0));
}

#[test]
fn an_inlined_call_inside_a_function_and_a_loop_evaluates_once_per_call() {
    let source = with_tick(
        "fn cube(x: int) -> int { x * x * x }\nfn total() -> int {\n    let mut s = 0\n    for i in 0..3 {\n        s = s + cube(tick())\n    }\n    return s\n}\nprintln(total())\n",
    );
    assert_every_level(&source, &["tick 1", "tick 2", "tick 3", "36"], Some(0));
}

#[test]
fn a_conditional_body_still_evaluates_every_argument() {
    let source = with_tick(
        "fn pick(c: bool, a: int, b: int) -> int { if c { a } else { b } }\nprintln(pick(true, tr(\"a\", 1), tr(\"b\", 2)))\n",
    );
    assert_every_level(&source, &["a", "b", "1"], Some(0));
}

#[test]
fn a_method_argument_is_evaluated_once() {
    let source = with_tick(
        "struct P { k: int }\nimpl P {\n    fn sq(self, x: int) -> int { x * x * self.k }\n}\nlet p = P { k: 2 }\nprintln(p.sq(tick()))\n",
    );
    assert_every_level(&source, &["tick 1", "2"], Some(0));
}

#[test]
fn a_float_argument_used_twice_is_evaluated_once() {
    let source = r#"let mut calls = 0
fn half() -> float {
    calls = calls + 1
    0.5
}
fn sq(x: float) -> float { x * x }
println(sq(half()))
println(calls)
"#;
    assert_every_level(source, &["0.25", "1"], Some(0));
}

#[test]
fn a_dividing_argument_of_an_unused_parameter_traps() {
    let source = with_tick(
        "fn second(x: int, y: int) -> int { y }\nlet z = 0\nprintln(second(10 / z, 1))\nprintln(\"unreachable\")\n",
    );
    assert_traps_at_every_level(&source, &[], "division by zero");
}

#[test]
fn a_trapping_argument_traps_after_the_earlier_arguments_and_before_the_later_ones() {
    let source = with_tick(
        "fn first(a: int, b: int, c: int) -> int { a }\nlet z = 0\nprintln(first(tr(\"before\", 1), 10 / z, tr(\"after\", 2)))\n",
    );
    assert_traps_at_every_level(&source, &["before"], "division by zero");
}

#[test]
fn an_out_of_bounds_index_argument_traps_once_in_order() {
    let source = with_tick(
        "fn first(a: int, b: int) -> int { a }\nlet v = vec![1, 2, 3]\nprintln(first(tr(\"ok\", 1), 2))\nprintln(first(v[tr(\"index\", 7)], tr(\"after\", 2)))\n",
    );
    assert_traps_at_every_level(&source, &["ok", "1", "index"], "index out of bounds");
}

#[test]
fn a_trapping_body_traps_after_every_argument_ran() {
    let source = with_tick(
        "fn div(a: int, b: int) -> int { a / b }\nprintln(div(tr(\"a\", 1), tr(\"b\", 0)))\n",
    );
    assert_traps_at_every_level(&source, &["a", "b"], "division by zero");
}
