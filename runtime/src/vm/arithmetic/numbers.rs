use super::super::VM;
use super::super::Value;
use aelys_bytecode::IntOp;
use aelys_common::error::RuntimeError;

fn both_ints(vm: &VM, left: Value, right: Value) -> bool {
    vm.is_int_value(left) && vm.is_int_value(right)
}

fn floats(vm: &VM, left: Value, right: Value) -> Option<(f64, f64)> {
    if left.as_float().is_none() && right.as_float().is_none() {
        return None;
    }
    Some((vm.number_as_f64(left)?, vm.number_as_f64(right)?))
}

pub(super) fn try_numbers(
    vm: &mut VM,
    op: IntOp,
    left: Value,
    right: Value,
) -> Result<Option<Value>, RuntimeError> {
    if both_ints(vm, left, right) {
        return vm.int_binary(op, left, right).map(Some);
    }
    let Some((a, b)) = floats(vm, left, right) else {
        return Ok(None);
    };
    Ok(Some(Value::float(match op {
        IntOp::Add => a + b,
        IntOp::Sub => a - b,
        IntOp::Mul => a * b,
        IntOp::Div => a / b,
        _ => a % b,
    })))
}
