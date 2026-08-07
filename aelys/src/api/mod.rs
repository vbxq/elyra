mod call;
mod native_registry;
mod repl;
mod run;
mod v2;
mod vm;

pub use call::{CallableFunction, call_function, get_function};
pub use native_registry::{NativeModuleRegistration, ValidatedNativeModule};
pub use repl::{run_with_vm, run_with_vm_and_opt};
pub use run::{run, run_source, run_with_config, run_with_config_and_opt};
pub use v2::{
    CompileOptions, CompiledModule, ExecutionOutcome, ExecutionReport, InterruptHandle, Isolate,
    IsolateConfig, JitConfig, JitConfigError, JitMode, ModuleInstance, RunOptions, Runtime,
    StructuredCloneError, StructuredValue,
};
pub use vm::{new_vm, new_vm_with_config};

pub use aelys_runtime::{VM, Value};
