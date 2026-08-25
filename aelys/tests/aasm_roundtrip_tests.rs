use aelys_backend::Compiler;
use aelys_bytecode::asm::{assemble, deserialize, disassemble, serialize};
use aelys_bytecode::{
    EnumFieldSchema, EnumSchema, EnumVariantSchema, Function as BcFunction, IntWidth, OpCode,
    StructFieldSchema, StructSchema, TypeDescriptor,
};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_runtime::{VM, Value};
use aelys_syntax::Source;

fn run_source(source: &str) -> Value {
    aelys::run(source, "<test>").expect("Execution failed")
}

fn compile_source(source: &str) -> aelys_runtime::Function {
    let src = Source::new("<test>", source);
    let tokens = Lexer::with_source(src.clone())
        .scan()
        .expect("Lexer failed");
    let stmts = Parser::new(tokens, src.clone())
        .parse()
        .expect("Parser failed");

    let typed_program = aelys_sema::TypeInference::infer_program(stmts, src.clone())
        .expect("Type inference failed");
    let (func, _globals) = Compiler::new(None, src)
        .compile_typed(&typed_program)
        .expect("Compiler failed");

    func
}

fn run_function(func: aelys_runtime::Function) -> Value {
    let src = Source::new("<test>", "");
    let mut vm = VM::new(src).unwrap();

    let func_ref = vm.alloc_function(func).unwrap();
    vm.execute(func_ref).expect("Execution failed")
}

#[test]
fn test_asm_roundtrip_simple() {
    let source = "42";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let functions = assemble(&asm_text).expect("Assemble failed");
    let result_roundtrip = run_function(functions.into_iter().next().unwrap());

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
}

#[test]
fn test_bytecode_asm_roundtrip_api() {
    let src = r#"
.function 0
  .name "main"
  .arity 0
  .registers 0

  .code
    0000: Return0
"#;
    let functions = aelys_bytecode::asm::assemble(src).expect("Assemble failed");
    let text = aelys_bytecode::asm::disassemble(&functions[0]);
    assert!(text.contains(".function 0"));
}

#[test]
fn test_asm_roundtrip_arithmetic() {
    let source = "(10 + 5) * 2 - 3";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let functions = assemble(&asm_text).expect("Assemble failed");
    let result_roundtrip = run_function(functions.into_iter().next().unwrap());

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
    assert_eq!(result_direct.as_int(), Some(27));
}

#[test]
fn test_asm_roundtrip_conditionals() {
    let source = "10 > 5"; // Returns true/false - simpler test for jumps
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let functions = assemble(&asm_text).expect("Assemble failed");
    let result_roundtrip = run_function(functions.into_iter().next().unwrap());

    assert_eq!(result_direct.as_bool(), result_roundtrip.as_bool());
    assert_eq!(result_direct.as_bool(), Some(true));
}

#[test]
fn test_asm_roundtrip_while_loop() {
    let source = r#"
        let mut sum = 0
        let mut i = 1
        while i <= 5 {
            sum += i
            i++
        }
        sum
    "#;
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let functions = assemble(&asm_text).expect("Assemble failed");
    let result_roundtrip = run_function(functions.into_iter().next().unwrap());

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
    assert_eq!(result_direct.as_int(), Some(15)); // 1+2+3+4+5
}

#[test]
fn test_binary_roundtrip_simple() {
    let source = "42";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
}

#[test]
fn test_binary_roundtrip_with_strings() {
    let source = r#"
        let x = "hello"
        42
    "#;
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
}

#[test]
fn test_binary_roundtrip_with_floats() {
    let source = "3.14159";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    let direct_f = result_direct.as_float().expect("Expected float");
    let roundtrip_f = result_roundtrip.as_float().expect("Expected float");
    assert!((direct_f - roundtrip_f).abs() < 0.00001);
}

#[test]
fn test_binary_roundtrip_conditionals() {
    let source = r#"
        let x = 10
        if x > 5 { x + 1 } else { x - 1 }
    "#;
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
}

#[test]
fn test_double_roundtrip() {
    let source = r#"
        let x = 10
        let y = 20
        x + y
    "#;
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let asm_funcs = assemble(&asm_text).expect("Assemble failed");

    let bytes = serialize(&asm_funcs[0]).unwrap();
    let final_func = deserialize(&bytes).expect("Deserialize failed");
    let result_final = run_function(final_func);

    assert_eq!(result_direct.as_int(), result_final.as_int());
    assert_eq!(result_direct.as_int(), Some(30));
}

#[test]
fn test_empty_function() {
    let func = aelys_runtime::Function::new(Some("empty".to_string()), 0);
    let asm_text = disassemble(&func);
    assert!(asm_text.contains(".function 0"));
    assert!(asm_text.contains(".name \"empty\""));

    let bytes = serialize(&func).unwrap();
    let loaded = deserialize(&bytes).expect("Deserialize failed");
    assert_eq!(loaded.name, Some("empty".to_string()));
}

