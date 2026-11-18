use super::Compiler;
use aelys_common::Result;
use aelys_sema::{TypedExpr, TypedExprKind, TypedStmt, TypedStmtKind};
use aelys_syntax::ast::BinaryOp;

const HOISTED_PER_LOOP: usize = 4;

impl Compiler {
    pub(crate) fn hoisted_literal(&self, literal: i64) -> Option<u16> {
        self.hoisted_literals
            .iter()
            .find(|(value, _)| *value == literal)
            .map(|(_, register)| *register)
    }

    pub(crate) fn hoist_loop_literals(
        &mut self,
        condition: Option<&TypedExpr>,
        body: &TypedStmt,
    ) -> Result<usize> {
        let mut found = Vec::new();
        if let Some(condition) = condition {
            collect_expr(condition, &mut found);
        }
        collect_stmt(body, &mut found);
        let mut hoisted = 0;
        for literal in found {
            if hoisted == HOISTED_PER_LOOP {
                break;
            }
            if self.hoisted_literal(literal.0).is_some() {
                continue;
            }
            let register = self.alloc_register()?;
            self.compile_typed_expr(&literal.1, register)?;
            self.hoisted_literals.push((literal.0, register));
            hoisted += 1;
        }
        Ok(hoisted)
    }

    pub(crate) fn release_loop_literals(&mut self, count: usize) {
        for _ in 0..count {
            if let Some((_, register)) = self.hoisted_literals.pop() {
                self.free_register(register);
            }
        }
    }
}

fn takes_immediate(op: BinaryOp, literal: i64) -> bool {
    (0..=255).contains(&literal)
        && matches!(
            op,
            BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Shr
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
        )
}

fn note(found: &mut Vec<(i64, TypedExpr)>, literal: &TypedExpr, value: i64) {
    if !found.iter().any(|(seen, _)| *seen == value) {
        found.push((value, literal.clone()));
    }
}

fn collect_expr(expr: &TypedExpr, found: &mut Vec<(i64, TypedExpr)>) {
    match &expr.kind {
        TypedExprKind::Binary { left, op, right } => {
            if let TypedExprKind::Int(value) = right.kind
                && !takes_immediate(*op, value)
                && aelys_sema::ResolvedType::from_infer_type(&left.ty).is_integer()
            {
                note(found, right, value);
            }
            collect_expr(left, found);
            collect_expr(right, found);
        }
        TypedExprKind::Unary { operand, .. } => collect_expr(operand, found),
        TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
            collect_expr(left, found);
            collect_expr(right, found);
        }
        TypedExprKind::Call { callee, args } => {
            collect_expr(callee, found);
            args.iter().for_each(|arg| collect_expr(arg, found));
        }
        TypedExprKind::Assign { value, .. } => collect_expr(value, found),
        TypedExprKind::Grouping(inner) | TypedExprKind::Cast { expr: inner, .. } => {
            collect_expr(inner, found)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expr(condition, found);
            collect_expr(then_branch, found);
            collect_expr(else_branch, found);
        }
        TypedExprKind::Index { object, index } => {
            collect_expr(object, found);
            collect_expr(index, found);
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_expr(object, found);
            collect_expr(index, found);
            collect_expr(value, found);
        }
        TypedExprKind::StructField { object, .. } => collect_expr(object, found),
        TypedExprKind::MemberAssign { object, value, .. } => {
            collect_expr(object, found);
            collect_expr(value, found);
        }
        _ => {}
    }
}

fn collect_stmt(stmt: &TypedStmt, found: &mut Vec<(i64, TypedExpr)>) {
    match &stmt.kind {
        TypedStmtKind::Expression(expr) => collect_expr(expr, found),
        TypedStmtKind::Let { initializer, .. } => collect_expr(initializer, found),
        TypedStmtKind::Block(stmts) => stmts.iter().for_each(|stmt| collect_stmt(stmt, found)),
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expr(condition, found);
            collect_stmt(then_branch, found);
            if let Some(else_branch) = else_branch {
                collect_stmt(else_branch, found);
            }
        }
        TypedStmtKind::While {
            condition, body, ..
        } => {
            collect_expr(condition, found);
            collect_stmt(body, found);
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            collect_expr(start, found);
            collect_expr(end, found);
            if let Some(step) = step.as_ref() {
                collect_expr(step, found);
            }
            collect_stmt(body, found);
        }
        TypedStmtKind::Return(Some(expr)) => collect_expr(expr, found),
        _ => {}
    }
}
