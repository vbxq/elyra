use super::super::{GcRef, HostRoot, VM, Value};
use super::layout::UNNAMED_GLOBAL;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn pin_host_ref(&self, reference: GcRef) -> HostRoot {
        HostRoot::new(&self.host_roots, reference, self.id)
    }

    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.named_global(name)
    }

    pub(crate) fn named_global(&self, name: &str) -> Option<Value> {
        if !self.global_stale.is_empty()
            && let Some(&id) = self.global_name_ids.get(name)
            && let Some(Some(value)) = self.global_latest.get(id as usize)
        {
            return Some(*value);
        }
        self.globals.get(name).copied()
    }

    pub(crate) fn flush_stale_globals(&mut self) {
        if self.global_stale.is_empty() {
            return;
        }
        let mut stale = std::mem::take(&mut self.global_stale);
        for id in stale.drain(..) {
            let id = id as usize;
            let Some(value) = self.global_latest[id].take() else {
                continue;
            };
            let name = &self.global_names_by_id[id];
            match self.globals.get_mut(name) {
                Some(existing) => *existing = value,
                None => {
                    self.globals.insert(name.clone(), value);
                }
            }
        }
        self.global_stale = stale;
    }

    pub(crate) fn stale_global_values(&self) -> impl Iterator<Item = Value> + '_ {
        self.global_stale
            .iter()
            .filter_map(|&id| self.global_latest[id as usize])
    }

    pub fn set_global(&mut self, name: String, value: Value) {
        self.flush_stale_globals();
        self.bump_jit_global_epoch();
        self.globals.insert(name, value);
        self.invalidate_global_mapping();
    }

    pub fn remove_global(&mut self, name: &str) -> Option<Value> {
        self.flush_stale_globals();
        self.bump_jit_global_epoch();
        let value = self.globals.remove(name);
        self.invalidate_global_mapping();
        value
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn global_index_error(&self, operation: &str, index: usize) -> RuntimeError {
        let mapped = self.globals_by_index.len();
        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "{operation} expected a global index below the mapped {mapped}, found {index}"
        )))
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn global_read_error(&self, operation: &str, index: usize) -> RuntimeError {
        if index < self.globals_by_index.len() {
            self.uninitialized_global_error(index)
        } else {
            self.global_index_error(operation, index)
        }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn uninitialized_global_error(&self, index: usize) -> RuntimeError {
        let name = self
            .frames
            .last()
            .and_then(|frame| self.global_layout_of(frame.function()))
            .and_then(|layout| layout.names().get(index).cloned())
            .unwrap_or_else(|| format!("#{index}"));
        self.runtime_error(RuntimeErrorKind::UninitializedGlobal(name))
    }

    fn global_layout_of(
        &self,
        function: GcRef,
    ) -> Option<std::sync::Arc<super::super::GlobalLayout>> {
        match &self.heap.get(function)?.kind {
            super::super::ObjectKind::Function(function) => {
                Some(std::sync::Arc::clone(&function.function.global_layout))
            }
            super::super::ObjectKind::Closure(closure) => {
                match &self.heap.get(closure.function)?.kind {
                    super::super::ObjectKind::Function(function) => {
                        Some(std::sync::Arc::clone(&function.function.global_layout))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn global_names(&self) -> Vec<String> {
        self.globals.keys().cloned().collect()
    }

    // the indexed write through the current prepared view; false when it has none or the slot lies outside it, before any effect
    #[inline(always)]
    pub(crate) fn store_global_in_view(&mut self, idx: usize, value: Value) -> bool {
        let mapping = self.current_global_mapping_id;
        let Some(Some(view)) = self.globals_by_index_cache.get_mut(mapping) else {
            return false;
        };
        let Some(&name_id) = view.name_ids.get(idx) else {
            return false;
        };
        let (Some(slot), Some(generation)) = (
            self.globals_by_index.get_mut(idx),
            self.global_generations.get_mut(idx),
        ) else {
            return false;
        };
        *slot = value;
        *generation = generation.wrapping_add(1);
        if name_id == UNNAMED_GLOBAL {
            view.values[idx] = value;
        } else {
            self.write_named_global(name_id, value);
        }
        self.bump_jit_global_epoch();
        true
    }

    pub fn set_global_by_index(&mut self, idx: usize, value: Value) {
        if self.store_global_in_view(idx, value) {
            return;
        }
        if idx < self.globals_by_index.len() {
            self.register_current_view();
            if self.store_global_in_view(idx, value) {
                return;
            }
        }
        if idx >= self.globals_by_index.len() {
            self.globals_by_index.resize(idx + 1, Value::null());
        }
        if idx >= self.global_generations.len() {
            self.global_generations.resize(idx + 1, 0);
        }
        self.globals_by_index[idx] = value;
        self.global_generations[idx] = self.global_generations[idx].wrapping_add(1);
        self.bump_jit_global_epoch();
        let mapping = self.current_global_mapping_id;
        let name_id = self
            .globals_by_index_cache
            .get(mapping)
            .and_then(Option::as_ref)
            .and_then(|view| view.name_ids.get(idx).copied());
        match name_id {
            Some(UNNAMED_GLOBAL) => {
                if let Some(Some(view)) = self.globals_by_index_cache.get_mut(mapping) {
                    view.values[idx] = value;
                }
            }
            Some(name_id) => self.write_named_global(name_id, value),
            None => {
                self.clear_prepared_globals();
                self.globals_by_index_dirty = true;
            }
        }
    }

    // the interpreter's SetGlobalIdx for a function whose view may not be the current one; globals are roots, so no barrier
    pub(crate) fn frame_store_global(
        &mut self,
        mapping: usize,
        index: usize,
        value: Value,
    ) -> bool {
        let Some(Some(view)) = self.globals_by_index_cache.get(mapping) else {
            return false;
        };
        let Some(&name_id) = view.name_ids.get(index) else {
            return false;
        };
        self.bump_jit_global_epoch();
        if name_id == UNNAMED_GLOBAL {
            if let Some(Some(view)) = self.globals_by_index_cache.get_mut(mapping) {
                view.values[index] = value;
            }
            if mapping == self.current_global_mapping_id
                && let Some(slot) = self.globals_by_index.get_mut(index)
            {
                *slot = value;
            }
        } else {
            self.write_named_global(name_id, value);
        }
        if mapping == self.current_global_mapping_id
            && let Some(generation) = self.global_generations.get_mut(index)
        {
            *generation = generation.wrapping_add(1);
        }
        true
    }

    fn write_named_global(&mut self, name_id: u32, value: Value) {
        let current = self.current_global_mapping_id;
        let name_id = name_id as usize;
        for &(mapping, slot) in &self.global_users[name_id] {
            let slot = slot as usize;
            if let Some(Some(view)) = self.globals_by_index_cache.get_mut(mapping) {
                view.values[slot] = value;
            }
            if mapping == current
                && let Some(current_slot) = self.globals_by_index.get_mut(slot)
            {
                *current_slot = value;
            }
        }
        if self.global_latest[name_id].replace(value).is_none() {
            self.global_stale.push(name_id as u32);
            let name = &self.global_names_by_id[name_id];
            match self.globals.get_mut(name) {
                Some(existing) => *existing = value,
                None => {
                    self.globals.insert(name.clone(), value);
                }
            }
        }
    }

    pub(crate) fn set_global_by_index_checked(
        &mut self,
        idx: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        if idx >= self.globals_by_index.len() {
            const SLOT_BYTES: u64 =
                (std::mem::size_of::<Value>() + std::mem::size_of::<u64>()) as u64;
            let projected = u64::try_from(idx)
                .ok()
                .and_then(|idx| idx.checked_add(1))
                .and_then(|slots| slots.checked_mul(SLOT_BYTES))
                .unwrap_or(u64::MAX);
            self.ensure_heap_capacity(projected)?;
        }
        self.set_global_by_index(idx, value);
        Ok(())
    }

    pub(crate) fn sync_global_generation_len(&mut self, len: usize) {
        if self.global_generations.len() < len {
            self.global_generations.resize(len, 0);
        }
        self.global_generations.truncate(len);
    }

    pub(crate) fn bump_global_generations(&mut self, len: usize) {
        if self.global_generations.len() < len {
            self.global_generations.resize(len, 0);
        }
        for generation in &mut self.global_generations[..len] {
            *generation = generation.wrapping_add(1);
        }
        self.global_generations.truncate(len);
    }

    pub fn invalidate_global_mapping(&mut self) {
        self.bump_jit_global_epoch();
        self.globals_by_index.clear();
        self.clear_prepared_globals();
        self.current_global_mapping_id = usize::MAX;
    }

    pub fn global_mutability(&self) -> &std::collections::HashMap<String, bool> {
        &self.global_mutability
    }

    pub fn update_global_mutability(
        &mut self,
        new_globals: std::collections::HashMap<String, bool>,
    ) {
        self.global_mutability.extend(new_globals);
    }
}
