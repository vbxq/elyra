use aelys::{
    CallableFunction, CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions,
    Runtime, Value,
};
use aelys_common::error::{AelysError, CompileErrorKind, RuntimeErrorKind};

const PROBE_SOURCE: &str = "fn main() { probe.answer() }";
const PROBE_CALL: &str = "probe.answer()";

fn register(runtime: &Runtime) -> Result<(), AelysError> {
    unsafe { runtime.register_native_module(probe_module::descriptor()) }
}

fn rejection_reason(error: AelysError) -> String {
    match error {
        AelysError::Compile(error) => match error.kind {
            CompileErrorKind::InvalidNativeModule { module, reason } => {
                format!("{module}: {reason}")
            }
            other => panic!("expected InvalidNativeModule, got {other:?}"),
        },
        other => panic!("expected a compile error, got {other:?}"),
    }
}

#[test]
fn a_registered_native_module_compiles() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();

    runtime
        .compile(PROBE_SOURCE, CompileOptions::default())
        .unwrap();
}

#[test]
fn an_empty_runtime_compiles_exactly_as_before() {
    let runtime = Runtime::new();
    assert!(
        runtime
            .compile(PROBE_SOURCE, CompileOptions::default())
            .is_err()
    );
    runtime
        .compile("fn main() { sys.pid() }", CompileOptions::default())
        .unwrap();
}

#[test]
fn a_duplicate_alias_is_rejected() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();

    assert_eq!(
        rejection_reason(register(&runtime).unwrap_err()),
        "probe: a native module with this alias is already registered"
    );
    runtime
        .compile(PROBE_SOURCE, CompileOptions::default())
        .unwrap();
}

