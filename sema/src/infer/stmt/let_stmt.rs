use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExprKind, TypedStmtKind};
use crate::types::InferType;
use aelys_syntax::{Expr, Span, TypeAnnotation};

impl TypeInference {
    pub(super) fn infer_let_stmt(
        &mut self,
        span: Span,
        name: &str,
        mutable: bool,
        type_annotation: &Option<TypeAnnotation>,
        initializer: &Expr,
        is_pub: bool,
    ) -> TypedStmtKind {
        let mut typed_init = self.infer_expr(initializer);
        let read_only_alias = collection_binding_name(initializer)
            .is_some_and(|source| self.env.is_read_only(source));

        let declared_type = type_annotation
            .as_ref()
            .map(|ann| self.type_from_annotation(ann));

        let var_type = if let Some(decl) = &declared_type {
            if let TypedExprKind::Int(value) = &typed_init.kind
                && decl.is_integer()
                && *decl != InferType::I64
            {
                if InferType::int_fits(*value, decl) {
                    typed_init.ty = decl.clone();
                } else {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Mismatch {
                            expected: decl.clone(),
                            found: InferType::I64,
                        },
                        span: typed_init.span,
                        reason: ConstraintReason::IntLiteralOverflow {
                            value: *value,
                            target: decl.clone(),
                        },
                    });
                }
            } else if let TypedExprKind::Float(value) = &typed_init.kind
                && *decl == InferType::F32
            {
                if value.is_finite() && value.abs() <= f32::MAX as f64 {
                    typed_init.ty = InferType::F32;
                } else {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Mismatch {
                            expected: InferType::F32,
                            found: InferType::F64,
                        },
                        span: typed_init.span,
                        reason: ConstraintReason::TypeAnnotation {
                            var_name: name.to_string(),
                        },
                    });
                }
            } else if self.reject_dynamic(
                &typed_init.ty,
                decl,
                typed_init.span,
                ConstraintReason::TypeAnnotation {
                    var_name: name.to_string(),
                },
            ) {
            } else if let InferType::UntypedNative(native) = &typed_init.ty {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UntypedNativeBoundary {
                        name: native.clone(),
                    },
                    span: typed_init.span,
                    reason: ConstraintReason::TypeAnnotation {
                        var_name: name.to_string(),
                    },
                });
            } else if matches!(decl, InferType::Dynamic) {
            } else {
                self.constraints.push(Constraint::equal(
                    decl.clone(),
                    typed_init.ty.clone(),
                    span,
                    ConstraintReason::TypeAnnotation {
                        var_name: name.to_string(),
                    },
                ));
            }
            decl.clone()
        } else {
            typed_init.ty.clone()
        };

        if mutable
            && is_collection_type(&var_type)
            && collection_binding_name(initializer).is_some()
            && !read_only_alias
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionAlias,
                span: initializer.span,
                reason: ConstraintReason::Other("mutable collection alias".to_string()),
            });
        }

        if name != "_" {
            let explicit_dynamic_annotation =
                type_annotation.is_some() && var_type.contains_dynamic();
            let known_length = self.constant_collection_length(initializer).filter(|_| {
                matches!(
                    &var_type,
                    InferType::Array(_) | InferType::FixedArray(_, _) | InferType::Vec(_)
                )
            });
            if let Some(length) = known_length
                && !explicit_dynamic_annotation
                && !self.is_explicit_dynamic_expr(&typed_init)
            {
                self.env.define_local_with_collection_length(
                    name.to_string(),
                    var_type.clone(),
                    length,
                );
            } else if explicit_dynamic_annotation || self.is_explicit_dynamic_expr(&typed_init) {
                self.env
                    .define_explicit_dynamic_local(name.to_string(), var_type.clone());
            } else {
                self.env.define_local(name.to_string(), var_type.clone());
            }
            self.env.set_mutable(name, mutable);
            if read_only_alias {
                self.env.mark_read_only(name);
            }
        }

        TypedStmtKind::Let {
            name: name.to_string(),
            mutable,
            initializer: typed_init,
            var_type,
            is_pub,
        }
    }
}

fn collection_binding_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        aelys_syntax::ExprKind::Identifier(name) => Some(name.as_str()),
        aelys_syntax::ExprKind::Grouping(inner) => collection_binding_name(inner),
        aelys_syntax::ExprKind::Index { object, .. } => collection_binding_name(object),
        _ => None,
    }
}

fn is_collection_type(ty: &InferType) -> bool {
    matches!(
        ty,
        InferType::Array(_) | InferType::FixedArray(_, _) | InferType::Vec(_)
    )
}
