use crate::{EnumSchema, Function, SchemaId, StructSchema, Value};
use std::sync::{Arc, OnceLock};

/// the schema tables a struct or enum instruction reads
#[derive(Debug)]
pub struct FunctionSchemas {
    pub structs: Vec<StructSchema>,
    pub enums: Vec<EnumSchema>,
    pub runtime_ids: Vec<SchemaId>,
}

/// a wrapped bytecode function for the gc heap.
#[derive(Debug, Clone)]
pub struct AelysFunction {
    pub function: Function,
    pub constants: Vec<Value>,
    pub verified: bool,
    schemas: OnceLock<Arc<FunctionSchemas>>,
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
            schemas: OnceLock::new(),
        }
    }

    /// runtime schema ids are materialized before this object is built, so the first reader observes the final tables.
    pub fn schemas(&self) -> Arc<FunctionSchemas> {
        Arc::clone(self.schemas.get_or_init(|| {
            Arc::new(FunctionSchemas {
                structs: self.function.struct_schemas.clone(),
                enums: self.function.enum_schemas.clone(),
                runtime_ids: self.function.schema_ids.clone(),
            })
        }))
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
