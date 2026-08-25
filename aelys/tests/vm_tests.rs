
use aelys_bytecode::Register;
use aelys_common::{RuntimeError, RuntimeErrorKind};
use aelys_runtime::{
    AelysClosure, AelysUpvalue, CallFrame, Function, GcObject, GlobalLayout, MAX_FRAMES,
    ObjectKind, OpCode, UpvalueDescriptor, VM, Value, builtin_type,
};
use aelys_syntax::Source;
use std::sync::Arc;

fn make_test_source() -> Arc<Source> {
    Source::new("test.aelys", "fn test() { }")
}

#[test]
fn test_vm_creation() {
    let source = make_test_source();
    let vm = VM::new(source).unwrap();

    assert_eq!(vm.frame_count(), 0);
    assert_eq!(vm.register_count(), 32768);
}

#[test]
fn test_push_pop_frame() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 5;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);

    vm.push_frame(frame).unwrap();
    assert_eq!(vm.frame_count(), 1);

    let popped = vm.pop_frame();
    assert!(popped.is_some());
    assert_eq!(vm.frame_count(), 0);
}

#[test]
fn root_deoptimization_resumes_at_the_exact_bytecode_ip() {
    let mut vm = VM::new(make_test_source()).unwrap();
    let mut function = Function::new(Some("deoptimized_root".to_string()), 0);
    function.num_registers = 1;
    function.emit_b(OpCode::LoadI, 0, 99, 1);
    let return_ip = u32::try_from(function.current_offset()).unwrap();
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    let function = vm.alloc_function(function).unwrap();

    let result = vm
        .execute_deoptimized(function, return_ip, vec![(0, Value::int(42))])
        .unwrap();

    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_frame_stack_overflow() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 5;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    for _ in 0..MAX_FRAMES {
        let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
        vm.push_frame(frame).unwrap();
    }

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    let result = vm.push_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_read_write_register() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 10;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    vm.push_frame(frame).unwrap();

    vm.write_register(0, Value::int(42)).unwrap();
    assert_eq!(vm.read_register(0).unwrap().as_int(), Some(42));

    vm.write_register(5, Value::bool(true)).unwrap();
    assert_eq!(vm.read_register(5).unwrap().as_bool(), Some(true));
}

#[test]
fn test_windowed_registers() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func1 = Function::new(Some("func1".to_string()), 0);
    func1.num_registers = 5;
    let func1_ref = vm.alloc_function(func1).unwrap();

    let frame1 = CallFrame::new(func1_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    vm.push_frame(frame1).unwrap();

    vm.write_register(0, Value::int(100)).unwrap();
    vm.write_register(1, Value::int(200)).unwrap();

    let mut func2 = Function::new(Some("func2".to_string()), 0);
    func2.num_registers = 3;
    let func2_ref = vm.alloc_function(func2).unwrap();

    let frame2 = CallFrame::new(func2_ref, 5, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    vm.push_frame(frame2).unwrap();

    vm.write_register(0, Value::int(10)).unwrap();
    vm.write_register(1, Value::int(20)).unwrap();

    assert_eq!(vm.read_register(0).unwrap().as_int(), Some(10));
    assert_eq!(vm.read_register(1).unwrap().as_int(), Some(20));

    vm.pop_frame();

    assert_eq!(vm.read_register(0).unwrap().as_int(), Some(100));
    assert_eq!(vm.read_register(1).unwrap().as_int(), Some(200));
}

#[test]
fn test_global_variables() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    assert!(vm.get_global("x").is_none());

    vm.set_global("x".to_string(), Value::int(42));
    assert_eq!(vm.get_global("x"), Some(Value::int(42)));

    vm.set_global("x".to_string(), Value::bool(true));
    assert_eq!(vm.get_global("x"), Some(Value::bool(true)));
}

#[test]
fn test_collect_marks_registers() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let str1 = vm.alloc_string("keep me").unwrap();
    let str2 = vm.alloc_string("free me").unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 5;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 5);
    vm.push_frame(frame).unwrap();

    vm.write_register(0, Value::ptr(str1.index())).unwrap();

    vm.collect();

    assert!(vm.heap().get(str1).is_some());
    assert!(vm.heap().get(str2).is_none());
}

