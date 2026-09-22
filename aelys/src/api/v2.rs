use super::native_registry::{NativeModuleRegistration, invalid_module_reason};
use crate::jit::provider::JitProvider;
use aelys_backend::Compiler;
use aelys_bytecode::asm::{deserialize, serialize};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_bytecode::{GcRef, ObjectKind};
use aelys_common::error::{
    AelysError, CompileError, CompileErrorKind, RuntimeError, RuntimeErrorKind,
};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_native::AelysModuleDescriptor;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::stdlib::StdModuleExports;
use aelys_runtime::{
    ExecutionControl, HostRoot, JitArgument, JitCallResult, JitDeoptValue, JitExecutor,
    JitFunctionKey, VM, Value, VmConfig, VmConfigError,
};
use aelys_sema::{InferType, TypeInference};
use aelys_syntax::{Source, Span};
use smallvec::SmallVec;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub use aelys_driver::CallableFunction;
pub use aelys_runtime::InterruptHandle;

const TIER1_CALL_THRESHOLD: u64 = 1_000;
const TIER2_CALL_THRESHOLD: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitMode {
    Off,
    Baseline,
    Tiered,
}

impl Default for JitMode {
    fn default() -> Self {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Self::Tiered
        } else {
            Self::Off
        }
    }
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
    StructValuesUnsupported,
    Cycle,
    MaximumDepth,
    Runtime(aelys_common::error::RuntimeError),
}

impl fmt::Display for StructuredCloneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHandle => formatter.write_str("invalid or stale heap handle"),
            Self::Unsupported(kind) => write!(formatter, "{kind} cannot be structured-cloned"),
            Self::StructValuesUnsupported => {
                formatter.write_str("struct values are not supported by structured clone")
            }
            Self::Cycle => formatter.write_str("cyclic values cannot be structured-cloned"),
            Self::MaximumDepth => formatter.write_str("structured clone depth limit exceeded"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StructuredCloneError {}

/// `allocations` is a monotone count since the beginning of the run;
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
    /// set when the module imported other modules, whose globals live in that one isolate
    linked_isolate: Option<u64>,
}

impl CompiledModule {
    pub fn avbc(&self) -> &[u8] {
        &self.avbc
    }
}

// `avbc()` hands bytes to callers who run them in another isolate or process, so
fn reject_unverifiable_module(
    function: &aelys_bytecode::Function,
    source: &Arc<Source>,
) -> Result<(), AelysError> {
    aelys_runtime::verify_emitted_function(function).map_err(|reason| {
        AelysError::Runtime(RuntimeError::from_verifier(
            RuntimeErrorKind::InvalidBytecode(reason),
            Vec::new(),
            Arc::clone(source),
        ))
    })
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    jit_mode: JitMode,
    jit: Option<Arc<JitProvider>>,
    native_modules: Mutex<Vec<Arc<NativeModuleRegistration>>>,
    pending_native_aliases: Mutex<HashSet<String>>,
    standard_symbols: OnceLock<Result<StandardSymbols, String>>,
}

type NativeSymbols = (HashSet<String>, HashSet<String>, HashMap<String, InferType>);

impl RuntimeInner {
    fn standard_module_symbols(&self) -> Result<StandardSymbols, AelysError> {
        let cached = self
            .standard_symbols
            .get_or_init(|| build_standard_module_symbols().map_err(|error| error.to_string()));
        match cached {
            Ok(symbols) => Ok(symbols.clone()),
            Err(message) => Err(AelysError::Runtime(RuntimeError::new(
                RuntimeErrorKind::InvalidBytecode(message.clone()),
                Vec::new(),
                Source::new("<compile-context>", ""),
            ))),
        }
    }

    fn native_module_symbols(&self) -> NativeSymbols {
        let modules = self
            .native_modules
            .lock()
            .expect("native module registry poisoned");
        let mut aliases = HashSet::new();
        let mut natives = HashSet::new();
        let mut signatures = HashMap::new();
        for registration in modules.iter() {
            aliases.insert(registration.alias.clone());
            natives.extend(registration.qualified_names());
            signatures.extend(registration.signatures.clone());
        }
        (aliases, natives, signatures)
    }
}

