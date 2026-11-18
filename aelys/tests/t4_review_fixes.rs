use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime};
use aelys_common::error::{AelysError, RuntimeErrorKind};
use std::path::{Path, PathBuf};
use std::process::Command;

mod fixes {
    use aelys_native::aelys_module;

    #[aelys_module(name = "fixes", version = "0.1.0")]
    mod exports {
        use aelys_native::{
            AelysValue, CallContext, NativeContext, alloc_string_from_context, value_as_int,
            value_int,
        };

        #[aelys_export]
        pub fn keep_first(context: &mut CallContext<'_>, junk: i64, width: i64) -> String {
            let first = context.alloc_string("FIRST-KEPT").unwrap();
            let filler = "x".repeat(width as usize);
            for i in 0..junk {
                context.alloc_string(&format!("{i}-{filler}")).unwrap();
            }
            context
                .read_string(first)
                .unwrap_or_else(|| "<freed>".to_string())
        }

        #[aelys_export(params = [Int, Int], result = String)]
        pub unsafe extern "C" fn return_first(
            context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let (junk, width) = unsafe { (value_as_int(*args), value_as_int(*args.add(1))) };
            let first = unsafe { alloc_string_from_context(context, "RAW-FIRST") }.unwrap();
            let filler = "y".repeat(width as usize);
            for i in 0..junk {
                unsafe { alloc_string_from_context(context, &format!("{i}-{filler}")) }.unwrap();
            }
            unsafe { *out = first };
            0
        }

        #[aelys_export]
        pub fn boom(buffer: &mut [u8]) -> i64 {
            let at = buffer.len() + 1000;
            buffer[at] = 1;
            0
        }

        #[aelys_export]
        pub fn boom_text(text: String) -> Result<String, String> {
            if text.is_empty() {
                return Err("empty".to_string());
            }
            panic!("refusing {text}")
        }

        #[aelys_export(params = [Int], result = Int)]
        pub unsafe extern "C-unwind" fn raw_boom(
            _context: *mut NativeContext,
            args: *const AelysValue,
            _arg_count: usize,
            out: *mut AelysValue,
        ) -> i32 {
            let value = unsafe { value_as_int(*args) };
            if value > 0 {
                panic!("raw export panicked on {value}");
            }
            unsafe { *out = value_int(-value).unwrap() };
            0
        }

        #[aelys_export]
        pub fn sum(buffer: &[u8]) -> i64 {
            buffer.iter().map(|&byte| i64::from(byte)).sum()
        }

        #[aelys_export]
        pub fn pair(first: &[u8], second: &[u8]) -> i64 {
            i64::from(first[0]) * 1000 + i64::from(second[1]) + (first.len() * 10) as i64
        }

        #[aelys_export]
        pub fn copy_into(target: &mut [u8], source: &[u8]) -> i64 {
            let count = target.len().min(source.len());
            target[..count].copy_from_slice(&source[..count]);
            count as i64
        }

        #[aelys_export]
        pub fn shared_by_context(context: &mut CallContext<'_>, view: &[u8], handle: i64) -> i64 {
            let handle = value_int(handle).unwrap();
            let again = match context.borrow_bytes_ref(handle) {
                Ok(bytes) => bytes,
                Err(code) => return -i64::from(code),
            };
            let third = match context.borrow_bytes_ref(handle) {
                Ok(bytes) => bytes,
                Err(code) => return -i64::from(code) * 10,
            };
            i64::from(view[0]) + i64::from(again[0]) * 100 + i64::from(third[0]) * 10000
        }

        #[aelys_export]
        pub fn shared_then_mut(context: &mut CallContext<'_>, view: &[u8], handle: i64) -> i64 {
            let handle = value_int(handle).unwrap();
            match context.borrow_bytes(handle) {
                Ok(bytes) => {
                    bytes[0] = 99;
                    1000 + i64::from(view[0])
                }
                Err(code) => i64::from(code),
            }
        }

        #[aelys_export]
        #[allow(clippy::too_many_arguments)]
        pub fn nine(
            a: &[u8],
            b: &[u8],
            c: &[u8],
            d: &[u8],
            e: &[u8],
            f: &[u8],
            g: &[u8],
            h: &[u8],
            i: &[u8],
        ) -> i64 {
            [a, b, c, d, e, f, g, h, i]
                .iter()
                .enumerate()
                .map(|(at, bytes)| (at as i64 + 1) * bytes.len() as i64)
                .sum()
        }