#[test]
fn test_collect_marks_globals() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let str1 = vm.alloc_string("global string").unwrap();
    let str2 = vm.alloc_string("unreachable").unwrap();

    vm.set_global("my_str".to_string(), Value::ptr(str1.index()));

    vm.collect();

    assert!(vm.heap().get(str1).is_some());
    assert!(vm.heap().get(str2).is_none());
}

#[test]
fn test_runtime_error_with_stack_trace() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test_func".to_string()), 2);
    func.num_registers = 10;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    vm.push_frame(frame).unwrap();

    let error = vm.runtime_error(RuntimeErrorKind::DivisionByZero);

    assert!(matches!(error.kind, RuntimeErrorKind::DivisionByZero));
    assert!(!error.stack_trace.is_empty());
}
#[test]
fn test_current_frame_methods() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 5;
    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();

    let frame = CallFrame::new(func_ref, 0, std::ptr::null(), 0, std::ptr::null(), 0, 0);
    vm.push_frame(frame).unwrap();

    assert_eq!(vm.current_frame().unwrap().ip(), 0);

    vm.current_frame_mut().unwrap().advance_ip();
    assert_eq!(vm.current_frame().unwrap().ip(), 1);

    vm.current_frame_mut().unwrap().set_ip(42);
    assert_eq!(vm.current_frame().unwrap().ip(), 42);
}


#[test]
fn test_execute_simple_return() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42
    func.emit_a(OpCode::Return, 0, 0, 0, 1); // return r0

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();

    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_execute_arithmetic() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 5;
    func.emit_b(OpCode::LoadI, 0, 10, 1); // r0 = 10
    func.emit_b(OpCode::LoadI, 1, 20, 1); // r1 = 20
    func.emit_a(OpCode::Add, 2, 0, 1, 1); // r2 = r0 + r1 (30)
    func.emit_b(OpCode::LoadI, 3, 5, 1); // r3 = 5
    func.emit_a(OpCode::Sub, 4, 2, 3, 1); // r4 = r2 - r3 (25)
    func.emit_a(OpCode::Return, 4, 0, 0, 1); // return r4

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();

    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_execute_multiplication_division() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 5;
    func.emit_b(OpCode::LoadI, 0, 6, 1); // r0 = 6
    func.emit_b(OpCode::LoadI, 1, 7, 1); // r1 = 7
    func.emit_a(OpCode::Mul, 2, 0, 1, 1); // r2 = r0 * r1 (42)
    func.emit_b(OpCode::LoadI, 3, 2, 1); // r3 = 2
    func.emit_a(OpCode::Div, 4, 2, 3, 1); // r4 = r2 / r3 (21)
    func.emit_a(OpCode::Return, 4, 0, 0, 1); // return r4

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();

    assert_eq!(result.as_int(), Some(21));
}

#[test]
fn test_execute_modulo() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_b(OpCode::LoadI, 0, 17, 1); // r0 = 17
    func.emit_b(OpCode::LoadI, 1, 5, 1); // r1 = 5
    func.emit_a(OpCode::Mod, 2, 0, 1, 1); // r2 = r0 % r1 (2)
    func.emit_a(OpCode::Return, 2, 0, 0, 1); // return r2

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();

    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_execute_negation() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42
    func.emit_a(OpCode::Neg, 1, 0, 0, 1); // r1 = -r0
    func.emit_a(OpCode::Return, 1, 0, 0, 1); // return r1

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();

    assert_eq!(result.as_int(), Some(-42));
}

#[test]
fn test_execute_division_by_zero() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_b(OpCode::LoadI, 0, 10, 1); // r0 = 10
    func.emit_b(OpCode::LoadI, 1, 0, 1); // r1 = 0
    func.emit_a(OpCode::Div, 2, 0, 1, 1); // r2 = r0 / r1 (error!)
    func.emit_a(OpCode::Return, 2, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e.kind, RuntimeErrorKind::DivisionByZero));
    }
}

