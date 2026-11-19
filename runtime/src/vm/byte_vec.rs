use super::{GcRef, ObjectKind, VM, Value};
use aelys_bytecode::object::{AelysVec, SumTag, VecData};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

use Order::{Big, Little};
use Sign::{Signed, Unsigned};

type Native = fn(&mut VM, &[Value]) -> Result<Value, RuntimeError>;

macro_rules! read_native {
    ($function:ident, $operation:literal, $width:literal, $sign:expr, $order:expr) => {
        fn $function(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
            read_int(vm, args, $operation, $width, $sign, $order)
        }
    };
}

macro_rules! write_native {
    ($function:ident, $operation:literal, $width:literal, $sign:expr, $order:expr) => {
        fn $function(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
            write_int(vm, args, $operation, $width, $sign, $order)
        }
    };
}

macro_rules! read_float_native {
    ($function:ident, $operation:literal, $width:literal, $order:expr) => {
        fn $function(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
            read_float(vm, args, $operation, $width, $order)
        }
    };
}

macro_rules! write_float_native {
    ($function:ident, $operation:literal, $width:literal, $order:expr) => {
        fn $function(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
            write_float(vm, args, $operation, $width, $order)
        }
    };
}

read_native!(read_u8, "read_u8", 1, Unsigned, Little);
write_native!(write_u8, "write_u8", 1, Unsigned, Little);
read_native!(read_i8, "read_i8", 1, Signed, Little);
write_native!(write_i8, "write_i8", 1, Signed, Little);
read_native!(read_u16_le, "read_u16_le", 2, Unsigned, Little);
write_native!(write_u16_le, "write_u16_le", 2, Unsigned, Little);
read_native!(read_u16_be, "read_u16_be", 2, Unsigned, Big);
write_native!(write_u16_be, "write_u16_be", 2, Unsigned, Big);
read_native!(read_i16_le, "read_i16_le", 2, Signed, Little);
write_native!(write_i16_le, "write_i16_le", 2, Signed, Little);
read_native!(read_i16_be, "read_i16_be", 2, Signed, Big);
write_native!(write_i16_be, "write_i16_be", 2, Signed, Big);
read_native!(read_u32_le, "read_u32_le", 4, Unsigned, Little);
write_native!(write_u32_le, "write_u32_le", 4, Unsigned, Little);
read_native!(read_u32_be, "read_u32_be", 4, Unsigned, Big);
write_native!(write_u32_be, "write_u32_be", 4, Unsigned, Big);
read_native!(read_i32_le, "read_i32_le", 4, Signed, Little);
write_native!(write_i32_le, "write_i32_le", 4, Signed, Little);
read_native!(read_i32_be, "read_i32_be", 4, Signed, Big);
write_native!(write_i32_be, "write_i32_be", 4, Signed, Big);
read_native!(read_u64_le, "read_u64_le", 8, Unsigned, Little);
write_native!(write_u64_le, "write_u64_le", 8, Unsigned, Little);
read_native!(read_u64_be, "read_u64_be", 8, Unsigned, Big);
write_native!(write_u64_be, "write_u64_be", 8, Unsigned, Big);
read_native!(read_i64_le, "read_i64_le", 8, Signed, Little);
write_native!(write_i64_le, "write_i64_le", 8, Signed, Little);
read_native!(read_i64_be, "read_i64_be", 8, Signed, Big);
write_native!(write_i64_be, "write_i64_be", 8, Signed, Big);
read_float_native!(read_f32_le, "read_f32_le", 4, Little);
write_float_native!(write_f32_le, "write_f32_le", 4, Little);
read_float_native!(read_f32_be, "read_f32_be", 4, Big);
write_float_native!(write_f32_be, "write_f32_be", 4, Big);
read_float_native!(read_f64_le, "read_f64_le", 8, Little);
write_float_native!(write_f64_le, "write_f64_le", 8, Little);
read_float_native!(read_f64_be, "read_f64_be", 8, Big);
write_float_native!(write_f64_be, "write_f64_be", 8, Big);

