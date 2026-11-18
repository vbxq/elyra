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
                    let truncated = value.trunc();
                    if !(truncated >= Value::INT_MIN as f64 && truncated <= Value::INT_MAX as f64) {
                        return Err(self.runtime_error(RuntimeErrorKind::IntegerOverflow));
                    }
                    return Value::int_checked(truncated as i64)
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
            CastTarget::I8
            | CastTarget::I16
            | CastTarget::I32
            | CastTarget::U8
            | CastTarget::U16
            | CastTarget::U32 => {
                let kind = target
                    .sized_kind()
                    .expect("a sized cast target names a width");
                if let Some(value) = value.as_int() {
                    return Ok(Value::int_wrapping(kind.wrap(value)));
                }
                if let Some(value) = value.as_float() {
                    if !value.is_finite() {
                        let spelling = if value.is_nan() {
                            "NaN"
                        } else if value > 0.0 {
                            "inf"
                        } else {
                            "-inf"
                        };
                        return Err(self.runtime_error(RuntimeErrorKind::NonFiniteCast {
                            value: spelling,
                            target: kind.name(),
                        }));
                    }
                    let modulus = (1u64 << kind.bits()) as f64;
                    // fmod is exact, so the low bits survive any magnitude
                    let low = value.trunc() % modulus;
                    return Ok(Value::int_wrapping(kind.wrap(low as i64)));
                }
                if let Some(value) = value.as_bool() {
                    return Ok(Value::int(i64::from(value)));
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
            CastTarget::Float => "float",
            CastTarget::Bool => "boolean",
            _ => "integer",
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
    vm.frames[current_frame_idx].ip = ip;
    let result = vm.cast_value(value, target)?;
    unsafe {
        *registers.add(dest) = result;
    }
    Ok(())
}
