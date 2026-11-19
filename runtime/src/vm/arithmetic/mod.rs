use super::VM;
use super::Value;
use aelys_bytecode::IntOp;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

mod numbers;
mod strings;

impl VM {
    pub fn add_values(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        if let Some(result) = numbers::try_numbers(self, IntOp::Add, left, right)? {
            return Ok(result);
        }

        if let Some(result) = self.try_concat_strings(left, right)? {
            return Ok(result);
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "addition",
            expected: "number or string",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn sub_values(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        if let Some(result) = numbers::try_numbers(self, IntOp::Sub, left, right)? {
            return Ok(result);
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "subtraction",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn mul_values(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        if let Some(result) = numbers::try_numbers(self, IntOp::Mul, left, right)? {
            return Ok(result);
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "multiplication",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn div_values(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        if let Some(result) = numbers::try_numbers(self, IntOp::Div, left, right)? {
            return Ok(result);
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "division",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn mod_values(&mut self, left: Value, right: Value) -> Result<Value, RuntimeError> {
        if let Some(result) = numbers::try_numbers(self, IntOp::Rem, left, right)? {
            return Ok(result);
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "modulo",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn neg_value(&mut self, operand: Value) -> Result<Value, RuntimeError> {
        if self.is_int_value(operand) {
            return self.int_negate(operand);
        }
        if let Some(f) = operand.as_float() {
            return Ok(Value::float(-f));
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "negation",
            expected: "number",
            got: self.value_type_name(operand).to_string(),
        }))
    }
}