pub(crate) const BYTE_VEC_NATIVES: &[(&str, u16, Native)] = &[
    ("__vec_read_u8", 2, read_u8),
    ("__vec_write_u8", 3, write_u8),
    ("__vec_read_i8", 2, read_i8),
    ("__vec_write_i8", 3, write_i8),
    ("__vec_read_u16_le", 2, read_u16_le),
    ("__vec_write_u16_le", 3, write_u16_le),
    ("__vec_read_u16_be", 2, read_u16_be),
    ("__vec_write_u16_be", 3, write_u16_be),
    ("__vec_read_i16_le", 2, read_i16_le),
    ("__vec_write_i16_le", 3, write_i16_le),
    ("__vec_read_i16_be", 2, read_i16_be),
    ("__vec_write_i16_be", 3, write_i16_be),
    ("__vec_read_u32_le", 2, read_u32_le),
    ("__vec_write_u32_le", 3, write_u32_le),
    ("__vec_read_u32_be", 2, read_u32_be),
    ("__vec_write_u32_be", 3, write_u32_be),
    ("__vec_read_i32_le", 2, read_i32_le),
    ("__vec_write_i32_le", 3, write_i32_le),
    ("__vec_read_i32_be", 2, read_i32_be),
    ("__vec_write_i32_be", 3, write_i32_be),
    ("__vec_read_u64_le", 2, read_u64_le),
    ("__vec_write_u64_le", 3, write_u64_le),
    ("__vec_read_u64_be", 2, read_u64_be),
    ("__vec_write_u64_be", 3, write_u64_be),
    ("__vec_read_i64_le", 2, read_i64_le),
    ("__vec_write_i64_le", 3, write_i64_le),
    ("__vec_read_i64_be", 2, read_i64_be),
    ("__vec_write_i64_be", 3, write_i64_be),
    ("__vec_read_f32_le", 2, read_f32_le),
    ("__vec_write_f32_le", 3, write_f32_le),
    ("__vec_read_f32_be", 2, read_f32_be),
    ("__vec_write_f32_be", 3, write_f32_be),
    ("__vec_read_f64_le", 2, read_f64_le),
    ("__vec_write_f64_le", 3, write_f64_le),
    ("__vec_read_f64_be", 2, read_f64_be),
    ("__vec_write_f64_be", 3, write_f64_be),
    ("__vec_to_utf8", 1, vec_to_utf8),
    ("__vec_copy_within", 4, vec_copy_within),
];

#[derive(Clone, Copy)]
enum Sign {
    Signed,
    Unsigned,
}

#[derive(Clone, Copy)]
enum Order {
    Little,
    Big,
}

fn sequence(vm: &VM, value: Value, operation: &'static str) -> Result<GcRef, RuntimeError> {
    let reference = value.as_ptr().map(GcRef::new);
    match reference.and_then(|r| vm.heap.get(r).map(|object| (r, &object.kind))) {
        Some((r, ObjectKind::Vec(_) | ObjectKind::Array(_))) => Ok(r),
        _ => Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "Vec<u8>",
            got: vm.value_type_name(value).to_string(),
        })),
    }
}

fn int_arg(vm: &VM, value: Value, operation: &'static str) -> Result<i64, RuntimeError> {
    vm.int_value(value)
        .map(|value| value.clamp(i64::MIN.into(), i64::MAX.into()) as i64)
        .ok_or_else(|| {
            vm.runtime_error(RuntimeErrorKind::TypeError {
                operation,
                expected: "int",
                got: vm.value_type_name(value).to_string(),
            })
        })
}

fn byte_len(vm: &VM, sequence: GcRef) -> usize {
    match vm.heap.get(sequence).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => vector.len(),
        Some(ObjectKind::Array(array)) => array.len(),
        _ => 0,
    }
}

