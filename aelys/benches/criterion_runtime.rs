use aelys::{CompileOptions, CompiledModule, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{Function, OpCode};
use criterion::{Criterion, criterion_group, criterion_main};

const WORKLOADS: [(&str, &str); 8] = [
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
    (
        "closure_calls",
        "fn make_adder(x) { return fn(y) { return x + y } } let add_one = make_adder(1); let mut n = 0; for i in 0..1000 { n = add_one(n) } n",
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

    let globals = globals_dispatch_function();
    let bitwise = bitwise_dispatch_function();
    let mut group = criterion.benchmark_group("dispatch_handlers");
    group.bench_function("globals", |bencher| {
        bencher.iter(|| execute_function(&globals));
    });
    group.bench_function("bitwise", |bencher| {
        bencher.iter(|| execute_function(&bitwise));
    });
    group.finish();

    tier1_integer_loop_benchmarks(criterion);
    jit_collection_loop_benchmarks(criterion);
    jit_profiled_leaf_inlining_benchmarks(criterion);
    tiered_cold_benchmarks(criterion);
}

fn jit_profiled_leaf_inlining_benchmarks(criterion: &mut Criterion) {
    let source = r#"
fn outer(value: int) -> int {
    fn advance(inner: int) -> int {
        let mut result = inner
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        result = result + 1
        return result
    }
    return advance(value) + 1
}
outer(20)
"#;
    let options = || CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let interpreter = Runtime::with_jit_mode(JitMode::Off);
    let interpreter_module = interpreter.compile(source, options()).unwrap();
    let mut interpreter_isolate = interpreter.new_isolate(IsolateConfig::default());

    let tiered = Runtime::with_jit_mode(JitMode::Tiered);
    let tiered_module = tiered.compile(source, options()).unwrap();
    let mut tiered_isolate = tiered.new_isolate(IsolateConfig::default());
    for _ in 0..10_000 {
        tiered_isolate
            .execute(&tiered_module, RunOptions::default())
            .expect("profiled leaf inlining warmup must succeed");
    }

    let mut group = criterion.benchmark_group("jit_profiled_leaf_inlining");
    group.bench_function("interpreter", |bencher| {
        bencher.iter(|| {
            interpreter_isolate
                .execute(&interpreter_module, RunOptions::default())
                .expect("profiled leaf interpreter benchmark must succeed")
        });
    });
    group.bench_function("optimized_jit", |bencher| {
        bencher.iter(|| {
            tiered_isolate
                .execute(&tiered_module, RunOptions::default())
                .expect("profiled leaf JIT benchmark must succeed")
        });
    });
    group.finish();
}

fn jit_collection_loop_benchmarks(criterion: &mut Criterion) {
    let elements = (0..4096)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
fn sum(values: Array<Int>) -> int {{
    let mut index = 0
    let mut total = 0
    while index < values.len() {{
        total = total + values[index]
        index = index + 1
    }}
    return total
}}
let values = Array[{elements}]
let mut result = 0
for repeat in 0..10 {{
    result = sum(values)
}}
result
"#
    );
    let options = || CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let interpreter = Runtime::with_jit_mode(JitMode::Off);
    let interpreter_module = interpreter
        .compile(&source, options())
        .expect("array interpreter benchmark must compile");
    let mut interpreter_isolate = interpreter.new_isolate(IsolateConfig::default());

    let baseline = Runtime::with_jit_mode(JitMode::Baseline);
    let baseline_module = baseline
        .compile(&source, options())
        .expect("array baseline JIT benchmark must compile");
    let mut baseline_isolate = baseline.new_isolate(IsolateConfig::default());
    baseline_isolate
        .execute(&baseline_module, RunOptions::default())
        .expect("array baseline JIT warmup must succeed");

    let tiered = Runtime::with_jit_mode(JitMode::Tiered);
    let tiered_module = tiered
        .compile(&source, options())
        .expect("array tiered JIT benchmark must compile");
    let mut tiered_isolate = tiered.new_isolate(IsolateConfig::default());
    for _ in 0..1_000 {
        tiered_isolate
            .execute(&tiered_module, RunOptions::default())
            .expect("array tiered JIT warmup must succeed");
    }

    let mut group = criterion.benchmark_group("jit_integer_array_loop");
    group.bench_function("interpreter", |bencher| {
        bencher.iter(|| {
            interpreter_isolate
                .execute(&interpreter_module, RunOptions::default())
                .expect("array interpreter benchmark must succeed")
        });
    });
    group.bench_function("baseline_jit", |bencher| {
        bencher.iter(|| {
            baseline_isolate
                .execute(&baseline_module, RunOptions::default())
                .expect("array baseline JIT benchmark must succeed")
        });
    });
    group.bench_function("optimized_jit", |bencher| {
        bencher.iter(|| {
            tiered_isolate
                .execute(&tiered_module, RunOptions::default())
                .expect("array tiered JIT benchmark must succeed")
        });
    });
    group.finish();
}

fn tier1_integer_loop_benchmarks(criterion: &mut Criterion) {
    let source = r#"
fn sum_to(limit: int) -> int {
    let mut index = 0
    let mut sum = 0
    while index < limit {
        sum = sum + index
        index = index + 1
    }
    return sum
}
sum_to(100000)
"#;
    let interpreter = Runtime::with_jit_mode(JitMode::Off);
    let interpreter_module = interpreter
        .compile(source, CompileOptions::default())
        .expect("interpreter JIT comparison workload must compile");
    let mut interpreter_isolate = interpreter.new_isolate(IsolateConfig::default());

    let baseline = Runtime::with_jit_mode(JitMode::Baseline);
    let baseline_module = baseline
        .compile(source, CompileOptions::default())
        .expect("baseline JIT comparison workload must compile");
    let mut baseline_isolate = baseline.new_isolate(IsolateConfig::default());
    baseline_isolate
        .execute(&baseline_module, RunOptions::default())
        .expect("baseline JIT comparison warmup must succeed");

    let tiered = Runtime::with_jit_mode(JitMode::Tiered);
    let tiered_module = tiered
        .compile(source, CompileOptions::default())
        .expect("optimized JIT comparison workload must compile");
    let mut tiered_isolate = tiered.new_isolate(IsolateConfig::default());
    for _ in 0..10_000 {
        tiered_isolate
            .execute(&tiered_module, RunOptions::default())
            .expect("optimized JIT comparison warmup must succeed");
    }

    let mut group = criterion.benchmark_group("tier1_integer_loop");
    group.bench_function("interpreter", |bencher| {
        bencher.iter(|| {
            interpreter_isolate
                .execute(&interpreter_module, RunOptions::default())
                .expect("interpreter JIT comparison must succeed")
        });
    });
    group.bench_function("baseline_jit", |bencher| {
        bencher.iter(|| {
            baseline_isolate
                .execute(&baseline_module, RunOptions::default())
                .expect("baseline JIT comparison must succeed")
        });
    });
    group.bench_function("optimized_jit", |bencher| {
        bencher.iter(|| {
            tiered_isolate
                .execute(&tiered_module, RunOptions::default())
                .expect("optimized JIT comparison must succeed")
        });
    });
    group.finish();

    let osr_runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let osr_module = osr_runtime
        .compile(source, CompileOptions::default())
        .expect("OSR comparison workload must compile");
    let mut osr_warmup = osr_runtime.new_isolate(IsolateConfig::default());
    osr_warmup
        .execute(&osr_module, RunOptions::default())
        .expect("OSR comparison warmup must succeed");
    assert_eq!(osr_runtime.jit_osr_executions(), 1);

    let mut group = criterion.benchmark_group("osr_first_hot_loop");
    group.bench_function("interpreter", |bencher| {
        bencher.iter(|| {
            let mut isolate = interpreter.new_isolate(IsolateConfig::default());
            isolate
                .execute(&interpreter_module, RunOptions::default())
                .expect("OSR interpreter comparison must succeed")
        });
    });
    group.bench_function("tiered_osr", |bencher| {
        bencher.iter(|| {
            let mut isolate = osr_runtime.new_isolate(IsolateConfig::default());
            isolate
                .execute(&osr_module, RunOptions::default())
                .expect("OSR tiered comparison must succeed")
        });
    });
    group.finish();
}

fn tiered_cold_benchmarks(criterion: &mut Criterion) {
    let source = "fn increment(value: int) -> int { return value + 1 } increment(41)";
    let options = || CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let interpreter = Runtime::with_jit_mode(JitMode::Off);
    let interpreter_module = interpreter
        .compile(source, options())
        .expect("cold interpreter workload must compile");
    let tiered = Runtime::with_jit_mode(JitMode::Tiered);
    let tiered_module = tiered
        .compile(source, options())
        .expect("cold tiered workload must compile");
    let backedge_source = r#"
fn count(limit: int) -> int {
    let mut value = 0
    while value < limit { value = value + 1 }
    return value
}
count(1000)
"#;
    let interpreter_backedges = Runtime::with_jit_mode(JitMode::Off);
    let interpreter_backedge_module = interpreter_backedges
        .compile(backedge_source, CompileOptions::default())
        .expect("cold interpreter backedge workload must compile");
    let tiered_backedges = Runtime::with_jit_mode(JitMode::Tiered);
    let tiered_backedge_module = tiered_backedges
        .compile(backedge_source, CompileOptions::default())
        .expect("cold tiered backedge workload must compile");

    let mut group = criterion.benchmark_group("tiered_cold");
    group.bench_function("interpreter", |bencher| {
        bencher.iter(|| execute_once(&interpreter, &interpreter_module));
    });
    group.bench_function("tiered_before_threshold", |bencher| {
        bencher.iter(|| execute_once(&tiered, &tiered_module));
    });
    group.bench_function("interpreter_backedges", |bencher| {
        bencher.iter(|| execute_once(&interpreter_backedges, &interpreter_backedge_module));
    });
    group.bench_function("tiered_backedges_before_threshold", |bencher| {
        bencher.iter(|| execute_once(&tiered_backedges, &tiered_backedge_module));
    });
    group.finish();
}

fn execute_once(runtime: &Runtime, module: &CompiledModule) {
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate
        .execute(module, RunOptions::default())
        .expect("benchmark execution must succeed");
}

fn execute_function(function: &Function) {
    let mut vm = aelys::new_vm().expect("benchmark VM creation must succeed");
    let function = vm
        .alloc_function(function.clone())
        .expect("benchmark function allocation must succeed");
    vm.execute(function)
        .expect("benchmark function execution must succeed");
}

fn globals_dispatch_function() -> Function {
    let mut function = Function::new(Some("globals_dispatch".to_string()), 0);
    function.num_registers = 2;
    let name =
        function.add_structural_constant(aelys_bytecode::Constant::String("value".to_string()));
    let name = u8::try_from(name).expect("benchmark constant index fits u8");
    for value in 0..4_096 {
        function.emit_b(OpCode::LoadI, 0, value, 1);
        function.emit_a(OpCode::SetGlobal, 0, name, 0, 1);
        function.emit_a(OpCode::GetGlobal, 1, name, 0, 1);
    }
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn bitwise_dispatch_function() -> Function {
    let mut function = Function::new(Some("bitwise_dispatch".to_string()), 0);
    function.num_registers = 2;
    function.emit_b(OpCode::LoadI, 0, 1, 1);
    function.emit_b(OpCode::LoadI, 1, 3, 1);
    for _ in 0..12_288 {
        function.emit_a(OpCode::BitXor, 0, 0, 1, 1);
    }
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

criterion_group!(runtime, runtime_benchmarks);
criterion_main!(runtime);
