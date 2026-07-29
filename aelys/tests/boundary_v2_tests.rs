use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime};
use aelys_backend::Compiler;
use aelys_bytecode::asm::{deserialize, serialize};
use aelys_bytecode::{Function, OpCode, Register};
use aelys_runtime::{VM, Value};
use aelys_syntax::Source;

fn balanced_sum(registers: &[String]) -> String {
    if registers.len() == 1 {
        return registers[0].clone();
    }
    let middle = registers.len() / 2;
    format!(
        "({} + {})",
        balanced_sum(&registers[..middle]),
        balanced_sum(&registers[middle..])
    )
}

#[test]
fn register_allocator_covers_the_full_u16_range() {
    let source = Source::new("register-boundary", "");
    let mut compiler = Compiler::new(None, source);
    for expected in 0..=u16::MAX {
        assert_eq!(compiler.alloc_register().unwrap(), expected);
    }
    assert!(compiler.alloc_register().is_err());
    assert_eq!(compiler.next_register, 65_536);
}

#[test]
fn source_compiler_emits_and_executes_register_256() {
    let mut source = String::from("fn wide_locals() {\n");
    for register in 0..=256 {
        source.push_str(&format!("let v{register} = {register};\n"));
    }
    source.push_str("return ");
    let registers: Vec<_> = (0..=256).map(|register| format!("v{register}")).collect();
    source.push_str(&balanced_sum(&registers));
    source.push_str("\n}\nwide_locals()");

    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let compiled = deserialize(module.avbc()).unwrap();
    let function = &compiled.nested_functions[0];
    assert!(function.num_registers > 256);
    assert!(
        function
            .bytecode
            .iter()
            .any(|word| (word >> 24) == u32::from(OpCode::Wide as u8))
    );
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("wide register source unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(32_896));
}

#[test]
fn call_with_255_arguments_reserves_256_register_slots_without_wrapping() {
    let parameters = (0..255)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let arguments = (0..255)
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn pick({parameters}) {{ return p254 }}\npick({arguments})");

    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("maximum-arity call unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(254));
}

#[test]
fn call_with_256_arguments_uses_wide_arity_and_executes() {
    let parameters = (0..256)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let arguments = (0..256)
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("fn pick({parameters}) {{ return p255 }}\npick({arguments})");

    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let compiled = deserialize(module.avbc()).unwrap();
    let assembly = aelys_bytecode::asm::disassemble(&compiled);
    assert!(assembly.contains("CallWide"));
    assert!(assembly.contains(", 256"));

    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("wide-arity call unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(255));
}

#[test]
fn call_wide_roundtrips_through_aasm() {
    let mut function = Function::new(None, 0);
    function.num_registers = 258;
    function.emit_call(
        Register::new(257),
        Register::new(0),
        aelys_bytecode::Arity::new(256),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        Register::new(257),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();

    let assembly = aelys_bytecode::asm::disassemble(&function);
    assert!(assembly.contains("CallWide  r257, r0, 256"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);
}

#[test]
fn array_and_vec_literals_with_256_elements_execute_with_wide_counts() {
    let elements = (0..256)
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let runtime = Runtime::new();
    for (source, opcode) in [
        (
            format!("let value = [{elements}]\nvalue[255]"),
            "ArrayLitWide",
        ),
        (
            format!("let value = Vec[{elements}]\nvalue[255]"),
            "VecLitWide",
        ),
    ] {
        let module = runtime
            .compile(
                &source,
                CompileOptions {
                    optimization_level: aelys_opt::OptimizationLevel::None,
                    ..CompileOptions::default()
                },
            )
            .unwrap();
        let compiled = deserialize(module.avbc()).unwrap();
        let assembly = aelys_bytecode::asm::disassemble(&compiled);
        assert!(assembly.contains(opcode));

        let mut isolate = runtime.new_isolate(IsolateConfig::default());
        let ExecutionOutcome::Returned(value) =
            isolate.execute(&module, RunOptions::default()).unwrap()
        else {
            panic!("wide literal execution unexpectedly exited");
        };
        assert_eq!(value.as_int(), Some(255));
    }

    let mut function = Function::new(None, 0);
    function.emit_counted_registers(
        OpCode::ArrayLit,
        OpCode::ArrayLitWide,
        Register::new(0),
        Register::new(1),
        256,
        1,
    );
    function.emit_counted_registers(
        OpCode::VecLit,
        OpCode::VecLitWide,
        Register::new(257),
        Register::new(1),
        256,
        1,
    );
    function.finalize_bytecode();
    let assembly = aelys_bytecode::asm::disassemble(&function);
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);
}

#[test]
fn invalid_opcode_gap_is_rejected_without_constructing_an_enum() {
    for opcode in [78, 104, 128, 129] {
        assert_eq!(OpCode::from_u8(opcode), None);
    }
}

#[test]
fn compact_global_integer_add_roundtrips_and_executes() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "let mut total = 0; for value in 0..10 { total += value } total",
            CompileOptions::default(),
        )
        .unwrap();
    let function = deserialize(module.avbc()).unwrap();
    assert!(
        function
            .bytecode
            .iter()
            .any(|word| (word >> 24) == u32::from(OpCode::AddGlobalI as u8))
    );
    let assembly = aelys_bytecode::asm::disassemble(&function);
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);

    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("global integer add unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(45));
}

#[test]
fn conditional_jumps_accept_register_256_and_roundtrip() {
    for (opcode, condition) in [(OpCode::JumpIf, true), (OpCode::JumpIfNot, false)] {
        let mut function = Function::new(None, 0);
        function.emit_b(OpCode::LoadI, 0, 42, 1);
        function.emit_wide_abc(
            OpCode::LoadBool,
            Register::new(256),
            Register::new(u16::from(condition)),
            Register::new(0),
            1,
        );
        let jump = function.emit_jump_if(opcode, 256, 1);
        function.emit_b(OpCode::LoadI, 0, 1, 1);
        function.patch_jump(jump);
        function.emit_a(OpCode::Return, 0, 0, 0, 1);
        function.finalize_bytecode();

        let assembly = aelys_bytecode::asm::disassemble(&function);
        assert!(assembly.contains("WideLong r256"));
        let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
        assert_eq!(assembled[0].bytecode, function.bytecode);

        let mut vm = VM::new(Source::new("wide-conditional.aelys", "")).unwrap();
        let function_ref = vm
            .alloc_function(assembled.into_iter().next().unwrap())
            .unwrap();
        assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(42));
    }
}

#[test]
fn wide_array_and_vec_accesses_roundtrip_and_execute() {
    for (literal, load, store, get, name) in [
        (
            OpCode::ArrayLit,
            OpCode::ArrayLoadI,
            OpCode::ArrayStoreI,
            OpCode::ArrayGetI,
            "wide-array-access.aelys",
        ),
        (
            OpCode::VecLit,
            OpCode::VecLoadI,
            OpCode::VecStoreI,
            OpCode::VecGetI,
            "wide-vec-access.aelys",
        ),
    ] {
        let wide_literal = if literal == OpCode::ArrayLit {
            OpCode::ArrayLitWide
        } else {
            OpCode::VecLitWide
        };
        let mut function = Function::new(None, 0);
        function.emit_b(OpCode::LoadI, 0, 10, 1);
        function.emit_b(OpCode::LoadI, 1, 20, 1);
        function.emit_b(OpCode::LoadI, 2, 1, 1);
        function.emit_b(OpCode::LoadI, 3, 42, 1);
        function.emit_counted_registers(
            literal,
            wide_literal,
            Register::new(256),
            Register::new(0),
            2,
            1,
        );
        function.emit_register_abc(
            OpCode::Move,
            Register::new(257),
            Register::new(2),
            Register::new(0),
            1,
        );
        function.emit_register_abc(
            OpCode::Move,
            Register::new(258),
            Register::new(3),
            Register::new(0),
            1,
        );
        function.emit_register_abc(
            load,
            Register::new(259),
            Register::new(256),
            Register::new(257),
            1,
        );
        function.emit_register_abc(
            store,
            Register::new(256),
            Register::new(257),
            Register::new(258),
            1,
        );
        function.emit_register_abc(
            get,
            Register::new(260),
            Register::new(256),
            Register::new(257),
            1,
        );
        function.emit_register_abc(
            OpCode::Add,
            Register::new(261),
            Register::new(259),
            Register::new(260),
            1,
        );
        function.emit_register_abc(
            OpCode::Return,
            Register::new(261),
            Register::new(0),
            Register::new(0),
            1,
        );
        function.finalize_bytecode();
        assert_eq!(function.num_registers, 262);

        let assembly = aelys_bytecode::asm::disassemble(&function);
        assert!(assembly.contains(&format!("Wide {}, r259, r256, r257", load as u8)));
        assert!(assembly.contains(&format!("Wide {}, r256, r257, r258", store as u8)));
        let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
        assert_eq!(assembled[0].bytecode, function.bytecode);

        let mut vm = VM::new(Source::new(name, "")).unwrap();
        let function_ref = vm.alloc_function(assembled[0].clone()).unwrap();
        assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(62));
    }
}

