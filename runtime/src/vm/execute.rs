use super::{CallFrame, GcRef, ObjectKind, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

impl VM {
    pub fn execute(&mut self, function: GcRef) -> Result<Value, RuntimeError> {
        self.execute_with_deoptimization(function, None)
    }

    pub fn execute_deoptimized(
        &mut self,
        function: GcRef,
        bytecode_ip: u32,
        registers: Vec<(u16, Value)>,
    ) -> Result<Value, RuntimeError> {
        self.execute_with_deoptimization(function, Some((bytecode_ip, registers)))
    }

    fn execute_with_deoptimization(
        &mut self,
        function: GcRef,
        deoptimization: Option<(u32, Vec<(u16, Value)>)>,
    ) -> Result<Value, RuntimeError> {
        match catch_unwind(AssertUnwindSafe(|| {
            self.execute_inner(function, deoptimization)
        })) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                self.abort_execution();
                Err(error)
            }
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                let kind = if message.starts_with("integer ")
                    && message.contains("outside the supported range")
                {
                    RuntimeErrorKind::IntegerOverflow
                } else {
                    RuntimeErrorKind::RuntimePanic { message }
                };
                let error = self.runtime_error(kind);
                self.abort_execution();
                Err(error)
            }
        }
    }

    fn abort_execution(&mut self) {
        self.sync_current_function_globals();
        self.close_upvalues_from(0);
        self.frames.clear();
        self.open_upvalues.clear();
        self.current_upvalues.clear();
        self.current_global_mapping_id = 0;
    }

    fn execute_inner(
        &mut self,
        function: GcRef,
        deoptimization: Option<(u32, Vec<(u16, Value)>)>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_function_verified(function)?;
        let (
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            num_registers,
            global_layout,
            global_mapping_id,
        ) = match self.heap.get(function) {
            Some(obj) => match &obj.kind {
                ObjectKind::Function(f) => {
                    let bc = &f.function.bytecode;
                    let consts = &f.constants;
                    let layout = Arc::clone(&f.function.global_layout);
                    let gmap_id = self.global_mapping_id_for_layout(&layout);
                    (
                        bc.as_ptr(),
                        bc.len(),
                        consts.as_ptr(),
                        consts.len(),
                        f.num_registers(),
                        layout,
                        gmap_id,
                    )
                }
                _ => {
                    return Err(self
                        .runtime_error(RuntimeErrorKind::NotCallable("non-function".to_string())));
                }
            },
            None => {
                return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                    "invalid reference".to_string(),
                )));
            }
        };

        if !global_layout.names().is_empty() {
            let needed_len = global_layout.names().len();
            if self.globals_by_index.len() < needed_len {
                self.globals_by_index.resize(needed_len, Value::null());
            }
            for (idx, name) in global_layout.names().iter().enumerate() {
                if !name.is_empty() {
                    self.globals_by_index[idx] =
                        self.globals.get(name).copied().unwrap_or(Value::null());
                } else {
                    self.globals_by_index[idx] = Value::null();
                }
            }
            self.bump_global_generations(needed_len);
        }

        self.current_global_mapping_id = global_mapping_id;

        let mut frame = CallFrame::new(
            function,
            0,
            bytecode_ptr,
            bytecode_len,
            constants_ptr,
            constants_len,
            num_registers,
        );
        let register_count = usize::try_from(num_registers).map_err(|_| {
            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "register count does not fit this target".to_string(),
            ))
        })?;
        if self.registers.len() < register_count {
            self.registers.resize(register_count, Value::null());
        }
        if let Some((bytecode_ip, registers)) = deoptimization {
            self.apply_jit_deoptimization(&mut frame, bytecode_ip, registers)?;
        }
        frame.global_mapping_id = global_mapping_id;
        self.push_frame(frame)?;
        self.run_fast()
    }

    pub(crate) fn ensure_function_verified(&mut self, func_ref: GcRef) -> Result<(), RuntimeError> {
        let needs_verify = match self.heap.get(func_ref) {
            Some(obj) => match &obj.kind {
                ObjectKind::Function(f) => !f.verified,
                _ => {
                    return Err(self
                        .runtime_error(RuntimeErrorKind::NotCallable("non-function".to_string())));
                }
            },
            None => {
                return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                    "invalid reference".to_string(),
                )));
            }
        };

        if needs_verify {
            let func = match self.heap.get(func_ref) {
                Some(obj) => match &obj.kind {
                    ObjectKind::Function(f) => &f.function,
                    _ => {
                        return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                            "non-function".to_string(),
                        )));
                    }
                },
                None => {
                    return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                        "invalid reference".to_string(),
                    )));
                }
            };

            super::verifier::verify_function(func, &self.heap, 0)
                .map_err(|msg| self.runtime_error(RuntimeErrorKind::InvalidBytecode(msg)))?;

            if let Some(obj) = self.heap.get_mut(func_ref)
                && let ObjectKind::Function(f) = &mut obj.kind
            {
                f.verified = true;
            }
        }

        Ok(())
    }

    pub(crate) fn verify_function_value(&self, func: &super::Function) -> Result<(), RuntimeError> {
        super::verifier::verify_function(func, &self.heap, 0)
            .map_err(|msg| self.runtime_error(RuntimeErrorKind::InvalidBytecode(msg)))
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else {
        "non-string panic payload".to_string()
    }
}
