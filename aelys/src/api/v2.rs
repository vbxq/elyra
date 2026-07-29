use crate::jit::provider::JitProvider;
use aelys_backend::Compiler;
use aelys_bytecode::asm::{deserialize, serialize};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_bytecode::{GcRef, ObjectKind};
use aelys_common::error::{AelysError, CompileError, CompileErrorKind, RuntimeErrorKind};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{
    ExecutionControl, JitCallResult, JitExecutor, JitFunctionKey, VM, Value, VmConfig,
    VmConfigError,
};
use aelys_sema::TypeInference;
use aelys_syntax::{Source, Span};
use smallvec::SmallVec;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub use aelys_runtime::InterruptHandle;

const TIER1_CALL_THRESHOLD: u64 = 1_000;
const TIER2_CALL_THRESHOLD: u64 = 10_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JitMode {
    #[default]
    Off,
    Baseline,
    Tiered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JitConfig {
    max_cache_entries: usize,
}

impl JitConfig {
    pub fn with_max_cache_entries(
        mut self,
        max_cache_entries: usize,
    ) -> Result<Self, JitConfigError> {
        if max_cache_entries == 0 {
            return Err(JitConfigError::EmptyCache);
        }
        self.max_cache_entries = max_cache_entries;
        Ok(self)
    }

    pub fn max_cache_entries(&self) -> usize {
        self.max_cache_entries
    }
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            max_cache_entries: 256,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JitConfigError {
    EmptyCache,
    Initialization(String),
}

impl fmt::Display for JitConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCache => formatter.write_str("JIT cache must contain at least one entry"),
            Self::Initialization(error) => write!(formatter, "JIT initialization failed: {error}"),
        }
    }
}

impl std::error::Error for JitConfigError {}

#[derive(Clone, Debug)]
pub struct CompileOptions {
    pub optimization_level: OptimizationLevel,
    pub source_name: String,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            optimization_level: OptimizationLevel::Standard,
            source_name: "<memory>".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct IsolateConfig {
    vm_config: VmConfig,
    pub program_args: Vec<String>,
    pub random_seed: Option<u64>,
}

impl IsolateConfig {
    pub fn with_max_heap_bytes(mut self, bytes: u64) -> Result<Self, VmConfigError> {
        self.vm_config = VmConfig::new(bytes)?;
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub max_instructions: Option<u64>,
    pub deadline: Option<Instant>,
    pub interrupt: Option<InterruptHandle>,
    pub safepoint_interval: u32,
    pub report: bool,
}

impl RunOptions {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = Instant::now().checked_add(timeout);
        self
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            max_instructions: None,
            deadline: None,
            interrupt: None,
            safepoint_interval: 1_024,
            report: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExecutionOutcome {
    Returned(Value),
    Exited(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum StructuredValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<StructuredValue>),
    Vec(Vec<StructuredValue>),
}

#[derive(Debug)]
pub enum StructuredCloneError {
    InvalidHandle,
    Unsupported(&'static str),
    Cycle,
    MaximumDepth,
    Runtime(aelys_common::error::RuntimeError),
}

impl fmt::Display for StructuredCloneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => formatter.write_str("invalid or stale heap handle"),
            Self::Unsupported(kind) => write!(formatter, "{kind} cannot be structured-cloned"),
            Self::Cycle => formatter.write_str("cyclic values cannot be structured-cloned"),
            Self::MaximumDepth => formatter.write_str("structured clone depth limit exceeded"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StructuredCloneError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionReport {
    pub instructions: u64,
    pub allocations: u64,
    pub allocated_bytes: usize,
    pub collections: u64,
    pub gc_pause_total_ns: u64,
    pub gc_pause_max_ns: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub function: Option<String>,
    pub instruction_pointer: Option<usize>,
    pub source: String,
    pub random_seed: u64,
}

#[derive(Clone)]
pub struct CompiledModule {
    avbc: Arc<[u8]>,
    function: Arc<aelys_bytecode::Function>,
    module_id: u64,
    source: Arc<Source>,
}

impl CompiledModule {
    pub fn avbc(&self) -> &[u8] {
        &self.avbc
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    jit_mode: JitMode,
    jit: Option<Arc<JitProvider>>,
    next_module_id: AtomicU64,
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_jit_mode(jit_mode: JitMode) -> Self {
        Self::with_jit_config(jit_mode, JitConfig::default())
            .expect("default JIT configuration must initialize")
    }

    pub fn with_jit_config(jit_mode: JitMode, config: JitConfig) -> Result<Self, JitConfigError> {
        if config.max_cache_entries == 0 {
            return Err(JitConfigError::EmptyCache);
        }
        let jit = if jit_mode == JitMode::Off
            || !cfg!(all(target_os = "linux", target_arch = "x86_64"))
        {
            None
        } else {
            let call_threshold = match jit_mode {
                JitMode::Off => unreachable!(),
                JitMode::Baseline => 1,
                JitMode::Tiered => TIER1_CALL_THRESHOLD,
            };
            let tier2_call_threshold =
                (jit_mode == JitMode::Tiered).then_some(TIER2_CALL_THRESHOLD);
            Some(Arc::new(
                JitProvider::new(
                    config.max_cache_entries,
                    call_threshold,
                    tier2_call_threshold,
                )
                .map_err(JitConfigError::Initialization)?,
            ))
        };
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                jit_mode,
                jit,
                next_module_id: AtomicU64::new(1),
            }),
        })
    }

