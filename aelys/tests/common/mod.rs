#![allow(dead_code)]

use aelys::{new_vm, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::Value;

pub fn run_aelys(source: &str) -> Value {
    let mut vm = new_vm().expect("Failed to create VM");
    run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .expect("Aelys execution should succeed")
}

pub fn run_aelys_ok(source: &str) -> Value {
    let mut vm = new_vm().expect("Failed to create VM");
    run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .expect("Expected success but got error")
}

pub fn run_aelys_err(source: &str) -> String {
    let mut vm = new_vm().expect("Failed to create VM");
    match run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard) {
        Ok(v) => panic!("Expected error but got success: {:?}", v),
        Err(e) => e.to_string(),
    }
}

pub fn assert_aelys_int(source: &str, expected: i64) {
    let result = run_aelys(source);
    assert_eq!(
        result.as_int(),
        Some(expected),
        "Expected int {} but got {:?}",
        expected,
        result
    );
}

pub fn assert_aelys_bool(source: &str, expected: bool) {
    let result = run_aelys(source);
    assert_eq!(
        result.as_bool(),
        Some(expected),
        "Expected bool {} but got {:?}",
        expected,
        result
    );
}

pub fn assert_aelys_null(source: &str) {
    let result = run_aelys(source);
    assert!(result.is_null(), "Expected null but got {:?}", result);
}

pub fn assert_aelys_str(source: &str, expected: &str) {
    use aelys::new_vm;
    let mut vm = new_vm().expect("Failed to create VM");
    let result = aelys::run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .expect("Aelys execution should succeed");
    if let Some(ptr) = result.as_ptr() {
        let heap = vm.heap();
        if let Some(obj) = heap.get(aelys_runtime::vm::GcRef::new(ptr))
            && let aelys_runtime::vm::ObjectKind::String(s) = &obj.kind
        {
            assert_eq!(
                s.as_str(),
                expected,
                "Expected string '{}' but got '{}'",
                expected,
                s.as_str()
            );
            return;
        }
    }
    panic!("Expected string '{}' but got {:?}", expected, result);
}

pub fn assert_aelys_error_contains(source: &str, expected_substring: &str) {
    let err = run_aelys_err(source);
    assert!(
        err.contains(expected_substring),
        "Expected error containing '{}' but got: {}",
        expected_substring,
        err
    );
}

pub fn run_aelys_result(source: &str) -> Result<Value, String> {
    let mut vm = new_vm().expect("Failed to create VM");
    run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .map_err(|e| e.to_string())
}

fn located_line(message: &str) -> Option<u32> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse::<u32>().ok())
}

// a dummy span renders as line 0, which every other check a compile-fail
pub fn assert_located(code: &str, message: &str) {
    assert!(
        message.contains("-->") && message.contains('^'),
        "{code} must render a source span with a caret: {message}"
    );
    assert!(
        located_line(message).is_some_and(|line| line > 0),
        "{code} must point at a real source line, not a dummy span: {message}"
    );
}

pub fn assert_located_at(code: &str, message: &str, line: u32) {
    assert_located(code, message);
    assert_eq!(
        located_line(message),
        Some(line),
        "{code} must point at line {line}: {message}"
    );
}

pub fn assert_associated_diagnostic(
    message: &str,
    code: &str,
    subject: &str,
    reason: &str,
    help: &str,
) {
    assert!(message.contains(code), "expected {code}, got: {message}");
    assert_located(code, message);
    assert!(
        message.contains(subject),
        "{code} must name '{subject}': {message}"
    );
    assert!(
        message.contains(help),
        "{code} must offer the corrective clause '{help}': {message}"
    );
    let states_its_reason = code
        .strip_prefix("E0")
        .and_then(|digits| digits.parse::<u16>().ok())
        .is_some_and(|code| (421..=430).contains(&code) || code == 434);
    assert_eq!(
        states_its_reason,
        !reason.is_empty(),
        "{code} was given the reason '{reason}'"
    );
    if states_its_reason {
        assert!(
            message.contains(&format!("({reason})")),
            "{code} must state the reason '{reason}': {message}"
        );
    }
}