fn build_jit_provider(
    jit_mode: JitMode,
    config: &JitConfig,
) -> Result<Option<Arc<JitProvider>>, JitConfigError> {
    if config.max_cache_entries == 0 {
        return Err(JitConfigError::EmptyCache);
    }
    if jit_mode == JitMode::Off || !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Ok(None);
    }
    let call_threshold = match jit_mode {
        JitMode::Off => unreachable!("the mode that compiles nothing returned above"),
        JitMode::Baseline => 1,
        JitMode::Tiered => TIER1_CALL_THRESHOLD,
    };
    let tier2_call_threshold = (jit_mode == JitMode::Tiered).then_some(TIER2_CALL_THRESHOLD);
    Ok(Some(Arc::new(
        JitProvider::new(
            config.max_cache_entries,
            call_threshold,
            tier2_call_threshold,
        )
        .map_err(JitConfigError::Initialization)?,
    )))
}

/// the executor a host installs with `VM::configure_jit` when it runs a program through the driver rather than through an isolate
pub fn new_jit_executor(
    jit_mode: JitMode,
    config: JitConfig,
) -> Result<Option<Arc<dyn JitExecutor>>, JitConfigError> {
    Ok(build_jit_provider(jit_mode, &config)?
        .map(|provider| provider as Arc<dyn aelys_runtime::JitExecutor>))
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
        let jit = build_jit_provider(jit_mode, &config)?;
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                jit_mode,
                jit,
                native_modules: Mutex::new(Vec::new()),
                pending_native_aliases: Mutex::new(HashSet::new()),
                standard_symbols: OnceLock::new(),
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

    pub fn jit_deoptimizations(&self) -> u64 {
        self.inner
            .jit
            .as_ref()
            .map(|jit| jit.deoptimizations())
            .unwrap_or(0)
    }

    pub fn jit_osr_executions(&self) -> u64 {
        self.inner
            .jit
            .as_ref()
            .map(|jit| jit.osr_executions())
            .unwrap_or(0)
    }

    fn standard_module_symbols(&self) -> Result<StandardSymbols, AelysError> {
        self.inner.standard_module_symbols()
    }

    #[doc = "# Safety"]
    #[doc = "the module must refer to a valid static abi descriptor."]
    pub unsafe fn register_native_module(
        &self,
        module: &'static AelysModuleDescriptor,
    ) -> Result<(), AelysError> {
        let validated = unsafe { NativeModuleRegistration::validate(module) }?;
        if is_standard_module_alias(validated.alias()) {
            return Err(invalid_module_reason(
                validated.alias(),
                "alias is reserved by a standard module",
            ));
        }
        let alias = validated.alias().to_string();
        self.reserve_native_alias(&alias)?;

        // thread from initializing the same alias concurrently.
        let registration = match validated.initialize() {
            Ok(registration) => Arc::new(registration),
            Err(error) => {
                self.release_native_alias(&alias);
                return Err(error);
            }
        };
        let mut modules = self
            .inner
            .native_modules
            .lock()
            .expect("native module registry poisoned");
        self.inner
            .pending_native_aliases
            .lock()
            .expect("native module reservations poisoned")
            .remove(&alias);
        if modules
            .iter()
            .any(|existing| existing.alias == registration.alias)
        {
            return Err(duplicate_alias(&registration.alias));
        }
        modules.push(registration);
        Ok(())
    }

    fn reserve_native_alias(&self, alias: &str) -> Result<(), AelysError> {
        let modules = self
            .inner
            .native_modules
            .lock()
            .expect("native module registry poisoned");
        if modules.iter().any(|existing| existing.alias == alias) {
            return Err(duplicate_alias(alias));
        }
        let mut pending = self
            .inner
            .pending_native_aliases
            .lock()
            .expect("native module reservations poisoned");
        if !pending.insert(alias.to_string()) {
            return Err(duplicate_alias(alias));
        }
        Ok(())
    }

    fn release_native_alias(&self, alias: &str) {
        self.inner
            .pending_native_aliases
            .lock()
            .expect("native module reservations poisoned")
            .remove(alias);
    }

    pub(crate) fn native_module_symbols(&self) -> NativeSymbols {
        self.inner.native_module_symbols()
    }

    pub fn compile(
        &self,
        source: &str,
        options: CompileOptions,
    ) -> Result<CompiledModule, AelysError> {
        let source = Source::new(options.source_name, source);
        let tokens = Lexer::with_source(source.clone()).scan()?;
        let statements = Parser::new_rust_collections(tokens, source.clone()).parse()?;
        let (mut module_aliases, mut known_globals, mut known_native_globals) =
            self.standard_module_symbols()?;
        let (native_aliases, native_globals, native_signatures) = self.native_module_symbols();
        module_aliases.extend(native_aliases);
        known_globals.extend(native_globals.iter().cloned());
        known_native_globals.extend(native_globals);
        reject_unprovided_needs(&statements, &module_aliases, &source)?;
        let typed = TypeInference::infer_program_full_with_native_signatures(
            statements,
            source.clone(),
            module_aliases.clone(),
            known_globals.clone(),
            known_native_globals.clone(),
            native_signatures,
            aelys_sema::infer::imports::ImportedTypes::default(),
        )
        .map_err(|errors| type_inference_error(&errors, &source))?;
        let mut optimizer = Optimizer::new(options.optimization_level);
        let typed = optimizer.optimize(typed.program);
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
                Span::whole_unit(&source),
                source.clone(),
            )
        })?;
        reject_unverifiable_module(&function, &source)?;
        let module_id = aelys_runtime::next_jit_module_id();
        Ok(CompiledModule {
            avbc: Arc::from(avbc),
            function: Arc::new(function),
            module_id,
            source,
            linked_isolate: None,
        })
    }

    /// process-wide id and is therefore safe to execute through any isolate
    pub fn load_avbc(
        &self,
        avbc: &[u8],
        source_name: impl Into<String>,
    ) -> Result<CompiledModule, AelysError> {
        let source = Source::new(source_name.into(), "");
        let function = deserialize(avbc).map_err(|error| {
            AelysError::Runtime(RuntimeError::new(
                RuntimeErrorKind::InvalidBytecode(error.to_string()),
                Vec::new(),
                Arc::clone(&source),
            ))
        })?;
        let module_id = aelys_runtime::next_jit_module_id();
        Ok(CompiledModule {
            avbc: Arc::from(avbc),
            function: Arc::new(function),
            module_id,
            source,
            linked_isolate: None,
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
        self.bind_native_modules(&mut vm)?;
        if let Some(jit) = &self.inner.jit {
            let executor: Arc<dyn JitExecutor> = Arc::clone(jit) as Arc<dyn JitExecutor>;
            vm.configure_jit(Some(executor));
        }
        if let Some(seed) = config.random_seed {
            vm.set_random_seed(seed);
        }
        let persistent_globals = vm.global_names().into_iter().collect();
        Ok(Isolate {
            id: NEXT_ISOLATE_ID.fetch_add(1, Ordering::Relaxed),
            vm,
            runtime: Arc::clone(&self.inner),
            jit_call_counts: SmallVec::new(),
            last_report: None,
            persistent_globals,
            module_globals: HashSet::new(),
            active_module_root: None,
            _not_sync: Cell::new(()),
        })
    }

    pub fn new_isolate(&self, config: IsolateConfig) -> Isolate {
        self.try_new_isolate(config)
            .expect("validated isolate configuration must initialize")
    }

    fn bind_native_modules(&self, vm: &mut VM) -> Result<(), AelysError> {
        let modules = self
            .inner
            .native_modules
            .lock()
            .expect("native module registry poisoned");
        for registration in modules.iter() {
            for (qualified_name, arity, function, result_type) in &registration.functions {
                let reference = vm
                    .alloc_foreign_with_result(qualified_name, *arity, *function, *result_type)
                    .map_err(AelysError::Runtime)?;
                vm.set_global(qualified_name.clone(), Value::ptr(reference.index()));
            }
        }
        Ok(())
    }
}

fn type_inference_error(errors: &[aelys_sema::TypeError], source: &Arc<Source>) -> AelysError {
    let (message, span) = errors
        .first()
        .map(|error| (error.to_string(), error.span))
        .unwrap_or_else(|| ("unknown type error".to_string(), Span::dummy()));
    AelysError::Compile(CompileError::new(
        CompileErrorKind::NamedTypeError {
            code: errors
                .first()
                .map(aelys_sema::TypeError::diagnostic_code)
                .unwrap_or(301),
            message,
        },
        span,
        Arc::clone(source),
    ))
}

const HOST_SEARCH_SURFACE: [&str; 3] = [
    "<built-in modules>",
    "<native modules registered on this runtime>",
    "no module search root: use Isolate::compile_file or Isolate::compile_with_root to load modules from disk",
];

/// a `needs` the runtime cannot satisfy from its own modules is an error, not a silent no-op.
fn reject_unprovided_needs(
    statements: &[aelys_syntax::Stmt],
    module_aliases: &HashSet<String>,
    source: &Arc<Source>,
) -> Result<(), AelysError> {
    for statement in statements {
        let aelys_syntax::StmtKind::Needs(needs) = &statement.kind else {
            continue;
        };
        if is_runtime_provided(&needs.path, module_aliases) {
            continue;
        }
        let module_path = needs.path.join(".");
        let kind = if needs.path.first().is_some_and(|segment| segment == "std") {
            CompileErrorKind::StdlibNotAvailable {
                module: module_path,
            }
        } else {
            CompileErrorKind::ModuleNotFound {
                module_path,
                searched_paths: HOST_SEARCH_SURFACE.iter().map(|s| s.to_string()).collect(),
            }
        };
        return Err(AelysError::Compile(CompileError::new(
            kind,
            needs.span,
            Arc::clone(source),
        )));
    }
    Ok(())
}

fn is_runtime_provided(path: &[String], module_aliases: &HashSet<String>) -> bool {
    if aelys_runtime::stdlib::is_std_module(path) {
        return true;
    }
    path.len() == 1 && module_aliases.contains(&path[0])
}

fn duplicate_alias(alias: &str) -> AelysError {
    invalid_module_reason(
        alias,
        "a native module with this alias is already registered",
    )
}

impl Default for Runtime {
    fn default() -> Self {
        Self::with_jit_mode(JitMode::default())
    }
}

pub struct ModuleInstance {
    isolate_id: u64,
    module_id: u64,
    avbc: Arc<[u8]>,
    function_ref: Cell<GcRef>,
    global_layout: Arc<aelys_bytecode::GlobalLayout>,
    jit_function: Arc<aelys_bytecode::Function>,
    source: Arc<Source>,
    root: HostRoot,
}

static NEXT_ISOLATE_ID: AtomicU64 = AtomicU64::new(1);

pub struct Isolate {
    id: u64,
    vm: VM,
    runtime: Arc<RuntimeInner>,
    jit_call_counts: SmallVec<[(u64, u64); 4]>,
    last_report: Option<ExecutionReport>,
    persistent_globals: HashSet<String>,
    module_globals: HashSet<String>,
    active_module_root: Option<HostRoot>,
    _not_sync: Cell<()>,
}

enum RootJitResult {
    Unsupported,
    Returned(Value),
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, Value)>,
    },
}