    pub fn jit_mode(&self) -> JitMode {
        self.inner.jit_mode
    }

    pub fn jit_cache_entries(&self) -> usize {
        self.inner
            .jit
            .as_ref()
            .map(|jit| jit.cache_len())
            .unwrap_or(0)
    }

    pub fn compile(
        &self,
        source: &str,
        options: CompileOptions,
    ) -> Result<CompiledModule, AelysError> {
        let source = Source::new(options.source_name, source);
        let tokens = Lexer::with_source(source.clone()).scan()?;
        let statements = Parser::new(tokens, source.clone()).parse()?;
        let (module_aliases, known_globals, known_native_globals) = standard_module_symbols()?;
        let typed = TypeInference::infer_program_with_imports(
            statements,
            source.clone(),
            module_aliases.clone(),
            known_globals.clone(),
        )
        .map_err(|errors| {
            let (message, span) = errors
                .first()
                .map(|error| (error.to_string(), error.span))
                .unwrap_or_else(|| ("unknown type error".to_string(), Span::dummy()));
            AelysError::Compile(CompileError::new(
                CompileErrorKind::TypeInferenceError(message),
                span,
                source.clone(),
            ))
        })?;
        let mut optimizer = Optimizer::new(options.optimization_level);
        let typed = optimizer.optimize(typed);
        let (function, _) = Compiler::with_modules(
            None,
            source.clone(),
            module_aliases,
            known_globals,
            known_native_globals,
            HashMap::new(),
        )
        .compile_typed(&typed)?;

        let avbc = serialize(&function).map_err(|error| {
            CompileError::new(
                CompileErrorKind::CompilationLimitExceeded(error.to_string()),
                Span::dummy(),
                source.clone(),
            )
        })?;
        let module_id = self.inner.next_module_id.fetch_add(1, Ordering::Relaxed);
        Ok(CompiledModule {
            avbc: Arc::from(avbc),
            function: Arc::new(function),
            module_id,
            source,
        })
    }

    pub fn try_new_isolate(&self, config: IsolateConfig) -> Result<Isolate, AelysError> {
        let mut vm = VM::with_config_and_args(
            Source::new("<isolate>", ""),
            config.vm_config,
            config.program_args,
        )
        .map_err(AelysError::Runtime)?;
        register_standard_modules(&mut vm)?;
        if let Some(jit) = &self.inner.jit {
            let executor: Arc<dyn JitExecutor> = Arc::clone(jit) as Arc<dyn JitExecutor>;
            vm.configure_jit(Some(executor));
        }
        if let Some(seed) = config.random_seed {
            vm.set_random_seed(seed);
        }
        Ok(Isolate {
            vm,
            runtime: Arc::clone(&self.inner),
            jit_call_counts: SmallVec::new(),
            last_report: None,
            _not_sync: Cell::new(()),
        })
    }

