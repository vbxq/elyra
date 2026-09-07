use super::TypeInference;
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedParam, TypedStmt, TypedStmtKind};
use crate::types::InferType;
use std::collections::HashSet;

impl TypeInference {
    pub(super) fn collect_captures_from_stmts(
        &self,
        stmts: &[TypedStmt],
        params: &[TypedParam],
    ) -> Vec<(String, InferType)> {
        let mut captures = Vec::new();
        let mut seen = HashSet::new();

        // a name the body binds itself shadows the enclosing one, so it is never an upvalue
        let mut bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();

        for stmt in stmts {
            self.collect_captures_from_stmt(stmt, &mut bound, &mut captures, &mut seen);
        }

        captures
    }

    fn collect_captures_from_stmt(
        &self,
        stmt: &TypedStmt,
        bound: &mut HashSet<String>,
        captures: &mut Vec<(String, InferType)>,
        seen: &mut HashSet<String>,
    ) {
        match &stmt.kind {
            TypedStmtKind::Expression(expr) => {
                self.collect_captures_inner(expr, bound, captures, seen);
            }
            TypedStmtKind::Let {
                name, initializer, ..
            } => {
                self.collect_captures_inner(initializer, bound, captures, seen);
                bound.insert(name.clone());
            }
            TypedStmtKind::Block(stmts) => {
                let outer = bound.clone();
                for s in stmts {
                    self.collect_captures_from_stmt(s, bound, captures, seen);
                }
                *bound = outer;
            }
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_captures_inner(condition, bound, captures, seen);
                self.collect_captures_from_stmt(then_branch, bound, captures, seen);
                if let Some(els) = else_branch {
                    self.collect_captures_from_stmt(els, bound, captures, seen);
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.collect_captures_inner(condition, bound, captures, seen);
                self.collect_captures_from_stmt(body, bound, captures, seen);
            }
            TypedStmtKind::For {
                iterator,
                start,
                end,
                step,
                body,
                ..
            } => {
                self.collect_captures_inner(start, bound, captures, seen);
                self.collect_captures_inner(end, bound, captures, seen);
                if let Some(step_expr) = step.as_ref().as_ref() {
                    self.collect_captures_inner(step_expr, bound, captures, seen);
                }
                let outer = bound.clone();
                bound.insert(iterator.clone());
                self.collect_captures_from_stmt(body, bound, captures, seen);
                *bound = outer;
            }
            TypedStmtKind::ForEach {
                iterator,
                iterable,
                body,
                ..
            } => {
                self.collect_captures_inner(iterable, bound, captures, seen);
                let outer = bound.clone();
                bound.insert(iterator.clone());
                self.collect_captures_from_stmt(body, bound, captures, seen);
                *bound = outer;
            }
            TypedStmtKind::Return(Some(expr)) => {
                self.collect_captures_inner(expr, bound, captures, seen);
            }
            TypedStmtKind::Return(None) | TypedStmtKind::Break | TypedStmtKind::Continue => {}
            TypedStmtKind::Function(_) => {}
            TypedStmtKind::ImplDecl { .. } => {}
            TypedStmtKind::Needs(_) => {}
            TypedStmtKind::StructDecl { .. } => {}
            TypedStmtKind::EnumDecl { .. } => {}
            TypedStmtKind::TraitDecl { .. } => {}
        }
    }

