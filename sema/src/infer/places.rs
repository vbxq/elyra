use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, ReferenceKind, Span, Stmt, StmtKind};

pub(crate) struct AliasResidual {
    ty: InferType,
    span: Span,
    place: String,
    root: String,
    parameter: Option<String>,
}

pub(crate) fn place_root(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.as_str()),
        ExprKind::Grouping(inner) => place_root(inner),
        ExprKind::Member {
            object,
            separator: aelys_syntax::MemberSeparator::Dot,
            ..
        }
        | ExprKind::Index { object, .. } => place_root(object),
        _ => None,
    }
}

pub(crate) fn render_place(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Identifier(name) => name.clone(),
        ExprKind::Grouping(inner) => render_place(inner),
        ExprKind::Member { object, member, .. } => format!("{}.{member}", render_place(object)),
        ExprKind::Index { object, index } => {
            let index = match &index.kind {
                ExprKind::Identifier(name) => name.clone(),
                ExprKind::Int(value) => value.to_string(),
                _ => "..".to_string(),
            };
            format!("{}[{index}]", render_place(object))
        }
        ExprKind::Call { callee, .. } => format!("{}(..)", render_place(callee)),
        _ => "value".to_string(),
    }
}

fn tail_expr(stmts: &[Stmt]) -> Option<&Expr> {
    match stmts.last().map(|stmt| &stmt.kind) {
        Some(StmtKind::Expression(expr)) => Some(expr),
        _ => None,
    }
}

fn source_places<'a>(expr: &'a Expr, places: &mut Vec<&'a Expr>) {
    match &expr.kind {
        ExprKind::Grouping(inner) => source_places(inner, places),
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            source_places(then_branch, places);
            source_places(else_branch, places);
        }
        ExprKind::Match { arms, .. } => {
            for arm in arms {
                match &arm.body {
                    aelys_syntax::MatchArmBody::Expr(body) => source_places(body, places),
                    aelys_syntax::MatchArmBody::Block(stmts) => {
                        if let Some(tail) = tail_expr(stmts) {
                            source_places(tail, places);
                        }
                    }
                }
            }
        }
        _ if place_root(expr).is_some() => places.push(expr),
        _ => {}
    }
}

impl TypeInference {
    pub(crate) fn place_root_is_mutable(&self, root: &str) -> bool {
        match self.env.borrow_kind(root) {
            Some(ReferenceKind::Mutable) => true,
            Some(_) => false,
            None => self.env.is_mutable(root),
        }
    }

    pub(crate) fn require_mutable_place(
        &mut self,
        object: &Expr,
        receiver: &InferType,
        method: &str,
        through_any_loan: bool,
    ) {
        let root = place_root(object);
        if let Some(root) = root
            && self.env.is_read_only(root)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ReadOnlyCollectionRequired {
                    method: method.to_string(),
                    receiver: receiver.clone(),
                },
                span: object.span,
                reason: ConstraintReason::Other("read-only collection receiver".to_string()),
            });
            return;
        }
        let mutable = root.is_some_and(|root| {
            (through_any_loan && self.env.borrow_kind(root).is_some())
                || self.place_root_is_mutable(root)
        });
        if !mutable {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionRequired {
                    method: method.to_string(),
                    place: render_place(object),
                    root: root.map(str::to_string),
                },
                span: object.span,
                reason: ConstraintReason::Other("mutable collection receiver".to_string()),
            });
        }
    }

    pub(crate) fn record_mutable_alias(
        &mut self,
        source: &Expr,
        ty: &InferType,
        parameter: Option<&str>,
    ) {
        let mut places = Vec::new();
        source_places(source, &mut places);
        for place in places {
            let Some(root) = place_root(place) else {
                continue;
            };
            if self.place_root_is_mutable(root) || self.env.is_read_only(root) {
                continue;
            }
            if !self.env.contains(root) && self.env.lookup(root).is_none() {
                continue;
            }
            self.alias_residuals.push(AliasResidual {
                ty: ty.clone(),
                span: place.span,
                place: render_place(place),
                root: root.to_string(),
                parameter: parameter.map(str::to_string),
            });
        }
    }

    pub(crate) fn validate_alias_residuals(&mut self, subst: &crate::unify::Substitution) {
        let residuals = std::mem::take(&mut self.alias_residuals);
        for residual in residuals {
            let ty = subst.apply(&residual.ty);
            if !self.is_mutable_aggregate(&ty) {
                continue;
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionAlias {
                    place: residual.place,
                    root: residual.root,
                    parameter: residual.parameter,
                },
                span: residual.span,
                reason: ConstraintReason::Other("mutable alias".to_string()),
            });
        }
    }

    fn is_mutable_aggregate(&self, ty: &InferType) -> bool {
        match ty {
            InferType::Array(_) | InferType::FixedArray(_, _) | InferType::Vec(_) => true,
            InferType::Struct(name) | InferType::Applied { name, .. } => {
                self.type_table.get_struct(name).is_some()
            }
            _ => false,
        }
    }
}
