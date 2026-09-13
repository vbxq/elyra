use aelys_syntax::{
    AssociatedBinding, EnumVariantFields, Expr, ExprKind, MatchArmBody, MemberSeparator, Pattern,
    PatternKind, Span, Stmt, StmtKind, TypeAnnotation, WhereClause,
};
use std::collections::{BTreeMap, HashSet};

pub fn renamed_module_nominal(name: &str, module_path: &str) -> String {
    format!("{name}@{module_path}")
}

fn rename_head(value: &mut String, renames: &BTreeMap<String, String>) {
    if let Some(replacement) = renames.get(value) {
        *value = replacement.clone();
    }
}

fn rename_path(path: &mut [String], renames: &BTreeMap<String, String>) {
    if let Some(first) = path.first_mut() {
        rename_head(first, renames);
    }
}

pub fn rename_annotation(annotation: &mut TypeAnnotation, renames: &BTreeMap<String, String>) {
    rename_head(&mut annotation.name, renames);
    rename_path(&mut annotation.path, renames);
    for param in &mut annotation.type_params {
        rename_annotation(param, renames);
    }
    if let Some(params) = &mut annotation.fn_params {
        for param in params {
            rename_annotation(param, renames);
        }
    }
    if let Some(ret) = &mut annotation.fn_ret {
        rename_annotation(ret.as_mut(), renames);
    }
    for (_, binding, _) in &mut annotation.associated_bindings {
        if let AssociatedBinding::Type(ty) = binding {
            rename_annotation(ty, renames);
        }
    }
}

pub fn collect_annotation_names(annotation: &TypeAnnotation, out: &mut HashSet<String>) {
    out.insert(annotation.name.clone());
    out.extend(annotation.path.iter().cloned());
    for param in &annotation.type_params {
        collect_annotation_names(param, out);
    }
    if let Some(params) = &annotation.fn_params {
        for param in params {
            collect_annotation_names(param, out);
        }
    }
    if let Some(ret) = &annotation.fn_ret {
        collect_annotation_names(ret, out);
    }
    for (_, binding, _) in &annotation.associated_bindings {
        if let AssociatedBinding::Type(ty) = binding {
            collect_annotation_names(ty, out);
        }
    }
}

fn rename_where_clauses(clauses: &mut [WhereClause], renames: &BTreeMap<String, String>) {
    for clause in clauses {
        rename_annotation(&mut clause.type_annotation, renames);
        for bound in &mut clause.bounds {
            rename_annotation(bound, renames);
        }
    }
}

fn where_clause_name_span(clauses: &[WhereClause], name: &str) -> Option<Span> {
    clauses.iter().find_map(|clause| {
        annotation_name_span(&clause.type_annotation, name).or_else(|| {
            clause
                .bounds
                .iter()
                .find_map(|bound| annotation_name_span(bound, name))
        })
    })
}

fn collect_where_names(clauses: &[WhereClause], out: &mut HashSet<String>) {
    for clause in clauses {
        collect_annotation_names(&clause.type_annotation, out);
        for bound in &clause.bounds {
            collect_annotation_names(bound, out);
        }
    }
}

fn rename_pattern(pattern: &mut Pattern, renames: &BTreeMap<String, String>) {
    match &mut pattern.kind {
        PatternKind::Variant {
            path,
            type_args,
            fields,
        } => {
            rename_path(path, renames);
            for arg in type_args {
                rename_annotation(arg, renames);
            }
            for field in fields {
                rename_pattern(field, renames);
            }
        }
        PatternKind::Struct {
            path,
            type_args,
            fields,
            ..
        } => {
            rename_path(path, renames);
            for arg in type_args {
                rename_annotation(arg, renames);
            }
            for field in fields {
                rename_pattern(&mut field.pattern, renames);
            }
        }
        PatternKind::Or(alternatives) => {
            for alternative in alternatives {
                rename_pattern(alternative, renames);
            }
        }
        PatternKind::Wildcard
        | PatternKind::Binding(_)
        | PatternKind::Int(_)
        | PatternKind::String(_)
        | PatternKind::Bool(_) => {}
    }
}

