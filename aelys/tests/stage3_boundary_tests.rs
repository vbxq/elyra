use aelys::{CompileOptions, Runtime, call_function, new_vm, run_with_vm};
use aelys_runtime::Value;

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

fn assert_deferred(message: &str, code: &str, fragment: &str) {
    assert!(
        message.contains(code),
        "expected {code} in the diagnostic: {message}"
    );
    assert!(
        message.contains(fragment),
        "expected the fragment {fragment:?} in the diagnostic: {message}"
    );
}

#[test]
fn shared_borrowing_receiver_is_accepted_in_stage3() {
    let result = Runtime::new().compile(
        r#"
trait Scorable {
    fn score(&self) -> int;
}
"#,
        CompileOptions::default(),
    );
    assert!(
        result.is_ok(),
        "Stage 3 shared receiver should compile: {:?}",
        result.err()
    );
}

#[test]
fn mutable_borrowing_receiver_is_accepted_in_stage3() {
    let result = Runtime::new().compile(
        r#"
struct Point { x: int }
impl Point {
    fn bump(&mut self) -> int { 0 }
}
"#,
        CompileOptions::default(),
    );
    assert!(
        result.is_ok(),
        "Stage 3 mutable receiver should compile: {:?}",
        result.err()
    );
}

#[test]
fn associated_type_in_a_trait_is_accepted_in_stage3() {
    let result = Runtime::new().compile(
        r#"
trait Container {
    type Item;
    fn count(self) -> int;
}
"#,
        CompileOptions::default(),
    );
    assert!(
        result.is_ok(),
        "Stage 3 associated type should compile: {:?}",
        result.err()
    );
}

#[test]
fn associated_constant_in_a_trait_is_accepted_in_stage3() {
    let result = Runtime::new().compile(
        r#"
trait Limits {
    const MAX: int;
    fn ceiling(self) -> int;
}
"#,
        CompileOptions::default(),
    );
    assert!(
        result.is_ok(),
        "Stage 3 associated constant should compile: {:?}",
        result.err()
    );
}

#[test]
fn associated_type_in_an_impl_is_accepted_in_stage3() {
    let result = Runtime::new().compile(
        r#"
trait Source {
    type Item;
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int;
}
"#,
        CompileOptions::default(),
    );
    assert!(
        result.is_ok(),
        "Stage 3 associated type definition should compile: {:?}",
        result.err()
    );
}

#[test]
fn trait_object_is_named_and_deferred() {
    let message = compile_message(
        r#"
trait Shape {
    fn area(self) -> int;
}
fn paint(shape: dyn Shape) -> int { 0 }
"#,
    );
    assert_deferred(
        &message,
        "error[E0113]",
        "trait object 'dyn Shape' is deferred to Stage 3",
    );
}

#[test]
fn negative_impl_is_named_and_deferred() {
    let message = compile_message(
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Point { x: int }
impl !Scorable for Point {}
"#,
    );
    assert_deferred(
        &message,
        "error[E0114]",
        "negative impl is deferred to Stage 3",
    );
}

#[test]
fn specialization_in_an_impl_is_named_and_deferred() {
    let message = compile_message(
        r#"
struct Point { x: int }
impl Point {
    default fn score(self) -> int { self.x }
}
"#,
    );
    assert_deferred(
        &message,
        "error[E0115]",
        "specialization with 'default fn' is deferred to Stage 3",
    );
}

#[test]
fn specialization_in_a_trait_is_named_and_deferred() {
    let message = compile_message(
        r#"
trait Scorable {
    default fn score(self) -> int { 0 }
}
"#,
    );
    assert_deferred(
        &message,
        "error[E0115]",
        "specialization with 'default fn' is deferred to Stage 3",
    );
}

// the rejection above must not be satisfied by breaking receivers altogether
#[test]
fn by_value_self_receiver_still_parses_and_runs() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Point { x: int }
impl Scorable for Point {
    fn score(self) -> int { self.x + 1 }
}
impl Point {
    fn shift(mut self, dx: int) -> int {
        self.x = self.x + dx
        self.x
    }
}
fn use_by_value_self() -> int {
    let point = Point { x: 6 }
    let mut moved = Point { x: 2 }
    point.score() + moved.shift(3)
}
"#,
        "by-value-self",
    )
    .expect("by-value self should compile");

    assert_eq!(
        call_function(&mut vm, "use_by_value_self", &[]).expect("use_by_value_self"),
        Value::int(12)
    );
}
