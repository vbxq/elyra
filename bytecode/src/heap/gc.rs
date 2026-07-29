use super::{Heap, MajorCollection};
use crate::object::{AelysClosure, AelysString, AelysUpvalue, GcRef, ObjectKind};
use crate::value::Value;

impl Heap {
    pub fn mark(&mut self, root: GcRef) {
        let mut worklist = vec![root];

        while let Some(r) = worklist.pop() {
            worklist.extend(self.mark_one(r));
        }
    }

    pub(super) fn mark_one(&mut self, root: GcRef) -> Vec<GcRef> {
        let should_trace = self
            .objects
            .get_mut(root.slot_index())
            .filter(|slot| slot.generation == root.generation())
            .and_then(|slot| slot.object.as_mut())
            .map(|object| {
                if object.marked {
                    false
                } else {
                    object.marked = true;
                    true
                }
            })
            .unwrap_or(false);

        if !should_trace {
            return Vec::new();
        }

        self.children_of(root)
    }

    pub fn mark_young(&mut self, roots: impl IntoIterator<Item = GcRef>) {
        let mut young_worklist = Vec::new();
        let mut old_roots = std::collections::HashSet::new();

        for root in roots {
            match self.heap_generation(root) {
                Some(super::HeapGeneration::Young) => young_worklist.push(root),
                Some(super::HeapGeneration::Old) => {
                    old_roots.insert(root);
                }
                None => {}
            }
        }
        for index in self.remembered_set.iter().copied() {
            let Some(slot) = self.objects.get(index as usize) else {
                continue;
            };
            if slot.heap_generation == super::HeapGeneration::Old && slot.object.is_some() {
                old_roots.insert(GcRef::from_parts(index, slot.generation));
            }
        }

        for root in old_roots {
            let young_children: Vec<_> = self
                .children_of(root)
                .into_iter()
                .filter(|child| self.heap_generation(*child) == Some(super::HeapGeneration::Young))
                .collect();
            if !young_children.is_empty() {
                self.remembered_set
                    .insert(u32::try_from(root.slot_index()).expect("heap slot index exceeds u32"));
            }
            young_worklist.extend(young_children);
        }

        while let Some(root) = young_worklist.pop() {
            if self.heap_generation(root) != Some(super::HeapGeneration::Young) {
                continue;
            }
            let should_trace = self
                .objects
                .get_mut(root.slot_index())
                .and_then(|slot| slot.object.as_mut())
                .map(|object| {
                    if object.marked {
                        false
                    } else {
                        object.marked = true;
                        true
                    }
                })
                .unwrap_or(false);
            if !should_trace {
                continue;
            }
            young_worklist.extend(self.children_of(root).into_iter().filter(|child| {
                self.heap_generation(*child) == Some(super::HeapGeneration::Young)
            }));
        }
    }

    fn children_of(&self, root: GcRef) -> Vec<GcRef> {
        let Some(object) = self.get(root) else {
            return Vec::new();
        };
        let mut children = Vec::new();
        match &object.kind {
            ObjectKind::Function(function) => {
                for value in &function.constants {
                    if let Some(pointer) = value.as_ptr() {
                        children.push(GcRef::new(pointer));
                    }
                }
            }
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
            ObjectKind::String(_) | ObjectKind::Native(_) => {}
            ObjectKind::Array(array) => {
                if let Some(objects) = array.data.as_objects() {
                    children.extend(
                        objects
                            .iter()
                            .filter_map(|value| value.as_ptr().map(GcRef::new)),
                    );
                }
            }
            ObjectKind::Vec(vector) => {
                if let Some(objects) = vector.objects() {
                    children.extend(
                        objects
                            .iter()
                            .filter_map(|value| value.as_ptr().map(GcRef::new)),
                    );
                }
            }
        }
        children
    }

    pub fn sweep(&mut self) -> usize {
        let mut freed = 0;
        self.major_collection = MajorCollection::Idle;
        for index in 0..self.objects.len() {
            freed += usize::from(self.sweep_slot(index, false));
        }
        self.finish_major_collection();
        freed
    }