fn byte_window(
    vm: &VM,
    sequence: GcRef,
    offset: i64,
    width: usize,
    operation: &'static str,
) -> Result<usize, RuntimeError> {
    let length = byte_len(vm, sequence);
    usize::try_from(offset)
        .ok()
        .filter(|start| start.checked_add(width).is_some_and(|end| end <= length))
        .ok_or_else(|| {
            vm.runtime_error(RuntimeErrorKind::ByteRangeOutOfBounds {
                operation,
                offset,
                width,
                length,
            })
        })
}

fn element_byte(value: Option<Value>) -> u8 {
    value.and_then(|value| value.as_int()).unwrap_or(0) as u8
}

fn load_bytes(vm: &VM, sequence: GcRef, start: usize, out: &mut [u8]) {
    match vm.heap.get(sequence).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => match &vector.data {
            VecData::Bytes(bytes) => out.copy_from_slice(&bytes[start..start + out.len()]),
            VecData::Ints(ints) => {
                for (byte, slot) in out.iter_mut().zip(&ints[start..]) {
                    *byte = *slot as u8;
                }
            }
            _ => {
                for (index, byte) in out.iter_mut().enumerate() {
                    *byte = element_byte(vector.get(start + index));
                }
            }
        },
        Some(ObjectKind::Array(array)) => match array.data.as_ints() {
            Some(ints) => {
                for (byte, slot) in out.iter_mut().zip(&ints[start..]) {
                    *byte = *slot as u8;
                }
            }
            None => {
                for (index, byte) in out.iter_mut().enumerate() {
                    *byte = element_byte(array.get(start + index));
                }
            }
        },
        _ => {}
    }
}

fn store_bytes(vm: &mut VM, sequence: GcRef, start: usize, bytes: &[u8]) {
    match vm.heap.get_mut(sequence).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(vector)) => match &mut vector.data {
            VecData::Bytes(slots) => slots[start..start + bytes.len()].copy_from_slice(bytes),
            VecData::Ints(ints) => {
                for (slot, byte) in ints[start..].iter_mut().zip(bytes) {
                    *slot = i64::from(*byte);
                }
            }
            _ => {
                for (index, byte) in bytes.iter().enumerate() {
                    vector.set(start + index, Value::int(i64::from(*byte)));
                }
            }
        },
        Some(ObjectKind::Array(array)) => match array.data.as_ints_mut() {
            Some(ints) => {
                for (slot, byte) in ints[start..].iter_mut().zip(bytes) {
                    *slot = i64::from(*byte);
                }
            }
            None => {
                for (index, byte) in bytes.iter().enumerate() {
                    array.set(start + index, Value::int(i64::from(*byte)));
                }
            }
        },
        _ => {}
    }
}

fn read_window<const N: usize>(
    vm: &VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    order: Order,
) -> Result<[u8; N], RuntimeError> {
    let sequence = sequence(vm, args[0], operation)?;
    let offset = int_arg(vm, args[1], operation)?;
    let start = byte_window(vm, sequence, offset, width, operation)?;
    let mut bytes = [0u8; N];
    load_bytes(vm, sequence, start, &mut bytes[..width]);
    if let Order::Big = order {
        bytes[..width].reverse();
    }
    Ok(bytes)
}

fn int_value(vm: &mut VM, value: i128, operation: &'static str) -> Result<Value, RuntimeError> {
    match i64::try_from(value) {
        Ok(value) => vm.make_int(value),
        Err(_) => Err(vm.runtime_error(RuntimeErrorKind::ByteValueOutOfRange {
            operation,
            value: value.to_string(),
            target: "int",
        })),
    }
}

