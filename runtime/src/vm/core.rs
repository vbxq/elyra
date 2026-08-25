use super::config::VmConfig;
use super::control::{ExecutionControl, ExecutionStats};
use super::frame::CallFrame;
use super::roots::HostRootSet;
use super::{GcRef, Heap, NativeFunctionImpl, Value};
use crate::native::NativeModule;
use crate::stdlib::Resource;
use aelys_bytecode::{SchemaId, StructSchema};
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const MAX_FRAMES: usize = 1024;
pub const MAX_REGISTERS: usize = aelys_bytecode::asm::MAX_REGISTERS as usize;

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
    pub(crate) last_inline_call_cache: Option<(InlineCacheKey, InlineCallCacheEntry)>,
    pub(crate) jit_executor: Option<Arc<dyn crate::JitExecutor>>,
    pub(crate) jit_function_keys: crate::jit::InlineMap<GcRef, crate::JitFunctionKey>,
    pub(crate) jit_call_counts: crate::jit::InlineMap<crate::JitFunctionKey, u64>,
    pub(crate) jit_backedge_counts: crate::jit::InlineMap<crate::JitFunctionKey, u64>,
    pub(crate) schema_registry: HashMap<SchemaId, StructSchema>,
    pub(crate) next_schema_id: u32,
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
    pub(crate) jit_control_error: Option<aelys_common::error::RuntimeError>,
    pub(crate) host_roots: Arc<HostRootSet>,
    pub(crate) id: u64,

    pub(crate) current_global_mapping_id: usize,
    pub(crate) program_args: Vec<String>,
    pub(crate) script_path: Option<String>,
    pub(crate) repl_module_aliases: HashSet<String>,
    pub(crate) repl_known_globals: HashSet<String>,
    pub(crate) repl_known_native_globals: HashSet<String>,
    pub(crate) repl_symbol_origins: HashMap<String, String>,
}

// cached pointers refer to allocations owned by that same vm and are never shared.
unsafe impl Send for VM {}

#[derive(Debug)]
pub enum StepResult {
    Continue,
    Return(Value),
}
