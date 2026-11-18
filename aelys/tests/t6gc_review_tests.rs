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

mod heap {
    use aelys_bytecode::{AelysVec, GcObject, GcRef, Heap, HeapGeneration, ObjectKind, Value};

    fn vec_of(heap: &mut Heap, values: Vec<Value>) -> GcRef {
        heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
            values,
        ))))
    }

    fn old_vec(heap: &mut Heap, len: usize) -> GcRef {
        let owner = vec_of(heap, vec![Value::null(); len]);
        heap.mark_young([owner]);
        heap.sweep_young();
        assert_eq!(heap.heap_generation(owner), Some(HeapGeneration::Old));
        owner
    }

    fn store(heap: &mut Heap, owner: GcRef, index: usize, value: Value) {
        if let Some(object) = heap.get_mut(owner)
            && let ObjectKind::Vec(vector) = &mut object.kind
        {
            vector.set(index, value);
        }
        heap.write_barrier_element(owner, index, value);
    }

    fn element(heap: &Heap, owner: GcRef, index: usize) -> Value {
        match &heap.get(owner).expect("owner alive").kind {
            ObjectKind::Vec(vector) => vector.get(index).expect("element"),
            _ => panic!("owner is not a vec"),
        }
    }

    fn string_at(heap: &Heap, value: Value) -> String {
        match &heap
            .get(GcRef::new(value.as_ptr().expect("pointer")))
            .expect("element freed")
            .kind
        {
            ObjectKind::String(string) => string.as_str().to_string(),
            _ => panic!("not a string"),
        }
    }

    #[test]
    fn one_young_object_stored_into_every_slot_of_a_large_old_vec_keeps_the_remembered_set_bounded()
    {
        let mut heap = Heap::new();
        let len = 100_000;
        let owner = old_vec(&mut heap, len);
        let young = heap.alloc_string("shared young payload");
        for index in 0..len {
            store(&mut heap, owner, index, Value::ptr(young.index()));
        }
        assert!(
            heap.remembered_count() <= Heap::REMEMBERED_LIMIT,
            "{} remembered entries for one young object",
            heap.remembered_count()
        );
        assert_eq!(heap.verify_remembered(), Ok(()));
        heap.mark_young(std::iter::empty());
        heap.sweep_young();
        for index in [0, Heap::REMEMBERED_LIMIT, len - 1] {
            assert_eq!(
                string_at(&heap, element(&heap, owner, index)),
                "shared young payload"
            );
        }
    }

    #[test]
    fn distinct_young_vecs_stored_past_the_limit_are_promoted_with_their_children() {
        let mut heap = Heap::new();
        let len = 3 * Heap::REMEMBERED_LIMIT;
        let owner = old_vec(&mut heap, len);
        for index in 0..len {
            let leaf = heap.alloc_string(&format!("leaf-{index}"));
            let holder = vec_of(&mut heap, vec![Value::ptr(leaf.index())]);
            store(&mut heap, owner, index, Value::ptr(holder.index()));
        }
        assert!(heap.remembered_count() <= len);
        assert_eq!(heap.verify_remembered(), Ok(()));
        heap.mark_young(std::iter::empty());
        heap.sweep_young();
        assert_eq!(heap.verify_remembered(), Ok(()));
        for index in 0..len {
            let holder = GcRef::new(element(&heap, owner, index).as_ptr().expect("holder"));
            assert_eq!(heap.heap_generation(holder), Some(HeapGeneration::Old));
            assert_eq!(
                string_at(&heap, element(&heap, holder, 0)),
                format!("leaf-{index}")
            );
        }
    }

    #[test]
    fn a_young_value_stored_into_a_vec_without_its_index_is_promoted_instead_of_rescanning_the_vec()
    {
        let mut heap = Heap::new();
        let owner = old_vec(&mut heap, 50_000);
        let young = heap.alloc_string("unindexed");
        if let Some(object) = heap.get_mut(owner)
            && let ObjectKind::Vec(vector) = &mut object.kind
        {
            vector.set(49_999, Value::ptr(young.index()));
        }
        heap.write_barrier_value(owner, Value::ptr(young.index()));
        assert_eq!(heap.remembered_count(), 0);
        assert_eq!(heap.heap_generation(young), Some(HeapGeneration::Old));
        heap.mark_young(std::iter::empty());
        heap.sweep_young();
        assert_eq!(string_at(&heap, element(&heap, owner, 49_999)), "unindexed");
    }

    #[test]
    fn gc_verify_reports_a_young_element_moved_inside_an_old_vec_without_a_barrier() {
        let mut heap = Heap::new();
        let owner = old_vec(&mut heap, 8);
        let young = heap.alloc_string("young payload");
        store(&mut heap, owner, 2, Value::ptr(young.index()));
        assert_eq!(heap.verify_remembered(), Ok(()));
        if let Some(object) = heap.get_mut(owner)
            && let ObjectKind::Vec(vector) = &mut object.kind
        {
            let (a, b) = (vector.get(2).unwrap(), vector.get(5).unwrap());
            vector.set(2, b);
            vector.set(5, a);
        }
        let report = heap.verify_remembered().expect_err("missing barrier");
        assert!(report.contains("slot 5"), "{report}");
        heap.write_barrier_element(owner, 5, Value::ptr(young.index()));
        assert_eq!(heap.verify_remembered(), Ok(()));
        heap.mark_young(std::iter::empty());
        heap.sweep_young();
        assert_eq!(string_at(&heap, element(&heap, owner, 5)), "young payload");
    }

    #[test]
    #[should_panic(expected = "gc-verify")]
    fn gc_verify_mode_stops_the_minor_on_an_unremembered_edge() {
        let mut heap = Heap::new();
        heap.set_verify(true);
        let owner = old_vec(&mut heap, 4);
        let young = heap.alloc_string("hidden");
        if let Some(object) = heap.get_mut(owner)
            && let ObjectKind::Vec(vector) = &mut object.kind
        {
            vector.set(1, Value::ptr(young.index()));
        }
        heap.mark_young(std::iter::empty());
    }

    #[test]
    fn the_nursery_shrinks_after_an_expensive_minor_and_grows_back_when_minors_are_cheap() {
        let mut heap = Heap::new();
        heap.set_nursery_bytes(256 * 1024);
        assert_eq!(heap.nursery_limit(), 256 * 1024);
        assert_eq!(heap.nursery_bytes(), 64 * 1024);
        heap.note_minor_cost(2 * Heap::MINOR_TARGET_NANOS);
        assert_eq!(heap.nursery_bytes(), 16 * 1024);
        heap.note_minor_cost(4 * Heap::MINOR_TARGET_NANOS);
        assert_eq!(heap.nursery_bytes(), Heap::ADAPTIVE_NURSERY_FLOOR);
        heap.note_minor_cost(Heap::MINOR_TARGET_NANOS / 2);
        assert_eq!(heap.nursery_bytes(), Heap::ADAPTIVE_NURSERY_FLOOR);
        let mut previous = heap.nursery_bytes();
        let mut cheap_minors = 0;
        while heap.nursery_bytes() < heap.nursery_limit() {
            heap.note_minor_cost(Heap::MINOR_TARGET_NANOS / 10);
            let current = heap.nursery_bytes();
            assert!(current >= previous, "a cheap minor shrank the nursery");
            assert!(
                current <= previous + previous / 15 + 1,
                "the nursery grew from {previous} to {current} in one step"
            );
            previous = current;
            cheap_minors += 1;
            assert!(cheap_minors < 200, "the nursery never grew back");
        }
        assert!(cheap_minors > 20, "grew back in {cheap_minors} minors");
        heap.note_minor_cost(Heap::MINOR_TARGET_NANOS / 10);
        assert_eq!(heap.nursery_bytes(), 256 * 1024);
        heap.note_minor_cost(3 * Heap::MINOR_TARGET_NANOS);
        assert!(heap.nursery_bytes().abs_diff(256 * 1024 / 6) <= 1);
    }

    #[test]
    fn a_small_configured_nursery_is_never_grown_or_shrunk_past_its_bounds() {
        let mut heap = Heap::new();
        heap.set_nursery_bytes(4 * 1024);
        assert_eq!(heap.nursery_bytes(), 4 * 1024);
        heap.note_minor_cost(10 * Heap::MINOR_TARGET_NANOS);
        assert_eq!(heap.nursery_bytes(), 4 * 1024);
        heap.note_minor_cost(1);
        assert_eq!(heap.nursery_bytes(), 4 * 1024);
        let mut stressed = Heap::new();
        stressed.set_stress(true);
        stressed.note_minor_cost(1);
        assert_eq!(stressed.nursery_bytes(), Heap::MIN_NURSERY_BYTES);
    }
}

