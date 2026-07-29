mod alloc;
mod args;
mod builtins;
mod call_api;
mod config;
mod config_access;
mod control;
mod core;
mod errors;
mod execute;
mod frame;
mod frames;
mod gc;
mod globals;
mod init;
mod native;
mod native_registry;
mod random;
mod repl;
mod resources;

// Implementation modules (extend VM with impl blocks)
mod arithmetic;
mod call_data;
mod calls;
mod closures;
mod comparison;
mod dispatch;
mod helpers;
mod verifier;

pub use aelys_bytecode::{
    AelysClosure, AelysFunction, AelysString, AelysUpvalue, GcObject, GcRef, NativeFunction,
    ObjectKind, UpvalueLocation,
};
pub use aelys_bytecode::{
    BytecodeBuffer, Function, GlobalLayout, Heap, InstructionFormat, IntegerOverflowError, OpCode,
    UpvalueDescriptor, Value, WideRegisterOperands, decode_a, decode_b, decode_c,
};
pub use args::{VmArgsError, VmArgsParsed, parse_vm_args};
pub use builtins::{builtin_type, register_builtins};
pub use config::{VmConfig, VmConfigError};
pub use control::{ExecutionControl, ExecutionStats, InterruptHandle};
pub use core::{MAX_FRAMES, MAX_REGISTERS, StepResult, VM};
pub use frame::CallFrame;
pub use native::{NativeFn, NativeFunctionImpl, build_native_vm_api};
