mod common;

use aelys::{CompileOptions, Runtime};
use common::run_aelys;

fn run_ok(source: &str) -> aelys_runtime::Value {
    run_aelys(source)
}

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn println_accepts_the_display_scalars() {
    let value = run_ok(
        r#"
use std::io
fn probe() -> int {
    println(1)
    println("hello")
    println(true)
    println(2.5)
    7
}
probe()
"#,
    );
    assert_eq!(value.as_int(), Some(7));
}

#[test]
fn convert_to_string_and_tostring_accept_display_scalars() {
    let value = run_ok(
        r#"
use std::string
fn probe() -> int {
    let a: string = 123.to_string()
    let b: string = __tostring(true)
    let c: string = "already".to_string()
    string::len(a) + string::len(b) + string::len(c)
}
probe()
"#,
    );
    assert_eq!(value.as_int(), Some(3 + 4 + 7));
}

#[test]
fn convert_scalar_conversions_still_run() {
    let value = run_ok(
        r#"
fn probe() -> int {
    let a: int = "41".to_int().unwrap_or(0)
    let b: float = 1 as float
    a + b as int
}
probe()
"#,
    );
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn reflective_natives_are_gone() {
    for call in [
        "convert::type_of(Marker { v: 1 })",
        "convert::is_int(1)",
        "convert::to_bool(Marker { v: 1 })",
    ] {
        let message = compile_message(&format!("struct Marker {{ v: int }}\nlet x = {call}\n"));
        assert!(message.contains("error[E0449]"), "{call}: {message}");
    }
}

#[test]
fn println_rejects_a_type_with_no_display_provision() {
    let message = compile_message(
        r#"
use std::io
struct Silent { v: int }
fn probe() -> int {
    println(Silent { v: 1 })
    7
}
probe()
"#,
    );
    assert!(
        message.contains("error[E0338]"),
        "expected E0338: {message}"
    );
    assert!(message.contains("Display"), "expected the bound: {message}");
}

#[test]
fn tostring_rejects_a_type_with_no_display_provision() {
    let message = compile_message(
        r#"
use std::string
struct Silent { v: int }
fn probe() -> int {
    let text: string = __tostring(Silent { v: 1 })
    string::len(text)
}
probe()
"#,
    );
    assert!(
        message.contains("error[E0338]"),
        "expected E0338: {message}"
    );
}

#[test]
fn a_display_impl_satisfies_the_println_bound() {
    let message = Runtime::new()
        .compile(
            r#"
use std::io
struct Loud { v: int }
impl Display for Loud {
    fn to_display(self) -> string { "loud" }
}
fn probe() -> int {
    println(Loud { v: 1 })
    7
}
probe()
"#,
            CompileOptions::default(),
        )
        .err()
        .map(|error| error.to_string());
    assert_eq!(message, None, "a Display impl must satisfy the bound");
}

#[test]
fn convert_to_int_rejects_a_non_scalar_source() {
    let message = compile_message(
        r#"
struct Silent { v: int }
fn probe() -> int {
    (Silent { v: 1 }) as int
}
probe()
"#,
    );
    assert!(
        message.contains("error[E0301]"),
        "expected E0301: {message}"
    );
}

#[test]
fn a_generic_display_bound_is_proved_for_a_scalar_instance() {
    let value = run_ok(
        r#"
use std::string
fn render<T>(value: T) -> string where T: Display { value.to_string() }
fn probe() -> int {
    string::len(render(123))
}
probe()
"#,
    );
    assert_eq!(value.as_int(), Some(3));
}
