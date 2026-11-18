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

pub(crate) struct PreparedGlobals {
    pub(crate) values: Vec<Value>,
    pub(crate) name_ids: Box<[u32]>,
}

pub struct VM {
    pub(crate) heap: Heap,
    pub(crate) config: VmConfig,
    pub(crate) registers: Vec<Value>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) globals: HashMap<String, Value>,
    pub(crate) global_mutability: HashMap<String, bool>,
    /// prepared views by mapping id. reaching them by index rather than by hash matters
    pub(crate) globals_by_index_cache: Vec<Option<PreparedGlobals>>,
    pub(crate) global_name_ids: HashMap<String, u32>,
    pub(crate) global_names_by_id: Vec<String>,
    pub(crate) global_users: Vec<Vec<(usize, u32)>>,
    /// by name id: the value an indexed write left while the name keyed entry still holds an older one
    pub(crate) global_latest: Vec<Option<Value>>,
    pub(crate) global_stale: Vec<u32>,
    pub(crate) globals_by_index: Vec<Value>,
    /// set when an indexed write has not reached the name keyed table yet, so a call between two readers costs nothing
    pub(crate) globals_by_index_dirty: bool,
    pub(crate) global_generations: Vec<u64>,
    pub(crate) inline_call_cache: HashMap<InlineCacheKey, InlineCallCacheEntry>,
    pub(crate) last_inline_call_cache: Option<(InlineCacheKey, InlineCallCacheEntry)>,
    pub(crate) jit_executor: Option<Arc<dyn crate::JitExecutor>>,
    pub(crate) jit_function_keys: crate::jit::InlineMap<GcRef, crate::JitFunctionKey>,
    pub(crate) frame_jit: super::jit_frame::FrameJitState,
    pub(crate) schema_registry: HashMap<SchemaId, StructSchema>,
    pub(crate) schema_index: HashMap<super::alloc::StructIdentity, SchemaId>,
    pub(crate) next_schema_id: u32,
    pub(crate) enum_index: HashMap<super::alloc::EnumIdentity, u16>,
    pub(crate) enum_registry: Vec<Box<[usize]>>,
    pub(crate) source: Arc<Source>,
    pub(crate) open_upvalues: Vec<GcRef>,
    pub(crate) current_upvalues: Vec<GcRef>,
    pub(crate) resources: Vec<Option<Resource>>,
    /// one immutable Ok(()) handed out by every native that succeeds with unit; it stays rooted
    pub(crate) unit_ok: Option<GcRef>,
    pub(crate) native_modules: HashMap<String, NativeModule>,
    pub(crate) native_registry: HashMap<String, u32>,
    pub(crate) native_impls: Vec<NativeFunctionImpl>,
    pub(crate) random_state: u64,
    pub(crate) random_seed: u64,
    pub(crate) execution_control: ExecutionControl,
    pub(crate) execution_stats: ExecutionStats,
    pub(crate) gc_pauses: super::gc::GcPauseStats,
    pub(crate) host_roots: Arc<HostRootSet>,
    pub(crate) native_call_roots: Vec<GcRef>,
    pub(crate) id: u64,

    pub(crate) current_global_mapping_id: usize,
    pub(crate) program_args: Vec<String>,
    pub(crate) script_path: Option<String>,
    pub(crate) repl_module_aliases: HashSet<String>,
    pub(crate) repl_known_globals: HashSet<String>,
    pub(crate) repl_known_native_globals: HashSet<String>,
    pub(crate) repl_symbol_origins: HashMap<String, String>,
    pub(crate) prelude: Arc<PreludeScope>,
}

#[derive(Debug, Clone, Default)]
pub struct PreludeScope {
    pub module_aliases: HashSet<String>,
    pub known_globals: HashSet<String>,
    pub known_native_globals: HashSet<String>,
    pub symbol_origins: HashMap<String, String>,
}

// cached pointers refer to allocations owned by that same vm and are never shared.
unsafe impl Send for VM {}

#[derive(Debug)]
pub enum StepResult {
    Continue,
    Return(Value),
}