type StandardModuleRegister = fn(&mut VM) -> Result<StdModuleExports, RuntimeError>;

const STANDARD_MODULES: [(&str, StandardModuleRegister); 4] = [
    ("sys", aelys_runtime::stdlib::sys::register),
    ("fs", aelys_runtime::stdlib::fs::register),
    ("net", aelys_runtime::stdlib::net::register),
    ("bytes", aelys_runtime::stdlib::bytes::register),
];

fn register_standard_modules(vm: &mut VM) -> Result<(), AelysError> {
    for (_, register) in STANDARD_MODULES {
        register(vm).map_err(AelysError::Runtime)?;
    }
    Ok(())
}

const STANDARD_MODULE_ALIASES: [&str; 9] = [
    "string", "io", "math", "convert", "time", "sys", "fs", "net", "bytes",
];

fn is_standard_module_alias(alias: &str) -> bool {
    STANDARD_MODULE_ALIASES.contains(&alias)
}

type StandardSymbols = (HashSet<String>, HashSet<String>, HashSet<String>);

fn build_standard_module_symbols() -> Result<StandardSymbols, AelysError> {
    let mut vm = VM::new(Source::new("<compile-context>", "")).map_err(AelysError::Runtime)?;
    let mut aliases = vm.repl_module_aliases().clone();
    let mut globals = vm.repl_known_globals().clone();
    let mut natives = vm.repl_known_native_globals().clone();
    for (module, register) in STANDARD_MODULES {
        let exports = register(&mut vm).map_err(AelysError::Runtime)?;
        aliases.insert(module.to_string());
        for name in exports.all_exports {
            globals.insert(format!("{module}::{name}"));
        }
        natives.extend(exports.native_functions);
    }
    Ok((aliases, globals, natives))
}

