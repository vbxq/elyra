pub(super) mod array;
mod assign;
mod binary;
mod call;
mod if_expr;
mod lambda;
pub(crate) mod member;
mod primary;

use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedFmtStringPart};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind};

impl TypeInference {
    pub(crate) fn require_display(&mut self, arg: &mut TypedExpr, reason: &str) {
        let ty = arg.ty.clone();
        let span = arg.span;
        crate::infer::monomorphize::mark_display_argument(arg);
        self.bound_residuals.push(crate::infer::BoundResidual {
            ty,
            trait_name: crate::prelude::DISPLAY_TRAIT.to_string(),
            trait_args: Vec::new(),
            span,
            reason: ConstraintReason::Other(reason.to_string()),
            nominal_only: true,
        });
    }

    pub(super) fn infer_expr(&mut self, expr: &Expr) -> TypedExpr {
        self.depth += 1;
        if self.depth > super::MAX_INFERENCE_DEPTH {
            self.errors.push(TypeError::recursion_limit(expr.span));
            self.depth -= 1;
            return TypedExpr {
                kind: TypedExprKind::Null,
                ty: InferType::Poison,
                span: expr.span,
            };
        }

        let (kind, ty) = match &expr.kind {
            ExprKind::Int(n) => (TypedExprKind::Int(*n), InferType::I64),
            ExprKind::Float(f) => (TypedExprKind::Float(*f), InferType::F64),
            ExprKind::Bool(b) => (TypedExprKind::Bool(*b), InferType::Bool),
            ExprKind::String(s) => (TypedExprKind::String(s.clone()), InferType::String),
            ExprKind::Unit => (TypedExprKind::Unit, InferType::Unit),
            ExprKind::FmtString(parts) => {
                let typed_parts = parts
                    .iter()
                    .map(|p| match p {
                        aelys_syntax::FmtStringPart::Literal(s) => {
                            TypedFmtStringPart::Literal(s.clone())
                        }
                        aelys_syntax::FmtStringPart::Expr(e) => {
                            let mut typed = self.infer_expr(e);
                            self.require_display(&mut typed, "format string interpolation");
                            TypedFmtStringPart::Expr(Box::new(typed))
                        }
                        aelys_syntax::FmtStringPart::Placeholder => TypedFmtStringPart::Placeholder,
                    })
                    .collect();
                (TypedExprKind::FmtString(typed_parts), InferType::String)
            }
            ExprKind::Null => (TypedExprKind::Null, InferType::Null),
            ExprKind::Try(inner) => self.infer_try_expr(inner, expr.span),
            ExprKind::Match { scrutinee, arms } => {
                self.infer_match_expr(scrutinee, arms, expr.span)
            }
            ExprKind::Identifier(name) => self.infer_identifier_expr(name, expr.span),
            ExprKind::Binary { left, op, right } => {
                let mut typed_left = self.infer_expr(left);
                let mut typed_right = self.infer_expr(right);
                Self::narrow_binop_int_literals(&mut typed_left, &mut typed_right);
                let result_type = self.infer_binary_op(*op, &typed_left, &typed_right, expr.span);

                (
                    TypedExprKind::Binary {
                        left: Box::new(typed_left),
                        op: *op,
                        right: Box::new(typed_right),
                    },
                    result_type,
                )
            }
            ExprKind::Unary { op, operand } => {
                let typed_operand = self.infer_expr(operand);
                let result_type = self.infer_unary_op(*op, &typed_operand, expr.span);

                (
                    TypedExprKind::Unary {
                        op: *op,
                        operand: Box::new(typed_operand),
                    },
                    result_type,
                )
            }
            ExprKind::And { left, right } => self.infer_logical_expr("and", left, right, expr),
            ExprKind::Or { left, right } => self.infer_logical_expr("or", left, right, expr),
            ExprKind::Call { callee, args } => self.infer_call_expr(callee, args, expr.span),
            ExprKind::GenericApply { callee, type_args } => {
                self.infer_generic_apply(callee, type_args, expr.span)
            }
            ExprKind::Assign { name, value } => self.infer_assign_expr(name, value, expr.span),
            ExprKind::MemberAssign {
                object,
                member,
                value,
            } => self.infer_member_assign_expr(object, member, value, expr.span),
            ExprKind::Grouping(inner) => {
                let typed_inner = self.infer_expr(inner);
                let ty = typed_inner.ty.clone();
                (TypedExprKind::Grouping(Box::new(typed_inner)), ty)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.infer_if_expr(condition, then_branch, else_branch, expr.span),
            ExprKind::Lambda {
                params,
                return_type,
                body,
            } => self.infer_lambda_expr(params, return_type.as_ref(), body, expr.span),
            ExprKind::Member {
                object,
                member,
                separator,
            } => self.infer_member_expr(object, member, *separator, expr.span),
            ExprKind::ArrayLiteral {
                element_type,
                elements,
            } => {
                self.infer_array_literal(element_type, elements, expr.repeat.as_deref(), expr.span)
            }
            ExprKind::ArraySized { element_type, size } => {
                self.infer_array_sized(element_type, size, expr.span)
            }
            ExprKind::VecLiteral {
                element_type,
                elements,
            } => self.infer_vec_literal(element_type, elements, expr.repeat.as_deref(), expr.span),
            ExprKind::Index { object, index } => self.infer_index_expr(object, index, expr.span),
            ExprKind::IndexAssign {
                object,
                index,
                value,
            } => self.infer_index_assign_expr(object, index, value, expr.span),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.infer_range_expr(start, end, *inclusive, expr.span),
            ExprKind::Slice { object, range } => self.infer_slice_expr(object, range, expr.span),
            ExprKind::StructLiteral {
                name,
                type_args,
                fields,
            } => self.infer_struct_literal(name, type_args, fields, expr.span),
            ExprKind::EnumLiteral { path, fields } => {
                self.infer_enum_literal(path, &[], fields, expr.span)
            }
            ExprKind::GenericEnumLiteral {
                path,
                type_args,
                fields,
            } => self.infer_enum_literal(path, type_args, fields, expr.span),
            ExprKind::Cast {
                expr: inner,
                target,
            } => {
                let typed_inner = self.infer_expr(inner);
                let target_ty = self.type_from_annotation(target);
                let target_has_invalid_arity = self.annotation_has_invalid_generic_arity(target);
                let src = &typed_inner.ty;
                let src_is_type_param = matches!(src, InferType::Var(_))
                    || matches!(src, InferType::Struct(name) if self.type_params_in_scope.contains(name));
                let allowed = src_is_type_param
                    || ((src.is_numeric()
                        || *src == InferType::Bool
                        || *src == InferType::Dynamic
                        || matches!(src, InferType::UntypedNative(_)))
                        && (target_ty.is_numeric() || target_ty == InferType::Bool));
                if !target_has_invalid_arity && !allowed {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Mismatch {
                            expected: target_ty.clone(),
                            found: src.clone(),
                        },
                        span: inner.span,
                        reason: ConstraintReason::InvalidCast,
                    });
                }
                (
                    TypedExprKind::Cast {
                        expr: Box::new(typed_inner),
                        target: target_ty.clone(),
                    },
                    target_ty,
                )
            }
        };

