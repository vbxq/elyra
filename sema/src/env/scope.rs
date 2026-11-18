use super::TypeEnv;
use crate::types::InferType;
use aelys_syntax::ReferenceKind;

impl TypeEnv {
    pub fn push_scope(&mut self) {
        self.locals.push(std::collections::HashMap::new());
        self.borrow_bindings.push(std::collections::HashMap::new());
        self.local_mutability.push(std::collections::HashMap::new());
        self.local_aliases.push(std::collections::HashMap::new());
        self.read_only_bindings
            .push(std::collections::HashSet::new());
        self.known_collection_lengths
            .push(std::collections::HashMap::new());
        self.explicit_dynamic_locals
            .push(std::collections::HashSet::new());
        self.constant_values.push(std::collections::HashMap::new());
        self.function_value_params
            .push(std::collections::HashMap::new());
        self.binding_origins.push(std::collections::HashMap::new());
        self.declared_constants
            .push(std::collections::HashSet::new());
    }

    pub fn pop_scope(&mut self) {
        if self.locals.len() > 1 {
            self.locals.pop();
            self.borrow_bindings.pop();
            self.local_mutability.pop();
            self.local_aliases.pop();
            self.read_only_bindings.pop();
            self.known_collection_lengths.pop();
            self.explicit_dynamic_locals.pop();
            self.constant_values.pop();
            self.function_value_params.pop();
            self.binding_origins.pop();
            self.declared_constants.pop();
        }
    }

    pub fn define_local(&mut self, name: String, ty: InferType) {
        self.define_local_with_dynamic_origin(name, ty, false);
    }

    pub fn define_local_alias(&mut self, name: String, alias: String) {
        if let Some(scope) = self.local_aliases.last_mut() {
            scope.insert(name, alias);
        }
    }

    // the innermost binding wins, so an alias is reported only when that binding is the
    pub fn lookup_alias(&self, name: &str) -> Option<&str> {
        for (scope, aliases) in self
            .locals
            .iter()
            .rev()
            .zip(self.local_aliases.iter().rev())
        {
            if scope.contains_key(name) {
                return aliases.get(name).map(String::as_str);
            }
        }
        if self.captures.contains_key(name) {
            return self.capture_aliases.get(name).map(String::as_str);
        }
        None
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
        if let Some(mutable) = self.capture_mutability.get(name) {
            return *mutable;
        }
        self.mutable_imports.contains(name) && self.functions.contains_key(name)
    }

    pub fn set_mutable_imports(&mut self, names: impl IntoIterator<Item = String>) {
        self.mutable_imports = names.into_iter().collect();
    }

    pub fn forget_mutable_import(&mut self, name: &str) {
        self.mutable_imports.remove(name);
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

    pub fn define_constant(&mut self, name: &str, value: crate::infer::constants::ConstValue) {
        if let Some(constants) = self.constant_values.last_mut() {
            constants.insert(name.to_string(), value);
        }
    }

    pub fn lookup_constant(&self, name: &str) -> Option<crate::infer::constants::ConstValue> {
        for (scope, constants) in self
            .locals
            .iter()
            .rev()
            .zip(self.constant_values.iter().rev())
        {
            if scope.contains_key(name) {
                return constants.get(name).cloned();
            }
        }
        if self.captures.contains_key(name) {
            return self.capture_constants.get(name).cloned();
        }
        None
    }

    pub fn define_function_value(&mut self, name: &str, mutable_params: Vec<Option<String>>) {
        if let Some(params) = self.function_value_params.last_mut() {
            params.insert(name.to_string(), mutable_params);
        }
    }

    pub fn lookup_function_value(&self, name: &str) -> Option<&Vec<Option<String>>> {
        for (scope, params) in self
            .locals
            .iter()
            .rev()
            .zip(self.function_value_params.iter().rev())
        {
            if scope.contains_key(name) {
                return params.get(name);
            }
        }
        if self.captures.contains_key(name) {
            return self.capture_function_value_params.get(name);
        }
        None
    }

    pub fn first_constant_declaration(&mut self, name: &str) -> bool {
        self.declared_constants
            .last_mut()
            .is_none_or(|declared| declared.insert(name.to_string()))
    }

    pub fn define_binding_origin(&mut self, name: &str, root: String, root_mutable: bool) {
        if let Some(origins) = self.binding_origins.last_mut() {
            origins.insert(name.to_string(), (root, root_mutable));
        }
    }

    pub fn binding_origin(&self, name: &str) -> Option<&(String, bool)> {
        for (scope, origins) in self
            .locals
            .iter()
            .rev()
            .zip(self.binding_origins.iter().rev())
        {
            if scope.contains_key(name) {
                return origins.get(name);
            }
        }
        if self.captures.contains_key(name) {
            return self.capture_binding_origins.get(name);
        }
        None
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
            if let Some(constants) = self.constant_values.last_mut() {
                constants.remove(&name);
            }
            if let Some(params) = self.function_value_params.last_mut() {
                params.remove(&name);
            }
            if let Some(origins) = self.binding_origins.last_mut() {
                origins.remove(&name);
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

    pub fn binds_value(&self, name: &str) -> bool {
        self.locals.iter().any(|scope| scope.contains_key(name)) || self.captures.contains_key(name)
    }

    pub fn is_imported_binding(&self, name: &str) -> bool {
        !self.locals.iter().any(|scope| scope.contains_key(name))
            && !self.captures.contains_key(name)
            && self.functions.contains_key(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    pub fn depth(&self) -> usize {
        self.locals.len()
    }
}
