use aelys_bytecode::asm::disasm::escape_string;
use aelys_bytecode::asm::{assemble, deserialize, disassemble, serialize};
use aelys_runtime::{Function, Value};

#[test]
fn test_escape_string() {
    assert_eq!(escape_string("hello"), "hello");
    assert_eq!(escape_string("hello\nworld"), "hello\\nworld");
    assert_eq!(escape_string("tab\there"), "tab\\there");
    assert_eq!(escape_string("quote\"here"), "quote\\\"here");
    assert_eq!(escape_string("back\\slash"), "back\\\\slash");
}

#[test]
fn test_disassemble_empty_function() {
    let func = Function::new(Some("test".to_string()), 0);
    let output = disassemble(&func);
    assert!(output.contains(".function 0"));
    assert!(output.contains(".name \"test\""));
    assert!(output.contains(".arity 0"));
}

#[test]
fn test_basic_assembly() {
    let source = r#"
.version 3

.function 0
  .name "main"
  .arity 0
  .registers 2

  .code
    0000: LoadI     r0, 42
    0001: Return0
"#;
    let functions = assemble(source).unwrap();
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, Some("main".to_string()));
    assert_eq!(functions[0].arity, 0);
    assert_eq!(functions[0].bytecode.len(), 2);
}

#[test]
fn test_label_resolution() {
    let source = r#"
.function 0
  .arity 0
  .registers 1

  .code
    0000: JumpIfNot r0, L0
    0001: LoadI     r0, 1
    0002: Jump      L1
  L0:
    0003: LoadI     r0, 0
  L1:
    0004: Return    r0
"#;
    let functions = assemble(source).unwrap();
    assert_eq!(functions[0].bytecode.len(), 5);
}

#[test]
fn test_binary_basic_roundtrip() {
    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 2;
    func.set_bytecode(vec![0x01_00_00_2A]); // LoadI r0, 42

    let bytes = serialize(&func).unwrap();

    assert_eq!(&bytes[0..4], b"VBXQ");

    let loaded = deserialize(&bytes).unwrap();
    assert_eq!(loaded.name, Some("test".to_string()));
    assert_eq!(loaded.arity, 0);
    assert_eq!(loaded.num_registers, 2);
    assert_eq!(loaded.bytecode, func.bytecode);
}

#[test]
fn test_binary_with_constants() {
    let mut func = Function::new(None, 0);
    func.constants = vec![
        Value::int(42).into(),
        Value::float(2.72).into(),
        Value::bool(true).into(),
        Value::null().into(),
    ];

    let bytes = serialize(&func).unwrap();
    let loaded = deserialize(&bytes).unwrap();

    assert_eq!(loaded.constants.len(), 4);
    assert_eq!(loaded.constants[0].as_int(), Some(42));
    assert!((loaded.constants[1].as_float().unwrap() - 2.72).abs() < 0.001);
    assert_eq!(loaded.constants[2].as_bool(), Some(true));
    assert!(loaded.constants[3].is_null());
}

#[test]
fn serializer_rejects_lengths_that_do_not_fit_v2_fields() {
    let function = Function::new(Some("x".repeat(usize::from(u16::MAX) + 1)), 0);
    let error = serialize(&function).expect_err("oversized name must be rejected");
    assert!(error.to_string().contains("function name length"));

    let mut function = Function::new(None, 0);
    function.upvalue_descriptors = vec![
        aelys_bytecode::UpvalueDescriptor {
            is_local: true,
            index: 0,
        };
        257
    ];
    let error = serialize(&function).expect_err("oversized upvalue table must be rejected");
    assert!(error.to_string().contains("upvalue descriptor count"));
}
