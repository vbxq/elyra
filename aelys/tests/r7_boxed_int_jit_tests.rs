mod t9_support;

use t9_support::run_with;

const MODES: [&[&str]; 5] = [
    &[],
    &["-ae.jit=on"],
    &["-ae.jit=on", "-ae.jit-sync=on"],
    &[
        "-ae.jit=on",
        "-ae.jit-sync=on",
        "-ae.gc-stress=on",
        "-ae.gc-verify=on",
    ],
    &["-O3", "-ae.jit=on", "-ae.jit-sync=on"],
];

fn assert_prints(source: &str, expected: i64) {
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(0),
            "flags {flags:?}\nstderr: {}",
            outcome.stderr
        );
        assert_eq!(
            outcome.lines(),
            vec![expected.to_string().as_str()],
            "flags {flags:?}"
        );
    }
}

#[test]
fn a_boxed_global_read_by_a_compiled_loop() {
    let big = 1i64 << 49;
    let mut acc = 0i64;
    for i in 0..40000i64 {
        acc = (acc + big % 1000 + i) % 1_000_003;
    }
    assert_prints(
        "let mut big = 562949953421312
fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        acc = (acc + big % 1000 + i) % 1000003
    }
    acc
}
println(\"{run(40000)}\")
",
        acc,
    );
}

#[test]
fn values_that_leave_the_inline_range_inside_a_compiled_loop() {
    let mut acc = 0i64;
    for i in 0..40000i64 {
        let x = if i < 20000 { i } else { i * 8_589_934_592 };
        acc = (acc + x % 1_000_003) % 1_000_003;
    }
    assert_prints(
        "fn run(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        let x = if i < 20000 { i } else { i * 8589934592 }
        acc = (acc + x % 1000003) % 1000003
    }
    acc
}
println(\"{run(40000)}\")
",
        acc,
    );
}

#[test]
fn a_boxed_struct_field_read_by_a_compiled_loop() {
    let mut v = 1i64;
    let mut acc = 0i64;
    for i in 0..40000i64 {
        if i == 25000 {
            v = 1 << 60;
        }
        acc = (acc + v % 1009 + i % 7) % 1_000_003;
    }
    assert_prints(
        "struct Cell { v: int }
fn run(n: int) -> int {
    let mut c = Cell { v: 1 }
    let mut acc = 0
    for i in 0..n {
        if i == 25000 { c.v = 1152921504606846976 }
        acc = (acc + c.v % 1009 + i % 7) % 1000003
    }
    acc
}
println(\"{run(40000)}\")
",
        acc,
    );
}

#[test]
fn a_vec_that_receives_boxed_ints_while_a_compiled_loop_reads_it() {
    let mut v = vec![0i64; 64];
    let mut acc = 0i64;
    for i in 0..40000i64 {
        let k = (i % 64) as usize;
        v[k] = if i > 30000 { i * 4_398_046_511_104 } else { i };
        acc = (acc + v[(k + 1) % 64] % 1_000_003) % 1_000_003;
    }
    assert_prints(
        "fn run(n: int) -> int {
    let mut v: Vec<int> = vec![0; 64]
    let mut acc = 0
    for i in 0..n {
        let k = i % 64
        v[k] = if i > 30000 { i * 4398046511104 } else { i }
        acc = (acc + v[(k + 1) % 64] % 1000003) % 1000003
    }
    acc
}
println(\"{run(40000)}\")
",
        acc,
    );
}
