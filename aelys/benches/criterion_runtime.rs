use aelys::{CompileOptions, CompiledModule, IsolateConfig, RunOptions, Runtime};
use criterion::{Criterion, criterion_group, criterion_main};

const WORKLOADS: [(&str, &str); 7] = [
    (
        "calls",
        "fn inc(x) { return x + 1 } let mut n = 0; for i in 0..1000 { n = inc(n) } n",
    ),
    ("loops", "let mut n = 0; for i in 0..10000 { n += i } n"),
    (
        "dispatch",
        "let mut n = 1; for i in 0..5000 { if i % 2 == 0 { n = n + i } else { n = n - 1 } } n",
    ),
    (
        "arrays",
        "let mut n = 0; for i in 0..1000 { let a = [i, i + 1, i + 2, i + 3]; n += a[2] } n",
    ),
    (
        "allocations",
        "let mut s = \"\"; for i in 0..256 { s = s + \"x\" } s.len()",
    ),
    (
        "gc",
        "let mut n = 0; for i in 0..2000 { let v = Vec[i, i + 1, i + 2]; n += v[1] } n",
    ),
    (
        "raizen_scheduler",
        "fn task_step(id, tick) { return (id * 17 + tick * 31) % 997 } let mut sum = 0; for tick in 0..200 { for id in 0..64 { sum += task_step(id, tick) } } sum",
    ),
];

fn runtime_benchmarks(criterion: &mut Criterion) {
    let runtime = Runtime::new();
    let modules = WORKLOADS.map(|(name, source)| {
        let module = runtime
            .compile(source, CompileOptions::default())
            .unwrap_or_else(|error| panic!("benchmark workload {name} failed to compile: {error}"));
        (name, module)
    });

    let mut group = criterion.benchmark_group("interpreter_v2");
    for (name, module) in &modules {
        group.bench_function(*name, |bencher| {
            bencher.iter(|| execute_once(&runtime, module));
        });
    }
    group.finish();
}

fn execute_once(runtime: &Runtime, module: &CompiledModule) {
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate
        .execute(module, RunOptions::default())
        .expect("benchmark execution must succeed");
}

criterion_group!(runtime, runtime_benchmarks);
criterion_main!(runtime);
