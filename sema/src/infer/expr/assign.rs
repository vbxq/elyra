use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError};
use crate::typed_ast::TypedExprKind;
use crate::types::InferType;
use aelys_syntax::{Expr, Span};

impl TypeInference {
    pub(super) fn infer_assign_expr(
        &mut self,
        name: &str,
        value: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        self.check_write_binding(name, span, "assignment");
        if self.immutable_imports.contains(name) && self.env.is_imported_binding(name) {
            self.errors.push(TypeError {
                kind: crate::constraint::TypeErrorKind::AssignToImmutableImport {
                    name: name.to_string(),
                    function: matches!(self.env.lookup(name), Some(InferType::Function { .. })),
                },
                span,
                reason: ConstraintReason::Other("assignment".to_string()),
            });
        }
        let mut typed_value = self.infer_expr(value);

        if let Some(var_type) = self.env.lookup(name).cloned() {
            self.fit_int_literal(&mut typed_value, &var_type);
            self.env.invalidate_collection_length(name);
            self.record_mutable_alias(
                value,
                &var_type,
                crate::constraint::AliasSite::Assignment(name.to_string()),
            );
            let reason = ConstraintReason::Assignment {
                var_name: name.to_string(),
            };
            if !self.reject_dynamic(&typed_value.ty, &var_type, span, reason.clone()) {
                self.constraints.push(Constraint::equal(
                    var_type.clone(),
                    typed_value.ty.clone(),
                    span,
                    reason,
                ));
            }

            let resolved = self.env.lookup_alias(name).unwrap_or(name).to_string();
            (
                TypedExprKind::Assign {
                    name: resolved,
                    value: Box::new(typed_value),
                },
                var_type,
            )
        } else if let Some(modules) = self.ambiguous_imports.get(name) {
            self.errors.push(TypeError {
                kind: crate::constraint::TypeErrorKind::AmbiguousImport {
                    name: name.to_string(),
                    modules: modules.clone(),
                },
                span,
                reason: ConstraintReason::Other("assignment".to_string()),
            });
            (
                TypedExprKind::Assign {
                    name: name.to_string(),
                    value: Box::new(typed_value),
                },
                InferType::Poison,
            )
        } else {
            self.errors
                .push(TypeError::undefined_variable(name.to_string(), span));
            (
                TypedExprKind::Assign {
                    name: name.to_string(),
                    value: Box::new(typed_value),
                },
                InferType::Poison,
            )
        }
    }
}
