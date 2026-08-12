use super::{Heap, HeapGeneration, MajorCollection};
use crate::object::{GcObject, GcRef, ObjectKind};

impl Heap {
    pub fn get(&self, gc_ref: GcRef) -> Option<&GcObject> {
        let slot = self.objects.get(gc_ref.slot_index())?;
        (slot.generation == gc_ref.generation())
            .then_some(slot.object.as_ref())
            .flatten()
    }

    pub fn get_mut(&mut self, gc_ref: GcRef) -> Option<&mut GcObject> {
        let index = gc_ref.slot_index();
        let remember = self.objects.get(index).is_some_and(|slot| {
            slot.generation == gc_ref.generation()
                && slot.object.is_some()
                && slot.heap_generation == HeapGeneration::Old
        });
        if remember {
            self.remembered_set
                .insert(u32::try_from(index).expect("heap slot index exceeds u32"));
        }
        let retrace = matches!(self.major_collection, MajorCollection::Mark { .. })
            && self.objects.get_mut(index).is_some_and(|slot| {
                if slot.generation != gc_ref.generation() {
                    return false;
                }
                slot.object.as_mut().is_some_and(|object| {
                    let was_marked = object.marked;
                    object.marked = false;
                    was_marked
                })
            });
        if retrace && let MajorCollection::Mark { worklist } = &mut self.major_collection {
            worklist.push(gc_ref);
        }
        let slot = self.objects.get_mut(index)?;
        (slot.generation == gc_ref.generation())
            .then_some(slot.object.as_mut())
            .flatten()
    }

    pub fn heap_generation(&self, gc_ref: GcRef) -> Option<HeapGeneration> {
        let slot = self.objects.get(gc_ref.slot_index())?;
        (slot.generation == gc_ref.generation() && slot.object.is_some())
            .then_some(slot.heap_generation)
    }

    pub fn write_barrier_ref(&mut self, owner: GcRef, child: GcRef) {
        if self.heap_generation(owner) == Some(HeapGeneration::Old)
            && self.heap_generation(child) == Some(HeapGeneration::Young)
        {
            self.remembered_set
                .insert(u32::try_from(owner.slot_index()).expect("heap slot index exceeds u32"));
        }
        if let MajorCollection::Mark { worklist } = &mut self.major_collection {
            worklist.push(child);
        }
    }

    pub fn write_barrier_value(&mut self, owner: GcRef, value: crate::Value) {
        if let Some(raw) = value.as_ptr() {
            self.write_barrier_ref(owner, GcRef::new(raw));
        }
    }

    pub fn remembered_count(&self) -> usize {
        self.remembered_set.len()
    }

    pub fn minor_collection_count(&self) -> u64 {
        self.minor_collections
    }

    pub fn major_collection_count(&self) -> u64 {
        self.major_collections
    }

    pub fn get_type_name(&self, gc_ref: GcRef) -> &'static str {
        if let Some(obj) = self.get(gc_ref) {
            match &obj.kind {
                ObjectKind::String(_) => "String",
                ObjectKind::Function(_) => "Function",
                ObjectKind::Native(_) => "NativeFunction",
                ObjectKind::Upvalue(_) => "Upvalue",
                ObjectKind::Closure(_) => "Closure",
                ObjectKind::Array(_) => "Array",
                ObjectKind::Vec(_) => "Vec",
                ObjectKind::Range(_) => "Range",
                ObjectKind::Sum(_) => "Sum",
            }
        } else {
            "Unknown"
        }
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_allocated >= self.next_gc
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    pub fn next_gc_threshold(&self) -> usize {
        self.next_gc
    }

    pub fn object_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|slot| slot.object.is_some())
            .count()
    }

    pub fn allocation_count(&self) -> u64 {
        self.allocation_count
    }
}
