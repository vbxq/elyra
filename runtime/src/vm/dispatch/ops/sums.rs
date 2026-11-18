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
        failed: Option<Value>,
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
        let payload = failed.and_then(|value| self.failed_sum_payload(value));
        let message = match (message, payload) {
            (Some(message), Some(payload)) => Some(format!("{message}: {payload}")),
            (None, Some(payload)) => Some(format!("{family}::unwrap failed on Err: {payload}")),
            (message, None) => message,
        };
        self.runtime_error(RuntimeErrorKind::SumUnwrapFailed { family, message })
    }

    fn failed_sum_payload(&self, value: Value) -> Option<String> {
        let pointer = value.as_ptr()?;
        let object = self.heap.get(aelys_bytecode::GcRef::new(pointer))?;
        let ObjectKind::Sum(sum) = &object.kind else {
            return None;
        };
        if sum.tag != SumTag::ResultErr {
            return None;
        }
        let schemas = self.current_frame().ok().and_then(|frame| {
            match self.heap.get(frame.function).map(|o| &o.kind) {
                Some(ObjectKind::Function(function)) => Some(function.schemas()),
                Some(ObjectKind::Closure(closure)) => match self.heap.get(closure.function) {
                    Some(object) => match &object.kind {
                        ObjectKind::Function(function) => Some(function.schemas()),
                        _ => None,
                    },
                    None => None,
                },
                _ => None,
            }
        });
        let payload = sum.payload;
        if let Some(pointer) = payload.as_ptr()
            && let Some(object) = self.heap.get(aelys_bytecode::GcRef::new(pointer))
            && let ObjectKind::String(string) = &object.kind
        {
            return Some(string.as_str().to_string());
        }
        Some(self.debug_render(payload, schemas.as_deref(), 0))
    }

    fn debug_render(
        &self,
        value: Value,
        schemas: Option<&aelys_bytecode::object::FunctionSchemas>,
        depth: usize,
    ) -> String {
        if depth > 8 {
            return "..".to_string();
        }
        let Some(object) = value
            .as_ptr()
            .and_then(|pointer| self.heap.get(aelys_bytecode::GcRef::new(pointer)))
        else {
            return self.value_to_string(value);
        };
        let render_all = |values: &[Value]| {
            values
                .iter()
                .map(|slot| self.debug_render(*slot, schemas, depth + 1))
                .collect::<Vec<_>>()
        };
        match &object.kind {
            ObjectKind::String(string) => format!("{:?}", string.as_str()),
            ObjectKind::Sum(sum) => {
                let name = match sum.tag {
                    SumTag::OptionSome => "Some",
                    SumTag::ResultOk => "Ok",
                    SumTag::ResultErr => "Err",
                    SumTag::ErrorMessage => "Error",
                };
                format!(
                    "{name}({})",
                    self.debug_render(sum.payload, schemas, depth + 1)
                )
            }
            ObjectKind::Enum(value) => {
                let fields = render_all(&value.slots);
                let variant = schemas
                    .and_then(|schemas| {
                        let index = schemas
                            .enum_runtime_ids
                            .iter()
                            .position(|id| *id == value.enum_id)?;
                        schemas.enums.get(index)
                    })
                    .and_then(|schema| {
                        let variant = schema.variants.get(usize::from(value.variant_id))?;
                        let named = variant.fields.len() == fields.len()
                            && variant.fields.iter().all(|field| field.name.is_some());
                        let body = if fields.is_empty() {
                            String::new()
                        } else if named {
                            let pairs = variant
                                .fields
                                .iter()
                                .zip(&fields)
                                .map(|(field, text)| {
                                    format!("{}: {text}", field.name.as_deref().unwrap_or("_"))
                                })
                                .collect::<Vec<_>>();
                            format!(" {{ {} }}", pairs.join(", "))
                        } else {
                            format!("({})", fields.join(", "))
                        };
                        let enum_name = schema.name.rsplit("::").next().unwrap_or(&schema.name);
                        Some(format!("{enum_name}::{}{body}", variant.name))
                    });
                variant.unwrap_or_else(|| {
                    format!(
                        "enum#{}::{}({})",
                        value.enum_id,
                        value.variant_id,
                        fields.join(", ")
                    )
                })
            }
            ObjectKind::Struct(structure) => {
                let fields = render_all(&structure.slots);
                let named = schemas.and_then(|schemas| {
                    let index = schemas
                        .runtime_ids
                        .iter()
                        .position(|id| *id == structure.schema_id)?;
                    let schema = schemas.structs.get(index)?;
                    if schema.fields.len() != fields.len() {
                        return None;
                    }
                    let name = schema.ctor.display_name();
                    let name = name.rsplit("::").next().unwrap_or(&name).to_string();
                    let pairs = schema
                        .fields
                        .iter()
                        .zip(&fields)
                        .map(|(field, text)| format!("{}: {text}", field.name))
                        .collect::<Vec<_>>();
                    Some(format!("{name} {{ {} }}", pairs.join(", ")))
                });
                named.unwrap_or_else(|| self.value_to_string(value))
            }
            ObjectKind::Vec(vec) => {
                let items = (0..vec.len())
                    .filter_map(|index| vec.get(index))
                    .map(|item| self.debug_render(item, schemas, depth + 1))
                    .collect::<Vec<_>>();
                format!("vec![{}]", items.join(", "))
            }
            ObjectKind::Array(array) => {
                let items = (0..array.len())
                    .filter_map(|index| array.get(index))
                    .map(|item| self.debug_render(item, schemas, depth + 1))
                    .collect::<Vec<_>>();
                format!("[{}]", items.join(", "))
            }
            _ => self.value_to_string(value),
        }
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
                let message = if c & 1 == 1 {
                    Some(read(self, base + usize::from(b))?)
                } else {
                    None
                };
                let failed = if c & 2 == 2 {
                    Some(read(self, base + usize::from(b) + usize::from(c & 1))?)
                } else {
                    None
                };
                return Err(self.sum_unwrap_error(a, message, failed));
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
