pub mod ast;
pub mod module;
pub mod source;
pub mod span;
pub mod token;

pub use ast::*;
pub use module::ModuleId;
pub use source::Source;
pub use span::Span;
pub use token::{FmtPart, Token, TokenKind};

pub const TUPLE_STRUCT_PREFIX: &str = "AelysTuple";
pub const MAX_TUPLE_ARITY: usize = 8;

pub fn tuple_struct_name(arity: usize) -> String {
    format!("{TUPLE_STRUCT_PREFIX}{arity}")
}

pub fn tuple_arity(name: &str) -> Option<usize> {
    let base = name.split('<').next().unwrap_or(name);
    let base = base.rsplit("::").next().unwrap_or(base);
    base.strip_prefix(TUPLE_STRUCT_PREFIX)?
        .parse::<usize>()
        .ok()
        .filter(|arity| (2..=MAX_TUPLE_ARITY).contains(arity))
}