fn rename_expr(expr: &mut Expr, renames: &BTreeMap<String, String>) {
    if let Some(repeat) = &mut expr.repeat {
        rename_expr(repeat.as_mut(), renames);
    }
    match &mut expr.kind {
        ExprKind::Call { callee, args } => {
            rename_expr(callee.as_mut(), renames);
            for arg in args {
                rename_expr(arg, renames);
            }
        }
        ExprKind::GenericApply { callee, type_args } => {
            rename_expr(callee.as_mut(), renames);
            for arg in type_args {
                rename_annotation(arg, renames);
            }
        }
        ExprKind::Member { object, .. } => {
            rename_member_root(object, renames);
            rename_expr(object.as_mut(), renames);
        }
        ExprKind::StructLiteral {
            name,
            type_args,
            fields,
        } => {
            rename_head(name, renames);
            for arg in type_args {
                rename_annotation(arg, renames);
            }
            for field in fields {
                rename_expr(field.value.as_mut(), renames);
            }
        }
        ExprKind::EnumLiteral { path, fields } => {
            rename_path(path, renames);
            for field in fields {
                rename_expr(field.value.as_mut(), renames);
            }
        }
        ExprKind::GenericEnumLiteral {
            path,
            type_args,
            fields,
        } => {
            rename_path(path, renames);
            for arg in type_args {
                rename_annotation(arg, renames);
            }
            for field in fields {
                rename_expr(field.value.as_mut(), renames);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            rename_expr(scrutinee.as_mut(), renames);
            for arm in arms {
                rename_pattern(&mut arm.pattern, renames);
                if let Some(guard) = &mut arm.guard {
                    rename_expr(guard, renames);
                }
                match &mut arm.body {
                    MatchArmBody::Expr(body) => rename_expr(body.as_mut(), renames),
                    MatchArmBody::Block(stmts) => rename_stmts(stmts, renames),
                }
            }
        }
        ExprKind::Lambda {
            params,
            return_type,
            body,
        } => {
            for param in params {
                if let Some(annotation) = &mut param.type_annotation {
                    rename_annotation(annotation, renames);
                }
            }
            if let Some(ret) = return_type {
                rename_annotation(ret, renames);
            }
            rename_stmts(body, renames);
        }
        ExprKind::ArrayLiteral {
            element_type,
            elements,
        } => {
            if let Some(ty) = element_type {
                rename_annotation(ty, renames);
            }
            for element in elements {
                rename_expr(element, renames);
            }
        }
        ExprKind::ArraySized {
            element_type, size, ..
        } => {
            if let Some(ty) = element_type {
                rename_annotation(ty, renames);
            }
            rename_expr(size.as_mut(), renames);
        }
        ExprKind::VecLiteral {
            element_type,
            elements,
        } => {
            if let Some(ty) = element_type {
                rename_annotation(ty, renames);
            }
            for element in elements {
                rename_expr(element, renames);
            }
        }
        ExprKind::Cast {
            expr: inner,
            target,
        } => {
            rename_expr(inner.as_mut(), renames);
            rename_annotation(target, renames);
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::And { left, right }
        | ExprKind::Or { left, right } => {
            rename_expr(left.as_mut(), renames);
            rename_expr(right.as_mut(), renames);
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Borrow { operand, .. }
        | ExprKind::Grouping(operand)
        | ExprKind::Try(operand) => {
            rename_expr(operand.as_mut(), renames);
        }
        ExprKind::Assign { value, .. } => {
            rename_expr(value.as_mut(), renames);
        }
        ExprKind::MemberAssign { object, value, .. } => {
            rename_expr(object.as_mut(), renames);
            rename_expr(value.as_mut(), renames);
        }
        ExprKind::Index { object, index } => {
            rename_expr(object.as_mut(), renames);
            rename_expr(index.as_mut(), renames);
        }
        ExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            rename_expr(object.as_mut(), renames);
            rename_expr(index.as_mut(), renames);
            rename_expr(value.as_mut(), renames);
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                rename_expr(start.as_mut(), renames);
            }
            if let Some(end) = end {
                rename_expr(end.as_mut(), renames);
            }
        }
        ExprKind::Slice { object, range } => {
            rename_expr(object.as_mut(), renames);
            rename_expr(range.as_mut(), renames);
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rename_expr(condition.as_mut(), renames);
            rename_expr(then_branch.as_mut(), renames);
            rename_expr(else_branch.as_mut(), renames);
        }
        ExprKind::FmtString(parts) => {
            for part in parts {
                if let aelys_syntax::FmtStringPart::Expr(inner) = part {
                    rename_expr(inner.as_mut(), renames);
                }
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Identifier(_) => {}
    }
}

fn rename_member_root(object: &mut Expr, renames: &BTreeMap<String, String>) {
    let mut current = object;
    loop {
        match &mut current.kind {
            ExprKind::Member {
                object: inner,
                separator: MemberSeparator::Path,
                ..
            } => current = inner.as_mut(),
            ExprKind::GenericApply { callee, .. } => current = callee.as_mut(),
            ExprKind::Identifier(name) => {
                rename_head(name, renames);
                return;
            }
            _ => return,
        }
    }
}

pub fn rename_stmts(stmts: &mut [Stmt], renames: &BTreeMap<String, String>) {
    if renames.is_empty() {
        return;
    }
    for stmt in stmts {
        rename_stmt(stmt, renames);
    }
}

fn rename_stmt(stmt: &mut Stmt, renames: &BTreeMap<String, String>) {
    match &mut stmt.kind {
        StmtKind::Expression(expr) => rename_expr(expr, renames),
        StmtKind::Let {
            type_annotation,
            initializer,
            ..
        } => {
            if let Some(annotation) = type_annotation {
                rename_annotation(annotation, renames);
            }
            rename_expr(initializer, renames);
        }
        StmtKind::Block(stmts) => rename_stmts(stmts, renames),
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rename_expr(condition, renames);
            rename_stmt(then_branch.as_mut(), renames);
            if let Some(other) = else_branch {
                rename_stmt(other.as_mut(), renames);
            }
        }
        StmtKind::While { condition, body } => {
            rename_expr(condition, renames);
            rename_stmt(body.as_mut(), renames);
        }
        StmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            rename_expr(start, renames);
            rename_expr(end, renames);
            if let Some(step) = step.as_mut().as_mut() {
                rename_expr(step, renames);
            }
            rename_stmt(body.as_mut(), renames);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            rename_expr(iterable, renames);
            rename_stmt(body.as_mut(), renames);
        }
        StmtKind::Return(value) => {
            if let Some(value) = value {
                rename_expr(value, renames);
            }
        }
        StmtKind::Function(func) => {
            rename_where_clauses(&mut func.where_clauses, renames);
            for param in &mut func.params {
                if let Some(annotation) = &mut param.type_annotation {
                    rename_annotation(annotation, renames);
                }
            }
            if let Some(ret) = &mut func.return_type {
                rename_annotation(ret, renames);
            }
            rename_stmts(&mut func.body, renames);
        }
        StmtKind::ImplDecl {
            polarity: _,
            type_params: _,
            trait_path,
            self_type,
            where_clauses,
            methods,
            associated_types,
            associated_consts,
        } => {
            if let Some(path) = trait_path {
                rename_annotation(path, renames);
            }
            rename_annotation(self_type, renames);
            rename_where_clauses(where_clauses, renames);
            for def in associated_types.iter_mut() {
                rename_annotation(&mut def.value, renames);
            }
            for def in associated_consts.iter_mut() {
                rename_annotation(&mut def.type_annotation, renames);
                rename_expr(&mut def.value, renames);
            }
            for method in methods {
                rename_where_clauses(&mut method.where_clauses, renames);
                for param in &mut method.params {
                    if let Some(annotation) = &mut param.type_annotation {
                        rename_annotation(annotation, renames);
                    }
                }
                if let Some(ret) = &mut method.return_type {
                    rename_annotation(ret, renames);
                }
                rename_stmts(&mut method.body, renames);
            }
        }
        StmtKind::StructDecl { name, fields, .. } => {
            rename_head(name, renames);
            for field in fields {
                rename_annotation(&mut field.type_annotation, renames);
            }
        }
        StmtKind::EnumDecl { name, variants, .. } => {
            rename_head(name, renames);
            for variant in variants {
                match &mut variant.fields {
                    EnumVariantFields::Unit => {}
                    EnumVariantFields::Tuple(types) => {
                        for ty in types {
                            rename_annotation(ty, renames);
                        }
                    }
                    EnumVariantFields::Named(fields) => {
                        for field in fields {
                            rename_annotation(&mut field.type_annotation, renames);
                        }
                    }
                }
            }
        }
        StmtKind::TraitDecl {
            name,
            super_bounds,
            where_clauses,
            methods,
            associated_consts,
            ..
        } => {
            rename_head(name, renames);
            for bound in super_bounds {
                rename_annotation(bound, renames);
            }
            rename_where_clauses(where_clauses, renames);
            for method in methods {
                rename_where_clauses(&mut method.function.where_clauses, renames);
                for param in &mut method.function.params {
                    if let Some(annotation) = &mut param.type_annotation {
                        rename_annotation(annotation, renames);
                    }
                }
                if let Some(ret) = &mut method.function.return_type {
                    rename_annotation(ret, renames);
                }
                rename_stmts(&mut method.function.body, renames);
            }
            for def in associated_consts {
                rename_annotation(&mut def.type_annotation, renames);
            }
        }
        StmtKind::Break | StmtKind::Continue | StmtKind::Needs(_) => {}
    }
}

pub fn collect_infer_names(ty: &aelys_sema::InferType, out: &mut HashSet<String>) {
    use aelys_sema::InferType;
    match ty {
        InferType::Struct(name) | InferType::Applied { name, .. } => {
            out.insert(name.clone());
        }
        _ => {}
    }
    match ty {
        InferType::Applied { args, .. } => {
            for arg in args {
                collect_infer_names(arg, out);
            }
        }
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => collect_infer_names(inner, out),
        InferType::Result(ok, err) => {
            collect_infer_names(ok, out);
            collect_infer_names(err, out);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_infer_names(element, out);
            }
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_infer_names(param, out);
            }
            collect_infer_names(ret, out);
        }
        InferType::Projection { self_ty, .. } => collect_infer_names(self_ty, out),
        _ => {}
    }
}