impl Isolate {
    fn prepare_module_run(&mut self) {
        self.active_module_root = None;
        let old_globals = std::mem::take(&mut self.module_globals);
        for name in old_globals {
            if !self.persistent_globals.contains(&name) {
                self.vm.remove_global(&name);
            }
        }
        self.vm.invalidate_global_mapping();
    }

    fn try_execute_jit(
        &mut self,
        module_id: u64,
        function: &aelys_bytecode::Function,
        options: &RunOptions,
    ) -> Result<RootJitResult, AelysError> {
        if self.runtime.jit_mode == JitMode::Off || function.jit_unsupported_struct {
            return Ok(RootJitResult::Unsupported);
        }
        let Some(provider) = self.runtime.jit.as_ref() else {
            return Ok(RootJitResult::Unsupported);
        };
        self.vm.prepare_globals_for_layout(&function.global_layout);
        let calls = if let Some((_, calls)) = self
            .jit_call_counts
            .iter_mut()
            .find(|(id, _)| *id == module_id)
        {
            *calls = calls.saturating_add(1);
            *calls
        } else {
            self.jit_call_counts.push((module_id, 1));
            1
        };
        let key = JitFunctionKey::root(module_id);
        if !provider.should_execute(&key, calls) {
            return Ok(RootJitResult::Unsupported);
        }
        let context = Some(self.vm.jit_execution_context(
            options.report
                || options.max_instructions.is_some()
                || options.deadline.is_some()
                || options.interrupt.is_some(),
        ));
        match provider.try_execute_with_context(
            &key,
            function,
            &[] as &[JitArgument<'_>],
            calls,
            context.as_ref(),
        ) {
            JitCallResult::Unsupported => Ok(RootJitResult::Unsupported),
            JitCallResult::Returned(value) => Ok(RootJitResult::Returned(value)),
            JitCallResult::Aborted => Err(AelysError::Runtime(
                self.vm
                    .take_jit_control_error()
                    .unwrap_or_else(|| self.vm.runtime_error(RuntimeErrorKind::Interrupted)),
            )),
            JitCallResult::Deoptimized {
                bytecode_ip,
                registers,
            } => {
                let Some(registers) = registers
                    .into_iter()
                    .map(|(register, value)| match value {
                        JitDeoptValue::Value(value) => Some((register, value)),
                        JitDeoptValue::Argument(_) => None,
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(RootJitResult::Unsupported);
                };
                Ok(RootJitResult::Deoptimized {
                    bytecode_ip,
                    registers,
                })
            }
        }
    }

    /// `aelys run` does. imported modules are linked into this isolate, so the result runs here
    pub fn compile_file(
        &mut self,
        entry_file: &Path,
        options: CompileOptions,
    ) -> Result<CompiledModule, AelysError> {
        let name = entry_file.display().to_string();
        let text = std::fs::read_to_string(entry_file).map_err(|_| {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: name.clone(),
                    searched_paths: vec![name.clone()],
                },
                Span::dummy(),
                Source::new(&name, ""),
            ))
        })?;
        let module_root = entry_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let source = Source::new(&name, &text);
        self.compile_rooted(source, module_root, options.optimization_level)
    }

    pub fn compile_with_root(
        &mut self,
        source: &str,
        module_root: &Path,
        options: CompileOptions,
    ) -> Result<CompiledModule, AelysError> {
        let optimization_level = options.optimization_level;
        let source = Source::new(options.source_name, source);
        self.compile_rooted(source, module_root, optimization_level)
    }

    fn compile_rooted(
        &mut self,
        source: Arc<Source>,
        module_root: &Path,
        optimization_level: OptimizationLevel,
    ) -> Result<CompiledModule, AelysError> {
        let tokens = Lexer::with_source(Arc::clone(&source)).scan()?;
        let statements = Parser::new_rust_collections(tokens, Arc::clone(&source)).parse()?;

        let (mut module_aliases, mut known_globals, mut known_native_globals) =
            self.runtime.standard_module_symbols()?;
        let (native_aliases, native_globals, mut native_signatures) =
            self.runtime.native_module_symbols();
        module_aliases.extend(native_aliases.iter().cloned());
        known_globals.extend(native_globals.iter().cloned());
        known_native_globals.extend(native_globals);

        let before: HashSet<String> = self.vm.global_names().into_iter().collect();
        // the host natives are passed as already provided so a file of the same name cannot
        let imports = aelys_driver::modules::load_modules_in_dir(
            &statements,
            module_root,
            native_aliases,
            Arc::clone(&source),
            &mut self.vm,
        )?;
        for name in self.vm.global_names() {
            if !before.contains(&name) {
                self.persistent_globals.insert(name);
            }
        }

        module_aliases.extend(imports.module_aliases.iter().cloned());
        module_aliases.extend(self.vm.repl_module_aliases().iter().cloned());
        known_globals.extend(imports.known_globals.iter().cloned());
        known_globals.extend(self.vm.repl_known_globals().iter().cloned());
        known_native_globals.extend(imports.known_native_globals.iter().cloned());
        known_native_globals.extend(self.vm.repl_known_native_globals().iter().cloned());
        native_signatures.extend(imports.native_signatures.clone());

        let mut symbol_origins = imports.symbol_origins.clone();
        for (name, origin) in self.vm.repl_symbol_origins() {
            symbol_origins
                .entry(name.clone())
                .or_insert_with(|| origin.clone());
        }

        let main_statements: Vec<_> =
            imports
                .imported_impl_stmts
                .iter()
                .cloned()
                .chain(statements.into_iter().filter(|statement| {
                    !matches!(statement.kind, aelys_syntax::StmtKind::Needs(_))
                }))
                .collect();

        let typed = TypeInference::infer_program_full_with_native_signatures(
            main_statements,
            Arc::clone(&source),
            module_aliases.clone(),
            known_globals.clone(),
            known_native_globals.clone(),
            native_signatures,
            imports.imported_types.clone(),
        )
        .map_err(|errors| type_inference_error(&errors, &source))?;

        let mut optimizer = Optimizer::new(optimization_level);
        let typed = optimizer.optimize(typed.program);
        let (function, _) = Compiler::with_modules(
            None,
            Arc::clone(&source),
            module_aliases,
            known_globals,
            known_native_globals,
            symbol_origins,
        )
        .compile_typed(&typed)?;

        let avbc = serialize(&function).map_err(|error| {
            CompileError::new(
                CompileErrorKind::CompilationLimitExceeded(error.to_string()),
                Span::whole_unit(&source),
                Arc::clone(&source),
            )
        })?;
        reject_unverifiable_module(&function, &source)?;
        Ok(CompiledModule {
            avbc: Arc::from(avbc),
            function: Arc::new(function),
            module_id: aelys_runtime::next_jit_module_id(),
            source,
            linked_isolate: Some(self.id),
        })
    }

    pub fn execute(
        &mut self,
        module: &CompiledModule,
        options: RunOptions,
    ) -> Result<ExecutionOutcome, AelysError> {
        self.reject_foreign_module(module)?;
        self.prepare_module_run();
        self.configure_run(&options);
        let jit_result = self.try_execute_jit(module.module_id, &module.function, &options)?;
        if let RootJitResult::Returned(value) = &jit_result {
            self.last_report = self.jit_report(&options, &module.function, &module.source);
            return Ok(ExecutionOutcome::Returned(*value));
        }
        let instance = self.instantiate(module)?;
        self.run_instance(&instance, jit_result, options)
    }

    pub fn instantiate(&mut self, module: &CompiledModule) -> Result<ModuleInstance, AelysError> {
        self.reject_foreign_module(module)?;
        let function = self.deserialize_root(module.avbc())?;
        let global_layout = Arc::clone(&function.global_layout);
        self.vm.set_source(Arc::clone(&module.source));
        let function_ref = self.alloc_root_function(function, module.module_id)?;
        Ok(ModuleInstance {
            isolate_id: self.id,
            module_id: module.module_id,
            avbc: Arc::clone(&module.avbc),
            function_ref: Cell::new(function_ref),
            global_layout,
            jit_function: Arc::clone(&module.function),
            source: Arc::clone(&module.source),
            root: self.vm.pin_host_ref(function_ref),
        })
    }

    pub fn execute_instance(
        &mut self,
        instance: &ModuleInstance,
        options: RunOptions,
    ) -> Result<ExecutionOutcome, AelysError> {
        if instance.isolate_id != self.id {
            return Err(self.foreign_instance_error());
        }
        self.prepare_module_run();
        self.configure_run(&options);
        let jit_result =
            self.try_execute_jit(instance.module_id, &instance.jit_function, &options)?;
        if let RootJitResult::Returned(value) = &jit_result {
            self.last_report = self.jit_report(&options, &instance.jit_function, &instance.source);
            return Ok(ExecutionOutcome::Returned(*value));
        }
        self.run_instance(instance, jit_result, options)
    }

    fn jit_report(
        &self,
        options: &RunOptions,
        function: &aelys_bytecode::Function,
        source: &Source,
    ) -> Option<ExecutionReport> {
        if !options.report {
            return None;
        }
        let stats = self.vm.execution_stats();
        Some(ExecutionReport {
            instructions: stats.instructions,
            allocations: stats.allocations,
            allocated_bytes: self.vm.heap().bytes_allocated(),
            collections: stats.collections,
            gc_pause_total_ns: stats.gc_pause_micros.saturating_mul(1_000),
            gc_pause_max_ns: stats.gc_max_pause_micros.saturating_mul(1_000),
            cache_hits: stats.cache_hits,
            cache_misses: stats.cache_misses,
            instruction_pointer: stats.last_instruction_pointer,
            function: Some(function.name.as_deref().unwrap_or("<main>").to_string()),
            source: source.name.clone(),
            random_seed: self.vm.random_seed(),
        })
    }

    fn configure_run(&mut self, options: &RunOptions) {
        self.vm.configure_execution(ExecutionControl {
            max_instructions: options.max_instructions,
            deadline: options.deadline,
            interrupt: options.interrupt.clone(),
            safepoint_interval: options.safepoint_interval,
            report: options.report,
        });
    }

    fn deserialize_root(&self, avbc: &[u8]) -> Result<aelys_bytecode::Function, AelysError> {
        deserialize(avbc).map_err(|error| self.invalid_bytecode_error(error.to_string()))
    }

    fn alloc_root_function(
        &mut self,
        function: aelys_bytecode::Function,
        module_id: u64,
    ) -> Result<GcRef, AelysError> {
        self.vm
            .alloc_function_with_jit_key(function, JitFunctionKey::root(module_id))
            .map_err(AelysError::Runtime)
    }

    fn live_function_ref(&mut self, instance: &ModuleInstance) -> Result<GcRef, AelysError> {
        let reference = instance.function_ref.get();
        let alive = self
            .vm
            .heap()
            .get(reference)
            .is_some_and(|object| matches!(object.kind, ObjectKind::Function(_)));
        if alive {
            return Ok(reference);
        }
        let function = self.deserialize_root(&instance.avbc)?;
        let reference = self.alloc_root_function(function, instance.module_id)?;
        instance.function_ref.set(reference);
        Ok(reference)
    }

    fn run_instance(
        &mut self,
        instance: &ModuleInstance,
        jit_result: RootJitResult,
        options: RunOptions,
    ) -> Result<ExecutionOutcome, AelysError> {
        let global_layout = Arc::clone(&instance.global_layout);
        self.vm.set_source(Arc::clone(&instance.source));
        let function_ref = self.live_function_ref(instance)?;
        let result = match jit_result {
            RootJitResult::Deoptimized {
                bytecode_ip,
                registers,
            } => self
                .vm
                .execute_deoptimized(function_ref, bytecode_ip, registers),
            RootJitResult::Unsupported | RootJitResult::Returned(_) => {
                self.vm.execute(function_ref)
            }
        };
        if result.is_ok() {
            self.vm.sync_globals_to_hashmap(global_layout.names());
        }
        self.module_globals = self
            .vm
            .global_names()
            .into_iter()
            .filter(|name| !self.persistent_globals.contains(name))
            .collect();
        self.active_module_root = Some(instance.root.clone());
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
                source: instance.source.name.clone(),
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

    pub fn get_function(&self, name: &str) -> Result<CallableFunction, AelysError> {
        aelys_driver::get_function(&self.vm, name)
    }

    pub fn call(
        &mut self,
        function: &CallableFunction,
        args: &[Value],
    ) -> Result<Value, AelysError> {
        self.call_with_options(function, args, RunOptions::default())
    }

    /// bounded or reporting module load cannot silently affect every later
    pub fn call_with_options(
        &mut self,
        function: &CallableFunction,
        args: &[Value],
        options: RunOptions,
    ) -> Result<Value, AelysError> {
        self.configure_run(&options);
        let result = function.call(&mut self.vm, args);
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
                source: self.vm.source().name.clone(),
                random_seed: self.vm.random_seed(),
            });
        }
        result
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
                ObjectKind::Range(_) => Err(StructuredCloneError::Unsupported("range")),
                ObjectKind::Sum(_) => Err(StructuredCloneError::Unsupported("sum")),
                ObjectKind::Enum(_) => Err(StructuredCloneError::Unsupported("enum")),
                ObjectKind::Struct(_) => Err(StructuredCloneError::StructValuesUnsupported),
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

    fn reject_foreign_module(&self, module: &CompiledModule) -> Result<(), AelysError> {
        if let Some(isolate) = module.linked_isolate
            && isolate != self.id
        {
            return Err(self.invalid_bytecode_error(
                "module was compiled for a different isolate".to_string(),
            ));
        }
        Ok(())
    }

    fn foreign_instance_error(&self) -> AelysError {
        AelysError::Runtime(RuntimeError::new(
            RuntimeErrorKind::InvalidMemoryHandle,
            Vec::new(),
            Arc::clone(self.vm.source()),
        ))
    }

    fn invalid_bytecode_error(&self, message: String) -> AelysError {
        AelysError::Runtime(aelys_common::error::RuntimeError::new(
            RuntimeErrorKind::InvalidBytecode(message),
            Vec::new(),
            Arc::clone(self.vm.source()),
        ))
    }
}
