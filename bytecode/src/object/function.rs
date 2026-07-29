use crate::{Function, Value};

/// A wrapped bytecode function for the GC heap.
#[derive(Debug, Clone)]
pub struct AelysFunction {
    pub function: Function,
    pub constants: Vec<Value>,
    pub verified: bool,
}

impl AelysFunction {
    pub fn new(function: Function) -> Self {
        Self::with_constants(function, Vec::new())
    }

    pub fn with_constants(function: Function, constants: Vec<Value>) -> Self {
        Self {
            function,
            constants,
            verified: false,
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.function.name.as_deref()
    }

    pub fn arity(&self) -> u16 {
        self.function.arity
    }

    pub fn num_registers(&self) -> u32 {
        self.function.num_registers
    }
}
