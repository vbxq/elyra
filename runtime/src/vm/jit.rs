use super::{CallFrame, Function, GcRef, ObjectKind, VM, Value};
use crate::{JitCallResult, JitExecutor, JitFunctionKey};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::sync::Arc;

pub(crate) enum JitRegisterCallResult {
    Unsupported,
    Returned(Value),
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, Value)>,
    },
}

impl VM {
    pub fn configure_jit(&mut self, executor: Option<Arc<dyn JitExecutor>>) {
        self.jit_executor = executor;
        self.jit_call_counts.clear();
        self.jit_backedge_counts.clear();
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
    ) -> JitCallResult {
        if self.execution_control_enabled() {
            return JitCallResult::Unsupported;
        }
        let Some(executor) = self.jit_executor.as_ref().cloned() else {
            return JitCallResult::Unsupported;
        };
        let Some(key) = self.jit_function_keys.get(&function).cloned() else {
            return JitCallResult::Unsupported;
        };
        let calls = self.jit_call_counts.get(&key).copied().unwrap_or(0);
        let Some(object) = self.heap.get(function) else {
            return JitCallResult::Unsupported;
        };
        let ObjectKind::Function(function) = &object.kind else {
            return JitCallResult::Unsupported;
        };
        executor.try_execute(&key, &function.function, arguments, calls)
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
    ) -> Result<JitRegisterCallResult, RuntimeError> {
        if !self.prepare_jit_call(function) {
            return Ok(JitRegisterCallResult::Unsupported);
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
        Ok(match self.try_execute_jit_call(function, &arguments) {
            JitCallResult::Unsupported => JitRegisterCallResult::Unsupported,
            JitCallResult::Returned(value) => JitRegisterCallResult::Returned(value),
            JitCallResult::Deoptimized {
                bytecode_ip,
                registers,
            } => JitRegisterCallResult::Deoptimized {
                bytecode_ip,
                registers,
            },
        })
    }

    #[inline(always)]
    pub(crate) fn record_jit_backedge(&mut self, function: GcRef) {
        let initialized = self
            .frames
            .last()
            .map(|frame| frame.jit_backedges_initialized)
            .unwrap_or(false);
        if !initialized {
            let Some(key) = self.jit_function_keys.get(&function) else {
                return;
            };
            let base = self.jit_backedge_counts.get(key).copied().unwrap_or(0);
            let Some(frame) = self.frames.last_mut() else {
                return;
            };
            frame.jit_backedge_base = base;
            frame.jit_backedges_initialized = true;
        }
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        frame.jit_backedges = frame.jit_backedges.saturating_add(1);
        let backedges = frame.jit_backedge_base.saturating_add(frame.jit_backedges);
        if backedges != crate::JIT_TIER1_BACKEDGE_THRESHOLD {
            return;
        }
        frame.jit_backedge_compiled = true;
        let Some(key) = self.jit_function_keys.get(&function).cloned() else {
            return;
        };
        self.compile_jit_backedge(function, &key, backedges);
    }

    #[inline(never)]
    fn compile_jit_backedge(&self, function: GcRef, key: &JitFunctionKey, backedges: u64) {
        let Some(executor) = self.jit_executor.as_ref().cloned() else {
            return;
        };
        let Some(object) = self.heap.get(function) else {
            return;
        };
        let ObjectKind::Function(function) = &object.kind else {
            return;
        };
        executor.observe_backedge(key, &function.function, backedges);
    }

    pub(crate) fn pop_frame_with_jit_metadata(&mut self) -> Option<CallFrame> {
        let frame = self.frames.pop()?;
        self.flush_jit_backedges(&frame);
        Some(frame)
    }

    pub(crate) fn reset_frame_jit_metadata(&mut self, frame_index: usize) {
        let Some(frame) = self.frames.get(frame_index).cloned() else {
            return;
        };
        self.flush_jit_backedges(&frame);
        if let Some(frame) = self.frames.get_mut(frame_index) {
            frame.jit_backedge_base = 0;
            frame.jit_backedges = 0;
            frame.jit_backedges_initialized = false;
            frame.jit_backedge_compiled = false;
        }
    }

    fn flush_jit_backedges(&mut self, frame: &CallFrame) {
        if !frame.jit_backedges_initialized || frame.jit_backedges == 0 {
            return;
        }
        let Some(key) = self.jit_function_keys.get(&frame.function).cloned() else {
            return;
        };
        let previous = self.jit_backedge_counts.get(&key).copied().unwrap_or(0);
        let backedges = previous.saturating_add(frame.jit_backedges);
        self.jit_backedge_counts.insert(key.clone(), backedges);
        if !frame.jit_backedge_compiled
            && previous < crate::JIT_TIER1_BACKEDGE_THRESHOLD
            && backedges >= crate::JIT_TIER1_BACKEDGE_THRESHOLD
        {
            self.compile_jit_backedge(frame.function, &key, backedges);
        }
    }

    pub(crate) fn apply_jit_deoptimization(
        &mut self,
        frame: &mut CallFrame,
        bytecode_ip: u32,
        registers: Vec<(u16, Value)>,
    ) -> Result<(), RuntimeError> {
        let bytecode_ip = usize::try_from(bytecode_ip).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "JIT deoptimization IP does not fit this target".to_string(),
            ))
        })?;
        if bytecode_ip >= frame.bytecode_len {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "JIT deoptimization IP is outside the function".to_string(),
            )));
        }
        for (register, value) in registers {
            if u32::from(register) >= frame.num_registers {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                    reg: usize::from(register),
                    max: usize::try_from(frame.num_registers)
                        .unwrap_or(usize::MAX)
                        .saturating_sub(1),
                }));
            }
            let index = frame
                .base
                .checked_add(usize::from(register))
                .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
            if index >= self.registers.len() {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                    reg: index,
                    max: self.registers.len().saturating_sub(1),
                }));
            }
            self.registers[index] = value;
        }
        frame.ip = bytecode_ip;
        Ok(())
    }

    pub(crate) fn sweep_jit_metadata(&mut self) {
        let heap = &self.heap;
        self.jit_function_keys
            .retain(|reference, _| heap.get(*reference).is_some());
    }
}
