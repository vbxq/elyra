use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::TypedStmtKind;
use crate::types::InferType;
use aelys_syntax::{Expr, Span};

impl TypeInference {
    pub(super) fn infer_return_stmt(&mut self, span: Span, expr: Option<&Expr>) -> TypedStmtKind {
        let mut typed_expr = expr.map(|e| self.infer_expr(e));

        if let Some(expected_ret) = self.current_return_type().cloned() {
            if let Some(value) = typed_expr.as_ref().and_then(|typed| {
                if let crate::typed_ast::TypedExprKind::Float(value) = &typed.kind {
                    Some(*value)
                } else {
                    None
                }
            }) && expected_ret == InferType::F32
                && value.is_finite()
                && value.abs() <= f32::MAX as f64
                && let Some(typed) = typed_expr.as_mut()
            {
                typed.ty = InferType::F32;
            }
            let actual_ret = typed_expr
                .as_ref()
                .map(|e| e.ty.clone())
                .unwrap_or(InferType::Unit);

            if self.reject_dynamic(
                &actual_ret,
                &expected_ret,
                span,
                ConstraintReason::Return {
                    func_name: self
                        .env
                        .current_function()
                        .cloned()
                        .unwrap_or_else(|| "<anonymous>".to_string()),
                },
            ) {
            } else if let InferType::UntypedNative(name) = &actual_ret {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UntypedNativeBoundary { name: name.clone() },
                    span,
                    reason: ConstraintReason::Return {
                        func_name: self
                            .env
                            .current_function()
                            .cloned()
                            .unwrap_or_else(|| "<anonymous>".to_string()),
                    },
                });
            } else {
                self.constraints.push(Constraint::equal(
                    expected_ret,
                    actual_ret,
                    span,
                    ConstraintReason::Return {
                        func_name: self
                            .env
                            .current_function()
                            .cloned()
                            .unwrap_or_else(|| "<anonymous>".to_string()),
                    },
                ));
            }
        }

        TypedStmtKind::Return(typed_expr)
    }
}
