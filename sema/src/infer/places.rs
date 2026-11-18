use super::TypeInference;
use crate::constraint::{AliasSite, ConstraintReason, TypeError, TypeErrorKind};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, ReferenceKind, Span, Stmt, StmtKind};

pub(crate) struct AliasResidual {
    ty: InferType,
    span: Span,
    place: String,
    root: String,
    site: AliasSite,
    through: Option<(InferType, usize)>,
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

fn is_path_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Grouping(inner) => is_path_place(inner),
        ExprKind::Member {
            object,
            separator: aelys_syntax::MemberSeparator::Dot,
            ..
        }
        | ExprKind::Index { object, .. } => is_path_place(object),
        ExprKind::Member {
            object,
            separator: aelys_syntax::MemberSeparator::Path,
            ..
        } => matches!(object.kind, ExprKind::Identifier(_)),
        _ => false,
    }
}

pub(crate) fn render_place(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Identifier(name) => name.clone(),
        ExprKind::Grouping(inner) => render_place(inner),
        ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } => format!("{}::{member}", render_place(object)),
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

fn tail_places<'a>(stmts: &'a [Stmt], places: &mut Vec<&'a Expr>) {
    if let Some(stmt) = stmts.last() {
        stmt_tail_places(stmt, places);
    }
}

fn stmt_tail_places<'a>(stmt: &'a Stmt, places: &mut Vec<&'a Expr>) {
    match &stmt.kind {
        StmtKind::Expression(expr) => source_places(expr, places),
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => {
            stmt_tail_places(then_branch, places);
            stmt_tail_places(else_branch, places);
        }
        StmtKind::Block(stmts) => tail_places(stmts, places),
        _ => {}
    }
}

// depth counts the loops between a break and the loop whose value it gives
fn break_places<'a>(stmt: &'a Stmt, label: Option<&str>, depth: usize, places: &mut Vec<&'a Expr>) {
    match &stmt.kind {
        StmtKind::Break {
            label: target,
            value: Some(value),
        } if match target {
            Some(target) => Some(target.as_str()) == label,
            None => depth == 0,
        } =>
        {
            source_places(value, places)
        }
        StmtKind::Expression(expr) => expr_break_places(expr, label, depth, places),
        StmtKind::Block(stmts) => {
            for stmt in stmts {
                break_places(stmt, label, depth, places);
            }
        }
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            break_places(then_branch, label, depth, places);
            if let Some(else_branch) = else_branch {
                break_places(else_branch, label, depth, places);
            }
        }
        StmtKind::While { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForEach { body, .. } => break_places(body, label, depth + 1, places),
        _ => {}
    }
}

fn expr_break_places<'a>(
    expr: &'a Expr,
    label: Option<&str>,
    depth: usize,
    places: &mut Vec<&'a Expr>,
) {
    match &expr.kind {
        ExprKind::Block(stmts) => {
            for stmt in stmts {
                break_places(stmt, label, depth, places);
            }
        }
        ExprKind::Loop { body, .. } => {
            for stmt in body {
                break_places(stmt, label, depth + 1, places);
            }
        }
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            expr_break_places(then_branch, label, depth, places);
            expr_break_places(else_branch, label, depth, places);
        }
        ExprKind::Match { arms, .. } => {
            for arm in arms {
                match &arm.body {
                    aelys_syntax::MatchArmBody::Expr(body) => {
                        expr_break_places(body, label, depth, places)
                    }
                    aelys_syntax::MatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            break_places(stmt, label, depth, places);
                        }
                    }
                }
            }
        }
        ExprKind::Grouping(inner) => expr_break_places(inner, label, depth, places),
        _ => {}
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
                    aelys_syntax::MatchArmBody::Block(stmts) => tail_places(stmts, places),
                }
            }
        }
        ExprKind::Block(stmts) => tail_places(stmts, places),
        ExprKind::Loop { body, label } => {
            for stmt in body {
                break_places(stmt, label.as_deref(), 0, places);
            }
        }
        _ if place_root(expr).is_some() || is_path_place(expr) => places.push(expr),
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

    pub(crate) fn record_mutable_alias(&mut self, source: &Expr, ty: &InferType, site: AliasSite) {
        self.record_alias(source, ty, site, None);
    }

    // the callee type is read once solved: only a position it marks `mut` aliases the argument
    pub(crate) fn record_function_value_alias(
        &mut self,
        source: &Expr,
        ty: &InferType,
        callee: &InferType,
        position: usize,
    ) {
        let site = AliasSite::FunctionValue {
            position: position + 1,
            ty: String::new(),
        };
        self.record_alias(source, ty, site, Some((callee.clone(), position)));
    }

    fn record_alias(
        &mut self,
        source: &Expr,
        ty: &InferType,
        site: AliasSite,
        through: Option<(InferType, usize)>,
    ) {
        let mut places = Vec::new();
        source_places(source, &mut places);
        for place in places {
            let Some(written) = place_root(place) else {
                if let Some(root) = self.imported_immutable_root(place) {
                    self.alias_residuals.push(AliasResidual {
                        ty: ty.clone(),
                        span: place.span,
                        place: render_place(place),
                        root,
                        site: site.clone(),
                        through: through.clone(),
                    });
                }
                continue;
            };
            let (root, root_mutable) = self.alias_root(written);
            if root_mutable || (root == written && self.env.is_read_only(written)) {
                continue;
            }
            let root = root.as_str();
            if !self.env.contains(root) && self.env.lookup(root).is_none() {
                continue;
            }
            self.alias_residuals.push(AliasResidual {
                ty: ty.clone(),
                span: place.span,
                place: render_place(place),
                root: root.to_string(),
                site: site.clone(),
                through: through.clone(),
            });
        }
    }

    pub(crate) fn alias_root(&self, root: &str) -> (String, bool) {
        match self.env.binding_origin(root) {
            Some(origin) => origin.clone(),
            None => (root.to_string(), self.place_root_is_mutable(root)),
        }
    }

    pub(crate) fn record_binding_origins(&mut self, source: &Expr, names: &[String]) {
        let Some(root) = place_root(source) else {
            return;
        };
        let (root, root_mutable) = self.alias_root(root);
        for name in names {
            self.env
                .define_binding_origin(name, root.clone(), root_mutable);
        }
    }

    pub(crate) fn validate_alias_residuals(&mut self, subst: &crate::unify::Substitution) {
        let residuals = std::mem::take(&mut self.alias_residuals);
        for residual in residuals {
            let ty = subst.apply(&residual.ty);
            if !self.is_mutable_aggregate(&ty) {
                continue;
            }
            let mut site = residual.site;
            if let Some((callee, position)) = &residual.through {
                let callee = subst.apply(callee);
                let InferType::Function { modes, .. } = &callee else {
                    continue;
                };
                if subst.apply_mode(modes.get(*position)) != crate::types::ParamMode::Mut {
                    continue;
                }
                if let AliasSite::FunctionValue { ty, .. } = &mut site {
                    *ty = callee.source_spelling();
                }
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionAlias {
                    place: residual.place,
                    root: residual.root,
                    site,
                },
                span: residual.span,
                reason: ConstraintReason::Other("mutable alias".to_string()),
            });
        }
    }

    fn is_mutable_aggregate(&self, ty: &InferType) -> bool {
        match ty {
            InferType::Array(_)
            | InferType::FixedArray(_, _)
            | InferType::Vec(_)
            | InferType::Map(_, _) => true,
            InferType::Struct(name) | InferType::Applied { name, .. } => {
                self.type_table.get_struct(name).is_some()
            }
            _ => false,
        }
    }
}
