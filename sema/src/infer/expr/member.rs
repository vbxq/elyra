use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind};
use crate::types::InferType;
use aelys_syntax::{Expr, MemberSeparator, Span, StructFieldInit};

impl TypeInference {
    pub(super) fn infer_member_expr(
        &mut self,
        object: &Expr,
        member: &str,
        separator: MemberSeparator,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        if separator == MemberSeparator::Path
            && source_path_name(object).as_deref() == Some("convert")
            && member == "is_null"
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NullIsNotInSurface,
                span: _span,
                reason: ConstraintReason::Other("null inspection".to_string()),
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier("convert".to_string()),
                        InferType::Dynamic,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Dynamic,
            );
        }
        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
            && matches!(path.as_str(), "Option" | "Result" | "Error")
            && !valid_sum_variant(path.as_str(), member)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownVariant {
                    variant: format!("{path}::{member}"),
                    expected: path.clone(),
                },
                span: _span,
                reason: ConstraintReason::Other("sum value path".to_string()),
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier(path),
                        InferType::Dynamic,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Dynamic,
            );
        }
        let option_none = separator == MemberSeparator::Path
            && source_path_name(object).as_deref() == Some("Option")
            && member == "None";
        let typed_object = if option_none {
            TypedExpr::new(
                TypedExprKind::Identifier("Option".to_string()),
                crate::types::InferType::Dynamic,
                object.span,
            )
        } else {
            self.infer_expr(object)
        };

        if option_none {
            let ty = InferType::Option(Box::new(self.type_gen.fresh()));
            self.record_sum_type("Option::None", ty.clone(), object.span.merge(_span));
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                ty,
            );
        }

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
            && let Some(signature) = self
                .known_native_signatures
                .get(&format!("{}::{}", path, member))
        {
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                signature.clone(),
            );
        }

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
            && let Some(signature) =
                crate::native::function_signature(&format!("{}::{}", path, member))
        {
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                signature,
            );
        }

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
            && let Some(signature) =
                crate::native::constant_signature(&format!("{}::{}", path, member))
        {
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                signature,
            );
        }

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
        {
            let qualified = format!("{}::{}", path, member);
            if self.known_native_globals.contains(&qualified) {
                return (
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::UntypedNative(qualified),
                );
            }

            if let Some(root) = path.split("::").next()
                && self.module_aliases.contains(root)
                && !self.known_globals.contains(&qualified)
                && !self
                    .known_globals
                    .iter()
                    .any(|name| name.starts_with(&format!("{}::", qualified)))
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::ModuleMemberNotPublic {
                        module: path,
                        member: member.to_string(),
                    },
                    span: _span,
                    reason: ConstraintReason::Other("module export lookup".to_string()),
                });
                return (
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::Dynamic,
                );
            }
        }

        let ty = match &typed_object.ty {
            InferType::Struct(name) => {
                if let Some(def) = self.type_table.get_struct(name) {
                    if let Some(field) = def.fields.iter().find(|f| f.name == member) {
                        field.ty.clone()
                    } else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownField {
                                structure: name.clone(),
                                field: member.to_string(),
                            },
                            span: _span,
                            reason: ConstraintReason::Other("struct field lookup".to_string()),
                        });
                        InferType::Dynamic
                    }
                } else {
                    InferType::Dynamic
                }
            }
            _ => InferType::Dynamic,
        };

        (
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.to_string(),
                separator,
            },
            ty,
        )
    }

    pub(super) fn infer_struct_literal(
        &mut self,
        name: &str,
        fields: &[StructFieldInit],
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let def = self.type_table.get_struct(name).cloned();
        let typed_fields: Vec<(String, Box<TypedExpr>)> = fields
            .iter()
            .map(|f| {
                let typed_value = self.infer_expr(&f.value);

                if let Some(def) = &def {
                    if let Some(field_def) = def.fields.iter().find(|df| df.name == f.name) {
                        let reason = ConstraintReason::TypeAnnotation {
                            var_name: format!("{}.{}", name, f.name),
                        };
                        if !self.reject_dynamic(
                            &typed_value.ty,
                            &field_def.ty,
                            f.span,
                            reason.clone(),
                        ) && !self.reject_untyped_native(
                            &typed_value.ty,
                            &field_def.ty,
                            f.span,
                            reason.clone(),
                        ) {
                            self.constraints.push(Constraint::equal(
                                typed_value.ty.clone(),
                                field_def.ty.clone(),
                                f.span,
                                reason,
                            ));
                        }
                    } else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownField {
                                structure: name.to_string(),
                                field: f.name.clone(),
                            },
                            span: f.span,
                            reason: ConstraintReason::Other("struct literal field".to_string()),
                        });
                    }
                }

                (f.name.clone(), Box::new(typed_value))
            })
            .collect();

        if let Some(def) = &def {
            for field in &def.fields {
                if !fields.iter().any(|value| value.name == field.name) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::MissingField {
                            structure: name.to_string(),
                            field: field.name.clone(),
                        },
                        span: _span,
                        reason: ConstraintReason::Other("struct literal field".to_string()),
                    });
                }
            }
        }

        (
            TypedExprKind::StructLiteral {
                name: name.to_string(),
                fields: typed_fields,
            },
            InferType::Struct(name.to_string()),
        )
    }
}

fn source_path_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        aelys_syntax::ExprKind::Identifier(name) => Some(name.clone()),
        aelys_syntax::ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Path,
        } => {
            let mut path = source_path_name(object)?;
            path.push_str("::");
            path.push_str(member);
            Some(path)
        }
        _ => None,
    }
}

fn valid_sum_variant(family: &str, member: &str) -> bool {
    matches!(
        (family, member),
        ("Option", "Some" | "None") | ("Result", "Ok" | "Err") | ("Error", "Message")
    )
}