    pub fn new_isolate(&self, config: IsolateConfig) -> Isolate {
        self.try_new_isolate(config)
            .expect("validated isolate configuration must initialize")
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::with_jit_mode(JitMode::Off)
    }
}

pub struct Isolate {
    vm: VM,
    runtime: Arc<RuntimeInner>,
    jit_call_counts: SmallVec<[(u64, u64); 4]>,
    last_report: Option<ExecutionReport>,
    _not_sync: Cell<()>,
}

fn register_standard_modules(vm: &mut VM) -> Result<(), AelysError> {
    aelys_runtime::stdlib::sys::register(vm).map_err(AelysError::Runtime)?;
    aelys_runtime::stdlib::fs::register(vm).map_err(AelysError::Runtime)?;
    aelys_runtime::stdlib::net::register(vm).map_err(AelysError::Runtime)?;
    aelys_runtime::stdlib::bytes::register(vm).map_err(AelysError::Runtime)?;
    Ok(())
}

type StandardSymbols = (HashSet<String>, HashSet<String>, HashSet<String>);

fn standard_module_symbols() -> Result<StandardSymbols, AelysError> {
    let mut vm = VM::new(Source::new("<compile-context>", "")).map_err(AelysError::Runtime)?;
    let modules = [
        ("sys", aelys_runtime::stdlib::sys::register(&mut vm)),
        ("fs", aelys_runtime::stdlib::fs::register(&mut vm)),
        ("net", aelys_runtime::stdlib::net::register(&mut vm)),
        ("bytes", aelys_runtime::stdlib::bytes::register(&mut vm)),
    ];
    let mut aliases = HashSet::new();
    let mut globals = HashSet::new();
    let mut natives = HashSet::new();
    for (module, exports) in modules {
        let exports = exports.map_err(AelysError::Runtime)?;
        aliases.insert(module.to_string());
        for name in exports.all_exports {
            globals.insert(format!("{module}::{name}"));
        }
        natives.extend(exports.native_functions);
    }
    Ok((aliases, globals, natives))
}

impl Isolate {
    fn try_execute_jit(&mut self, module: &CompiledModule, options: &RunOptions) -> Option<Value> {
        if self.runtime.jit_mode == JitMode::Off {
            return None;
        }
        let provider = self.runtime.jit.as_ref()?;
        if options.max_instructions.is_some()
            || options.deadline.is_some()
            || options.interrupt.is_some()
            || options.report
        {
            return None;
        }
        let calls = if let Some((_, calls)) = self
            .jit_call_counts
            .iter_mut()
            .find(|(module_id, _)| *module_id == module.module_id)
        {
            *calls = calls.saturating_add(1);
            *calls
        } else {
            self.jit_call_counts.push((module.module_id, 1));
            1
        };
        let key = JitFunctionKey::root(module.module_id);
        if !provider.should_execute(&key, calls) {
            return None;
        }
        match provider.try_execute(&key, &module.function, &[], calls) {
            JitCallResult::Unsupported => None,
            JitCallResult::Returned(value) => Some(value),
            JitCallResult::Deoptimized { .. } => None,
        }
    }

    pub fn execute(
        &mut self,
        module: &CompiledModule,
        options: RunOptions,
    ) -> Result<ExecutionOutcome, AelysError> {
        if let Some(value) = self.try_execute_jit(module, &options) {
            self.last_report = None;
            return Ok(ExecutionOutcome::Returned(value));
        }
        self.vm.configure_execution(ExecutionControl {
            max_instructions: options.max_instructions,
            deadline: options.deadline,
            interrupt: options.interrupt.clone(),
            safepoint_interval: options.safepoint_interval,
            report: options.report,
        });
        let function = deserialize(module.avbc())
            .map_err(|error| self.invalid_bytecode_error(error.to_string()))?;
        self.vm.set_source(Arc::clone(&module.source));
        let function_ref = self
            .vm
            .alloc_function_with_jit_key(function, JitFunctionKey::root(module.module_id))
            .map_err(AelysError::Runtime)?;
        let result = self.vm.execute(function_ref);
        if options.report {
            let stats = self.vm.execution_stats();
            self.last_report = Some(ExecutionReport {
                instructions: stats.instructions,
                allocations: stats.allocations,
                allocated_bytes: self.vm.heap().bytes_allocated(),
                collections: stats.collections,
                gc_pause_total_ns: stats.gc_pause_micros.saturating_mul(1_000),
                gc_pause_max_ns: stats.gc_max_pause_micros.saturating_mul(1_000),
                cache_hits: stats.cache_hits,
                cache_misses: stats.cache_misses,
                function: self.vm.last_execution_function_name(),
                instruction_pointer: stats.last_instruction_pointer,
                source: module.source.name.clone(),
                random_seed: self.vm.random_seed(),
            });
        } else {
            self.last_report = None;
        }

        match result {
            Ok(value) => Ok(ExecutionOutcome::Returned(value)),
            Err(error) => match error.kind {
                RuntimeErrorKind::Exit(code) => Ok(ExecutionOutcome::Exited(code)),
                _ => Err(AelysError::Runtime(error)),
            },
        }
    }

