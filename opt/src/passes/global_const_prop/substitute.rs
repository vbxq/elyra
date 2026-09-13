use super::GlobalConstantPropagator;
use aelys_sema::{
    TypedExpr, TypedExprKind, TypedFunction, TypedPattern, TypedPatternKind, TypedStmt,
    TypedStmtKind,
};

impl GlobalConstantPropagator {
    pub(super) fn substitute_constants(&mut self, expr: &mut TypedExpr) {
        match &mut expr.kind {
            TypedExprKind::Identifier(name) => {
                let known = if self.shadows.is_shadowed(name) {
                    None
                } else {
                    self.constants.get(name)
                };
                if let Some(c) = known {
                    let ty = if expr.ty.is_integer() && c.ty.is_integer() {
                        expr.ty.clone()
                    } else {
                        c.ty.clone()
                    };
                    let span = c.span;
                    *expr = TypedExpr::new(c.kind.clone(), ty, span);
                    self.stats.globals_propagated += 1;
                }
            }
            TypedExprKind::Binary { left, right, .. } => {
                self.substitute_constants(left);
                self.substitute_constants(right);
            }
            TypedExprKind::Unary { operand, .. } => self.substitute_constants(operand),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.substitute_constants(left);
                self.substitute_constants(right);
            }
            TypedExprKind::Call { callee, args } => {
                self.substitute_constants(callee);
                for arg in args {
                    self.substitute_constants(arg);
                }
            }
            TypedExprKind::Assign { value, .. } => self.substitute_constants(value),
            TypedExprKind::Grouping(inner) => self.substitute_constants(inner),
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.substitute_constants(condition);
                self.substitute_constants(then_branch);
                self.substitute_constants(else_branch);
            }
            TypedExprKind::Lambda(inner) => self.substitute_constants(inner),
            TypedExprKind::LambdaInner { params, body, .. } => {
                self.shadows.push();
                for param in params.iter() {
                    self.shadows.shadow(param.name.clone());
                }
                for stmt in body {
                    self.substitute_in_stmt(stmt);
                }
                self.shadows.pop();
            }
            TypedExprKind::Member { object, .. } => self.substitute_constants(object),
            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => self.substitute_constants(object),
            TypedExprKind::MemberAssign { object, value, .. } => {
                self.substitute_constants(object);
                self.substitute_constants(value);
            }
            TypedExprKind::ArrayLiteral {
                elements, repeat, ..
            }
            | TypedExprKind::VecLiteral {
                elements, repeat, ..
            } => {
                for elem in elements {
                    self.substitute_constants(elem);
                }
                if let Some(repeat) = repeat {
                    self.substitute_constants(repeat);
                }
            }
            TypedExprKind::ArraySized { size, .. } => {
                self.substitute_constants(size);
            }
            TypedExprKind::Index { object, index } => {
                self.substitute_constants(object);
                self.substitute_constants(index);
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.substitute_constants(object);
                self.substitute_constants(index);
                self.substitute_constants(value);
            }
            TypedExprKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.substitute_constants(s);
                }
                if let Some(e) = end {
                    self.substitute_constants(e);
                }
            }
            TypedExprKind::Slice { object, range } => {
                self.substitute_constants(object);
                self.substitute_constants(range);
            }
            TypedExprKind::FmtString(parts) => {
                for part in parts {
                    if let aelys_sema::TypedFmtStringPart::Expr(e) = part {
                        self.substitute_constants(e);
                    }
                }
            }
            TypedExprKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.substitute_constants(value);
                }
            }
            TypedExprKind::EnumConstruct { fields, .. } => {
                for (_, value) in fields {
                    self.substitute_constants(value);
                }
            }
            TypedExprKind::Cast { expr, .. } => {
                self.substitute_constants(expr);
            }
            TypedExprKind::Try { operand, .. } => self.substitute_constants(operand),
            TypedExprKind::Match { scrutinee, arms } => {
                self.substitute_constants(scrutinee);
                for arm in arms {
                    self.shadows.push();
                    self.shadow_pattern(&arm.pattern);
                    if let Some(guard) = &mut arm.guard {
                        self.substitute_constants(guard);
                    }
                    match &mut arm.body {
                        aelys_sema::TypedMatchArmBody::Expr(expr) => {
                            self.substitute_constants(expr)
                        }
                        aelys_sema::TypedMatchArmBody::Block(stmts) => {
                            for stmt in stmts {
                                self.substitute_in_stmt(stmt);
                            }
                        }
                    }
                    self.shadows.pop();
                }
            }
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null
            | TypedExprKind::AssociatedConst { .. } => {}
        }
    }

    pub(super) fn substitute_in_stmt(&mut self, stmt: &mut TypedStmt) {
        match &mut stmt.kind {
            TypedStmtKind::Expression(expr) => self.substitute_constants(expr),
            TypedStmtKind::Let {
                name, initializer, ..
            } => {
                self.substitute_constants(initializer);
                self.shadows.shadow(name.clone());
            }
            TypedStmtKind::Block(stmts) => {
                self.shadows.push();
                for s in stmts {
                    self.substitute_in_stmt(s);
                }
                self.shadows.pop();
            }
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.substitute_constants(condition);
                self.shadows.push();
                self.substitute_in_stmt(then_branch);
                self.shadows.pop();
                if let Some(else_b) = else_branch {
                    self.shadows.push();
                    self.substitute_in_stmt(else_b);
                    self.shadows.pop();
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.substitute_constants(condition);
                self.shadows.push();
                self.substitute_in_stmt(body);
                self.shadows.pop();
            }
            TypedStmtKind::For {
                iterator,
                start,
                end,
                step,
                body,
                ..
            } => {
                self.substitute_constants(start);
                self.substitute_constants(end);
                if let Some(s) = &mut **step {
                    self.substitute_constants(s);
                }
                self.shadows.push();
                self.shadows.shadow(iterator.clone());
                self.substitute_in_stmt(body);
                self.shadows.pop();
            }
            TypedStmtKind::ForEach {
                iterator,
                iterable,
                body,
                ..
            } => {
                self.substitute_constants(iterable);
                self.shadows.push();
                self.shadows.shadow(iterator.clone());
                self.substitute_in_stmt(body);
                self.shadows.pop();
            }
            TypedStmtKind::Return(Some(expr)) => self.substitute_constants(expr),
            TypedStmtKind::Function(func) => self.substitute_in_function(func),
            TypedStmtKind::ImplDecl { methods, .. } => {
                for method in methods {
                    self.substitute_in_function(method);
                }
            }
            TypedStmtKind::Return(None)
            | TypedStmtKind::Break
            | TypedStmtKind::Continue
            | TypedStmtKind::Needs(_)
            | TypedStmtKind::StructDecl { .. }
            | TypedStmtKind::TraitDecl { .. }
            | TypedStmtKind::EnumDecl { .. } => {}
        }
    }

    fn substitute_in_function(&mut self, func: &mut TypedFunction) {
        self.shadows.push();
        for param in &func.params {
            self.shadows.shadow(param.name.clone());
        }
        for stmt in &mut func.body {
            self.substitute_in_stmt(stmt);
        }
        self.shadows.pop();
    }

    fn shadow_pattern(&mut self, pattern: &TypedPattern) {
        match &pattern.kind {
            TypedPatternKind::Binding(name) => self.shadows.shadow(name.clone()),
            TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
                for field in fields {
                    self.shadow_pattern(field);
                }
            }
            TypedPatternKind::Struct { fields, .. } => {
                for (_, field, _) in fields {
                    self.shadow_pattern(field);
                }
            }
            TypedPatternKind::Wildcard
            | TypedPatternKind::Int(_)
            | TypedPatternKind::String(_)
            | TypedPatternKind::Bool(_) => {}
        }
    }
}
