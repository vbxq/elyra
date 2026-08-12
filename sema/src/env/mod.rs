//! Type environment for inference.

mod captures;
mod closure;
mod free_vars;
mod functions;
mod scope;

use crate::types::InferType;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Type environment - maps names to types
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Local variables in current scope (name -> type)
    locals: Vec<HashMap<String, InferType>>,

    explicit_dynamic_locals: Vec<HashSet<String>>,

    /// Captured variables from enclosing scopes (upvalues)
    captures: HashMap<String, InferType>,

    explicit_dynamic_captures: HashSet<String>,

    /// Known function signatures (name -> function type)
    /// Uses Rc to avoid cloning function types during lookup
    functions: HashMap<String, Rc<InferType>>,

    /// Current function name (for recursive calls)
    current_function: Option<String>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            locals: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: HashMap::new(),
            explicit_dynamic_captures: HashSet::new(),
            functions: HashMap::new(),
            current_function: None,
        }
    }
}
