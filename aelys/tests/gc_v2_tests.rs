use aelys_bytecode::{
    AelysEnum, AelysVec, GcObject, Heap, HeapGeneration, MajorSliceResult, ObjectKind, SumTag,
    Value,
};
use aelys_runtime::VM;
use aelys_syntax::Source;
use std::time::Duration;

#[test]
fn young_survivors_promote_and_mutations_enter_remembered_set() {
    let mut heap = Heap::new();
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));

    for _ in 0..2 {
        heap.mark_young([owner]);
        heap.sweep_young();
    }
    assert_eq!(heap.heap_generation(owner), Some(HeapGeneration::Old));

    let child = heap.alloc_string("young child");
    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    assert!(values.push(Value::ptr(child.index())));
    assert_eq!(heap.remembered_count(), 1);

    heap.mark_young([owner]);
    heap.sweep_young();
    assert!(heap.get(child).is_some());
}

#[test]
fn minor_collection_reclaims_only_unreachable_young_objects() {
    let mut heap = Heap::new();
    let old = heap.alloc_string("old");
    for _ in 0..2 {
        heap.mark_young([old]);
        heap.sweep_young();
    }
    let young = heap.alloc_string("young");

    heap.sweep_young();

    assert!(heap.get(old).is_some());
    assert!(heap.get(young).is_none());
    assert_eq!(heap.minor_collection_count(), 3);
    assert_eq!(heap.major_collection_count(), 0);
}

#[test]
fn minor_collection_does_not_leave_old_objects_marked_for_major_gc() {
    let mut heap = Heap::new();
    let child = heap.alloc_string("old child");
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        vec![Value::ptr(child.index())],
    ))));
    for _ in 0..2 {
        heap.mark_young([owner]);
        heap.sweep_young();
    }

    assert!(heap.begin_major_collection(vec![owner]));
    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }

    assert!(heap.get(owner).is_some());
    assert!(heap.get(child).is_some());
}

#[test]
fn minor_collection_prunes_remembered_owners_without_young_edges() {
    let mut heap = Heap::new();
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));
    for _ in 0..2 {
        heap.mark_young([owner]);
        heap.sweep_young();
    }

    let child = heap.alloc_string("temporary child");
    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    assert!(values.push(Value::ptr(child.index())));
    heap.mark_young([owner]);
    heap.sweep_young();
    assert_eq!(heap.remembered_count(), 1);

    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    values.clear();
    heap.mark_young([owner]);
    heap.sweep_young();
    assert_eq!(heap.remembered_count(), 0);
}

#[test]
fn promotion_remembers_an_owner_that_still_points_to_a_younger_object() {
    let mut heap = Heap::new();
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));
    heap.mark_young([owner]);
    heap.sweep_young();

    let child = heap.alloc_string("younger child");
    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    assert!(values.push(Value::ptr(child.index())));
    assert_eq!(heap.remembered_count(), 0);

    heap.mark_young([owner]);
    heap.sweep_young();
    assert_eq!(heap.heap_generation(owner), Some(HeapGeneration::Old));
    assert_eq!(heap.heap_generation(child), Some(HeapGeneration::Young));
    assert_eq!(heap.remembered_count(), 1);

    heap.mark_young([]);
    heap.sweep_young();
    assert!(heap.get(child).is_some());
}

#[test]
fn major_promotion_rebuilds_old_to_young_edges() {
    let mut heap = Heap::new();
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));
    heap.mark_young([owner]);
    heap.sweep_young();

    let child = heap.alloc_string("major younger child");
    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    assert!(values.push(Value::ptr(child.index())));

    assert!(heap.begin_major_collection(vec![owner]));
    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }
    assert_eq!(heap.heap_generation(owner), Some(HeapGeneration::Old));
    assert_eq!(heap.heap_generation(child), Some(HeapGeneration::Young));
    assert_eq!(heap.remembered_count(), 1);

    heap.mark_young([]);
    heap.sweep_young();
    assert!(heap.get(child).is_some());
}

#[test]
fn incremental_major_collection_preserves_graph_and_reclaims_garbage() {
    let mut heap = Heap::new();
    let child = heap.alloc_string("reachable");
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        vec![Value::ptr(child.index())],
    ))));
    let garbage = heap.alloc_string("garbage");

    assert!(heap.begin_major_collection(vec![owner]));
    let mut slices = 0;
    while heap.major_collection_active() {
        let result = heap.major_collection_slice(Duration::ZERO);
        assert_ne!(result, MajorSliceResult::Idle);
        slices += 1;
    }

    assert!(slices > 1);
    assert!(heap.get(owner).is_some());
    assert!(heap.get(child).is_some());
    assert!(heap.get(garbage).is_none());
    assert_eq!(heap.major_collection_count(), 1);
}

#[test]
fn incremental_mark_retraces_mutated_objects() {
    let mut heap = Heap::new();
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));
    let late_child = heap.alloc_string("late child");

    assert!(heap.begin_major_collection(vec![owner]));
    assert_eq!(
        heap.major_collection_slice(Duration::ZERO),
        MajorSliceResult::InProgress
    );
    let object = heap.get_mut(owner).unwrap();
    let ObjectKind::Vec(values) = &mut object.kind else {
        panic!("owner must remain a vec");
    };
    assert!(values.push(Value::ptr(late_child.index())));

    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }
    assert!(heap.get(owner).is_some());
    assert!(heap.get(late_child).is_some());
}

#[test]
fn allocation_safepoints_collect_without_execution_controls() {
    let mut vm = VM::new(Source::new("allocation-safepoint", "")).unwrap();
    let payload = "x".repeat(4_096);
    for _ in 0..300 {
        vm.alloc_string(&payload).unwrap();
    }

    assert!(vm.heap().minor_collection_count() > 0);
}

#[test]
fn sum_payload_survives_collection() {
    let mut heap = Heap::new();
    let payload = heap.alloc_string("payload");
    let sum = heap.alloc_sum(SumTag::ResultErr, Value::ptr(payload.index()));

    assert!(heap.begin_major_collection(vec![sum]));
    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }

    assert!(heap.get(sum).is_some());
    assert!(heap.get(payload).is_some());
    let ObjectKind::Sum(value) = &heap.get(sum).unwrap().kind else {
        panic!("sum root was reclaimed or changed");
    };
    assert_eq!(value.tag, SumTag::ResultErr);
    assert_eq!(value.payload.as_ptr(), Some(payload.index()));
}

#[test]
fn enum_slots_survive_collection_and_count_in_heap_size() {
    let mut heap = Heap::new();
    let first = heap.alloc_string("first");
    let second = heap.alloc_string("second");
    let object = GcObject::new(ObjectKind::Enum(AelysEnum::new(
        7,
        3,
        vec![Value::ptr(first.index()), Value::ptr(second.index())],
    )));
    let expected_size = Heap::estimate_object_size(&object);
    let before = heap.bytes_allocated();
    let value = heap.alloc(object);

    assert_eq!(heap.bytes_allocated().saturating_sub(before), expected_size);

    assert!(heap.begin_major_collection(vec![value]));
    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }

    assert!(heap.get(first).is_some());
    assert!(heap.get(second).is_some());
    let ObjectKind::Enum(value) = &heap.get(value).unwrap().kind else {
        panic!("enum root was reclaimed or changed");
    };
    assert_eq!(value.enum_id, 7);
    assert_eq!(value.variant_id, 3);
    assert_eq!(value.slots.len(), 2);
}
