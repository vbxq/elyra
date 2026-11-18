use crate::{Function, Value};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::ffi::c_void;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::Arc;

pub const JIT_TIER1_BACKEDGE_THRESHOLD: u64 = 10_000;

static NEXT_JIT_MODULE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// the module half of a function key
pub fn next_jit_module_id() -> u64 {
    NEXT_JIT_MODULE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// per-invocation state passed to JIT code that runs under execution control
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JitExecutionContext {
    pub data: *mut c_void,
    pub poll: unsafe extern "C" fn(*mut c_void) -> i64,
    pub controlled: bool,
}

// keys are never attacker chosen, so a weak hash is safe
#[derive(Default)]
pub(crate) struct WordHasher(u64);

impl WordHasher {
    #[inline(always)]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for WordHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline(always)]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline(always)]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline(always)]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }
}

pub(crate) struct InlineMap<K, V> {
    inline: SmallVec<[(K, V); 4]>,
    heap: Option<HashMap<K, V, BuildHasherDefault<WordHasher>>>,
}

impl<K, V> Default for InlineMap<K, V> {
    fn default() -> Self {
        Self {
            inline: SmallVec::new(),
            heap: None,
        }
    }
}

impl<K: Eq + Hash, V> InlineMap<K, V> {
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        if let Some(heap) = &self.heap {
            return heap.get(key);
        }
        self.inline
            .iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if let Some(heap) = &mut self.heap {
            return heap.get_mut(key);
        }
        self.inline
            .iter_mut()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub(crate) fn clear(&mut self) {
        self.inline.clear();
        if let Some(heap) = &mut self.heap {
            heap.clear();
        }
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        if let Some(heap) = &mut self.heap {
            heap.retain(|key, value| keep(key, value));
        } else {
            self.inline.retain(|(key, value)| keep(key, value));
        }
    }
}

impl<K: Eq + Hash, V> InlineMap<K, V> {
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if let Some(heap) = &mut self.heap {
            heap.insert(key, value);
            return;
        }
        if let Some(existing) = self.get_mut(&key) {
            *existing = value;
            return;
        }
        if self.inline.len() < self.inline.inline_size() {
            self.inline.push((key, value));
            return;
        }
        let mut heap = HashMap::with_capacity_and_hasher(
            self.inline.len().saturating_mul(2),
            BuildHasherDefault::default(),
        );
        heap.extend(std::mem::take(&mut self.inline));
        heap.insert(key, value);
        self.heap = Some(heap);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JitFunctionKey {
    module: u64,
    path: SmallVec<[u32; 4]>,
}

impl JitFunctionKey {
    pub fn root(module: u64) -> Self {
        Self {
            module,
            path: SmallVec::from_slice(&[0]),
        }
    }

    pub fn child(&self, index: u32) -> Self {
        let mut path = self.path.clone();
        path.push(index);
        Self {
            module: self.module,
            path,
        }
    }

    pub fn module(&self) -> u64 {
        self.module
    }

    pub fn path(&self) -> &[u32] {
        &self.path
    }

    pub fn shared_path(&self) -> Arc<[u32]> {
        Arc::from(self.path.as_slice())
    }
}

pub const FRAME_STATUS_OK: u64 = 0;
pub const FRAME_STATUS_EXIT: u64 = 1;
pub const FRAME_STATUS_CALL_IN_INTERPRETER: u64 = 2;
pub const FRAME_STATUS_NATIVE_ERROR: u64 = 3;

// field offsets are read by generated code
#[repr(C)]
pub struct FrameJitContext {
    pub vm: *mut c_void,
    pub status: u64,
    pub resume_ip: u64,
    pub global_epoch: u64,
    pub registers_end: u64,
    pub depth: u64,
    pub scratch0: u64,
    pub scratch1: u64,
    pub globals: u64,
    pub globals_len: u64,
    pub view_generation: u64,
    pub progress: u64,
}

impl FrameJitContext {
    pub const STATUS: i32 = 8;
    pub const RESUME_IP: i32 = 16;
    pub const GLOBAL_EPOCH: i32 = 24;
    pub const REGISTERS_END: i32 = 32;
    pub const DEPTH: i32 = 40;
    pub const SCRATCH0: i32 = 48;
    pub const SCRATCH1: i32 = 56;
    pub const GLOBALS: i32 = 64;
    pub const GLOBALS_LEN: i32 = 72;
    pub const VIEW_GENERATION: i32 = 80;
    pub const PROGRESS: i32 = 88;
}

// a per-vm block of u64 words: header then one site record per call or global site
pub struct FrameData;

impl FrameData {
    pub const CONSTANTS: i32 = 0;
    pub const FUNCTION: i32 = 8;
    pub const CODE: i32 = 16;
    pub const VIEW_GENERATION: i32 = 32;
    pub const VIEW_VALUES: i32 = 40;
    pub const VIEW_LEN: i32 = 48;
    pub const MAPPING: i32 = 56;
    pub const HEADER_WORDS: usize = 8;
    pub const SITE_WORDS: usize = 5;
    pub const SITE_EPOCH: i32 = 0;
    pub const SITE_VALUE: i32 = 8;
    pub const SITE_ENTRY: i32 = 16;
    pub const SITE_DATA: i32 = 24;
    pub const SITE_REGISTERS: i32 = 32;

    pub fn site_offset(site: usize) -> Option<i32> {
        let words = Self::HEADER_WORDS.checked_add(site.checked_mul(Self::SITE_WORDS)?)?;
        i32::try_from(words.checked_mul(8)?).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameSiteKind {
    CallGlobal { name: Arc<str> },
    CallRegister,
    Native { name: Arc<str> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameCollectionKind {
    ArrayInts = 0,
    ArrayFloats = 1,
    ArrayBools = 2,
    ArrayObjects = 3,
    VecInts = 4,
    VecFloats = 5,
    VecBools = 6,
    VecObjects = 7,
    Length = 8,
}

impl FrameCollectionKind {
    pub fn from_u64(value: u64) -> Option<Self> {
        Some(match value {
            0 => Self::ArrayInts,
            1 => Self::ArrayFloats,
            2 => Self::ArrayBools,
            3 => Self::ArrayObjects,
            4 => Self::VecInts,
            5 => Self::VecFloats,
            6 => Self::VecBools,
            7 => Self::VecObjects,
            8 => Self::Length,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct FrameCodeInfo {
    pub address: usize,
    pub num_registers: u32,
    pub arity: u16,
    pub osr_ip: Option<u32>,
    pub sites: Vec<FrameSiteKind>,
}

pub trait FrameCode: Send + Sync {
    fn info(&self) -> &FrameCodeInfo;
}

pub type FrameEntry = unsafe extern "C" fn(*mut FrameJitContext, *mut u64, *mut u64) -> u64;

#[derive(Clone, Debug, PartialEq)]
pub enum JitCallResult {
    Unsupported,
    Returned(Value),
    Aborted,
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, JitDeoptValue)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JitDeoptValue {
    Value(Value),
    Argument(usize),
}

#[derive(Clone, Copy, Debug)]
pub enum JitArgument<'a> {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    IntegerArray(&'a [i64]),
    IntegerVec(&'a [i64]),
    Unused,
}

pub trait JitExecutor: Send + Sync {
    fn should_execute(&self, key: &JitFunctionKey, calls: u64) -> bool;

    fn compile_frame(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        osr_ip: Option<u32>,
    ) -> Option<Arc<dyn FrameCode>> {
        let _ = (key, function, osr_ip);
        None
    }

    fn frame_call_threshold(&self) -> u64 {
        u64::MAX
    }

    fn record_frame_osr_return(&self) {}

    fn record_frame_exit(&self) {}

    /// true when no later call can compile this function, so the VM can stop asking
    fn refuses_forever(&self, key: &JitFunctionKey) -> bool {
        let _ = key;
        false
    }

    fn observe_backedge(&self, key: &JitFunctionKey, function: &Function, backedges: u64);

    fn try_execute(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
    ) -> JitCallResult;

    /// execute with an isolate-local control context. providers that do not support controlled machine code retain the old fallback behavior.
    fn try_execute_with_context(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        let _ = context;
        self.try_execute(key, function, arguments, calls)
    }

    fn try_execute_osr(
        &self,
        _key: &JitFunctionKey,
        _function: &Function,
        _bytecode_ip: u32,
        _registers: &[JitArgument<'_>],
    ) -> JitCallResult {
        JitCallResult::Unsupported
    }

    fn try_execute_osr_with_context(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        bytecode_ip: u32,
        registers: &[JitArgument<'_>],
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        let _ = context;
        self.try_execute_osr(key, function, bytecode_ip, registers)
    }
}