/// where an impl's contract writes `name`, so a refusal underlines the leak and not the
pub fn impl_contract_name_span(stmt: &Stmt, name: &str) -> Option<Span> {
    let StmtKind::ImplDecl {
        trait_path,
        self_type,
        where_clauses,
        methods,
        associated_types,
        associated_consts,
        ..
    } = &stmt.kind
    else {
        return None;
    };
    for def in associated_types {
        if let Some(span) = annotation_name_span(&def.value, name) {
            return Some(span);
        }
    }
    for def in associated_consts {
        if let Some(span) = annotation_name_span(&def.type_annotation, name) {
            return Some(span);
        }
    }
    for method in methods {
        for param in &method.params {
            if let Some(annotation) = &param.type_annotation
                && let Some(span) = annotation_name_span(annotation, name)
            {
                return Some(span);
            }
        }
        if let Some(ret) = &method.return_type
            && let Some(span) = annotation_name_span(ret, name)
        {
            return Some(span);
        }
        if let Some(span) = where_clause_name_span(&method.where_clauses, name) {
            return Some(span);
        }
    }
    if let Some(span) = where_clause_name_span(where_clauses, name) {
        return Some(span);
    }
    if let Some(path) = trait_path
        && let Some(span) = annotation_name_span(path, name)
    {
        return Some(span);
    }
    annotation_name_span(self_type, name)
}