const CHURN_NEAR_MAX_HEAP: &str = r#"
struct P { x: int, y: int }
let live = 40000
let mut v: Vec<P> = vec![]
for k in 0..live { v.push(P { x: k, y: 1 }) }
let mut i = 0
while i < 200000 {
    v[i % live] = P { x: i, y: 3 }
    i = i + 1
}
let mut s = 0
for k in 0..live { s = s + v[k].y + v[k].x }
s
"#;

#[test]
fn a_heap_at_half_of_max_heap_is_collected_incrementally_without_blocking_fallbacks() {
    let live = 40000i64;
    let iterations = 200000i64;
    let expected: i64 = (0..live).map(|k| 3 + iterations - live + k).sum();
    let config = VmConfig {
        max_heap_bytes: 4 * 1024 * 1024,
        ..VmConfig::default()
    };
    let (value, vm) = run_int_with(config, CHURN_NEAR_MAX_HEAP);
    assert_eq!(value, expected);
    assert!(vm.heap().major_collection_count() >= 3);
    assert_eq!(vm.execution_stats().forced_collections, 0);
    assert!(vm.heap().peak_bytes() <= 4 * 1024 * 1024);
}

const ONE_YOUNG_IN_EVERY_SLOT: &str = r#"
struct P { x: int }
let n = 30000
let mut v: Vec<P> = vec![]
let seed = P { x: 0 }
for k in 0..n { v.push(seed) }
let mut round = 0
let mut total = 0
while round < 4 {
    let p = P { x: round + 1 }
    let mut i = 0
    while i < n {
        v[i] = p
        i = i + 1
    }
    let mut g = 0
    while g < 3000 {
        let junk = P { x: g }
        total = total + junk.x % 2
        g = g + 1
    }
    total = total + v[n - 1].x + v[0].x + v[n / 2].x
    round = round + 1
}
total
"#;

#[test]
fn one_young_struct_stored_into_every_slot_of_an_old_vec_survives_with_a_bounded_remembered_set() {
    let expected = 4 * 1500 + 3 * (1 + 2 + 3 + 4);
    for config in [
        stress(),
        VmConfig {
            gc_verify: true,
            gc_nursery_bytes: 4 * 1024,
            ..VmConfig::default()
        },
        VmConfig::default(),
    ] {
        let (value, vm) = run_int_with(config, ONE_YOUNG_IN_EVERY_SLOT);
        assert_eq!(value, expected);
        assert!(vm.heap().remembered_count() <= aelys_bytecode::Heap::REMEMBERED_LIMIT);
        assert!(vm.heap().minor_collection_count() > 0);
    }
}
