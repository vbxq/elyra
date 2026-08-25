use super::{CallFrame, Function, GcRef, ObjectKind, VM, Value};
use crate::{JitArgument, JitCallResult, JitDeoptValue, JitExecutor, JitFunctionKey};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::sync::Arc;

pub(crate) enum JitRegisterCallResult {
    Unsupported,
    Returned(Value),
    Aborted,
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
        if self.function_jit_unsupported(function) {
            return JitCallResult::Unsupported;
        }
        let Some(executor) = self.jit_executor.as_ref().cloned() else {
            return JitCallResult::Unsupported;
        };
        let Some(key) = self.jit_function_keys.get(&function).cloned() else {
            return JitCallResult::Unsupported;
        };
        let calls = self.jit_call_counts.get(&key).copied().unwrap_or(0);
        let needs_native_globals = self.heap.get(function).is_some_and(|object| {
            matches!(&object.kind, ObjectKind::Function(bytecode_function)
                if bytecode_function
                    .function
                    .global_layout
                    .names()
                    .iter()
                    .any(|name| name.contains("::")))
        });
        let previous_mapping = self.current_global_mapping_id;
        let target_mapping = self.get_global_mapping_id(function);
        let saved_globals = (needs_native_globals && target_mapping != previous_mapping)
            .then(|| self.globals_by_index.clone());
        if needs_native_globals {
            self.prepare_globals_for_function(function);
        }
        let result = (|| {
            let context = Some(self.jit_execution_context(
                self.execution_control.report || self.execution_control_enabled(),
            ));
            let mut jit_arguments = smallvec::SmallVec::<[JitArgument<'_>; 8]>::new();
            for argument in arguments {
                if let Some(value) = argument.as_int() {
                    jit_arguments.push(JitArgument::Integer(value));
                    continue;
                }
                if let Some(value) = argument.as_float() {
                    jit_arguments.push(JitArgument::Float(value));
                    continue;
                }
                let Some(reference) = argument.as_ptr().map(GcRef::new) else {
                    return JitCallResult::Unsupported;
                };
                let Some(object) = self.heap.get(reference) else {
                    return JitCallResult::Unsupported;
                };
                match &object.kind {
                    ObjectKind::Array(array) => {
                        let Some(values) = array.data.as_ints() else {
                            return JitCallResult::Unsupported;
                        };
                        jit_arguments.push(JitArgument::IntegerArray(values));
                    }
                    ObjectKind::Vec(vector) => {
                        let Some(values) = vector.data.as_ints() else {
                            return JitCallResult::Unsupported;
                        };
                        jit_arguments.push(JitArgument::IntegerVec(values));
                    }
                    _ => return JitCallResult::Unsupported,
                };
            }
            let Some(object) = self.heap.get(function) else {
                return JitCallResult::Unsupported;
            };
            let ObjectKind::Function(bytecode_function) = &object.kind else {
                return JitCallResult::Unsupported;
            };
            executor.try_execute_with_context(
                &key,
                &bytecode_function.function,
                &jit_arguments,
                calls,
                context.as_ref(),
            )
        })();
        if let Some(globals) = saved_globals {
            self.globals_by_index = globals;
            self.current_global_mapping_id = previous_mapping;
        }
        result
    }