#[test]
fn a_rejected_alias_never_initializes_the_module() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();

    let error = unsafe { runtime.register_native_module(probe_shadow::descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "probe: a native module with this alias is already registered"
    );
    assert_eq!(probe_shadow::init_calls(), 0);

    let error = unsafe { runtime.register_native_module(sys_shadow::descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "sys: alias is reserved by a standard module"
    );
    assert_eq!(sys_shadow::init_calls(), 0);
}

#[test]
fn a_descriptor_with_an_unknown_abi_is_not_named() {
    let runtime = Runtime::new();
    let error = unsafe { runtime.register_native_module(future_abi_descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        format!(
            "<unknown>: abi version mismatch: expected {}, found {}",
            aelys_native::AELYS_ABI_VERSION,
            aelys_native::AELYS_ABI_VERSION + 1
        )
    );
}

#[test]
fn the_reservation_covers_the_auto_registered_standard_modules() {
    let runtime = Runtime::new();
    let error = unsafe { runtime.register_native_module(math_shadow::descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "math: alias is reserved by a standard module"
    );
    assert_eq!(math_shadow::init_calls(), 0);

    let module = runtime
        .compile("math.floor(2.5)", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(2))
    );
}

#[test]
fn an_alias_reserved_by_a_standard_module_is_rejected() {
    let runtime = Runtime::new();
    let error = unsafe { runtime.register_native_module(sys_shadow::descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "sys: alias is reserved by a standard module"
    );
    runtime
        .compile("fn main() { sys.pid() }", CompileOptions::default())
        .unwrap();
}

#[test]
fn a_registered_native_module_executes() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();

    let module = runtime
        .compile(PROBE_CALL, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let outcome = isolate.execute(&module, RunOptions::default()).unwrap();
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    assert_eq!(isolate.value_to_string(value), "42");
}

#[test]
fn every_isolate_inherits_the_registered_module() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();
    let module = runtime
        .compile(PROBE_CALL, CompileOptions::default())
        .unwrap();

    for _ in 0..3 {
        let mut isolate = runtime.new_isolate(IsolateConfig::default());
        assert!(isolate.execute(&module, RunOptions::default()).is_ok());
    }
}

#[test]
fn isolates_bind_only_the_modules_registered_before_them() {
    let runtime = Runtime::new();
    let mut stale = runtime.new_isolate(IsolateConfig::default());
    register(&runtime).unwrap();

    let module = runtime
        .compile(PROBE_CALL, CompileOptions::default())
        .unwrap();
    assert!(stale.execute(&module, RunOptions::default()).is_err());

    let mut fresh = runtime.new_isolate(IsolateConfig::default());
    assert!(fresh.execute(&module, RunOptions::default()).is_ok());
}

#[test]
fn an_empty_runtime_builds_isolates_as_before() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("sys.pid()", CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert!(isolate.execute(&module, RunOptions::default()).is_ok());
}

#[test]
fn a_descriptor_failing_validation_is_rejected() {
    let runtime = Runtime::new();
    let error = unsafe { runtime.register_native_module(unhashed_descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "hollow: invalid descriptor: exports_hash is missing"
    );
}

const TWICE_SOURCE: &str = "fn twice(n: int) -> int { return n * 2 }\n0";

#[test]
fn a_named_function_is_callable_repeatedly() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(TWICE_SOURCE, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate.execute(&module, RunOptions::default()).unwrap();

    let twice = isolate.get_function("twice").unwrap();
    assert_eq!(twice.arity(), 1);
    for n in 0..1_000_i64 {
        let result = isolate.call(&twice, &[Value::int(n)]).unwrap();
        assert_eq!(isolate.value_to_string(result), (n * 2).to_string());
    }
}

#[test]
fn a_function_is_unresolvable_before_the_module_executes() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(TWICE_SOURCE, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    assert!(isolate.get_function("twice").is_err());
    isolate.execute(&module, RunOptions::default()).unwrap();
    assert!(isolate.get_function("twice").is_ok());
}

#[test]
fn re_executing_a_module_lets_the_function_be_resolved_again() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(TWICE_SOURCE, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    for _ in 0..3 {
        isolate.execute(&module, RunOptions::default()).unwrap();
        let twice = isolate.get_function("twice").unwrap();
        let result = isolate.call(&twice, &[Value::int(21)]).unwrap();
        assert_eq!(isolate.value_to_string(result), "42");
    }
}

#[test]
fn a_failed_run_does_not_clobber_the_globals_it_already_defined() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "fn helper(n: int) -> int { return n * 2 }\n\
             let mut x = 0\n\
             x = 0\n\
             fn boom(n: int) -> int { return n / x }\n\
             boom(1)",
            CompileOptions::default(),
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let error = isolate.execute(&module, RunOptions::default()).unwrap_err();
    assert!(
        matches!(runtime_error_kind(error), RuntimeErrorKind::DivisionByZero));

    let helper = isolate
        .get_function("helper")
        .expect("a function defined before the failure stays resolvable");
    let result = isolate.call(&helper, &[Value::int(21)]).unwrap();
    assert_eq!(isolate.value_to_string(result), "42");
}

#[test]
fn execute_still_accepts_a_bare_module() {
    let runtime = Runtime::new();
    let module = runtime.compile("7", CompileOptions::default()).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let outcome = isolate.execute(&module, RunOptions::default()).unwrap();
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    assert_eq!(isolate.value_to_string(value), "7");
}

#[test]
fn an_instance_is_deserialized_once() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime.compile("1", CompileOptions::default()).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let instance = isolate.instantiate(&module).unwrap();
    let options = RunOptions {
        report: true,
        ..RunOptions::default()
    };

    isolate.execute(&module, options.clone()).unwrap();
    let per_execute = isolate.last_report().unwrap().allocations;
    assert!(
        per_execute > 0);

    isolate
        .execute_instance(&instance, options.clone())
        .unwrap();
    let first = isolate.last_report().unwrap().allocations;
    for _ in 0..1_000 {
        isolate
            .execute_instance(&instance, options.clone())
            .unwrap();
    }
    let thousandth = isolate.last_report().unwrap().allocations;

    assert_eq!(
        (first, thousandth),
        (0, 0));
}

#[test]
fn an_instance_survives_a_collection_between_runs() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(TWICE_SOURCE, CompileOptions::default())
        .unwrap();
    let churn = runtime
        .compile(
            "let mut total = 0\n\
             let mut i = 0\n\
             while i < 60000 {\n\
                 let junk = [i, i, i, i, i, i, i, i]\n\
                 total += junk[7]\n\
                 i++\n\
             }\n\
             total",
            CompileOptions::default(),
        )
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());

    let instance = isolate.instantiate(&module).unwrap();
    isolate
        .execute_instance(&instance, RunOptions::default())
        .unwrap();

    let options = RunOptions {
        report: true,
        ..RunOptions::default()
    };
    isolate.execute(&churn, options).unwrap();
    assert!(
        isolate.last_report().unwrap().collections > 0);

    isolate
        .execute_instance(&instance, RunOptions::default())
        .unwrap();
    let twice = isolate.get_function("twice").unwrap();
    let result = isolate.call(&twice, &[Value::int(21)]).unwrap();
    assert_eq!(isolate.value_to_string(result), "42");
}

