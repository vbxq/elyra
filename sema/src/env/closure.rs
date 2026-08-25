use super::TypeEnv;
use std::collections::{HashMap, HashSet};

impl TypeEnv {
    pub fn for_function(&self) -> TypeEnv {
        TypeEnv {
            locals: vec![HashMap::new()],
            local_mutability: vec![HashMap::new()],
            read_only_bindings: vec![HashSet::new()],
            known_collection_lengths: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: HashMap::new(),
            capture_mutability: HashMap::new(),
            explicit_dynamic_captures: HashSet::new(),
            functions: self.functions.clone(),
            current_function: None,
            namespaces: self.namespaces.clone(),
        }
    }

    pub fn for_closure(&self) -> TypeEnv {
        let mut all_visible = HashMap::new();
        let mut explicit_dynamic = HashSet::new();
        let mut capture_mutability = HashMap::new();
        let mut known_collection_lengths = HashMap::new();

        for (scope, origins) in self.locals.iter().zip(&self.explicit_dynamic_locals) {
            for (name, ty) in scope {
                if origins.contains(name) {
                    explicit_dynamic.insert(name.clone());
                }
                all_visible.insert(name.clone(), ty.clone());
            }
        }

        for (scope, mutability) in self.locals.iter().zip(&self.local_mutability) {
            for name in scope.keys() {
                capture_mutability
                    .insert(name.clone(), mutability.get(name).copied().unwrap_or(false));
            }
        }

        for lengths in &self.known_collection_lengths {
            known_collection_lengths
                .extend(lengths.iter().map(|(name, length)| (name.clone(), *length)));
        }

        for (name, ty) in &self.captures {
            all_visible.insert(name.clone(), ty.clone());
            capture_mutability.insert(
                name.clone(),
                self.capture_mutability.get(name).copied().unwrap_or(false),
            );
        }
        explicit_dynamic.extend(self.explicit_dynamic_captures.iter().cloned());

        TypeEnv {
            locals: vec![HashMap::new()],
            local_mutability: vec![HashMap::new()],
            read_only_bindings: vec![HashSet::new()],
            known_collection_lengths: vec![known_collection_lengths],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: all_visible,
            capture_mutability,
            explicit_dynamic_captures: explicit_dynamic,
            functions: self.functions.clone(),
            current_function: None,
            namespaces: self.namespaces.clone(),
        }
    }
}