#[test]
fn test_execute_comparison_operators() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_b(OpCode::LoadI, 0, 10, 1); // r0 = 10
    func.emit_b(OpCode::LoadI, 1, 20, 1); // r1 = 20
    func.emit_a(OpCode::Lt, 2, 0, 1, 1); // r2 = r0 < r1
    func.emit_a(OpCode::Return, 2, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_execute_equality() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42
    func.emit_b(OpCode::LoadI, 1, 42, 1); // r1 = 42
    func.emit_a(OpCode::Eq, 2, 0, 1, 1); // r2 = r0 == r1
    func.emit_a(OpCode::Return, 2, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_execute_not_equal() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_b(OpCode::LoadI, 0, 10, 1); // r0 = 10
    func.emit_b(OpCode::LoadI, 1, 20, 1); // r1 = 20
    func.emit_a(OpCode::Ne, 2, 0, 1, 1); // r2 = r0 != r1
    func.emit_a(OpCode::Return, 2, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_execute_logical_not() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_a(OpCode::LoadBool, 0, 1, 0, 1); // r0 = true
    func.emit_a(OpCode::Not, 1, 0, 0, 1); // r1 = !r0
    func.emit_a(OpCode::Return, 1, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_execute_jump() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    func.emit_b(OpCode::Jump, 0, 2, 1); // jump forward 2 instructions
    func.emit_b(OpCode::LoadI, 0, 99, 1); // r0 = 99 (skipped)
    func.emit_b(OpCode::LoadI, 0, 99, 1); // r0 = 99 (skipped)
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42 (executed)
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_execute_jump_if() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_a(OpCode::LoadBool, 0, 1, 0, 1); // r0 = true
    func.emit_b(OpCode::JumpIf, 0, 1, 1); // if r0, jump forward 1
    func.emit_b(OpCode::LoadI, 1, 99, 1); // r1 = 99 (skipped)
    func.emit_b(OpCode::LoadI, 1, 42, 1); // r1 = 42 (executed)
    func.emit_a(OpCode::Return, 1, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_execute_jump_if_not() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_a(OpCode::LoadBool, 0, 0, 0, 1); // r0 = false
    func.emit_b(OpCode::JumpIfNot, 0, 1, 1); // if !r0, jump forward 1
    func.emit_b(OpCode::LoadI, 1, 99, 1); // r1 = 99 (skipped)
    func.emit_b(OpCode::LoadI, 1, 42, 1); // r1 = 42 (executed)
    func.emit_a(OpCode::Return, 1, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_execute_load_constant() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    let k = func.add_constant(Value::int(12345));
    func.emit_a(OpCode::LoadK, 0, k as u8, 0, 1); // r0 = constants[k]
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(12345));
}

#[test]
fn test_execute_load_null() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    func.emit_a(OpCode::LoadNull, 0, 0, 0, 1);
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert!(result.is_null());
}

#[test]
fn test_execute_load_bool() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    func.emit_a(OpCode::LoadBool, 0, 1, 0, 1); // r0 = true
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_execute_move() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42
    func.emit_a(OpCode::Move, 1, 0, 0, 1); // r1 = r0
    func.emit_a(OpCode::Return, 1, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_execute_global_variables() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 2;

    let k = func.add_structural_constant(aelys_bytecode::Constant::String("myvar".to_string()));

    func.emit_b(OpCode::LoadI, 0, 123, 1); // r0 = 123
    func.emit_a(OpCode::SetGlobal, 0, k as u8, 0, 1); // myvar = r0
    func.emit_a(OpCode::GetGlobal, 1, k as u8, 0, 1); // r1 = myvar
    func.emit_a(OpCode::Return, 1, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(123));
}

#[test]
fn test_execute_native_function_call() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    fn add_native(_vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
        let a = args[0].as_int().unwrap_or(0);
        let b = args[1].as_int().unwrap_or(0);
        Ok(Value::int(a + b))
    }

    let native_ref = vm.alloc_native("add", 2, add_native).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 5;

    vm.set_global("add".to_string(), Value::ptr(native_ref.index()));
    let k = func.add_structural_constant(aelys_bytecode::Constant::String("add".to_string()));

    func.emit_a(OpCode::GetGlobal, 0, k as u8, 0, 1); // r0 = native function
    func.emit_b(OpCode::LoadI, 1, 10, 1); // r1 = 10 (arg1)
    func.emit_b(OpCode::LoadI, 2, 20, 1); // r2 = 20 (arg2)
    func.emit_c(OpCode::Call, 3, 0, 2, 1); // r3 = r0(r1, r2)
    func.emit_a(OpCode::Return, 3, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn native_panic_is_structured_and_vm_can_be_reused() {
    fn panic_native(_vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
        panic!("native test panic")
    }

    let mut vm = VM::new(make_test_source()).unwrap();
    let native_ref = vm.alloc_native("panic_native", 0, panic_native).unwrap();
    vm.set_global("panic_native".to_string(), Value::ptr(native_ref.index()));

    let mut failing = Function::new(Some("failing".to_string()), 0);
    failing.num_registers = 2;
    let name = failing
        .add_structural_constant(aelys_bytecode::Constant::String("panic_native".to_string()));
    failing.emit_a(OpCode::GetGlobal, 0, u8::try_from(name).unwrap(), 0, 1);
    failing.emit_c(OpCode::Call, 1, 0, 0, 1);
    failing.emit_a(OpCode::Return, 1, 0, 0, 1);
    failing.finalize_bytecode();
    let failing_ref = vm.alloc_function(failing).unwrap();

    let error = vm.execute(failing_ref).unwrap_err();
    assert!(matches!(error.kind, RuntimeErrorKind::NativePanic));
    assert_eq!(vm.frame_count(), 0);

    let mut recovery = Function::new(Some("recovery".to_string()), 0);
    recovery.num_registers = 1;
    recovery.emit_b(OpCode::LoadI, 0, 42, 1);
    recovery.emit_a(OpCode::Return, 0, 0, 0, 1);
    recovery.finalize_bytecode();
    let recovery_ref = vm.alloc_function(recovery).unwrap();
    assert_eq!(vm.execute(recovery_ref).unwrap().as_int(), Some(42));
}

#[test]
fn test_execute_return0() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    func.emit_b(OpCode::LoadI, 0, 42, 1); // r0 = 42
    func.emit_a(OpCode::Return0, 0, 0, 0, 1); // return null (ignore r0)

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert!(result.is_unit());
}

#[test]
fn test_execute_no_gc_control() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 1;
    func.emit_b(OpCode::LoadI, 0, 42, 1);
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_type_error_add_incompatible() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.emit_a(OpCode::LoadNull, 0, 0, 0, 1);
    func.emit_b(OpCode::LoadI, 1, 10, 1);
    func.emit_a(OpCode::Add, 2, 0, 1, 1);
    func.emit_a(OpCode::Return, 2, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);
    assert!(result.is_err());
}

#[test]
fn test_globals_survive_gc() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    vm.set_global("test_var".to_string(), Value::int(42));

    vm.collect();

    let value = vm.get_global("test_var");
    assert!(value.is_some());
    assert_eq!(value.unwrap().as_int(), Some(42));
}

#[test]
fn test_callglobal_native_function() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();
    let type_fn = vm.alloc_native("type", 1, builtin_type).unwrap();
    vm.set_global("type".to_string(), Value::ptr(type_fn.index()));

    let mut func = Function::new(Some("main".to_string()), 0);
    func.num_registers = 3;
    func.global_layout = GlobalLayout::new(vec!["type".to_string()]);

    func.emit_b(OpCode::LoadI, 1, 42, 1);
    func.emit_a(OpCode::CallGlobal, 0, 0, 1, 1);
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    let result = vm.execute(func_ref);

    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(
        value.is_ptr(),
        "type() should return a string (ptr), got {:?}",
        value
    );
}

