use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind};
use crate::types::InferType;
use aelys_syntax::{Expr, MemberSeparator, ModuleId, Span, StructFieldInit};
use std::collections::HashMap;

pub(crate) struct FieldVisibilityCheck<'a> {
    pub structure: &'a str,
    pub field: &'a str,
    pub is_pub: bool,
    pub owner: &'a ModuleId,
    pub span: Span,
    pub operation: &'a str,
    pub construction: bool,
}

impl TypeInference {
    pub(crate) fn check_field_visibility(&mut self, check: FieldVisibilityCheck<'_>) -> bool {
        let FieldVisibilityCheck {
            structure,
            field,
            is_pub,
            owner,
            span,
            operation,
            construction,
        } = check;
        if is_pub || self.current_module.is_same_or_descendant_of(owner) {
            return true;
        }
        let kind = if construction {
            TypeErrorKind::PrivateFieldConstruction {
                structure: structure.to_string(),
                field: field.to_string(),
                owner: owner.clone(),
                current: self.current_module.clone(),
                operation: operation.to_string(),
            }
        } else {
            TypeErrorKind::PrivateFieldAccess {
                structure: structure.to_string(),
                field: field.to_string(),
                owner: owner.clone(),
                current: self.current_module.clone(),
                operation: operation.to_string(),
            }
        };
        self.errors.push(TypeError {
            kind,
            span,
            reason: ConstraintReason::Other(format!("private field {operation}")),
        });
        false
    }

