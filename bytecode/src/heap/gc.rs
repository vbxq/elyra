use super::{Heap, HeapGeneration, MajorPhase, REMEMBERED, WHOLE_OBJECT, YOUNG_MARK};
use crate::object::{AelysClosure, AelysString, AelysUpvalue, GcObject, GcRef, ObjectKind};
use crate::value::Value;

impl Heap {
    pub fn mark(&mut self, root: GcRef) {
        self.shade(root);
        while let Some(entry) = self.gray.pop() {
            self.scan_gray(entry);
        }
    }

    pub(super) fn shade(&mut self, reference: GcRef) {
        let Some(slot) = self.objects.get_mut(reference.slot_index()) else {
            return;
        };
        if slot.generation != reference.generation() {
            return;
        }
        let Some(object) = slot.object.as_mut() else {
            return;
        };
        if object.marked {
            return;
        }
        object.marked = true;
        if Self::may_have_children(&object.kind) {
            self.gray.push((reference, 0));
        }
    }

    pub(super) fn scan_gray(&mut self, (reference, start): (GcRef, u32)) -> usize {
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        let mut continuation = None;
        let mut scanned = 0;
        if let Some(object) = self.get(reference) {
            let start = start as usize;
            match Self::element_values(object) {
                Some(values) => {
                    let end = values.len().min(start.saturating_add(Self::MARK_CHUNK));
                    scanned = end.saturating_sub(start);
                    if start < end {
                        scratch.extend(
                            values[start..end]
                                .iter()
                                .filter_map(|value| value.as_ptr().map(GcRef::new)),
                        );
                    }
                    if end < values.len() {
                        continuation = u32::try_from(end).ok();
                    }
                }
                None => Self::collect_children(object, &mut scratch),
            }
        }
        if let Some(end) = continuation {
            self.gray.push((reference, end));
        }
        let work = scratch.len().max(scanned) + 1;
        for child in scratch.iter().copied() {
            self.shade(child);
        }
        self.scratch = scratch;
        work
    }

    pub(super) fn may_have_children(kind: &ObjectKind) -> bool {
        !matches!(
            kind,
            ObjectKind::String(_)
                | ObjectKind::Native(_)
                | ObjectKind::Range(_)
                | ObjectKind::Int(_)
        )
    }

    pub(super) fn element_values(object: &GcObject) -> Option<&[Value]> {
        match &object.kind {
            ObjectKind::Vec(vector) => Some(vector.objects().unwrap_or(&[])),
            ObjectKind::Array(array) => Some(array.data.as_objects().unwrap_or(&[])),
            _ => None,
        }
    }

    fn collect_children(object: &GcObject, children: &mut Vec<GcRef>) {
        let pointers = |values: &[Value], children: &mut Vec<GcRef>| {
            children.extend(
                values
                    .iter()
                    .filter_map(|value| value.as_ptr().map(GcRef::new)),
            );
        };
        match &object.kind {
            ObjectKind::Function(function) => pointers(&function.constants, children),
            ObjectKind::Closure(closure) => {
                children.push(closure.function);
                children.extend(closure.upvalues.iter().copied());
            }
            ObjectKind::Upvalue(upvalue) => {
                if let crate::object::UpvalueLocation::Closed(value) = &upvalue.location
                    && let Some(pointer) = value.as_ptr()
                {
                    children.push(GcRef::new(pointer));
                }
            }
            ObjectKind::String(_)
            | ObjectKind::Native(_)
            | ObjectKind::Range(_)
            | ObjectKind::Int(_) => {}
            ObjectKind::Array(array) => {
                if let Some(objects) = array.data.as_objects() {
                    pointers(objects, children);
                }
            }
            ObjectKind::Vec(vector) => {
                if let Some(objects) = vector.objects() {
                    pointers(objects, children);
                }
            }
            ObjectKind::Map(map) => {
                for (key, value) in map.entries() {
                    children.extend(key.as_ptr().map(GcRef::new));
                    children.extend(value.as_ptr().map(GcRef::new));
                }
            }
            ObjectKind::Sum(sum) => {
                if let Some(pointer) = sum.payload.as_ptr() {
                    children.push(GcRef::new(pointer));
                }
            }
            ObjectKind::Enum(value) => pointers(&value.slots, children),
            ObjectKind::Struct(structure) => pointers(&structure.slots, children),
        }
    }

