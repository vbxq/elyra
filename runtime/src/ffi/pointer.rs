use super::{address, integer_bits, read_c_string};
use crate::vm::{VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_common::ffi::FfiType;

type Native = fn(&mut VM, &[Value]) -> Result<Value, RuntimeError>;

fn failure(vm: &VM, operation: &str, message: String) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::PointerAccess {
        operation: operation.to_string(),
        message,
    })
}

fn pointer(vm: &VM, value: Value, operation: &str) -> Result<u64, RuntimeError> {
    address(vm, value).ok_or_else(|| failure(vm, operation, "expected a ptr".to_string()))
}

fn target(vm: &VM, args: &[Value], operation: &str) -> Result<usize, RuntimeError> {
    let base = pointer(vm, args[0], operation)?;
    if base == 0 {
        return Err(failure(
            vm,
            operation,
            "access through a null ptr".to_string(),
        ));
    }
    let offset = vm
        .int_value(args[1])
        .ok_or_else(|| failure(vm, operation, "expected an integer offset".to_string()))?;
    let address = i128::from(base) + offset;
    u64::try_from(address)
        .ok()
        .and_then(|address| usize::try_from(address).ok())
        .filter(|address| *address != 0)
        .ok_or_else(|| {
            failure(
                vm,
                operation,
                format!("offset {offset} moves the address outside the address space"),
            )
        })
}

fn read(vm: &mut VM, args: &[Value], ty: FfiType, operation: &str) -> Result<Value, RuntimeError> {
    let at = target(vm, args, operation)?;
    unsafe {
        match ty {
            FfiType::I8 => vm.make_int(i64::from((at as *const i8).read_unaligned())),
            FfiType::I16 => vm.make_int(i64::from((at as *const i16).read_unaligned())),
            FfiType::I32 => vm.make_int(i64::from((at as *const i32).read_unaligned())),
            FfiType::I64 => vm.make_int((at as *const i64).read_unaligned()),
            FfiType::U8 => vm.make_int(i64::from((at as *const u8).read_unaligned())),
            FfiType::U16 => vm.make_int(i64::from((at as *const u16).read_unaligned())),
            FfiType::U32 => vm.make_int(i64::from((at as *const u32).read_unaligned())),
            FfiType::U64 | FfiType::Ptr => {
                vm.make_wide_int(i128::from((at as *const u64).read_unaligned()))
            }
            FfiType::F32 => Ok(Value::float(f64::from((at as *const f32).read_unaligned()))),
            FfiType::F64 => Ok(Value::float((at as *const f64).read_unaligned())),
            other => Err(failure(vm, operation, format!("cannot read {other}"))),
        }
    }
}

fn write(vm: &mut VM, args: &[Value], ty: FfiType, operation: &str) -> Result<Value, RuntimeError> {
    let at = target(vm, args, operation)?;
    let value = args[2];
    match ty {
        FfiType::F32 | FfiType::F64 => {
            let float = value
                .as_float()
                .ok_or_else(|| failure(vm, operation, "expected a float".to_string()))?;
            unsafe {
                if ty == FfiType::F32 {
                    (at as *mut f32).write_unaligned(float as f32);
                } else {
                    (at as *mut f64).write_unaligned(float);
                }
            }
        }
        FfiType::Ptr => {
            let written = pointer(vm, value, operation)?;
            unsafe { (at as *mut u64).write_unaligned(written) };
        }
        _ => {
            let integer = vm
                .int_value(value)
                .ok_or_else(|| failure(vm, operation, "expected an integer".to_string()))?;
            let bits = integer_bits(ty, integer).ok_or_else(|| {
                failure(
                    vm,
                    operation,
                    format!("value {integer} does not fit in {ty}"),
                )
            })?;
            let width = usize::from(ty.width_bytes());
            unsafe { std::ptr::copy_nonoverlapping(bits.as_ptr(), at as *mut u8, width) };
        }
    }
    Ok(Value::unit())
}

macro_rules! accessors {
    ($(($read:ident, $write:ident, $ty:expr, $read_name:literal, $write_name:literal)),* $(,)?) => {
        $(
            fn $read(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
                read(vm, args, $ty, $read_name)
            }
            fn $write(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
                write(vm, args, $ty, $write_name)
            }
        )*
    };
}