#[test]
fn test_callglobal_native_target_mutation_survives_gc() {
    fn first(_vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
        Ok(Value::int(1))
    }

    fn second(_vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
        Ok(Value::int(2))
    }

    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();
    let first_ref = vm.alloc_native("target", 0, first).unwrap();
    vm.set_global("target".to_string(), Value::ptr(first_ref.index()));
    vm.set_global_by_index(0, Value::ptr(first_ref.index()));

    let mut main = Function::new(Some("main".to_string()), 0);
    main.num_registers = 1;
    main.global_layout = GlobalLayout::new(vec!["target".to_string()]);
    main.emit_a(OpCode::CallGlobal, 0, 0, 0, 1);
    main.push_raw(0);
    main.push_raw(0);
    main.emit_a(OpCode::Return, 0, 0, 0, 1);
    main.finalize_bytecode();
    let main_ref = vm.alloc_function(main).unwrap();
    vm.set_global("main".to_string(), Value::ptr(main_ref.index()));

    assert_eq!(vm.execute(main_ref).unwrap().as_int(), Some(1));

    let second_ref = vm.alloc_native("target", 0, second).unwrap();
    vm.set_global("target".to_string(), Value::ptr(second_ref.index()));
    vm.set_global_by_index(0, Value::ptr(second_ref.index()));
    vm.collect();

    assert_eq!(vm.execute(main_ref).unwrap().as_int(), Some(2));
}