        #[aelys_export]
        #[allow(clippy::too_many_arguments)]
        pub fn ten_mut(
            a: &mut [u8],
            b: &mut [u8],
            c: &mut [u8],
            d: &mut [u8],
            e: &mut [u8],
            f: &mut [u8],
            g: &mut [u8],
            h: &mut [u8],
            i: &mut [u8],
            j: &mut [u8],
        ) -> i64 {
            for (at, bytes) in [a, b, c, d, e, f, g, h, i, j].into_iter().enumerate() {
                bytes[0] = at as u8 + 1;
            }
            10
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_6669786573;
        unsafe { &*pointer }
    }
}

fn run_with(source: &str, config: IsolateConfig) -> Result<String, AelysError> {
    let runtime = Runtime::new();
    unsafe { runtime.register_native_module(fixes::descriptor()) }.unwrap();
    let module = runtime.compile(source, CompileOptions::default())?;
    let mut isolate = runtime.new_isolate(config);
    match isolate.execute(&module, RunOptions::default())? {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => panic!("expected a value, got {other:?}"),
    }
}

fn run_ok(source: &str) -> String {
    run_with(source, IsolateConfig::default())
        .unwrap_or_else(|error| panic!("{error}\n--- source\n{source}"))
}

fn run_runtime_error(source: &str) -> (RuntimeErrorKind, String) {
    match run_with(source, IsolateConfig::default()) {
        Err(AelysError::Runtime(error)) => {
            let text = error.to_string();
            (error.kind, text)
        }
        Err(other) => panic!("expected a runtime error, got {other}"),
        Ok(value) => panic!("expected a runtime error, got {value}"),
    }
}

#[test]
fn a_string_allocated_during_a_native_call_survives_until_it_returns() {
    let live = "let mut live = vec![\"start\"]\nfor i in 0..2000 { live.push(\"live-{i}\") }\n";
    for (junk, width) in [(20000, 64), (3000, 200), (1, 1)] {
        assert_eq!(
            run_ok(&format!("{live}fixes::keep_first({junk}, {width})")),
            "FIRST-KEPT",
            "typed export, {junk} strings of {width}"
        );
        assert_eq!(
            run_ok(&format!("{live}fixes::return_first({junk}, {width})")),
            "RAW-FIRST",
            "raw export, {junk} strings of {width}"
        );
    }
    let loop_source = format!(
        "{live}let mut bad = 0\nfor i in 0..40 {{\n    if fixes::keep_first(500 + i * 97, 48) != \"FIRST-KEPT\" {{ bad += 1 }}\n    if fixes::return_first(500 + i * 89, 48) != \"RAW-FIRST\" {{ bad += 1 }}\n}}\nbad * 100000 + live.len()"
    );
    assert_eq!(run_ok(&loop_source), "2001");
    let small = IsolateConfig::default()
        .with_max_heap_bytes(4 * 1024 * 1024)
        .unwrap();
    let source = "let mut n = 0\nfor i in 0..30 { if fixes::keep_first(4000, 64) == \"FIRST-KEPT\" { n += 1 } }\nfor i in 0..30 { if fixes::return_first(4000, 64) == \"RAW-FIRST\" { n += 1 } }\nn";
    assert_eq!(
        run_with(source, small).unwrap_or_else(|error| panic!("{error}")),
        "60"
    );
}

#[test]
fn a_panic_in_a_typed_export_is_a_runtime_error_naming_the_native() {
    let cases = [
        ("let a = bytes::alloc(4)\nfixes::boom(a)", "fixes::boom"),
        (
            "match fixes::boom_text(\"x\") { Ok(s) => s, Err(e) => e }",
            "fixes::boom_text",
        ),
        (
            "fn go(t: string) -> int {\n    return match fixes::boom_text(t) { Ok(s) => 1, Err(e) => 2 }\n}\ngo(\"y\")",
            "fixes::boom_text",
        ),
        ("fixes::raw_boom(3)", "fixes::raw_boom"),
        (
            "let f = fixes::raw_boom\nlet mut s = 0\nfor i in 0..3 { s += f(i - 1) }\ns",
            "fixes::raw_boom",
        ),
    ];
    for (source, name) in cases {
        let (kind, text) = run_runtime_error(source);
        assert!(
            matches!(kind, RuntimeErrorKind::NativePanic { .. }),
            "{source}: {kind:?}"
        );
        assert!(
            text.contains(&format!("native function '{name}' panicked")),
            "{text}"
        );
    }
    assert_eq!(
        run_ok("match fixes::boom_text(\"\") { Ok(s) => s, Err(e) => \"err:{e}\" }"),
        "err:empty"
    );
    assert_eq!(run_ok("fixes::raw_boom(0) - fixes::raw_boom(-4)"), "-4");
    assert_eq!(
        run_ok("let a = bytes::alloc(4)\nbytes::write_u8(a, 1, 7)\nfixes::pair(a, a)"),
        "47"
    );
}

#[test]
fn the_same_buffer_passed_read_only_twice_is_allowed() {
    let setup = "let a = bytes::alloc(4)\nbytes::write_u8(a, 0, 3)\nbytes::write_u8(a, 1, 5)\n";
    assert_eq!(run_ok(&format!("{setup}fixes::pair(a, a)")), "3045");
    assert_eq!(
        run_ok(&format!(
            "{setup}fn twice(h: int) -> int {{ return fixes::pair(h, h) }}\ntwice(a) + twice(a)"
        )),
        "6090"
    );
    assert_eq!(
        run_ok(&format!(
            "{setup}let f = fn(h: int) -> int {{ return fixes::pair(h, h) }}\nlet mut s = 0\nfor i in 0..3 {{ s += f(a) }}\ns"
        )),
        "9135"
    );
    assert_eq!(
        run_ok(&format!("{setup}fixes::shared_by_context(a, a)")),
        "30303"
    );
    assert_eq!(
        run_ok(&format!(
            "{setup}let b = bytes::alloc(2)\nbytes::write_u8(b, 0, 1)\nfixes::shared_then_mut(a, b) * 1000 + bytes::read_u8(b, 0)"
        )),
        "1003099"
    );
}

#[test]
fn a_mutable_borrow_still_conflicts_with_any_other_view_of_its_buffer() {
    let setup = "let a = bytes::alloc(4)\nbytes::write_u8(a, 0, 3)\n";
    for call in [
        "fixes::copy_into(a, a)",
        "fixes::ten_mut(a, bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), bytes::alloc(1), a)",
    ] {
        let (kind, text) = run_runtime_error(&format!("{setup}{call}"));
        assert!(
            matches!(kind, RuntimeErrorKind::NativeError { code: 5 }),
            "{call}: {kind:?}"
        );
        assert!(text.contains("borrowed mutably"), "{text}");
    }
    assert_eq!(run_ok(&format!("{setup}fixes::shared_then_mut(a, a)")), "5");
    assert_eq!(run_ok(&format!("{setup}bytes::read_u8(a, 0)")), "3");
}

#[test]
fn more_than_eight_distinct_buffers_borrow_in_one_call() {
    let lets: String = (1..=10)
        .map(|n| format!("let b{n} = bytes::alloc({n})\n"))
        .collect();
    let expected_nine: i64 = (1..=9).map(|n| n * n).sum();
    assert_eq!(
        run_ok(&format!(
            "{lets}fixes::nine(b1, b2, b3, b4, b5, b6, b7, b8, b9)"
        )),
        expected_nine.to_string()
    );
    assert_eq!(
        run_ok(&format!(
            "{lets}fixes::nine(b1, b1, b1, b1, b1, b1, b1, b1, b9)"
        )),
        (36 + 81).to_string()
    );
    let source = format!(
        "{lets}let r = fixes::ten_mut(b1, b2, b3, b4, b5, b6, b7, b8, b9, b10)\nlet mut s = 0\n{}\ns * 100 + r",
        (1..=10)
            .map(|n| format!("s = s * 11 + bytes::read_u8(b{n}, 0)"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let digits: i64 = (1..=10).fold(0, |acc, n| acc * 11 + n);
    assert_eq!(run_ok(&source), (digits * 100 + 10).to_string());
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn run_file_int(dir: &Path, body: &str) -> i64 {
    let path = script(dir, "main.aelys", body);
    let value =
        aelys_driver::run_file(&path).unwrap_or_else(|error| panic!("{error}\n--- source\n{body}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("expected an int, got {value:?}"))
}

fn run_file_error(dir: &Path, body: &str) -> String {
    let path = script(dir, "main.aelys", body);
    match aelys_driver::run_file(&path) {
        Ok(value) => panic!("expected a rejection, got {value:?}\n--- source\n{body}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_third_and_fourth_whole_user_module_keep_their_qualified_names() {
    let dir = tempfile::tempdir().unwrap();
    for (name, value) in [("ma", 1), ("mb", 2), ("mc", 3), ("md", 4)] {
        script(
            dir.path(),
            &format!("{name}.aelys"),
            &format!("pub fn val() -> int {{\n    return {value}\n}}\n"),
        );
    }
    let three = "ma::val() * 100 + mb::val() * 10 + mc::val()";
    for header in [
        "needs ma\nneeds mb\nneeds mc\n",
        "needs mc\nneeds ma\nneeds mb\n",
        "needs mb\nneeds mc\nneeds ma\n",
    ] {
        assert_eq!(
            run_file_int(dir.path(), &format!("{header}{three}\n")),
            123,
            "{header}"
        );
        assert_eq!(
            run_file_int(
                dir.path(),
                &format!("{header}fn go() -> int {{\n    return {three}\n}}\ngo()\n")
            ),
            123,
            "{header}"
        );
        assert_eq!(
            run_file_int(
                dir.path(),
                &format!("{header}let go = fn() -> int {{ return {three} }}\ngo()\n")
            ),
            123,
            "{header}"
        );
    }
    assert_eq!(
        run_file_int(
            dir.path(),
            "needs ma\nneeds mb\nneeds mc\nneeds md\nma::val() * 1000 + mb::val() * 100 + mc::val() * 10 + md::val()\n"
        ),
        1234
    );
    let message = run_file_error(dir.path(), "needs ma\nneeds mb\nneeds mc\nval()\n");
    assert!(message.contains("E0445"), "{message}");
    assert!(
        message.contains("'ma::val' or 'mb::val' or 'mc::val'"),
        "{message}"
    );
}

fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "deps"))
        .and_then(Path::parent)
        .expect("the test binary lives in a cargo deps directory")
        .to_path_buf()
}

fn build_sizer_module() -> PathBuf {
    let root = profile_dir().join("t4_native_crates");
    let crate_dir = root.join("sizer");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    let native = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("native");
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"sizer\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\naelys-native = {{ path = \"{}\" }}\n\n[workspace]\n",
            native.display()
        ),
    )
    .unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        "use aelys_native::aelys_module;\n\n#[aelys_module(name = \"sizer\", version = \"0.1.0\")]\nmod exports {\n    #[aelys_export]\n    pub fn size(buffer: &[u8]) -> i64 {\n        1000 + buffer.len() as i64\n    }\n\n    #[aelys_export]\n    pub fn copy(text: String) -> String {\n        format!(\"sizer:{text}\")\n    }\n}\n",
    )
    .unwrap();
    let lock = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("Cargo.lock");
    std::fs::copy(lock, crate_dir.join("Cargo.lock")).unwrap();
    let status = Command::new(env!("CARGO"))
        .args(["build", "--offline", "--quiet", "--manifest-path"])
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .status()
        .expect("cargo build runs");
    assert!(status.success(), "the sizer module must build");
    root.join("target/debug/libsizer.so")
}

#[cfg(target_os = "linux")]
#[test]
fn a_native_module_imported_after_fs_and_bytes_keeps_its_typed_qualified_names() {
    let library = build_sizer_module();
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(&library, dir.path().join("sizer.so")).unwrap();
    let file = script(dir.path(), "five.txt", "12345");
    let path = file.to_string_lossy().into_owned();
    let body = format!(
        "let a = bytes::alloc(4)\nlet f = match fs::size(\"{path}\") {{ Ok(n) => n, Err(e) => -1 }}\nlet c = sizer::copy(\"q\")\nlet r = sizer::size(a) * 1000 + bytes::size(a) * 10 + f + c.len() * 100000000\n"
    );
    let expected = 1004 * 1000 + 4 * 10 + 5 + 7 * 100_000_000;
    for header in [
        "needs std::fs\nneeds std::bytes\nneeds sizer\n",
        "needs std::bytes\nneeds std::fs\nneeds sizer\n",
        "needs sizer\nneeds std::fs\nneeds std::bytes\n",
        "needs std::fs\nneeds sizer\nneeds std::bytes\n",
    ] {
        assert_eq!(
            run_file_int(dir.path(), &format!("{header}{body}r\n")),
            expected,
            "{header}"
        );
        assert_eq!(
            run_file_int(
                dir.path(),
                &format!("{header}fn go() -> int {{\n{body}return r\n}}\ngo()\n")
            ),
            expected,
            "{header}"
        );
    }
    let message = run_file_error(
        dir.path(),
        "needs std::fs\nneeds std::bytes\nneeds sizer\nsize(bytes::alloc(1))\n",
    );
    assert!(message.contains("E0445"), "{message}");
    assert!(message.contains("'sizer::size'"), "{message}");
}

#[cfg(target_os = "linux")]
#[test]
fn read_file_bytes_stops_an_endless_file_at_the_byte_buffer_limit() {
    let dir = tempfile::tempdir().unwrap();
    let body = "needs std::fs\nneeds std::bytes\nneeds std::string\nmatch fs::read_file_bytes(\"/dev/zero\") {\n    Ok(h) => -bytes::size(h),\n    Err(e) => if string::contains(e, \"/dev/zero\") && string::contains(e, \"268435456\") { 1 } else { 2 },\n}\n";
    assert_eq!(run_file_int(dir.path(), body), 1);
}
