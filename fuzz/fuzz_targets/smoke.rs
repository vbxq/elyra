#![no_main]

use aelys::{CompileOptions, IsolateConfig, RunOptions, Runtime};
use aelys_bytecode::asm::deserialize;
use aelys_runtime::{ExecutionControl, VM};
use aelys_syntax::Source;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let runtime = Runtime::new();
        if let Ok(module) = runtime.compile(source, CompileOptions::default()) {
            let mut isolate = runtime.new_isolate(IsolateConfig::default());
            let _ = isolate.execute(
                &module,
                RunOptions {
                    max_instructions: Some(10_000),
                    ..RunOptions::default()
                },
            );
        }
    }

    if let Ok(function) = deserialize(data)
        && let Ok(mut vm) = VM::new(Source::new("<fuzz-avbc>", ""))
    {
        if let Ok(function_ref) = vm.alloc_function(function) {
            vm.configure_execution(ExecutionControl {
                max_instructions: Some(10_000),
                ..ExecutionControl::default()
            });
            let _ = vm.execute(function_ref);
        }
    }
});
