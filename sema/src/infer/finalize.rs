use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedFunction, TypedStmt, TypedStmtKind};
use crate::types::InferType;
use std::collections::HashSet;

impl TypeInference {
    pub(super) fn validate_surface_type_boundaries(&mut self, stmts: &[TypedStmt]) {
        for stmt in stmts {
            let mut declared = HashSet::new();
            collect_declared_type_params(stmt, &mut declared);
            let mut types = Vec::new();
            super::monomorphize::collect_stmt_types(stmt, &mut types);
            let forbidden = types.iter().find_map(|(ty, span)| {
                forbidden_surface_type(ty, &declared).map(|kind| (kind, *span))
            });
            let Some((kind, span)) = forbidden else {
                continue;
            };
            if self.has_reported_surface_annotation(stmt.span) {
                continue;
            }
            let kind = if matches!(kind, TypeErrorKind::PoisonedType) && self.errors.is_empty() {
                TypeErrorKind::UndeterminedType
            } else {
                kind
            };
            self.errors.push(TypeError {
                kind,
                span,
                reason: ConstraintReason::Other("final typed surface audit".to_string()),
            });
        }
        if let Some(InferType::Applied { name, .. }) = self
            .type_table
            .unmaterialized_applied_types()
            .into_iter()
            .next()
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnmaterializedAppliedType { name },
                span: aelys_syntax::Span::dummy(),
                reason: ConstraintReason::Other("final schema audit".to_string()),
            });
        }
    }

    fn has_reported_surface_annotation(&self, span: aelys_syntax::Span) -> bool {
        self.surface_dynamic_spans
            .iter()
            .any(|(start, end)| span.start <= *start && *end <= span.end)
    }

    pub(super) fn finalize_stmts(&self, stmts: Vec<TypedStmt>) -> Vec<TypedStmt> {
        stmts
    }
}

fn forbidden_surface_type(ty: &InferType, declared: &HashSet<String>) -> Option<TypeErrorKind> {
    match ty {
        InferType::Poison => Some(TypeErrorKind::PoisonedType),
        InferType::Dynamic => Some(TypeErrorKind::DynamicIsNotInSurface),
        InferType::Null => Some(TypeErrorKind::NullIsNotInSurface),
        InferType::UntypedNative(name) => {
            Some(TypeErrorKind::UntypedNativeBoundary { name: name.clone() })
        }
        InferType::Var(_) => Some(TypeErrorKind::UnresolvedTypeVariable),
        InferType::Param(name) if !declared.contains(name) => {
            Some(TypeErrorKind::UnresolvedGenericType { name: name.clone() })
        }
        InferType::Applied { name, args } if !args.iter().any(|arg| binds_param(arg, declared)) => {
            Some(TypeErrorKind::UnmaterializedAppliedType { name: name.clone() })
        }
        _ => None,
    }
}

fn binds_param(ty: &InferType, declared: &HashSet<String>) -> bool {
    match ty {
        InferType::Param(name) => declared.contains(name),
        InferType::Function { params, ret } => {
            params.iter().any(|param| binds_param(param, declared)) || binds_param(ret, declared)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => binds_param(inner, declared),
        InferType::Result(ok, err) => binds_param(ok, declared) || binds_param(err, declared),
        InferType::Tuple(elements) => elements
            .iter()
            .any(|element| binds_param(element, declared)),
        InferType::Applied { args, .. } => args.iter().any(|arg| binds_param(arg, declared)),
        _ => false,
    }
}

fn collect_declared_type_params(stmt: &TypedStmt, declared: &mut HashSet<String>) {
    match &stmt.kind {
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_declared_type_params(stmt, declared);
            }
        }
        TypedStmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_declared_type_params(then_branch, declared);
            if let Some(else_branch) = else_branch {
                collect_declared_type_params(else_branch, declared);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForEach { body, .. } => collect_declared_type_params(body, declared),
        TypedStmtKind::Function(function) => collect_function_type_params(function, declared),
        TypedStmtKind::ImplDecl {
            type_params,
            methods,
            ..
        } => {
            declared.extend(type_params.iter().cloned());
            for method in methods {
                collect_function_type_params(method, declared);
            }
        }
        TypedStmtKind::TraitDecl { type_params, .. }
        | TypedStmtKind::StructDecl { type_params, .. }
        | TypedStmtKind::EnumDecl { type_params, .. } => {
            declared.extend(type_params.iter().cloned());
        }
        TypedStmtKind::Expression(_)
        | TypedStmtKind::Let { .. }
        | TypedStmtKind::Return(_)
        | TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_) => {}
    }
}

fn collect_function_type_params(function: &TypedFunction, declared: &mut HashSet<String>) {
    declared.extend(function.type_params.iter().cloned());
    for stmt in &function.body {
        collect_declared_type_params(stmt, declared);
    }
}
