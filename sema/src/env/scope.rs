use super::TypeEnv;
use crate::types::InferType;
use aelys_syntax::ReferenceKind;

impl TypeEnv {
    pub fn push_scope(&mut self) {
        self.locals.push(std::collections::HashMap::new());
        self.borrow_bindings.push(std::collections::HashMap::new());
        self.local_mutability.push(std::collections::HashMap::new());
        self.read_only_bindings
            .push(std::collections::HashSet::new());
        self.known_collection_lengths
            .push(std::collections::HashMap::new());
        self.explicit_dynamic_locals
            .push(std::collections::HashSet::new());
    }

    pub fn pop_scope(&mut self) {
        if self.locals.len() > 1 {
            self.locals.pop();
            self.borrow_bindings.pop();
            self.local_mutability.pop();
            self.read_only_bindings.pop();
            self.known_collection_lengths.pop();
            self.explicit_dynamic_locals.pop();
        }
    }

    pub fn define_local(&mut self, name: String, ty: InferType) {
        self.define_local_with_dynamic_origin(name, ty, false);
    }

    pub fn set_mutable(&mut self, name: &str, mutable: bool) {
        for scope in self.locals.iter().rev() {
            if scope.contains_key(name) {
                if let Some(mutability) = self
                    .local_mutability
                    .iter_mut()
                    .rev()
                    .find(|scope| scope.contains_key(name))
                {
                    mutability.insert(name.to_string(), mutable);
                }
                return;
            }
        }
        if self.captures.contains_key(name)
            && let Some(value) = self.capture_mutability.get_mut(name)
        {
            *value = mutable;
        }
    }

    pub fn is_mutable(&self, name: &str) -> bool {
        for (scope, mutability) in self
            .locals
            .iter()
            .rev()
            .zip(self.local_mutability.iter().rev())
        {
            if scope.contains_key(name) {
                return mutability.get(name).copied().unwrap_or(false);
            }
        }
        self.capture_mutability.get(name).copied().unwrap_or(false)
    }

    pub fn define_borrow_binding(&mut self, name: String, kind: ReferenceKind) {
        if let Some(scope) = self.borrow_bindings.last_mut() {
            scope.insert(name, kind);
        }
    }

    pub fn borrow_kind(&self, name: &str) -> Option<ReferenceKind> {
        self.borrow_bindings
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .or_else(|| self.borrow_captures.get(name).copied())
    }

    pub fn define_local_with_collection_length(
        &mut self,
        name: String,
        ty: InferType,
        length: usize,
    ) {
        self.define_local(name.clone(), ty);
        if let Some(scope) = self.known_collection_lengths.last_mut() {
            scope.insert(name, length);
        }
    }

    pub fn collection_length(&self, name: &str) -> Option<usize> {
        self.known_collection_lengths
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub fn invalidate_collection_length(&mut self, name: &str) {
        for scope in self.known_collection_lengths.iter_mut().rev() {
            if scope.remove(name).is_some() {
                break;
            }
        }
    }

    pub fn mark_read_only(&mut self, name: &str) {
        if let Some(scope) = self.read_only_bindings.last_mut() {
            scope.insert(name.to_string());
        }
    }

    pub fn is_read_only(&self, name: &str) -> bool {
        self.read_only_bindings
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    pub fn define_explicit_dynamic_local(&mut self, name: String, ty: InferType) {
        self.define_local_with_dynamic_origin(name, ty, true);
    }

    fn define_local_with_dynamic_origin(
        &mut self,
        name: String,
        ty: InferType,
        explicit_dynamic: bool,
    ) {
        if let Some(scope) = self.locals.last_mut() {
            scope.insert(name.clone(), ty);
            if let Some(mutability) = self.local_mutability.last_mut() {
                mutability.insert(name.clone(), false);
            }
            if let Some(lengths) = self.known_collection_lengths.last_mut() {
                lengths.remove(&name);
            }
            if let Some(origins) = self.explicit_dynamic_locals.last_mut() {
                if explicit_dynamic {
                    origins.insert(name);
                } else {
                    origins.remove(&name);
                }
            }
        }
    }

    pub fn is_explicit_dynamic(&self, name: &str) -> bool {
        for (scope, origins) in self
            .locals
            .iter()
            .rev()
            .zip(self.explicit_dynamic_locals.iter().rev())
        {
            if scope.contains_key(name) {
                return origins.contains(name);
            }
        }
        self.captures.contains_key(name) && self.explicit_dynamic_captures.contains(name)
    }

    pub fn lookup(&self, name: &str) -> Option<&InferType> {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }

        if let Some(ty) = self.captures.get(name) {
            return Some(ty);
        }

        if let Some(ty) = self.functions.get(name) {
            return Some(ty.as_ref());
        }

        None
    }

    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    pub fn depth(&self) -> usize {
        self.locals.len()
    }
}
