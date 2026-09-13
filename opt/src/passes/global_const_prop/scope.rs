use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ShadowStack {
    // an empty stack is the top level, where the collected globals themselves are bound,
    scopes: Vec<HashSet<String>>,
}

impl ShadowStack {
    pub(super) fn push(&mut self) {
        self.scopes.push(HashSet::new());
    }

    pub(super) fn pop(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn clear(&mut self) {
        self.scopes.clear();
    }

    pub(super) fn shadow(&mut self, name: String) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name);
        }
    }

    pub(super) fn is_shadowed(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }
}