    pub(super) fn sweep_slot(&mut self, index: usize, young_only: bool) -> bool {
        let slot = &mut self.objects[index];
        let eligible = !young_only || slot.heap_generation == super::HeapGeneration::Young;
        let should_free = eligible && slot.object.as_ref().is_some_and(|object| !object.marked);

        if let Some(object) = slot.object.as_mut() {
            if eligible && object.marked {
                slot.survival_count = slot.survival_count.saturating_add(1);
                if slot.survival_count >= 2 {
                    slot.heap_generation = super::HeapGeneration::Old;
                }
            }
            if eligible {
                object.marked = false;
            }
        }

        if !should_free {
            return false;
        }

        let object = slot
            .object
            .take()
            .expect("sweep candidate must be occupied");
        self.bytes_allocated = self
            .bytes_allocated
            .saturating_sub(Self::estimate_object_size(&object));
        if let ObjectKind::String(string) = &object.kind {
            self.intern_table.remove(&string.hash());
        }
        if slot.generation != u16::MAX {
            slot.generation += 1;
            self.free_list
                .push(u32::try_from(index).expect("heap slot index exceeds u32"));
        }
        self.remembered_set
            .remove(&u32::try_from(index).expect("heap slot index exceeds u32"));
        true
    }

    pub(super) fn finish_major_collection(&mut self) {
        self.major_collection = MajorCollection::Idle;
        self.next_gc =
            (self.bytes_allocated * Self::GC_GROWTH_FACTOR).max(Self::INITIAL_GC_THRESHOLD);
        self.major_collections = self.major_collections.saturating_add(1);
        let candidates = self
            .objects
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                slot.heap_generation == super::HeapGeneration::Old && slot.object.is_some()
            })
            .map(|(index, _)| u32::try_from(index).expect("heap slot index exceeds u32"))
            .collect();
        self.rebuild_remembered_set(candidates);
    }

    pub fn sweep_young(&mut self) -> usize {
        let mut freed = 0;
        let mut remembered_candidates = self.remembered_set.clone();
        for index in 0..self.objects.len() {
            let was_young = self.objects[index].heap_generation == super::HeapGeneration::Young;
            freed += usize::from(self.sweep_slot(index, true));
            if was_young
                && self.objects[index].heap_generation == super::HeapGeneration::Old
                && self.objects[index].object.is_some()
            {
                remembered_candidates
                    .insert(u32::try_from(index).expect("heap slot index exceeds u32"));
            }
        }

        self.next_gc =
            (self.bytes_allocated * Self::GC_GROWTH_FACTOR).max(Self::INITIAL_GC_THRESHOLD);
        self.minor_collections = self.minor_collections.saturating_add(1);
        self.rebuild_remembered_set(remembered_candidates);
        freed
    }

    fn rebuild_remembered_set(&mut self, candidates: std::collections::HashSet<u32>) {
        let remembered = candidates
            .into_iter()
            .filter_map(|index| {
                let slot = self.objects.get(index as usize)?;
                (slot.heap_generation == super::HeapGeneration::Old && slot.object.is_some())
                    .then(|| GcRef::from_parts(index, slot.generation))
            })
            .filter(|owner| {
                self.children_of(*owner)
                    .into_iter()
                    .any(|child| self.heap_generation(child) == Some(super::HeapGeneration::Young))
            })
            .map(|owner| u32::try_from(owner.slot_index()).expect("heap slot index exceeds u32"))
            .collect();
        self.remembered_set = remembered;
    }

    pub fn estimate_object_size(obj: &crate::object::GcObject) -> usize {
        match &obj.kind {
            ObjectKind::String(s) => std::mem::size_of::<AelysString>() + s.len(),
            ObjectKind::Function(f) => Self::estimate_function_size(&f.function),
            ObjectKind::Native(_) => std::mem::size_of::<crate::object::NativeFunction>(),
            ObjectKind::Upvalue(_) => std::mem::size_of::<AelysUpvalue>(),
            ObjectKind::Closure(c) => std::mem::size_of::<AelysClosure>() + c.upvalues.len() * 8,
            ObjectKind::Array(a) => a.size_bytes(),
            ObjectKind::Vec(v) => v.size_bytes(),
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
    }
}
