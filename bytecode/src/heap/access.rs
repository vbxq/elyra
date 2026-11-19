use super::{Heap, HeapGeneration, MajorPhase, REMEMBERED, RememberedEntry, WHOLE_OBJECT};
use crate::object::{GcObject, GcRef, ObjectKind};

impl Heap {
    #[inline(always)]
    pub fn slot_object(&self, payload: u64) -> Option<&Option<GcObject>> {
        let slot = self.objects.get((payload & 0xff_ffff) as usize)?;
        (u64::from(slot.generation) == payload >> 24).then_some(&slot.object)
    }

    // stores of a pointer through this reference must be followed by a write barrier call
    #[inline(always)]
    pub fn slot_object_mut(&mut self, payload: u64) -> Option<&mut Option<GcObject>> {
        let slot = self.objects.get_mut((payload & 0xff_ffff) as usize)?;
        (u64::from(slot.generation) == payload >> 24).then_some(&mut slot.object)
    }

    pub fn get(&self, gc_ref: GcRef) -> Option<&GcObject> {
        let slot = self.objects.get(gc_ref.slot_index())?;
        (slot.generation == gc_ref.generation())
            .then_some(slot.object.as_ref())
            .flatten()
    }

    // stores of a pointer through this reference must be followed by a write barrier call
    pub fn get_mut(&mut self, gc_ref: GcRef) -> Option<&mut GcObject> {
        let slot = self.objects.get_mut(gc_ref.slot_index())?;
        (slot.generation == gc_ref.generation())
            .then_some(slot.object.as_mut())
            .flatten()
    }

    pub fn heap_generation(&self, gc_ref: GcRef) -> Option<HeapGeneration> {
        let slot = self.objects.get(gc_ref.slot_index())?;
        (slot.generation == gc_ref.generation() && slot.object.is_some())
            .then_some(slot.heap_generation)
    }

    #[inline]
    pub fn write_barrier_ref(&mut self, owner: GcRef, child: GcRef) {
        self.write_barrier(owner, WHOLE_OBJECT, child);
    }

    #[inline]
    pub fn write_barrier_value(&mut self, owner: GcRef, value: crate::Value) {
        if let Some(raw) = value.as_ptr() {
            self.write_barrier(owner, WHOLE_OBJECT, GcRef::new(raw));
        }
    }

    #[inline]
    pub fn write_barrier_element(&mut self, owner: GcRef, index: usize, value: crate::Value) {
        if let Some(raw) = value.as_ptr() {
            let element = u32::try_from(index)
                .ok()
                .filter(|element| *element != WHOLE_OBJECT)
                .unwrap_or(WHOLE_OBJECT);
            self.write_barrier(owner, element, GcRef::new(raw));
        }
    }

    pub fn write_barrier_elements(&mut self, owner: GcRef, range: std::ops::Range<usize>) {
        if self.heap_generation(owner) != Some(HeapGeneration::Old)
            && self.phase != MajorPhase::Mark
        {
            return;
        }
        for index in range {
            let Some(value) = self
                .get(owner)
                .and_then(Self::element_values)
                .and_then(|values| values.get(index).copied())
            else {
                break;
            };
            self.write_barrier_element(owner, index, value);
        }
    }

    fn write_barrier(&mut self, owner: GcRef, element: u32, child: GcRef) {
        let old_to_young = match (
            self.objects.get(owner.slot_index()),
            self.objects.get(child.slot_index()),
        ) {
            (Some(owner_slot), Some(child_slot)) => {
                owner_slot.heap_generation == HeapGeneration::Old
                    && child_slot.heap_generation == HeapGeneration::Young
                    && owner_slot.generation == owner.generation()
                    && child_slot.generation == child.generation()
                    && child_slot.object.is_some()
            }
            _ => false,
        };
        if old_to_young {
            let indexed_owner = self.objects[owner.slot_index()]
                .object
                .as_ref()
                .is_some_and(|object| {
                    matches!(
                        object.kind,
                        ObjectKind::Vec(_) | ObjectKind::Array(_) | ObjectKind::Map(_)
                    )
                });
            if self.remembered.len() >= Self::REMEMBERED_LIMIT
                || (indexed_owner && element == WHOLE_OBJECT)
            {
                self.promote(child);
            } else {
                self.remember(owner, element, indexed_owner);
            }
        }
        if self.phase == MajorPhase::Mark {
            self.shade(child);
        }
    }

