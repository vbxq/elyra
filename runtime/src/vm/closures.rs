use super::VM;
use super::{AelysUpvalue, GcObject, GcRef, ObjectKind, UpvalueLocation, Value};
use aelys_common::error::RuntimeError;

impl VM {
    pub fn close_upvalues_from(&mut self, start_reg: usize) {
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let upval_ref = self.open_upvalues[i];
            let should_close = if let Some(obj) = self.heap.get(upval_ref)
                && let ObjectKind::Upvalue(upval) = &obj.kind
                && let UpvalueLocation::Open {
                    frame_base,
                    register,
                } = upval.location
            {
                frame_base + register as usize >= start_reg
            } else {
                false
            };

            if should_close {
                let value = self.get_upvalue_value(upval_ref);
                if let Some(obj) = self.heap.get_mut(upval_ref)
                    && let ObjectKind::Upvalue(upval) = &mut obj.kind
                {
                    upval.location = UpvalueLocation::Closed(value);
                }
                self.heap.write_barrier_value(upval_ref, value);
                self.open_upvalues.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn capture_upvalue(
        &mut self,
        frame_base: usize,
        register: u16,
    ) -> Result<GcRef, RuntimeError> {
        for &upval_ref in &self.open_upvalues {
            if let Some(obj) = self.heap.get(upval_ref)
                && let ObjectKind::Upvalue(upval) = &obj.kind
                && let UpvalueLocation::Open {
                    frame_base: fb,
                    register: reg,
                } = upval.location
                && fb == frame_base
                && reg == register
            {
                return Ok(upval_ref);
            }
        }

        let upvalue = AelysUpvalue::new_open(frame_base, register);
        let upval_ref = self.alloc_object(GcObject::new(ObjectKind::Upvalue(upvalue)))?;

        self.open_upvalues.push(upval_ref);

        Ok(upval_ref)
    }

    pub fn get_upvalue_value(&self, upval_ref: GcRef) -> Value {
        if let Some(obj) = self.heap.get(upval_ref)
            && let ObjectKind::Upvalue(upval) = &obj.kind
        {
            match &upval.location {
                UpvalueLocation::Open {
                    frame_base,
                    register,
                } => {
                    let idx = *frame_base + *register as usize;
                    if idx < self.registers.len() {
                        return self.registers[idx];
                    }
                }
                UpvalueLocation::Closed(value) => {
                    return *value;
                }
            }
        }
        Value::null()
    }

    pub fn set_upvalue_value(&mut self, upval_ref: GcRef, value: Value) {
        let location = if let Some(obj) = self.heap.get(upval_ref)
            && let ObjectKind::Upvalue(upval) = &obj.kind
        {
            upval.location.clone()
        } else {
            return;
        };

        match location {
            UpvalueLocation::Open {
                frame_base,
                register,
            } => {
                let idx = frame_base + register as usize;
                if idx < self.registers.len() {
                    self.registers[idx] = value;
                }
            }
            UpvalueLocation::Closed(_) => {
                if let Some(obj) = self.heap.get_mut(upval_ref)
                    && let ObjectKind::Upvalue(upval) = &mut obj.kind
                {
                    upval.location = UpvalueLocation::Closed(value);
                }
                self.heap.write_barrier_value(upval_ref, value);
            }
        }
    }

    pub fn close_upvalues(&mut self, frame_base: usize, from_reg: u16) {
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let upval_ref = self.open_upvalues[i];
            let should_close = if let Some(obj) = self.heap.get(upval_ref)
                && let ObjectKind::Upvalue(upval) = &obj.kind
                && let UpvalueLocation::Open {
                    frame_base: fb,
                    register,
                } = upval.location
            {
                fb == frame_base && register >= from_reg
            } else {
                false
            };

            if should_close {
                let value = self.get_upvalue_value(upval_ref);

                if let Some(obj) = self.heap.get_mut(upval_ref)
                    && let ObjectKind::Upvalue(upval) = &mut obj.kind
                {
                    upval.location = UpvalueLocation::Closed(value);
                }
                self.heap.write_barrier_value(upval_ref, value);

                self.open_upvalues.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }
}
