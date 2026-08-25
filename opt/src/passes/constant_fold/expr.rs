use super::ConstantFolder;
use aelys_sema::{TypedExpr, TypedExprKind};

mod binary;
mod logic;
mod unary;

impl ConstantFolder {
    pub(super) fn unwrap_grouping(expr: &TypedExpr) -> &TypedExpr {
        match &expr.kind {
            TypedExprKind::Grouping(inner) => Self::unwrap_grouping(inner),
            _ => expr,
        }
    }

    pub(super) fn try_fold(&mut self, expr: &TypedExpr) -> Option<TypedExpr> {
        match &expr.kind {
            TypedExprKind::Binary { left, op, right } => {
                self.try_fold_binary(left, *op, right, expr)
            }
            TypedExprKind::Unary { op, operand } => self.try_fold_unary(*op, operand, expr),
            TypedExprKind::Grouping(inner) => self.try_fold(inner),
            TypedExprKind::And { left, right } => self.try_fold_and(left, right, expr),
            TypedExprKind::Or { left, right } => self.try_fold_or(left, right, expr),
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null => None,
            TypedExprKind::Try { .. } | TypedExprKind::Match { .. } => None,
            _ => None,
        }
    }

    pub fn optimize_expr(&mut self, expr: &mut TypedExpr) {
        match &mut expr.kind {
            TypedExprKind::Binary { left, right, .. } => {
                self.optimize_expr(left);
                self.optimize_expr(right);
            }
            TypedExprKind::Unary { operand, .. } => self.optimize_expr(operand),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.optimize_expr(left);
                self.optimize_expr(right);
            }
            TypedExprKind::Call { callee, args } => {
                self.optimize_expr(callee);
                for arg in args {
                    self.optimize_expr(arg);
                }
            }
            TypedExprKind::Assign { value, .. } => self.optimize_expr(value),
            TypedExprKind::Grouping(inner) => self.optimize_expr(inner),
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.optimize_expr(condition);
                self.optimize_expr(then_branch);
                self.optimize_expr(else_branch);
            }
            TypedExprKind::Lambda(inner) => self.optimize_expr(inner),
            TypedExprKind::LambdaInner { body, .. } => {
                for stmt in body {
                    self.optimize_stmt(stmt);
                }
            }
            TypedExprKind::Member { object, .. } => self.optimize_expr(object),
            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => self.optimize_expr(object),
            TypedExprKind::MemberAssign { object, value, .. } => {
                self.optimize_expr(object);
                self.optimize_expr(value);
            }
            TypedExprKind::ArrayLiteral { elements, .. }
            | TypedExprKind::VecLiteral { elements, .. } => {
                for elem in elements {
                    self.optimize_expr(elem);
                }
            }
            TypedExprKind::ArraySized { size, .. } => {
                self.optimize_expr(size);
            }
            TypedExprKind::Index { object, index } => {
                self.optimize_expr(object);
                self.optimize_expr(index);
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.optimize_expr(object);
                self.optimize_expr(index);
                self.optimize_expr(value);
            }
            TypedExprKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.optimize_expr(s);
                }
                if let Some(e) = end {
                    self.optimize_expr(e);
                }
            }
            TypedExprKind::Slice { object, range } => {
                self.optimize_expr(object);
                self.optimize_expr(range);
            }
            TypedExprKind::FmtString(parts) => {
                for part in parts {
                    if let aelys_sema::TypedFmtStringPart::Expr(e) = part {
                        self.optimize_expr(e);
                    }
                }
            }
            TypedExprKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.optimize_expr(value);
                }
            }
            TypedExprKind::EnumConstruct { fields, .. } => {
                for (_, value) in fields {
                    self.optimize_expr(value);
                }
            }
            TypedExprKind::Cast { expr, .. } => {
                self.optimize_expr(expr);
            }
            TypedExprKind::Try { operand, .. } => self.optimize_expr(operand),
            TypedExprKind::Match { scrutinee, arms } => {
                self.optimize_expr(scrutinee);
                for arm in arms {
                    if let Some(guard) = &mut arm.guard {
                        self.optimize_expr(guard);
                    }
                    match &mut arm.body {
                        aelys_sema::TypedMatchArmBody::Expr(expr) => self.optimize_expr(expr),
                        aelys_sema::TypedMatchArmBody::Block(stmts) => {
                            for stmt in stmts {
                                self.optimize_stmt(stmt);
                            }
                        }
                    }
                }
            }
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null
            | TypedExprKind::Identifier(_) => {}
        }

        if let Some(folded) = self.try_fold(expr) {
            *expr = folded;
        }
    }
}
