use aelys_bytecode::{
    AelysVec, GcObject, GcRef, Heap, HeapGeneration, MajorSliceResult, MajorStep, ObjectKind, Value,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

struct Model {
    heap: Heap,
    refs: Vec<GcRef>,
    edges: Vec<Vec<Option<usize>>>,
    roots: Vec<usize>,
}

impl Model {
    fn new() -> Self {
        Self {
            heap: Heap::new(),
            refs: Vec::new(),
            edges: Vec::new(),
            roots: Vec::new(),
        }
    }

    fn alloc(&mut self, children: Vec<Option<usize>>) -> usize {
        let id = self.refs.len();
        let mut values = vec![Value::int(id as i64)];
        values.extend(children.iter().map(|child| match child {
            Some(child) => Value::ptr(self.refs[*child].index()),
            None => Value::null(),
        }));
        let reference = self
            .heap
            .alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
                values,
            ))));
        self.refs.push(reference);
        self.edges.push(children);
        id
    }

    fn set_edge(&mut self, owner: usize, slot: usize, child: Option<usize>) {
        let value = match child {
            Some(child) => Value::ptr(self.refs[child].index()),
            None => Value::null(),
        };
        let reference = self.refs[owner];
        let ObjectKind::Vec(vector) = &mut self.heap.get_mut(reference).unwrap().kind else {
            panic!("model node must be a vec");
        };
        assert!(vector.set(slot + 1, value));
        self.heap.write_barrier_element(reference, slot + 1, value);
        self.edges[owner][slot] = child;
    }

    fn push_edge(&mut self, owner: usize, child: usize) {
        let value = Value::ptr(self.refs[child].index());
        let reference = self.refs[owner];
        let ObjectKind::Vec(vector) = &mut self.heap.get_mut(reference).unwrap().kind else {
            panic!("model node must be a vec");
        };
        assert!(vector.push(value));
        let index = vector.len() - 1;
        self.heap.write_barrier_element(reference, index, value);
        self.edges[owner].push(Some(child));
    }

    fn root_refs(&self) -> Vec<GcRef> {
        self.roots.iter().map(|root| self.refs[*root]).collect()
    }

    fn reachable(&self) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut stack = self.roots.clone();
        while let Some(node) = stack.pop() {
            if seen.insert(node) {
                stack.extend(self.edges[node].iter().flatten().copied());
            }
        }
        seen
    }

    fn verify(&self, context: &str) {
        for node in self.reachable() {
            let object = self
                .heap
                .get(self.refs[node])
                .unwrap_or_else(|| panic!("{context}: reachable node {node} was freed"));
            let ObjectKind::Vec(vector) = &object.kind else {
                panic!("{context}: node {node} changed kind");
            };
            let values = vector.objects().unwrap();
            assert_eq!(
                values[0].as_int(),
                Some(node as i64),
                "{context}: node {node} id"
            );
            assert_eq!(
                values.len(),
                self.edges[node].len() + 1,
                "{context}: node {node} len"
            );
            for (slot, child) in self.edges[node].iter().enumerate() {
                let expected = child.map(|child| self.refs[child].index());
                assert_eq!(
                    values[slot + 1].as_ptr(),
                    expected,
                    "{context}: node {node} slot {slot}"
                );
            }
        }
    }

    fn pick_reachable(&self, rng: &mut Rng) -> Option<usize> {
        let live: Vec<usize> = self.reachable().into_iter().collect();
        if live.is_empty() {
            None
        } else {
            let mut sorted = live;
            sorted.sort_unstable();
            Some(sorted[rng.below(sorted.len())])
        }
    }

    fn minor(&mut self) {
        let roots = self.root_refs();
        self.heap.mark_young(roots);
        self.heap.sweep_young();
    }

    fn major_slice(&mut self, units: usize) {
        if !self.heap.major_collection_active() {
            let roots = self.root_refs();
            self.heap.begin_major_collection(roots);
        }
        let mut budget = units;
        loop {
            match self.heap.major_step(&mut budget, None) {
                MajorStep::Idle | MajorStep::BudgetExhausted | MajorStep::Complete { .. } => {
                    return;
                }
                MajorStep::NeedsRoots => {
                    let roots = self.root_refs();
                    if !self.heap.add_major_roots(roots) {
                        self.heap.finish_marking();
                    }
                }
            }
        }
    }
}

