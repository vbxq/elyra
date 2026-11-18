use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedFmtStringPart, TypedFunction, TypedMatchArmBody, TypedPattern,
    TypedPatternKind, TypedStmt, TypedStmtKind,
};
use crate::types::InferType;
use aelys_syntax::Span;
use std::collections::{HashMap, HashSet};

struct Binding {
    ty: InferType,
    span: Span,
    is_pub: bool,
    used: bool,
    dynamic: bool,
}

struct Scope {
    names: HashMap<String, usize>,
    bindings: Vec<usize>,
}

struct LoopFrame {
    reads: HashSet<usize>,
    // assigned and still unread where the loop is left unconditionally, so no next iteration reads it
    left_unread: HashSet<usize>,
    function_depth: usize,
}

struct Analyzer {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    errors: Vec<TypeError>,
    loops: Vec<LoopFrame>,
    function_depth: usize,
}

fn direct_assignment_name(expr: &TypedExpr) -> Option<String> {
    match &expr.kind {
        TypedExprKind::Assign { name, .. } => Some(name.clone()),
        TypedExprKind::Grouping(inner) => direct_assignment_name(inner),
        _ => None,
    }
}

impl Analyzer {
    fn new() -> Self {
        Self {
            scopes: Vec::new(),
            bindings: Vec::new(),
            errors: Vec::new(),
            loops: Vec::new(),
            function_depth: 0,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            names: HashMap::new(),
            bindings: Vec::new(),
        });
    }

    fn pop_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for binding_id in scope.bindings {
            let binding = &self.bindings[binding_id];
            if binding.is_pub || binding.used {
                continue;
            }
            let kind = match binding.ty {
                InferType::Result(_, _) => TypeErrorKind::IgnoredResult,
                InferType::Option(_) => TypeErrorKind::IgnoredOption,
                _ => continue,
            };
            self.errors.push(TypeError {
                kind,
                span: binding.span,
                reason: ConstraintReason::Other("unused named sum binding".to_string()),
            });
        }
    }

    fn define(&mut self, name: &str, ty: InferType, span: Span, is_pub: bool) {
        let dynamic = ty == InferType::Dynamic;
        self.define_with_dynamic(name, ty, span, is_pub, dynamic);
    }

    fn define_with_dynamic(
        &mut self,
        name: &str,
        ty: InferType,
        span: Span,
        is_pub: bool,
        dynamic: bool,
    ) {
        if name == "_" {
            return;
        }
        let binding_id = self.bindings.len();
        self.bindings.push(Binding {
            ty,
            span,
            is_pub,
            used: false,
            dynamic,
        });
        let scope = self
            .scopes
            .last_mut()
            .expect("must-use analyzer always has a scope");
        scope.names.insert(name.to_string(), binding_id);
        scope.bindings.push(binding_id);
    }

    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter().rev() {
            if let Some(binding_id) = scope.names.get(name) {
                self.bindings[*binding_id].used = true;
                for frame in &mut self.loops {
                    frame.reads.insert(*binding_id);
                }
                return;
            }
        }
    }

    fn mark_assigned(&mut self, name: &str, span: Span, value_ty: &InferType) {
        for scope in self.scopes.iter().rev() {
            if let Some(binding_id) = scope.names.get(name) {
                let binding = &mut self.bindings[*binding_id];
                if binding.dynamic {
                    binding.ty = value_ty.clone();
                }
                binding.used = false;
                binding.span = span;
                let binding_id = *binding_id;
                for frame in &mut self.loops {
                    frame.left_unread.remove(&binding_id);
                }
                return;
            }
        }
    }

    // a value assigned late in a loop body is read by the next iteration's earlier reads
    fn in_loop(&mut self, visit: impl FnOnce(&mut Self)) {
        self.loops.push(LoopFrame {
            reads: HashSet::new(),
            left_unread: HashSet::new(),
            function_depth: self.function_depth,
        });
        visit(self);
        if let Some(frame) = self.loops.pop() {
            for binding_id in frame.reads.difference(&frame.left_unread) {
                self.bindings[*binding_id].used = true;
            }
        }
    }

    fn in_function(&mut self, visit: impl FnOnce(&mut Self)) {
        self.function_depth += 1;
        visit(self);
        self.function_depth -= 1;
    }

    fn visit_stmts(&mut self, stmts: &[TypedStmt]) {
        let mut assigned = Vec::new();
        for stmt in stmts {
            self.visit_stmt(stmt);
            match &stmt.kind {
                TypedStmtKind::Expression(expr) => {
                    if let Some(name) = direct_assignment_name(expr)
                        && let Some(binding_id) = self.lookup(&name)
                    {
                        assigned.push(binding_id);
                    }
                }
                TypedStmtKind::Break { .. } => self.leave_loops(&assigned, false),
                TypedStmtKind::Return(_) => self.leave_loops(&assigned, true),
                _ => {}
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<usize> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.names.get(name).copied())
    }

    fn leave_loops(&mut self, assigned: &[usize], every_loop: bool) {
        let depth = self.function_depth;
        let frames = self
            .loops
            .iter_mut()
            .rev()
            .take_while(|frame| frame.function_depth == depth)
            .take(if every_loop { usize::MAX } else { 1 });
        for frame in frames {
            for binding_id in assigned {
                if !self.bindings[*binding_id].used {
                    frame.left_unread.insert(*binding_id);
                }
            }
        }
    }

    fn visit_stmt(&mut self, stmt: &TypedStmt) {
        match &stmt.kind {
            TypedStmtKind::Expression(expr) => self.visit_expr(expr),
            TypedStmtKind::Let {
                name,
                initializer,
                var_type,
                is_pub,
                ..
            } => {
                if name == "_" {
                    self.visit_expr(initializer);
                } else {
                    self.visit_consuming_expr(initializer);
                }
                let binding_type = if *var_type == InferType::Dynamic {
                    initializer.ty.clone()
                } else {
                    var_type.clone()
                };
                self.define_with_dynamic(
                    name,
                    binding_type,
                    stmt.span,
                    *is_pub,
                    *var_type == InferType::Dynamic,
                );
            }
            TypedStmtKind::Block(stmts) => {
                self.push_scope();
                self.visit_stmts(stmts);
                self.pop_scope();
            }
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                self.visit_stmt(then_branch);
                if let Some(else_branch) = else_branch {
                    self.visit_stmt(else_branch);
                }
            }
            TypedStmtKind::While {
                condition, body, ..
            } => self.in_loop(|this| {
                this.visit_expr(condition);
                this.visit_stmt(body);
            }),
            TypedStmtKind::For {
                start,
                end,
                step,
                body,
                iterator,
                ..
            } => {
                self.visit_expr(start);
                self.visit_expr(end);
                if let Some(step) = step.as_ref() {
                    self.visit_expr(step);
                }
                self.push_scope();
                self.define(iterator, InferType::I64, stmt.span, false);
                self.in_loop(|this| this.visit_stmt(body));
                self.pop_scope();
            }
            TypedStmtKind::ForEach {
                iterator,
                iterable,
                elem_type,
                body,
                ..
            } => {
                self.visit_expr(iterable);
                self.push_scope();
                self.define(iterator, elem_type.clone(), stmt.span, false);
                self.in_loop(|this| this.visit_stmt(body));
                self.pop_scope();
            }
            TypedStmtKind::Return(expr) | TypedStmtKind::Break { value: expr, .. } => {
                if let Some(expr) = expr {
                    self.visit_consuming_expr(expr);
                }
            }
            TypedStmtKind::Function(function) => self.visit_function(function),
            TypedStmtKind::ImplDecl { methods, .. } => {
                for method in methods {
                    self.visit_function(method);
                }
            }
            TypedStmtKind::Continue { .. }
            | TypedStmtKind::Needs(_)
            | TypedStmtKind::StructDecl { .. }
            | TypedStmtKind::EnumDecl { .. }
            | TypedStmtKind::TraitDecl { .. } => {}
        }
    }

    fn visit_function(&mut self, function: &TypedFunction) {
        self.in_function(|this| {
            this.push_scope();
            for param in &function.params {
                this.define(&param.name, param.ty.clone(), param.span, false);
            }
            this.visit_stmts(&function.body);
            this.pop_scope();
        });
    }

    fn visit_expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::Identifier(name) => self.mark_used(name),
            TypedExprKind::Binary { left, right, .. }
            | TypedExprKind::And { left, right }
            | TypedExprKind::Or { left, right } => {
                self.visit_consuming_expr(left);
                self.visit_consuming_expr(right);
            }
            TypedExprKind::Unary { operand, .. } | TypedExprKind::Try { operand, .. } => {
                self.visit_consuming_expr(operand)
            }
            TypedExprKind::Grouping(operand) => self.visit_expr(operand),
            TypedExprKind::Cast { expr: operand, .. } => self.visit_consuming_expr(operand),
            TypedExprKind::Lambda(operand) => self.visit_expr(operand),
            TypedExprKind::Call { callee, args } => {
                self.visit_consuming_expr(callee);
                for arg in args {
                    self.visit_consuming_expr(arg);
                }
            }
            TypedExprKind::Assign { name, value } => {
                self.visit_expr(value);
                self.mark_assigned(name, expr.span, &value.ty);
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_consuming_expr(condition);
                self.visit_expr(then_branch);
                self.visit_expr(else_branch);
            }
            TypedExprKind::Match { scrutinee, arms } => {
                self.visit_consuming_expr(scrutinee);
                for arm in arms {
                    self.push_scope();
                    self.define_pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.visit_consuming_expr(guard);
                    }
                    match &arm.body {
                        TypedMatchArmBody::Expr(expr) => self.visit_expr(expr),
                        TypedMatchArmBody::Block(stmts) => self.visit_stmts(stmts),
                    }
                    self.pop_scope();
                }
            }
            TypedExprKind::Member { object, .. } => {
                let assignment = direct_assignment_name(object);
                self.visit_expr(object);
                if let Some(name) = assignment {
                    self.mark_used(&name);
                }
            }
            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => self.visit_expr(object),
            TypedExprKind::MemberAssign { object, value, .. } => {
                self.visit_consuming_expr(object);
                self.visit_consuming_expr(value);
            }
            TypedExprKind::Slice { object, range } => {
                self.visit_consuming_expr(object);
                self.visit_consuming_expr(range);
            }
            TypedExprKind::Index { object, index } => {
                self.visit_consuming_expr(object);
                self.visit_consuming_expr(index);
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => {
                self.visit_consuming_expr(object);
                self.visit_consuming_expr(index);
                self.visit_consuming_expr(value);
            }
            TypedExprKind::ArrayLiteral { elements, .. }
            | TypedExprKind::VecLiteral { elements, .. } => {
                for element in elements {
                    self.visit_consuming_expr(element);
                }
            }
            TypedExprKind::ArraySized { size, .. } => self.visit_consuming_expr(size),
            TypedExprKind::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.visit_consuming_expr(start);
                }
                if let Some(end) = end {
                    self.visit_consuming_expr(end);
                }
            }
            TypedExprKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.visit_consuming_expr(value);
                }
            }
            TypedExprKind::EnumConstruct { fields, .. } => {
                for (_, value) in fields {
                    self.visit_consuming_expr(value);
                }
            }
            TypedExprKind::LambdaInner { params, body, .. } => self.in_function(|this| {
                this.push_scope();
                for param in params {
                    this.define(&param.name, param.ty.clone(), param.span, false);
                }
                this.visit_stmts(body);
                this.pop_scope();
            }),
            TypedExprKind::FmtString(parts) => {
                for part in parts {
                    if let TypedFmtStringPart::Expr(expr) = part {
                        self.visit_consuming_expr(expr);
                    }
                }
            }
            TypedExprKind::Block(stmts) => {
                self.push_scope();
                self.visit_stmts(stmts);
                self.pop_scope();
            }
            TypedExprKind::Loop { body: stmts, .. } => self.in_loop(|this| {
                this.push_scope();
                this.visit_stmts(stmts);
                this.pop_scope();
            }),
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Char(_)
            | TypedExprKind::Unit
            | TypedExprKind::AssociatedConst { .. }
            | TypedExprKind::Null => {}
        }
    }

    fn visit_consuming_expr(&mut self, expr: &TypedExpr) {
        let assignment = direct_assignment_name(expr);
        self.visit_expr(expr);
        if let Some(name) = assignment {
            self.mark_used(&name);
        }
    }

    fn define_pattern(&mut self, pattern: &TypedPattern) {
        match &pattern.kind {
            TypedPatternKind::Binding(name) => {
                self.define(name, pattern.ty.clone(), pattern.span, false)
            }
            TypedPatternKind::Variant { fields, .. } => {
                for field in fields {
                    self.define_pattern(field);
                }
            }
            TypedPatternKind::Struct { fields, .. } => {
                for (_, field, _) in fields {
                    self.define_pattern(field);
                }
            }
            TypedPatternKind::Or(patterns) => {
                if let Some(pattern) = patterns.first() {
                    self.define_pattern(pattern);
                }
            }
            TypedPatternKind::Wildcard
            | TypedPatternKind::Int(_)
            | TypedPatternKind::String(_)
            | TypedPatternKind::Char(_)
            | TypedPatternKind::Bool(_)
            | TypedPatternKind::Range { .. }
            | TypedPatternKind::CharRange { .. } => {}
        }
    }
}

impl TypeInference {
    pub(super) fn validate_unused_sum_bindings(&mut self, stmts: &[TypedStmt]) {
        let mut analyzer = Analyzer::new();
        analyzer.push_scope();
        analyzer.visit_stmts(stmts);
        analyzer.pop_scope();
        self.errors.extend(analyzer.errors);
    }
}
