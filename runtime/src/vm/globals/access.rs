use super::super::{GcRef, HostRoot, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn pin_host_ref(&self, reference: GcRef) -> HostRoot {
        HostRoot::new(&self.host_roots, reference, self.id)
    }

    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).copied()
    }

    pub fn set_global(&mut self, name: String, value: Value) {
        self.globals.insert(name, value);
        self.invalidate_global_mapping();
    }

    pub fn remove_global(&mut self, name: &str) -> Option<Value> {
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

    pub fn global_names(&self) -> Vec<String> {
        self.globals.keys().cloned().collect()
    }

    pub fn set_global_by_index(&mut self, idx: usize, value: Value) {
        if idx >= self.globals_by_index.len() {
            self.globals_by_index.resize(idx + 1, Value::null());
        }
        if idx >= self.global_generations.len() {
            self.global_generations.resize(idx + 1, 0);
        }
        self.globals_by_index[idx] = value;
        self.global_generations[idx] = self.global_generations[idx].wrapping_add(1);
        self.globals_by_index_cache.clear();
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
        self.globals_by_index.clear();
        self.globals_by_index_cache.clear();
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