accessors!(
    (
        read_i8,
        write_i8,
        FfiType::I8,
        "ptr.read_i8",
        "ptr.write_i8"
    ),
    (
        read_i16,
        write_i16,
        FfiType::I16,
        "ptr.read_i16",
        "ptr.write_i16"
    ),
    (
        read_i32,
        write_i32,
        FfiType::I32,
        "ptr.read_i32",
        "ptr.write_i32"
    ),
    (
        read_i64,
        write_i64,
        FfiType::I64,
        "ptr.read_i64",
        "ptr.write_i64"
    ),
    (
        read_u8,
        write_u8,
        FfiType::U8,
        "ptr.read_u8",
        "ptr.write_u8"
    ),
    (
        read_u16,
        write_u16,
        FfiType::U16,
        "ptr.read_u16",
        "ptr.write_u16"
    ),
    (
        read_u32,
        write_u32,
        FfiType::U32,
        "ptr.read_u32",
        "ptr.write_u32"
    ),
    (
        read_u64,
        write_u64,
        FfiType::U64,
        "ptr.read_u64",
        "ptr.write_u64"
    ),
    (
        read_int,
        write_int,
        FfiType::I64,
        "ptr.read_int",
        "ptr.write_int"
    ),
    (
        read_f32,
        write_f32,
        FfiType::F32,
        "ptr.read_f32",
        "ptr.write_f32"
    ),
    (
        read_f64,
        write_f64,
        FfiType::F64,
        "ptr.read_f64",
        "ptr.write_f64"
    ),
    (
        read_ptr,
        write_ptr,
        FfiType::Ptr,
        "ptr.read_ptr",
        "ptr.write_ptr"
    ),
);

fn null(_vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::int(0))
}

fn is_null(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::bool(pointer(vm, args[0], "ptr.is_null")? == 0))
}

fn addr(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let address = pointer(vm, args[0], "ptr.addr")?;
    vm.make_wide_int(i128::from(address))
}

fn from_addr(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let integer = vm
        .int_value(args[0])
        .ok_or_else(|| failure(vm, "ptr::from_addr", "expected an integer".to_string()))?;
    let address = u64::try_from(integer).map_err(|_| {
        failure(
            vm,
            "ptr::from_addr",
            format!("{integer} is not an address (0..=18446744073709551615)"),
        )
    })?;
    vm.make_wide_int(i128::from(address))
}

fn offset(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let base = pointer(vm, args[0], "ptr.offset")?;
    let delta = vm
        .int_value(args[1])
        .ok_or_else(|| failure(vm, "ptr.offset", "expected an integer offset".to_string()))?;
    let moved = u64::try_from(i128::from(base) + delta).map_err(|_| {
        failure(
            vm,
            "ptr.offset",
            format!("offset {delta} moves the address outside the address space"),
        )
    })?;
    vm.make_wide_int(i128::from(moved))
}

fn alloc(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let size = vm
        .int_value(args[0])
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(|| {
            failure(
                vm,
                "ptr::alloc",
                "the size must be a non-negative integer".to_string(),
            )
        })?;
    let memory = unsafe { libc::calloc(size.max(1), 1) };
    if memory.is_null() {
        return Err(failure(
            vm,
            "ptr::alloc",
            format!("cannot allocate {size} bytes"),
        ));
    }
    vm.make_wide_int(memory as usize as i128)
}

fn free(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let address = pointer(vm, args[0], "ptr::free")?;
    unsafe { libc::free(address as usize as *mut libc::c_void) };
    Ok(Value::unit())
}

fn to_string(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let address = pointer(vm, args[0], "ptr::to_string")?;
    let text = read_c_string(address as usize)
        .map_err(|message| failure(vm, "ptr::to_string", format!("read {message}")))?;
    let reference = vm.alloc_string(&text)?;
    Ok(Value::ptr(reference.index()))
}

fn display(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let address = pointer(vm, args[0], "ptr.to_string")?;
    let reference = vm.alloc_string(&format!("{address:#x}"))?;
    Ok(Value::ptr(reference.index()))
}

pub(crate) const POINTER_NATIVES: &[(&str, u16, Native)] = &[
    ("__ptr_null", 0, null),
    ("__ptr_is_null", 1, is_null),
    ("__ptr_addr", 1, addr),
    ("__ptr_from_addr", 1, from_addr),
    ("__ptr_offset", 2, offset),
    ("__ptr_alloc", 1, alloc),
    ("__ptr_free", 1, free),
    ("__ptr_to_string", 1, to_string),
    ("__ptr_display", 1, display),
    ("__ptr_read_i8", 2, read_i8),
    ("__ptr_write_i8", 3, write_i8),
    ("__ptr_read_i16", 2, read_i16),
    ("__ptr_write_i16", 3, write_i16),
    ("__ptr_read_i32", 2, read_i32),
    ("__ptr_write_i32", 3, write_i32),
    ("__ptr_read_i64", 2, read_i64),
    ("__ptr_write_i64", 3, write_i64),
    ("__ptr_read_u8", 2, read_u8),
    ("__ptr_write_u8", 3, write_u8),
    ("__ptr_read_u16", 2, read_u16),
    ("__ptr_write_u16", 3, write_u16),
    ("__ptr_read_u32", 2, read_u32),
    ("__ptr_write_u32", 3, write_u32),
    ("__ptr_read_u64", 2, read_u64),
    ("__ptr_write_u64", 3, write_u64),
    ("__ptr_read_int", 2, read_int),
    ("__ptr_write_int", 3, write_int),
    ("__ptr_read_f32", 2, read_f32),
    ("__ptr_write_f32", 3, write_f32),
    ("__ptr_read_f64", 2, read_f64),
    ("__ptr_write_f64", 3, write_f64),
    ("__ptr_read_ptr", 2, read_ptr),
    ("__ptr_write_ptr", 3, write_ptr),
];
