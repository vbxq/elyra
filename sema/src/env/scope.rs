use super::TypeEnv;
use crate::types::InferType;

impl TypeEnv {
    /// Enter a new scope
    pub fn push_scope(&mut self) {
        self.locals.push(std::collections::HashMap::new());
        self.explicit_dynamic_locals
            .push(std::collections::HashSet::new());
    }

    /// Exit the current scope
    pub fn pop_scope(&mut self) {
        if self.locals.len() > 1 {
            self.locals.pop();
            self.explicit_dynamic_locals.pop();
        }
    }

    /// Define a local variable in the current scope
    pub fn define_local(&mut self, name: String, ty: InferType) {
        self.define_local_with_dynamic_origin(name, ty, false);
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

    /// Look up a variable (searches from innermost to outermost scope)
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

    /// Check if a variable exists
    pub fn contains(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    /// Current scope depth
    pub fn depth(&self) -> usize {
        self.locals.len()
    }
}