#[test]
fn test_callglobal_user_defined_function() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut add_func = Function::new(Some("add".to_string()), 2);
    add_func.num_registers = 3;
    add_func.emit_a(OpCode::Add, 2, 0, 1, 1); // r2 = r0 + r1
    add_func.emit_a(OpCode::Return, 2, 0, 0, 1);

    add_func.finalize_bytecode();
    add_func.finalize_bytecode();
    let add_func_ref = vm.alloc_function(add_func).unwrap();

    let mut main_func = Function::new(Some("main".to_string()), 0);
    main_func.num_registers = 4;
    main_func.global_layout = GlobalLayout::new(vec!["add".to_string()]);

    vm.set_global_by_index(0, Value::ptr(add_func_ref.index()));
    vm.set_global("add".to_string(), Value::ptr(add_func_ref.index()));

    main_func.emit_b(OpCode::LoadI, 1, 10, 1); // r1 = 10
    main_func.emit_b(OpCode::LoadI, 2, 20, 1); // r2 = 20
    main_func.emit_a(OpCode::CallGlobal, 0, 0, 2, 1);
    main_func.emit_a(OpCode::Return, 0, 0, 0, 1);

    main_func.finalize_bytecode();
    main_func.finalize_bytecode();
    let main_func_ref = vm.alloc_function(main_func).unwrap();
    let result = vm.execute(main_func_ref);

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value.as_int(), Some(30), "add(10, 20) should return 30");
}

#[test]
fn test_dispatch_state_survives_frame_switch_and_register_resize() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut callee = Function::new(Some("large_callee".to_string()), 0);
    callee.num_registers = 20_000;
    callee.emit_b(OpCode::LoadI, 0, 42, 1);
    callee.emit_a(OpCode::Return, 0, 0, 0, 1);
    callee.finalize_bytecode();
    let callee_ref = vm.alloc_function(callee).unwrap();

    vm.set_global_by_index(0, Value::ptr(callee_ref.index()));
    vm.set_global("large_callee".to_string(), Value::ptr(callee_ref.index()));

    let mut main = Function::new(Some("main".to_string()), 0);
    main.num_registers = 4;
    main.global_layout = GlobalLayout::new(vec!["large_callee".to_string()]);
    let answer =
        main.add_structural_constant(aelys_bytecode::Constant::String("answer".to_string()));
    let answer = u8::try_from(answer).unwrap();
    main.emit_a(OpCode::CallGlobal, 0, 0, 0, 1);
    main.emit_b(OpCode::LoadI, 1, 15, 1);
    main.emit_a(OpCode::BitAnd, 2, 0, 1, 1);
    main.emit_a(OpCode::SetGlobal, 2, answer, 0, 1);
    main.emit_a(OpCode::GetGlobal, 3, answer, 0, 1);
    main.emit_a(OpCode::Return, 3, 0, 0, 1);
    main.finalize_bytecode();
    let main_ref = vm.alloc_function(main).unwrap();

    assert_eq!(vm.execute(main_ref).unwrap().as_int(), Some(10));
    assert_eq!(vm.get_global("answer").unwrap().as_int(), Some(10));
}