#[test]
fn an_instance_belongs_to_the_isolate_that_created_it() {
    let runtime = Runtime::new();
    let one = runtime.compile("1", CompileOptions::default()).unwrap();
    let two = runtime.compile("2", CompileOptions::default()).unwrap();

    let mut owner = runtime.new_isolate(IsolateConfig::default());
    let mut other = runtime.new_isolate(IsolateConfig::default());
    let foreign = owner.instantiate(&one).unwrap();
    let native = other.instantiate(&two).unwrap();

    let error = other.execute_instance(&foreign, RunOptions::default());
    assert!(
        matches!(
            error.map_err(runtime_error_kind),
            Err(RuntimeErrorKind::InvalidMemoryHandle)
        ));

    let outcome = other
        .execute_instance(&native, RunOptions::default())
        .unwrap();
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    assert_eq!(other.value_to_string(value), "2");
}

#[test]
fn a_module_from_another_runtime_is_not_served_the_wrong_compiled_root() {
    let other = Runtime::with_jit_mode(JitMode::Baseline);
    let host = Runtime::with_jit_mode(JitMode::Baseline);
    let foreign = other.compile("42", CompileOptions::default()).unwrap();
    let own = host.compile("4", CompileOptions::default()).unwrap();

    let mut isolate = host.new_isolate(IsolateConfig::default());
    let outcome = isolate.execute(&own, RunOptions::default()).unwrap();
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    assert_eq!(isolate.value_to_string(value), "4");

    let outcome = isolate.execute(&foreign, RunOptions::default()).unwrap();
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    assert_eq!(
        isolate.value_to_string(value),
        "42");
}

const BOUNDED_LOAD_SOURCE: &str = "fn work() -> int {\n\
                                       let mut total = 0\n\
                                       let mut i = 0\n\
                                       while i < 100000 {\n\
                                           total = total + i\n\
                                           i = i + 1\n\
                                       }\n\
                                       total\n\
                                   }\n\
                                   0";
const BOUNDED_LOAD_RESULT: &str = "4999950000";

#[test]
fn a_default_run_does_not_leave_the_previous_runs_limits_installed() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let script = runtime
        .compile(BOUNDED_LOAD_SOURCE, CompileOptions::default())
        .unwrap();
    let hot = runtime.compile("4", CompileOptions::default()).unwrap();

    let mut control = runtime.new_isolate(IsolateConfig::default());
    control.execute(&script, RunOptions::default()).unwrap();
    let work = control.get_function("work").unwrap();
    let value = control.call(&work, &[]).unwrap();
    assert_eq!(control.value_to_string(value), BOUNDED_LOAD_RESULT);

    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let bounded = RunOptions {
        max_instructions: Some(50),
        ..RunOptions::default()
    };
    isolate.execute(&script, bounded).unwrap();
    let work = isolate.get_function("work").unwrap();

    isolate.execute(&hot, RunOptions::default()).unwrap();

    let value = isolate
        .call(&work, &[])
        .expect("the default run's absence of limits must be what is in force");
    assert_eq!(isolate.value_to_string(value), BOUNDED_LOAD_RESULT);
}

#[test]
fn a_statically_registered_module_may_not_declare_dependencies() {
    let runtime = Runtime::new();
    let error =
        unsafe { runtime.register_native_module(dependent_module::descriptor()) }.unwrap_err();
    assert_eq!(
        rejection_reason(error),
        "dependent: statically linked modules cannot declare dependencies"
    );
    assert_eq!(dependent_module::init_calls(), 0);

    assert!(
        runtime
            .compile("dependent.noop()", CompileOptions::default())
            .is_err()
    );
}

