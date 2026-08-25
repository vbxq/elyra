use aelys::run;
use aelys_runtime::Value;

fn run_ok(source: &str) -> Value {
    run(source, "pattern_reachability.aelys").expect("expected the program to run successfully")
}

fn run_err(source: &str) -> String {
    run(source, "pattern_reachability.aelys")
        .expect_err("expected the program to be rejected")
        .to_string()
}

#[test]
fn an_arm_after_an_unguarded_wildcard_is_unreachable() {
    let error = run_err(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape) -> int {
            match s {
                _ => 0,
                Shape::A => 1,
            }
        }
        probe(Shape::A)
        "#,
    );
    assert!(
        error.contains("error[E0356]") && error.contains("unreachable pattern"),
        "expected E0356 for an arm after an unguarded wildcard, got: {error}"
    );
}

#[test]
fn an_arm_after_an_unguarded_binding_is_unreachable() {
    let error = run_err(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape) -> int {
            match s {
                other => 0,
                Shape::A => 1,
            }
        }
        probe(Shape::B)
        "#,
    );
    assert!(
        error.contains("error[E0356]") && error.contains("unreachable pattern"),
        "expected E0356 for an arm after an unguarded binding, got: {error}"
    );
}

#[test]
fn a_duplicate_constructor_arm_is_unreachable() {
    let error = run_err(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A => 1,
                Shape::B => 2,
                Shape::A => 3,
            }
        }
        probe(Shape::A)
        "#,
    );
    assert!(
        error.contains("error[E0356]") && error.contains("Shape::A"),
        "expected E0356 for a duplicate constructor arm, got: {error}"
    );
}

#[test]
fn a_guarded_duplicate_stays_reachable() {
    let result = run_ok(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape, flag: bool) -> int {
            match s {
                Shape::A if flag => 1,
                Shape::A => 2,
                Shape::B => 3,
            }
        }
        probe(Shape::A, false)
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn a_guarded_wildcard_does_not_shadow_the_arms_after_it() {
    let result = run_ok(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape, flag: bool) -> int {
            match s {
                _ if flag => 9,
                Shape::A => 1,
                Shape::B => 2,
            }
        }
        probe(Shape::B, false)
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn an_alternation_covers_the_union_of_its_constructors() {
    let error = run_err(
        r#"
        enum Shape { A, B, C }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A | Shape::B => 1,
                Shape::B => 2,
                Shape::C => 3,
            }
        }
        probe(Shape::A)
        "#,
    );
    assert!(
        error.contains("error[E0356]") && error.contains("Shape::B"),
        "expected E0356 for a constructor already covered by an alternation, got: {error}"
    );
}

#[test]
fn an_alternation_of_uncovered_constructors_stays_reachable() {
    let result = run_ok(
        r#"
        enum Shape { A, B, C }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A => 1,
                Shape::B | Shape::C => 2,
            }
        }
        probe(Shape::C)
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn an_alternation_is_unreachable_only_when_every_branch_is_covered() {
    let result = run_ok(
        r#"
        enum Shape { A, B, C }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A => 1,
                Shape::A | Shape::C => 2,
                Shape::B => 3,
            }
        }
        probe(Shape::C)
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn a_distinct_payload_arm_stays_reachable() {
    let result = run_ok(
        r#"
        enum Shape { A(int), B }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A(1) => 10,
                Shape::A(value) => value,
                Shape::B => 0,
            }
        }
        probe(Shape::A(7))
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn a_covered_payload_arm_is_unreachable() {
    let error = run_err(
        r#"
        enum Shape { A(int), B }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A(value) => value,
                Shape::A(1) => 10,
                Shape::B => 0,
            }
        }
        probe(Shape::A(7))
        "#,
    );
    assert!(
        error.contains("error[E0356]"),
        "expected E0356 for an arm whose payload is already covered, got: {error}"
    );
}

#[test]
fn a_trailing_wildcard_after_full_coverage_is_unreachable() {
    let error = run_err(
        r#"
        enum Shape { A, B }
        fn probe(s: Shape) -> int {
            match s {
                Shape::A => 1,
                Shape::B => 2,
                _ => 3,
            }
        }
        probe(Shape::A)
        "#,
    );
    assert!(
        error.contains("error[E0356]"),
        "expected E0356 for a wildcard after full constructor coverage, got: {error}"
    );
}

#[test]
fn a_duplicate_option_constructor_is_unreachable() {
    let error = run_err(
        r#"
        fn probe(value: Option<int>) -> int {
            match value {
                Some(inner) => inner,
                None => 0,
                Some(other) => other,
            }
        }
        probe(Some(5))
        "#,
    );
    assert!(
        error.contains("error[E0356]"),
        "expected E0356 for a duplicate Option constructor, got: {error}"
    );
}