#[test]
fn test_tail_call_upvalue_reloads_constant_bounds() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut target = Function::new(Some("target".to_string()), 0);
    target.num_registers = 1;
    target.add_constant(Value::int(11));
    target.add_constant(Value::int(77));
    target.emit_b(OpCode::LoadK, 0, 1, 1);
    target.emit_a(OpCode::Return, 0, 0, 0, 1);
    target.finalize_bytecode();
    let target_ref = vm.alloc_function(target).unwrap();
    let target_closure = {
        let target = vm.heap().get(target_ref).unwrap();
        let ObjectKind::Function(target) = &target.kind else {
            panic!("allocated target is not a function");
        };
        AelysClosure::with_cache(
            target_ref,
            Vec::new(),
            aelys_bytecode::ClosureCache {
                bytecode_ptr: target.function.bytecode.as_ptr(),
                bytecode_len: target.function.bytecode.len(),
                constants_ptr: target.constants.as_ptr(),
                constants_len: target.constants.len(),
                arity: target.function.arity,
                num_registers: target.function.num_registers,
            },
        )
    };
    let target_closure_ref = vm
        .alloc_object(GcObject::new(ObjectKind::Closure(target_closure)))
        .unwrap();

    let mut caller = Function::new(Some("caller".to_string()), 0);
    caller.num_registers = 1;
    caller.upvalue_descriptors.push(UpvalueDescriptor {
        is_local: false,
        index: 0,
    });
    caller.emit_a(OpCode::TailCallUpval, 0, 0, 0, 1);
    caller.finalize_bytecode();
    let caller_ref = vm.alloc_function(caller).unwrap();

    let (bytecode_ptr, bytecode_len, constants_ptr, constants_len, arity, num_registers) = {
        let caller = vm.heap().get(caller_ref).unwrap();
        let ObjectKind::Function(caller) = &caller.kind else {
            panic!("allocated caller is not a function");
        };
        (
            caller.function.bytecode.as_ptr(),
            caller.function.bytecode.len(),
            caller.constants.as_ptr(),
            caller.constants.len(),
            caller.function.arity,
            caller.function.num_registers,
        )
    };
    let mut upvalue = AelysUpvalue::new_open(0, 0);
    upvalue.close(Value::ptr(target_closure_ref.index()));
    let upvalue_ref = vm
        .alloc_object(GcObject::new(ObjectKind::Upvalue(upvalue)))
        .unwrap();
    let closure = AelysClosure::with_cache(
        caller_ref,
        vec![upvalue_ref],
        aelys_bytecode::ClosureCache {
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            arity,
            num_registers,
        },
    );
    let closure_ref = vm
        .alloc_object(GcObject::new(ObjectKind::Closure(closure)))
        .unwrap();

    let result = vm.call_value(Value::ptr(closure_ref.index()), &[]).unwrap();
    assert_eq!(result.as_int(), Some(77));
}

