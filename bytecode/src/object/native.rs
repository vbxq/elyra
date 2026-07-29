// just metadata, actual execution logic is in runtime crate
#[derive(Clone, Debug)]
pub struct NativeFunction {
    pub name: String,
    pub arity: u16,
}

impl NativeFunction {
    pub fn new(name: impl Into<String>, arity: u16) -> Self {
        Self {
            name: name.into(),
            arity,
        }
    }
}