fn runtime_error_kind(error: AelysError) -> RuntimeErrorKind {
    match error {
        AelysError::Runtime(error) => error.kind,
        other => panic!("expected a runtime error, got {other:?}"),
    }
}

#[test]
fn an_unknown_function_name_is_an_error_not_a_panic() {
    let runtime = Runtime::new();
    let module = runtime.compile("0", CompileOptions::default()).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate.execute(&module, RunOptions::default()).unwrap();

    assert!(matches!(
        runtime_error_kind(isolate.get_function("absent").unwrap_err()),
        RuntimeErrorKind::UndefinedVariable(_)
    ));
}

#[test]
fn a_name_that_is_not_a_function_is_an_error_not_a_panic() {
    for source in [
        "let mut answer = 41\nanswer = answer + 1\nanswer",
        "let answer = [1, 2, 3]\nanswer",
    ] {
        let runtime = Runtime::new();
        let module = runtime.compile(source, CompileOptions::default()).unwrap();
        let mut isolate = runtime.new_isolate(IsolateConfig::default());
        isolate.execute(&module, RunOptions::default()).unwrap();

        let Err(error) = isolate.get_function("answer") else {
            panic!("{source:?}: a non-function global must not resolve");
        };
        assert!(
            matches!(runtime_error_kind(error), RuntimeErrorKind::NotCallable(_)));
    }
}

#[test]
fn calling_with_the_wrong_arity_is_an_error() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(TWICE_SOURCE, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate.execute(&module, RunOptions::default()).unwrap();

    let twice: CallableFunction = isolate.get_function("twice").unwrap();
    assert!(isolate.call(&twice, &[]).is_err());

    let result = isolate.call(&twice, &[Value::int(21)]).unwrap();
    assert_eq!(isolate.value_to_string(result), "42");
}

#[test]
fn a_native_module_export_is_callable_by_name() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();
    let module = runtime
        .compile(PROBE_CALL, CompileOptions::default())
        .unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate.execute(&module, RunOptions::default()).unwrap();

    let answer = isolate.get_function("probe::answer").unwrap();
    let result = isolate.call(&answer, &[]).unwrap();
    assert_eq!(isolate.value_to_string(result), "42");
}

const FRAME_HOOK_SOURCE: &str = "fn frame(n: int) -> int { probe.answer() + n }\n0";

const FRAME_TICK_SOURCE: &str = "let width = 384\nlet height = 448\n(width + height) * 2";
const FRAME_TICK_RESULT: i64 = (384 + 448) * 2;

#[test]
fn a_game_engine_frame_loop_runs_without_growing_the_heap() {
    let runtime = Runtime::new();
    register(&runtime).unwrap();

    let script = runtime
        .compile(FRAME_HOOK_SOURCE, CompileOptions::default())
        .unwrap();
    let tick = runtime
        .compile(FRAME_TICK_SOURCE, CompileOptions::default())
        .unwrap();

    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let script_instance = isolate.instantiate(&script).unwrap();
    let tick_instance = isolate.instantiate(&tick).unwrap();
    let options = RunOptions {
        report: true,
        ..RunOptions::default()
    };

    isolate
        .execute_instance(&script_instance, options.clone())
        .unwrap();
    let after_first_run = isolate.last_report().unwrap().allocated_bytes;
    isolate
        .execute_instance(&script_instance, options.clone())
        .unwrap();
    let (per_run_bytes, per_run_allocations, before_loop) = {
        let report = isolate.last_report().unwrap();
        (
            report
                .allocated_bytes
                .checked_sub(after_first_run)
                .expect("the heap must not have shrunk between two runs that allocate the same"),
            report.allocations,
            report.allocated_bytes,
        )
    };

    let frame: CallableFunction = isolate.get_function("frame").unwrap();
    assert_eq!(frame.arity(), 1);

    for n in 0..3_600_i64 {
        assert_eq!(
            isolate
                .execute_instance(&tick_instance, options.clone())
                .unwrap(),
            ExecutionOutcome::Returned(Value::int(FRAME_TICK_RESULT))
        );
        let value = isolate.call(&frame, &[Value::int(n)]).unwrap();
        assert_eq!(isolate.value_to_string(value), (42 + n).to_string());
    }

    assert!(
        runtime.jit_cache_entries() >= 1);
    assert_eq!(
        runtime.jit_deoptimizations(),
        0);
    let last_tick = isolate.last_report().unwrap();
    assert_eq!(
        last_tick.instructions, 0);
    assert_eq!(
        last_tick.allocations, 0);

    isolate
        .execute_instance(&script_instance, options.clone())
        .unwrap();
    let (across_loop, post_loop_allocations) = {
        let report = isolate.last_report().unwrap();
        (
            report.allocated_bytes.saturating_sub(before_loop),
            report.allocations,
        )
    };
    assert!(
        across_loop <= per_run_bytes);
    assert_eq!(
        post_loop_allocations, per_run_allocations);

    isolate.execute(&script, options).unwrap();
    let per_execute = isolate.last_report().unwrap().allocations;
    assert_eq!(
        (per_run_allocations, per_execute),
        (1, 2));
}

