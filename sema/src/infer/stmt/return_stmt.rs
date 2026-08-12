use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::TypedStmtKind;
use crate::types::InferType;
use aelys_syntax::{Expr, Span};

impl TypeInference {
    pub(super) fn infer_return_stmt(&mut self, span: Span, expr: Option<&Expr>) -> TypedStmtKind {
        let typed_expr = expr.map(|e| self.infer_expr(e));

        if let Some(expected_ret) = self.current_return_type().cloned() {
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
                if !matches!(expected_ret, InferType::Dynamic) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::UntypedNativeTypeMismatch {
                            name: name.clone(),
                            expected: expected_ret.clone(),
                        },
                        span,
                        reason: ConstraintReason::Return {
                            func_name: self
                                .env
                                .current_function()
                                .cloned()
                                .unwrap_or_else(|| "<anonymous>".to_string()),
                        },
                    });
                }
            } else {
                self.constraints.push(Constraint::equal(
                    actual_ret,
                    expected_ret,
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
