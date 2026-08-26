use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedMatchArmBody, TypedStmt, TypedStmtKind};
use crate::types::InferType;
use aelys_syntax::{BinaryOp, Span, UnaryOp};

impl TypeInference {
    pub(super) fn infer_binary_op(
        &mut self,
        op: BinaryOp,
        left: &TypedExpr,
        right: &TypedExpr,
        span: Span,
    ) -> InferType {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let left_invalid = self.reject_dynamic_arithmetic(left, span, op);
                let right_invalid = self.reject_dynamic_arithmetic(right, span, op);
                if left_invalid || right_invalid {
                    return InferType::Poison;
                }
                if mixed_numeric(&left.ty, &right.ty) {
                    return InferType::F64;
                }

                let result_type = self.type_gen.fresh();

                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    right.ty.clone(),
                    span,
                    ConstraintReason::BinaryOp { op: op.to_string() },
                ));

                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    result_type.clone(),
                    span,
                    ConstraintReason::BinaryOp { op: op.to_string() },
                ));

                if op == BinaryOp::Add {
                    let mut options = InferType::all_numeric_types();
                    options.push(InferType::String);
                    self.constraints.push(Constraint::one_of(
                        left.ty.clone(),
                        options,
                        span,
                        ConstraintReason::BinaryOp { op: op.to_string() },
                    ));
                } else {
                    self.constraints.push(Constraint::one_of(
                        left.ty.clone(),
                        InferType::all_numeric_types(),
                        span,
                        ConstraintReason::BinaryOp { op: op.to_string() },
                    ));
                }

                result_type
            }

            BinaryOp::Mod => {
                let left_invalid = self.reject_dynamic_arithmetic(left, span, op);
                let right_invalid = self.reject_dynamic_arithmetic(right, span, op);
                if left_invalid || right_invalid {
                    return InferType::Poison;
                }
                if mixed_numeric(&left.ty, &right.ty) {
                    return InferType::F64;
                }

                let result_type = self.type_gen.fresh();

                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    right.ty.clone(),
                    span,
                    ConstraintReason::BinaryOp { op: op.to_string() },
                ));

                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    result_type.clone(),
                    span,
                    ConstraintReason::BinaryOp { op: op.to_string() },
                ));

                self.constraints.push(Constraint::one_of(
                    left.ty.clone(),
                    InferType::all_numeric_types(),
                    span,
                    ConstraintReason::BinaryOp { op: op.to_string() },
                ));

                result_type
            }

            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let left_invalid = self.reject_dynamic(
                    &left.ty,
                    &InferType::Numeric,
                    span,
                    ConstraintReason::Comparison,
                ) || self.reject_untyped_native(
                    &left.ty,
                    &InferType::Numeric,
                    span,
                    ConstraintReason::Comparison,
                );
                let right_invalid = self.reject_dynamic(
                    &right.ty,
                    &InferType::Numeric,
                    span,
                    ConstraintReason::Comparison,
                ) || self.reject_untyped_native(
                    &right.ty,
                    &InferType::Numeric,
                    span,
                    ConstraintReason::Comparison,
                );
                if left_invalid || right_invalid {
                    return InferType::Bool;
                }
                if !mixed_numeric(&left.ty, &right.ty) {
                    self.constraints.push(Constraint::equal(
                        left.ty.clone(),
                        right.ty.clone(),
                        span,
                        ConstraintReason::Comparison,
                    ));
                }

                self.constraints.push(Constraint::one_of(
                    left.ty.clone(),
                    InferType::all_numeric_types(),
                    span,
                    ConstraintReason::Comparison,
                ));

                InferType::Bool
            }

            BinaryOp::Eq | BinaryOp::Ne => {
                let left_invalid = self.reject_dynamic_value_operand(left, &right.ty, span)
                    || self.reject_untyped_native(
                        &left.ty,
                        &right.ty,
                        span,
                        ConstraintReason::Comparison,
                    );
                let right_invalid = self.reject_dynamic_value_operand(right, &left.ty, span)
                    || self.reject_untyped_native(
                        &right.ty,
                        &left.ty,
                        span,
                        ConstraintReason::Comparison,
                    );
                if left_invalid || right_invalid {
                    return InferType::Bool;
                }
                if !mixed_numeric(&left.ty, &right.ty) {
                    self.constraints.push(Constraint::equal(
                        left.ty.clone(),
                        right.ty.clone(),
                        span,
                        ConstraintReason::Comparison,
                    ));
                }

                InferType::Bool
            }

            BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor => {
                let left_invalid = self.reject_dynamic(
                    &left.ty,
                    &InferType::I64,
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                ) || self.reject_untyped_native(
                    &left.ty,
                    &InferType::I64,
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                );
                let right_invalid = self.reject_dynamic(
                    &right.ty,
                    &InferType::I64,
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                ) || self.reject_untyped_native(
                    &right.ty,
                    &InferType::I64,
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                );
                if left_invalid || right_invalid {
                    return InferType::Poison;
                }
                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    right.ty.clone(),
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                ));

                self.constraints.push(Constraint::one_of(
                    left.ty.clone(),
                    InferType::all_integer_types(),
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                ));

                let result_type = self.type_gen.fresh();
                self.constraints.push(Constraint::equal(
                    left.ty.clone(),
                    result_type.clone(),
                    span,
                    ConstraintReason::BitwiseOp { op: op.to_string() },
                ));

                result_type
            }
        }
    }

    fn reject_dynamic_arithmetic(&mut self, expr: &TypedExpr, span: Span, op: BinaryOp) -> bool {
        if matches!(expr.ty, InferType::Dynamic) && !self.is_explicit_dynamic_expr(expr) {
            return false;
        }
        let reason = ConstraintReason::BinaryOp { op: op.to_string() };
        self.reject_dynamic(&expr.ty, &InferType::Numeric, span, reason.clone())
            || self.reject_untyped_native(&expr.ty, &InferType::Numeric, span, reason)
    }

    pub(crate) fn is_explicit_dynamic_expr(&self, expr: &TypedExpr) -> bool {
        if Self::explicit_dynamic_shape(&expr.ty) {
            return true;
        }
        match &expr.kind {
            TypedExprKind::Identifier(name) => {
                self.env.is_explicit_dynamic(name)
                    || self.explicit_dynamic_functions.contains(name)
                    || self
                        .env
                        .lookup(name)
                        .is_some_and(Self::explicit_dynamic_shape)
            }
            TypedExprKind::Grouping(inner)
            | TypedExprKind::Unary { operand: inner, .. }
            | TypedExprKind::Try { operand: inner, .. }
            | TypedExprKind::Cast { expr: inner, .. }
            | TypedExprKind::Lambda(inner) => self.is_explicit_dynamic_expr(inner),
            TypedExprKind::Binary { left, right, .. }
            | TypedExprKind::And { left, right }
            | TypedExprKind::Or { left, right } => {
                self.is_explicit_dynamic_expr(left) || self.is_explicit_dynamic_expr(right)
            }
            TypedExprKind::Call { callee, args } => {
                let explicit_function = typed_path_name(callee)
                    .is_some_and(|name| self.explicit_dynamic_functions.contains(&name));
                explicit_function
                    || self.is_explicit_dynamic_expr(callee)
                    || args.iter().any(|arg| self.is_explicit_dynamic_expr(arg))
            }
            TypedExprKind::Assign { value, .. }
            | TypedExprKind::Member { object: value, .. }
            | TypedExprKind::StructField { object: value, .. }
            | TypedExprKind::StructMethod { object: value, .. }
            | TypedExprKind::Index { object: value, .. }
            | TypedExprKind::Slice { object: value, .. } => self.is_explicit_dynamic_expr(value),
            TypedExprKind::MemberAssign { object, value, .. } => {
                self.is_explicit_dynamic_expr(object) || self.is_explicit_dynamic_expr(value)
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.is_explicit_dynamic_expr(condition)
                    || self.is_explicit_dynamic_expr(then_branch)
                    || self.is_explicit_dynamic_expr(else_branch)
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.is_explicit_dynamic_expr(object)
                    || self.is_explicit_dynamic_expr(index)
                    || self.is_explicit_dynamic_expr(value)
            }
            TypedExprKind::ArrayLiteral { elements, .. }
            | TypedExprKind::VecLiteral { elements, .. } => {
                expr.ty.contains_dynamic()
                    || elements
                        .iter()
                        .any(|element| self.is_explicit_dynamic_expr(element))
            }
            TypedExprKind::ArraySized { size, .. } => self.is_explicit_dynamic_expr(size),
            TypedExprKind::Range { start, end, .. } => {
                start
                    .as_deref()
                    .is_some_and(|value| self.is_explicit_dynamic_expr(value))
                    || end
                        .as_deref()
                        .is_some_and(|value| self.is_explicit_dynamic_expr(value))
            }
            TypedExprKind::StructLiteral { fields, .. } => fields
                .iter()
                .any(|(_, value)| self.is_explicit_dynamic_expr(value)),
            TypedExprKind::EnumConstruct { fields, .. } => fields
                .iter()
                .any(|(_, value)| self.is_explicit_dynamic_expr(value)),
            TypedExprKind::LambdaInner { return_type, .. } => {
                matches!(return_type, InferType::Dynamic)
            }
            TypedExprKind::Match { scrutinee, arms } => {
                self.is_explicit_dynamic_expr(scrutinee)
                    || arms.iter().any(|arm| {
                        arm.explicit_dynamic
                            || arm
                                .guard
                                .as_ref()
                                .is_some_and(|guard| self.is_explicit_dynamic_expr(guard))
                            || match &arm.body {
                                TypedMatchArmBody::Expr(expr) => {
                                    self.is_explicit_dynamic_expr(expr)
                                }
                                TypedMatchArmBody::Block(stmts) => {
                                    self.is_explicit_dynamic_stmt_tail(stmts)
                                }
                            }
                    })
            }
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::FmtString(_)
            | TypedExprKind::Unit
            | TypedExprKind::AssociatedConst { .. }
            | TypedExprKind::Null => false,
        }
    }

    fn explicit_dynamic_shape(ty: &InferType) -> bool {
        match ty {
            InferType::Array(inner)
            | InferType::FixedArray(inner, _)
            | InferType::Vec(inner)
            | InferType::Option(inner) => Self::explicit_dynamic_value(inner),
            InferType::Result(ok, err) => {
                Self::explicit_dynamic_value(ok) || Self::explicit_dynamic_value(err)
            }
            InferType::Tuple(elements) => elements.iter().any(Self::explicit_dynamic_value),
            _ => false,
        }
    }

    fn explicit_dynamic_value(ty: &InferType) -> bool {
        matches!(ty, InferType::Dynamic) || Self::explicit_dynamic_shape(ty)
    }

    pub(crate) fn is_explicit_dynamic_stmt_tail(&self, stmts: &[TypedStmt]) -> bool {
        stmts
            .last()
            .is_some_and(|stmt| self.is_explicit_dynamic_stmt(stmt))
    }

    pub(crate) fn is_explicit_dynamic_stmt(&self, stmt: &TypedStmt) -> bool {
        match &stmt.kind {
            TypedStmtKind::Expression(expr) => self.is_explicit_dynamic_expr(expr),
            TypedStmtKind::Block(stmts) => self.is_explicit_dynamic_stmt_tail(stmts),
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.is_explicit_dynamic_expr(condition)
                    || self.is_explicit_dynamic_stmt(then_branch)
                    || else_branch
                        .as_deref()
                        .is_some_and(|branch| self.is_explicit_dynamic_stmt(branch))
            }
            TypedStmtKind::While { condition, body } => {
                self.is_explicit_dynamic_expr(condition) || self.is_explicit_dynamic_stmt(body)
            }
            TypedStmtKind::For {
                start,
                end,
                step,
                body,
                ..
            } => {
                self.is_explicit_dynamic_expr(start)
                    || self.is_explicit_dynamic_expr(end)
                    || step
                        .as_ref()
                        .as_ref()
                        .is_some_and(|value| self.is_explicit_dynamic_expr(value))
                    || self.is_explicit_dynamic_stmt(body)
            }
            TypedStmtKind::ForEach { iterable, body, .. } => {
                self.is_explicit_dynamic_expr(iterable) || self.is_explicit_dynamic_stmt(body)
            }
            TypedStmtKind::Return(expr) => expr
                .as_ref()
                .is_some_and(|value| self.is_explicit_dynamic_expr(value)),
            TypedStmtKind::Let { initializer, .. } => self.is_explicit_dynamic_expr(initializer),
            TypedStmtKind::Function(function) => function
                .body
                .last()
                .is_some_and(|stmt| self.is_explicit_dynamic_stmt(stmt)),
            TypedStmtKind::ImplDecl { methods, .. } => methods.iter().any(|function| {
                function
                    .body
                    .last()
                    .is_some_and(|stmt| self.is_explicit_dynamic_stmt(stmt))
            }),
            TypedStmtKind::Break
            | TypedStmtKind::Continue
            | TypedStmtKind::Needs(_)
            | TypedStmtKind::StructDecl { .. }
            | TypedStmtKind::TraitDecl { .. }
            | TypedStmtKind::EnumDecl { .. } => false,
        }
    }

    pub(super) fn infer_unary_op(
        &mut self,
        op: UnaryOp,
        operand: &TypedExpr,
        span: Span,
    ) -> InferType {
        match op {
            UnaryOp::Neg => {
                let invalid = self.reject_dynamic_unary_operand(
                    operand,
                    &InferType::Numeric,
                    span,
                    ConstraintReason::BinaryOp {
                        op: "-".to_string(),
                    },
                );
                if invalid {
                    return InferType::Poison;
                }
                self.constraints.push(Constraint::one_of(
                    operand.ty.clone(),
                    InferType::all_numeric_types(),
                    span,
                    ConstraintReason::BinaryOp {
                        op: "-".to_string(),
                    },
                ));
                operand.ty.clone()
            }
            UnaryOp::Not => {
                let invalid = self.reject_dynamic(
                    &operand.ty,
                    &InferType::Bool,
                    span,
                    ConstraintReason::IfCondition,
                ) || self.reject_untyped_native(
                    &operand.ty,
                    &InferType::Bool,
                    span,
                    ConstraintReason::IfCondition,
                );
                if !invalid {
                    self.constraints.push(Constraint::equal(
                        operand.ty.clone(),
                        InferType::Bool,
                        span,
                        ConstraintReason::IfCondition,
                    ));
                }
                InferType::Bool
            }
            UnaryOp::BitNot => {
                let invalid = self.reject_dynamic_unary_operand(
                    operand,
                    &InferType::I64,
                    span,
                    ConstraintReason::BitwiseOp {
                        op: "~".to_string(),
                    },
                );
                if invalid {
                    return InferType::Poison;
                }
                self.constraints.push(Constraint::one_of(
                    operand.ty.clone(),
                    InferType::all_integer_types(),
                    span,
                    ConstraintReason::BitwiseOp {
                        op: "~".to_string(),
                    },
                ));
                operand.ty.clone()
            }
        }
    }

    fn reject_dynamic_unary_operand(
        &mut self,
        expr: &TypedExpr,
        expected: &InferType,
        span: Span,
        reason: ConstraintReason,
    ) -> bool {
        if matches!(expr.ty, InferType::Dynamic) && !self.is_explicit_dynamic_expr(expr) {
            return false;
        }
        self.reject_dynamic(&expr.ty, expected, span, reason.clone())
            || self.reject_untyped_native(&expr.ty, expected, span, reason)
    }

    fn reject_dynamic_value_operand(
        &mut self,
        expr: &TypedExpr,
        expected: &InferType,
        span: Span,
    ) -> bool {
        if matches!(expr.ty, InferType::Dynamic) && !self.is_explicit_dynamic_expr(expr) {
            return false;
        }
        self.reject_dynamic(&expr.ty, expected, span, ConstraintReason::Comparison)
    }
}

fn mixed_numeric(left: &InferType, right: &InferType) -> bool {
    (left.is_integer() && right.is_float()) || (left.is_float() && right.is_integer())
}

fn typed_path_name(expr: &TypedExpr) -> Option<String> {
    match &expr.kind {
        TypedExprKind::Identifier(name) => Some(name.clone()),
        TypedExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } => {
            let mut path = typed_path_name(object)?;
            path.push_str("::");
            path.push_str(member);
            Some(path)
        }
        _ => None,
    }
}
