use super::{GcRef, NativeFunctionImpl, Value};

/// Cached function call data to avoid repeated heap lookups.
pub(super) enum CallData {
    Function {
        func_ref: GcRef,
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
    },
    Native {
        arity: u16,
        func: Option<NativeFunctionImpl>,
    },
    Closure {
        inner_func_ref: GcRef,
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        upvalues_ptr: *const GcRef,
        upvalues_len: usize,
    },
}