fn run_random_graph(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let mut model = Model::new();
    for _ in 0..8 {
        let node = model.alloc(vec![None; 3]);
        model.roots.push(node);
    }
    for step in 0..steps {
        match rng.below(100) {
            0..=29 => {
                let mut children = Vec::new();
                for _ in 0..rng.below(4) {
                    children.push(model.pick_reachable(&mut rng));
                }
                let node = model.alloc(children);
                if rng.below(3) == 0 || model.roots.len() < 2 {
                    model.roots.push(node);
                } else if let Some(owner) = model.pick_reachable(&mut rng) {
                    model.push_edge(owner, node);
                }
            }
            30..=54 => {
                if let Some(owner) = model.pick_reachable(&mut rng)
                    && !model.edges[owner].is_empty()
                {
                    let slot = rng.below(model.edges[owner].len());
                    let child = if rng.below(5) == 0 {
                        None
                    } else {
                        model.pick_reachable(&mut rng)
                    };
                    model.set_edge(owner, slot, child);
                }
            }
            55..=62 => {
                if model.roots.len() > 1 {
                    let index = rng.below(model.roots.len());
                    model.roots.swap_remove(index);
                }
            }
            63..=72 => {
                if let Some(node) = model.pick_reachable(&mut rng) {
                    model.roots.push(node);
                }
            }
            73..=84 => {
                model.minor();
                model.verify(&format!("seed {seed} step {step} after minor"));
            }
            _ => {
                let units = 1 + rng.below(40);
                model.major_slice(units);
                model.verify(&format!("seed {seed} step {step} after major slice"));
            }
        }
    }
    model.minor();
    while model.heap.major_collection_active() {
        model.major_slice(usize::MAX);
    }
    model.major_slice(usize::MAX);
    while model.heap.major_collection_active() {
        model.major_slice(usize::MAX);
    }
    model.verify(&format!("seed {seed} final"));
    let reachable = model.reachable();
    for (node, reference) in model.refs.iter().enumerate() {
        if !reachable.contains(&node) {
            assert!(
                model.heap.get(*reference).is_none(),
                "seed {seed}: unreachable node {node} survived a full collection"
            );
        }
    }
    assert_eq!(model.heap.object_count(), reachable.len());
}

#[test]
fn random_graphs_with_interleaved_minor_and_incremental_major_keep_every_reachable_node() {
    for seed in [
        0x9e37_79b9_7f4a_7c15,
        7,
        1234567,
        0xdead_beef,
        42,
        99991,
        31337,
        5,
    ] {
        run_random_graph(seed, 3000);
    }
}

#[test]
fn cycles_survive_while_rooted_and_are_reclaimed_once_unrooted() {
    let mut model = Model::new();
    let a = model.alloc(vec![None]);
    let b = model.alloc(vec![Some(a)]);
    model.set_edge(a, 0, Some(b));
    model.roots.push(a);
    for _ in 0..5 {
        model.minor();
        model.verify("rooted cycle after minor");
    }
    model.major_slice(usize::MAX);
    while model.heap.major_collection_active() {
        model.major_slice(usize::MAX);
    }
    model.verify("rooted cycle after major");
    model.roots.clear();
    model.major_slice(usize::MAX);
    while model.heap.major_collection_active() {
        model.major_slice(usize::MAX);
    }
    assert!(model.heap.get(model.refs[a]).is_none());
    assert!(model.heap.get(model.refs[b]).is_none());
}

#[test]
fn old_to_young_edge_through_a_large_vec_slot_survives_minor_without_rooting_the_child() {
    let mut model = Model::new();
    let owner = model.alloc(vec![None; 100_000]);
    model.roots.push(owner);
    model.minor();
    assert_eq!(
        model.heap.heap_generation(model.refs[owner]),
        Some(HeapGeneration::Old)
    );
    let mut stored = Vec::new();
    for round in 0..50 {
        let child = model.alloc(Vec::new());
        let slot = (round * 7919) % 100_000;
        model.set_edge(owner, slot, Some(child));
        stored.push(child);
    }
    assert_eq!(model.heap.remembered_count(), 50);
    model.minor();
    model.verify("large vec old to young");
    assert_eq!(model.heap.remembered_count(), 0);
}