fn read_int(
    vm: &mut VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    sign: Sign,
    order: Order,
) -> Result<Value, RuntimeError> {
    let bytes = read_window::<8>(vm, args, operation, width, order)?;
    let raw = u64::from_le_bytes(bytes);
    let bits = 8 * width as u32;
    let value = match sign {
        Sign::Unsigned => i128::from(raw),
        Sign::Signed => {
            let shift = 64 - bits;
            i128::from(((raw << shift) as i64) >> shift)
        }
    };
    int_value(vm, value, operation)
}

fn read_float(
    vm: &mut VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    order: Order,
) -> Result<Value, RuntimeError> {
    let bytes = read_window::<8>(vm, args, operation, width, order)?;
    let value = if width == 4 {
        f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    } else {
        f64::from_le_bytes(bytes)
    };
    Ok(Value::float(value))
}

fn integer_target(width: usize, sign: Sign) -> (&'static str, i128, i128) {
    match (width, sign) {
        (1, Sign::Unsigned) => ("u8", 0, i128::from(u8::MAX)),
        (1, Sign::Signed) => ("i8", i128::from(i8::MIN), i128::from(i8::MAX)),
        (2, Sign::Unsigned) => ("u16", 0, i128::from(u16::MAX)),
        (2, Sign::Signed) => ("i16", i128::from(i16::MIN), i128::from(i16::MAX)),
        (4, Sign::Unsigned) => ("u32", 0, i128::from(u32::MAX)),
        (4, Sign::Signed) => ("i32", i128::from(i32::MIN), i128::from(i32::MAX)),
        (_, Sign::Unsigned) => ("u64", 0, i128::from(u64::MAX)),
        (_, Sign::Signed) => ("i64", i128::from(i64::MIN), i128::from(i64::MAX)),
    }
}

fn write_window(
    vm: &mut VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    order: Order,
    mut bytes: [u8; 8],
) -> Result<Value, RuntimeError> {
    let sequence = sequence(vm, args[0], operation)?;
    let offset = int_arg(vm, args[1], operation)?;
    let start = byte_window(vm, sequence, offset, width, operation)?;
    if let Order::Big = order {
        bytes[..width].reverse();
    }
    store_bytes(vm, sequence, start, &bytes[..width]);
    Ok(Value::unit())
}

fn write_int(
    vm: &mut VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    sign: Sign,
    order: Order,
) -> Result<Value, RuntimeError> {
    let value = vm.int_value(args[2]).ok_or_else(|| {
        vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "int",
            got: vm.value_type_name(args[2]).to_string(),
        })
    })?;
    let (target, min, max) = integer_target(width, sign);
    if !(min..=max).contains(&value) {
        return Err(vm.runtime_error(RuntimeErrorKind::ByteValueOutOfRange {
            operation,
            value: value.to_string(),
            target,
        }));
    }
    let bytes = (value as u64).to_le_bytes();
    write_window(vm, args, operation, width, order, bytes)
}

fn write_float(
    vm: &mut VM,
    args: &[Value],
    operation: &'static str,
    width: usize,
    order: Order,
) -> Result<Value, RuntimeError> {
    let value = args[2].as_float().ok_or_else(|| {
        vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "float",
            got: vm.value_type_name(args[2]).to_string(),
        })
    })?;
    let mut bytes = [0u8; 8];
    if width == 4 {
        bytes[..4].copy_from_slice(&(value as f32).to_le_bytes());
    } else {
        bytes = value.to_le_bytes();
    }
    write_window(vm, args, operation, width, order, bytes)
}

fn vec_to_utf8(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = sequence(vm, args[0], "to_utf8")?;
    let mut bytes = vec![0u8; byte_len(vm, sequence)];
    load_bytes(vm, sequence, 0, &mut bytes);
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Value::none());
    };
    let text = vm.alloc_string(&text)?;
    let some = vm.alloc_sum(SumTag::OptionSome, Value::ptr(text.index()))?;
    Ok(Value::ptr(some.index()))
}

