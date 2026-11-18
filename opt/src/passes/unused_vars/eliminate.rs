use super::super::OptimizationStats;
use crate::passes::effects::has_effects;
use aelys_sema::{InferType, TypedExpr, TypedExprKind, TypedFunction, TypedStmt, TypedStmtKind};
use std::collections::HashSet;

pub fn eliminate_unused_in_block(
    stmts: &mut Vec<TypedStmt>,
    used_vars: &HashSet<String>,
    stats: &mut OptimizationStats,
) {
    for stmt in stmts.iter_mut() {
        eliminate_unused_in_stmt(stmt, used_vars, stats);
    }

    let removable = |stmt: &TypedStmt| match &stmt.kind {
        TypedStmtKind::Let {
            name,
            initializer,
            is_pub,
            ..
        } => !*is_pub && !used_vars.contains(name) && !has_effects(initializer),
        _ => false,
    };

    // the last statement is the value of a script or a block, so a dropped trailing let leaves unit
    if let Some(last) = stmts.last_mut()
        && removable(last)
    {
        stats.dead_code_eliminated += 1;
        let unit = TypedExpr::new(TypedExprKind::Unit, InferType::Unit, last.span);
        last.kind = TypedStmtKind::Expression(unit);
    }

    stmts.retain(|stmt| {
        if removable(stmt) {
            stats.dead_code_eliminated += 1;
            return false;
        }
        true
    });
}

fn eliminate_unused_in_stmt(
    stmt: &mut TypedStmt,
    used_vars: &HashSet<String>,
    stats: &mut OptimizationStats,
) {
    match &mut stmt.kind {
        TypedStmtKind::Block(stmts) => eliminate_unused_in_block(stmts, used_vars, stats),
        TypedStmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            eliminate_unused_in_stmt(then_branch, used_vars, stats);
            if let Some(else_b) = else_branch {
                eliminate_unused_in_stmt(else_b, used_vars, stats);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForEach { body, .. } => {
            eliminate_unused_in_stmt(body, used_vars, stats);
        }
        TypedStmtKind::Function(func) => eliminate_unused_in_function(func, used_vars, stats),
        _ => {}
    }
}

fn eliminate_unused_in_function(
    func: &mut TypedFunction,
    used_vars: &HashSet<String>,
    stats: &mut OptimizationStats,
) {
    let mut local_used = used_vars.clone();
    for stmt in &func.body {
        super::analysis::collect_uses_in_stmt_recursive(stmt, &mut local_used);
    }
    for param in &func.params {
        local_used.insert(param.name.clone());
    }
    eliminate_unused_in_block(&mut func.body, &local_used, stats);
}
