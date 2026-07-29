use aelys::{CompileOptions, CompiledModule, IsolateConfig, RunOptions, Runtime};
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
