use super::super::{GcRef, HostRoot, VM, Value};

impl VM {
    /// Return a stable identity for this VM. Embedding handles use it to
    /// reject a callable resolved from a different heap.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Pin a heap reference held by host code until the returned token drops.
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

    pub(crate) fn bump_global_generations(&mut self, len: usize) {
        if self.global_generations.len() < len {
            self.global_generations.resize(len, 0);
        }
        for generation in &mut self.global_generations[..len] {
            *generation = generation.wrapping_add(1);
        }
        self.global_generations.truncate(len);
    }

    /// Invalidate the indexed view after host-side global mutation. The
    /// sentinel keeps even the canonical empty layout from being mistaken for
    /// an already-prepared mapping on the next JIT or call entry.
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