    pub(crate) fn projection_path_parts(&self, expr: &Expr) -> Option<(String, String)> {
        let aelys_syntax::ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Path,
        } = &expr.kind
        else {
            return None;
        };
        Some((source_path_name(object)?, member.clone()))
    }

    // an array length written `bounds::limit`, `limits::limit` or `self::limit`
    pub(crate) fn constant_path_int(&self, expr: &Expr) -> Option<i64> {
        let (receiver, item) = self.projection_path_parts(expr)?;
        if !self.names_associated_items(&receiver) {
            return None;
        }
        match self.resolve_constant(&receiver, &item) {
            crate::infer::ConstResolution::Value(value) => Some(value),
            _ => None,
        }
    }

    fn names_associated_items(&self, receiver: &str) -> bool {
        receiver == "Self"
            || self.type_table.has_nominal(receiver)
            || self.type_table.get_trait(receiver).is_some()
    }

    fn report_no_such_member(
        &mut self,
        receiver: &InferType,
        member: &str,
        span: Span,
    ) -> InferType {
        let kind = match receiver {
            InferType::Poison => return InferType::Poison,
            InferType::Var(_) => TypeErrorKind::UnresolvedTypeVariable,
            InferType::Param(param) => TypeErrorKind::UnboundTypeParamMethod {
                param: param.clone(),
                method: member.to_string(),
            },
            receiver => TypeErrorKind::NoSuchMember {
                receiver: receiver.clone(),
                member: member.to_string(),
            },
        };
        self.errors.push(TypeError {
            kind,
            span,
            reason: ConstraintReason::Other("member lookup".to_string()),
        });
        InferType::Poison
    }

    fn enum_path_names_an_impl_item(&self, enum_name: &str, member: &str) -> bool {
        let names_a_variant = self
            .type_table
            .get_enum(enum_name)
            .is_some_and(|definition| {
                definition
                    .variants
                    .iter()
                    .any(|variant| variant.name == member)
            });
        !names_a_variant
            && (self.type_table.method(enum_name, member).is_some()
                || !self.type_table.trait_methods(enum_name, member).is_empty())
    }

    pub(super) fn infer_member_expr(
        &mut self,
        object: &Expr,
        member: &str,
        separator: MemberSeparator,
        _span: Span,
        callee_position: bool,
    ) -> (TypedExprKind, InferType) {
        if separator == MemberSeparator::Dot
            && let Some(path) = source_path_name(object)
            && self.module_aliases.contains(&path)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ModulePathSeparator {
                    module: path.clone(),
                    member: member.to_string(),
                },
                span: _span,
                reason: ConstraintReason::Other("module path separator".to_string()),
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier(path),
                        InferType::Poison,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }

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
                        InferType::Poison,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }

        // emitted and the monomorphizer replaces it with the literal.
        if separator == MemberSeparator::Path
            && let Some(param) = source_path_name(object)
            && self.type_params_in_scope.contains(&param)
        {
            let bounds = self.bounds_in_scope_for_param(&param);
            let declaring = bounds.into_iter().find(|(name, _)| {
                self.type_table.get_trait(name).is_some_and(|definition| {
                    definition
                        .associated_consts
                        .iter()
                        .any(|(const_name, _)| const_name == member)
                })
            });
            let Some((trait_name, _)) = declaring else {
                let cause = match self.associated_item_namespace(
                    &param,
                    member,
                    crate::constraint::ItemNamespace::Const,
                ) {
                    Some(crate::infer::ProjectionNamespace::WrongNamespace { found }) => {
                        crate::constraint::ProjectionFailure::WrongNamespace { found }
                    }
                    _ => crate::constraint::ProjectionFailure::Unbound,
                };
                let reason = self.occurrence_reason("a value expression");
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver: param.clone(),
                        item: member.to_string(),
                        cause,
                    },
                    span: _span,
                    reason,
                });
                return (TypedExprKind::Null, InferType::Poison);
            };
            let declared = self
                .type_table
                .get_trait(&trait_name)
                .and_then(|definition| {
                    definition
                        .associated_consts
                        .iter()
                        .find(|(const_name, _)| const_name == member)
                        .map(|(_, ty)| ty.clone())
                })
                .unwrap_or(InferType::I64);
            return (
                TypedExprKind::AssociatedConst {
                    param,
                    trait_name,
                    item: member.to_string(),
                },
                declared,
            );
        }

        // `bounds::limit`, `source::limit` or `self::limit` one branch for
        if separator == MemberSeparator::Path
            && let Some(receiver) = source_path_name(object)
            && self.names_associated_items(&receiver)
        {
            match self.resolve_constant(&receiver, member) {
                crate::infer::ConstResolution::Value(value) => {
                    return (TypedExprKind::Int(value), InferType::I64);
                }
                crate::infer::ConstResolution::Missing => {
                    if receiver == "Self"
                        && self.current_impl_self.is_none()
                        && self.current_trait_name.is_none()
                    {
                        let reason = self.occurrence_reason("a value expression");
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::AmbiguousAssociatedProjection {
                                receiver: receiver.clone(),
                                item: member.to_string(),
                                cause: crate::constraint::ProjectionFailure::SelfOutsideImpl,
                            },
                            span: _span,
                            reason,
                        });
                        return (TypedExprKind::Null, InferType::Poison);
                    }
                    if let Some(crate::infer::ProjectionNamespace::WrongNamespace { found }) = self
                        .associated_item_namespace(
                            &receiver,
                            member,
                            crate::constraint::ItemNamespace::Const,
                        )
                    {
                        let reason = self.occurrence_reason("a value expression");
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::AmbiguousAssociatedProjection {
                                receiver: self.projection_receiver(&receiver),
                                item: member.to_string(),
                                cause: crate::constraint::ProjectionFailure::WrongNamespace {
                                    found,
                                },
                            },
                            span: _span,
                            reason,
                        });
                        return (TypedExprKind::Null, InferType::Poison);
                    }
                }
                failure => {
                    let reason = self.occurrence_reason("a value expression");
                    let cause = projection_failure_for(&failure, &receiver, member);
                    let named = match cause {
                        crate::constraint::ProjectionFailure::Cyclic { .. } => receiver.clone(),
                        _ => self.projection_receiver(&receiver),
                    };
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::AmbiguousAssociatedProjection {
                            receiver: named,
                            item: member.to_string(),
                            cause,
                        },
                        span: _span,
                        reason,
                    });
                    return (TypedExprKind::Null, InferType::Poison);
                }
            }
        }

        if separator == MemberSeparator::Path
            && let Some(path_name) = source_path_name(object)
            && let Some(module) = self.withholding_module(&path_name).map(str::to_string)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::TypeNotImported {
                    name: path_name.clone(),
                    module,
                },
                span: _span,
                reason: ConstraintReason::UnknownType {
                    name: path_name.clone(),
                },
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier(path_name),
                        InferType::Poison,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }

        if separator == MemberSeparator::Path
            && let Some(enum_name) = source_path_name(object)
            && let Some(enum_def) = self.type_table.get_enum(&enum_name).cloned()
            && !self.enum_path_names_an_impl_item(&enum_name, member)
        {
            let explicit_type_args = source_path_type_args(object)
                .unwrap_or_default()
                .iter()
                .map(|annotation| self.type_from_annotation(annotation))
                .collect::<Vec<_>>();
            let substitutions = enum_def
                .type_params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    (
                        param.clone(),
                        explicit_type_args
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| self.type_gen.fresh()),
                    )
                })
                .collect::<HashMap<_, _>>();
            if !explicit_type_args.is_empty()
                && explicit_type_args.len() != enum_def.type_params.len()
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::GenericArityMismatch {
                        name: enum_name.clone(),
                        expected: enum_def.type_params.len(),
                        found: explicit_type_args.len(),
                    },
                    span: _span,
                    reason: ConstraintReason::Other("enum type arguments".to_string()),
                });
            }
            let enum_type = if enum_def.type_params.is_empty() {
                InferType::Struct(enum_name.clone())
            } else {
                InferType::Applied {
                    name: enum_name.clone(),
                    args: enum_def
                        .type_params
                        .iter()
                        .map(|param| {
                            substitutions
                                .get(param)
                                .cloned()
                                .unwrap_or(InferType::Poison)
                        })
                        .collect(),
                }
            };
            let Some((variant_index, variant)) = enum_def
                .variants
                .iter()
                .enumerate()
                .find(|(_, variant)| variant.name == member)
            else {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnknownVariant {
                        variant: format!("{enum_name}::{member}"),
                        expected: enum_name.clone(),
                    },
                    span: _span,
                    reason: ConstraintReason::Other("enum constructor path".to_string()),
                });
                return (
                    TypedExprKind::Member {
                        object: Box::new(TypedExpr::new(
                            TypedExprKind::Identifier(enum_name),
                            InferType::Poison,
                            object.span,
                        )),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::Poison,
                );
            };
            if !matches!(&variant.fields, crate::types::EnumVariantFieldsDef::Unit) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidSumMethod {
                        method: format!("{enum_name}::{member}"),
                        receiver: InferType::Struct(enum_name.clone()),
                    },
                    span: _span,
                    reason: ConstraintReason::Other("enum constructor arity".to_string()),
                });
                return (
                    TypedExprKind::Member {
                        object: Box::new(TypedExpr::new(
                            TypedExprKind::Identifier(enum_name),
                            InferType::Poison,
                            object.span,
                        )),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::Poison,
                );
            }
            return (
                TypedExprKind::EnumConstruct {
                    enum_name: enum_name.clone(),
                    variant: member.to_string(),
                    schema_index: self.type_table.enum_schema_index(&enum_name).unwrap_or(0),
                    variant_index: u16::try_from(variant_index).unwrap_or(0),
                    fields: Vec::new(),
                },
                enum_type,
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
                        InferType::Poison,
                        object.span,
                    )),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }
        let option_none = separator == MemberSeparator::Path
            && source_path_name(object).as_deref() == Some("Option")
            && member == "None";

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
            && let Some(method) = self.type_table.method(&path, member).cloned()
            && !method.has_self
        {
            let typed_object = TypedExpr::new(
                TypedExprKind::Identifier(path),
                InferType::Poison,
                object.span,
            );
            return (
                TypedExprKind::StructMethod {
                    object: Box::new(typed_object),
                    symbol: method.symbol,
                    method: member.to_string(),
                    separator,
                },
                InferType::Function {
                    params: method.params,
                    ret: Box::new(method.return_type),
                },
            );
        }

        if separator == MemberSeparator::Path
            && let Some(path) = source_path_name(object)
        {
            let candidates: Vec<_> = self
                .type_table
                .trait_methods(&path, member)
                .iter()
                .filter(|candidate| !candidate.has_self)
                .cloned()
                .collect();
            match candidates.as_slice() {
                [method] => {
                    let typed_object = TypedExpr::new(
                        TypedExprKind::Identifier(path),
                        InferType::Poison,
                        object.span,
                    );
                    return (
                        TypedExprKind::StructMethod {
                            object: Box::new(typed_object),
                            symbol: method.symbol.clone(),
                            method: member.to_string(),
                            separator,
                        },
                        InferType::Function {
                            params: method.params.clone(),
                            ret: Box::new(method.return_type.clone()),
                        },
                    );
                }
                [] => {}
                _ => {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::AmbiguousTraitMethod {
                            target: path,
                            method: member.to_string(),
                        },
                        span: _span,
                        reason: ConstraintReason::Other(
                            "associated trait method lookup".to_string(),
                        ),
                    });
                }
            }
        }
        // the root of a '::' path names a namespace, so it is never inferred as a value
        let path_root = (separator == MemberSeparator::Path)
            .then(|| source_path_name(object))
            .flatten();
        let typed_object = match &path_root {
            Some(path) => namespace_root(path, object.span),
            None => self.infer_expr(object),
        };

        if separator == MemberSeparator::Dot && member == "to_string" {
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                InferType::Function {
                    params: Vec::new(),
                    ret: Box::new(InferType::String),
                },
            );
        }

        if separator == MemberSeparator::Dot && matches!(typed_object.ty, InferType::Var(_)) {
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                self.type_gen.fresh(),
            );
        }

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

            if let Some(root) = path.split("::").next().map(str::to_string)
                && self.module_aliases.contains(&root)
                && !self.known_globals.contains(&qualified)
                && !self
                    .known_globals
                    .iter()
                    .any(|name| name.starts_with(&format!("{}::", qualified)))
            {
                // only a path that is itself the alias names a module member,
                let kind = if self.module_aliases.contains(&path) {
                    TypeErrorKind::ModuleMemberNotPublic {
                        module: path,
                        member: member.to_string(),
                    }
                } else {
                    TypeErrorKind::UnknownTypeName { name: root }
                };
                self.errors.push(TypeError {
                    kind,
                    span: _span,
                    reason: ConstraintReason::Other("module export lookup".to_string()),
                });
                return (
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::Poison,
                );
            }
        }

        if let Some(path) = &path_root {
            let lookup_reason = ConstraintReason::Other("namespace path lookup".to_string());
            let (kind, reason) =
                if self.env.lookup(path).is_some() || self.env.lookup_function(path).is_some() {
                    (
                        TypeErrorKind::NotANamespace { name: path.clone() },
                        lookup_reason,
                    )
                } else if !callee_position && self.names_associated_items(path) {
                    (
                        TypeErrorKind::AmbiguousAssociatedProjection {
                            receiver: self.projection_receiver(path),
                            item: member.to_string(),
                            cause: crate::constraint::ProjectionFailure::NoImpl,
                        },
                        self.occurrence_reason("a value expression"),
                    )
                } else {
                    (
                        TypeErrorKind::UndefinedFunction {
                            name: format!("{}::{}", path, member),
                        },
                        lookup_reason,
                    )
                };
            self.errors.push(TypeError {
                kind,
                span: _span,
                reason,
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }

        if separator == MemberSeparator::Dot
            && let InferType::Param(param) = typed_object.ty.clone()
        {
            return self.infer_type_param_method(typed_object, &param, member, _span, separator);
        }

        if separator == MemberSeparator::Dot
            && let Some((name, substitutions)) = nominal_parts(&typed_object.ty, &self.type_table)
        {
            let has_field = self
                .type_table
                .get_struct(&name)
                .is_some_and(|def| def.fields.iter().any(|field| field.name == member));
            let inherent = (!has_field)
                .then(|| self.type_table.method(&name, member).cloned())
                .flatten()
                .filter(|method| method.has_self);
            let (mut method, trait_ambiguous) = if inherent.is_some() {
                (inherent, false)
            } else if !has_field {
                let candidates: Vec<_> = self
                    .type_table
                    .trait_methods(&name, member)
                    .iter()
                    .filter(|candidate| candidate.has_self)
                    .cloned()
                    .collect();
                match candidates.as_slice() {
                    [candidate] => (
                        Some(crate::types::StructMethod {
                            name: candidate.name.clone(),
                            symbol: candidate.symbol.clone(),
                            params: candidate.params.clone(),
                            return_type: candidate.return_type.clone(),
                            has_self: candidate.has_self,
                            mutable_self: candidate.mutable_self,
                        }),
                        false,
                    ),
                    [] => (None, false),
                    _ => (None, true),
                }
            } else {
                (None, false)
            };
            if trait_ambiguous {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousTraitMethod {
                        target: name.clone(),
                        method: member.to_string(),
                    },
                    span: _span,
                    reason: ConstraintReason::Other("trait method lookup".to_string()),
                });
                return (
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: member.to_string(),
                        separator,
                    },
                    InferType::Poison,
                );
            }
            if let Some(mut method) = method.take() {
                if method.mutable_self
                    && !root_binding_name(object).is_some_and(|root| self.env.is_mutable(root))
                {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::ImmutableStructMethod {
                            method: member.to_string(),
                        },
                        span: _span,
                        reason: ConstraintReason::Other("mutable struct receiver".to_string()),
                    });
                }
                method.params = method
                    .params
                    .iter()
                    .map(|param| param.substitute_params(&substitutions))
                    .collect();
                method.return_type = method.return_type.substitute_params(&substitutions);
                let params = method.params.into_iter().skip(1).collect();
                return (
                    TypedExprKind::StructMethod {
                        object: Box::new(typed_object),
                        symbol: method.symbol,
                        method: member.to_string(),
                        separator,
                    },
                    InferType::Function {
                        params,
                        ret: Box::new(method.return_type),
                    },
                );
            }
        }

        let ty = match nominal_parts(&typed_object.ty, &self.type_table) {
            Some((name, substitutions)) => {
                if let Some(def) = self.type_table.get_struct(&name).cloned() {
                    if let Some(field) = def.fields.iter().find(|f| f.name == member) {
                        if !self.check_field_visibility(FieldVisibilityCheck {
                            structure: &name,
                            field: member,
                            is_pub: field.is_pub,
                            owner: &def.owner,
                            span: _span,
                            operation: "read",
                            construction: false,
                        }) {
                            return (
                                TypedExprKind::Member {
                                    object: Box::new(typed_object),
                                    member: member.to_string(),
                                    separator,
                                },
                                InferType::Poison,
                            );
                        }
                        let offset = field.ordinal;
                        let schema_index = self.type_table.schema_index(&name).unwrap_or(0);
                        return (
                            TypedExprKind::StructField {
                                object: Box::new(typed_object),
                                member: member.to_string(),
                                offset,
                                schema_index,
                            },
                            field.ty.substitute_params(&substitutions),
                        );
                    } else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownField {
                                structure: name.clone(),
                                field: member.to_string(),
                            },
                            span: _span,
                            reason: ConstraintReason::Other("struct field lookup".to_string()),
                        });
                        InferType::Poison
                    }
                } else {
                    self.report_no_such_member(&typed_object.ty, member, _span)
                }
            }
            _ => self.report_no_such_member(&typed_object.ty, member, _span),
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
        type_args: &[aelys_syntax::TypeAnnotation],
        fields: &[StructFieldInit],
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let def = self.type_table.get_struct(name).cloned();
        if def.is_none() {
            self.errors.push(TypeError {
                kind: self.nominal_error_kind(
                    name,
                    TypeErrorKind::UnknownStruct {
                        name: name.to_string(),
                    },
                ),
                span: _span,
                reason: ConstraintReason::UnknownType {
                    name: name.to_string(),
                },
            });
        }
        let explicit_type_args = type_args
            .iter()
            .map(|annotation| self.type_from_annotation(annotation))
            .collect::<Vec<_>>();
        if let Some(definition) = &def
            && !explicit_type_args.is_empty()
            && explicit_type_args.len() != definition.type_params.len()
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::GenericArityMismatch {
                    name: name.to_string(),
                    expected: definition.type_params.len(),
                    found: explicit_type_args.len(),
                },
                span: _span,
                reason: ConstraintReason::Other("struct type arguments".to_string()),
            });
        }
        let substitutions = def
            .as_ref()
            .map(|definition| {
                definition
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        (
                            param.clone(),
                            explicit_type_args
                                .get(index)
                                .cloned()
                                .unwrap_or_else(|| self.type_gen.fresh()),
                        )
                    })
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let instance_type = if let Some(definition) = &def {
            if definition.type_params.is_empty() {
                InferType::Struct(name.to_string())
            } else {
                InferType::Applied {
                    name: name.to_string(),
                    args: definition
                        .type_params
                        .iter()
                        .map(|param| {
                            substitutions
                                .get(param)
                                .cloned()
                                .unwrap_or(InferType::Poison)
                        })
                        .collect(),
                }
            }
        } else {
            InferType::Poison
        };
        let mut seen = std::collections::HashSet::new();
        let mut field_visibility_error = false;
        let mut field_offsets = Vec::with_capacity(fields.len());
        let typed_fields: Vec<(String, Box<TypedExpr>)> = fields
            .iter()
            .filter_map(|f| {
                let typed_value = self.infer_expr(&f.value);

                if !seen.insert(f.name.clone()) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::DuplicateStructField {
                            structure: name.to_string(),
                            field: f.name.clone(),
                        },
                        span: f.span,
                        reason: ConstraintReason::Other("struct literal field".to_string()),
                    });
                }

                if let Some(def) = &def {
                    if let Some(field_def) = def.fields.iter().find(|df| df.name == f.name) {
                        let visible = self.check_field_visibility(FieldVisibilityCheck {
                            structure: name,
                            field: &f.name,
                            is_pub: field_def.is_pub,
                            owner: &def.owner,
                            span: f.span,
                            operation: "a struct literal",
                            construction: true,
                        });
                        if !visible {
                            field_visibility_error = true;
                            return None;
                        }
                        field_offsets.push(field_def.ordinal);
                        let field_ty = field_def.ty.substitute_params(&substitutions);
                        let reason = ConstraintReason::TypeAnnotation {
                            var_name: format!("{}.{}", name, f.name),
                        };
                        if !self.reject_dynamic(&typed_value.ty, &field_ty, f.span, reason.clone())
                            && !self.reject_untyped_native(
                                &typed_value.ty,
                                &field_ty,
                                f.span,
                                reason.clone(),
                            )
                        {
                            self.constraints.push(Constraint::equal(
                                field_ty,
                                typed_value.ty.clone(),
                                f.span,
                                reason,
                            ));
                        }
                    } else {
                        field_offsets.push(0);
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

                if def.is_none() {
                    field_offsets.push(0);
                }

                Some((f.name.clone(), Box::new(typed_value)))
            })
            .collect();

        if let Some(def) = &def {
            for field in &def.fields {
                if !fields.iter().any(|value| value.name == field.name) {
                    if !self.check_field_visibility(FieldVisibilityCheck {
                        structure: name,
                        field: &field.name,
                        is_pub: field.is_pub,
                        owner: &def.owner,
                        span: _span,
                        operation: "a struct literal",
                        construction: true,
                    }) {
                        continue;
                    }
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

        if field_visibility_error {
            return (
                TypedExprKind::StructLiteral {
                    name: name.to_string(),
                    schema_index: self.type_table.schema_index(name).unwrap_or(0),
                    fields: typed_fields,
                    field_offsets,
                },
                InferType::Poison,
            );
        }

        let schema_index = self.type_table.schema_index(name).unwrap_or(0);
        (
            TypedExprKind::StructLiteral {
                name: name.to_string(),
                schema_index,
                fields: typed_fields,
                field_offsets,
            },
            instance_type,
        )
    }

    pub(super) fn infer_member_assign_expr(
        &mut self,
        object: &Expr,
        member: &str,
        value: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_object = self.infer_expr(object);
        let typed_value = self.infer_expr(value);
        let Some(root) = root_binding_name(object) else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ImmutableStructField {
                    field: member.to_string(),
                },
                span,
                reason: ConstraintReason::Other("struct place assignment".to_string()),
            });
            return (
                TypedExprKind::MemberAssign {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    offset: 0,
                    schema_index: 0,
                    value: Box::new(typed_value),
                },
                InferType::Poison,
            );
        };
        self.check_write_access(object, span, "mutation");
        if self.env.borrow_kind(root).is_none() && !self.env.is_mutable(root) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ImmutableStructField {
                    field: member.to_string(),
                },
                span,
                reason: ConstraintReason::Other("struct place assignment".to_string()),
            });
        }
        let (offset, schema_index, field_ty) =
            match nominal_parts(&typed_object.ty, &self.type_table) {
                Some((name, substitutions)) => {
                    if let Some(definition) = self.type_table.get_struct(&name).cloned()
                        && let Some(field) =
                            definition.fields.iter().find(|field| field.name == member)
                        && !self.check_field_visibility(FieldVisibilityCheck {
                            structure: &name,
                            field: member,
                            is_pub: field.is_pub,
                            owner: &definition.owner,
                            span,
                            operation: "write",
                            construction: false,
                        })
                    {
                        return (
                            TypedExprKind::MemberAssign {
                                object: Box::new(typed_object),
                                member: member.to_string(),
                                offset: 0,
                                schema_index: 0,
                                value: Box::new(typed_value),
                            },
                            InferType::Poison,
                        );
                    }
                    let offset = self.type_table.field_offset(&name, member);
                    let field_ty = self
                        .type_table
                        .get_struct(&name)
                        .and_then(|def| def.fields.iter().find(|field| field.name == member))
                        .map(|field| field.ty.substitute_params(&substitutions));
                    match (offset, field_ty) {
                        (Some(offset), Some(field_ty)) => (
                            offset,
                            self.type_table.schema_index(&name).unwrap_or(0),
                            field_ty,
                        ),
                        _ => {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::UnknownField {
                                    structure: name.clone(),
                                    field: member.to_string(),
                                },
                                span,
                                reason: ConstraintReason::Other(
                                    "struct field assignment".to_string(),
                                ),
                            });
                            (0, 0, InferType::Poison)
                        }
                    }
                }
                _ => (0, 0, InferType::Poison),
            };
        if !self.reject_dynamic(
            &typed_value.ty,
            &field_ty,
            value.span,
            ConstraintReason::Assignment {
                var_name: format!("struct field {}", member),
            },
        ) {
            self.constraints.push(Constraint::equal(
                field_ty.clone(),
                typed_value.ty.clone(),
                value.span,
                ConstraintReason::Assignment {
                    var_name: format!("struct field {}", member),
                },
            ));
        }
        (
            TypedExprKind::MemberAssign {
                object: Box::new(typed_object),
                member: member.to_string(),
                offset,
                schema_index,
                value: Box::new(typed_value),
            },
            field_ty,
        )
    }

    pub(super) fn infer_enum_literal(
        &mut self,
        path: &[String],
        type_args: &[aelys_syntax::TypeAnnotation],
        fields: &[StructFieldInit],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let enum_name = path.first().cloned().unwrap_or_default();
        let variant_name = path.last().cloned().unwrap_or_default();
        let Some(def) = self.type_table.get_enum(&enum_name).cloned() else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownVariant {
                    variant: path.join("::"),
                    expected: enum_name.clone(),
                },
                span,
                reason: ConstraintReason::UnknownType {
                    name: enum_name.clone(),
                },
            });
            return (
                TypedExprKind::EnumConstruct {
                    enum_name,
                    variant: variant_name,
                    schema_index: 0,
                    variant_index: 0,
                    fields: Vec::new(),
                },
                InferType::Poison,
            );
        };
        if path.len() < 2 || path[..path.len() - 1].join("::") != enum_name {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownVariant {
                    variant: path.join("::"),
                    expected: enum_name.clone(),
                },
                span,
                reason: ConstraintReason::Other("enum constructor path".to_string()),
            });
            return (
                TypedExprKind::EnumConstruct {
                    enum_name,
                    variant: variant_name,
                    schema_index: 0,
                    variant_index: 0,
                    fields: Vec::new(),
                },
                InferType::Poison,
            );
        }
        let explicit_types = type_args
            .iter()
            .map(|annotation| self.type_from_annotation(annotation))
            .collect::<Vec<_>>();
        if explicit_types.len() != def.type_params.len() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::GenericArityMismatch {
                    name: enum_name.clone(),
                    expected: def.type_params.len(),
                    found: explicit_types.len(),
                },
                span,
                reason: ConstraintReason::Other("enum literal type arguments".to_string()),
            });
        }
        let substitutions = def
            .type_params
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                (
                    parameter.clone(),
                    explicit_types
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| self.type_gen.fresh()),
                )
            })
            .collect::<HashMap<_, _>>();
        let enum_type = if def.type_params.is_empty() {
            InferType::Struct(enum_name.clone())
        } else {
            InferType::Applied {
                name: enum_name.clone(),
                args: def
                    .type_params
                    .iter()
                    .map(|parameter| {
                        substitutions
                            .get(parameter)
                            .cloned()
                            .unwrap_or(InferType::Poison)
                    })
                    .collect(),
            }
        };
        let Some((variant_index, variant)) = def
            .variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == variant_name)
        else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownVariant {
                    variant: path.join("::"),
                    expected: enum_name.clone(),
                },
                span,
                reason: ConstraintReason::Other("enum literal variant".to_string()),
            });
            return (
                TypedExprKind::EnumConstruct {
                    enum_name,
                    variant: variant_name,
                    schema_index: 0,
                    variant_index: 0,
                    fields: Vec::new(),
                },
                InferType::Poison,
            );
        };
        let crate::types::EnumVariantFieldsDef::Named(expected_fields) = &variant.fields else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::InvalidSumMethod {
                    method: format!("{}::{}", enum_name, variant_name),
                    receiver: enum_type.clone(),
                },
                span,
                reason: ConstraintReason::Other("enum literal field shape".to_string()),
            });
            return (
                TypedExprKind::EnumConstruct {
                    enum_name,
                    variant: variant_name,
                    schema_index: 0,
                    variant_index: u16::try_from(variant_index).unwrap_or(0),
                    fields: Vec::new(),
                },
                InferType::Poison,
            );
        };

        let mut seen = std::collections::HashSet::new();
        let mut field_visibility_error = false;
        let mut typed_by_name = std::collections::HashMap::new();
        for field in fields {
            let typed_value = self.infer_expr(&field.value);
            if !seen.insert(field.name.clone()) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::DuplicateStructField {
                        structure: format!("{}::{}", enum_name, variant_name),
                        field: field.name.clone(),
                    },
                    span: field.span,
                    reason: ConstraintReason::Other("enum literal field".to_string()),
                });
            }
            if let Some(expected) = expected_fields.iter().find(|f| f.name == field.name) {
                let visible = self.check_field_visibility(FieldVisibilityCheck {
                    structure: &format!("{}::{}", enum_name, variant_name),
                    field: &field.name,
                    is_pub: expected.is_pub,
                    owner: &def.owner,
                    span: field.span,
                    operation: "an enum literal",
                    construction: true,
                });
                if !visible {
                    field_visibility_error = true;
                    continue;
                }
                let reason = ConstraintReason::TypeAnnotation {
                    var_name: format!("{}::{}.{}", enum_name, variant_name, field.name),
                };
                let expected_ty = expected.ty.substitute_params(&substitutions);
                if !self.reject_dynamic(&typed_value.ty, &expected_ty, field.span, reason.clone())
                    && !self.reject_untyped_native(
                        &typed_value.ty,
                        &expected_ty,
                        field.span,
                        reason.clone(),
                    )
                {
                    self.constraints.push(Constraint::equal(
                        expected_ty,
                        typed_value.ty.clone(),
                        field.span,
                        reason,
                    ));
                }
            } else {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnknownField {
                        structure: format!("{}::{}", enum_name, variant_name),
                        field: field.name.clone(),
                    },
                    span: field.span,
                    reason: ConstraintReason::Other("enum literal field".to_string()),
                });
            }
            typed_by_name
                .entry(field.name.clone())
                .or_insert_with(|| Box::new(typed_value));
        }
        let mut typed_fields = Vec::with_capacity(expected_fields.len());
        for expected in expected_fields {
            if let Some(value) = typed_by_name.remove(&expected.name) {
                typed_fields.push((Some(expected.name.clone()), value));
            } else {
                if !self.check_field_visibility(FieldVisibilityCheck {
                    structure: &format!("{}::{}", enum_name, variant_name),
                    field: &expected.name,
                    is_pub: expected.is_pub,
                    owner: &def.owner,
                    span,
                    operation: "an enum literal",
                    construction: true,
                }) {
                    field_visibility_error = true;
                    continue;
                }
                self.errors.push(TypeError {
                    kind: TypeErrorKind::MissingField {
                        structure: format!("{}::{}", enum_name, variant_name),
                        field: expected.name.clone(),
                    },
                    span,
                    reason: ConstraintReason::Other("enum literal field".to_string()),
                });
            }
        }
        if field_visibility_error {
            return (
                TypedExprKind::EnumConstruct {
                    enum_name: enum_name.clone(),
                    variant: variant_name,
                    schema_index: self.type_table.enum_schema_index(&enum_name).unwrap_or(0),
                    variant_index: u16::try_from(variant_index).unwrap_or(0),
                    fields: typed_fields,
                },
                InferType::Poison,
            );
        }
        (
            TypedExprKind::EnumConstruct {
                enum_name: enum_name.clone(),
                variant: variant_name,
                schema_index: self.type_table.enum_schema_index(&enum_name).unwrap_or(0),
                variant_index: u16::try_from(variant_index).unwrap_or(0),
                fields: typed_fields,
            },
            enum_type,
        )
    }

    // a lambda body is a different "current function", so the bounds of the
    pub(crate) fn bounds_in_scope_for_param(&self, param: &str) -> Vec<(String, Vec<InferType>)> {
        let bounds = self.param_bounds_in_scope(param);
        if !bounds.is_empty() {
            return bounds;
        }
        self.current_function_bounds
            .iter()
            .filter(|(subject, _, _)| subject == param)
            .map(|(_, trait_name, args)| (trait_name.clone(), args.clone()))
            .collect()
    }

    fn param_bounds_in_scope(&self, param: &str) -> Vec<(String, Vec<InferType>)> {
        let Some(function) = self.env.current_function() else {
            return Vec::new();
        };
        let Some(declared) = self.generic_function_bounds.get(function) else {
            return Vec::new();
        };
        let mut pending: Vec<(String, Vec<InferType>)> = declared
            .iter()
            .filter(|(subject, _, _)| subject == param)
            .map(|(_, trait_name, trait_args)| (trait_name.clone(), trait_args.clone()))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut resolved = Vec::new();
        while let Some((trait_name, trait_args)) = pending.pop() {
            if !seen.insert(trait_name.clone()) {
                continue;
            }
            if let Some(definition) = self.type_table.get_trait(&trait_name) {
                for super_bound in &definition.super_bounds {
                    pending.push((super_bound.clone(), Vec::new()));
                }
            }
            resolved.push((trait_name, trait_args));
        }
        resolved
    }

    fn infer_type_param_method(
        &mut self,
        typed_object: TypedExpr,
        param: &str,
        member: &str,
        span: Span,
        separator: MemberSeparator,
    ) -> (TypedExprKind, InferType) {
        let mut candidates = Vec::new();
        for (trait_name, trait_args) in self.bounds_in_scope_for_param(param) {
            let Some(definition) = self.type_table.get_trait(&trait_name) else {
                continue;
            };
            let Some(method) = definition
                .methods
                .iter()
                .find(|candidate| candidate.name == member && candidate.has_self)
            else {
                continue;
            };
            candidates.push((
                trait_name,
                definition.type_params.clone(),
                method.clone(),
                trait_args,
            ));
        }

        if candidates.len() > 1 {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AmbiguousTraitMethod {
                    target: param.to_string(),
                    method: member.to_string(),
                },
                span,
                reason: ConstraintReason::Other("type parameter bound lookup".to_string()),
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        }

        let Some((trait_name, trait_type_params, method, trait_args)) = candidates.pop() else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnboundTypeParamMethod {
                    param: param.to_string(),
                    method: member.to_string(),
                },
                span,
                reason: ConstraintReason::Other("type parameter bound lookup".to_string()),
            });
            return (
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.to_string(),
                    separator,
                },
                InferType::Poison,
            );
        };

        let mut substitutions =
            HashMap::from([("Self".to_string(), InferType::Param(param.to_string()))]);
        for (parameter, argument) in trait_type_params.iter().zip(&trait_args) {
            substitutions.insert(parameter.clone(), argument.clone());
        }
        let params = method
            .params
            .iter()
            .skip(1)
            .map(|parameter| parameter.substitute_params(&substitutions))
            .collect();
        let return_type = self.normalize_projection_types(
            &method.return_type.substitute_params(&substitutions),
            span,
            &ConstraintReason::Return {
                func_name: member.to_string(),
            },
        );
        (
            TypedExprKind::StructMethod {
                object: Box::new(typed_object),
                symbol: crate::infer::monomorphize::bound_marker_symbol(&trait_name, param, member),
                method: member.to_string(),
                separator,
            },
            InferType::Function {
                params,
                ret: Box::new(return_type),
            },
        )
    }
}

