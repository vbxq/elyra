// runs one program through the v2 API under a chosen JIT mode and prints the counters the API exposes, which the CLI path does not reach.
use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use std::path::Path;
use std::time::Instant;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() < 2 {
        eprintln!("usage: jitprobe FILE [off|baseline|tiered] [repeats]");
        std::process::exit(2);
    }
    let path = arguments[1].clone();
    let mode = match arguments.get(2).map(String::as_str) {
        Some("off") => JitMode::Off,
        Some("baseline") => JitMode::Baseline,
        _ => JitMode::Tiered,
    };
    let repeats: usize = arguments
        .get(3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);

    let runtime = Runtime::with_jit_mode(mode);
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let module = match isolate.compile_file(Path::new(&path), CompileOptions::default()) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("compile error: {error}");
            std::process::exit(1);
        }
    };

    for round in 0..repeats {
        let start = Instant::now();
        let outcome = isolate.execute(&module, RunOptions::default());
        let elapsed = start.elapsed();
        match outcome {
            Ok(ExecutionOutcome::Returned(_)) | Ok(ExecutionOutcome::Exited(_)) => {}
            Err(error) => {
                eprintln!("runtime error: {error}");
                std::process::exit(1);
            }
        }
        println!(
            "round {round} mode {:?} wall_ms {} cache {} deopt {} osr {}",
            mode,
            elapsed.as_millis(),
            runtime.jit_cache_entries(),
            runtime.jit_deoptimizations(),
            runtime.jit_osr_executions(),
        );
    }
}
