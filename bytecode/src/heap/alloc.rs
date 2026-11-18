use super::Heap;
use crate::Function;
use crate::object::{
    AelysEnum, AelysFunction, AelysString, AelysStruct, AelysSum, GcObject, GcRef, NativeFunction,
    ObjectKind, SumTag,
};

impl Heap {
    pub fn alloc(&mut self, mut obj: GcObject) -> GcRef {
        let size = Self::estimate_object_size(&obj);
        if let ObjectKind::String(string) = &obj.kind {
            string.commit_charge();
        }
        self.bytes_allocated += size;
        self.peak_bytes = self.peak_bytes.max(self.bytes_allocated);
        self.alloc_clock = self
            .alloc_clock
            .saturating_add(u64::try_from(size).unwrap_or(u64::MAX));
        self.allocation_count = self.allocation_count.saturating_add(1);
        self.live_objects += 1;

        let index = match self.free_list.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.objects.len()).expect("heap slot limit exceeded");
                self.objects.push(Box::new(super::HeapSlot {
                    generation: 0,
                    card_table: 0,
                    heap_generation: super::HeapGeneration::Young,
                    flags: 0,
                    object: None,
                }));
                index
            }
        };
        let position = index as usize;
        obj.marked = match self.phase {
            super::MajorPhase::Idle => false,
            super::MajorPhase::Mark => true,
            super::MajorPhase::Sweep => position >= self.sweep_cursor,
        };
        let gray = self.phase == super::MajorPhase::Mark && Self::may_have_children(&obj.kind);
        let slot = self.objects[position].as_mut();
        slot.object = Some(obj);
        slot.heap_generation = super::HeapGeneration::Young;
        slot.flags = 0;
        let reference = GcRef::from_parts(index, slot.generation);
        self.young.push(index);
        if gray {
            self.gray.push((reference, 0));
        }
        reference
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
        self.alloc(GcObject::new(ObjectKind::Function(Box::new(
            AelysFunction::with_constants(func, constants),
        ))))
    }

    pub fn alloc_native(&mut self, name: &str, arity: u16) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Native(NativeFunction::new(
            name, arity,
        ))))
    }

    pub fn alloc_foreign(&mut self, name: &str, arity: u16) -> GcRef {
        self.alloc_native(name, arity)
    }

    pub fn alloc_sum(&mut self, tag: SumTag, payload: crate::Value) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Sum(AelysSum::new(tag, payload))))
    }

    pub fn alloc_enum(&mut self, enum_id: u16, variant_id: u16, slots: Vec<crate::Value>) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Enum(AelysEnum::new(
            enum_id, variant_id, slots,
        ))))
    }

    pub fn alloc_struct(&mut self, schema_id: crate::SchemaId, slots: Vec<crate::Value>) -> GcRef {
        self.alloc(GcObject::new(ObjectKind::Struct(Box::new(
            AelysStruct::new(schema_id, slots),
        ))))
    }
}
