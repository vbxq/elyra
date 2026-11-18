use super::state::{Compiler, Local, Upvalue};
use aelys_bytecode::Function;
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

impl Compiler {
    pub fn new(name: Option<String>, source: Arc<Source>) -> Self {
        Self {
            current: sourced_function(name, &source),
            source,
            scopes: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            enclosing_locals: None,
            enclosing_upvalues: None,
            all_enclosing_locals: Vec::new(),
            loop_stack: Vec::new(),
            loop_variables: Vec::new(),
            hoisted_literals: Vec::new(),
            scope_depth: 0,
            next_register: 0,
            register_pool: vec![false; 65_536].into_boxed_slice(),
            register_search_start: 0,
            call_window_fault: None,
            globals: HashMap::new(),
            global_indices: HashMap::new(),
            next_global_index: 0,
            module_aliases: Arc::new(HashSet::new()),
            known_globals: Arc::new(HashSet::new()),
            known_native_globals: Arc::new(HashSet::new()),
            symbol_origins: Arc::new(HashMap::new()),
            accessed_globals: HashSet::new(),
            function_depth: 0,
            current_return_type: None,
            struct_schemas: Arc::new(Vec::new()),
            enum_schemas: Arc::new(Vec::new()),
            module_sources: Arc::new(HashMap::new()),
            module_scopes: Arc::default(),
            own_slots: Arc::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn for_nested_function(
        name: Option<String>,
        source: Arc<Source>,
        globals: HashMap<String, bool>,
        global_indices: HashMap<String, u32>,
        next_global_index: u32,
        enclosing_locals: Vec<Local>,
        enclosing_upvalues: Vec<Upvalue>,
        parent_all_enclosing_locals: Vec<Vec<Local>>,
        module_aliases: Arc<HashSet<String>>,
        known_globals: Arc<HashSet<String>>,
        known_native_globals: Arc<HashSet<String>>,
        symbol_origins: Arc<HashMap<String, String>>,
    ) -> Self {
        let mut all_enclosing_locals = vec![enclosing_locals.clone()];
        all_enclosing_locals.extend(parent_all_enclosing_locals);

        Self {
            current: sourced_function(name, &source),
            source,
            scopes: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            enclosing_locals: Some(enclosing_locals),
            enclosing_upvalues: Some(enclosing_upvalues),
            all_enclosing_locals,
            loop_stack: Vec::new(),
            loop_variables: Vec::new(),
            hoisted_literals: Vec::new(),
            scope_depth: 0,
            next_register: 0,
            register_pool: vec![false; 65_536].into_boxed_slice(),
            register_search_start: 0,
            call_window_fault: None,
            globals,
            global_indices,
            next_global_index,
            module_aliases,
            known_globals,
            known_native_globals,
            symbol_origins,
            accessed_globals: HashSet::new(),
            function_depth: 1,
            current_return_type: None,
            struct_schemas: Arc::new(Vec::new()),
            enum_schemas: Arc::new(Vec::new()),
            module_sources: Arc::new(HashMap::new()),
            module_scopes: Arc::default(),
            own_slots: Arc::default(),
        }
    }

    pub fn with_modules(
        name: Option<String>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
        symbol_origins: HashMap<String, String>,
    ) -> Self {
        Self {
            current: sourced_function(name, &source),
            source,
            scopes: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            enclosing_locals: None,
            enclosing_upvalues: None,
            all_enclosing_locals: Vec::new(),
            loop_stack: Vec::new(),
            loop_variables: Vec::new(),
            hoisted_literals: Vec::new(),
            scope_depth: 0,
            next_register: 0,
            register_pool: vec![false; 65_536].into_boxed_slice(),
            register_search_start: 0,
            call_window_fault: None,
            globals: HashMap::new(),
            global_indices: HashMap::new(),
            next_global_index: 0,
            module_aliases: Arc::new(module_aliases),
            known_globals: Arc::new(known_globals),
            known_native_globals: Arc::new(known_native_globals),
            symbol_origins: Arc::new(symbol_origins),
            accessed_globals: HashSet::new(),
            function_depth: 0,
            current_return_type: None,
            struct_schemas: Arc::new(Vec::new()),
            enum_schemas: Arc::new(Vec::new()),
            module_sources: Arc::new(HashMap::new()),
            module_scopes: Arc::default(),
            own_slots: Arc::default(),
        }
    }

    pub fn with_modules_and_globals(
        name: Option<String>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
        symbol_origins: HashMap<String, String>,
        globals: HashMap<String, bool>,
    ) -> Self {
        Self {
            current: sourced_function(name, &source),
            source,
            scopes: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            enclosing_locals: None,
            enclosing_upvalues: None,
            all_enclosing_locals: Vec::new(),
            loop_stack: Vec::new(),
            loop_variables: Vec::new(),
            hoisted_literals: Vec::new(),
            scope_depth: 0,
            next_register: 0,
            register_pool: vec![false; 65_536].into_boxed_slice(),
            register_search_start: 0,
            call_window_fault: None,
            globals,
            global_indices: HashMap::new(),
            next_global_index: 0,
            module_aliases: Arc::new(module_aliases),
            known_globals: Arc::new(known_globals),
            known_native_globals: Arc::new(known_native_globals),
            symbol_origins: Arc::new(symbol_origins),
            accessed_globals: HashSet::new(),
            function_depth: 0,
            current_return_type: None,
            struct_schemas: Arc::new(Vec::new()),
            enum_schemas: Arc::new(Vec::new()),
            module_sources: Arc::new(HashMap::new()),
            module_scopes: Arc::default(),
            own_slots: Arc::default(),
        }
    }

    pub fn with_own_slots(mut self, slots: HashMap<String, String>) -> Self {
        self.own_slots = Arc::new(slots);
        self
    }

    pub fn with_module_sources(mut self, sources: HashMap<String, Arc<Source>>) -> Self {
        self.module_sources = Arc::new(sources);
        self
    }

    pub fn with_module_scopes(
        mut self,
        scopes: std::collections::BTreeMap<String, aelys_sema::infer::imports::ModuleScope>,
    ) -> Self {
        self.module_scopes = Arc::new(scopes);
        self
    }
}

fn sourced_function(name: Option<String>, source: &Arc<Source>) -> Function {
    let mut function = Function::new(name, 0);
    function.source = Some(Arc::clone(source));
    function
}
