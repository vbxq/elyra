use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime};
use aelys_common::error::{AelysError, RuntimeErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

mod buffers {
    use aelys_native::aelys_module;

    #[aelys_module(name = "buffers", version = "0.1.0")]
    mod exports {
        use aelys_native::{
            AELYS_NATIVE_INVALID_ARGUMENT, AelysValue, CallContext, NativeContext, value_as_int,
            value_int, value_is_int, with_call_context,
        };

        #[aelys_export]
        pub fn sum(buffer: &[u8]) -> i64 {
            buffer.iter().map(|&byte| i64::from(byte)).sum()
        }

        #[aelys_export]
        pub fn fill(buffer: &mut [u8], value: i64) {
            buffer.fill(value as u8);
        }

        #[aelys_export]
        pub fn copy_into(target: &mut [u8], source: &[u8]) -> i64 {
            let count = target.len().min(source.len());
            target[..count].copy_from_slice(&source[..count]);
            count as i64
        }

        #[aelys_export]
        pub fn both_lengths(first: &[u8], second: &[u8]) -> i64 {
            (first.len() * 1000 + second.len()) as i64
        }

        #[aelys_export]
        pub fn describe(context: &mut CallContext<'_>, buffer: &[u8], label: String) -> String {
            let echoed = context
                .alloc_string(&label)
                .expect("the vm allocates the label");
            let read_back = context.read_string(echoed).expect("the label reads back");
            format!("{read_back}:{}", buffer.len())
        }

        fn reply(out: *mut AelysValue, value: i64) -> i32 {
            unsafe { *out = value_int(value).unwrap() };
            0
        }

        #[aelys_export(params = [BytesMut], result = Int)]
        pub unsafe extern "C" fn borrow_twice(
            context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let vector = unsafe { *args };
            let value = unsafe {
                with_call_context(context, |call| {
                    let first = match call.borrow_bytes(vector) {
                        Ok(bytes) => bytes,
                        Err(code) => return -i64::from(code),
                    };
                    first[0] = 42;
                    match call.borrow_bytes(vector) {
                        Ok(_) => -1,
                        Err(code) => i64::from(code) * 100 + i64::from(first[0]),
                    }
                })
            };
            reply(out, value)
        }

        #[aelys_export(params = [BytesMut, Bytes], result = Int)]
        pub unsafe extern "C" fn mut_then_shared(
            context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let (target, other) = unsafe { (*args, *args.add(1)) };
            let value = unsafe {
                with_call_context(context, |call| {
                    let buffer = match call.borrow_bytes(target) {
                        Ok(bytes) => bytes,
                        Err(code) => return -i64::from(code),
                    };
                    buffer[0] = 9;
                    match call.borrow_bytes_ref(other) {
                        Ok(view) => 1000 + i64::from(view[0]),
                        Err(code) => i64::from(code),
                    }
                })
            };
            reply(out, value)
        }

        #[aelys_export(params = [Int], result = Int)]
        pub unsafe extern "C" fn borrow_an_int(
            context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let value = unsafe {
                with_call_context(context, |call| match call.borrow_bytes_ref(*args) {
                    Ok(bytes) => bytes.len() as i64,
                    Err(code) => -i64::from(code),
                })
            };
            reply(out, value)
        }

        #[aelys_export(params = [Int, Bytes], result = Int)]
        pub unsafe extern "C" fn peek(
            context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let (index, handle) = unsafe { (*args, *args.add(1)) };
            if !value_is_int(index) {
                return AELYS_NATIVE_INVALID_ARGUMENT;
            }
            let status = unsafe {
                with_call_context(context, |call| {
                    let bytes = call.borrow_bytes_ref(handle)?;
                    let index = usize::try_from(value_as_int(index))
                        .map_err(|_| AELYS_NATIVE_INVALID_ARGUMENT)?;
                    let byte = *bytes.get(index).ok_or(AELYS_NATIVE_INVALID_ARGUMENT)?;
                    value_int(i64::from(byte)).ok_or(AELYS_NATIVE_INVALID_ARGUMENT)
                })
            };
            match status {
                Ok(value) => {
                    unsafe { *out = value };
                    0
                }
                Err(code) => code,
            }
        }

        #[aelys_export(arity = 0)]
        pub unsafe extern "C" fn untyped(
            _context: *mut NativeContext,
            _args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            unsafe { *out = value_int(7).unwrap() };
            0
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_62756666657273;
        unsafe { &*pointer }
    }
}

fn runtime() -> Runtime {
    let runtime = Runtime::new();
    unsafe { runtime.register_native_module(buffers::descriptor()) }.unwrap();
    runtime
}

fn run(source: &str) -> Result<String, AelysError> {
    let runtime = runtime();
    let module = runtime.compile(source, CompileOptions::default())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate.execute(&module, RunOptions::default())? {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => panic!("expected a value, got {other:?}"),
    }
}

fn run_ok(source: &str) -> String {
    run(source).unwrap_or_else(|error| panic!("{error}\n--- source\n{source}"))
}

fn run_runtime_error(source: &str) -> (RuntimeErrorKind, String) {
    match run(source) {
        Err(AelysError::Runtime(error)) => {
            let text = error.to_string();
            (error.kind, text)
        }
        Err(other) => panic!("expected a runtime error, got {other}"),
        Ok(value) => panic!("expected a runtime error, got {value}"),
    }
}

const SETUP: &str = "let mut a: Vec<u8> = vec![1, 2, 3, 250]\nlet mut b: Vec<u8> = vec![0; 4]\n";

#[test]
fn a_typed_export_reads_a_borrowed_buffer() {
    assert_eq!(run_ok(&format!("{SETUP}buffers::sum(&a)")), "256");
    assert_eq!(
        run_ok(&format!(
            "{SETUP}fn total(h: Vec<u8>) -> int {{ return buffers::sum(&h) + buffers::sum(&h) }}\ntotal(a)"
        )),
        "512"
    );
    assert_eq!(
        run_ok(&format!(
            "{SETUP}let mut s = 0\nfor i in 0..3 {{ s += buffers::sum(&a) }}\ns"
        )),
        "768"
    );
}

#[test]
fn a_mutable_borrow_writes_through_to_the_elyra_buffer() {
    assert_eq!(
        run_ok(&format!(
            "{SETUP}buffers::fill(&mut b, 7)\nb.read_u8(0) * 1000 + b.read_u8(3)"
        )),
        "7007"
    );
    assert_eq!(
        run_ok(&format!(
            "{SETUP}let n = buffers::copy_into(&mut b, &a)\nn * 1000 + b.read_u8(3)"
        )),
        "4250"
    );
}

#[test]
fn two_distinct_buffers_borrow_in_one_call() {
    let source = format!("{SETUP}let c: Vec<u8> = vec![0; 9]\nbuffers::both_lengths(&a, &c)");
    assert_eq!(run_ok(&source), "4009");
}

#[test]
fn one_vec_borrowed_mutably_and_again_in_one_call_is_refused() {
    let (kind, text) =
        run_runtime_error(&format!("{SETUP}let copy = buffers::copy_into\ncopy(a, a)"));
    assert!(
        matches!(kind, RuntimeErrorKind::NativeError { code: 5 }),
        "{kind:?}"
    );
    assert!(
        text.contains("borrowed mutably while another borrow of it is live"),
        "{text}"
    );
    let error = run(&format!("{SETUP}buffers::copy_into(&mut a, &a)"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("E0416") || error.contains("E0415"),
        "{error}"
    );
    assert_eq!(
        run_ok(&format!("{SETUP}buffers::both_lengths(&a, &a)")),
        "4004"
    );
    assert_eq!(
        run_ok(&format!(
            "{SETUP}let r = buffers::borrow_twice(&mut a)\nr * 1000 + a.read_u8(0)"
        )),
        "542042"
    );
    assert_eq!(
        run_ok(&format!(
            "{SETUP}let f = buffers::mut_then_shared\nf(a, a) * 1000 + a.read_u8(0)"
        )),
        "5009"
    );
    assert_eq!(
        run_ok(&format!(
            "{SETUP}b[0] = 66\nbuffers::mut_then_shared(&mut a, &b) * 1000 + a.read_u8(0)"
        )),
        "1066009"
    );
    assert_eq!(run_ok("buffers::borrow_an_int(7)"), "-1");
}

#[test]
fn a_borrow_ends_with_its_call() {
    let source = format!(
        "{SETUP}let mut s = 0\nfor i in 0..4 {{ s += buffers::copy_into(&mut b, &a) }}\ns + buffers::sum(&b)"
    );
    assert_eq!(run_ok(&source), "272");
}

#[test]
fn a_typed_export_takes_the_context_and_allocates_a_string() {
    assert_eq!(
        run_ok(&format!("{SETUP}buffers::describe(&a, \"frame\")")),
        "frame:4"
    );
}

#[test]
fn a_byte_parameter_takes_a_vec_of_u8_only() {
    for source in [
        "buffers::sum(123456)",
        "let t = time::timer()\nbuffers::sum(&t)",
        "let v: Vec<int> = vec![1]\nbuffers::sum(&v)",
        "let s = \"text\"\nbuffers::sum(&s)",
    ] {
        let error = run(source).unwrap_err().to_string();
        assert!(
            error.contains("E0301") || error.contains("E0417"),
            "{source}: {error}"
        );
    }
}

#[test]
fn a_raw_export_declares_its_signature_and_arity() {
    assert_eq!(run_ok(&format!("{SETUP}buffers::peek(3, &a)")), "250");
    let error = run(&format!("{SETUP}buffers::peek(&a)"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("E0301") || error.contains("argument"),
        "{error}"
    );
    let error = run(&format!("{SETUP}buffers::peek(true, &a)"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("type mismatch"), "{error}");
    let (kind, _) = run_runtime_error(&format!("{SETUP}buffers::peek(9, &a)"));
    assert!(
        matches!(kind, RuntimeErrorKind::NativeError { code: 1 }),
        "{kind:?}"
    );
}

#[test]
fn a_raw_export_with_only_an_arity_stays_out_of_typed_code() {
    let error = run("buffers::untyped()").unwrap_err().to_string();
    assert!(error.contains("E0379"), "{error}");
}

fn native_crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("native")
}

fn check_probe(name: &str, body: &str) -> (bool, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/t4_borrow_probes");
    let crate_dir = root.join(name);
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"rlib\"]\n\n[dependencies]\naelys-native = {{ path = \"{}\" }}\n\n[workspace]\n",
            native_crate_dir().display()
        ),
    )
    .unwrap();
    let lock = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("Cargo.lock");
    std::fs::copy(lock, crate_dir.join("Cargo.lock")).unwrap();
    std::fs::write(crate_dir.join("src/lib.rs"), body).unwrap();
    let output = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--offline")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("cargo check runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

const PROBE_HEADER: &str = "#![allow(static_mut_refs, dead_code)]\nuse aelys_native::*;\nstatic mut KEPT: Option<&'static [u8]> = None;\n";

#[test]
fn a_borrowed_slice_cannot_outlive_its_call() {
    let control = format!(
        "{PROBE_HEADER}#[aelys_module(name = \"probe_ok\")]\nmod exports {{\n    use aelys_native::*;\n    #[aelys_export]\n    pub fn keep(buffer: &[u8]) -> i64 {{ buffer.len() as i64 }}\n    #[aelys_export(params = [Bytes], result = Int)]\n    pub unsafe extern \"C\" fn raw(context: *mut NativeContext, args: *const AelysValue, _n: usize, out: *mut AelysValue) -> i32 {{\n        unsafe {{ with_call_context(context, |call| {{ let _ = call.borrow_bytes_ref(*args); *out = value_int(0).unwrap(); }}) }};\n        0\n    }}\n}}\n"
    );
    let (ok, stderr) = check_probe("borrow_probe_control", &control);
    assert!(ok, "the control module must compile:\n{stderr}");

    let escapes = [
        (
            "borrow_probe_param",
            "#[aelys_module(name = \"probe_param\")]\nmod exports {\n    #[aelys_export]\n    pub fn keep(buffer: &[u8]) -> i64 {\n        unsafe { super::KEPT = Some(buffer) };\n        0\n    }\n}\n",
        ),
        (
            "borrow_probe_context",
            "#[aelys_module(name = \"probe_context\")]\nmod exports {\n    use aelys_native::*;\n    #[aelys_export]\n    pub fn keep(context: &mut CallContext<'_>, handle: i64) -> i64 {\n        let bytes = context.borrow_bytes_ref(value_int(handle).unwrap()).unwrap();\n        unsafe { super::KEPT = Some(bytes) };\n        0\n    }\n}\n",
        ),
        (
            "borrow_probe_static_context",
            "#[aelys_module(name = \"probe_static\")]\nmod exports {\n    use aelys_native::*;\n    #[aelys_export]\n    pub fn keep(context: &mut CallContext<'static>) -> i64 {\n        0\n    }\n}\n",
        ),
        (
            "borrow_probe_raw",
            "pub unsafe fn raw(context: *mut NativeContext, handle: AelysValue) {\n    unsafe { with_call_context(context, |call| { KEPT = call.borrow_bytes_ref(handle).ok(); }) }\n}\n",
        ),
    ];
    for (name, body) in escapes {
        let (ok, stderr) = check_probe(name, &format!("{PROBE_HEADER}{body}"));
        assert!(!ok, "{name} must not compile");
        assert!(
            stderr.contains("borrowed data escapes")
                || stderr.contains("lifetime may not live long enough")
                || stderr.contains("does not live long enough")
                || stderr.contains("one type is more general than the other")
                || stderr.contains("implementation of `FnOnce` is not general enough"),
            "{name} must fail on the borrow lifetime:\n{stderr}"
        );
    }
}
