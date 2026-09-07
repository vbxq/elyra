use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason};
use crate::typed_ast::{TypedStmt, TypedStmtKind};
use crate::types::InferType;
use aelys_syntax::Stmt;

impl TypeInference {
    pub(crate) fn infer_stmt_with_implicit_return(
        &mut self,
        stmt: &Stmt,
        return_type: &InferType,
    ) -> TypedStmt {
        match &stmt.kind {
            aelys_syntax::StmtKind::Expression(expr) => {
                let mut typed_expr = self.infer_expr(expr);

                if let crate::typed_ast::TypedExprKind::Float(value) = &typed_expr.kind
                    && *return_type == InferType::F32
                    && value.is_finite()
                    && value.abs() <= f32::MAX as f64
                {
                    typed_expr.ty = InferType::F32;
                }

                if matches!(return_type, InferType::Unit | InferType::Dynamic) {
                    self.record_must_use_value(&typed_expr);
                } else if !self.reject_dynamic(
                    &typed_expr.ty,
                    return_type,
                    expr.span,
                    ConstraintReason::Return {
                        func_name: self
                            .env
                            .current_function()
                            .cloned()
                            .unwrap_or_else(|| "<anonymous>".to_string()),
                    },
                ) {
                    self.constraints.push(Constraint::equal(
                        return_type.clone(),
                        typed_expr.ty.clone(),
                        expr.span,
                        ConstraintReason::Return {
                            func_name: self
                                .env
                                .current_function()
                                .cloned()
                                .unwrap_or_else(|| "<anonymous>".to_string()),
                        },
                    ));
                }

                TypedStmt {
                    kind: TypedStmtKind::Expression(typed_expr),
                    span: stmt.span,
                    definition_module: stmt.definition_module.clone(),
                }
            }

            aelys_syntax::StmtKind::If {
                condition,
                then_branch,
                else_branch: Some(else_branch),
            } => {
                let typed_cond = self.infer_expr(condition);

                let invalid_condition = self.reject_dynamic(
                    &typed_cond.ty,
                    &InferType::Bool,
                    condition.span,
                    ConstraintReason::IfCondition,
                ) || self.reject_untyped_native(
                    &typed_cond.ty,
                    &InferType::Bool,
                    condition.span,
                    ConstraintReason::IfCondition,
                );
                if !invalid_condition {
                    self.constraints.push(Constraint::equal(
                        InferType::Bool,
                        typed_cond.ty.clone(),
                        condition.span,
                        ConstraintReason::IfCondition,
                    ));
                }

                let typed_then = self.infer_stmt_with_implicit_return(then_branch, return_type);
                let typed_else = self.infer_stmt_with_implicit_return(else_branch, return_type);

                TypedStmt {
                    kind: TypedStmtKind::If {
                        condition: typed_cond,
                        then_branch: Box::new(typed_then),
                        else_branch: Some(Box::new(typed_else)),
                    },
                    span: stmt.span,
                    definition_module: stmt.definition_module.clone(),
                }
            }

            aelys_syntax::StmtKind::Block(stmts) if !stmts.is_empty() => {
                self.env.push_scope();

                let mut typed_stmts: Vec<TypedStmt> = stmts[..stmts.len() - 1]
                    .iter()
                    .map(|s| self.infer_stmt(s))
                    .collect();

                let typed_last =
                    self.infer_stmt_with_implicit_return(&stmts[stmts.len() - 1], return_type);
                typed_stmts.push(typed_last);

                self.env.pop_scope();

                TypedStmt {
                    kind: TypedStmtKind::Block(typed_stmts),
                    span: stmt.span,
                    definition_module: stmt.definition_module.clone(),
                }
            }

            _ => self.infer_stmt(stmt),
        }
    }
}
