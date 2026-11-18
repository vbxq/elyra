use super::GlobalConstantPropagator;
use crate::passes::effects::Effects;
use crate::passes::unused_vars::analysis::collect_uses_in_stmt_recursive;
use aelys_sema::{TypedExpr, TypedExprKind, TypedStmt, TypedStmtKind};
use std::collections::HashSet;

impl GlobalConstantPropagator {
    pub(super) fn is_constant_expr(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Null => true,
            TypedExprKind::Identifier(name) => self.constants.contains_key(name),
            TypedExprKind::Binary { left, right, .. } => {
                self.is_constant_expr(left) && self.is_constant_expr(right)
            }
            TypedExprKind::Unary { operand, .. } => self.is_constant_expr(operand),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.is_constant_expr(left) && self.is_constant_expr(right)
            }
            TypedExprKind::Grouping(inner) => self.is_constant_expr(inner),
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.is_constant_expr(condition)
                    && self.is_constant_expr(then_branch)
                    && self.is_constant_expr(else_branch)
            }
            // calls, assigns, lambdas, member access - not constant
            _ => false,
        }
    }

    pub(super) fn collect_global_constants(&mut self, stmts: &[TypedStmt]) {
        let rebound = rebound_globals(stmts);

        for _ in 0..10 {
            let prev_count = self.constants.len();

            for stmt in stmts {
                if let TypedStmtKind::Let {
                    name,
                    mutable,
                    initializer,
                    ..
                } = &stmt.kind
                {
                    if *mutable || rebound.contains(name) || self.constants.contains_key(name) {
                        continue;
                    }
                    if self.is_constant_expr(initializer) {
                        let mut resolved = initializer.clone();
                        self.substitute_in_expr_for_collection(&mut resolved);
                        if !Effects::default().has_effects(&resolved) {
                            self.constants.insert(name.clone(), resolved);
                        }
                    }
                }
            }

            if self.constants.len() == prev_count {
                break;
            }
        }

        self.late = late_constants(stmts, &self.constants);
    }

    pub(super) fn substitute_in_expr_for_collection(&self, expr: &mut TypedExpr) {
        match &mut expr.kind {
            TypedExprKind::Identifier(name) => {
                if let Some(c) = self.constants.get(name) {
                    *expr = TypedExpr::new(c.kind.clone(), c.ty.clone(), expr.span);
                }
            }
            TypedExprKind::Binary { left, right, .. } => {
                self.substitute_in_expr_for_collection(left);
                self.substitute_in_expr_for_collection(right);
            }
            TypedExprKind::Unary { operand, .. } => self.substitute_in_expr_for_collection(operand),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.substitute_in_expr_for_collection(left);
                self.substitute_in_expr_for_collection(right);
            }
            TypedExprKind::Grouping(inner) => self.substitute_in_expr_for_collection(inner),
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.substitute_in_expr_for_collection(condition);
                self.substitute_in_expr_for_collection(then_branch);
                self.substitute_in_expr_for_collection(else_branch);
            }
            _ => {}
        }
    }
}

fn late_constants(
    stmts: &[TypedStmt],
    constants: &std::collections::HashMap<String, TypedExpr>,
) -> HashSet<String> {
    let mut late = HashSet::new();
    let mut code_ran = false;
    for stmt in stmts {
        match &stmt.kind {
            TypedStmtKind::Let { name, .. } if constants.contains_key(name) => {
                if code_ran {
                    late.insert(name.clone());
                }
            }
            TypedStmtKind::Function(_)
            | TypedStmtKind::ImplDecl { .. }
            | TypedStmtKind::StructDecl { .. }
            | TypedStmtKind::TraitDecl { .. }
            | TypedStmtKind::EnumDecl { .. }
            | TypedStmtKind::Needs(_) => {}
            _ => code_ran = true,
        }
    }
    late
}

// a name the top level binds twice is two variables, and the substitution walk has no way
fn rebound_globals(stmts: &[TypedStmt]) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut read_so_far = HashSet::new();
    let mut rebound = HashSet::new();
    for stmt in stmts {
        // a name read before its binding still means an import or an earlier value there
        collect_uses_in_stmt_recursive(stmt, &mut read_so_far);
        if let TypedStmtKind::Let { name, .. } = &stmt.kind
            && (!seen.insert(name.clone()) || read_so_far.contains(name))
        {
            rebound.insert(name.clone());
        }
    }
    rebound
}