#[test]
fn wide_collection_management_operations_roundtrip_and_execute() {
    let mut array_function = Function::new(None, 0);
    array_function.emit_b(OpCode::LoadI, 0, 3, 1);
    array_function.emit_register_abc(
        OpCode::Move,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    array_function.emit_register_abc(
        OpCode::ArrayNewI,
        Register::new(257),
        Register::new(256),
        Register::new(0),
        1,
    );
    array_function.emit_register_abc(
        OpCode::ArrayLen,
        Register::new(258),
        Register::new(257),
        Register::new(0),
        1,
    );
    array_function.emit_register_abc(
        OpCode::Return,
        Register::new(258),
        Register::new(0),
        Register::new(0),
        1,
    );
    array_function.finalize_bytecode();

    let assembly = aelys_bytecode::asm::disassemble(&array_function);
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, array_function.bytecode);
    let mut vm = VM::new(Source::new("wide-array-management.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(assembled[0].clone()).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(3));

    let mut vec_function = Function::new(None, 0);
    vec_function.emit_b(OpCode::LoadI, 0, 42, 1);
    vec_function.emit_b(OpCode::LoadI, 1, 10, 1);
    vec_function.emit_register_abc(
        OpCode::VecNewI,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::Move,
        Register::new(257),
        Register::new(0),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::VecPushI,
        Register::new(256),
        Register::new(257),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::Move,
        Register::new(258),
        Register::new(1),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::VecReserve,
        Register::new(256),
        Register::new(258),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::VecCap,
        Register::new(259),
        Register::new(256),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::VecPopI,
        Register::new(260),
        Register::new(256),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::VecLen,
        Register::new(261),
        Register::new(256),
        Register::new(0),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::Add,
        Register::new(262),
        Register::new(260),
        Register::new(261),
        1,
    );
    vec_function.emit_register_abc(
        OpCode::Return,
        Register::new(262),
        Register::new(0),
        Register::new(0),
        1,
    );
    vec_function.finalize_bytecode();
    assert_eq!(vec_function.num_registers, 263);

    let assembly = aelys_bytecode::asm::disassemble(&vec_function);
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, vec_function.bytecode);
    let mut vm = VM::new(Source::new("wide-vec-management.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(assembled[0].clone()).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(42));
}

#[test]
fn wide_string_character_load_roundtrips_and_executes() {
    let mut function = Function::new(None, 0);
    function
        .constants
        .push(aelys_bytecode::Constant::String("aéz".to_string()));
    function.emit_a(OpCode::LoadK, 0, 0, 0, 1);
    function.emit_b(OpCode::LoadI, 1, 1, 1);
    function.emit_register_abc(
        OpCode::Move,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.emit_register_abc(
        OpCode::Move,
        Register::new(257),
        Register::new(1),
        Register::new(0),
        1,
    );
    function.emit_register_abc(
        OpCode::StringLoadChar,
        Register::new(258),
        Register::new(256),
        Register::new(257),
        1,
    );
    function.emit_register_abc(
        OpCode::Return,
        Register::new(258),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();

    let assembly = aelys_bytecode::asm::disassemble(&function);
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);
    let mut vm = VM::new(Source::new("wide-string-access.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(assembled[0].clone()).unwrap();
    let value = vm.execute(function_ref).unwrap();
    assert_eq!(vm.value_to_string(value), "é");
}

#[test]
fn wide_closure_destination_and_upvalue_registers_roundtrip_and_execute() {
    let mut nested = Function::new(Some("wide_capture".to_string()), 0);
    nested
        .upvalue_descriptors
        .push(aelys_bytecode::UpvalueDescriptor {
            is_local: true,
            index: 256,
        });
    nested.emit_wide_abc(
        OpCode::LoadI,
        Register::new(256),
        Register::new(43),
        Register::new(0),
        1,
    );
    nested.emit_register_abc(
        OpCode::SetUpval,
        Register::new(0),
        Register::new(256),
        Register::new(0),
        1,
    );
    nested.emit_register_abc(
        OpCode::GetUpval,
        Register::new(257),
        Register::new(0),
        Register::new(0),
        1,
    );
    nested.emit_register_abc(
        OpCode::Return,
        Register::new(257),
        Register::new(0),
        Register::new(0),
        1,
    );
    nested.finalize_bytecode();

    let mut function = Function::new(Some("wide_closure_parent".to_string()), 0);
    function
        .constants
        .push(aelys_bytecode::Constant::NestedFunction(0));
    function.nested_functions.push(nested);
    function.emit_b(OpCode::LoadI, 0, 42, 1);
    function.emit_register_abc(
        OpCode::Move,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.emit_closure_register_wide(Register::new(257), 0, 1, 1);
    function.emit_call(
        Register::new(258),
        Register::new(257),
        aelys_bytecode::Arity::new(0),
        1,
    );
    function.emit_register_abc(
        OpCode::CloseUpvals,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.emit_register_abc(
        OpCode::Return,
        Register::new(258),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();
    assert_eq!(function.num_registers, 259);

    let assembly = aelys_bytecode::asm::disassemble(&function);
    assert!(assembly.contains("MakeClosureRegisterWide r257, 0, 1"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);

    let mut vm = VM::new(Source::new("wide-closure.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(function).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(43));
}

#[test]
fn source_compiler_calls_a_closure_above_register_255() {
    let mut source = String::from("fn run(parameter) {\n");
    for register in 0..255 {
        source.push_str(&format!("let v{register} = {register}\n"));
    }
    let captures = (0..255)
        .map(|register| format!("v{register}"))
        .collect::<Vec<_>>();
    source.push_str(&format!(
        "let closure = fn() {{ return {} }}\nreturn closure() + parameter\n}}\nrun(1)",
        balanced_sum(&captures)
    ));

    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let compiled = deserialize(module.avbc()).unwrap();
    let assembly = aelys_bytecode::asm::disassemble(&compiled);
    assert!(assembly.contains("CallWide"));

    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("wide closure source unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(32_386));
}

#[test]
fn source_compiler_executes_a_loop_above_register_255() {
    let mut source = String::from("fn run(parameter) {\n");
    for register in 0..255 {
        source.push_str(&format!("let v{register} = {register}\n"));
    }
    let captures = (0..255)
        .map(|register| format!("v{register}"))
        .collect::<Vec<_>>();
    source.push_str(&format!(
        "let closure = fn() {{ return {} }}\nlet mut total = 0\nfor i in 0..3 {{ total += i }}\nwhile total < 6 {{ total += 1 }}\nreturn total\n}}\nrun(1)",
        balanced_sum(&captures)
    ));

    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let compiled = deserialize(module.avbc()).unwrap();
    let assembly = aelys_bytecode::asm::disassemble(&compiled);
    assert!(assembly.contains("LoopWideLong ForLoopILong"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[1].bytecode, compiled.nested_functions[0].bytecode);
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("wide loop source unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(6));
}

#[test]
fn wide_while_loop_pair_roundtrips_and_executes() {
    let mut function = Function::new(None, 0);
    function.emit_wide_abc(
        OpCode::LoadI,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.emit_wide_abc(
        OpCode::LoadI,
        Register::new(257),
        Register::new(3),
        Register::new(0),
        1,
    );
    let jump = function.emit_jump(OpCode::JumpLong, 1);
    function.emit_wide_abc(
        OpCode::AddI,
        Register::new(256),
        Register::new(256),
        Register::new(1),
        1,
    );
    function.patch_jump(jump);
    function.emit_wide_abc(
        OpCode::WhileLoopLt,
        Register::new(256),
        Register::new(u16::from_ne_bytes((-6i16).to_ne_bytes())),
        Register::new(0),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();

    let assembly = aelys_bytecode::asm::disassemble(&function);
    assert!(assembly.contains("Wide 48, r256"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(assembled[0].bytecode, function.bytecode);
    let mut vm = VM::new(Source::new("wide-while.aelys", "")).unwrap();
    let reference = vm.alloc_function(assembled[0].clone()).unwrap();
    assert_eq!(vm.execute(reference).unwrap().as_int(), Some(3));
}

#[test]
fn consecutive_wide_literals_keep_distinct_local_values() {
    let elements = (0..256)
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source =
        format!("let items = [{elements}]\nlet values = Vec[{elements}]\nitems[255] + values[255]");
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            &source,
            CompileOptions {
                optimization_level: aelys_opt::OptimizationLevel::None,
                ..CompileOptions::default()
            },
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) =
        isolate.execute(&module, RunOptions::default()).unwrap()
    else {
        panic!("consecutive wide literal execution unexpectedly exited");
    };
    assert_eq!(value.as_int(), Some(510));
}

#[test]
fn inference_depth_limit_is_a_compile_error_instead_of_null_bytecode() {
    let expression = (0..=200).fold("0".to_string(), |left, value| format!("{left} + {value}"));
    let result = Runtime::new().compile(
        &expression,
        CompileOptions {
            optimization_level: aelys_opt::OptimizationLevel::None,
            ..CompileOptions::default()
        },
    );
    let Err(error) = result else {
        panic!("inference depth overflow must reject compilation");
    };
    assert!(error.to_string().contains("recursion limit"));
}

#[test]
fn verifier_accepts_register_255_when_high_water_is_saturated() {
    let mut function = Function::new(None, 0);
    function.num_registers = 256;
    function.emit_a(OpCode::LoadNull, u8::MAX, 0, 0, 1);
    function.emit_a(OpCode::Return, u8::MAX, 0, 0, 1);
    function.finalize_bytecode();
    assert_eq!(function.num_registers, 256);

    let source = Source::new("register-boundary", "");
    let mut vm = aelys_runtime::VM::new(source).unwrap();
    let function = vm.alloc_function(function).unwrap();
    assert!(vm.execute(function).unwrap().is_null());
}

#[test]
fn avbc_v2_preserves_u16_arity_and_u32_register_count() {
    let mut function = Function::new(Some("wide_metadata".to_string()), u16::MAX);
    function.num_registers = 65_536;
    let bytes = serialize(&function).unwrap();
    let loaded = deserialize(&bytes).unwrap();

    assert_eq!(loaded.arity, u16::MAX);
    assert_eq!(loaded.num_registers, 65_536);
}

#[test]
fn constant_index_65536_roundtrips_and_executes() {
    let mut function = Function::new(Some("wide_constant".to_string()), 0);
    function.num_registers = 1;
    function.constants = (0i64..=65_536).map(aelys_bytecode::Constant::Int).collect();
    function.emit_index32(OpCode::LoadKWide, 0, 65_536, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    let bytes = serialize(&function).unwrap();
    let loaded = deserialize(&bytes).unwrap();
    assert_eq!(loaded.constants.len(), 65_537);
    assert_eq!(loaded.bytecode.as_slice()[1], 65_536);

    let mut vm = VM::new(Source::new("wide-constant.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(loaded).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(65_536));
}

#[test]
fn global_index_65536_roundtrips_and_executes() {
    let mut function = Function::new(Some("wide_global".to_string()), 0);
    function.num_registers = 2;
    function.emit_b(OpCode::LoadI, 0, 42, 1);
    function.emit_index32(OpCode::SetGlobalIdxWide, 0, 65_536, 1);
    function.emit_index32(OpCode::GetGlobalIdxWide, 1, 65_536, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();

    let bytes = serialize(&function).unwrap();
    let loaded = deserialize(&bytes).unwrap();
    let assembly = aelys_bytecode::asm::disassemble(&loaded);
    assert!(assembly.contains("SetGlobalIdxWide 65536, r0"));
    assert!(assembly.contains("GetGlobalIdxWide r1, 65536"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();

    let mut vm = VM::new(Source::new("wide-global.aelys", "")).unwrap();
    let function_ref = vm
        .alloc_function(assembled.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(42));
}

#[test]
fn register_256_uses_verified_wide_words() {
    let mut function = Function::new(Some("wide_register".to_string()), 0);
    function.emit_b(OpCode::LoadI, 0, 20, 1);
    function.emit_b(OpCode::LoadI, 1, 22, 1);
    function.emit_wide_abc(
        OpCode::Add,
        Register::new(256),
        Register::new(0),
        Register::new(1),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        Register::new(256),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();
    assert_eq!(function.num_registers, 257);

    let assembly = aelys_bytecode::asm::disassemble(&function);
    assert!(assembly.contains("AddWide r256, r0, r1"));
    assert!(assembly.contains("ReturnWide r256"));
    let assembled = aelys_bytecode::asm::assemble(&assembly).unwrap();

    let mut vm = VM::new(Source::new("wide-register.aelys", "")).unwrap();
    let function_ref = vm
        .alloc_function(assembled.into_iter().next().unwrap())
        .unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(42));
}

#[test]
fn register_65535_executes_at_the_v2_boundary() {
    let mut function = Function::new(Some("maximum_register".to_string()), 0);
    function.emit_wide_abc(
        OpCode::LoadNull,
        Register::new(u16::MAX),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        Register::new(u16::MAX),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();
    assert_eq!(function.num_registers, 65_536);

    let mut vm = VM::new(Source::new("maximum-register.aelys", "")).unwrap();
    let function = vm.alloc_function(function).unwrap();
    assert!(vm.execute(function).unwrap().is_null());
}

#[test]
fn wide_arithmetic_and_bitwise_forms_roundtrip_and_execute() {
    let mut function = Function::new(Some("wide families".to_string()), 0);
    function.emit_b(OpCode::LoadI, 0, 50, 1);
    function.emit_b(OpCode::LoadI, 1, 8, 1);
    function.emit_wide_abc(
        OpCode::Move,
        aelys_bytecode::Register::try_from(256).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        1,
    );
    function.emit_wide_abc(
        OpCode::Sub,
        aelys_bytecode::Register::try_from(257).unwrap(),
        aelys_bytecode::Register::try_from(256).unwrap(),
        aelys_bytecode::Register::try_from(1).unwrap(),
        1,
    );
    function.emit_wide_abc(
        OpCode::BitOr,
        aelys_bytecode::Register::try_from(258).unwrap(),
        aelys_bytecode::Register::try_from(257).unwrap(),
        aelys_bytecode::Register::try_from(1).unwrap(),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        aelys_bytecode::Register::try_from(258).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        1,
    );
    function.finalize_bytecode();
    assert_eq!(function.num_registers, 259);

    let assembly = aelys_bytecode::asm::disassemble(&function);
    assert!(assembly.contains("Wide 6, r257, r256, r1"));
    assert!(assembly.contains("Wide 108, r258, r257, r1"));
    let functions = aelys_bytecode::asm::assemble(&assembly).unwrap();
    let mut vm = VM::new(Source::new("wide.aelys", "")).unwrap();
    let reference = vm.alloc_function(functions[0].clone()).unwrap();
    let result = vm.execute(reference).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn wide_comparison_uses_u16_registers() {
    let mut function = Function::new(Some("wide comparison".to_string()), 0);
    function.emit_b(OpCode::LoadI, 0, 50, 1);
    function.emit_b(OpCode::LoadI, 1, 8, 1);
    function.emit_wide_abc(
        OpCode::Move,
        aelys_bytecode::Register::try_from(256).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        1,
    );
    function.emit_wide_abc(
        OpCode::Gt,
        aelys_bytecode::Register::try_from(257).unwrap(),
        aelys_bytecode::Register::try_from(256).unwrap(),
        aelys_bytecode::Register::try_from(1).unwrap(),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        aelys_bytecode::Register::try_from(257).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        1,
    );
    function.finalize_bytecode();

    let mut vm = VM::new(Source::new("wide.aelys", "")).unwrap();
    let reference = vm.alloc_function(function).unwrap();
    let result = vm.execute(reference).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn wide_register_count_includes_non_returned_destinations() {
    let mut function = Function::new(Some("wide register count".to_string()), 0);
    function.emit_b(OpCode::LoadI, 0, 50, 1);
    function.emit_b(OpCode::LoadI, 1, 8, 1);
    function.emit_wide_abc(
        OpCode::Sub,
        aelys_bytecode::Register::try_from(257).unwrap(),
        aelys_bytecode::Register::try_from(0).unwrap(),
        aelys_bytecode::Register::try_from(1).unwrap(),
        1,
    );
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    assert_eq!(function.num_registers, 258);
    let mut vm = VM::new(Source::new("wide-count.aelys", "")).unwrap();
    let reference = vm.alloc_function(function).unwrap();
    assert_eq!(vm.execute(reference).unwrap().as_int(), Some(50));
}

#[test]
fn wide_register_loads_immediates_and_constants() {
    let mut function = Function::new(Some("wide loads".to_string()), 0);
    function.constants = (0i64..=65_536).map(aelys_bytecode::Constant::Int).collect();
    function.emit_wide_abc(
        OpCode::LoadI,
        Register::new(256),
        Register::new(u16::from_ne_bytes((-7i16).to_ne_bytes())),
        Register::new(0),
        1,
    );
    function.emit_wide_abc(
        OpCode::LoadK,
        Register::new(257),
        Register::new(1),
        Register::new(0),
        1,
    );
    function.emit_wide_abc(
        OpCode::Add,
        Register::new(258),
        Register::new(256),
        Register::new(257),
        1,
    );
    function.emit_wide_abc(
        OpCode::Return,
        Register::new(258),
        Register::new(0),
        Register::new(0),
        1,
    );
    function.finalize_bytecode();

    let mut vm = VM::new(Source::new("wide-loads.aelys", "")).unwrap();
    let reference = vm.alloc_function(function).unwrap();
    assert_eq!(vm.execute(reference).unwrap().as_int(), Some(65_529));
}

#[test]
fn typed_register_emission_selects_compact_or_wide_encoding() {
    let mut function = Function::new(Some("typed emission".to_string()), 0);
    function.emit_register_abc(
        OpCode::Add,
        Register::new(255),
        Register::new(0),
        Register::new(1),
        1,
    );
    function.emit_register_abc(
        OpCode::Add,
        Register::new(256),
        Register::new(0),
        Register::new(1),
        1,
    );
    function.emit_register_abc(
        OpCode::Call,
        Register::new(257),
        Register::new(0),
        Register::new(0),
        1,
    );

    assert_eq!(function.current_offset(), 4);
    assert_eq!(function.wide_operand_error(), Some(OpCode::Call));
}

#[test]
fn assembler_rejects_v1_explicitly() {
    let error = aelys_bytecode::asm::assemble(".version 1\n").expect_err("v1 must be rejected");
    assert!(
        error
            .to_string()
            .contains("Unsupported assembly version: 1")
    );
}

#[test]
fn jump_over_i16_range_uses_a_verified_i32_extension() {
    let mut function = Function::new(Some("long_jump".to_string()), 0);
    function.num_registers = 1;
    let jump = function.emit_jump(OpCode::Jump, 1);
    for _ in 0..32_768 {
        function.emit_a(OpCode::LoadNull, 0, 0, 0, 1);
    }
    function.patch_jump(jump);
    function.emit_a(OpCode::Return0, 0, 0, 0, 1);
    function.finalize_bytecode();

    assert_eq!(
        function.bytecode.as_slice()[0] >> 24,
        OpCode::JumpLong as u32
    );
    assert_eq!(function.bytecode.as_slice()[1] as i32, 32_768);

    let mut vm = VM::new(Source::new("long-jump.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(function).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap(), Value::null());
}

#[test]
fn long_continue_patch_preserves_the_extension_word() {
    let mut function = Function::new(Some("long_continue".to_string()), 0);
    let jump = function.emit_jump(OpCode::Jump, 1);
    for _ in 0..32_768 {
        function.emit_a(OpCode::LoadNull, 0, 0, 0, 1);
    }
    let target = function.current_offset();
    function.patch_jump_to(jump, target);
    function.emit_a(OpCode::Return0, 0, 0, 0, 1);
    function.finalize_bytecode();

    assert_eq!(
        function.bytecode.as_slice()[0] >> 24,
        u32::from(OpCode::JumpLong as u8)
    );
    assert_eq!(
        i32::from_ne_bytes(function.bytecode.as_slice()[1].to_ne_bytes()),
        32_768
    );
    let mut vm = VM::new(Source::new("long-continue.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(function).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap(), Value::null());
}

#[test]
fn long_integer_loop_roundtrips_and_executes() {
    let mut function = Function::new(Some("long_integer_loop".to_string()), 0);
    function.emit_b(OpCode::LoadI, 0, -1, 1);
    function.emit_b(OpCode::LoadI, 1, 2, 1);
    function.emit_b(OpCode::LoadI, 2, 1, 1);
    let first_check = function.emit_jump(OpCode::Jump, 1);
    let body_start = function.current_offset();
    for _ in 0..32_768 {
        function.emit_a(OpCode::LoadNull, 3, 0, 0, 1);
    }
    let check = function.current_offset();
    function.patch_jump_to(first_check, check);
    function.emit_loop_back(OpCode::ForLoopI, OpCode::ForLoopILong, 0, body_start, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    assert!(function.bytecode.iter().any(|word| {
        OpCode::from_u8(u8::try_from(word >> 24).unwrap()) == Some(OpCode::ForLoopILong)
    }));
    let assembly = aelys_bytecode::asm::disassemble(&function);
    let reassembled = aelys_bytecode::asm::assemble(&assembly).unwrap();
    assert_eq!(
        reassembled[0].bytecode.as_slice(),
        function.bytecode.as_slice()
    );
    let mut vm = VM::new(Source::new("long-loop.aelys", "")).unwrap();
    let function_ref = vm.alloc_function(reassembled[0].clone()).unwrap();
    assert_eq!(vm.execute(function_ref).unwrap().as_int(), Some(2));
}
