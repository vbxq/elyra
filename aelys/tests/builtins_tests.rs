use aelys_runtime::{VM, register_builtins};
use aelys_syntax::Source;

fn make_test_vm() -> VM {
    let source = Source::new("test.aelys".to_string(), "".to_string());
    VM::new(source).unwrap()
}

#[test]
fn test_register_builtins() {
    let mut vm = make_test_vm();
    register_builtins(&mut vm).unwrap();

    assert!(vm.get_global("type").is_none());
    assert!(vm.get_global("__tostring").is_some());
}