fn annotation_name_span(annotation: &TypeAnnotation, name: &str) -> Option<Span> {
    for param in &annotation.type_params {
        if let Some(span) = annotation_name_span(param, name) {
            return Some(span);
        }
    }
    if let Some(params) = &annotation.fn_params {
        for param in params {
            if let Some(span) = annotation_name_span(param, name) {
                return Some(span);
            }
        }
    }
    if let Some(ret) = &annotation.fn_ret
        && let Some(span) = annotation_name_span(ret, name)
    {
        return Some(span);
    }
    for (_, binding, _) in &annotation.associated_bindings {
        if let AssociatedBinding::Type(ty) = binding
            && let Some(span) = annotation_name_span(ty, name)
        {
            return Some(span);
        }
    }
    (annotation.name == name || annotation.path.iter().any(|segment| segment == name))
        .then_some(annotation.span)
}

pub fn collect_impl_header_names(stmt: &Stmt, out: &mut HashSet<String>) {
    collect_impl_header_names_inner(stmt, out, false);
}

pub fn collect_impl_leak_names(stmt: &Stmt, out: &mut HashSet<String>) {
    collect_impl_header_names_inner(stmt, out, true);
}

fn collect_impl_header_names_inner(stmt: &Stmt, out: &mut HashSet<String>, skip_target: bool) {
    let StmtKind::ImplDecl {
        trait_path,
        self_type,
        where_clauses,
        methods,
        associated_types,
        associated_consts,
        ..
    } = &stmt.kind
    else {
        return;
    };
    if let Some(path) = trait_path {
        collect_annotation_names(path, out);
    }
    if !skip_target {
        collect_annotation_names(self_type, out);
    }
    collect_where_names(where_clauses, out);
    for def in associated_types {
        collect_annotation_names(&def.value, out);
    }
    for def in associated_consts {
        collect_annotation_names(&def.type_annotation, out);
    }
    for method in methods {
        collect_where_names(&method.where_clauses, out);
        for param in &method.params {
            if let Some(annotation) = &param.type_annotation {
                collect_annotation_names(annotation, out);
            }
        }
        if let Some(ret) = &method.return_type {
            collect_annotation_names(ret, out);
        }
    }
}
