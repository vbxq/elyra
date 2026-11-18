use super::Compiler;

impl Compiler {
    pub const BUILTINS: &'static [&'static str] = &[
        "alloc",
        "free",
        "load",
        "store",
        "__tostring",
        "__clone",
        "panic",
        "assert",
    ];
    pub fn is_builtin(name: &str) -> bool {
        Self::BUILTINS.contains(&name)
    }
}
