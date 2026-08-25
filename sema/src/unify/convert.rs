use super::UnifyError;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use aelys_syntax::Span;

pub fn unify_error_to_type_error(
    error: UnifyError,
    span: Span,
    reason: ConstraintReason,
) -> TypeError {
    let kind = match error {
        UnifyError::Mismatch(expected, found) => TypeErrorKind::Mismatch { expected, found },
        UnifyError::UntypedNativeBoundary(name) => TypeErrorKind::UntypedNativeBoundary { name },
        UnifyError::InfiniteType(var, ty) => TypeErrorKind::InfiniteType { var, ty },
        UnifyError::ArityMismatch(expected, found) => {
            TypeErrorKind::ArityMismatch { expected, found }
        }
        UnifyError::Poisoned => TypeErrorKind::PoisonedType,
    };

    TypeError { kind, span, reason }
}
