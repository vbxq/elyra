use super::{CallFrame, Function, GcRef, VM, Value};
use crate::{JitExecutor, JitFunctionKey};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::sync::Arc;

impl VM {
    pub fn configure_jit(&mut self, executor: Option<Arc<dyn JitExecutor>>) {
        self.jit_executor = executor;
        self.reset_frame_jit();
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

    #[inline(always)]
    pub(crate) fn record_jit_backedge(
        &mut self,
        function: GcRef,
        bytecode_ip: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        if self
            .frames
            .last()
            .is_some_and(|frame| frame.jit_quiet_ip == bytecode_ip)
        {
            return Ok(None);
        }
        self.frame_jit_backedge(function, bytecode_ip)
            .unwrap_or(Ok(None))
    }

    pub(crate) fn finish_jit_osr(&mut self, result: Value) -> Result<Option<Value>, RuntimeError> {
        if self.take_frame_osr_resume() {
            return Ok(None);
        }
        let current = self
            .frames
            .last()
            .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
        let destination = current.return_dest();
        let current_globals = current.global_mapping_id;
        let caller_globals = self
            .frames
            .get(self.frames.len().saturating_sub(2))
            .map_or(0, |frame| frame.global_mapping_id);
        let switch_globals = current_globals != caller_globals;
        if switch_globals {
            self.sync_current_function_globals();
        }
        self.frames.pop();
        let Some(caller) = self.frames.last() else {
            return Ok(Some(result));
        };
        let caller_function = caller.function;
        let destination = caller
            .base
            .checked_add(usize::from(destination))
            .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
        if switch_globals {
            self.prepare_globals_for_function(caller_function);
        }
        if destination >= self.registers.len() {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: destination,
                max: self.registers.len().saturating_sub(1),
            }));
        }
        self.registers[destination] = result;
        Ok(None)
    }

    pub(crate) fn resume_jit_exit(
        &mut self,
        frame: &mut CallFrame,
        bytecode_ip: u32,
    ) -> Result<(), RuntimeError> {
        let bytecode_ip = usize::try_from(bytecode_ip).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "machine-code exit IP does not fit this target".to_string(),
            ))
        })?;
        if bytecode_ip >= frame.bytecode_len {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "machine-code exit IP is outside the function".to_string(),
            )));
        }
        frame.ip = bytecode_ip;
        Ok(())
    }

    pub(crate) fn sweep_jit_metadata(&mut self) {
        let heap = &self.heap;
        self.jit_function_keys
            .retain(|reference, _| heap.get(*reference).is_some());
        self.sweep_frame_jit();
    }
}
