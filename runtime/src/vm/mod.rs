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
mod inline_cache;
mod ints;
mod jit;
mod jit_frame;
mod native;
mod native_registry;
mod random;
mod repl;
mod resources;
mod roots;

mod arithmetic;
pub(crate) mod byte_vec;
mod call_data;
mod calls;
mod clone;
mod closures;
pub(crate) mod collections;
mod comparison;
mod dispatch;
mod helpers;
mod verifier;

pub use aelys_bytecode::{
    AelysClosure, AelysFunction, AelysString, AelysStruct, AelysUpvalue, GcObject, GcRef,
    NativeFunction, ObjectKind, UpvalueLocation,
};
pub use aelys_bytecode::{
    BytecodeBuffer, CastTarget, Function, GlobalLayout, Heap, InstructionFormat,
    IntegerOverflowError, OpCode, UpvalueDescriptor, Value, WideRegisterOperands, decode_a,
    decode_b, decode_c,
};
pub use args::{VmArgsError, VmArgsParsed, parse_vm_args};
pub use builtins::{builtin_type, register_builtins};
pub use config::{VmConfig, VmConfigError};
pub use control::{ExecutionControl, ExecutionStats, InterruptHandle};
pub use core::{MAX_FRAMES, MAX_REGISTERS, PreludeScope, StepResult, VM};
pub use frame::CallFrame;
pub use gc::GcPauseStats;
pub use jit_frame::{
    FRAME_ENUM_INVALID, FRAME_STORAGE_BYTES, aelys_frame_call, aelys_frame_collection,
    aelys_frame_element, aelys_frame_enum, aelys_frame_fmod, aelys_frame_promote_loop,
    aelys_frame_push, aelys_frame_set_global, aelys_frame_shl_div, aelys_frame_store_reference,
    aelys_frame_struct, aelys_frame_tier_up, aelys_frame_vec_push, aelys_frame_view,
};
pub use native::{
    ByteParams, ForeignPayloadKind, ForeignReturnKind, NativeFn, NativeFunctionImpl,
    build_native_vm_api,
};
pub use roots::HostRoot;
pub use verifier::{verify_emitted_function, verify_function};