    pub(crate) fn prepare_jit_call(&mut self, function: GcRef) -> bool {
        if self.function_jit_unsupported(function) {
            return false;
        }
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
            JitCallResult::Aborted => JitRegisterCallResult::Aborted,
            JitCallResult::Deoptimized {
                bytecode_ip,
                registers,
            } => {
                let Some(registers) = registers
                    .into_iter()
                    .map(|(register, value)| {
                        let value = match value {
                            JitDeoptValue::Value(value) => value,
                            JitDeoptValue::Argument(index) => *arguments.get(index)?,
                        };
                        Some((register, value))
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(JitRegisterCallResult::Unsupported);
                };
                JitRegisterCallResult::Deoptimized {
                    bytecode_ip,
                    registers,
                }
            }
        })
    }

    #[inline(always)]
    pub(crate) fn record_jit_backedge(
        &mut self,
        function: GcRef,
        bytecode_ip: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        if self.function_jit_unsupported(function) {
            return Ok(None);
        }
        let initialized = self
            .frames
            .last()
            .map(|frame| frame.jit_backedges_initialized)
            .unwrap_or(false);
        if !initialized {
            let Some(key) = self.jit_function_keys.get(&function) else {
                return Ok(None);
            };
            let base = self.jit_backedge_counts.get(key).copied().unwrap_or(0);
            let Some(frame) = self.frames.last_mut() else {
                return Ok(None);
            };
            frame.jit_backedge_base = base;
            frame.jit_backedges_initialized = true;
        }
        let Some(frame) = self.frames.last_mut() else {
            return Ok(None);
        };
        frame.jit_backedges = frame.jit_backedges.saturating_add(1);
        let backedges = frame.jit_backedge_base.saturating_add(frame.jit_backedges);
        if backedges != crate::JIT_TIER1_BACKEDGE_THRESHOLD {
            return Ok(None);
        }
        frame.jit_backedge_compiled = true;
        let Some(key) = self.jit_function_keys.get(&function).cloned() else {
            return Ok(None);
        };
        if let Some(result) = self.execute_jit_osr(function, &key, bytecode_ip) {
            return match result {
                JitCallResult::Returned(value) => Ok(Some(value)),
                JitCallResult::Aborted => Err(self
                    .take_jit_control_error()
                    .unwrap_or_else(|| self.runtime_error(RuntimeErrorKind::Interrupted))),
                JitCallResult::Unsupported | JitCallResult::Deoptimized { .. } => Ok(None),
            };
        }
        self.compile_jit_backedge(function, &key, backedges);
        Ok(None)
    }

    #[inline(never)]
    fn execute_jit_osr(
        &mut self,
        function: GcRef,
        key: &JitFunctionKey,
        bytecode_ip: usize,
    ) -> Option<JitCallResult> {
        if self.function_jit_unsupported(function) {
            return None;
        }
        let executor = self.jit_executor.as_ref()?.clone();
        let context = Some(self.jit_execution_context(
            self.execution_control.report || self.execution_control_enabled(),
        ));
        let frame = self.frames.last()?;
        let end = frame
            .base
            .checked_add(usize::try_from(frame.num_registers).ok()?)?;
        let values = self.registers.get(frame.base..end)?;
        let mut registers = smallvec::SmallVec::<[JitArgument<'_>; 16]>::new();
        for value in values {
            if let Some(value) = value.as_int() {
                registers.push(JitArgument::Integer(value));
            } else if let Some(value) = value.as_float() {
                registers.push(JitArgument::Float(value));
            } else if let Some(value) = value.as_bool() {
                registers.push(JitArgument::Boolean(value));
            } else if let Some(reference) = value.as_ptr().map(GcRef::new) {
                match &self.heap.get(reference)?.kind {
                    ObjectKind::Array(array) => {
                        registers.push(JitArgument::IntegerArray(array.data.as_ints()?));
                    }
                    ObjectKind::Vec(vector) => {
                        registers.push(JitArgument::IntegerVec(vector.data.as_ints()?));
                    }
                    _ => registers.push(JitArgument::Unused),
                }
            } else {
                registers.push(JitArgument::Unused);
            }
        }
        let object = self.heap.get(function)?;
        let ObjectKind::Function(function) = &object.kind else {
            return None;
        };
        let bytecode_ip = u32::try_from(bytecode_ip).ok()?;
        Some(executor.try_execute_osr_with_context(
            key,
            &function.function,
            bytecode_ip,
            &registers,
            context.as_ref(),
        ))
    }

    fn function_jit_unsupported(&self, function: GcRef) -> bool {
        self.heap.get(function).is_some_and(|object| {
            matches!(&object.kind, ObjectKind::Function(function) if function.function.jit_unsupported_struct)
        })
    }

    pub(crate) fn finish_jit_osr(&mut self, result: Value) -> Result<Option<Value>, RuntimeError> {
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
        let switch_globals = current_globals != 0 && current_globals != caller_globals;
        if switch_globals && caller_globals != 0 {
            self.sync_current_function_globals();
        }
        self.pop_frame_with_jit_metadata();
        let Some(caller) = self.frames.last() else {
            return Ok(Some(result));
        };
        let caller_function = caller.function;
        let destination = caller
            .base
            .checked_add(usize::from(destination))
            .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
        if switch_globals && caller_globals != 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JitArgument;
    use aelys_bytecode::object::AelysArray;
    use aelys_syntax::Source;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct ArrayExecutor {
        observed: Arc<AtomicBool>,
    }

    impl JitExecutor for ArrayExecutor {
        fn should_execute(&self, _key: &JitFunctionKey, _calls: u64) -> bool {
            true
        }

        fn observe_backedge(&self, _key: &JitFunctionKey, _function: &Function, _backedges: u64) {}

        fn try_execute(
            &self,
            _key: &JitFunctionKey,
            _function: &Function,
            arguments: &[JitArgument<'_>],
            _calls: u64,
        ) -> JitCallResult {
            let [JitArgument::IntegerArray(elements)] = arguments else {
                return JitCallResult::Unsupported;
            };
            self.observed.store(true, Ordering::Relaxed);
            JitCallResult::Returned(Value::int(elements[1]))
        }
    }

    #[test]
    fn jit_arguments_borrow_validated_integer_arrays() {
        let mut vm = VM::new(Source::new("jit-array", "")).unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        vm.configure_jit(Some(Arc::new(ArrayExecutor {
            observed: Arc::clone(&observed),
        })));
        let function = vm
            .alloc_function_with_jit_key(
                Function::new(Some("read_array".to_string()), 1),
                JitFunctionKey::root(1),
            )
            .unwrap();
        let array = vm.alloc_array(AelysArray::from_ints(vec![19, 42])).unwrap();

        let result = vm.try_execute_jit_call(function, &[Value::ptr(array.index())]);

        assert_eq!(result, JitCallResult::Returned(Value::int(42)));
        assert!(observed.load(Ordering::Relaxed));
    }
}
