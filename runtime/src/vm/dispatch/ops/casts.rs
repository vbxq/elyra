use super::super::decode::decode_abc;
use crate::vm::{CastTarget, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    pub(crate) fn cast_value(
        &mut self,
        value: Value,
        target: CastTarget,
    ) -> Result<Value, RuntimeError> {
        match target {
            CastTarget::Int => {
                if let Some(value) = value.as_int() {
                    return Value::int_checked(value)
                        .map_err(|_| self.runtime_error(RuntimeErrorKind::IntegerOverflow));
                }
                if let Some(value) = value.as_float() {
                    return Value::int_checked(value as i64)
                        .map_err(|_| self.runtime_error(RuntimeErrorKind::IntegerOverflow));
                }
                if let Some(value) = value.as_bool() {
                    return Ok(Value::int(if value { 1 } else { 0 }));
                }
            }
            CastTarget::Float => {
                if let Some(value) = value.as_float() {
                    return Ok(Value::float(value));
                }
                if let Some(value) = value.as_int() {
                    return Ok(Value::float(value as f64));
                }
                if let Some(value) = value.as_bool() {
                    return Ok(Value::float(if value { 1.0 } else { 0.0 }));
                }
            }
            CastTarget::Bool => {
                if let Some(value) = value.as_bool() {
                    return Ok(Value::bool(value));
                }
                if let Some(value) = value.as_int() {
                    return Ok(Value::bool(value != 0));
                }
                if let Some(value) = value.as_float() {
                    return Ok(Value::bool(value != 0.0));
                }
            }
        }

        let expected = match target {
            CastTarget::Int => "integer",
            CastTarget::Float => "float",
            CastTarget::Bool => "boolean",
        };
        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "as",
            expected,
            got: self.value_type_name(value).to_string(),
        }))
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn execute(
    vm: &mut VM,
    ip: usize,
    base: usize,
    current_frame_idx: usize,
    registers: *mut Value,
    registers_len: usize,
    instr: u32,
) -> Result<(), RuntimeError> {
    let (dest, source, target) = decode_abc(instr);
    let dest = base + usize::from(dest);
    let source = base + usize::from(source);
    if dest >= registers_len || source >= registers_len {
        vm.frames[current_frame_idx].ip = ip;
        return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
            reg: dest.max(source),
            max: registers_len,
        }));
    }
    let target = CastTarget::from_u8(target).ok_or_else(|| {
        vm.frames[current_frame_idx].ip = ip;
        vm.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
            "invalid cast target {target}"
        )))
    })?;
    let value = unsafe { *registers.add(source) };
    let result = vm.cast_value(value, target)?;
    unsafe {
        *registers.add(dest) = result;
    }
    Ok(())
}
