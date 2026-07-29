use super::super::{CallFrame, GcRef, NativeFunction, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    /// Ultra-fast function call using pre-cached metadata.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn call_cached_function(
        &mut self,
        func_ref: GcRef,
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        args: &[Value],
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

        let needed = num_registers as usize;
        if needed > self.registers.len() {
            self.registers.resize(needed, Value::null());
        }

        for (i, arg) in args.iter().enumerate() {
            self.registers[i] = *arg;
        }

        self.prepare_globals_for_function(func_ref);

        let frame = CallFrame::new(
            func_ref,
            0usize,
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            num_registers,
        );
        self.push_frame(frame)?;

        self.run_fast()
    }

    /// Ultra-fast closure call using pre-cached metadata.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn call_cached_closure(
        &mut self,
        func_ref: GcRef,
        arity: u16,
        num_registers: u32,
        bytecode_ptr: *const u32,
        bytecode_len: usize,
        constants_ptr: *const Value,
        constants_len: usize,
        upvalues_ptr: *const GcRef,
        upvalues_len: usize,
        args: &[Value],
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

        let needed = num_registers as usize;
        if needed > self.registers.len() {
            self.registers.resize(needed, Value::null());
        }

        for (i, arg) in args.iter().enumerate() {
            self.registers[i] = *arg;
        }

        self.prepare_globals_for_function(func_ref);

        let frame = CallFrame::with_upvalues(
            func_ref,
            0,
            0u16,
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            upvalues_ptr,
            upvalues_len,
            num_registers,
        );
        self.push_frame(frame)?;

        self.run_fast()
    }

    /// Call a registered native function by name.
    #[inline]
    pub fn call_cached_native(
        &mut self,
        native: &NativeFunction,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let nargs = u16::try_from(args.len()).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
                count: args.len(),
                max: u16::MAX,
            })
        })?;
        if native.arity != nargs {
            return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                expected: native.arity,
                got: nargs,
            }));
        }

        let func = self
            .native_registry
            .get(&native.name)
            .copied()
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::UndefinedVariable(native.name.clone()))
            })?;
        func.call(self, args)
    }
}
