use super::{Function, GcRef, ObjectKind, VM, Value};
use crate::{JitCallResult, JitExecutor, JitFunctionKey};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::sync::Arc;

impl VM {
    pub fn configure_jit(&mut self, executor: Option<Arc<dyn JitExecutor>>) {
        self.jit_executor = executor;
        self.jit_call_counts.clear();
    }

    pub fn alloc_function_with_jit_key(
        &mut self,
        function: Function,
        key: JitFunctionKey,
    ) -> Result<GcRef, RuntimeError> {
        let reference = self.alloc_function(function)?;
        self.jit_function_keys.insert(reference, key);
        Ok(reference)
    }

    pub(crate) fn inherit_jit_key(&mut self, parent: GcRef, child: GcRef, index: usize) {
        let Some(parent) = self.jit_function_keys.get(&parent).cloned() else {
            return;
        };
        let Ok(index) = u32::try_from(index) else {
            return;
        };
        self.jit_function_keys.insert(child, parent.child(index));
    }

    pub(crate) fn try_execute_jit_call(
        &mut self,
        function: GcRef,
        arguments: &[Value],
    ) -> Option<Value> {
        if self.execution_control_enabled() {
            return None;
        }
        let executor = self.jit_executor.as_ref()?.clone();
        let key = self.jit_function_keys.get(&function)?.clone();
        let calls = self.jit_call_counts.get(&key).copied().unwrap_or(0);
        let object = self.heap.get(function)?;
        let ObjectKind::Function(function) = &object.kind else {
            return None;
        };
        match executor.try_execute(&key, &function.function, arguments, calls) {
            JitCallResult::Unsupported => None,
            JitCallResult::Returned(value) => Some(value),
        }
    }

    pub(crate) fn prepare_jit_call(&mut self, function: GcRef) -> bool {
        let Some(executor) = self.jit_executor.as_ref().cloned() else {
            return false;
        };
        let Some(key) = self.jit_function_keys.get(&function).cloned() else {
            return false;
        };
        let calls = if let Some(calls) = self.jit_call_counts.get_mut(&key) {
            *calls = calls.saturating_add(1);
            *calls
        } else {
            self.jit_call_counts.insert(key.clone(), 1);
            1
        };
        executor.should_execute(&key, calls)
    }

    #[inline(never)]
    pub(crate) fn try_execute_jit_register_call(
        &mut self,
        function: GcRef,
        argument_start: usize,
        argument_count: u16,
    ) -> Result<Option<Value>, RuntimeError> {
        if !self.prepare_jit_call(function) {
            return Ok(None);
        }
        let argument_end = argument_start
            .checked_add(usize::from(argument_count))
            .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
        let arguments = self
            .registers
            .get(argument_start..argument_end)
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::InvalidRegister {
                    reg: argument_end.saturating_sub(1),
                    max: self.registers.len().saturating_sub(1),
                })
            })?
            .to_vec();
        Ok(self.try_execute_jit_call(function, &arguments))
    }

    pub(crate) fn sweep_jit_metadata(&mut self) {
        let heap = &self.heap;
        self.jit_function_keys
            .retain(|reference, _| heap.get(*reference).is_some());
    }
}