    pub fn write_barrier_all_elements(&mut self, owner: GcRef) {
        if self.heap_generation(owner) != Some(HeapGeneration::Old)
            && self.phase != MajorPhase::Mark
        {
            return;
        }
        let marking = self.phase == MajorPhase::Mark;
        let mut pairs = Vec::new();
        if let Some(object) = self.get(owner) {
            Self::indexed_child_pairs(object, &mut pairs, |child| {
                marking || self.heap_generation(child) == Some(HeapGeneration::Young)
            });
        }
        for (element, child) in pairs {
            let element = u32::try_from(element)
                .ok()
                .filter(|element| *element != WHOLE_OBJECT)
                .unwrap_or(WHOLE_OBJECT);
            self.write_barrier(owner, element, child);
        }
    }

    fn promote(&mut self, child: GcRef) {
        let slot = self.objects[child.slot_index()].as_mut();
        slot.heap_generation = HeapGeneration::Old;
        let has_children = slot
            .object
            .as_ref()
            .is_some_and(|object| Self::may_have_children(&object.kind));
        if has_children && slot.flags & REMEMBERED == 0 {
            slot.flags |= REMEMBERED;
            self.remembered.push(RememberedEntry {
                owner: child,
                element: WHOLE_OBJECT,
            });
        }
    }

    fn remember(&mut self, owner: GcRef, element: u32, indexed_owner: bool) {
        let index = owner.slot_index();
        if !indexed_owner {
            let slot = self.objects[index].as_mut();
            if slot.flags & REMEMBERED == 0 {
                slot.flags |= REMEMBERED;
                self.remembered.push(RememberedEntry {
                    owner,
                    element: WHOLE_OBJECT,
                });
            }
            return;
        }
        let mut table = self.objects[index].card_table;
        if table == 0 {
            table = match self.free_card_tables.pop() {
                Some(table) => table,
                None => {
                    self.card_tables.push(Vec::new());
                    u32::try_from(self.card_tables.len() - 1).expect("card table limit exceeded")
                }
            };
            self.objects[index].card_table = table;
        }
        let bits = &mut self.card_tables[table as usize];
        let word = (element / 64) as usize;
        let bit = 1u64 << (element % 64);
        if bits.len() <= word {
            bits.resize(word + 1, 0);
        }
        if bits[word] & bit == 0 {
            bits[word] |= bit;
            self.remembered.push(RememberedEntry { owner, element });
        }
    }

    pub fn interned_count(&self) -> usize {
        self.intern_table.len()
    }

    pub fn remembered_count(&self) -> usize {
        self.remembered.len()
    }

    pub fn young_count(&self) -> usize {
        self.young.len()
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
                ObjectKind::Map(_) => "Map",
                ObjectKind::Range(_) => "Range",
                ObjectKind::Sum(_) => "Sum",
                ObjectKind::Enum(_) => "Enum",
                ObjectKind::Struct(_) => "Struct",
                ObjectKind::Int(_) => "Int",
            }
        } else {
            "Unknown"
        }
    }

    pub fn should_collect(&self) -> bool {
        self.minor_due() || self.major_due()
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    pub fn account_reallocation(&mut self, previous_size: usize, current_size: usize) {
        if current_size > previous_size {
            let growth = current_size - previous_size;
            self.bytes_allocated = self.bytes_allocated.saturating_add(growth);
            self.peak_bytes = self.peak_bytes.max(self.bytes_allocated);
            self.alloc_clock = self
                .alloc_clock
                .saturating_add(u64::try_from(growth).unwrap_or(u64::MAX));
        } else {
            self.bytes_allocated = self
                .bytes_allocated
                .saturating_sub(previous_size - current_size);
        }
    }

    pub fn next_gc_threshold(&self) -> usize {
        self.next_gc
    }

    pub fn object_count(&self) -> usize {
        self.live_objects
    }

    pub fn slot_count(&self) -> usize {
        self.objects.len()
    }

    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes
    }

    pub fn total_allocated_bytes(&self) -> u64 {
        self.alloc_clock
    }

    pub fn allocation_count(&self) -> u64 {
        self.allocation_count
    }
}
