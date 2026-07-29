use crate::pipeline::types::{PipelineError, Stage, StageInput, StageOutput};
use aelys_runtime::VM;

pub struct VMStage {
    vm: Option<VM>,
} // Compiled -> Value

impl VMStage {
    pub fn new() -> Self {
        Self { vm: None }
    }
    pub fn with_vm(vm: VM) -> Self {
        Self { vm: Some(vm) }
    }
}

impl Default for VMStage {
    fn default() -> Self {
        Self::new()
    }
}

impl Stage for VMStage {
    fn name(&self) -> &str {
        "vm"
    }
    fn cacheable(&self) -> bool {
        false
    } // side effects - don't cache

    fn execute(&mut self, input: StageInput) -> Result<StageOutput, PipelineError> {
        let (function, source) = match input {
            StageInput::Compiled(f, s) => (*f, s),
            other => {
                return Err(PipelineError::TypeMismatch {
                    expected: "Compiled",
                    got: other.type_name(),
                });
            }
        };

        let mut vm = match self.vm.take() {
            Some(vm) => vm,
            None => VM::new(source).map_err(|e| PipelineError::StageError {
                stage: "vm".to_string(),
                message: e.to_string(),
            })?,
        };

        let func_ref = vm
            .alloc_function(function)
            .map_err(|e| PipelineError::StageError {
                stage: "vm".to_string(),
                message: e.to_string(),
            })?;
        let result = vm
            .execute(func_ref)
            .map_err(|e| PipelineError::StageError {
                stage: "vm".to_string(),
                message: e.to_string(),
            })?;

        self.vm = Some(vm);

        Ok(StageOutput::Value(result))
    }
}
