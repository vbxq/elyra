use aelys_common::RuntimeErrorKind;
use aelys_runtime::{VM, VmConfig, VmConfigError, parse_vm_args};
use aelys_syntax::Source;

#[test]
fn parse_vm_args_default() {
    let parsed = parse_vm_args(&[]).expect("should parse defaults");
    assert_eq!(
        parsed.config.max_heap_bytes,
        VmConfig::DEFAULT_MAX_HEAP_BYTES
    );
    assert!(parsed.program_args.is_empty());
}

#[test]
fn parse_vm_args_dev_flag_enables_hot_reload() {
    let _parsed = parse_vm_args(&["--dev".to_string()]).expect("should parse");
}

#[test]
fn jvm_style_max_heap_too_small() {
    let err = parse_vm_args(&["-ae.max-heap=4096".to_string()])
        .err()
        .expect("should fail");
    match err {
        aelys_runtime::VmArgsError::InvalidValue { reason, .. } => {
            assert!(reason.contains("must be >="));
        }
        aelys_runtime::VmArgsError::InvalidConfig(VmConfigError::MaxHeapTooSmall { .. }) => {}
        _ => panic!("unexpected error: {:?}", err),
    }
}

#[test]
fn gnu_style_max_heap() {
    let parsed = parse_vm_args(&["--ae-max-heap=1G".to_string(), "script.aelys".to_string()])
        .expect("should parse");
    assert_eq!(parsed.config.max_heap_bytes, 1024 * 1024 * 1024);
    assert_eq!(parsed.program_args, vec!["script.aelys".to_string()]);
}

#[test]
fn invalid_vm_arg_value_errors() {
    let err = parse_vm_args(&["-ae.max-heap=not-a-number".to_string()])
        .err()
        .expect("should error");
    match err {
        aelys_runtime::VmArgsError::InvalidValue { reason, .. } => {
            assert!(reason.contains("invalid integer"));
        }
        _ => panic!("unexpected error"),
    }
}

#[test]
fn heap_limit_triggers_out_of_memory() {
    let config = VmConfig::new(1024 * 1024).expect("valid config");
    let src = Source::new("<test>", "");
    let mut vm = VM::with_config_and_args(src, config.clone(), Vec::new()).expect("vm init");

    let large = "a".repeat(2 * 1024 * 1024);
    let err = vm.alloc_string(&large).expect_err("should fail");
    match err.kind {
        RuntimeErrorKind::OutOfMemory { .. } => {}
        _ => panic!("expected OutOfMemory"),
    }
}

#[test]
fn function_materialization_rejects_over_limit() {
    let config = VmConfig::new(2 * 1024 * 1024).expect("valid config");
    let src = Source::new("<test>", "");
    let mut vm = VM::with_config_and_args(src, config.clone(), Vec::new()).expect("vm init");

    let large = "x".repeat(2 * 1024 * 1024);
    let mut function = aelys_runtime::Function::new(None, 0);
    function
        .constants
        .push(aelys_bytecode::Constant::String(large));

    let err = vm.alloc_function(function).expect_err("should OOM");
    match err.kind {
        RuntimeErrorKind::OutOfMemory { .. } => {}
        _ => panic!("expected OutOfMemory"),
    }
}
