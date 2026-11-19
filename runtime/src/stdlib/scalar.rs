use crate::stdlib::helpers::{get_int, get_string, make_string, option_some};
use crate::vm::{VM, Value};
use aelys_bytecode::{SizedInt, wrapping_sized_neg};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

type Native = fn(&mut VM, &[Value]) -> Result<Value, RuntimeError>;

pub(crate) const STRING_PARSERS: &[(&str, u16, Native)] = &[
    ("to_int", 1, native_parse_int),
    ("to_int_radix", 2, native_parse_int_radix),
    ("to_float", 1, native_parse_float),
    ("to_bool", 1, native_parse_bool),
];

pub(crate) const INT_FORMATTERS: &[(&str, u16, Native)] = &[
    ("__int_to_hex", 1, native_to_hex),
    ("__int_to_binary", 1, native_to_binary),
    ("__int_to_octal", 1, native_to_octal),
    ("__int_to_radix", 2, native_to_radix),
    ("__number_abs", 2, native_number_abs),
];

// the kind operand is a SizedInt code, -1 for the checked int, -2 for a float
fn native_number_abs(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let kind = args[1].as_int().unwrap_or(-1);
    if kind == -2 {
        let value = args[0].as_float().unwrap_or(f64::NAN);
        return Ok(Value::float(value.abs()));
    }
    match u8::try_from(kind).ok().and_then(SizedInt::from_u8) {
        Some(sized) if !sized.signed() => Ok(args[0]),
        Some(sized) => {
            let value = get_wide_int(vm, args[0], "abs")?;
            let value = i64::try_from(value).unwrap_or(i64::MIN);
            if value < 0 {
                vm.make_int(wrapping_sized_neg(sized, value))
            } else {
                Ok(args[0])
            }
        }
        None => match get_wide_int(vm, args[0], "abs")?.checked_abs() {
            Some(abs) if abs <= i128::from(i64::MAX) => vm.make_wide_int(abs),
            _ => Err(vm.runtime_error(RuntimeErrorKind::IntegerOverflow)),
        },
    }
}

fn convert_error(vm: &VM, op: &'static str, msg: String) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::TypeError {
        operation: op,
        expected: "valid conversion",
        got: msg,
    })
}

fn parse_int_text(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (radix, digits) = match unsigned.get(..2) {
        Some("0x" | "0X") => (16, &unsigned[2..]),
        Some("0o" | "0O") => (8, &unsigned[2..]),
        Some("0b" | "0B") => (2, &unsigned[2..]),
        _ => (10, unsigned),
    };
    parse_digits(digits, radix, negative)
}

fn parse_digits(digits: &str, radix: u32, negative: bool) -> Option<i64> {
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    let magnitude = i128::from(u64::from_str_radix(digits, radix).ok()?);
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).ok()
}

fn native_parse_int(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let s = get_string(vm, args[0], "string.to_int")?;
    match parse_int_text(s) {
        Some(n) => {
            let value = vm.make_int(n)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

fn native_parse_int_radix(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let s = get_string(vm, args[0], "string.to_int_radix")?;
    let radix = get_int(vm, args[1], "string.to_int_radix")?;

    if !(2..=36).contains(&radix) {
        return Err(convert_error(
            vm,
            "string.to_int_radix",
            format!("radix must be 2-36, got {}", radix),
        ));
    }

    let radix = u32::try_from(radix).expect("radix was range checked");
    let trimmed = s.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    match parse_digits(digits, radix, negative) {
        Some(n) => {
            let value = vm.make_int(n)?;
            option_some(vm, value)
        }
        None => Ok(Value::none()),
    }
}

fn native_parse_float(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let s = get_string(vm, args[0], "string.to_float")?;
    match s.trim().parse::<f64>() {
        Ok(value) => option_some(vm, Value::float(value)),
        Err(_) => Ok(Value::none()),
    }
}

fn native_parse_bool(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let s = get_string(vm, args[0], "string.to_bool")?;
    match s.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => option_some(vm, Value::bool(true)),
        "false" | "0" | "no" | "off" => option_some(vm, Value::bool(false)),
        _ => Ok(Value::none()),
    }
}

fn get_wide_int(vm: &VM, value: Value, op: &'static str) -> Result<i128, RuntimeError> {
    vm.int_value(value).ok_or_else(|| {
        vm.runtime_error(RuntimeErrorKind::TypeError {
            operation: op,
            expected: "int",
            got: vm.value_type_name(value).to_string(),
        })
    })
}

fn native_to_hex(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let n = get_wide_int(vm, args[0], "int.to_hex")?;
    let s = if n < 0 {
        format!("-{:x}", n.unsigned_abs())
    } else {
        format!("{:x}", n)
    };
    make_string(vm, &s)
}

fn native_to_binary(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let n = get_wide_int(vm, args[0], "int.to_binary")?;
    make_string(
        vm,
        &if n >= 0 {
            format!("{:b}", n)
        } else {
            format!("-{:b}", n.unsigned_abs())
        },
    )
}

fn native_to_octal(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let n = get_wide_int(vm, args[0], "int.to_octal")?;
    if n >= 0 {
        make_string(vm, &format!("{:o}", n))
    } else {
        make_string(vm, &format!("-{:o}", n.unsigned_abs()))
    }
}

fn native_to_radix(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let n = get_wide_int(vm, args[0], "int.to_radix")?;
    let radix = get_int(vm, args[1], "int.to_radix")?;

    if !(2..=36).contains(&radix) {
        return Err(convert_error(
            vm,
            "int.to_radix",
            format!("radix must be 2-36, got {}", radix),
        ));
    }

    let digits = "0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = String::new();
    let mut num = n.unsigned_abs();
    let radix = radix as u128;

    if num == 0 {
        return make_string(vm, "0");
    }

    while num > 0 {
        let digit = (num % radix) as usize;
        let ch = digits.as_bytes().get(digit).copied().ok_or_else(|| {
            convert_error(vm, "int.to_radix", format!("digit out of range: {}", digit))
        })?;
        result.insert(0, ch as char);
        num /= radix;
    }

    if n < 0 {
        result.insert(0, '-');
    }

    make_string(vm, &result)
}