#[test]
fn test_tail_call_upvalue_resizes_register_window() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let wide_register = Register::new(39_999);
    let mut target = Function::new(Some("wide_target".to_string()), 0);
    target.num_registers = 40_000;
    target.emit_wide_abc(
        OpCode::Move,
        wide_register,
        Register::new(0),
        Register::new(0),
        1,
    );
    target.emit_wide_abc(
        OpCode::Return,
        wide_register,
        Register::new(0),
        Register::new(0),
        1,
    );
    target.finalize_bytecode();
    let target_ref = vm.alloc_function(target).unwrap();

    let mut caller = Function::new(Some("caller".to_string()), 0);
    caller.num_registers = 1;
    caller.upvalue_descriptors.push(UpvalueDescriptor {
        is_local: false,
        index: 0,
    });
    caller.emit_a(OpCode::TailCallUpval, 0, 0, 0, 1);
    caller.finalize_bytecode();
    let caller_ref = vm.alloc_function(caller).unwrap();

    let (bytecode_ptr, bytecode_len, constants_ptr, constants_len, arity, num_registers) = {
        let caller = vm.heap().get(caller_ref).unwrap();
        let ObjectKind::Function(caller) = &caller.kind else {
            panic!("allocated caller is not a function");
        };
        (
            caller.function.bytecode.as_ptr(),
            caller.function.bytecode.len(),
            caller.constants.as_ptr(),
            caller.constants.len(),
            caller.function.arity,
            caller.function.num_registers,
        )
    };
    let mut upvalue = AelysUpvalue::new_open(0, 0);
    upvalue.close(Value::ptr(target_ref.index()));
    let upvalue_ref = vm
        .alloc_object(GcObject::new(ObjectKind::Upvalue(upvalue)))
        .unwrap();
    let closure = AelysClosure::with_cache(
        caller_ref,
        vec![upvalue_ref],
        aelys_bytecode::ClosureCache {
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            arity,
            num_registers,
        },
    );
    let closure_ref = vm
        .alloc_object(GcObject::new(ObjectKind::Closure(closure)))
        .unwrap();

    let result = vm.call_value(Value::ptr(closure_ref.index()), &[]).unwrap();
    assert!(result.is_null());
    assert_eq!(vm.register_count(), 40_000);
}

#[test]
fn test_callglobal_recursive_function() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut fact_func = Function::new(Some("fact".to_string()), 1);
    fact_func.num_registers = 5;
    fact_func.global_layout = GlobalLayout::new(vec!["fact".to_string()]);

    fact_func.emit_b(OpCode::LoadI, 1, 2, 1); // r1 = 2
    fact_func.emit_a(OpCode::Lt, 2, 0, 1, 1); // r2 = r0 < r1 (n < 2)
    fact_func.emit_b(OpCode::JumpIfNot, 2, 2, 1); // if not (n < 2), skip 2 instructions

    fact_func.emit_b(OpCode::LoadI, 3, 1, 1); // r3 = 1
    fact_func.emit_a(OpCode::Return, 3, 0, 0, 1); // return 1

    fact_func.emit_a(OpCode::SubI, 2, 0, 1, 1); // r2 = n - 1
    fact_func.emit_a(OpCode::CallGlobal, 3, 0, 1, 1); // r3 = fact(r2) - args at r3+1=r4, so we need to put r2 in the right place

    let mut fact_func = Function::new(Some("fact".to_string()), 1);
    fact_func.num_registers = 6;
    fact_func.global_layout = GlobalLayout::new(vec!["fact".to_string()]);

    fact_func.emit_b(OpCode::LoadI, 1, 2, 1); // r1 = 2
    fact_func.emit_a(OpCode::Lt, 2, 0, 1, 1); // r2 = r0 < r1 (n < 2)
    fact_func.emit_b(OpCode::JumpIfNot, 2, 2, 1); // if not (n < 2), skip 2 instructions

    fact_func.emit_b(OpCode::LoadI, 3, 1, 1); // r3 = 1
    fact_func.emit_a(OpCode::Return, 3, 0, 0, 1); // return 1

    fact_func.emit_a(OpCode::SubI, 4, 0, 1, 1); // r4 = n - 1
    fact_func.emit_a(OpCode::CallGlobal, 3, 0, 1, 1); // r3 = fact(r4) where args at r3+1=r4
    fact_func.emit_a(OpCode::Mul, 5, 0, 3, 1); // r5 = n * r3
    fact_func.emit_a(OpCode::Return, 5, 0, 0, 1); // return r5

    fact_func.finalize_bytecode();
    let fact_func_ref = vm.alloc_function(fact_func).unwrap();
    vm.set_global_by_index(0, Value::ptr(fact_func_ref.index()));
    vm.set_global("fact".to_string(), Value::ptr(fact_func_ref.index()));

    let mut main_func = Function::new(Some("main".to_string()), 0);
    main_func.num_registers = 3;
    main_func.global_layout = GlobalLayout::new(vec!["fact".to_string()]);

    main_func.emit_b(OpCode::LoadI, 1, 5, 1); // r1 = 5 (argument at dest+1=r0+1=r1)
    main_func.emit_a(OpCode::CallGlobal, 0, 0, 1, 1); // r0 = fact(5)
    main_func.emit_a(OpCode::Return, 0, 0, 0, 1);

    main_func.finalize_bytecode();
    let main_func_ref = vm.alloc_function(main_func).unwrap();
    let result = vm.execute(main_func_ref);

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value.as_int(), Some(120), "fact(5) should return 120");
}

