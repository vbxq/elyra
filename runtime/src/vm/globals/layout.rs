use super::super::core::PreparedGlobals;
use super::super::{GcRef, ObjectKind, VM, Value};

pub(crate) const UNNAMED_GLOBAL: u32 = u32::MAX;
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

    #[inline]
    pub(crate) fn switch_globals(&mut self, mapping_id: usize, function: GcRef) {
        if mapping_id != self.current_global_mapping_id
            && !self.install_prepared_mapping(mapping_id)
        {
            self.prepare_globals_for_function(function);
        }
    }

    #[inline]
    fn install_prepared_mapping(&mut self, mapping_id: usize) -> bool {
        let Some(Some(prepared)) = self.globals_by_index_cache.get(mapping_id) else {
            return false;
        };
        self.globals_by_index.clear();
        self.globals_by_index.extend_from_slice(&prepared.values);
        let len = prepared.values.len();
        if self.global_generations.len() != len {
            self.sync_global_generation_len(len);
        }
        self.current_global_mapping_id = mapping_id;
        true
    }

    fn store_prepared_mapping(&mut self, mapping_id: usize, prepared: PreparedGlobals) {
        if mapping_id > Self::MAX_PREPARED_MAPPING {
            return;
        }
        if self.globals_by_index_cache.len() <= mapping_id {
            self.globals_by_index_cache
                .resize_with(mapping_id + 1, || None);
        }
        for (slot, &name_id) in prepared.name_ids.iter().enumerate() {
            if name_id != UNNAMED_GLOBAL {
                self.global_users[name_id as usize].push((mapping_id, slot as u32));
            }
        }
        self.globals_by_index_cache[mapping_id] = Some(prepared);
    }

    pub(crate) fn clear_prepared_globals(&mut self) {
        self.globals_by_index_cache.clear();
        for users in &mut self.global_users {
            users.clear();
        }
    }

    fn global_name_id(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.global_name_ids.get(name) {
            return id;
        }
        let id = u32::try_from(self.global_names_by_id.len()).unwrap_or(UNNAMED_GLOBAL);
        if id == UNNAMED_GLOBAL {
            return id;
        }
        self.global_name_ids.insert(name.to_string(), id);
        self.global_names_by_id.push(name.to_string());
        self.global_users.push(Vec::new());
        id
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
        let mut name_ids = Vec::with_capacity(needed_len);
        for (idx, name) in names.iter().enumerate() {
            if !name.is_empty() {
                self.globals_by_index[idx] =
                    self.globals.get(name).copied().unwrap_or(Value::uninit());
                name_ids.push(self.global_name_id(name));
            } else {
                self.globals_by_index[idx] = Value::null();
                name_ids.push(UNNAMED_GLOBAL);
            }
        }
        self.globals_by_index.truncate(needed_len);
        self.sync_global_generation_len(needed_len);

        self.current_global_mapping_id = mapping_id;
        let prepared = PreparedGlobals {
            values: self.globals_by_index.clone(),
            name_ids: name_ids.into_boxed_slice(),
        };
        self.store_prepared_mapping(mapping_id, prepared);
        mapping_id
    }

    fn prepare_empty_global_mapping(&mut self, mapping_id: usize) -> usize {
        if mapping_id == self.current_global_mapping_id {
            return mapping_id;
        }
        self.globals_by_index.clear();
        self.current_global_mapping_id = mapping_id;
        self.store_prepared_mapping(
            mapping_id,
            PreparedGlobals {
                values: Vec::new(),
                name_ids: Box::new([]),
            },
        );
        mapping_id
    }
}
