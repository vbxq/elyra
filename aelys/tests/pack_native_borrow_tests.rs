use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_common::error::AelysError;
use aelys_opt::OptimizationLevel;

mod lend {
    use aelys_native::aelys_module;

    #[aelys_module(name = "lend", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn address(bytes: &[u8]) -> i64 {
            bytes.as_ptr() as i64
        }

        #[aelys_export]
        pub fn same(first: &[u8], second: &[u8]) -> bool {
            first.as_ptr() == second.as_ptr()
        }

        #[aelys_export]
        pub fn stamp(bytes: &mut [u8], value: i64) -> i64 {
            for byte in bytes.iter_mut() {
                *byte = value as u8;
            }
            bytes.as_ptr() as i64
        }

        #[aelys_export]
        pub fn lent_at(bytes: &[u8], address: i64) -> bool {
            bytes.as_ptr() as i64 == address
        }

        #[aelys_export]
        pub fn total(bytes: &[u8]) -> i64 {
            bytes.iter().map(|byte| i64::from(*byte)).sum()
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_6c656e64;
        unsafe { &*pointer }
    }
}

const LEVELS: [OptimizationLevel; 2] = [OptimizationLevel::None, OptimizationLevel::Standard];
const JITS: [JitMode; 2] = [JitMode::Off, JitMode::Tiered];

fn run_at(source: &str, level: OptimizationLevel, jit: JitMode) -> Result<String, AelysError> {
    let runtime = Runtime::with_jit_mode(jit);
    unsafe { runtime.register_native_module(lend::descriptor()) }.unwrap();
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

// a copied argument would sit in one reused scratch buffer, so two vectors would share an address
#[test]
fn a_native_borrows_the_vec_storage_itself_not_a_copy() {
    assert_everywhere(
        "let a: Vec<u8> = vec![1; 64]\nlet b: Vec<u8> = vec![2; 64]\n\"{lend::address(&a) == lend::address(&a)} {lend::address(&a) == lend::address(&b)} {lend::same(&a, &a)} {lend::same(&a, &b)}\"",
        "true false true false",
    );
}

#[test]
fn a_mutable_borrow_writes_in_place_into_the_packed_storage() {
    assert_everywhere(
        "let mut a: Vec<u8> = vec![0; 1000]\nlet b: Vec<u8> = vec![0; 1000]\nlet at = lend::stamp(&mut a, 300)\n\"{lend::lent_at(&a, at)} {lend::lent_at(&b, at)} {a[0]} {a[999]} {lend::total(&a)} {lend::total(&b)}\"",
        "true false 44 44 44000 0",
    );
    assert_everywhere(
        "let mut frame: Vec<u8> = vec![0; 4096]\nlet mut sum = 0\nfor round in 0..200 {\n    let _ = lend::stamp(&mut frame, round)\n    frame[round] = 255\n    sum += lend::total(&frame)\n}\nsum",
        &(0..200i64)
            .map(|round| (round % 256) * 4095 + 255)
            .sum::<i64>()
            .to_string(),
    );
}

#[test]
fn every_kind_of_vec_u8_is_lent_packed() {
    assert_everywhere(
        "let text = \"hello\".to_bytes()\nlet literal: Vec<u8> = vec![1, 2, 3]\nlet sliced = literal[1..3]\nlet copied = literal.clone()\nlet mut pushed: Vec<u8> = vec![]\npushed.push(9)\n\"{lend::total(&text)} {lend::total(&literal)} {lend::total(&sliced)} {lend::total(&copied)} {lend::total(&pushed)} {lend::same(&literal, &copied)}\"",
        "532 6 5 6 9 false",
    );
}