fn future_abi_descriptor() -> &'static aelys_native::AelysModuleDescriptor {
    static NAME: &[u8] = b"from-the-future\0";
    static DESCRIPTOR: aelys_native::AelysModuleDescriptor = aelys_native::AelysModuleDescriptor {
        abi_version: aelys_native::AELYS_ABI_VERSION + 1,
        descriptor_size: size_of::<aelys_native::AelysModuleDescriptor>() as u32,
        module_name: NAME.as_ptr() as *const std::ffi::c_char,
        module_version: std::ptr::null(),
        vm_version_min: std::ptr::null(),
        vm_version_max: std::ptr::null(),
        descriptor_hash: 0,
        exports_hash: 0,
        export_count: 0,
        exports: std::ptr::null(),
        required_module_count: 0,
        required_modules: std::ptr::null(),
        init: None,
    };
    &DESCRIPTOR
}

fn unhashed_descriptor() -> &'static aelys_native::AelysModuleDescriptor {
    static NAME: &[u8] = b"hollow\0";
    static DESCRIPTOR: aelys_native::AelysModuleDescriptor = aelys_native::AelysModuleDescriptor {
        abi_version: aelys_native::AELYS_ABI_VERSION,
        descriptor_size: size_of::<aelys_native::AelysModuleDescriptor>() as u32,
        module_name: NAME.as_ptr() as *const std::ffi::c_char,
        module_version: std::ptr::null(),
        vm_version_min: std::ptr::null(),
        vm_version_max: std::ptr::null(),
        descriptor_hash: 0,
        exports_hash: 0,
        export_count: 0,
        exports: std::ptr::null(),
        required_module_count: 0,
        required_modules: std::ptr::null(),
        init: None,
    };
    &DESCRIPTOR
}

mod probe_module {
    use aelys_native::aelys_module;

    #[aelys_module(name = "probe", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn answer() -> i64 {
            42
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor;
        unsafe { &*pointer }
    }
}

mod dependent_module {
    use aelys_native::{
        AelysExport, AelysExportKind, AelysModuleDescriptor, AelysRequiredModule, AelysVmApi,
    };
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NAME: &[u8] = b"dependent\0";
    static EXPORT_NAME: &[u8] = b"noop\0";
    static REQUIRED_NAME: &[u8] = b"probe\0";
    static REQUIRED_VERSION: &[u8] = b"^0.1\0";
    static INIT_CALLS: AtomicUsize = AtomicUsize::new(0);

    pub fn init_calls() -> usize {
        INIT_CALLS.load(Ordering::Relaxed)
    }

    extern "C" fn dependent_init(_api: *const AelysVmApi) -> i32 {
        INIT_CALLS.fetch_add(1, Ordering::Relaxed);
        0
    }

