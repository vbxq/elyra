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
        let mut heap = Heap::new();
        heap.set_nursery_bytes(config.gc_nursery_bytes);
        heap.set_stress(config.gc_stress);
        heap.set_max_heap_bytes(config.max_heap_bytes);
        heap.set_verify(config.gc_verify);
        let mut vm = Self {
            heap,
            config,
            registers: {
                let mut regs = Vec::with_capacity(MAX_REGISTERS);
                regs.resize(32768, Value::null());
                regs
            },
            frames: Vec::with_capacity(MAX_FRAMES),
            globals: HashMap::new(),
            global_mutability: HashMap::new(),
            globals_by_index_cache: Vec::new(),
            global_name_ids: HashMap::new(),
            global_names_by_id: Vec::new(),
            global_users: Vec::new(),
            global_latest: Vec::new(),
            global_stale: Vec::new(),
            globals_by_index: Vec::with_capacity(64),
            globals_by_index_dirty: false,
            global_generations: Vec::with_capacity(64),
            inline_call_cache: HashMap::with_capacity(64),
            last_inline_call_cache: None,
            jit_executor: None,
            jit_function_keys: crate::jit::InlineMap::default(),
            frame_jit: super::jit_frame::FrameJitState::default(),
            schema_registry: HashMap::new(),
            schema_index: HashMap::new(),
            next_schema_id: 1,
            enum_index: HashMap::new(),
            enum_registry: Vec::new(),
            source,
            open_upvalues: Vec::new(),
            current_upvalues: Vec::new(),
            resources: Vec::with_capacity(16),
            unit_ok: None,
            native_modules: HashMap::new(),
            native_registry: HashMap::new(),
            native_impls: Vec::new(),
            random_state: random_seed,
            random_seed,
            execution_control: super::ExecutionControl::default(),
            execution_stats: super::ExecutionStats::default(),
            gc_pauses: super::gc::GcPauseStats::default(),
            host_roots: Arc::new(super::roots::HostRootSet::new()),
            native_call_roots: Vec::new(),
            id: NEXT_VM_ID.fetch_add(1, Ordering::Relaxed),
            current_global_mapping_id: 0,
            program_args,
            script_path: None,
            repl_module_aliases: HashSet::new(),
            repl_known_globals: HashSet::new(),
            repl_known_native_globals: HashSet::new(),
            repl_symbol_origins: HashMap::new(),
            prelude: Arc::default(),
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

        vm.prelude = Arc::new(super::core::PreludeScope {
            module_aliases: vm.repl_module_aliases.clone(),
            known_globals: vm.repl_known_globals.clone(),
            known_native_globals: vm.repl_known_native_globals.clone(),
            symbol_origins: vm.repl_symbol_origins.clone(),
        });
        Ok(vm)
    }
}
