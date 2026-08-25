use super::Compiler;

impl Compiler {
    pub const BUILTINS: &'static [&'static str] = &["alloc", "free", "load", "store", "__tostring"];
    pub fn is_builtin(name: &str) -> bool {
        Self::BUILTINS.contains(&name)
    }
}
