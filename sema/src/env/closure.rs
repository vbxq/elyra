use super::TypeEnv;
use std::collections::{HashMap, HashSet};

impl TypeEnv {
    pub fn for_function(&self) -> TypeEnv {
        TypeEnv {
            locals: vec![HashMap::new()],
            borrow_bindings: vec![HashMap::new()],
            local_mutability: vec![HashMap::new()],
            local_aliases: vec![HashMap::new()],
            read_only_bindings: vec![HashSet::new()],
            known_collection_lengths: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            constant_values: vec![HashMap::new()],
            capture_constants: HashMap::new(),
            binding_origins: vec![HashMap::new()],
            declared_constants: vec![HashSet::new()],
            capture_binding_origins: HashMap::new(),
            captures: HashMap::new(),
            borrow_captures: HashMap::new(),
            capture_mutability: HashMap::new(),
            capture_aliases: HashMap::new(),
            explicit_dynamic_captures: HashSet::new(),
            functions: self.functions.clone(),
            current_function: None,
            namespaces: self.namespaces.clone(),
            mutable_imports: self.mutable_imports.clone(),
        }
    }

    pub fn for_closure(&self) -> TypeEnv {
        let mut all_visible = HashMap::new();
        let mut all_borrowed = HashMap::new();
        let mut explicit_dynamic = HashSet::new();
        let mut capture_mutability = HashMap::new();
        let mut known_collection_lengths = HashMap::new();
        let mut capture_aliases = HashMap::new();
        let mut capture_constants = self.capture_constants.clone();

        for (scope, origins) in self.locals.iter().zip(&self.explicit_dynamic_locals) {
            for (name, ty) in scope {
                if origins.contains(name) {
                    explicit_dynamic.insert(name.clone());
                }
                all_visible.insert(name.clone(), ty.clone());
            }
        }

        for borrowed in &self.borrow_bindings {
            all_borrowed.extend(borrowed.iter().map(|(name, kind)| (name.clone(), *kind)));
        }

        for (scope, mutability) in self.locals.iter().zip(&self.local_mutability) {
            for name in scope.keys() {
                capture_mutability
                    .insert(name.clone(), mutability.get(name).copied().unwrap_or(false));
            }
        }

        for (scope, aliases) in self.locals.iter().zip(&self.local_aliases) {
            for name in scope.keys() {
                match aliases.get(name) {
                    Some(alias) => capture_aliases.insert(name.clone(), alias.clone()),
                    None => capture_aliases.remove(name),
                };
            }
        }

        for (scope, constants) in self.locals.iter().zip(&self.constant_values) {
            for name in scope.keys() {
                match constants.get(name) {
                    Some(value) => capture_constants.insert(name.clone(), value.clone()),
                    None => capture_constants.remove(name),
                };
            }
        }

        let mut capture_binding_origins = self.capture_binding_origins.clone();
        for (scope, origins) in self.locals.iter().zip(&self.binding_origins) {
            for name in scope.keys() {
                match origins.get(name) {
                    Some(origin) => capture_binding_origins.insert(name.clone(), origin.clone()),
                    None => capture_binding_origins.remove(name),
                };
            }
        }

        for lengths in &self.known_collection_lengths {
            known_collection_lengths
                .extend(lengths.iter().map(|(name, length)| (name.clone(), *length)));
        }

        for (name, ty) in &self.captures {
            all_visible.insert(name.clone(), ty.clone());
            if let Some(kind) = self.borrow_captures.get(name) {
                all_borrowed.insert(name.clone(), *kind);
            }
            capture_mutability.insert(
                name.clone(),
                self.capture_mutability.get(name).copied().unwrap_or(false),
            );
            match self.capture_aliases.get(name) {
                Some(alias) => capture_aliases.insert(name.clone(), alias.clone()),
                None => capture_aliases.remove(name),
            };
        }
        explicit_dynamic.extend(self.explicit_dynamic_captures.iter().cloned());

        TypeEnv {
            locals: vec![HashMap::new()],
            borrow_bindings: vec![HashMap::new()],
            local_mutability: vec![HashMap::new()],
            local_aliases: vec![HashMap::new()],
            read_only_bindings: vec![HashSet::new()],
            known_collection_lengths: vec![known_collection_lengths],
            explicit_dynamic_locals: vec![HashSet::new()],
            constant_values: vec![HashMap::new()],
            capture_constants,
            binding_origins: vec![HashMap::new()],
            declared_constants: vec![HashSet::new()],
            capture_binding_origins,
            captures: all_visible,
            borrow_captures: all_borrowed,
            capture_mutability,
            capture_aliases,
            explicit_dynamic_captures: explicit_dynamic,
            functions: self.functions.clone(),
            current_function: None,
            namespaces: self.namespaces.clone(),
            mutable_imports: self.mutable_imports.clone(),
        }
    }
}
