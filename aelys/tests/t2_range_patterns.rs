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
        let value = run_with_vm_and_opt(&mut vm, source, "t2_range.aelys", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{level:?} on:\n{source}");
    }
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(
        &mut vm,
        source,
        "t2_range.aelys",
        OptimizationLevel::Standard,
    ) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn inclusive_open_and_exclusive_ranges_select_the_right_arm() {
    int_at_every_level(
        r#"
        fn class(n: int) -> int {
            match n {
                ..=-1 => 1,
                0 => 2,
                1..=9 => 3,
                10..100 => 4,
                100.. => 5,
            }
        }
        class(-50) * 100000 + class(0) * 10000 + class(9) * 1000 + class(10) * 100 + class(99) * 10 + class(100)
        "#,
        123_445,
    );
}

#[test]
fn ranges_nest_inside_variants_and_alternatives() {
    int_at_every_level(
        r#"
        fn f(o: Option<int>) -> int {
            match o {
                Some(1..=3 | 7) => 1,
                Some(-5..=-1) => 2,
                Some(_) => 3,
                None => 4,
            }
        }
        f(Some(2)) * 10000 + f(Some(7)) * 1000 + f(Some(-3)) * 100 + f(Some(5)) * 10 + f(None)
        "#,
        11_234,
    );
}

#[test]
fn a_fully_covered_sized_integer_needs_no_wildcard() {
    int_at_every_level(
        r#"
        fn half(b: u8) -> int { match b { 0..=127 => 0, 128..=255 => 1 } }
        let lo: u8 = 3
        let hi: u8 = 200
        half(lo) * 10 + half(hi)
        "#,
        1,
    );
    int_at_every_level(
        r#"
        fn sign(n: int) -> int { match n { ..=-1 => -1, 0 => 0, 1.. => 1 } }
        sign(-7) * 100 + sign(0) * 10 + sign(44)
        "#,
        -99,
    );
}

#[test]
fn an_integer_match_with_a_gap_names_the_gap() {
    let message = rejected(
        r#"
        fn half(b: u8) -> int { match b { 0..=127 => 0, 129..=255 => 1 } }
        let x: u8 = 1
        half(x)
        "#,
    );
    assert!(
        message.contains("E0302") && message.contains("missing 128"),
        "{message}"
    );
    let message = rejected("fn f(n: int) -> int { match n { 0..=9 => 1 } }\nf(1)");
    assert!(message.contains("missing ..=-1"), "{message}");
}

#[test]
fn a_range_covered_by_earlier_arms_is_unreachable() {
    let message =
        rejected("fn f(n: int) -> int { match n { 0..=9 => 1, 3..=5 => 2, _ => 3 } }\nf(1)");
    assert!(
        message.contains("E0356") && message.contains("3..=5"),
        "{message}"
    );
    let message = rejected("fn f(n: int) -> int { match n { ..=0 => 1, 1.. => 2, _ => 3 } }\nf(1)");
    assert!(message.contains("E0356"), "{message}");
}

#[test]
fn malformed_range_patterns_are_rejected() {
    let message = rejected("fn f(n: int) -> int { match n { 9..=1 => 1, _ => 2 } }\nf(1)");
    assert!(message.contains("E0108"), "{message}");
    let message = rejected("fn f(n: int) -> int { match n { 1..= => 1, _ => 2 } }\nf(1)");
    assert!(message.contains("E0108"), "{message}");
    let message = rejected("fn f(s: string) -> int { match s { 1..=3 => 1, _ => 2 } }\nf(\"a\")");
    assert!(message.contains("E0301"), "{message}");
    let message =
        rejected("fn f(b: u8) -> int { match b { 0..=300 => 1, _ => 2 } }\nlet x: u8 = 1\nf(x)");
    assert!(message.contains("E0301"), "{message}");
}
