use aelys::{
    CompileOptions, IsolateConfig, RunOptions, Runtime, StructuredValue, new_vm_with_config,
    run_with_vm_and_opt,
};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{VM, VmConfig};

fn run_int_with(config: VmConfig, source: &str) -> (i64, VM) {
    let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
    let value = run_with_vm_and_opt(&mut vm, source, "<t6gc>", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("program failed: {error}"));
    (value.as_int().expect("program must return an int"), vm)
}

fn stress() -> VmConfig {
    VmConfig {
        gc_stress: true,
        gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
        ..VmConfig::default()
    }
}

fn import_round_trip(value: StructuredValue) {
    let runtime = Runtime::new();
    let module = runtime.compile("0", CompileOptions::default()).unwrap();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate.execute(&module, RunOptions::default()).unwrap();
    let imported = isolate.import_clone(&value).expect("import");
    let back = isolate
        .structured_clone(imported)
        .unwrap_or_else(|error| panic!("imported value lost elements: {error:?}"));
    assert!(back == value, "imported value differs from the original");
}

#[test]
fn import_clone_of_a_large_vec_of_strings_keeps_every_element() {
    let items = (0..20000)
        .map(|i| StructuredValue::String(format!("element-{i}-{}", "x".repeat(40))))
        .collect();
    import_round_trip(StructuredValue::Vec(items));
}

#[test]
fn import_clone_of_nested_vecs_keeps_every_element() {
    let items = (0..3000)
        .map(|i| {
            StructuredValue::Vec(vec![
                StructuredValue::Int(i),
                StructuredValue::String(format!("s{i}")),
            ])
        })
        .collect();
    import_round_trip(StructuredValue::Vec(items));
}

#[test]
fn import_clone_of_nested_arrays_inside_a_vec_keeps_every_element() {
    let items = (0..3000)
        .map(|i| {
            StructuredValue::Array(vec![
                StructuredValue::String(format!("a{i}-{}", "y".repeat(30))),
                StructuredValue::Vec(vec![StructuredValue::String(format!("b{i}"))]),
            ])
        })
        .collect();
    import_round_trip(StructuredValue::Vec(items));
}

const REINTERN_GARBAGE_STRING: &str = r#"
struct P { x: int, y: int }
fn check(s: string, k: int) -> int {
    let z = k * 0
    let want = "gamma-{z}"
    if s == want { return 1 }
    return 1000
}
fn use_gamma(k: int) -> int {
    let f = fn() -> string { return "gamma-0" }
    return check(f(), k)
}
fn noise(n: int) -> int {
    let mut a = P { x: 0, y: 0 }
    let mut b = P { x: 1, y: 1 }
    let mut c = P { x: 2, y: 2 }
    let mut i = 0
    while i < n {
        a = P { x: i, y: b.y }
        b = P { x: c.x, y: i }
        c = P { x: a.x, y: 1 }
        i = i + 1
    }
    return a.x + b.x + c.y - a.x - b.x
}
let mut total = 0
let mut r = 0
while r < 20000 {
    total = total + use_gamma(r)
    total = total + noise(r % 7)
    r = r + 1
}
total
"#;

#[test]
fn an_interned_string_handed_out_again_during_a_major_is_not_swept() {
    let rounds = 20000i64;
    let zero_noise_rounds = (0..rounds).filter(|r| r % 7 == 0).count() as i64;
    let expected = rounds + rounds + zero_noise_rounds;
    let (value, vm) = run_int_with(stress(), REINTERN_GARBAGE_STRING);
    assert_eq!(value, expected);
    assert!(vm.heap().major_collection_count() > 10);
}