fn root_binding_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        aelys_syntax::ExprKind::Identifier(name) => Some(name),
        aelys_syntax::ExprKind::Grouping(inner) => root_binding_name(inner),
        aelys_syntax::ExprKind::Member { object, .. } => root_binding_name(object),
        _ => None,
    }
}

pub(crate) fn nominal_parts(
    ty: &InferType,
    type_table: &crate::types::TypeTable,
) -> Option<(String, HashMap<String, InferType>)> {
    let (name, args) = match ty {
        InferType::Struct(name) => (name, &[][..]),
        InferType::Applied { name, args } => (name, args.as_slice()),
        _ => return None,
    };
    let type_params = match type_table.get_struct(name) {
        Some(definition) => &definition.type_params,
        None => &type_table.get_enum(name)?.type_params,
    };
    if type_params.len() != args.len() {
        return None;
    }
    let substitutions = type_params
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect();
    Some((name.clone(), substitutions))
}

fn namespace_root(path: &str, span: Span) -> TypedExpr {
    TypedExpr::new(
        TypedExprKind::Identifier(path.to_string()),
        InferType::Poison,
        span,
    )
}

fn source_path_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        aelys_syntax::ExprKind::Identifier(name) => Some(name.clone()),
        aelys_syntax::ExprKind::GenericApply { callee, .. } => source_path_name(callee),
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