    fn young_shade(&mut self, reference: GcRef) {
        let Some(slot) = self.objects.get_mut(reference.slot_index()) else {
            return;
        };
        if slot.generation != reference.generation()
            || slot.heap_generation != HeapGeneration::Young
            || slot.flags & YOUNG_MARK != 0
        {
            return;
        }
        let Some(object) = slot.object.as_ref() else {
            return;
        };
        slot.flags |= YOUNG_MARK;
        if Self::may_have_children(&object.kind) {
            self.young_stack.push(reference);
        }
    }

    fn live_old_owner(&self, owner: GcRef) -> Option<&GcObject> {
        let slot = self.objects.get(owner.slot_index())?;
        if slot.generation != owner.generation() || slot.heap_generation != HeapGeneration::Old {
            return None;
        }
        slot.object.as_ref()
    }

    pub fn verify_remembered(&self) -> Result<(), String> {
        let mut children = Vec::new();
        for (index, slot) in self.objects.iter().enumerate() {
            let Some(object) = slot.object.as_ref() else {
                continue;
            };
            if slot.heap_generation != HeapGeneration::Old || slot.flags & REMEMBERED != 0 {
                continue;
            }
            children.clear();
            match Self::element_values(object) {
                Some(values) => {
                    children.extend(values.iter().enumerate().filter_map(|(element, value)| {
                        value.as_ptr().map(|raw| (Some(element), GcRef::new(raw)))
                    }))
                }
                None => {
                    let mut refs = Vec::new();
                    Self::collect_children(object, &mut refs);
                    children.extend(refs.into_iter().map(|child| (None, child)));
                }
            }
            for (element, child) in children.iter().copied() {
                if self.heap_generation(child) != Some(HeapGeneration::Young) {
                    continue;
                }
                let remembered = element.is_some_and(|element| {
                    slot.card_table != 0
                        && self.card_tables[slot.card_table as usize]
                            .get(element / 64)
                            .is_some_and(|word| word & (1u64 << (element % 64)) != 0)
                });
                if !remembered {
                    let place =
                        element.map_or_else(String::new, |element| format!(" slot {element}"));
                    return Err(format!(
                        "gc-verify: old {} #{index}{place} holds young {} #{} without a remembered entry (missing write barrier)",
                        self.get_type_name(GcRef::from_parts(
                            u32::try_from(index).unwrap_or(u32::MAX),
                            slot.generation
                        )),
                        self.get_type_name(child),
                        child.slot_index()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn mark_young(&mut self, roots: impl IntoIterator<Item = GcRef>) {
        if self.verify
            && let Err(message) = self.verify_remembered()
        {
            panic!("{message}");
        }
        let mut work = 0u64;
        for root in roots {
            work += 1;
            self.young_shade(root);
        }
        let mut scratch = std::mem::take(&mut self.scratch);
        for position in 0..self.remembered.len() {
            let entry = self.remembered[position];
            scratch.clear();
            let mut scanned = 0;
            if let Some(object) = self.live_old_owner(entry.owner) {
                if entry.element == WHOLE_OBJECT {
                    scanned = Self::element_values(object).map_or(0, <[Value]>::len);
                    Self::collect_children(object, &mut scratch);
                } else if let Some(value) = Self::element_values(object)
                    .and_then(|values| values.get(entry.element as usize))
                    && let Some(pointer) = value.as_ptr()
                {
                    scratch.push(GcRef::new(pointer));
                }
            }
            work += 1 + scratch.len().max(scanned) as u64;
            for child in scratch.iter().copied() {
                self.young_shade(child);
            }
        }
        while let Some(reference) = self.young_stack.pop() {
            scratch.clear();
            let mut scanned = 0;
            if let Some(object) = self.get(reference) {
                scanned = Self::element_values(object).map_or(0, <[Value]>::len);
                Self::collect_children(object, &mut scratch);
            }
            work += 1 + scratch.len().max(scanned) as u64;
            for child in scratch.iter().copied() {
                self.young_shade(child);
            }
        }
        self.scratch = scratch;
        self.pause_work = self.pause_work.saturating_add(work);
    }

    pub fn sweep_young(&mut self) -> usize {
        let mut freed = 0;
        let mut young = std::mem::take(&mut self.young);
        self.pause_work = self
            .pause_work
            .saturating_add((young.len() + self.remembered.len()) as u64);
        for index in young.iter().copied() {
            let position = index as usize;
            let slot = self.objects[position].as_mut();
            if slot.object.is_none() || slot.heap_generation != HeapGeneration::Young {
                continue;
            }
            if slot.flags & YOUNG_MARK != 0 {
                slot.flags &= !YOUNG_MARK;
                slot.heap_generation = HeapGeneration::Old;
            } else {
                self.free_slot(position);
                freed += 1;
            }
        }
        young.clear();
        self.young = young;
        self.clear_remembered();
        self.minor_collections = self.minor_collections.saturating_add(1);
        self.minor_at = self.alloc_clock.saturating_add(self.nursery_bytes);
        self.update_next_event();
        freed
    }

    fn clear_remembered(&mut self) {
        let remembered = std::mem::take(&mut self.remembered);
        for entry in remembered.iter() {
            let Some(slot) = self.objects.get_mut(entry.owner.slot_index()) else {
                continue;
            };
            if slot.generation != entry.owner.generation() {
                continue;
            }
            if entry.element == WHOLE_OBJECT {
                slot.flags &= !REMEMBERED;
            } else if slot.card_table != 0 {
                let bits = &mut self.card_tables[slot.card_table as usize];
                if let Some(word) = bits.get_mut((entry.element / 64) as usize) {
                    *word &= !(1u64 << (entry.element % 64));
                }
            }
        }
        let mut remembered = remembered;
        remembered.clear();
        self.remembered = remembered;
    }

    pub fn sweep(&mut self) -> usize {
        let mut freed = 0;
        self.pause_work = self.pause_work.saturating_add(self.objects.len() as u64);
        for index in 0..self.objects.len() {
            let slot = self.objects[index].as_mut();
            let Some(object) = slot.object.as_mut() else {
                continue;
            };
            if object.marked {
                object.marked = false;
            } else {
                self.free_slot(index);
                freed += 1;
            }
        }
        self.gray.clear();
        self.clear_remembered();
        let mut young = std::mem::take(&mut self.young);
        young.retain(|index| {
            let slot = &self.objects[*index as usize];
            slot.object.is_some() && slot.heap_generation == HeapGeneration::Young
        });
        self.young = young;
        self.finish_major_collection();
        freed
    }

    pub(super) fn sweep_major_slot(&mut self, index: usize) -> bool {
        let Some(object) = self.objects[index].object.as_mut() else {
            return false;
        };
        if object.marked {
            object.marked = false;
            return false;
        }
        self.free_slot(index);
        true
    }

    fn free_slot(&mut self, index: usize) {
        let slot = self.objects[index].as_mut();
        let Some(object) = slot.object.take() else {
            return;
        };
        let reference = GcRef::from_parts(
            u32::try_from(index).expect("heap slot index exceeds u32"),
            slot.generation,
        );
        slot.flags = 0;
        let table = std::mem::take(&mut slot.card_table);
        if slot.generation != GcRef::MAX_GENERATION {
            slot.generation += 1;
            self.free_list
                .push(u32::try_from(index).expect("heap slot index exceeds u32"));
        }
        if table != 0 {
            self.card_tables[table as usize] = Vec::new();
            self.free_card_tables.push(table);
        }
        self.live_objects = self.live_objects.saturating_sub(1);
        self.bytes_allocated = self
            .bytes_allocated
            .saturating_sub(Self::freed_object_size(&object));
        if let ObjectKind::String(string) = &object.kind
            && string.is_interned()
        {
            let key = Self::fnv1a_hash(string.as_bytes());
            if self.intern_table.get(&key) == Some(&reference) {
                self.intern_table.remove(&key);
            }
        }
    }

    pub(super) fn finish_major_collection(&mut self) {
        self.phase = MajorPhase::Idle;
        self.sweep_cursor = 0;
        self.sweep_freed = 0;
        self.next_gc = (self.bytes_allocated * Self::GC_GROWTH_FACTOR)
            .max(Self::INITIAL_GC_THRESHOLD)
            .min(self.max_heap / 2);
        self.major_collections = self.major_collections.saturating_add(1);
        self.slice_at = u64::MAX;
        self.update_next_event();
    }

    fn freed_object_size(obj: &crate::object::GcObject) -> usize {
        match &obj.kind {
            ObjectKind::String(s) => std::mem::size_of::<AelysString>() + s.released_charge(),
            _ => Self::estimate_object_size(obj),
        }
    }

    pub fn estimate_object_size(obj: &crate::object::GcObject) -> usize {
        match &obj.kind {
            ObjectKind::String(s) => std::mem::size_of::<AelysString>() + s.pending_charge(),
            ObjectKind::Function(f) => Self::estimate_function_size(&f.function),
            ObjectKind::Native(_) => std::mem::size_of::<crate::object::NativeFunction>(),
            ObjectKind::Upvalue(_) => std::mem::size_of::<AelysUpvalue>(),
            ObjectKind::Closure(c) => std::mem::size_of::<AelysClosure>() + c.upvalues.len() * 8,
            ObjectKind::Array(a) => a.size_bytes(),
            ObjectKind::Vec(v) => v.size_bytes(),
            ObjectKind::Map(map) => map.accounted_bytes(),
            ObjectKind::Range(_) => std::mem::size_of::<crate::object::AelysRange>(),
            ObjectKind::Sum(_) => std::mem::size_of::<crate::object::AelysSum>(),
            ObjectKind::Enum(value) => value.size_bytes(),
            ObjectKind::Struct(structure) => structure.size_bytes(),
            ObjectKind::Int(_) => std::mem::size_of::<crate::object::BoxedInt>(),
        }
    }

    pub fn estimate_function_size(function: &crate::Function) -> usize {
        std::mem::size_of::<crate::object::AelysFunction>()
            .saturating_add(function.bytecode.len().saturating_mul(4))
            .saturating_add(
                function
                    .constants
                    .len()
                    .saturating_mul(std::mem::size_of::<Value>()),
            )
            .saturating_add(
                function
                    .struct_schemas
                    .iter()
                    .map(|schema| {
                        std::mem::size_of_val(schema)
                            .saturating_add(def_id_size(&schema.ctor))
                            .saturating_add(
                                schema.type_args.iter().map(descriptor_size).sum::<usize>(),
                            )
                            .saturating_add(
                                schema
                                    .fields
                                    .iter()
                                    .map(|field| {
                                        std::mem::size_of_val(field)
                                            .saturating_add(field.name.len())
                                            .saturating_add(descriptor_size(&field.ty))
                                    })
                                    .sum::<usize>(),
                            )
                    })
                    .sum::<usize>(),
            )
            .saturating_add(
                function
                    .enum_schemas
                    .iter()
                    .map(|schema| {
                        std::mem::size_of_val(schema)
                            .saturating_add(schema.name.len())
                            .saturating_add(
                                schema
                                    .variants
                                    .iter()
                                    .map(|variant| {
                                        std::mem::size_of_val(variant)
                                            .saturating_add(variant.name.len())
                                            .saturating_add(
                                                variant
                                                    .fields
                                                    .iter()
                                                    .map(|field| {
                                                        std::mem::size_of_val(field)
                                                            .saturating_add(
                                                                field
                                                                    .name
                                                                    .as_ref()
                                                                    .map_or(0, String::len),
                                                            )
                                                            .saturating_add(descriptor_size(
                                                                &field.ty,
                                                            ))
                                                    })
                                                    .sum::<usize>(),
                                            )
                                    })
                                    .sum::<usize>(),
                            )
                    })
                    .sum::<usize>(),
            )
    }
}

fn descriptor_size(descriptor: &crate::TypeDescriptor) -> usize {
    use crate::TypeDescriptor;
    std::mem::size_of_val(descriptor)
        + match descriptor {
            TypeDescriptor::Option(inner)
            | TypeDescriptor::Array(inner)
            | TypeDescriptor::Vec(inner) => descriptor_size(inner),
            TypeDescriptor::Result(ok, err) | TypeDescriptor::Map(ok, err) => {
                descriptor_size(ok) + descriptor_size(err)
            }
            TypeDescriptor::FixedArray(inner, _) => descriptor_size(inner),
            _ => 0,
        }
}

fn def_id_size(def_id: &crate::DefId) -> usize {
    def_id
        .package
        .len()
        .saturating_add(def_id.module.iter().map(String::len).sum::<usize>())
}
