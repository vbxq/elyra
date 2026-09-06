use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason};
use crate::typed_ast::TypedExprKind;
use crate::types::InferType;
use aelys_syntax::{Expr, Span};

impl TypeInference {
    pub(super) fn infer_if_expr(
        &mut self,
        condition: &Expr,
        then_branch: &Expr,
        else_branch: &Expr,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_cond = self.infer_expr(condition);
        let typed_then = self.infer_expr(then_branch);
        let typed_else = self.infer_expr(else_branch);
        let dynamic_result = matches!(typed_then.ty, InferType::Dynamic)
            || matches!(typed_else.ty, InferType::Dynamic);
        let poisoned_result = matches!(typed_then.ty, InferType::Poison)
            || matches!(typed_else.ty, InferType::Poison);

        if !self.reject_dynamic(
            &typed_cond.ty,
            &InferType::Bool,
            condition.span,
            ConstraintReason::IfCondition,
        ) && !self.reject_untyped_native(
            &typed_cond.ty,
            &InferType::Bool,
            condition.span,
            ConstraintReason::IfCondition,
        ) {
            self.constraints.push(Constraint::equal(
                InferType::Bool,
                typed_cond.ty.clone(),
                condition.span,
                ConstraintReason::IfCondition,
            ));
        }

        let result_type = self.type_gen.fresh();
        if !self.reject_dynamic(
            &typed_then.ty,
            &result_type,
            then_branch.span,
            ConstraintReason::IfBranches,
        ) {
            self.constraints.push(Constraint::equal(
                result_type.clone(),
                typed_then.ty.clone(),
                then_branch.span,
                ConstraintReason::IfBranches,
            ));
        }
        if !self.reject_dynamic(
            &typed_else.ty,
            &result_type,
            else_branch.span,
            ConstraintReason::IfBranches,
        ) {
            self.constraints.push(Constraint::equal(
                result_type.clone(),
                typed_else.ty.clone(),
                else_branch.span,
                ConstraintReason::IfBranches,
            ));
        }

        (
            TypedExprKind::If {
                condition: Box::new(typed_cond),
                then_branch: Box::new(typed_then),
                else_branch: Box::new(typed_else),
            },
            if poisoned_result {
                InferType::Poison
            } else if dynamic_result {
                InferType::Dynamic
            } else {
                result_type
            },
        )
    }
}
