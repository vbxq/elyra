use super::super::{CallFrame, GcRef, NativeFunction, VM, Value};
use crate::JitCallResult;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::sync::Arc;

pub(super) enum FuncKind {
    Function {
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        global_layout: Arc<super::super::GlobalLayout>,
    },
    Native {
        native: NativeFunction,
    },
    Closure {
        inner_func_ref: GcRef,
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        upvalues: Vec<GcRef>,
        global_layout: Arc<super::super::GlobalLayout>,
    },
}

impl VM {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call_function_kind(
        &mut self,
        func_ref: GcRef,
        args: &[Value],
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        global_layout: Arc<super::super::GlobalLayout>,
    ) -> Result<Value, RuntimeError> {
        let nargs = u16::try_from(args.len()).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
                count: args.len(),
                max: u16::MAX,
            })
        })?;
        if arity != nargs {
            return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                expected: arity,
                got: nargs,
            }));
        }

        self.ensure_function_verified(func_ref)?;

        // Host-resolved callables are the hot entry point for embedders. Give
        // them the same JIT opportunity as a bytecode Call. A deoptimization
        // simply falls through to the interpreter from the function entry;
        // no interpreter frame has been published yet, so that is an exact
        // and safe fallback.
        if self.prepare_jit_call(func_ref) {
            match self.try_execute_jit_call(func_ref, args) {
                JitCallResult::Returned(value) => return Ok(value),
                JitCallResult::Aborted => {
                    return Err(self
                        .take_jit_control_error()
                        .unwrap_or_else(|| self.runtime_error(RuntimeErrorKind::Interrupted)));
                }
                JitCallResult::Unsupported | JitCallResult::Deoptimized { .. } => {}
            }
        }

        let needed = num_registers as usize;
        if needed > self.registers.len() {
            self.registers.resize(needed, Value::null());
        }

        for (i, arg) in args.iter().enumerate() {
            self.registers[i] = *arg;
        }

        let gmap_id = self.global_mapping_id_for_layout(&global_layout);
        self.prepare_globals_for_function(func_ref);

        let mut frame = CallFrame::new(
            func_ref,
            0usize,
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            num_registers,
        );
        frame.global_mapping_id = gmap_id;
        self.push_frame(frame)?;

        self.run_fast()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn call_closure_kind(
        &mut self,
        inner_func_ref: GcRef,
        args: &[Value],
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        upvalues: Vec<GcRef>,
        global_layout: Arc<super::super::GlobalLayout>,
    ) -> Result<Value, RuntimeError> {
        let nargs = u16::try_from(args.len()).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
                count: args.len(),
                max: u16::MAX,
            })
        })?;
        if arity != nargs {
            return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                expected: arity,
                got: nargs,
            }));
        }

        self.ensure_function_verified(inner_func_ref)?;

        let needed = num_registers as usize;
        if needed > self.registers.len() {
            self.registers.resize(needed, Value::null());
        }

        for (i, arg) in args.iter().enumerate() {
            self.registers[i] = *arg;
        }

        self.current_upvalues = upvalues;

        let gmap_id = self.global_mapping_id_for_layout(&global_layout);
        self.prepare_globals_for_function(inner_func_ref);

        let mut frame = CallFrame::with_upvalues(
            inner_func_ref,
            0,
            0u16,
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            self.current_upvalues.as_ptr(),
            self.current_upvalues.len(),
            num_registers,
        );
        frame.global_mapping_id = gmap_id;
        self.push_frame(frame)?;

        self.run_fast()
    }
}
