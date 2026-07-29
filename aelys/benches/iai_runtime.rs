use aelys::{CompileOptions, IsolateConfig, JitMode, RunOptions, Runtime};
use iai_callgrind::{library_benchmark, library_benchmark_group, main};

fn execute(source: &str) {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("benchmark workload must compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate
        .execute(&module, RunOptions::default())
        .expect("benchmark execution must succeed");
}

#[library_benchmark]
fn calls() {
    execute("fn inc(x) { return x + 1 } let mut n = 0; for i in 0..1000 { n = inc(n) } n");
}

#[library_benchmark]
fn loops() {
    execute("let mut n = 0; for i in 0..10000 { n += i } n");
}

#[library_benchmark]
fn dispatch() {
    execute("let mut n = 1; for i in 0..5000 { if i % 2 == 0 { n = n + i } else { n = n - 1 } } n");
}

#[library_benchmark]
fn arrays() {
    execute("let mut n = 0; for i in 0..1000 { let a = [i, i + 1, i + 2, i + 3]; n += a[2] } n");
}

#[library_benchmark]
fn allocations() {
    execute("let mut s = \"\"; for i in 0..256 { s = s + \"x\" } s.len()");
}

#[library_benchmark]
fn gc() {
    execute("let mut n = 0; for i in 0..2000 { let v = Vec[i, i + 1, i + 2]; n += v[1] } n");
}

#[library_benchmark]
fn raizen_scheduler() {
    execute(
        "fn task_step(id, tick) { return (id * 17 + tick * 31) % 997 } let mut sum = 0; for tick in 0..200 { for id in 0..64 { sum += task_step(id, tick) } } sum",
    );
}

library_benchmark_group!(
    name = runtime;
    benchmarks = calls, loops, dispatch, arrays, allocations, gc, raizen_scheduler
);
main!(library_benchmark_groups = runtime);
