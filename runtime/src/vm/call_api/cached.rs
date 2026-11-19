use super::super::{GcRef, NativeFunction, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    #[inline]
    pub fn call_cached_function(
        &mut self,
        func_ref: GcRef,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.call_ref(func_ref, args)
    }

    #[inline]
    pub fn call_cached_closure(
        &mut self,
        closure_ref: GcRef,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.call_ref(closure_ref, args)
    }

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

        let func = self.native_impl(native).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::UndefinedVariable(native.name.clone()))
        })?;
        func.call(self, args)
            .map_err(|error| self.name_panicking_native(error, |_| native.name.clone()))
    }
}