    pub fn last_report(&self) -> Option<&ExecutionReport> {
        self.last_report.as_ref()
    }

    pub fn value_to_string(&self, value: Value) -> String {
        self.vm.value_to_string(value)
    }

    pub fn structured_clone(&self, value: Value) -> Result<StructuredValue, StructuredCloneError> {
        self.clone_value(value, 0, &mut HashSet::new())
    }

    pub fn import_clone(&mut self, value: &StructuredValue) -> Result<Value, StructuredCloneError> {
        self.import_value(value, 0)
    }

    fn clone_value(
        &self,
        value: Value,
        depth: usize,
        visiting: &mut HashSet<GcRef>,
    ) -> Result<StructuredValue, StructuredCloneError> {
        if depth > 256 {
            return Err(StructuredCloneError::MaximumDepth);
        }
        if value.is_null() {
            return Ok(StructuredValue::Null);
        }
        if let Some(value) = value.as_bool() {
            return Ok(StructuredValue::Bool(value));
        }
        if let Some(value) = value.as_int() {
            return Ok(StructuredValue::Int(value));
        }
        if let Some(value) = value.as_float() {
            return Ok(StructuredValue::Float(value));
        }

        let reference = value
            .as_ptr()
            .map(GcRef::new)
            .ok_or(StructuredCloneError::Unsupported("value"))?;
        if !visiting.insert(reference) {
            return Err(StructuredCloneError::Cycle);
        }
        let result = match self.vm.heap().get(reference) {
            Some(object) => match &object.kind {
                ObjectKind::String(value) => {
                    Ok(StructuredValue::String(value.as_str().to_string()))
                }
                ObjectKind::Array(array) => {
                    let mut values = Vec::with_capacity(array.len());
                    for index in 0..array.len() {
                        let value = array
                            .get(index)
                            .ok_or(StructuredCloneError::InvalidHandle)?;
                        values.push(self.clone_value(value, depth + 1, visiting)?);
                    }
                    Ok(StructuredValue::Array(values))
                }
                ObjectKind::Vec(vector) => {
                    let mut values = Vec::with_capacity(vector.len());
                    for index in 0..vector.len() {
                        let value = vector
                            .get(index)
                            .ok_or(StructuredCloneError::InvalidHandle)?;
                        values.push(self.clone_value(value, depth + 1, visiting)?);
                    }
                    Ok(StructuredValue::Vec(values))
                }
                ObjectKind::Function(_) => Err(StructuredCloneError::Unsupported("function")),
                ObjectKind::Closure(_) => Err(StructuredCloneError::Unsupported("closure")),
                ObjectKind::Native(_) => Err(StructuredCloneError::Unsupported("native function")),
                ObjectKind::Upvalue(_) => Err(StructuredCloneError::Unsupported("upvalue")),
            },
            None => Err(StructuredCloneError::InvalidHandle),
        };
        visiting.remove(&reference);
        result
    }

    fn import_value(
        &mut self,
        value: &StructuredValue,
        depth: usize,
    ) -> Result<Value, StructuredCloneError> {
        if depth > 256 {
            return Err(StructuredCloneError::MaximumDepth);
        }
        match value {
            StructuredValue::Null => Ok(Value::null()),
            StructuredValue::Bool(value) => Ok(Value::bool(*value)),
            StructuredValue::Int(value) => Value::int_checked(*value)
                .map_err(|_| StructuredCloneError::Unsupported("out-of-range integer")),
            StructuredValue::Float(value) => Ok(Value::float(*value)),
            StructuredValue::String(value) => self
                .vm
                .intern_string(value)
                .map(|reference| Value::ptr(reference.index()))
                .map_err(StructuredCloneError::Runtime),
            StructuredValue::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.import_value(value, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                self.vm
                    .alloc_array(AelysArray::from_objects(values))
                    .map(|reference| Value::ptr(reference.index()))
                    .map_err(StructuredCloneError::Runtime)
            }
            StructuredValue::Vec(values) => {
                let values = values
                    .iter()
                    .map(|value| self.import_value(value, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                self.vm
                    .alloc_vec(AelysVec::from_objects(values))
                    .map(|reference| Value::ptr(reference.index()))
                    .map_err(StructuredCloneError::Runtime)
            }
        }
    }

    fn invalid_bytecode_error(&self, message: String) -> AelysError {
        AelysError::Runtime(aelys_common::error::RuntimeError::new(
            RuntimeErrorKind::InvalidBytecode(message),
            Vec::new(),
            Arc::clone(self.vm.source()),
        ))
    }
}
