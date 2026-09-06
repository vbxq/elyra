use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::TypedStmtKind;
use crate::types::InferType;
use aelys_syntax::{Expr, Span, Stmt};

impl TypeInference {
    pub(super) fn infer_if_stmt(
        &mut self,
        condition: &Expr,
        then_branch: &Stmt,
        else_branch: Option<&Stmt>,
    ) -> TypedStmtKind {
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

        let typed_then = self.infer_stmt(then_branch);
        let typed_else = else_branch.map(|e| Box::new(self.infer_stmt(e)));

        TypedStmtKind::If {
            condition: typed_cond,
            then_branch: Box::new(typed_then),
            else_branch: typed_else,
        }
    }

    pub(super) fn infer_while_stmt(&mut self, condition: &Expr, body: &Stmt) -> TypedStmtKind {
        let typed_cond = self.infer_expr(condition);

        if !self.reject_dynamic(
            &typed_cond.ty,
            &InferType::Bool,
            condition.span,
            ConstraintReason::WhileCondition,
        ) && !self.reject_untyped_native(
            &typed_cond.ty,
            &InferType::Bool,
            condition.span,
            ConstraintReason::WhileCondition,
        ) {
            self.constraints.push(Constraint::equal(
                InferType::Bool,
                typed_cond.ty.clone(),
                condition.span,
                ConstraintReason::WhileCondition,
            ));
        }

        let typed_body = self.infer_stmt(body);

        TypedStmtKind::While {
            condition: typed_cond,
            body: Box::new(typed_body),
        }
    }

    pub(super) fn infer_for_stmt(
        &mut self,
        iterator: &str,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        step: Option<&Expr>,
        body: &Stmt,
    ) -> TypedStmtKind {
        let typed_start = self.infer_expr(start);
        let typed_end = self.infer_expr(end);
        let typed_step = step.map(|s| self.infer_expr(s));

        if !self.reject_dynamic(
            &typed_start.ty,
            &InferType::I64,
            start.span,
            ConstraintReason::ForBounds,
        ) && !self.reject_untyped_native(
            &typed_start.ty,
            &InferType::I64,
            start.span,
            ConstraintReason::ForBounds,
        ) {
            self.constraints.push(Constraint::equal(
                InferType::I64,
                typed_start.ty.clone(),
                start.span,
                ConstraintReason::ForBounds,
            ));
        }
        if !self.reject_dynamic(
            &typed_end.ty,
            &InferType::I64,
            end.span,
            ConstraintReason::ForBounds,
        ) && !self.reject_untyped_native(
            &typed_end.ty,
            &InferType::I64,
            end.span,
            ConstraintReason::ForBounds,
        ) {
            self.constraints.push(Constraint::equal(
                InferType::I64,
                typed_end.ty.clone(),
                end.span,
                ConstraintReason::ForBounds,
            ));
        }
        if let Some(ref ts) = typed_step {
            let step_span = step.map(|s| s.span).unwrap_or(body.span);
            if !self.reject_dynamic(
                &ts.ty,
                &InferType::I64,
                step_span,
                ConstraintReason::ForBounds,
            ) && !self.reject_untyped_native(
                &ts.ty,
                &InferType::I64,
                step_span,
                ConstraintReason::ForBounds,
            ) {
                self.constraints.push(Constraint::equal(
                    InferType::I64,
                    ts.ty.clone(),
                    step_span,
                    ConstraintReason::ForBounds,
                ));
            }
        }

        self.env.push_scope();
        self.env.define_local(iterator.to_string(), InferType::I64);
        let typed_body = self.infer_stmt(body);
        self.env.pop_scope();

        TypedStmtKind::For {
            iterator: iterator.to_string(),
            start: typed_start,
            end: typed_end,
            inclusive,
            step: Box::new(typed_step),
            body: Box::new(typed_body),
        }
    }

    pub(super) fn infer_for_each_stmt(
        &mut self,
        iterator: &str,
        iterable: &Expr,
        body: &Stmt,
        read_only: bool,
        _span: Span,
    ) -> TypedStmtKind {
        let typed_iterable = self.infer_expr(iterable);

        let elem_type = match &typed_iterable.ty {
            InferType::String => InferType::String,
            InferType::Vec(inner) => (**inner).clone(),
            InferType::Array(inner) | InferType::FixedArray(inner, _) => (**inner).clone(),
            InferType::Dynamic => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NotIterable {
                        receiver: InferType::Dynamic,
                    },
                    span: iterable.span,
                    reason: ConstraintReason::Other("for iteration".to_string()),
                });
                InferType::Poison
            }
            InferType::Var(_) => {
                let element = self.type_gen.fresh();
                self.constraints.push(Constraint::one_of(
                    typed_iterable.ty.clone(),
                    vec![
                        InferType::String,
                        InferType::Array(Box::new(element.clone())),
                        InferType::Vec(Box::new(element.clone())),
                    ],
                    iterable.span,
                    ConstraintReason::Other("for iteration".to_string()),
                ));
                element
            }
            InferType::UntypedNative(_) => {
                self.reject_untyped_native(
                    &typed_iterable.ty,
                    &InferType::Dynamic,
                    iterable.span,
                    ConstraintReason::Other("for iteration".to_string()),
                );
                InferType::Poison
            }
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NotIterable {
                        receiver: receiver.clone(),
                    },
                    span: iterable.span,
                    reason: ConstraintReason::Other("for iteration".to_string()),
                });
                InferType::Poison
            }
        };

        self.env.push_scope();
        self.env
            .define_local(iterator.to_string(), elem_type.clone());
        if let Some(name) = collection_binding_name(iterable) {
            self.env.mark_read_only(name);
        }
        let typed_body = self.infer_stmt(body);
        self.env.pop_scope();

        TypedStmtKind::ForEach {
            iterator: iterator.to_string(),
            iterable: typed_iterable,
            elem_type,
            read_only,
            body: Box::new(typed_body),
        }
    }
}

fn collection_binding_name(expr: &aelys_syntax::Expr) -> Option<&str> {
    match &expr.kind {
        aelys_syntax::ExprKind::Identifier(name) => Some(name.as_str()),
        aelys_syntax::ExprKind::Grouping(inner) => collection_binding_name(inner),
        aelys_syntax::ExprKind::Index { object, .. } => collection_binding_name(object),
        _ => None,
    }
}
