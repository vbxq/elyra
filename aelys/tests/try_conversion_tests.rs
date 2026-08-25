use aelys::{CompileOptions, Runtime, call_function, new_vm, run_with_vm};
use aelys_runtime::Value;

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn user_from_impl_drives_the_question_mark_failure_path() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct MyErr { code: int }
struct OtherErr { code: int }

impl From<MyErr> for OtherErr {
    fn from(source: MyErr) -> OtherErr { OtherErr { code: source.code + 100 } }
}

fn read() -> Result<int, MyErr> { Err(MyErr { code: 7 }) }

fn convert() -> Result<int, OtherErr> {
    let value = read()?
    Ok(value)
}

fn converted_code() -> int {
    match convert() {
        Ok(_) => 0,
        Err(problem) => problem.code,
    }
}
"#,
        "try-user-from",
    )
    .expect("the user From impl program should compile");

    assert_eq!(
        call_function(&mut vm, "converted_code", &[]).expect("converted_code"),
        Value::int(107)
    );
}

#[test]
fn identity_conversion_still_propagates_the_same_error_type() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct MyErr { code: int }

fn read() -> Result<int, MyErr> { Err(MyErr { code: 5 }) }

fn pass() -> Result<int, MyErr> {
    let value = read()?
    Ok(value)
}

fn passed_code() -> int {
    match pass() {
        Ok(_) => 0,
        Err(problem) => problem.code,
    }
}
"#,
        "try-identity",
    )
    .expect("the identity conversion program should compile");

    assert_eq!(
        call_function(&mut vm, "passed_code", &[]).expect("passed_code"),
        Value::int(5)
    );
}

#[test]
fn the_string_to_error_conversion_still_works() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
fn read() -> Result<int, string> { Err("bad") }

fn convert() -> Result<int, Error> {
    let value = read()?
    Ok(value)
}

fn message_matches() -> int {
    match convert() {
        Ok(_) => 0,
        Err(Error::Message(text)) => if text == "bad" { 1 } else { 0 },
    }
}
"#,
        "try-string-to-error",
    )
    .expect("the string to Error program should compile");

    assert_eq!(
        call_function(&mut vm, "message_matches", &[]).expect("message_matches"),
        Value::int(1)
    );
}

#[test]
fn two_from_impls_for_one_destination_dispatch_independently() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
enum Low { Io(int), Parse(int) }
struct High { code: int }

impl From<Low> for High {
    fn from(source: Low) -> High {
        match source {
            Low::Io(n) => High { code: n + 10 },
            Low::Parse(n) => High { code: n + 20 },
        }
    }
}

impl From<string> for High {
    fn from(source: string) -> High { High { code: source.len() + 300 } }
}

fn low_fail() -> Result<int, Low> { Err(Low::Parse(5)) }
fn text_fail() -> Result<int, string> { Err("abcd") }

fn from_low() -> Result<int, High> {
    let value = low_fail()?
    Ok(value)
}

fn from_text() -> Result<int, High> {
    let value = text_fail()?
    Ok(value)
}

fn low_code() -> int {
    match from_low() {
        Ok(_) => 0,
        Err(problem) => problem.code,
    }
}

fn text_code() -> int {
    match from_text() {
        Ok(_) => 0,
        Err(problem) => problem.code,
    }
}
"#,
        "try-two-impls",
    )
    .expect("two From impls for one destination should compile");

    assert_eq!(
        call_function(&mut vm, "low_code", &[]).expect("low_code"),
        Value::int(25)
    );
    assert_eq!(
        call_function(&mut vm, "text_code", &[]).expect("text_code"),
        Value::int(304)
    );
}

#[test]
fn a_missing_conversion_is_reported_by_code() {
    let message = compile_message(
        r#"
struct MyErr { code: int }
struct OtherErr { code: int }

fn read() -> Result<int, MyErr> { Err(MyErr { code: 1 }) }

fn convert() -> Result<int, OtherErr> {
    let value = read()?
    Ok(value)
}

let _ = convert()
"#,
    );
    assert!(message.contains("error[E0374]"), "{message}");
    assert!(message.contains("map_err"), "{message}");
}

#[test]
fn two_candidate_impls_are_reported_by_code() {
    let message = compile_message(
        r#"
struct MyErr { code: int }
struct AltErr { code: int }
struct Target { code: int }

impl From<MyErr> for Target {
    fn from(source: MyErr) -> Target { Target { code: source.code } }
}

impl From<AltErr> for Target {
    fn from(source: AltErr) -> Target { Target { code: source.code } }
}

fn pass<T>(input: Result<int, T>) -> Result<int, Target> {
    let value = input?
    Ok(value)
}

0
"#,
    );
    assert!(message.contains("error[E0374]"), "{message}");
    assert!(message.contains("From<MyErr> for Target"), "{message}");
    assert!(message.contains("From<AltErr> for Target"), "{message}");
}

#[test]
fn an_option_residual_in_a_result_function_is_reported_by_code() {
    let message = compile_message(
        r#"
fn maybe() -> Option<int> { Some(1) }

fn convert() -> Result<int, string> {
    let value = maybe()?
    Ok(value)
}

let _ = convert()
"#,
    );
    assert!(message.contains("error[E0373]"), "{message}");
}

#[test]
fn a_user_identity_from_impl_is_rejected_by_code() {
    let message = compile_message(
        r#"
struct MyErr { code: int }

impl From<MyErr> for MyErr {
    fn from(source: MyErr) -> MyErr { source }
}

0
"#,
    );
    assert!(message.contains("error[E0375]"), "{message}");
}