fn source_path_type_args(expr: &Expr) -> Option<&[aelys_syntax::TypeAnnotation]> {
    match &expr.kind {
        aelys_syntax::ExprKind::GenericApply { type_args, .. } => Some(type_args),
        aelys_syntax::ExprKind::Member {
            object,
            separator: MemberSeparator::Path,
            ..
        } => source_path_type_args(object),
        _ => None,
    }
}

fn valid_sum_variant(family: &str, member: &str) -> bool {
    matches!(
        (family, member),
        ("Option", "Some" | "None") | ("Result", "Ok" | "Err") | ("Error", "Message")
    )
}

pub(crate) fn projection_failure_for(
    failure: &crate::infer::ConstResolution,
    receiver: &str,
    item: &str,
) -> crate::constraint::ProjectionFailure {
    use crate::constraint::ProjectionFailure;
    use crate::infer::ConstResolution;
    match failure {
        ConstResolution::Ambiguous(traits) => ProjectionFailure::Ambiguous {
            traits: traits.clone(),
        },
        ConstResolution::NotComputable => ProjectionFailure::NotComputable,
        ConstResolution::NotConstant => ProjectionFailure::NotConstant,
        ConstResolution::Cyclic => ProjectionFailure::Cyclic {
            path: vec![format!("{receiver}::{item}")],
            namespace: crate::constraint::ItemNamespace::Const,
        },
        ConstResolution::Missing | ConstResolution::Value(_) => ProjectionFailure::NoImpl,
    }
}
