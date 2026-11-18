use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        let mut vm = new_vm().expect("vm");
        let value = run_with_vm_and_opt(&mut vm, source, "t2_loop.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(
        &mut vm,
        source,
        "t2_loop.aelys",
        OptimizationLevel::Standard,
    ) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn loop_with_break_value_yields_it() {
    int_at_every_level(
        r#"
        let mut i = 0
        let found = loop {
            i = i + 1
            if i * i > 50 { break i }
        }
        found
        "#,
        8,
    );
}

#[test]
fn loop_value_as_function_tail_and_in_a_closure() {
    int_at_every_level(
        r#"
        fn first_multiple(of: int, above: int) -> int {
            let mut n = above + 1
            loop {
                if n % of == 0 { break n }
                n = n + 1
            }
        }
        let collatz = fn(start: int) -> int {
            let mut n = start
            let mut steps = 0
            loop {
                if n == 1 { break steps }
                n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 }
                steps = steps + 1
            }
        }
        first_multiple(7, 20) * 100 + collatz(6)
        "#,
        2108,
    );
}

#[test]
fn a_loop_left_only_by_return_satisfies_the_return_type() {
    int_at_every_level(
        r#"
        fn search(v: Vec<int>, target: int) -> int {
            let mut i = 0
            loop {
                if v[i] == target { return i }
                i = i + 1
            }
        }
        search(vec![4, 8, 15, 16], 15)
        "#,
        2,
    );
}

#[test]
fn labeled_break_and_continue_cross_nested_loops() {
    int_at_every_level(
        r#"
        let mut total = 0
        'outer: for x in 0..10 {
            for y in 0..10 {
                if y > x { continue 'outer }
                if x == 5 { break 'outer }
                total = total + 1
            }
        }
        let mut w = 0
        let mut hits = 0
        'scan: while w < 100 {
            w = w + 1
            let mut k = 0
            loop {
                k = k + 1
                if k == 3 { continue 'scan }
                if w == 4 { break 'scan }
                hits = hits + 1
            }
        }
        total * 1000 + w * 10 + hits
        "#,
        15_046,
    );
}

#[test]
fn break_value_targets_a_labeled_outer_loop() {
    int_at_every_level(
        r#"
        let pair = 'find: loop {
            for a in 1..20 {
                for b in a..20 {
                    if a * b == 91 { break 'find a * 100 + b }
                }
            }
            break 0
        }
        pair
        "#,
        713,
    );
}

#[test]
fn block_expressions_scope_their_statements() {
    int_at_every_level(
        r#"
        let x = 1
        let b = {
            let x = 10
            let y = x * 2
            y + x
        }
        let c = { 5 }
        b * 100 + c * 10 + x
        "#,
        3051,
    );
}

#[test]
fn closures_created_in_a_loop_keep_their_iteration_after_break() {
    int_at_every_level(
        r#"
        let mut fs: Vec<fn() -> int> = vec![]
        let mut i = 0
        loop {
            let captured = i * 10
            fs.push(fn() -> int { captured })
            i = i + 1
            if i == 3 { break }
        }
        fs[0]() + fs[1]() + fs[2]()
        "#,
        30,
    );
}

#[test]
fn label_and_break_value_misuse_is_rejected() {
    let message = rejected("for i in 0..3 { break 'nowhere }\n0");
    assert!(message.contains("E0390"), "{message}");
    let message = rejected("let mut i = 0\nwhile i < 3 { i = i + 1; break 5 }\ni");
    assert!(message.contains("E0391"), "{message}");
    let message = rejected("let x: int = loop { break }\nx");
    assert!(message.contains("E0301"), "{message}");
    let message = rejected("let x = loop { break 1; break \"s\" }\n0");
    assert!(message.contains("E0301"), "{message}");
}
