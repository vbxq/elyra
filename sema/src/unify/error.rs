use crate::types::{InferType, TypeVarId};

pub type UnifyResult<T> = Result<T, UnifyError>;

#[derive(Debug, Clone)]
pub enum UnifyError {
    Mismatch(InferType, InferType),
    UntypedNativeBoundary(String),
    InfiniteType(TypeVarId, InferType),
    ArityMismatch(usize, usize),
    Poisoned,
}

impl std::fmt::Display for UnifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnifyError::Mismatch(t1, t2) => write!(f, "cannot unify {} with {}", t1, t2),
            UnifyError::UntypedNativeBoundary(name) => {
                write!(f, "untyped native '{}' crosses the typed boundary", name)
            }
            UnifyError::InfiniteType(var, ty) => write!(f, "infinite type: {} = {}", var, ty),
            UnifyError::ArityMismatch(expected, found) => {
                write!(f, "arity mismatch: expected {}, found {}", expected, found)
            }
            UnifyError::Poisoned => write!(f, "poisoned type cannot unify"),
        }
    }
}

impl std::error::Error for UnifyError {}
