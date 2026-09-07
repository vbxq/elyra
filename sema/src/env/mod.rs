mod captures;
mod closure;
mod free_vars;
mod functions;
mod scope;

use crate::types::InferType;
use aelys_syntax::ReferenceKind;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    locals: Vec<HashMap<String, InferType>>,

    borrow_bindings: Vec<HashMap<String, ReferenceKind>>,

    local_mutability: Vec<HashMap<String, bool>>,

    local_aliases: Vec<HashMap<String, String>>,

    read_only_bindings: Vec<HashSet<String>>,

    known_collection_lengths: Vec<HashMap<String, usize>>,

    explicit_dynamic_locals: Vec<HashSet<String>>,

    captures: HashMap<String, InferType>,

    borrow_captures: HashMap<String, ReferenceKind>,

    capture_mutability: HashMap<String, bool>,

    capture_aliases: HashMap<String, String>,

    explicit_dynamic_captures: HashSet<String>,

    functions: HashMap<String, Rc<InferType>>,

    current_function: Option<String>,

    // a namespace name is legal only as the root of a path, so it never gets a type
    namespaces: HashSet<String>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            locals: vec![HashMap::new()],
            borrow_bindings: vec![HashMap::new()],
            local_mutability: vec![HashMap::new()],
            local_aliases: vec![HashMap::new()],
            read_only_bindings: vec![HashSet::new()],
            known_collection_lengths: vec![HashMap::new()],
            explicit_dynamic_locals: vec![HashSet::new()],
            captures: HashMap::new(),
            borrow_captures: HashMap::new(),
            capture_mutability: HashMap::new(),
            capture_aliases: HashMap::new(),
            explicit_dynamic_captures: HashSet::new(),
            functions: HashMap::new(),
            current_function: None,
            namespaces: HashSet::new(),
        }
    }

    // a namespace carries no type because it is never a value, so it lives outside the type maps
    pub fn define_namespace(&mut self, name: String) {
        self.namespaces.insert(name);
    }

    pub fn is_namespace(&self, name: &str) -> bool {
        self.namespaces.contains(name)
    }
}
