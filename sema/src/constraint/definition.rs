use super::ConstraintReason;
use crate::types::InferType;
use aelys_syntax::Span;

#[derive(Debug, Clone)]
pub enum Constraint {
    Equal {
        left: InferType,
        right: InferType,
        span: Span,
        reason: ConstraintReason,
    },

    OneOf {
        ty: InferType,
        options: Vec<InferType>,
        span: Span,
        reason: ConstraintReason,
    },
}

impl Constraint {
    pub fn equal(left: InferType, right: InferType, span: Span, reason: ConstraintReason) -> Self {
        Constraint::Equal {
            left,
            right,
            span,
            reason,
        }
    }

    pub fn one_of(
        ty: InferType,
        options: Vec<InferType>,
        span: Span,
        reason: ConstraintReason,
    ) -> Self {
        Constraint::OneOf {
            ty,
            options,
            span,
            reason,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Constraint::Equal { span, .. } => *span,
            Constraint::OneOf { span, .. } => *span,
        }
    }

    pub fn reason_mut(&mut self) -> &mut ConstraintReason {
        match self {
            Constraint::Equal { reason, .. } => reason,
            Constraint::OneOf { reason, .. } => reason,
        }
    }

    pub fn reason(&self) -> &ConstraintReason {
        match self {
            Constraint::Equal { reason, .. } => reason,
            Constraint::OneOf { reason, .. } => reason,
        }
    }
}