        self.depth -= 1;
        TypedExpr {
            kind,
            ty,
            span: expr.span,
        }
    }

    fn narrow_binop_int_literals(left: &mut TypedExpr, right: &mut TypedExpr) {
        let narrow = |lit: &mut TypedExpr, target: &InferType| {
            if let TypedExprKind::Int(v) = &lit.kind
                && target.is_integer()
                && *target != InferType::I64
                && InferType::int_fits(*v, target)
            {
                lit.ty = target.clone();
            }
        };
        if matches!(&left.kind, TypedExprKind::Int(_)) && right.ty.is_integer() {
            narrow(left, &right.ty.clone());
        } else if matches!(&right.kind, TypedExprKind::Int(_)) && left.ty.is_integer() {
            narrow(right, &left.ty.clone());
        }
    }

    fn infer_logical_expr(
        &mut self,
        op_label: &str,
        left: &Expr,
        right: &Expr,
        _expr: &Expr,
    ) -> (TypedExprKind, InferType) {
        let typed_left = self.infer_expr(left);
        let typed_right = self.infer_expr(right);

        let reason = |op_label: &str| ConstraintReason::BinaryOp {
            op: op_label.to_string(),
        };
        if !self.reject_dynamic(
            &typed_left.ty,
            &InferType::Bool,
            left.span,
            reason(op_label),
        ) && !self.reject_untyped_native(
            &typed_left.ty,
            &InferType::Bool,
            left.span,
            reason(op_label),
        ) {
            self.constraints.push(Constraint::equal(
                typed_left.ty.clone(),
                InferType::Bool,
                left.span,
                reason(op_label),
            ));
        }
        if !self.reject_dynamic(
            &typed_right.ty,
            &InferType::Bool,
            right.span,
            reason(op_label),
        ) && !self.reject_untyped_native(
            &typed_right.ty,
            &InferType::Bool,
            right.span,
            reason(op_label),
        ) {
            self.constraints.push(Constraint::equal(
                typed_right.ty.clone(),
                InferType::Bool,
                right.span,
                reason(op_label),
            ));
        }

        (
            if op_label == "and" {
                TypedExprKind::And {
                    left: Box::new(typed_left),
                    right: Box::new(typed_right),
                }
            } else {
                TypedExprKind::Or {
                    left: Box::new(typed_left),
                    right: Box::new(typed_right),
                }
            },
            InferType::Bool,
        )
    }
}
