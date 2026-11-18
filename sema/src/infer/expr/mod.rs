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
        let callee_position = std::mem::take(&mut self.callee_position);
        let expectation = self.nominal_expectation.take();
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
            ExprKind::Int(n) => {
                if let Some(error) = TypeError::integer_literal_out_of_range(*n, expr.span) {
                    self.errors.push(error);
                }
                (TypedExprKind::Int(*n), InferType::I64)
            }
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
                self.narrow_binop_int_literals(&mut typed_left, &mut typed_right);
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
            ExprKind::Borrow { mutable, operand } => {
                self.infer_borrow_expr(*mutable, operand, expr.span)
            }
            ExprKind::And { left, right } => self.infer_logical_expr("and", left, right, expr),
            ExprKind::Or { left, right } => self.infer_logical_expr("or", left, right, expr),
            ExprKind::Call { callee, args } => {
                self.constructor_expectation = expectation;
                let typed = self.infer_call_expr(callee, args, expr.span);
                self.constructor_expectation = None;
                typed
            }
            ExprKind::GenericApply { callee, type_args } => {
                self.callee_position = callee_position;
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
            } => self.infer_member_expr(object, member, *separator, expr.span, callee_position),
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
            } => self.infer_struct_literal(name, type_args, fields, expr.span, expectation),
            ExprKind::EnumLiteral { path, fields } => {
                self.infer_enum_literal(path, &[], fields, expr.span, expectation)
            }
            ExprKind::GenericEnumLiteral {
                path,
                type_args,
                fields,
            } => self.infer_enum_literal(path, type_args, fields, expr.span, expectation),
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

    fn narrow_binop_int_literals(&mut self, left: &mut TypedExpr, right: &mut TypedExpr) {
        if literal_shaped(left) && right.ty.is_sized_integer() {
            let target = right.ty.clone();
            self.retype_int_literal(left, &target);
        } else if literal_shaped(right) && left.ty.is_sized_integer() {
            let target = left.ty.clone();
            self.retype_int_literal(right, &target);
        } else if literal_shaped(left) && matches!(right.ty, InferType::Var(_)) {
            let target = right.ty.clone();
            self.defer_int_literal(left, target);
        } else if literal_shaped(right) && matches!(left.ty, InferType::Var(_)) {
            let target = left.ty.clone();
            self.defer_int_literal(right, target);
        }
    }

    pub(crate) fn infer_expr_expecting(&mut self, expr: &Expr, expected: &InferType) -> TypedExpr {
        self.nominal_expectation = Some(expected.clone());
        let typed = self.infer_expr(expr);
        self.nominal_expectation = None;
        typed
    }

    pub(crate) fn expected_type_args(expected: Option<InferType>, name: &str) -> Vec<InferType> {
        match expected {
            Some(InferType::Applied {
                name: applied,
                args,
            }) if applied == name => args,
            _ => Vec::new(),
        }
    }

    pub(crate) fn unify_branch_literals(&mut self, branches: &mut [&mut TypedExpr]) {
        let target = branches
            .iter()
            .filter(|branch| !literal_shaped(branch))
            .map(|branch| branch.ty.clone())
            .find(|ty| ty.is_sized_integer() || matches!(ty, InferType::Var(_)));
        let Some(target) = target else {
            return;
        };
        for branch in branches.iter_mut() {
            if literal_shaped(branch) {
                if target.is_sized_integer() {
                    self.retype_int_literal(branch, &target);
                } else {
                    self.defer_int_literal(branch, target.clone());
                }
            }
        }
    }

    fn defer_int_literal(&mut self, expr: &mut TypedExpr, target: InferType) {
        let mut values = Vec::new();
        retype_open_literal(expr, &target, &mut values);
        self.pending_int_literals.push((target, values));
    }

    pub(crate) fn settle_pending_int_literals(&mut self, subst: &mut crate::unify::Substitution) {
        for (target, values) in std::mem::take(&mut self.pending_int_literals) {
            let resolved = subst.apply(&target);
            match &resolved {
                InferType::Var(id) => subst.bind(*id, InferType::I64),
                InferType::Poison | InferType::Dynamic | InferType::Never => {}
                ty if ty.is_integer() => {
                    for (value, span) in values {
                        if !InferType::int_fits(value, ty) {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::Mismatch {
                                    expected: ty.clone(),
                                    found: InferType::I64,
                                },
                                span,
                                reason: ConstraintReason::IntLiteralOverflow {
                                    value,
                                    target: ty.clone(),
                                },
                            });
                        }
                    }
                }
                other => {
                    if let Some((_, span)) = values.first() {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::Mismatch {
                                expected: other.clone(),
                                found: InferType::I64,
                            },
                            span: *span,
                            reason: ConstraintReason::BinaryOp {
                                op: "integer literal".to_string(),
                            },
                        });
                    }
                }
            }
        }
    }

    pub(crate) fn fit_int_literal(&mut self, expr: &mut TypedExpr, expected: &InferType) {
        match expected {
            target if target.is_sized_integer() => {
                if literal_shaped(expr) {
                    self.retype_int_literal(expr, target);
                }
            }
            InferType::Array(inner) | InferType::FixedArray(inner, _) | InferType::Vec(inner) => {
                let elements = match &mut expr.kind {
                    TypedExprKind::ArrayLiteral {
                        element_type: None,
                        elements,
                        ..
                    } if !matches!(expected, InferType::Vec(_)) => elements,
                    TypedExprKind::VecLiteral {
                        element_type: None,
                        elements,
                        ..
                    } if matches!(expected, InferType::Vec(_)) => elements,
                    _ => return,
                };
                for element in elements.iter_mut() {
                    self.fit_int_literal(element, inner);
                }
                if elements.is_empty() || elements.iter().any(|element| element.ty != **inner) {
                    return;
                }
                expr.ty = match &expr.ty {
                    InferType::FixedArray(_, length) => {
                        InferType::FixedArray(inner.clone(), *length)
                    }
                    InferType::Array(_) => InferType::Array(inner.clone()),
                    InferType::Vec(_) => InferType::Vec(inner.clone()),
                    other => other.clone(),
                };
            }
            _ => {}
        }
    }

    fn retype_int_literal(&mut self, expr: &mut TypedExpr, target: &InferType) {
        match &mut expr.kind {
            TypedExprKind::Int(value) => {
                if !InferType::int_fits(*value, target) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Mismatch {
                            expected: target.clone(),
                            found: InferType::I64,
                        },
                        span: expr.span,
                        reason: ConstraintReason::IntLiteralOverflow {
                            value: *value,
                            target: target.clone(),
                        },
                    });
                }
            }
            TypedExprKind::Grouping(inner) | TypedExprKind::Unary { operand: inner, .. } => {
                self.retype_int_literal(inner, target);
            }
            TypedExprKind::Binary { left, right, .. } => {
                self.retype_int_literal(left, target);
                self.retype_int_literal(right, target);
            }
            TypedExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.retype_int_literal(then_branch, target);
                self.retype_int_literal(else_branch, target);
            }
            TypedExprKind::Match { arms, .. } => {
                for arm in arms.iter_mut() {
                    if let crate::typed_ast::TypedMatchArmBody::Expr(body) = &mut arm.body {
                        self.retype_int_literal(body, target);
                    }
                }
            }
            _ => return,
        }
        expr.ty = target.clone();
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
                InferType::Bool,
                typed_left.ty.clone(),
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
                InferType::Bool,
                typed_right.ty.clone(),
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

