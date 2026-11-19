use super::super::{VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    pub fn call_function_by_name(
        &mut self,
        name: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let func_value = self.named_global(name).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                "function '{}' not found",
                name
            )))
        })?;

        self.call_value(func_value, args)
    }

    pub fn get_function_value(&self, name: &str) -> Option<Value> {
        self.named_global(name)
    }
}
