use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;

fn run_int(source: &str) -> i64 {
    let mut vm = new_vm().expect("vm");
    run_with_vm_and_opt(&mut vm, source, "t2_exh.aelys", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("{error}\n{source}"))
        .as_int()
        .expect("an int result")
}

fn rejected(source: &str) -> String {
    let mut vm = new_vm().expect("vm");
    match run_with_vm_and_opt(&mut vm, source, "t2_exh.aelys", OptimizationLevel::Standard) {
        Ok(value) => panic!("expected a rejection, got {value:?} for:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn err_arms_over_every_user_variant_are_exhaustive() {
    assert_eq!(
        run_int(
            r#"
            enum AppE { Io(string), Bad(int) }
            fn run(n: int) -> Result<int, AppE> {
                if n == 0 { Ok(1) } else if n == 1 { Err(AppE::Io("x")) } else { Err(AppE::Bad(n)) }
            }
            fn code(n: int) -> int {
                match run(n) { Ok(_) => 1, Err(AppE::Io(_)) => 2, Err(AppE::Bad(k)) => k * 10 }
            }
            code(0) * 1000 + code(1) * 100 + code(4)
            "#
        ),
        1240
    );
}

#[test]
fn some_arms_over_every_user_variant_are_exhaustive() {
    assert_eq!(
        run_int(
            r#"
            enum K { A, B }
            fn f(o: Option<K>) -> int { match o { Some(K::A) => 1, Some(K::B) => 2, None => 0 } }
            f(Some(K::A)) * 100 + f(Some(K::B)) * 10 + f(None)
            "#
        ),
        120
    );
}

#[test]
fn three_levels_of_nesting_are_exhaustive() {
    assert_eq!(
        run_int(
            r#"
            enum In { X, Y }
            enum Mid { M(In), N }
            fn f(r: Result<Option<Mid>, int>) -> int {
                match r {
                    Ok(Some(Mid::M(In::X))) => 1,
                    Ok(Some(Mid::M(In::Y))) => 2,
                    Ok(Some(Mid::N)) => 3,
                    Ok(None) => 4,
                    Err(_) => 5,
                }
            }
            f(Ok(Some(Mid::M(In::Y)))) * 10 + f(Err(0))
            "#
        ),
        25
    );
}

#[test]
fn a_missing_nested_constructor_is_named_precisely() {
    let message = rejected(
        r#"
        enum AppE { Io(string), Bad(int) }
        fn run() -> Result<int, AppE> { Ok(1) }
        match run() { Ok(_) => 1, Err(AppE::Io(_)) => 2 }
        "#,
    );
    assert!(
        message.contains("E0302") && message.contains("missing Err(AppE::Bad(..))"),
        "{message}"
    );
    let message = rejected(
        r#"
        enum K { A, B }
        let o: Option<K> = None
        match o { Some(K::A) => 1, None => 0 }
        "#,
    );
    assert!(message.contains("missing Some(K::B)"), "{message}");
    let message = rejected(
        r#"
        enum In { X, Y }
        enum Mid { M(In), N }
        fn f(r: Result<Option<Mid>, int>) -> int {
            match r { Ok(Some(Mid::M(In::X))) => 1, Ok(Some(Mid::N)) => 3, Ok(None) => 4, Err(_) => 5 }
        }
        f(Err(1))
        "#,
    );
    assert!(
        message.contains("missing Ok(Some(Mid::M(In::Y)))"),
        "{message}"
    );
}

#[test]
fn every_missing_top_level_variant_is_listed() {
    let message = rejected(
        r#"
        enum C { Red, Green, Blue }
        fn f(c: C) -> int { match c { C::Green => 1 } }
        f(C::Green)
        "#,
    );
    assert!(message.contains("missing C::Red, C::Blue"), "{message}");
}

#[test]
fn a_misspelled_bare_variant_is_rejected_with_a_suggestion() {
    let message = rejected(
        r#"
        enum Color { Red, Green, Blue }
        fn f(c: Color) -> int { match c { Color::Red => 1, Color::Green => 2, Blu => 3 } }
        f(Color::Green)
        "#,
    );
    assert!(
        message.contains("E0392") && message.contains("did you mean Color::Blue?"),
        "{message}"
    );
}

#[test]
fn a_bare_variant_name_in_any_position_is_rejected() {
    let message = rejected(
        r#"
        enum Color { Red, Green }
        fn f(c: Color) -> int { match c { Red => 1, Color::Green => 2 } }
        f(Color::Green)
        "#,
    );
    assert!(message.contains("did you mean Color::Red?"), "{message}");
    let message = rejected(
        r#"
        enum K { Alpha, Beta }
        fn f(o: Option<K>) -> int { match o { Some(Alpah) => 1, None => 0 } }
        f(None)
        "#,
    );
    assert!(message.contains("did you mean K::Alpha?"), "{message}");
    let message = rejected(
        r#"
        enum K { Alpha, Beta }
        fn f(k: K) -> int { match k { K::Alpha => 1, Zebra => 0 } }
        f(K::Beta)
        "#,
    );
    assert!(
        message.contains("E0392") && message.contains("`Zebra`"),
        "{message}"
    );
}

#[test]
fn lowercase_bindings_on_enums_stay_catch_alls() {
    assert_eq!(
        run_int(
            r#"
            enum Color { Red, Green }
            fn f(c: Color) -> int { match c { Color::Red => 1, other => 2 } }
            f(Color::Green)
            "#
        ),
        2
    );
}

#[test]
fn an_arm_already_covered_by_nested_arms_is_unreachable() {
    let message = rejected(
        r#"
        enum K { A, B }
        fn f(o: Option<K>) -> int { match o { Some(K::A) => 1, Some(K::B) => 2, Some(_) => 3, None => 0 } }
        f(None)
        "#,
    );
    assert!(message.contains("E0356"), "{message}");
}

#[test]
fn bool_tuple_payloads_report_the_uncovered_combination() {
    let message = rejected(
        r#"
        enum E { A(bool, bool), B }
        fn f(e: E) -> int { match e { E::A(true, _) => 1, E::A(_, true) => 2, E::B => 3 } }
        f(E::B)
        "#,
    );
    assert!(message.contains("missing E::A(false, false)"), "{message}");
}
