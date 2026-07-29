use super::super::Compiler;
use std::collections::HashSet;

impl Compiler {
    pub fn free_dead_locals(
        &mut self,
        stmt_idx: usize,
        liveness: &super::super::liveness::LivenessAnalysis,
        already_freed: &mut HashSet<String>,
    ) -> usize {
        let mut freed = 0;
        for local in &mut self.locals {
            if local.is_freed || already_freed.contains(&local.name) {
                continue;
            }

            if local.is_captured {
                continue;
            }

            if liveness.is_dead_after(&local.name, stmt_idx) {
                let index = usize::from(local.register);
                self.register_pool[index] = false;
                self.register_search_start = self.register_search_start.min(index);
                local.is_freed = true;
                already_freed.insert(local.name.clone());
                freed += 1;
            }
        }
        freed
    }
}
