use super::super::{GcRef, ObjectKind, VM, Value};
use std::sync::Arc;

impl VM {
    pub fn get_global_mapping_id(&self, func_ref: GcRef) -> usize {
        if let Some(obj) = self.heap.get(func_ref) {
            match &obj.kind {
                ObjectKind::Function(f) => f.function.global_layout.id(),
                ObjectKind::Closure(c) => {
                    if let Some(inner_obj) = self.heap.get(c.function) {
                        if let ObjectKind::Function(f) = &inner_obj.kind {
                            f.function.global_layout.id()
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        } else {
            0
        }
    }

    pub(crate) fn global_mapping_id_for_layout(
        &self,
        layout: &super::super::GlobalLayout,
    ) -> usize {
        layout.id()
    }

    pub fn prepare_globals_for_function(&mut self, func_ref: GcRef) -> usize {
        let mapping_id = self.get_global_mapping_id(func_ref);
        if mapping_id == self.current_global_mapping_id {
            return mapping_id;
        }
        if self.install_prepared_mapping(mapping_id) {
            return mapping_id;
        }
        let layout = if let Some(obj) = self.heap.get(func_ref) {
            match &obj.kind {
                ObjectKind::Function(function) => {
                    Some(Arc::clone(&function.function.global_layout))
                }
                ObjectKind::Closure(closure) => self.heap.get(closure.function).and_then(|inner| {
                    if let ObjectKind::Function(function) = &inner.kind {
                        Some(Arc::clone(&function.function.global_layout))
                    } else {
                        None
                    }
                }),
                _ => None,
            }
        } else {
            None
        };
        match layout {
            Some(layout) if !layout.names().is_empty() => self.prepare_globals_for_layout(&layout),
            Some(layout) => self.prepare_empty_global_mapping(layout.id()),
            None => self.prepare_empty_global_mapping(0),
        }
    }

    /// mapping ids come from a process wide counter, so an unusual number of layouts stops being cached
    const MAX_PREPARED_MAPPING: usize = 1 << 20;

    fn install_prepared_mapping(&mut self, mapping_id: usize) -> bool {
        let Some(Some(prepared)) = self.globals_by_index_cache.get(mapping_id).cloned() else {
            return false;
        };
        self.globals_by_index.clear();
        self.globals_by_index.extend_from_slice(&prepared);
        if self.global_generations.len() != prepared.len() {
            self.sync_global_generation_len(prepared.len());
        }
        self.current_global_mapping_id = mapping_id;
        true
    }

    fn store_prepared_mapping(&mut self, mapping_id: usize, prepared: Arc<Vec<Value>>) {
        if mapping_id > Self::MAX_PREPARED_MAPPING {
            return;
        }
        if self.globals_by_index_cache.len() <= mapping_id {
            self.globals_by_index_cache.resize(mapping_id + 1, None);
        }
        self.globals_by_index_cache[mapping_id] = Some(prepared);
    }

    pub fn prepare_globals_for_layout(&mut self, layout: &super::super::GlobalLayout) -> usize {
        let mapping_id = layout.id();
        if mapping_id == self.current_global_mapping_id {
            return mapping_id;
        }

        if self.install_prepared_mapping(mapping_id) {
            return mapping_id;
        }

        let names = layout.names();

        let needed_len = names.len();
        if self.globals_by_index.len() < needed_len {
            self.globals_by_index.resize(needed_len, Value::null());
        }
        for (idx, name) in names.iter().enumerate() {
            if !name.is_empty() {
                self.globals_by_index[idx] =
                    self.globals.get(name).copied().unwrap_or(Value::null());
            } else {
                self.globals_by_index[idx] = Value::null();
            }
        }
        self.globals_by_index.truncate(needed_len);
        self.sync_global_generation_len(needed_len);

        self.current_global_mapping_id = mapping_id;
        let snapshot = Arc::new(self.globals_by_index.clone());
        self.store_prepared_mapping(mapping_id, snapshot);
        mapping_id
    }

    fn prepare_empty_global_mapping(&mut self, mapping_id: usize) -> usize {
        if mapping_id == self.current_global_mapping_id {
            return mapping_id;
        }
        self.globals_by_index.clear();
        self.current_global_mapping_id = mapping_id;
        self.store_prepared_mapping(mapping_id, Arc::new(Vec::new()));
        mapping_id
    }
}
