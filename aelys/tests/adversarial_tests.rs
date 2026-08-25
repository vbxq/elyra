mod common;
use aelys_runtime::{Function, OpCode, VM, Value};
use aelys_syntax::Source;
use common::*;

#[test]
fn malicious_bytecode_invalid_opcode() {
    let mut vm = VM::new(Source::new("test.aelys", "")).unwrap();
    let mut func = Function::new(Some("malicious".to_string()), 0);
    func.num_registers = 1;

    func.set_bytecode(vec![0xDEADBEEF]);

    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_err());
}

#[test]
fn bytecode_constant_pool_oob() {
    let mut vm = VM::new(Source::new("test.aelys", "")).unwrap();
    let mut func = Function::new(Some("const_oob".to_string()), 0);
    func.num_registers = 1;
    func.constants.push(Value::int(42).into());

    func.emit_a(OpCode::LoadK, 0, 5, 0, 1);
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);

    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_err());
}

#[test]
fn bytecode_jump_to_invalid_address() {
    let mut vm = VM::new(Source::new("test.aelys", "")).unwrap();
    let mut func = Function::new(Some("bad_jump".to_string()), 0);
    func.num_registers = 1;

    func.emit_b(OpCode::Jump, 0, 1000, 1);
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);

    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_err());
}

#[test]
fn bytecode_negative_jump() {
    let mut vm = VM::new(Source::new("test.aelys", "")).unwrap();
    let mut func = Function::new(Some("neg_jump".to_string()), 0);
    func.num_registers = 1;

    func.emit_b(OpCode::Jump, 0, -100, 1);
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);

    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_err());
}

// (path_traversal_dotdot → security_audit_tests::fs_join_rejects_parent_escape)

#[test]
fn path_traversal_url_encoded() {
    let code = r#"
needs std::fs
match fs::join("/app", "..%2F..%2Fetc%2Fpasswd") { Ok(_) => 0, Err(_) => 1 }
"#;
    let result = run_aelys(code);
    assert!(result.as_int().is_some(), "expected a handled Result");
}

#[test]
fn type_confusion_null_as_int() {
    let code = r#"
let x = null
    x + 5
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("null is not part of Aelys"));
}

#[test]
fn type_confusion_string_as_number() {
    let code = r#"
let x = "hello"
x * 2
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("type") || err.contains("Type"));
}

#[test]
fn type_confusion_bool_arithmetic() {
    let code = r#"
let x = true
x + 10
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("type") || err.contains("Type"));
}

#[test]
#[ignore]
fn infinite_string_concat_oom() {
    let code = r#"
let mut s = "x"
let mut i = 0
while i < 100000 {
    s = s + s
    i++
}
42
"#;
    let result = run_aelys_result(code);
    let _ = result;
}

#[test]
#[ignore]
fn deep_recursion_stack_overflow() {
    let code = r#"
fn recurse(n) {
    return recurse(n + 1)
}
recurse(0)
"#;
    let err = run_aelys_err(code);
    assert!(
        err.contains("stack")
            || err.contains("frame")
            || err.contains("recursion")
            || err.contains("Stack")
    );
}

#[test]
#[ignore]
fn allocation_bomb() {
    let code = r#"
let mut i = 0
while i < 100000 {
    let p = alloc(1000)
    i++
}
42
"#;
    let _ = run_aelys_result(code);
}

#[test]
fn integer_overflow_checked() {
    let code = r#"
let max = 140737488355327
max + 1
"#;
    let error = run_aelys_result(code).expect_err("integer overflow must be rejected");
    assert!(error.contains("integer overflow"));
}

#[test]
fn integer_multiply_overflow() {
    let code = r#"
let big = 10000000000
big * big
"#;
    let error = run_aelys_result(code).expect_err("integer overflow must be rejected");
    assert!(error.contains("integer overflow"));
}

#[test]
fn time_format_string_attack() {
    let code = r#"
format("%n%n%n%n%n")
"#;
    let _ = run_aelys(code);
}

#[test]
fn binary_oversized_function_count() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VBXQ");
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    bytes.extend_from_slice(&0u16.to_le_bytes()); // name len
    bytes.extend_from_slice(&0u16.to_le_bytes()); // arity
    bytes.extend_from_slice(&0u32.to_le_bytes()); // num_registers
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes()); // constants
    bytes.extend_from_slice(&1u32.to_le_bytes()); // bytecode length
    bytes.extend_from_slice(&0u32.to_le_bytes()); // Return0
    bytes.extend_from_slice(&5000u16.to_le_bytes()); // nested functions

    let result = aelys_bytecode::asm::deserialize(&bytes);
    assert!(result.is_err());
}

#[test]
fn binary_oversized_constants() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"VBXQ");
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&1_000_001u32.to_le_bytes());

    let result = aelys_bytecode::asm::deserialize(&bytes);
    assert!(result.is_err());
}

#[test]
fn unicode_bidi_override_attack() {
    let code = r#"
let safe = "hello"
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn unicode_homoglyph_attack() {
    let code = r#"
let х = 42
х
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn division_by_zero_direct() {
    let code = "10 / 0";
    assert_aelys_error_contains(code, "division");
}

#[test]
fn division_by_zero_variable() {
    let code = r#"
let x = 0
10 / x
"#;
    assert_aelys_error_contains(code, "division");
}

#[test]
fn modulo_by_zero() {
    let code = "10 % 0";
    assert_aelys_error_contains(code, "division");
}

#[test]
fn gc_collection_during_critical_section() {
    let code = r#"
let mut i = 0
while i < 5000 {
    let s1 = "string" + " concatenation"
    let s2 = "more " + "strings"
    let s3 = s1 + s2
    i++
}
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn function_modification_attempt() {
    let code = r#"
fn test() { return 42 }
let result = test()
result
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn null_function_call() {
    let code = r#"
let f = null
f()
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("null is not part of Aelys"));
}
