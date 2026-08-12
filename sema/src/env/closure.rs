use super::TypeEnv;
use std::collections::{HashMap, HashSet};

impl TypeEnv {
    /// Clone with fresh captures (for entering a new function)
    pub fn for_function(&self) -> TypeEnv {
        TypeEnv {
            locals: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: HashMap::new(),
            explicit_dynamic_captures: HashSet::new(),
            functions: self.functions.clone(),
            current_function: None,
        }
    }

    /// Clone with inherited captures (for closures)
    pub fn for_closure(&self) -> TypeEnv {
        let mut all_visible = HashMap::new();
        let mut explicit_dynamic = HashSet::new();

        for (scope, origins) in self.locals.iter().zip(&self.explicit_dynamic_locals) {
            for (name, ty) in scope {
                if origins.contains(name) {
                    explicit_dynamic.insert(name.clone());
                }
                all_visible.insert(name.clone(), ty.clone());
            }
        }

        for (name, ty) in &self.captures {
            all_visible.insert(name.clone(), ty.clone());
        }
        explicit_dynamic.extend(self.explicit_dynamic_captures.iter().cloned());

        TypeEnv {
            locals: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: all_visible,
            explicit_dynamic_captures: explicit_dynamic,
            functions: self.functions.clone(),
            current_function: None,
        }
    }
}