#[test]
fn test_negative_numbers() {
    let source = "-42";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let asm_text = disassemble(&func);
    let functions = assemble(&asm_text).expect("Assemble failed");
    let result_roundtrip = run_function(functions.into_iter().next().unwrap());

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
    assert_eq!(result_direct.as_int(), Some(-42));
}

#[test]
fn test_large_numbers() {
    let source = "123456789";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    assert_eq!(result_direct.as_int(), result_roundtrip.as_int());
}

#[test]
fn test_booleans() {
    let source = "true";
    let result_direct = run_source(source);

    let func = compile_source(source);
    let bytes = serialize(&func).unwrap();
    let loaded_func = deserialize(&bytes).expect("Deserialize failed");
    let result_roundtrip = run_function(loaded_func);

    assert_eq!(result_direct.as_bool(), result_roundtrip.as_bool());
    assert_eq!(result_direct.as_bool(), Some(true));
}

#[test]
fn test_null_literal_is_rejected() {
    let error = aelys::run("null", "<test>").expect_err("null is not surface syntax");
    assert!(error.to_string().contains("null is not part of Aelys"));
}

#[test]
fn test_string_escaping() {
    let source = r#""hello\nworld""#;
    let func = compile_source(source);

    let asm_text = disassemble(&func);
    assert!(asm_text.contains("\\n")); // Should be escaped

    let bytes = serialize(&func).unwrap();
    deserialize(&bytes).expect("Deserialize failed");
}

fn struct_op_function(name: &str, op: OpCode, a: u16, b: u16, c: u16) -> BcFunction {
    let mut function = BcFunction::new(Some(name.to_string()), 0);
    function.num_registers = 4;
    function.struct_schemas = vec![StructSchema::new(
        "Point".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "x".to_string(),
            ty: TypeDescriptor::Int(IntWidth::I64),
        }],
    )];
    function.jit_unsupported_struct = true;
    function.emit_struct(op, 0, a, b, c, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn enum_op_function(name: &str, op: OpCode, a: u16, b: u16, c: u16, d: u16) -> BcFunction {
    let mut function = BcFunction::new(Some(name.to_string()), 0);
    function.num_registers = 4;
    function.enum_schemas = vec![EnumSchema::new(
        "test::Shape".to_string(),
        vec![EnumVariantSchema {
            variant_id: 0,
            name: "Pair".to_string(),
            fields: vec![EnumFieldSchema {
                offset: 0,
                name: None,
                ty: TypeDescriptor::Int(IntWidth::I64),
            }]
            .into_boxed_slice(),
        }],
    )];
    function.jit_unsupported_struct = true;
    function.emit_enum(op, 0, a, b, c, d, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn assert_aasm_words_identical(function: &BcFunction) {
    let assembly = disassemble(function);
    let assembled = match assemble(&assembly) {
        Ok(assembled) => assembled,
        Err(error) => panic!(
            "reassembling disassembled text failed: {error}\n--- assembly ---\n{assembly}\n--- end ---"
        ),
    };
    assert_eq!(
        assembled[0].bytecode.as_slice(),
        function.bytecode.as_slice(),
        "aasm round-trip changed the bytecode words\n--- assembly ---\n{assembly}\n--- end ---"
    );
}

#[test]
fn test_aasm_roundtrip_struct_new_is_word_identical() {
    assert_aasm_words_identical(&struct_op_function(
        "struct_new",
        OpCode::StructNew,
        1,
        0,
        1,
    ));
}

#[test]
fn test_aasm_roundtrip_struct_load_is_word_identical() {
    assert_aasm_words_identical(&struct_op_function(
        "struct_load",
        OpCode::StructLoad,
        0,
        1,
        0,
    ));
}

#[test]
fn test_aasm_roundtrip_struct_store_is_word_identical() {
    assert_aasm_words_identical(&struct_op_function(
        "struct_store",
        OpCode::StructStore,
        1,
        0,
        0,
    ));
}

#[test]
fn test_aasm_roundtrip_enum_new_is_word_identical() {
    assert_aasm_words_identical(&enum_op_function("enum_new", OpCode::EnumNew, 1, 0, 0, 1));
}

#[test]
fn test_aasm_roundtrip_enum_test_is_word_identical() {
    assert_aasm_words_identical(&enum_op_function("enum_test", OpCode::EnumTest, 1, 0, 0, 0));
}

#[test]
fn test_aasm_roundtrip_enum_load_is_word_identical() {
    assert_aasm_words_identical(&enum_op_function("enum_load", OpCode::EnumLoad, 0, 1, 0, 0));
}
