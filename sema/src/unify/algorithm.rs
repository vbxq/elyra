use super::error::UnifyResult;
use super::occurs::occurs_check;
use super::{Substitution, UnifyError};
use crate::types::InferType;

pub fn unify(t1: &InferType, t2: &InferType, subst: &mut Substitution) -> UnifyResult<()> {
    let t1 = subst.apply(t1);
    let t2 = subst.apply(t2);

    match (&t1, &t2) {
        (InferType::Poison, _) | (_, InferType::Poison) => Err(UnifyError::Poisoned),

        (InferType::I8, InferType::I8)
        | (InferType::I16, InferType::I16)
        | (InferType::I32, InferType::I32)
        | (InferType::I64, InferType::I64)
        | (InferType::U8, InferType::U8)
        | (InferType::U16, InferType::U16)
        | (InferType::U32, InferType::U32)
        | (InferType::U64, InferType::U64)
        | (InferType::F32, InferType::F32)
        | (InferType::F64, InferType::F64)
        | (InferType::Bool, InferType::Bool)
        | (InferType::String, InferType::String)
        | (InferType::Unit, InferType::Unit)
        | (InferType::Null, InferType::Null) => Ok(()),

        (InferType::Error, InferType::Error) => Ok(()),
        (InferType::Never, _) | (_, InferType::Never) => Ok(()),

        (InferType::Numeric, InferType::Numeric) => Ok(()),
        (InferType::Numeric, ty) | (ty, InferType::Numeric) if ty.is_numeric() => Ok(()),

        (InferType::UntypedNative(a), InferType::UntypedNative(b)) if a == b => Ok(()),

        (InferType::UntypedNative(name), InferType::Dynamic)
        | (InferType::Dynamic, InferType::UntypedNative(name)) => {
            Err(UnifyError::UntypedNativeBoundary(name.clone()))
        }

        (InferType::UntypedNative(a), InferType::UntypedNative(b)) if a != b => {
            Err(UnifyError::Mismatch(t1.clone(), t2.clone()))
        }

        (InferType::Struct(a), InferType::Struct(b)) if a == b => Ok(()),

        (
            InferType::Applied {
                name: a,
                args: args_a,
            },
            InferType::Applied {
                name: b,
                args: args_b,
            },
        ) if a == b => {
            if args_a.len() != args_b.len() {
                return Err(UnifyError::ArityMismatch(args_a.len(), args_b.len()));
            }
            for (left, right) in args_a.iter().zip(args_b) {
                unify(left, right, subst)?;
            }
            Ok(())
        }

        (InferType::Param(a), InferType::Param(b)) if a == b => Ok(()),

        (InferType::Var(id1), InferType::Var(id2)) if id1 == id2 => Ok(()),

        (InferType::Var(v), ty) => {
            if occurs_check(*v, ty) {
                return Err(UnifyError::InfiniteType(*v, ty.clone()));
            }
            subst.bind(*v, ty.clone());
            Ok(())
        }

        (ty, InferType::Var(v)) => {
            if occurs_check(*v, ty) {
                return Err(UnifyError::InfiniteType(*v, ty.clone()));
            }
            subst.bind(*v, ty.clone());
            Ok(())
        }

        (
            InferType::Function {
                params: p1,
                ret: r1,
            },
            InferType::Function {
                params: p2,
                ret: r2,
            },
        ) => {
            if p1.len() != p2.len() {
                return Err(UnifyError::ArityMismatch(p1.len(), p2.len()));
            }

            for (param1, param2) in p1.iter().zip(p2.iter()) {
                unify(param1, param2, subst)?;
            }

            unify(r1, r2, subst)
        }

        (InferType::Array(inner1), InferType::Array(inner2)) => unify(inner1, inner2, subst),

        (InferType::FixedArray(inner1, len1), InferType::FixedArray(inner2, len2))
            if len1 == len2 =>
        {
            unify(inner1, inner2, subst)
        }

        (InferType::FixedArray(inner1, _), InferType::Array(inner2))
        | (InferType::Array(inner1), InferType::FixedArray(inner2, _)) => {
            unify(inner1, inner2, subst)
        }

        (InferType::Vec(inner1), InferType::Vec(inner2)) => unify(inner1, inner2, subst),

        (InferType::Option(inner1), InferType::Option(inner2)) => unify(inner1, inner2, subst),

        (InferType::Result(ok1, err1), InferType::Result(ok2, err2)) => {
            unify(ok1, ok2, subst)?;
            unify(err1, err2, subst)
        }

        (
            InferType::Projection {
                item: item1,
                self_ty: self1,
                ..
            },
            InferType::Projection {
                item: item2,
                self_ty: self2,
                ..
            },
        ) if item1 == item2 => unify(self1, self2, subst),

        (InferType::Range, InferType::Range) => Ok(()),

        (InferType::Tuple(elems1), InferType::Tuple(elems2)) => {
            if elems1.len() != elems2.len() {
                return Err(UnifyError::Mismatch(t1.clone(), t2.clone()));
            }

            for (e1, e2) in elems1.iter().zip(elems2.iter()) {
                unify(e1, e2, subst)?;
            }

            Ok(())
        }

        _ => Err(UnifyError::Mismatch(t1.clone(), t2.clone())),
    }
}
