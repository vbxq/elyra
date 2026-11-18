use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_common::error::{AelysError, RuntimeErrorKind};
use aelys_opt::OptimizationLevel;

mod frames {
    use aelys_native::aelys_module;

    #[aelys_module(name = "frames", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn checksum(frame: &[u8]) -> i64 {
            frame
                .iter()
                .enumerate()
                .map(|(at, &byte)| (at as i64 + 1) * i64::from(byte))
                .sum()
        }

        #[aelys_export]
        pub fn invert(frame: &mut [u8]) -> i64 {
            for byte in frame.iter_mut() {
                *byte = 255 - *byte;
            }
            frame.len() as i64
        }

        #[aelys_export]
        pub fn blit(target: &mut [u8], source: &[u8]) -> Result<(), String> {
            if target.len() != source.len() {
                return Err(format!("{} != {}", target.len(), source.len()));
            }
            target.copy_from_slice(source);
            Ok(())
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_6672616d6573;
        unsafe { &*pointer }
    }
}

const LEVELS: [OptimizationLevel; 2] = [OptimizationLevel::None, OptimizationLevel::Standard];
const JITS: [JitMode; 2] = [JitMode::Off, JitMode::Tiered];

fn runtime(jit: JitMode) -> Runtime {
    let runtime = Runtime::with_jit_mode(jit);
    unsafe { runtime.register_native_module(frames::descriptor()) }.unwrap();
    runtime
}

fn run_at(source: &str, level: OptimizationLevel, jit: JitMode) -> Result<String, AelysError> {
    let runtime = runtime(jit);
    let module = runtime.compile(
        source,
        CompileOptions {
            optimization_level: level,
            ..CompileOptions::default()
        },
    )?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate.execute(&module, RunOptions::default())? {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => panic!("expected a value, got {other:?}"),
    }
}

fn assert_everywhere(source: &str, expected: &str) {
    for level in LEVELS {
        for jit in JITS {
            match run_at(source, level, jit) {
                Ok(got) => assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}"),
                Err(error) => panic!("{level:?} {jit:?}: {error}\n{source}"),
            }
        }
    }
}

fn rejected(source: &str) -> String {
    match runtime(JitMode::Off).compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_native_reads_a_vec_of_u8_passed_by_shared_borrow() {
    assert_everywhere(
        "let v: Vec<u8> = vec![1, 2, 255]\nframes::checksum(&v)",
        "770",
    );
    assert_everywhere("let v: Vec<u8> = vec![]\nframes::checksum(&v)", "0");
    assert_everywhere(
        "let mut v: Vec<u8> = vec![0; 300]\nlet mut total = 0\nfor i in 0..300 {\n    v[i] = (i % 256) as u8\n    total += frames::checksum(&v)\n}\ntotal",
        &(1..=300i64)
            .map(|n| (0..n).map(|i| (i + 1) * (i % 256)).sum::<i64>())
            .sum::<i64>()
            .to_string(),
    );
    assert_everywhere(
        "struct Screen { pixels: Vec<u8> }\nlet s = Screen { pixels: vec![3, 4] }\nfn sum(s: Screen) -> int { frames::checksum(&s.pixels) }\nsum(s)",
        "11",
    );
}

#[test]
fn a_native_writes_through_a_mutable_borrow_into_the_vec() {
    assert_everywhere(
        "let mut v: Vec<u8> = vec![0, 1, 254, 255]\nlet n = frames::invert(&mut v)\n\"{n} {v[0]} {v[1]} {v[2]} {v[3]}\"",
        "4 255 254 1 0",
    );
    assert_everywhere(
        "let src: Vec<u8> = vec![9, 8, 7]\nlet mut dst: Vec<u8> = vec![0; 3]\nlet mut rounds = 0\nfor i in 0..500 {\n    frames::blit(&mut dst, &src).unwrap()\n    dst[0] = 0\n    rounds += 1\n}\nframes::blit(&mut dst, &src).unwrap()\n\"{rounds} {dst[0]}{dst[1]}{dst[2]}\"",
        "500 987",
    );
    assert_everywhere(
        "let src: Vec<u8> = vec![1]\nlet mut dst: Vec<u8> = vec![0; 2]\nmatch frames::blit(&mut dst, &src) { Ok(_) => \"ok\", Err(e) => e }",
        "2 != 1",
    );
}

#[test]
fn the_borrow_rules_are_checked_before_the_call() {
    let missing = rejected("let v: Vec<u8> = vec![1]\nframes::checksum(v)");
    assert!(missing.contains("E0417"), "{missing}");
    let immutable = rejected("let v: Vec<u8> = vec![1]\nframes::invert(&mut v)");
    assert!(immutable.contains("error[E04"), "{immutable}");
    let shared_for_mut = rejected("let mut v: Vec<u8> = vec![1]\nframes::invert(&v)");
    assert!(shared_for_mut.contains("error[E04"), "{shared_for_mut}");
    let overlap = rejected("let mut v: Vec<u8> = vec![1]\nframes::blit(&mut v, &v)");
    assert!(
        overlap.contains("E0416") || overlap.contains("E0415"),
        "{overlap}"
    );
    let wrong = rejected("let v: Vec<int> = vec![1]\nframes::checksum(&v)");
    assert!(wrong.contains("E0301"), "{wrong}");
    let temporary = rejected("frames::checksum(&\"ab\".to_bytes())");
    assert!(temporary.contains("E0419"), "{temporary}");
}

#[test]
fn a_mutable_byte_parameter_called_through_a_function_value_still_guards_the_vec() {
    let error = rejected("let v: Vec<u8> = vec![1]\nlet f = frames::invert\nf(v)");
    assert!(error.contains("E0323"), "{error}");
    match run_at(
        "let mut v: Vec<u8> = vec![1, 2]\nlet f = frames::blit\nlet _ = f(v, v)\n0",
        OptimizationLevel::None,
        JitMode::Off,
    ) {
        Err(AelysError::Runtime(error)) => assert!(
            matches!(error.kind, RuntimeErrorKind::NativeError { code: 5 }),
            "{error}"
        ),
        other => panic!("expected the runtime borrow conflict, got {other:?}"),
    }
}

#[test]
fn fs_writes_and_reads_a_vec_of_u8_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("all.bin");
    let shown = path.to_string_lossy().replace('\\', "/");
    let source = format!(
        "needs std::fs\nlet mut v: Vec<u8> = vec![0; 256]\nfor i in 0..256 {{ v[i] = i as u8 }}\nfs::write_bytes(\"{shown}\", &v).unwrap()\nlet back = fs::read_bytes(\"{shown}\").unwrap()\nlet mut same = back.len() == 256\nfor i in 0..256 {{ if back[i] != v[i] {{ same = false }} }}\nlet f = fs::open(\"{shown}\", \"r\").unwrap()\nlet head = fs::read_chunk(f, 4).unwrap()\nlet _ = fs::close(f)\nlet missing = fs::read_bytes(\"{shown}.none\").is_err()\n\"{{same}} {{head.read_u32_be(0)}} {{missing}}\""
    );
    assert_everywhere(&source, "true 66051 true");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        (0..=255u8).collect::<Vec<_>>()
    );
}