#[test]
fn allocation_during_major_sweep_does_not_hide_children_from_the_next_minor() {
    let mut heap = Heap::new();
    let root = heap.alloc_string("root");
    let garbage = heap.alloc_string("old garbage");
    for _ in 0..2 {
        heap.mark_young([root, garbage]);
        heap.sweep_young();
    }
    assert_eq!(heap.heap_generation(garbage), Some(HeapGeneration::Old));

    assert!(heap.begin_major_collection(vec![root]));
    let mut guard = 0;
    while heap.get(garbage).is_some() {
        assert_ne!(
            heap.major_collection_slice(Duration::ZERO),
            MajorSliceResult::Idle
        );
        guard += 1;
        assert!(guard < 10_000, "major sweep never reached the garbage slot");
    }
    assert!(heap.major_collection_active());
    let holder = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        Vec::new(),
    ))));
    while heap.major_collection_active() {
        heap.major_collection_slice(Duration::ZERO);
    }

    let child = heap.alloc_string("young child stored after the major");
    let ObjectKind::Vec(values) = &mut heap.get_mut(holder).unwrap().kind else {
        panic!("holder must be a vec");
    };
    assert!(values.push(Value::ptr(child.index())));
    heap.write_barrier_value(holder, Value::ptr(child.index()));

    heap.mark_young([holder]);
    heap.sweep_young();
    assert!(
        heap.get(child).is_some(),
        "a child of a live holder was freed by the minor"
    );
    let ObjectKind::Vec(values) = &heap.get(holder).unwrap().kind else {
        panic!("holder must be a vec");
    };
    assert_eq!(values.objects().unwrap()[0].as_ptr(), Some(child.index()));
}

#[test]
fn incremental_marking_terminates_while_a_large_vec_is_rewritten_between_every_slice() {
    const LEN: usize = 20_000;
    let mut heap = Heap::new();
    let mut initial = Vec::with_capacity(LEN);
    for index in 0..LEN {
        initial.push(Value::ptr(heap.alloc_string(&format!("e{index}")).index()));
    }
    let owner = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        initial,
    ))));
    for _ in 0..2 {
        heap.mark_young([owner]);
        heap.sweep_young();
    }

    assert!(heap.begin_major_collection(vec![owner]));
    let mut iterations = 0usize;
    let mut expected = HashMap::new();
    while heap.major_collection_active() {
        let index = iterations % LEN;
        let child = heap.alloc_string(&format!("w{iterations}"));
        let value = Value::ptr(child.index());
        let ObjectKind::Vec(values) = &mut heap.get_mut(owner).unwrap().kind else {
            panic!("owner must remain a vec");
        };
        assert!(values.set(index, value));
        heap.write_barrier_value(owner, value);
        expected.insert(index, format!("w{iterations}"));
        heap.major_collection_slice(Duration::from_micros(50));
        iterations += 1;
        assert!(
            iterations < 1_000,
            "marking did not terminate within 1000 slices while the vec was rewritten"
        );
    }
    heap.mark_young([owner]);
    heap.sweep_young();
    let ObjectKind::Vec(values) = &heap.get(owner).unwrap().kind else {
        panic!("owner must remain a vec");
    };
    for (index, value) in values.objects().unwrap().iter().enumerate() {
        let reference = GcRef::new(value.as_ptr().expect("element must be a pointer"));
        let ObjectKind::String(text) = &heap.get(reference).expect("element freed").kind else {
            panic!("element must be a string");
        };
        let wanted = expected
            .get(&index)
            .cloned()
            .unwrap_or_else(|| format!("e{index}"));
        assert_eq!(text.as_str(), wanted);
    }
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes one timespec through a valid pointer
    let status = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    assert_eq!(status, 0, "the thread cpu clock is unavailable");
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

