// just metadata, actual execution logic is in runtime crate
#[derive(Clone, Debug)]
pub struct NativeFunction {
    pub name: String,
    pub arity: u16,
    pub slot: u32,
}

impl NativeFunction {
    pub const UNREGISTERED: u32 = u32::MAX;

    pub fn new(name: impl Into<String>, arity: u16) -> Self {
        Self::with_slot(name, arity, Self::UNREGISTERED)
    }

    pub fn with_slot(name: impl Into<String>, arity: u16, slot: u32) -> Self {
        Self {
            name: name.into(),
            arity,
            slot,
        }
    }
}
