use super::Heap;
use crate::Function;
use crate::object::{
    AelysFunction, AelysString, AelysSum, GcObject, GcRef, NativeFunction, ObjectKind, SumTag,
};

impl Heap {
    pub fn alloc(&mut self, mut obj: GcObject) -> GcRef {
        self.bytes_allocated += Self::estimate_object_size(&obj);
        self.allocation_count = self.allocation_count.saturating_add(1);
        if self.major_collection_active() {
            obj.marked = true;
        }

        // reuse free slots when possible
        if let Some(idx) = self.free_list.pop() {
            let slot = self.objects[idx as usize].as_mut();
            slot.object = Some(obj);
            slot.heap_generation = super::HeapGeneration::Young;
            slot.survival_count = 0;
            GcRef::from_parts(idx, slot.generation)
        } else {
            let idx = u32::try_from(self.objects.len()).expect("heap slot limit exceeded");
            self.objects.push(Box::new(super::HeapSlot {
                generation: 0,
                heap_generation: super::HeapGeneration::Young,
                survival_count: 0,
                object: Some(obj),
            }));
            GcRef::from_parts(idx, 0)
        }
    }

    pub fn alloc_string(&mut self, s: &str) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::String(AelysString::new(s))))
    }

    pub fn alloc_function(&mut self, func: Function) -> GcRef {
        let constants = func
            .constants
            .iter()
            .map(|constant| constant.materialize(self))
            .collect();
        self.alloc(GcObject::new(ObjectKind::Function(
            AelysFunction::with_constants(func, constants),
        )))
    }

    pub fn alloc_native(&mut self, name: &str, arity: u16) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Native(NativeFunction::new(
            name, arity,
        ))))
    }

    // same as alloc_native, just different name for clarity in calling code
    pub fn alloc_foreign(&mut self, name: &str, arity: u16) -> GcRef {
        self.alloc_native(name, arity)
    }

    pub fn alloc_sum(&mut self, tag: SumTag, payload: crate::Value) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Sum(AelysSum::new(tag, payload))))
    }
}