fn minor_pause_with_old_heap(live: usize) -> Duration {
    let mut heap = Heap::new();
    let mut values = Vec::with_capacity(live);
    for index in 0..live {
        values.push(Value::ptr(heap.alloc_string(&format!("{index}")).index()));
    }
    let holder = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        values,
    ))));
    for _ in 0..2 {
        heap.mark_young([holder]);
        heap.sweep_young();
    }
    let mut pauses = Vec::new();
    for _ in 0..15 {
        for index in 0..2_000 {
            heap.alloc_string(&format!("garbage {index}"));
        }
        let started = thread_cpu_time();
        heap.mark_young([holder]);
        heap.sweep_young();
        pauses.push(thread_cpu_time() - started);
    }
    assert_eq!(heap.object_count(), live + 1);
    median(pauses)
}

#[test]
fn minor_pause_does_not_grow_with_the_old_heap() {
    let small = minor_pause_with_old_heap(10_000);
    let large = minor_pause_with_old_heap(300_000);
    assert!(
        large < small * 4 + Duration::from_micros(500),
        "minor pause grew with the old heap: {small:?} at 10k live, {large:?} at 300k live"
    );
}

#[test]
fn every_major_slice_stays_near_its_budget_on_a_large_heap() {
    let mut heap = Heap::new();
    let mut values = Vec::with_capacity(300_000);
    for index in 0..300_000 {
        values.push(Value::ptr(heap.alloc_string(&format!("{index}")).index()));
    }
    let holder = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        values,
    ))));
    for _ in 0..2 {
        heap.mark_young([holder]);
        heap.sweep_young();
    }
    let mut garbage = Vec::with_capacity(100_000);
    for index in 0..100_000 {
        garbage.push(Value::ptr(
            heap.alloc_string(&format!("old garbage {index}")).index(),
        ));
    }
    let garbage = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        garbage,
    ))));
    for _ in 0..2 {
        heap.mark_young([holder, garbage]);
        heap.sweep_young();
    }
    assert_eq!(heap.heap_generation(garbage), Some(HeapGeneration::Old));

    assert!(heap.begin_major_collection(vec![holder]));
    let mut slices = Vec::new();
    while heap.major_collection_active() {
        let started = thread_cpu_time();
        heap.major_collection_slice(Duration::from_micros(500));
        slices.push(thread_cpu_time() - started);
    }
    let worst = slices.iter().max().copied().unwrap_or_default();
    let typical = median(slices.clone());
    assert!(slices.len() > 3);
    assert!(
        worst < typical * 10 + Duration::from_millis(15),
        "a major slice took {worst:?} of cpu time while the median slice took {typical:?}"
    );
    assert_eq!(heap.object_count(), 300_001);
}

#[test]
fn a_major_slice_stops_soon_after_its_deadline_on_a_large_heap() {
    let mut heap = Heap::new();
    let values = (0..300_000)
        .map(|index| Value::ptr(heap.alloc_string(&format!("{index}")).index()))
        .collect();
    let holder = heap.alloc(GcObject::new(ObjectKind::Vec(AelysVec::from_objects(
        values,
    ))));
    for _ in 0..2 {
        heap.mark_young([holder]);
        heap.sweep_young();
    }
    assert!(heap.begin_major_collection(vec![holder]));
    // the deadline is read every 512 units, and one unit can be a scanned chunk of 2048 elements
    const AFTER_DEADLINE: usize = 512 + 2_049;
    let mut slices = 0usize;
    loop {
        let mut units = usize::MAX;
        let step = heap.major_step(&mut units, Some(Instant::now()));
        let spent = usize::MAX - units;
        assert!(
            spent <= AFTER_DEADLINE,
            "a slice whose deadline had passed did {spent} units of work"
        );
        slices += 1;
        match step {
            MajorStep::BudgetExhausted => {}
            MajorStep::NeedsRoots => heap.finish_marking(),
            MajorStep::Complete { .. } | MajorStep::Idle => break,
        }
    }
    assert!(slices > 100, "the collection ran in only {slices} slices");
    assert_eq!(heap.object_count(), 300_001);
}
