use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, ReferenceKind, Span, TypeAnnotation};

#[derive(Debug, Clone)]
pub(crate) struct ActiveLoan {
    pub(crate) root: String,
    pub(crate) kind: ReferenceKind,
}

impl TypeInference {
    pub(super) fn with_borrow_call_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.borrow_call_scopes.push(Vec::new());
        let result = f(self);
        self.borrow_call_scopes.pop();
        result
    }

    pub(super) fn infer_call_argument(
        &mut self,
        arg: &Expr,
        expected: Option<ReferenceKind>,
    ) -> TypedExpr {
        let saved = self.allow_direct_borrow;
        self.allow_direct_borrow = matches!(arg.kind, ExprKind::Borrow { .. });
        let typed = self.infer_expr(arg);
        self.allow_direct_borrow = saved;

        if matches!(arg.kind, ExprKind::Borrow { .. }) {
            self.validate_explicit_borrow_argument(arg, expected);
        } else {
            self.check_read_access(arg);
            self.validate_forwarded_argument(arg, expected);
        }
        typed
    }

    pub(super) fn infer_borrow_expr(
        &mut self,
        mutable: bool,
        operand: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let saved = self.allow_direct_borrow;
        self.allow_direct_borrow = false;
        let typed_operand = self.infer_expr(operand);
        self.allow_direct_borrow = saved;
        let ty = typed_operand.ty.clone();

        if !saved {
            self.push_borrow_error(
                TypeErrorKind::BorrowEscapes {
                    place: place_label(operand),
                    context: "not a call argument or receiver".to_string(),
                },
                span,
            );
            return (typed_operand.kind, ty);
        }

        if !is_place(operand) {
            self.push_borrow_error(
                TypeErrorKind::TemporaryBorrow {
                    place: place_label(operand),
                },
                span,
            );
            return (typed_operand.kind, ty);
        }

        let Some(root) = root_binding_name(operand) else {
            self.push_borrow_error(
                TypeErrorKind::TemporaryBorrow {
                    place: place_label(operand),
                },
                span,
            );
            return (typed_operand.kind, ty);
        };

        let kind = if mutable {
            ReferenceKind::Mutable
        } else {
            ReferenceKind::Shared
        };
        if mutable && !self.env.is_mutable(&root) {
            self.push_borrow_error(
                TypeErrorKind::MutableLoanOverlap {
                    place: root.clone(),
                },
                span,
            );
        }
        self.check_new_loan(&root, kind, span);

        (typed_operand.kind, ty)
    }

    pub(super) fn register_receiver_borrow(
        &mut self,
        object: &Expr,
        kind: ReferenceKind,
        span: Span,
    ) {
        if !is_place(object) {
            self.push_borrow_error(
                TypeErrorKind::TemporaryBorrow {
                    place: place_label(object),
                },
                span,
            );
            return;
        }
        let Some(root) = root_binding_name(object) else {
            self.push_borrow_error(
                TypeErrorKind::TemporaryBorrow {
                    place: place_label(object),
                },
                span,
            );
            return;
        };
        if kind == ReferenceKind::Mutable && !self.env.is_mutable(&root) {
            self.push_borrow_error(
                TypeErrorKind::MutableLoanOverlap {
                    place: root.clone(),
                },
                span,
            );
        }
        self.check_new_loan(&root, kind, span);
    }

    pub(super) fn reference_modes_for_call(
        &self,
        callee: &Expr,
        typed_callee: &TypedExpr,
    ) -> Vec<Option<ReferenceKind>> {
        if let TypedExprKind::StructMethod { symbol, .. } = &typed_callee.kind
            && let Some(modes) = self.function_reference_modes.get(symbol)
        {
            return modes.clone();
        }
        source_call_path(callee)
            .and_then(|name| self.function_reference_modes.get(&name).cloned())
            .unwrap_or_default()
    }

    pub(super) fn validate_reference_annotation(
        &mut self,
        annotation: &TypeAnnotation,
        allow_top_level: bool,
    ) {
        if annotation.reference.is_some() && !allow_top_level {
            self.push_borrow_error(
                TypeErrorKind::BorrowEscapes {
                    place: annotation.name.clone(),
                    context: "a type annotation".to_string(),
                },
                annotation.span,
            );
        }
        if let Some(params) = &annotation.fn_params {
            for param in params {
                self.validate_reference_annotation(param, false);
            }
        }
        if let Some(ret) = &annotation.fn_ret {
            self.validate_reference_annotation(ret, false);
        }
        for param in &annotation.type_params {
            self.validate_reference_annotation(param, false);
        }
    }

    pub(super) fn type_from_parameter_annotation(
        &mut self,
        annotation: &TypeAnnotation,
    ) -> InferType {
        let saved = self.allow_reference_annotation;
        self.allow_reference_annotation = true;
        let ty = self.type_from_annotation_as(crate::infer::OccurrenceRole::Parameter, annotation);
        self.allow_reference_annotation = saved;
        ty
    }

    pub(super) fn check_write_access(&mut self, expr: &Expr, span: Span, context: &str) {
        let Some(root) = root_binding_name(expr) else {
            return;
        };
        self.check_write_root(&root, span, context);
    }

    pub(super) fn check_write_binding(&mut self, name: &str, span: Span, context: &str) {
        self.check_write_root(name, span, context);
    }

    fn check_write_root(&mut self, root: &str, span: Span, context: &str) {
        if self.has_active_mutable_loan(root) {
            self.push_borrow_error(
                TypeErrorKind::MutableLoanAccess {
                    place: root.to_string(),
                    access: "write".to_string(),
                },
                span,
            );
            return;
        }
        if self.forwarded_mutable_borrows.contains(root) {
            self.push_borrow_error(
                TypeErrorKind::BorrowInvalidated {
                    place: root.to_string(),
                    context: context.to_string(),
                },
                span,
            );
            return;
        }
        if self.env.borrow_kind(root) == Some(ReferenceKind::Shared) {
            self.push_borrow_error(
                TypeErrorKind::SharedLoanMutation {
                    place: root.to_string(),
                },
                span,
            );
        }
    }

    fn validate_explicit_borrow_argument(&mut self, arg: &Expr, expected: Option<ReferenceKind>) {
        let ExprKind::Borrow { mutable, operand } = &arg.kind else {
            return;
        };
        let Some(expected) = expected else {
            self.push_borrow_error(
                TypeErrorKind::BorrowEscapes {
                    place: place_label(operand),
                    context: "a by-value or native parameter".to_string(),
                },
                arg.span,
            );
            return;
        };
        if expected == ReferenceKind::Mutable && !*mutable {
            self.push_borrow_error(
                TypeErrorKind::MutableLoanOverlap {
                    place: place_label(operand),
                },
                arg.span,
            );
        }
    }

    fn validate_forwarded_argument(&mut self, arg: &Expr, expected: Option<ReferenceKind>) {
        let Some(root) = root_binding_name(arg) else {
            if expected.is_some() {
                self.push_borrow_error(
                    TypeErrorKind::BorrowEscapes {
                        place: place_label(arg),
                        context: "a reference parameter requires a place borrow".to_string(),
                    },
                    arg.span,
                );
            }
            return;
        };
        let Some(existing) = self.env.borrow_kind(&root) else {
            if expected.is_some() {
                self.push_borrow_error(
                    TypeErrorKind::BorrowEscapes {
                        place: root,
                        context: "a reference parameter requires '&'".to_string(),
                    },
                    arg.span,
                );
            }
            return;
        };
        let Some(expected) = expected else {
            self.push_borrow_error(
                TypeErrorKind::BorrowEscapes {
                    place: root,
                    context: "a by-value or native parameter".to_string(),
                },
                arg.span,
            );
            return;
        };
        match (existing, expected) {
            (ReferenceKind::Shared, ReferenceKind::Mutable) => {
                self.push_borrow_error(TypeErrorKind::MutableLoanOverlap { place: root }, arg.span)
            }
            (ReferenceKind::Mutable, ReferenceKind::Shared) => {
                self.forwarded_mutable_borrows.insert(root.clone());
            }
            _ => {}
        }
    }

    fn check_new_loan(&mut self, root: &str, kind: ReferenceKind, span: Span) {
        if let Some(existing) = self.env.borrow_kind(root) {
            self.report_loan_conflict(root, kind, existing, span);
        }
        let existing: Vec<_> = self
            .borrow_call_scopes
            .iter()
            .flat_map(|scope| scope.iter())
            .filter(|loan| loan.root == root)
            .map(|loan| loan.kind)
            .collect();
        for prior in existing {
            self.report_loan_conflict(root, kind, prior, span);
        }
        if let Some(scope) = self.borrow_call_scopes.last_mut() {
            scope.push(ActiveLoan {
                root: root.to_string(),
                kind,
            });
        }
    }

    fn report_loan_conflict(
        &mut self,
        root: &str,
        requested: ReferenceKind,
        existing: ReferenceKind,
        span: Span,
    ) {
        let kind = match (requested, existing) {
            (ReferenceKind::Shared, ReferenceKind::Mutable) => TypeErrorKind::MutableLoanAccess {
                place: root.to_string(),
                access: "read".to_string(),
            },
            (ReferenceKind::Mutable, _) => TypeErrorKind::MutableLoanOverlap {
                place: root.to_string(),
            },
            _ => return,
        };
        self.push_borrow_error(kind, span);
    }

    fn has_active_mutable_loan(&self, root: &str) -> bool {
        self.borrow_call_scopes.iter().any(|scope| {
            scope
                .iter()
                .any(|loan| loan.root == root && loan.kind == ReferenceKind::Mutable)
        })
    }

    fn check_read_access(&mut self, expr: &Expr) {
        let Some(root) = root_binding_name(expr) else {
            return;
        };
        if self.has_active_mutable_loan(&root) {
            self.push_borrow_error(
                TypeErrorKind::MutableLoanAccess {
                    place: root,
                    access: "read".to_string(),
                },
                expr.span,
            );
        }
    }

    fn push_borrow_error(&mut self, kind: TypeErrorKind, span: Span) {
        self.errors.push(TypeError {
            kind,
            span,
            reason: ConstraintReason::Other("call-scoped borrow checking".to_string()),
        });
    }
}

fn source_call_path(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.clone()),
        ExprKind::GenericApply { callee, .. } => source_call_path(callee),
        ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } => {
            let mut path = source_call_path(object)?;
            path.push_str("::");
            path.push_str(member);
            Some(path)
        }
        _ => None,
    }
}

fn is_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Identifier(_) => true,
        ExprKind::Grouping(inner) => is_place(inner),
        ExprKind::Member { object, .. } => is_place(object),
        ExprKind::Index { object, index } => {
            is_place(object) && !matches!(index.kind, ExprKind::Borrow { .. })
        }
        _ => false,
    }
}

fn root_binding_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.clone()),
        ExprKind::Grouping(inner) => root_binding_name(inner),
        ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
            root_binding_name(object)
        }
        _ => None,
    }
}

fn place_label(expr: &Expr) -> String {
    root_binding_name(expr).unwrap_or_else(|| "temporary".to_string())
}