    unsafe extern "C" fn dependent_export(
        _context: *mut aelys_native::NativeContext,
        _args: *const aelys_native::AelysValue,
        _arg_count: usize,
        _out: *mut aelys_native::AelysValue,
    ) -> i32 {
        0
    }

    static EXPORTS: [AelysExport; 1] = [AelysExport {
        name: EXPORT_NAME.as_ptr() as *const c_char,
        kind: AelysExportKind::Function,
        arity: 0,
        _padding: [0; 2],
        value: dependent_export as *const c_void,
    }];

    static REQUIRED: [AelysRequiredModule; 1] = [AelysRequiredModule {
        name: REQUIRED_NAME.as_ptr() as *const c_char,
        version_req: REQUIRED_VERSION.as_ptr() as *const c_char,
    }];

    static mut DESCRIPTOR: AelysModuleDescriptor = AelysModuleDescriptor {
        abi_version: aelys_native::AELYS_ABI_VERSION,
        descriptor_size: size_of::<AelysModuleDescriptor>() as u32,
        module_name: NAME.as_ptr() as *const c_char,
        module_version: std::ptr::null(),
        vm_version_min: std::ptr::null(),
        vm_version_max: std::ptr::null(),
        descriptor_hash: 0,
        exports_hash: 0,
        export_count: 1,
        exports: EXPORTS.as_ptr(),
        required_module_count: 1,
        required_modules: REQUIRED.as_ptr(),
        init: Some(dependent_init),
    };

    aelys_native::aelys_init_exports_hash!(DESCRIPTOR);

    pub unsafe fn descriptor() -> &'static AelysModuleDescriptor {
        let pointer = &raw const DESCRIPTOR;
        unsafe { &*pointer }
    }
}

macro_rules! counting_shadow_module {
    ($module:ident, $name:literal, $export:literal) => {
        mod $module {
            use aelys_native::{AelysExport, AelysExportKind, AelysModuleDescriptor, AelysVmApi};
            use std::ffi::{c_char, c_void};
            use std::sync::atomic::{AtomicUsize, Ordering};

            static NAME: &[u8] = concat!($name, "\0").as_bytes();
            static EXPORT_NAME: &[u8] = concat!($export, "\0").as_bytes();
            static INIT_CALLS: AtomicUsize = AtomicUsize::new(0);

            pub fn init_calls() -> usize {
                INIT_CALLS.load(Ordering::Relaxed)
            }

            extern "C" fn shadow_init(_api: *const AelysVmApi) -> i32 {
                INIT_CALLS.fetch_add(1, Ordering::Relaxed);
                0
            }

            unsafe extern "C" fn shadow_export(
                _context: *mut aelys_native::NativeContext,
                _args: *const aelys_native::AelysValue,
                _arg_count: usize,
                _out: *mut aelys_native::AelysValue,
            ) -> i32 {
                0
            }

            static EXPORTS: [AelysExport; 1] = [AelysExport {
                name: EXPORT_NAME.as_ptr() as *const c_char,
                kind: AelysExportKind::Function,
                arity: 0,
                _padding: [0; 2],
                value: shadow_export as *const c_void,
            }];

            static mut DESCRIPTOR: AelysModuleDescriptor = AelysModuleDescriptor {
                abi_version: aelys_native::AELYS_ABI_VERSION,
                descriptor_size: size_of::<AelysModuleDescriptor>() as u32,
                module_name: NAME.as_ptr() as *const c_char,
                module_version: std::ptr::null(),
                vm_version_min: std::ptr::null(),
                vm_version_max: std::ptr::null(),
                descriptor_hash: 0,
                exports_hash: 0,
                export_count: 1,
                exports: EXPORTS.as_ptr(),
                required_module_count: 0,
                required_modules: std::ptr::null(),
                init: Some(shadow_init),
            };

            aelys_native::aelys_init_exports_hash!(DESCRIPTOR);

            pub fn descriptor() -> &'static AelysModuleDescriptor {
                let pointer = &raw const DESCRIPTOR;
                unsafe { &*pointer }
            }
        }
    };
}

counting_shadow_module!(sys_shadow, "sys", "pid");
counting_shadow_module!(probe_shadow, "probe", "answer");
counting_shadow_module!(math_shadow, "math", "sin");

