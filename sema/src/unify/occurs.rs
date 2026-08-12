use crate::types::{InferType, TypeVarId};

pub(super) fn occurs_check(var: TypeVarId, ty: &InferType) -> bool {
    match ty {
        InferType::Var(id) => *id == var,
        InferType::Function { params, ret } => {
            params.iter().any(|p| occurs_check(var, p)) || occurs_check(var, ret)
        }
        InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
            occurs_check(var, inner)
        }
        InferType::Result(ok, err) => occurs_check(var, ok) || occurs_check(var, err),
        InferType::Tuple(elems) => elems.iter().any(|e| occurs_check(var, e)),
        InferType::I8
        | InferType::I16
        | InferType::I32
        | InferType::I64
        | InferType::U8
        | InferType::U16
        | InferType::U32
        | InferType::U64
        | InferType::F32
        | InferType::F64
        | InferType::Bool
        | InferType::String
        | InferType::Unit
        | InferType::Null
        | InferType::Error
        | InferType::Never
        | InferType::Numeric
        | InferType::UntypedNative(_)
        | InferType::Range
        | InferType::Struct(_)
        | InferType::Dynamic => false,
    }
}
