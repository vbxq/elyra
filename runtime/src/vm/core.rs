use super::config::VmConfig;
use super::control::{ExecutionControl, ExecutionStats};
use super::frame::CallFrame;
use super::{GcRef, Heap, NativeFunctionImpl, Value};
use crate::native::NativeModule;
use crate::stdlib::Resource;
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const MAX_FRAMES: usize = 1024;
pub const MAX_REGISTERS: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InlineCacheKey {
    pub function: GcRef,
    pub instruction_pointer: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineCallCacheEntry {
    pub global_index: usize,
    pub global_generation: u64,
    pub target: GcRef,
}

// windowed regs like Lua
pub struct VM {
    pub(crate) heap: Heap,
    pub(crate) config: VmConfig,
    pub(crate) registers: Vec<Value>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) globals: HashMap<String, Value>,
    pub(crate) global_mutability: HashMap<String, bool>,
    pub(crate) globals_by_index_cache: HashMap<usize, Arc<Vec<Value>>>,
    pub(crate) globals_by_index: Vec<Value>,
    pub(crate) global_generations: Vec<u64>,
    pub(crate) inline_call_cache: HashMap<InlineCacheKey, InlineCallCacheEntry>,
    pub(crate) source: Arc<Source>,
    pub(crate) open_upvalues: Vec<GcRef>,
    pub(crate) current_upvalues: Vec<GcRef>,
    pub(crate) resources: Vec<Option<Resource>>,
    pub(crate) native_modules: HashMap<String, NativeModule>,
    pub(crate) native_registry: HashMap<String, NativeFunctionImpl>,
    pub(crate) random_state: u64,
    pub(crate) random_seed: u64,
    pub(crate) execution_control: ExecutionControl,
    pub(crate) execution_stats: ExecutionStats,

    pub(crate) current_global_mapping_id: usize,
    pub(crate) program_args: Vec<String>,
    pub(crate) script_path: Option<String>,
    pub(crate) repl_module_aliases: HashSet<String>,
    pub(crate) repl_known_globals: HashSet<String>,
    pub(crate) repl_known_native_globals: HashSet<String>,
    pub(crate) repl_symbol_origins: HashMap<String, String>,
}

// SAFETY: moving an idle VM transfers exclusive ownership of its heap and frames;
// cached pointers refer to allocations owned by that same VM and are never shared.
unsafe impl Send for VM {}

#[derive(Debug)]
pub enum StepResult {
    Continue,
    Return(Value),
}