    fn collect_captures_inner(
        &self,
        expr: &TypedExpr,
        bound: &mut HashSet<String>,
        captures: &mut Vec<(String, InferType)>,
        seen: &mut HashSet<String>,
    ) {
        match &expr.kind {
            TypedExprKind::Identifier(name) => {
                if !bound.contains(name)
                    && !seen.contains(name)
                    && let Some(ty) = self.env.captures().get(name).or_else(|| {
                        self.env
                            .captures()
                            .get(crate::infer::unscoped_global_name(name))
                    })
                {
                    captures.push((name.clone(), ty.clone()));
                    seen.insert(name.clone());
                }
            }
            TypedExprKind::Binary { left, right, .. } => {
                self.collect_captures_inner(left, bound, captures, seen);
                self.collect_captures_inner(right, bound, captures, seen);
            }
            TypedExprKind::Unary { operand, .. } => {
                self.collect_captures_inner(operand, bound, captures, seen);
            }
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.collect_captures_inner(left, bound, captures, seen);
                self.collect_captures_inner(right, bound, captures, seen);
            }
            TypedExprKind::Call { callee, args } => {
                self.collect_captures_inner(callee, bound, captures, seen);
                for arg in args {
                    self.collect_captures_inner(arg, bound, captures, seen);
                }
            }
            TypedExprKind::Assign { value, .. } => {
                self.collect_captures_inner(value, bound, captures, seen);
            }
            TypedExprKind::Grouping(inner) => {
                self.collect_captures_inner(inner, bound, captures, seen);
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_captures_inner(condition, bound, captures, seen);
                self.collect_captures_inner(then_branch, bound, captures, seen);
                self.collect_captures_inner(else_branch, bound, captures, seen);
            }
            TypedExprKind::Try { operand, .. } => {
                self.collect_captures_inner(operand, bound, captures, seen);
            }
            TypedExprKind::Match { scrutinee, arms } => {
                self.collect_captures_inner(scrutinee, bound, captures, seen);
                for arm in arms {
                    let outer = bound.clone();
                    collect_pattern_bindings(&arm.pattern, bound);
                    if let Some(guard) = &arm.guard {
                        self.collect_captures_inner(guard, bound, captures, seen);
                    }
                    match &arm.body {
                        crate::typed_ast::TypedMatchArmBody::Expr(expr) => {
                            self.collect_captures_inner(expr, bound, captures, seen);
                        }
                        crate::typed_ast::TypedMatchArmBody::Block(stmts) => {
                            for stmt in stmts {
                                self.collect_captures_from_stmt(stmt, bound, captures, seen);
                            }
                        }
                    }
                    *bound = outer;
                }
            }
            TypedExprKind::Lambda(inner) => {
                self.collect_captures_inner(inner, bound, captures, seen);
            }
            TypedExprKind::LambdaInner {
                params: lambda_params,
                body: stmts,
                ..
            } => {
                let outer = bound.clone();
                bound.extend(lambda_params.iter().map(|param| param.name.clone()));
                for stmt in stmts {
                    self.collect_captures_from_stmt(stmt, bound, captures, seen);
                }
                *bound = outer;
            }
            TypedExprKind::Member { object, .. } => {
                self.collect_captures_inner(object, bound, captures, seen);
            }
            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => {
                self.collect_captures_inner(object, bound, captures, seen);
            }
            TypedExprKind::MemberAssign { object, value, .. } => {
                self.collect_captures_inner(object, bound, captures, seen);
                self.collect_captures_inner(value, bound, captures, seen);
            }
            TypedExprKind::ArrayLiteral { elements, .. }
            | TypedExprKind::VecLiteral { elements, .. } => {
                for elem in elements {
                    self.collect_captures_inner(elem, bound, captures, seen);
                }
            }
            TypedExprKind::ArraySized { size, .. } => {
                self.collect_captures_inner(size, bound, captures, seen);
            }
            TypedExprKind::Index { object, index } => {
                self.collect_captures_inner(object, bound, captures, seen);
                self.collect_captures_inner(index, bound, captures, seen);
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.collect_captures_inner(object, bound, captures, seen);
                self.collect_captures_inner(index, bound, captures, seen);
                self.collect_captures_inner(value, bound, captures, seen);
            }
            TypedExprKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.collect_captures_inner(s, bound, captures, seen);
                }
                if let Some(e) = end {
                    self.collect_captures_inner(e, bound, captures, seen);
                }
            }
            TypedExprKind::Slice { object, range } => {
                self.collect_captures_inner(object, bound, captures, seen);
                self.collect_captures_inner(range, bound, captures, seen);
            }
            TypedExprKind::FmtString(parts) => {
                for part in parts {
                    if let crate::typed_ast::TypedFmtStringPart::Expr(e) = part {
                        self.collect_captures_inner(e, bound, captures, seen);
                    }
                }
            }
            TypedExprKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.collect_captures_inner(value, bound, captures, seen);
                }
            }
            TypedExprKind::EnumConstruct { fields, .. } => {
                for (_, value) in fields {
                    self.collect_captures_inner(value, bound, captures, seen);
                }
            }
            TypedExprKind::Cast { expr, .. } => {
                self.collect_captures_inner(expr, bound, captures, seen);
            }
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::AssociatedConst { .. }
            | TypedExprKind::Null => {}
        }
    }
}

fn collect_pattern_bindings(pattern: &crate::typed_ast::TypedPattern, bound: &mut HashSet<String>) {
    use crate::typed_ast::TypedPatternKind;
    match &pattern.kind {
        TypedPatternKind::Binding(name) => {
            bound.insert(name.clone());
        }
        TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
            for field in fields {
                collect_pattern_bindings(field, bound);
            }
        }
        TypedPatternKind::Struct { fields, .. } => {
            for (_, field, _) in fields {
                collect_pattern_bindings(field, bound);
            }
        }
        TypedPatternKind::Wildcard
        | TypedPatternKind::Int(_)
        | TypedPatternKind::String(_)
        | TypedPatternKind::Bool(_) => {}
    }
}
