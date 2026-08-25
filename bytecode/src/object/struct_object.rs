use crate::SchemaId;
use crate::Value;

#[derive(Debug, Clone)]
pub struct AelysStruct {
    pub schema_id: SchemaId,
    pub slots: Box<[Value]>,
}

impl AelysStruct {
    pub fn new(schema_id: SchemaId, slots: Vec<Value>) -> Self {
        Self {
            schema_id,
            slots: slots.into_boxed_slice(),
        }
    }

    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.slots.len() * std::mem::size_of::<Value>()
    }
}
