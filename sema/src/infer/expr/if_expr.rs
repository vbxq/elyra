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
        let mut typed_then = self.infer_expr(then_branch);
        let mut typed_else = self.infer_expr(else_branch);
        self.unify_branch_literals(&mut [&mut typed_then, &mut typed_else]);
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

        let result_type = [&typed_then.ty, &typed_else.ty]
            .into_iter()
            .find(|ty| known_aggregate(ty))
            .cloned()
            .unwrap_or_else(|| self.type_gen.fresh());
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

        let settled = settled_branch_type(&typed_then.ty, &typed_else.ty);
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
                settled.unwrap_or(result_type)
            },
        )
    }
}

fn known_aggregate(ty: &InferType) -> bool {
    matches!(
        ty,
        InferType::Struct(_)
            | InferType::Applied { .. }
            | InferType::Array(_)
            | InferType::FixedArray(_, _)
            | InferType::Vec(_)
            | InferType::Map(_, _)
    ) && ty.is_concrete()
}

// a method call on the if needs its receiver type before the constraints are solved
pub(crate) fn settled_branch_type(then_ty: &InferType, else_ty: &InferType) -> Option<InferType> {
    match (then_ty, else_ty) {
        (InferType::Never, other) | (other, InferType::Never) if other.is_concrete() => {
            Some(other.clone())
        }
        (left, right) if left == right && left.is_concrete() => Some(left.clone()),
        (left, right) if left.is_concrete() && fits_open(right, left) => Some(left.clone()),
        (left, right) if right.is_concrete() && fits_open(left, right) => Some(right.clone()),
        _ => None,
    }
}

pub(crate) fn settled_arm_type(arms: &[InferType]) -> Option<InferType> {
    let settled = arms
        .iter()
        .find(|ty| ty.is_concrete() && !matches!(ty, InferType::Never))?;
    arms.iter()
        .all(|ty| matches!(ty, InferType::Never) || fits_open(ty, settled))
        .then(|| settled.clone())
}

fn fits_open(open: &InferType, concrete: &InferType) -> bool {
    match (open, concrete) {
        (InferType::Var(_), _) => true,
        (InferType::Option(open), InferType::Option(concrete))
        | (InferType::Vec(open), InferType::Vec(concrete))
        | (InferType::Array(open), InferType::Array(concrete)) => fits_open(open, concrete),
        (InferType::FixedArray(open, open_len), InferType::FixedArray(concrete, concrete_len)) => {
            open_len == concrete_len && fits_open(open, concrete)
        }
        (InferType::Result(open_ok, open_err), InferType::Result(ok, err))
        | (InferType::Map(open_ok, open_err), InferType::Map(ok, err)) => {
            fits_open(open_ok, ok) && fits_open(open_err, err)
        }
        (InferType::Tuple(open), InferType::Tuple(concrete)) => {
            open.len() == concrete.len()
                && open
                    .iter()
                    .zip(concrete)
                    .all(|(open, concrete)| fits_open(open, concrete))
        }
        (
            InferType::Applied {
                name: open_name,
                args: open,
            },
            InferType::Applied { name, args },
        ) => {
            open_name == name
                && open.len() == args.len()
                && open
                    .iter()
                    .zip(args)
                    .all(|(open, concrete)| fits_open(open, concrete))
        }
        (open, concrete) => open == concrete,
    }
}
