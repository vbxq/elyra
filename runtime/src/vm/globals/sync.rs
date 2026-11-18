use super::super::VM;

impl VM {
    /// synchronize indexed globals back to the hash map.
    pub fn sync_globals_to_hashmap(&mut self, global_names: &[String]) {
        self.sync_current_function_globals();
        for (idx, name) in global_names.iter().enumerate() {
            if !name.is_empty()
                && idx < self.globals_by_index.len()
                && self
                    .global_view
                    .names
                    .get(idx)
                    .is_none_or(|merged| merged.is_empty() || merged == name)
            {
                let value = self.globals_by_index[idx];
                match self.globals.get_mut(name) {
                    Some(slot) => *slot = value,
                    None => {
                        self.globals.insert(name.clone(), value);
                    }
                }
            }
        }
    }

    /// sync the current function's globals_by_index to the globals hashmap.
    pub fn sync_current_function_globals(&mut self) {
        if !self.globals_by_index_dirty {
            return;
        }
        self.globals_by_index_dirty = false;
        let view = &mut self.global_view;
        for &index in &view.written {
            let index = index as usize;
            if let Some(flag) = view.written_flags.get_mut(index) {
                *flag = false;
            }
            let (Some(name), Some(value)) =
                (view.names.get(index), self.globals_by_index.get(index))
            else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            match self.globals.get_mut(name) {
                Some(slot) => *slot = *value,
                None => {
                    self.globals.insert(name.clone(), *value);
                }
            }
        }
        view.written.clear();
    }
}
