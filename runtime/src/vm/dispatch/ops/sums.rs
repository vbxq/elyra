use super::super::decode::decode_abc;
use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{ObjectKind, VM, Value};
use aelys_bytecode::object::SumTag;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

const NONE_TAG: u8 = 4;

impl VM {
    pub(in crate::vm::dispatch) fn sum_unwrap_error(
        &self,
        family_code: u8,
        message: Option<Value>,
    ) -> RuntimeError {
        let family = match family_code {
            1 => "Option",
            2 => "Result",
            _ => "sum",
        };
        let message = message.and_then(|value| {
            value.as_ptr().and_then(|pointer| {
                self.heap
                    .get(aelys_bytecode::GcRef::new(pointer))
                    .and_then(|object| match &object.kind {
                        ObjectKind::String(string) => Some(string.as_str().to_string()),
                        _ => None,
                    })
            })
        });
        self.runtime_error(RuntimeErrorKind::SumUnwrapFailed { family, message })
    }

    #[inline(always)]
    pub(in crate::vm::dispatch) fn execute_sum(
        &mut self,
        state: &DispatchState,
        instruction_pointer: &mut usize,
        opcode_byte: u8,
        instr: u32,
    ) -> Result<DispatchControl, RuntimeError> {
        let ip = *instruction_pointer;
        let base = state.base;
        let (a, b, c) = decode_abc(instr);
        let read = |vm: &mut VM, index: usize| state.read_register(vm, index, ip);
        let write =
            |vm: &mut VM, index: usize, value: Value| state.write_register(vm, index, value, ip);

        match opcode_byte {
            185 => write(self, base + usize::from(a), Value::unit())?,
            186 => write(self, base + usize::from(a), Value::none())?,
            187 => {
                let tag = SumTag::from_u8(c).ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                        "invalid sum tag {c}"
                    )))
                })?;
                let payload = read(self, base + usize::from(b))?;
                let sum = self.alloc_sum(tag, payload)?;
                write(self, base + usize::from(a), Value::ptr(sum.index()))?;
            }
            188 => {
                let value = read(self, base + usize::from(b))?;
                let matches = if c == NONE_TAG {
                    value.is_none()
                } else {
                    let expected = SumTag::from_u8(c).ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "invalid sum tag {c}"
                        )))
                    })?;
                    match value.as_ptr() {
                        Some(pointer) => match self.heap.get(aelys_bytecode::GcRef::new(pointer)) {
                            Some(object) => match &object.kind {
                                ObjectKind::Sum(sum) => sum.tag == expected,
                                _ => false,
                            },
                            None => {
                                return Err(self.runtime_error(RuntimeErrorKind::UseAfterFree));
                            }
                        },
                        None => false,
                    }
                };
                write(self, base + usize::from(a), Value::bool(matches))?;
            }
            189 => {
                let value = read(self, base + usize::from(b))?;
                let pointer = value.as_ptr().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "sum payload",
                        expected: "sum",
                        got: value.type_name().to_string(),
                    })
                })?;
                let object = self
                    .heap
                    .get(aelys_bytecode::GcRef::new(pointer))
                    .ok_or_else(|| self.runtime_error(RuntimeErrorKind::UseAfterFree))?;
                let ObjectKind::Sum(sum) = &object.kind else {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "sum payload",
                        expected: "sum",
                        got: value.type_name().to_string(),
                    }));
                };
                write(self, base + usize::from(a), sum.payload)?;
            }
            190 => {
                if a == 0 {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "match reached an invalid runtime value".to_string(),
                    )));
                }
                let message = if c == 1 {
                    Some(read(self, base + usize::from(b))?)
                } else {
                    None
                };
                return Err(self.sum_unwrap_error(a, message));
            }
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidOpcode {
                    opcode: opcode_byte,
                }));
            }
        }

        *instruction_pointer = ip;
        Ok(DispatchControl::Continue)
    }
}