pub(crate) fn literal_shaped(expr: &TypedExpr) -> bool {
    match &expr.kind {
        TypedExprKind::Int(_) => expr.ty == InferType::I64,
        TypedExprKind::Grouping(inner) => literal_shaped(inner),
        TypedExprKind::Unary {
            op: aelys_syntax::UnaryOp::Neg | aelys_syntax::UnaryOp::BitNot,
            operand,
        } => literal_shaped(operand),
        TypedExprKind::Binary { left, op, right } => {
            !matches!(
                op,
                aelys_syntax::BinaryOp::Lt
                    | aelys_syntax::BinaryOp::Le
                    | aelys_syntax::BinaryOp::Gt
                    | aelys_syntax::BinaryOp::Ge
                    | aelys_syntax::BinaryOp::Eq
                    | aelys_syntax::BinaryOp::Ne
            ) && literal_shaped(left)
                && literal_shaped(right)
        }
        TypedExprKind::If {
            then_branch,
            else_branch,
            ..
        } => literal_shaped(then_branch) && literal_shaped(else_branch),
        TypedExprKind::Match { arms, .. } => {
            !arms.is_empty()
                && arms.iter().all(|arm| match &arm.body {
                    crate::typed_ast::TypedMatchArmBody::Expr(body) => literal_shaped(body),
                    crate::typed_ast::TypedMatchArmBody::Block(_) => false,
                })
        }
        _ => false,
    }
}

fn retype_open_literal(
    expr: &mut TypedExpr,
    target: &InferType,
    values: &mut Vec<(i64, aelys_syntax::Span)>,
) {
    match &mut expr.kind {
        TypedExprKind::Int(value) => values.push((*value, expr.span)),
        TypedExprKind::Grouping(inner) | TypedExprKind::Unary { operand: inner, .. } => {
            retype_open_literal(inner, target, values);
        }
        TypedExprKind::Binary { left, right, .. } => {
            retype_open_literal(left, target, values);
            retype_open_literal(right, target, values);
        }
        TypedExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            retype_open_literal(then_branch, target, values);
            retype_open_literal(else_branch, target, values);
        }
        TypedExprKind::Match { arms, .. } => {
            for arm in arms.iter_mut() {
                if let crate::typed_ast::TypedMatchArmBody::Expr(body) = &mut arm.body {
                    retype_open_literal(body, target, values);
                }
            }
        }
        _ => return,
    }
    expr.ty = target.clone();
}
