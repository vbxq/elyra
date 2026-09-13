use aelys_sema::TypedExpr;
use std::collections::HashMap;

pub struct ScopeStack {
    scopes: Vec<HashMap<String, Option<TypedExpr>>>,
}

impl ScopeStack {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    pub fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn insert(&mut self, name: String, expr: TypedExpr) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, Some(expr));
        }
    }

    pub fn shadow(&mut self, name: String) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, None);
        }
    }

    pub fn get(&self, name: &str) -> Option<&TypedExpr> {
        for scope in self.scopes.iter().rev() {
            if let Some(entry) = scope.get(name) {
                return entry.as_ref();
            }
        }
        None
    }

    pub fn invalidate(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(entry) = scope.get_mut(name) {
                *entry = None;
                return;
            }
        }
    }
}
