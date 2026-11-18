use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason};
use crate::typed_ast::{TypedStmt, TypedStmtKind};
use crate::types::InferType;
use aelys_syntax::{Span, Stmt, StmtKind};

impl TypeInference {
    pub(crate) fn infer_block_value(&mut self, stmts: &[Stmt]) -> (Vec<TypedStmt>, InferType) {
        self.env.push_scope();
        let (typed, ty) = self.infer_stmts_value(stmts);
        self.env.pop_scope();
        (typed, ty.unwrap_or(InferType::Never))
    }

    // none means every path through the statements diverges
    pub(crate) fn infer_stmts_value(
        &mut self,
        stmts: &[Stmt],
    ) -> (Vec<TypedStmt>, Option<InferType>) {
        let Some((last, prefix)) = stmts.split_last() else {
            return (Vec::new(), Some(InferType::Unit));
        };
        let mut typed: Vec<TypedStmt> = prefix.iter().map(|stmt| self.infer_stmt(stmt)).collect();
        let (typed_last, ty) = self.infer_tail_value(last);
        typed.push(typed_last);
        (typed, ty)
    }

    fn infer_tail_value(&mut self, stmt: &Stmt) -> (TypedStmt, Option<InferType>) {
        let wrap = |kind: TypedStmtKind| TypedStmt {
            kind,
            span: stmt.span,
            definition_module: stmt.definition_module.clone(),
        };
        match &stmt.kind {
            StmtKind::Expression(expr) => {
                let typed = self.infer_expr(expr);
                let ty = (!matches!(typed.ty, InferType::Never)).then(|| typed.ty.clone());
                (wrap(TypedStmtKind::Expression(typed)), ty)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch: Some(else_branch),
            } => {
                let typed_cond = self.infer_expr(condition);
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
                let (typed_then, then_ty) = self.infer_tail_value(then_branch);
                let (typed_else, else_ty) = self.infer_tail_value(else_branch);
                let ty = self.join_branch_types(&[
                    (then_ty, then_branch.span),
                    (else_ty, else_branch.span),
                ]);
                (
                    wrap(TypedStmtKind::If {
                        condition: typed_cond,
                        then_branch: Box::new(typed_then),
                        else_branch: Some(Box::new(typed_else)),
                    }),
                    ty,
                )
            }
            StmtKind::Block(stmts) => {
                self.env.push_scope();
                let (typed, ty) = self.infer_stmts_value(stmts);
                self.env.pop_scope();
                (wrap(TypedStmtKind::Block(typed)), ty)
            }
            StmtKind::Return(_) | StmtKind::Break { .. } | StmtKind::Continue { .. } => {
                (self.infer_stmt(stmt), None)
            }
            _ => (self.infer_stmt(stmt), Some(InferType::Unit)),
        }
    }

    pub(crate) fn join_branch_types(
        &mut self,
        branches: &[(Option<InferType>, Span)],
    ) -> Option<InferType> {
        let live: Vec<&(Option<InferType>, Span)> =
            branches.iter().filter(|(ty, _)| ty.is_some()).collect();
        match live.as_slice() {
            [] => None,
            [(ty, _)] => ty.clone(),
            _ => {
                if live
                    .iter()
                    .any(|(ty, _)| matches!(ty, Some(InferType::Dynamic)))
                {
                    return Some(InferType::Dynamic);
                }
                let result = self.type_gen.fresh();
                for (ty, span) in live {
                    let Some(ty) = ty else { continue };
                    if !self.reject_dynamic(ty, &result, *span, ConstraintReason::IfBranches) {
                        self.constraints.push(Constraint::equal(
                            result.clone(),
                            ty.clone(),
                            *span,
                            ConstraintReason::IfBranches,
                        ));
                    }
                }
                Some(result)
            }
        }
    }
}
