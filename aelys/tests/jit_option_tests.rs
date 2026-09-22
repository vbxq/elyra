use aelys_runtime::{VmArgsError, parse_vm_args};

#[test]
fn the_jit_is_off_unless_it_is_asked_for() {
    let parsed = parse_vm_args(&[]).expect("parses defaults");
    assert!(!parsed.jit);
}

#[test]
fn the_jit_is_turned_on_by_its_option() {
    for argument in ["-ae.jit=on", "-ae.jit=true", "-ae.jit=1", "--ae-jit=on"] {
        let parsed = parse_vm_args(&[argument.to_string()]).expect("parses");
        assert!(parsed.jit, "{argument} must turn the jit on");
    }
}

#[test]
fn the_jit_is_turned_off_by_its_option() {
    for argument in ["-ae.jit=off", "-ae.jit=false", "-ae.jit=0"] {
        let parsed = parse_vm_args(&[argument.to_string()]).expect("parses");
        assert!(!parsed.jit, "{argument} must leave the jit off");
    }
}

#[test]
fn an_unreadable_jit_value_is_refused() {
    let error = parse_vm_args(&["-ae.jit=maybe".to_string()])
        .err()
        .expect("must be refused");
    match error {
        VmArgsError::InvalidValue { arg, value, .. } => {
            assert_eq!(arg, "-ae.jit=maybe");
            assert_eq!(value, "maybe");
        }
        other => panic!("expected an invalid value, got {other:?}"),
    }
}

#[test]
fn an_executor_is_built_only_for_a_mode_that_compiles() {
    let off = aelys::new_jit_executor(aelys::JitMode::Off, aelys::JitConfig::default())
        .expect("a mode that compiles nothing is not an error");
    assert!(off.is_none());

    let tiered = aelys::new_jit_executor(aelys::JitMode::Tiered, aelys::JitConfig::default())
        .expect("the tiered mode initializes");
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert!(tiered.is_some(), "this target compiles");
    } else {
        assert!(tiered.is_none(), "this target compiles nothing");
    }
}