#[test]
fn test_callglobal_arity_mismatch() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut add_func = Function::new(Some("add".to_string()), 2);
    add_func.num_registers = 3;
    add_func.emit_a(OpCode::Add, 2, 0, 1, 1);
    add_func.emit_a(OpCode::Return, 2, 0, 0, 1);

    add_func.finalize_bytecode();
    let add_func_ref = vm.alloc_function(add_func).unwrap();
    vm.set_global_by_index(0, Value::ptr(add_func_ref.index()));
    vm.set_global("add".to_string(), Value::ptr(add_func_ref.index()));

    let mut main_func = Function::new(Some("main".to_string()), 0);
    main_func.num_registers = 3;
    main_func.global_layout = GlobalLayout::new(vec!["add".to_string()]);

    main_func.emit_b(OpCode::LoadI, 1, 10, 1); // Only provide 1 arg
    main_func.emit_a(OpCode::CallGlobal, 0, 0, 1, 1); // Call with nargs=1, but add expects 2
    main_func.emit_a(OpCode::Return, 0, 0, 0, 1);

    main_func.finalize_bytecode();
    let main_func_ref = vm.alloc_function(main_func).unwrap();
    let result = vm.execute(main_func_ref);

    assert!(result.is_err(), "Should fail with arity mismatch");
    if let Err(err) = result {
        assert!(matches!(
            err.kind,
            RuntimeErrorKind::ArityMismatch {
                expected: 2,
                got: 1
            }
        ));
    }
}

#[test]
fn test_callglobal_cache_invalidation_on_gc() {
    let source = make_test_source();
    let mut vm = VM::new(source).unwrap();

    let mut func = Function::new(Some("test".to_string()), 0);
    func.num_registers = 1;
    func.emit_b(OpCode::LoadI, 0, 42, 1);
    func.emit_a(OpCode::Return, 0, 0, 0, 1);

    func.finalize_bytecode();
    func.finalize_bytecode();
    let func_ref = vm.alloc_function(func).unwrap();
    vm.set_global("test".to_string(), Value::ptr(func_ref.index()));
    vm.set_global_by_index(0, Value::ptr(func_ref.index()));

    let mut main_func = Function::new(Some("main".to_string()), 0);
    main_func.num_registers = 2;
    main_func.global_layout = GlobalLayout::new(vec!["test".to_string()]);

    main_func.emit_a(OpCode::CallGlobal, 0, 0, 0, 1);
    main_func.emit_a(OpCode::Return, 0, 0, 0, 1);

    main_func.finalize_bytecode();
    let main_func_ref = vm.alloc_function(main_func).unwrap();

    let result1 = vm.execute(main_func_ref);
    assert!(result1.is_ok());
    assert_eq!(result1.unwrap().as_int(), Some(42));

    vm.collect();

    let mut main_func2 = Function::new(Some("main".to_string()), 0);
    main_func2.num_registers = 2;
    main_func2.global_layout = GlobalLayout::new(vec!["test".to_string()]);
    main_func2.emit_a(OpCode::CallGlobal, 0, 0, 0, 1);
    main_func2.emit_a(OpCode::Return, 0, 0, 0, 1);
    main_func2.finalize_bytecode();
    let main_func_ref2 = vm.alloc_function(main_func2).unwrap();

    let result2 = vm.execute(main_func_ref2);
    assert!(
        result2.is_ok(),
        "Execution should succeed after GC: {:?}",
        result2
    );
    assert_eq!(result2.unwrap().as_int(), Some(42));
}
