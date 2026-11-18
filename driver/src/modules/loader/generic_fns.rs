use aelys_syntax::{
    Expr, ExprKind, FmtStringPart, MatchArmBody, MemberSeparator, Pattern, PatternKind, Stmt,
    StmtKind,
};
use std::collections::{HashMap, HashSet};

pub fn generic_function_symbol(name: &str, module_path: &str) -> String {
    format!("{name}@{module_path}")
}

// a generic function is specialized by each importer, so its spellings name the carried copy
pub fn rewrite_generic_references(stmts: &mut [Stmt], targets: &HashMap<String, String>) {
    if targets.is_empty() {
        return;
    }
    let own = stmts.iter().filter_map(|stmt| match &stmt.kind {
        StmtKind::Function(func) if !targets.values().any(|target| *target == func.name) => {
            Some(func.name.clone())
        }
        _ => None,
    });
    let mut rewriter = Rewriter {
        targets,
        scopes: vec![own.collect()],
    };
    rewriter.stmts(stmts);
}

struct Rewriter<'a> {
    targets: &'a HashMap<String, String>,
    scopes: Vec<HashSet<String>>,
}

impl Rewriter<'_> {
    fn shadowed(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn bind(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn scoped(&mut self, names: impl IntoIterator<Item = String>, f: impl FnOnce(&mut Self)) {
        self.scopes.push(names.into_iter().collect());
        f(self);
        self.scopes.pop();
    }

    fn target(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Identifier(name) if !self.shadowed(name) => self.targets.get(name).cloned(),
            ExprKind::Member {
                object,
                member,
                separator: MemberSeparator::Path,
            } => match &object.kind {
                ExprKind::Identifier(alias) if !self.shadowed(alias) => {
                    self.targets.get(&format!("{alias}::{member}")).cloned()
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn stmts(&mut self, stmts: &mut [Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn block(&mut self, stmt: &mut Stmt) {
        self.scoped(Vec::new(), |this| this.stmt(stmt));
    }

    fn function(&mut self, func: &mut aelys_syntax::Function) {
        let params = func.params.iter().map(|param| param.name.clone());
        self.scoped(params.collect::<Vec<_>>(), |this| {
            this.stmts(&mut func.body)
        });
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Expression(expr) => self.expr(expr),
            StmtKind::Let {
                name, initializer, ..
            } => {
                self.expr(initializer);
                self.bind(name);
            }
            StmtKind::Block(stmts) => self.scoped(Vec::new(), |this| this.stmts(stmts)),
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.block(then_branch);
                if let Some(else_branch) = else_branch {
                    self.block(else_branch);
                }
            }
            StmtKind::While {
                condition, body, ..
            } => {
                self.expr(condition);
                self.block(body);
            }
            StmtKind::For {
                iterator,
                start,
                end,
                step,
                body,
                ..
            } => {
                self.expr(start);
                self.expr(end);
                if let Some(step) = step.as_mut() {
                    self.expr(step);
                }
                self.scoped(vec![iterator.clone()], |this| this.stmt(body));
            }
            StmtKind::ForEach {
                iterator,
                iterable,
                body,
                ..
            } => {
                self.expr(iterable);
                self.scoped(vec![iterator.clone()], |this| this.stmt(body));
            }
            StmtKind::Return(Some(expr))
            | StmtKind::Break {
                value: Some(expr), ..
            } => self.expr(expr),
            StmtKind::Function(func) => {
                if self.scopes.len() > 1 {
                    self.bind(&func.name);
                }
                self.function(func);
            }
            StmtKind::ImplDecl { methods, .. } => {
                for method in methods {
                    self.function(method);
                }
            }
            StmtKind::TraitDecl { methods, .. } => {
                for method in methods {
                    self.function(&mut method.function);
                }
            }
            StmtKind::Return(None)
            | StmtKind::Break { value: None, .. }
            | StmtKind::Continue { .. }
            | StmtKind::Needs(_)
            | StmtKind::StructDecl { .. }
            | StmtKind::EnumDecl { .. } => {}
        }
    }

    fn expr(&mut self, expr: &mut Expr) {
        if let Some(target) = self.target(expr) {
            expr.kind = ExprKind::Identifier(target);
            return;
        }
        if let Some(repeat) = &mut expr.repeat {
            self.expr(repeat);
        }
        match &mut expr.kind {
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::GenericApply { callee, .. } => self.expr(callee),
            ExprKind::Member { object, .. } => self.expr(object),
            ExprKind::StructLiteral { fields, .. }
            | ExprKind::EnumLiteral { fields, .. }
            | ExprKind::GenericEnumLiteral { fields, .. } => {
                for field in fields {
                    self.expr(&mut field.value);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    let mut bound = Vec::new();
                    pattern_bindings(&arm.pattern, &mut bound);
                    self.scoped(bound, |this| {
                        if let Some(guard) = &mut arm.guard {
                            this.expr(guard);
                        }
                        match &mut arm.body {
                            MatchArmBody::Expr(body) => this.expr(body),
                            MatchArmBody::Block(stmts) => this.stmts(stmts),
                        }
                    });
                }
            }
            ExprKind::Block(stmts) | ExprKind::Loop { body: stmts, .. } => {
                self.scoped(Vec::new(), |this| this.stmts(stmts))
            }
            ExprKind::Lambda { params, body, .. } => {
                let names: Vec<String> = params.iter().map(|param| param.name.clone()).collect();
                self.scoped(names, |this| this.stmts(body));
            }
            ExprKind::ArrayLiteral { elements, .. } | ExprKind::VecLiteral { elements, .. } => {
                for element in elements {
                    self.expr(element);
                }
            }
            ExprKind::ArraySized { size, .. } => self.expr(size),
            ExprKind::Cast { expr: inner, .. } => self.expr(inner),
            ExprKind::Binary { left, right, .. }
            | ExprKind::And { left, right }
            | ExprKind::Or { left, right } => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Unary { operand, .. }
            | ExprKind::Borrow { operand, .. }
            | ExprKind::Grouping(operand)
            | ExprKind::Try(operand) => self.expr(operand),
            ExprKind::Assign { value, .. } => self.expr(value),
            ExprKind::MemberAssign { object, value, .. } => {
                self.expr(object);
                self.expr(value);
            }
            ExprKind::Index { object, index } => {
                self.expr(object);
                self.expr(index);
            }
            ExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.expr(object);
                self.expr(index);
                self.expr(value);
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.expr(start);
                }
                if let Some(end) = end {
                    self.expr(end);
                }
            }
            ExprKind::Slice { object, range } => {
                self.expr(object);
                self.expr(range);
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.expr(then_branch);
                self.expr(else_branch);
            }
            ExprKind::FmtString(parts) => {
                for part in parts {
                    if let FmtStringPart::Expr(inner) = part {
                        self.expr(inner);
                    }
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Unit
            | ExprKind::Identifier(_) => {}
        }
    }
}

fn pattern_bindings(pattern: &Pattern, out: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Binding(name) => out.push(name.clone()),
        PatternKind::Variant { fields, .. } => {
            for field in fields {
                pattern_bindings(field, out);
            }
        }
        PatternKind::Struct { fields, .. } => {
            for field in fields {
                pattern_bindings(&field.pattern, out);
            }
        }
        PatternKind::Or(patterns) => {
            for pattern in patterns {
                pattern_bindings(pattern, out);
            }
        }
        PatternKind::Wildcard
        | PatternKind::Int(_)
        | PatternKind::String(_)
        | PatternKind::Char(_)
        | PatternKind::Bool(_)
        | PatternKind::Range { .. }
        | PatternKind::CharRange { .. } => {}
    }
}