fn vec_copy_within(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let operation = "copy_within";
    let sequence = sequence(vm, args[0], operation)?;
    let start = int_arg(vm, args[1], operation)?;
    let end = int_arg(vm, args[2], operation)?;
    let dest = int_arg(vm, args[3], operation)?;
    let width = usize::try_from(end.saturating_sub(start)).unwrap_or(0);
    if end < start {
        return Err(vm.runtime_error(RuntimeErrorKind::ByteRangeOutOfBounds {
            operation,
            offset: start,
            width: 0,
            length: byte_len(vm, sequence),
        }));
    }
    let from = byte_window(vm, sequence, start, width, operation)?;
    let to = byte_window(vm, sequence, dest, width, operation)?;
    if let Some(ObjectKind::Vec(vector)) = vm.heap.get_mut(sequence).map(|object| &mut object.kind)
        && let VecData::Bytes(bytes) = &mut vector.data
    {
        bytes.copy_within(from..from + width, to);
        return Ok(Value::unit());
    }
    let mut bytes = vec![0u8; width];
    load_bytes(vm, sequence, from, &mut bytes);
    store_bytes(vm, sequence, to, &bytes);
    Ok(Value::unit())
}

pub(crate) fn bytes_vec(bytes: &[u8]) -> AelysVec {
    AelysVec::from_bytes(bytes.to_vec())
}

pub(crate) fn string_to_bytes(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = match vm.string_bytes(args[0]) {
        Some(bytes) => bytes_vec(bytes),
        None => {
            return Err(vm.runtime_error(RuntimeErrorKind::TypeError {
                operation: "to_bytes",
                expected: "string",
                got: vm.value_type_name(args[0]).to_string(),
            }));
        }
    };
    let reference = vm.alloc_vec(vector)?;
    Ok(Value::ptr(reference.index()))
}

pub(crate) fn sequence_bytes(vm: &VM, value: Value) -> Option<Vec<u8>> {
    let reference = value.as_ptr().map(GcRef::new)?;
    if let Some(ObjectKind::Vec(vector)) = vm.heap.get(reference).map(|object| &object.kind)
        && let VecData::Bytes(bytes) = &vector.data
    {
        return Some(bytes.clone());
    }
    if !matches!(
        vm.heap.get(reference).map(|object| &object.kind),
        Some(ObjectKind::Vec(_) | ObjectKind::Array(_))
    ) {
        return None;
    }
    let mut bytes = vec![0u8; byte_len(vm, reference)];
    load_bytes(vm, reference, 0, &mut bytes);
    Some(bytes)
}

// a borrowed Vec<u8> lends its own packed storage; one still in wider storage is packed in place first
pub(crate) fn lend_bytes(vm: &mut VM, value: Value, exclusive: bool) -> Option<(*mut u8, usize)> {
    let reference = value.as_ptr().map(GcRef::new)?;
    let packed = match vm.heap.get(reference).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => vector.data.as_bytes().is_some(),
        _ => return None,
    };
    if !packed {
        let Some(ObjectKind::Vec(vector)) =
            vm.heap.get_mut(reference).map(|object| &mut object.kind)
        else {
            return None;
        };
        let before = vector.size_bytes();
        if !vector.pack_bytes() {
            return None;
        }
        let after = vector.size_bytes();
        vm.heap.account_reallocation(before, after);
    }
    if exclusive {
        match vm.heap.get_mut(reference).map(|object| &mut object.kind) {
            Some(ObjectKind::Vec(vector)) => vector
                .data
                .as_bytes_mut()
                .map(|bytes| (bytes.as_mut_ptr(), bytes.len())),
            _ => None,
        }
    } else {
        match vm.heap.get(reference).map(|object| &object.kind) {
            Some(ObjectKind::Vec(vector)) => vector
                .data
                .as_bytes()
                .map(|bytes| (bytes.as_ptr().cast_mut(), bytes.len())),
            _ => None,
        }
    }
}
