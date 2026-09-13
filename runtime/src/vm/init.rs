use super::config::VmConfig;
use super::{Heap, Value};
use super::{MAX_FRAMES, MAX_REGISTERS, VM};
use aelys_common::error::RuntimeError;
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_VM_ID: AtomicU64 = AtomicU64::new(1);

impl VM {
    pub fn new(source: Arc<Source>) -> Result<Self, RuntimeError> {
        Self::with_config_and_args(source, VmConfig::default(), Vec::new())
    }

    pub fn with_config(source: Arc<Source>, config: VmConfig) -> Result<Self, RuntimeError> {
        Self::with_config_and_args(source, config, Vec::new())
    }

    pub fn with_config_and_args(
        source: Arc<Source>,
        config: VmConfig,
        program_args: Vec<String>,
    ) -> Result<Self, RuntimeError> {
        let random_seed = super::random::initial_random_state();
        let mut vm = Self {
            heap: Heap::new(),
            config,
            registers: {
                let mut regs = Vec::with_capacity(MAX_REGISTERS);
                regs.resize(32768, Value::null());
                regs
            },
            frames: Vec::with_capacity(MAX_FRAMES),
            globals: HashMap::new(),
            global_mutability: HashMap::new(),
            globals_by_index_cache: HashMap::with_capacity(32),
            globals_by_index: Vec::with_capacity(64),
            global_generations: Vec::with_capacity(64),
            inline_call_cache: HashMap::with_capacity(64),
            last_inline_call_cache: None,
            jit_executor: None,
            jit_function_keys: crate::jit::InlineMap::default(),
            jit_call_counts: crate::jit::InlineMap::default(),
            jit_backedge_counts: crate::jit::InlineMap::default(),
            schema_registry: HashMap::new(),
            schema_index: HashMap::new(),
            next_schema_id: 1,
            source,
            open_upvalues: Vec::new(),
            current_upvalues: Vec::new(),
            resources: Vec::with_capacity(16),
            native_modules: HashMap::new(),
            native_registry: HashMap::new(),
            random_state: random_seed,
            random_seed,
            execution_control: super::ExecutionControl::default(),
            execution_stats: super::ExecutionStats::default(),
            jit_control_error: None,
            host_roots: Arc::new(super::roots::HostRootSet::new()),
            id: NEXT_VM_ID.fetch_add(1, Ordering::Relaxed),
            current_global_mapping_id: 0,
            program_args,
            script_path: None,
            repl_module_aliases: HashSet::new(),
            repl_known_globals: HashSet::new(),
            repl_known_native_globals: HashSet::new(),
            repl_symbol_origins: HashMap::new(),
        };
        super::builtins::register_builtins(&mut vm)?;

        let string_exports = crate::stdlib::string::register(&mut vm)?;
        vm.repl_module_aliases.insert("string".to_string());
        for name in &string_exports.all_exports {
            let qualified = format!("string::{}", name);
            vm.repl_known_globals.insert(qualified.clone());
            vm.repl_known_native_globals.insert(qualified.clone());
            vm.repl_symbol_origins
                .insert(name.clone(), format!("string::{}", name));
        }

        // io, math, convert, time: qualified + unqualified aliases
        type RegFn = fn(&mut VM) -> Result<crate::stdlib::StdModuleExports, RuntimeError>;
        let auto_modules: &[(&str, RegFn)] = &[
            ("io", crate::stdlib::io::register),
            ("math", crate::stdlib::math::register),
            ("convert", crate::stdlib::convert::register),
            ("time", crate::stdlib::time::register),
        ];
        for &(module_name, register_fn) in auto_modules {
            let exports = register_fn(&mut vm)?;
            vm.repl_module_aliases.insert(module_name.to_string());
            for name in &exports.all_exports {
                let qualified = format!("{}::{}", module_name, name);
                if let Some(value) = vm.get_global(&qualified) {
                    vm.set_global(name.clone(), value);
                }
                vm.repl_known_globals.insert(name.clone());
                vm.repl_known_native_globals.insert(name.clone());
                vm.repl_known_native_globals.insert(qualified.clone());
                vm.repl_symbol_origins.insert(name.clone(), qualified);
            }
        }

        Ok(vm)
    }
}
