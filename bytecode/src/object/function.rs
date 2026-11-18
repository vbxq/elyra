use crate::{EnumSchema, Function, SchemaId, StructSchema, Value};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

/// the schema tables a struct or enum instruction reads
#[derive(Debug)]
pub struct FunctionSchemas {
    pub structs: Vec<StructSchema>,
    pub enums: Vec<EnumSchema>,
    pub runtime_ids: Vec<SchemaId>,
}

/// a wrapped bytecode function for the gc heap.
#[derive(Debug)]
pub struct AelysFunction {
    pub function: Function,
    pub constants: Vec<Value>,
    pub verified: bool,
    schemas: OnceLock<Arc<FunctionSchemas>>,
    jit_slot: AtomicU32,
}

impl Clone for AelysFunction {
    fn clone(&self) -> Self {
        Self {
            function: self.function.clone(),
            constants: self.constants.clone(),
            verified: self.verified,
            schemas: self.schemas.clone(),
            jit_slot: AtomicU32::new(0),
        }
    }
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
            jit_slot: AtomicU32::new(0),
        }
    }

    pub fn jit_slot(&self) -> u32 {
        self.jit_slot.load(Ordering::Relaxed)
    }

    pub fn set_jit_slot(&self, slot: u32) {
        self.jit_slot.store(slot, Ordering::Relaxed);
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
