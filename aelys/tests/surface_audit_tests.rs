use aelys::{CompileOptions, Runtime, run};

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected by the final surface audit"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn unresolved_type_variable_is_rejected_by_the_final_audit() {
    let message = compile_message("fn identity(x) { x }\n1");
    assert!(
        message.contains("error[E0353]"),
        "expected E0353: {message}"
    );
}

#[test]
fn unresolved_type_variable_in_an_impl_is_rejected_by_the_final_audit() {
    let message = compile_message(
        "struct Counter { total: int }\nimpl Counter {\n    fn ignore(self, value) -> int { 1 }\n}\n1",
    );
    assert!(
        message.contains("error[E0353]"),
        "expected E0353: {message}"
    );
}

#[test]
fn inferred_struct_member_access_is_resolved_after_constraints() {
    let value = run(
        "struct Point { x: int }\nfn read(value) -> int { value.x }\nread(Point { x: 1 })",
        "test.aelys",
    )
    .expect("member lookup must wait for the argument constraint");
    assert_eq!(value.as_int(), Some(1));
}

#[test]
fn generic_struct_declaration_is_audited_not_skipped() {
    let message = compile_message("struct Holder<T> { value: dynamic }\n1");
    assert!(
        message.contains("error[E0347]"),
        "expected E0347: {message}"
    );
}

#[test]
fn generic_impl_with_a_forbidden_type_is_rejected() {
    let message = compile_message(
        "struct Holder<T> { value: T }\nimpl<T> Holder<T> {\n    fn get(self) -> T { self.value }\n    fn ignore(self, other) -> int { 1 }\n}\nlet holder = Holder { value: 1 }\nholder.get()",
    );
    assert!(
        message.contains("error[E0353]"),
        "expected E0353: {message}"
    );
}

#[test]
fn generic_declarations_with_parameter_fields_still_compile() {
    let value = run(
        "struct Holder<T> { value: T }\nenum Slot<T> { Full(T), Empty }\nlet holder = Holder { value: 7 }\nholder.value",
        "test.aelys",
    )
    .expect("a generic declaration bound to its own parameters must still compile");
    assert_eq!(value.as_int(), Some(7));
}

#[test]
fn untyped_parameters_still_compile_and_run() {
    let value = run("fn add(a, b) { a + b }\nadd(2, 3)", "test.aelys")
        .expect("an untyped parameter program must still compile and run");
    assert_eq!(value.as_int(), Some(5));
}
