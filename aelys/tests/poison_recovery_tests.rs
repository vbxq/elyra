use aelys::run;
use aelys_sema::TypeError;
use aelys_sema::types::{InferType, TypeVarId};
use aelys_sema::unify::{Substitution, unify};

fn inference_errors(text: &str) -> Vec<TypeError> {
    let source = aelys_syntax::Source::new("<poison>", text);
    let tokens = aelys_frontend::lexer::Lexer::with_source(source.clone())
        .scan()
        .expect("the fixture must lex");
    let statements = aelys_frontend::parser::Parser::new(tokens, source.clone())
        .parse()
        .expect("the fixture must parse");
    match aelys_sema::TypeInference::infer_program_full(
        statements,
        source,
        Default::default(),
        Default::default(),
    ) {
        Ok(_) => Vec::new(),
        Err(errors) => errors,
    }
}

fn codes(errors: &[TypeError]) -> Vec<u16> {
    errors.iter().map(TypeError::diagnostic_code).collect()
}

fn rendered(errors: &[TypeError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

const THREE_INDEPENDENT_ERRORS: &str = r#"
fn takes_int(value: int) -> int { value }

fn first() -> int {
    let a: int = "one"
    a
}

fn second() -> int {
    let b: bool = 2
    3
}

fn third() -> int {
    let c: string = takes_int(4)
    5
}

first() + second() + third()
"#;

#[test]
fn every_type_error_is_reported_not_only_the_first() {
    let errors = inference_errors(THREE_INDEPENDENT_ERRORS);
    let mismatches = codes(&errors).iter().filter(|code| **code == 301).count();
    assert_eq!(
        mismatches,
        3,
        "recovery must survive the first error and report all three; got {:?}\n{}",
        codes(&errors),
        rendered(&errors)
    );
}

#[test]
fn poison_never_unifies_with_anything() {
    let mut subst = Substitution::new();
    assert!(
        unify(&InferType::Poison, &InferType::Poison, &mut subst).is_err(),
        "poison must not absorb itself"
    );
    assert!(unify(&InferType::Poison, &InferType::I64, &mut subst).is_err());
    assert!(unify(&InferType::String, &InferType::Poison, &mut subst).is_err());
    assert!(
        unify(&InferType::Poison, &InferType::Never, &mut subst).is_err(),
        "never must not absorb poison"
    );
    assert!(
        unify(&InferType::Poison, &InferType::Error, &mut subst).is_err(),
        "the error type must not absorb poison"
    );
    assert!(
        unify(
            &InferType::Poison,
            &InferType::Var(TypeVarId(7)),
            &mut subst
        )
        .is_err(),
        "a type variable must not bind to poison"
    );
    assert!(
        subst.is_empty(),
        "a rejected unification must leave no binding behind"
    );
}

#[test]
fn dynamic_no_longer_absorbs_a_concrete_type() {
    let mut subst = Substitution::new();
    assert!(unify(&InferType::Dynamic, &InferType::I64, &mut subst).is_err());
    assert!(unify(&InferType::Bool, &InferType::Dynamic, &mut subst).is_err());
}

const POISONED_BINDING_FLOWS_ON: &str = r#"
fn takes_string(value: string) -> int { 1 }

fn probe() -> int {
    let inferred = []
    takes_string(inferred)
}

probe()
"#;

#[test]
fn poisoned_recovery_fails_the_compilation_after_all_diagnostics() {
    let errors = inference_errors(POISONED_BINDING_FLOWS_ON);
    let codes = codes(&errors);
    assert!(
        codes.contains(&301),
        "the original mismatch must still be reported: {:?}\n{}",
        codes,
        rendered(&errors)
    );
    assert!(
        codes.contains(&354),
        "the poisoned type must reach the final audit and fail the build: {:?}\n{}",
        codes,
        rendered(&errors)
    );
    assert!(
        !codes.contains(&347),
        "recovery must no longer smuggle dynamic into the surface: {:?}",
        codes
    );

    let message = run(POISONED_BINDING_FLOWS_ON, "poison.aelys")
        .expect_err("a poisoned program must not compile")
        .to_string();
    assert!(
        message.contains("error["),
        "the driver must surface a diagnostic: {message}"
    );
}

const VALID_PROGRAM: &str = r#"
struct Point { x: int, y: int }

impl Point {
    fn total(self) -> int { self.x + self.y }
}

fn pick<T>(value: T) -> T { value }

fn accumulate(values: [int; 3]) -> int {
    let mut total = 0
    for value in values {
        total = total + value
    }
    total
}

fn probe() -> int {
    let p = Point { x: 3, y: 4 }
    let numbers = [p.x, p.y, pick(5)]
    let label = pick("ok")
    let bonus = if label == "ok" { 1 } else { 0 }
    p.total() + accumulate(numbers) + bonus
}

probe()
"#;

#[test]
fn a_valid_program_still_compiles_and_runs() {
    let value = run(VALID_PROGRAM, "valid.aelys").expect("the strict unifier must accept this");
    assert_eq!(value.as_int(), Some(20));
}
