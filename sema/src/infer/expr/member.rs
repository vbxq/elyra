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

    pub(crate) fn constant_length_origin(&self, expr: &Expr) -> Option<String> {
        let (receiver, item) = self.projection_path_parts(expr)?;
        if !self.names_associated_items(&receiver) {
            return None;
        }
        Some(format!("{}::{item}", self.projection_receiver(&receiver)))
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
        let nominal = match receiver {
            InferType::Struct(name) | InferType::Applied { name, .. } => Some(name.as_str()),
            _ => None,
        };
        if let Some(target) = nominal
            && let Some(gated) = self.supertrait_gate_error(target, member, span)
        {
            self.errors.push(gated);
            return InferType::Poison;
        }
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
            let looked_up = self
                .method_param_renames
                .get(&param)
                .cloned()
                .unwrap_or_else(|| param.clone());
            let bounds = self.bounds_in_scope_for_param(&looked_up);
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
                    &looked_up,
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
                    param: looked_up,
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
                    let cause = projection_failure_for(
                        &failure,
                        &receiver,
                        member,
                        self.type_table.get_trait(&receiver).is_some()
                            && !self.type_table.has_nominal(&receiver),
                        self.non_integer_const_type(&receiver, member),
                    );
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
            && let Some(module) = self
                .withholding_module(&path_name)
                .or_else(|| self.refused_private_nominal(&path_name))
                .map(str::to_string)
        {
            let kind = match self.private_nominal_error(&path_name, _span) {
                Some(error) if self.unexported_nominals.contains(&path_name) => error.kind,
                _ => TypeErrorKind::TypeNotImported {
                    name: path_name.clone(),
                    module,
                },
            };
            self.errors.push(TypeError {
                kind,
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
            && let Some(path) =
                source_path_name(object).map(|written| self.associated_lookup_receiver(&written))
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
            && let Some(written) = source_path_name(object)
        {
            let path = self.associated_lookup_receiver(&written);
            let candidates: Vec<_> = self
                .visible_trait_methods(&path, member)
                .into_iter()
                .filter(|candidate| !candidate.has_self)
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
                    let symbols: Vec<String> = candidates
                        .iter()
                        .map(|candidate| candidate.symbol.clone())
                        .collect();
                    let mut kind = crate::infer::signatures::ambiguous_trait_method_kind(
                        &self.type_table,
                        &path,
                        member,
                        &symbols,
                    );
                    let named = self.projection_receiver(&written);
                    match &mut kind {
                        TypeErrorKind::AmbiguousTraitInstantiation { target, .. }
                        | TypeErrorKind::AmbiguousTraitMethod { target, .. } => *target = named,
                        _ => {}
                    }
                    self.errors.push(TypeError {
                        kind,
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
                            name: format!("{}::{}", self.projection_receiver(path), member),
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
            // the header binds the receiver only when it is the one that will run
            let off_instantiation: Vec<String> = match &self.adopted_instantiation {
                Some((trait_name, header, args)) if *header == typed_object.ty => self
                    .type_table
                    .symbols_at_other_instantiations(trait_name, header, args, member),
                _ => Vec::new(),
            };
            let settled = typed_object.ty.is_concrete() || typed_object.ty.is_rigid();
            let standing: Vec<String> = match settled {
                true => {
                    let mut ruled_out = self
                        .type_table
                        .symbols_not_applying(&typed_object.ty, member);
                    ruled_out.extend(self.type_table.symbols_outranked(&typed_object.ty, member));
                    ruled_out.extend(off_instantiation.iter().cloned());
                    self.visible_trait_methods(&name, member)
                        .iter()
                        .filter(|candidate| {
                            candidate.has_self && !ruled_out.contains(&candidate.symbol)
                        })
                        .map(|candidate| candidate.symbol.clone())
                        .collect()
                }
                false => Vec::new(),
            };
            let covered = matches!(standing.as_slice(), [only]
                if self.type_table.impl_covers(only, &typed_object.ty));
            // an impl that reaches a generic receiver without covering it runs for some instances only
            let left_to_instances = settled
                && !typed_object.ty.is_concrete()
                && !covered
                && typed_through_declaration(&self.type_table, &name, member);
            let chosen_by_receiver = standing.len() == 1 && !left_to_instances;
            let header_decides = inherent.is_some()
                || (self
                    .visible_trait_methods(&name, member)
                    .iter()
                    .filter(|candidate| candidate.has_self)
                    .count()
                    == 1
                    && !left_to_instances);
            let mut ambiguous_symbols: Vec<String> = Vec::new();
            let mut unreached_by: Option<String> = None;
            let mut candidate_root: Option<String> = None;
            let (mut method, trait_ambiguous) = if inherent.is_some() {
                (inherent, false)
            } else if !has_field {
                let mut inapplicable = self
                    .type_table
                    .symbols_not_applying(&typed_object.ty, member);
                inapplicable.extend(self.type_table.symbols_outranked(&typed_object.ty, member));
                inapplicable.extend(off_instantiation.iter().cloned());
                let candidates: Vec<_> = self
                    .visible_trait_methods(&name, member)
                    .into_iter()
                    .filter(|candidate| candidate.has_self)
                    .filter(|candidate| !inapplicable.contains(&candidate.symbol))
                    .collect();
                // the receiver's type may still hold inference variables here, so
                for supplier in self.type_table.traits_supplying(&name, member) {
                    self.bound_residuals.push(crate::infer::BoundResidual {
                        ty: typed_object.ty.clone(),
                        trait_name: supplier,
                        trait_args: Vec::new(),
                        span: _span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method call".to_string(),
                        ),
                        nominal_only: true,
                    });
                }
                let symbols: Vec<String> = candidates
                    .iter()
                    .map(|candidate| candidate.symbol.clone())
                    .collect();
                candidate_root = self.type_table.specialization_root(&name, member, &symbols);
                match candidates.as_slice() {
                    [candidate] => (
                        Some(crate::types::StructMethod {
                            name: candidate.name.clone(),
                            symbol: candidate.symbol.clone(),
                            params: candidate.params.clone(),
                            return_type: candidate.return_type.clone(),
                            has_self: candidate.has_self,
                            mutable_self: candidate.mutable_self,
                            own_type_params: candidate.own_type_params.clone(),
                        }),
                        false,
                    ),
                    // no impl that supplies the method reaches the receiver, so no instance of the call can succeed
                    [] => {
                        // only a trait the call can see: one it cannot answers as an absent member would
                        let visible: Vec<String> = self
                            .visible_trait_methods(&name, member)
                            .into_iter()
                            .filter(|candidate| candidate.has_self)
                            .map(|candidate| candidate.symbol)
                            .collect();
                        let mut traits: Vec<String> = self
                            .type_table
                            .trait_impl_defs()
                            .iter()
                            .filter(|definition| {
                                definition
                                    .methods
                                    .iter()
                                    .any(|entry| visible.contains(&entry.symbol))
                            })
                            .map(|definition| definition.trait_name.clone())
                            .collect();
                        traits.sort();
                        traits.dedup();
                        if settled && let [trait_name] = traits.as_slice() {
                            unreached_by = Some(trait_name.clone());
                        }
                        (None, false)
                    }
                    _ => match candidate_root.clone() {
                        Some(root) => (
                            candidates
                                .iter()
                                .find(|candidate| candidate.symbol == root)
                                .map(|candidate| crate::types::StructMethod {
                                    name: candidate.name.clone(),
                                    symbol: candidate.symbol.clone(),
                                    params: candidate.params.clone(),
                                    return_type: candidate.return_type.clone(),
                                    has_self: candidate.has_self,
                                    mutable_self: candidate.mutable_self,
                                    own_type_params: candidate.own_type_params.clone(),
                                }),
                            false,
                        ),
                        // a trait without parameters is typed through its declaration, its impl chosen once the receiver is known
                        None if typed_through_declaration(&self.type_table, &name, member) => (
                            candidates
                                .first()
                                .map(|candidate| crate::types::StructMethod {
                                    name: candidate.name.clone(),
                                    symbol: candidate.symbol.clone(),
                                    params: candidate.params.clone(),
                                    return_type: candidate.return_type.clone(),
                                    has_self: candidate.has_self,
                                    mutable_self: candidate.mutable_self,
                                    own_type_params: candidate.own_type_params.clone(),
                                }),
                            false,
                        ),
                        None => {
                            ambiguous_symbols =
                                candidates.iter().map(|c| c.symbol.clone()).collect();
                            (None, true)
                        }
                    },
                }
            } else {
                (None, false)
            };
            // the root of a specialization chain covers every receiver the chain covers, so its header binds the receiver whichever member runs
            let chosen_is_root = method
                .as_ref()
                .is_some_and(|chosen| candidate_root.as_deref() == Some(chosen.symbol.as_str()));
            if trait_ambiguous {
                self.errors.push(TypeError {
                    kind: crate::infer::signatures::ambiguous_trait_method_kind(
                        &self.type_table,
                        &name,
                        member,
                        &ambiguous_symbols,
                    ),
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
            if let Some(trait_name) = unreached_by {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnsatisfiedTraitBound {
                        trait_name,
                        trait_args: Vec::new(),
                        ty: typed_object.ty.clone(),
                        denied: false,
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
                // among several impls none speaks for the call
                let mut substitutions = substitutions;
                let supplying = self.type_table.traits_supplying(&name, member);
                let mut typed_by_trait = false;
                if !header_decides
                    && let [trait_name] = supplying.as_slice()
                    && let Some((trait_params, declared)) = self
                        .type_table
                        .get_trait(trait_name)
                        .filter(|definition| definition.type_params.is_empty() || chosen_is_root)
                        .and_then(|definition| {
                            definition
                                .methods
                                .iter()
                                .find(|declared| declared.name == member && declared.has_self)
                                .map(|declared| (definition.type_params.clone(), declared.clone()))
                        })
                {
                    let mut at_receiver =
                        HashMap::from([("Self".to_string(), typed_object.ty.clone())]);
                    // a specialization gives the trait its root's arguments, so the root's header fixes them
                    if !trait_params.is_empty()
                        && let Some(root) = self
                            .type_table
                            .trait_impl_defs()
                            .iter()
                            .find(|definition| {
                                definition
                                    .methods
                                    .iter()
                                    .any(|candidate| candidate.symbol == method.symbol)
                            })
                            .cloned()
                    {
                        let mut names = Vec::new();
                        let mut seen = std::collections::HashSet::new();
                        super::call::collect_generic_params(&root.self_type, &mut names, &mut seen);
                        let fresh: HashMap<String, InferType> = names
                            .into_iter()
                            .map(|name| (name, self.type_gen.fresh()))
                            .collect();
                        self.constraints.push(Constraint::equal(
                            root.self_type.substitute_params(&fresh),
                            typed_object.ty.clone(),
                            _span,
                            ConstraintReason::Other("method receiver".to_string()),
                        ));
                        for (param, arg) in trait_params.iter().zip(&root.trait_args) {
                            at_receiver.insert(param.clone(), arg.substitute_params(&fresh));
                        }
                    }
                    let apart = own_params_apart(&declared.own_type_params, &typed_object.ty);
                    method.params = declared
                        .params
                        .iter()
                        .map(|param| {
                            param
                                .substitute_params(&apart)
                                .substitute_params(&at_receiver)
                        })
                        .collect();
                    method.return_type = declared
                        .return_type
                        .substitute_params(&apart)
                        .substitute_params(&at_receiver);
                    method.own_type_params = respelled_own(&declared.own_type_params, &apart);
                    substitutions = HashMap::new();
                    typed_by_trait = true;
                }
                // the signature is written in the impl's names, which need not be the nominal's
                if (header_decides || chosen_by_receiver || chosen_is_root)
                    && !typed_by_trait
                    && method.has_self
                    && let Some(header @ InferType::Applied { .. }) = method.params.first()
                {
                    let mut names = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    super::call::collect_generic_params(header, &mut names, &mut seen);
                    let fresh: HashMap<String, InferType> = names
                        .into_iter()
                        .map(|name| (name, self.type_gen.fresh()))
                        .collect();
                    self.constraints.push(Constraint::equal(
                        header.substitute_params(&fresh),
                        typed_object.ty.clone(),
                        _span,
                        ConstraintReason::Other("method receiver".to_string()),
                    ));
                    substitutions = fresh;
                }
                let call_order = own_params_in_call_order(
                    &method.params,
                    &method.return_type,
                    method.has_self,
                    &method.own_type_params,
                );
                for parameter in &method.own_type_params {
                    substitutions
                        .entry(parameter.clone())
                        .or_insert_with(|| self.type_gen.fresh());
                }
                self.freshened_own_params = Some(
                    call_order
                        .iter()
                        .filter_map(|parameter| substitutions.get(parameter).cloned())
                        .collect(),
                );
                method.params = method
                    .params
                    .iter()
                    .map(|param| param.substitute_params(&substitutions))
                    .collect();
                method.return_type = method.return_type.substitute_params(&substitutions);
                // a projection under a constructor, as `w<Self::Out>`, is read here: the solver only answers one standing alone
                let reason = ConstraintReason::Return {
                    func_name: member.to_string(),
                };
                let params = method
                    .params
                    .into_iter()
                    .skip(1)
                    .map(|param| self.normalize_projection_types(&param, _span, &reason))
                    .collect();
                let return_type =
                    self.normalize_projection_types(&method.return_type, _span, &reason);
                return (
                    TypedExprKind::StructMethod {
                        object: Box::new(typed_object),
                        symbol: method.symbol,
                        method: member.to_string(),
                        separator,
                    },
                    InferType::Function {
                        params,
                        ret: Box::new(return_type),
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
                    } else if let Some(gated) = self.supertrait_gate_error(&name, member, _span) {
                        self.errors.push(gated);
                        InferType::Poison
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
        if let Some(error) = self.private_nominal_error(name, _span) {
            self.errors.push(error);
            return (TypedExprKind::Null, InferType::Poison);
        }
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
        // the same traversal the obligation check runs, and the same identity: a
        while let Some((trait_name, trait_args)) = pending.pop() {
            if !seen.insert(crate::types::instantiation_key(&trait_name, &trait_args)) {
                continue;
            }
            if let Some(definition) = self.type_table.get_trait(&trait_name) {
                let mut substitution = crate::unify::Substitution::new();
                for (param, ty) in definition.type_params.iter().zip(&trait_args) {
                    substitution.bind_param(param.clone(), ty.clone());
                }
                for (super_name, super_args) in &definition.super_bounds {
                    pending.push((
                        super_name.clone(),
                        super_args
                            .iter()
                            .map(|arg| substitution.apply(arg))
                            .collect(),
                    ));
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
            let first = candidates[0].0.clone();
            let one_trait = candidates.iter().all(|(name, ..)| *name == first);
            let kind = if one_trait {
                TypeErrorKind::AmbiguousTraitInstantiation {
                    target: param.to_string(),
                    method: member.to_string(),
                    trait_name: first.clone(),
                    instantiations: {
                        // supertraits were written in must not reach it
                        let mut out: Vec<String> = candidates
                            .iter()
                            .map(|(name, _, _, args)| {
                                crate::types::trait_instantiation_spelling(name, args)
                            })
                            .collect();
                        out.sort();
                        out
                    },
                }
            } else {
                TypeErrorKind::AmbiguousTraitMethod {
                    target: param.to_string(),
                    method: member.to_string(),
                }
            };
            self.errors.push(TypeError {
                kind,
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
        // the trait's parameters are fixed by the bound and rigid here; a method
        let call_order = own_params_in_call_order(
            &method.params,
            &method.return_type,
            method.has_self,
            &method.own_type_params,
        );
        for parameter in &method.own_type_params {
            substitutions
                .entry(parameter.clone())
                .or_insert_with(|| self.type_gen.fresh());
        }
        self.freshened_own_params = Some(
            call_order
                .iter()
                .filter_map(|parameter| substitutions.get(parameter).cloned())
                .collect(),
        );
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
    receiver_is_trait: bool,
    non_integer: Option<String>,
) -> crate::constraint::ProjectionFailure {
    use crate::constraint::ProjectionFailure;
    use crate::infer::ConstResolution;
    match failure {
        ConstResolution::Ambiguous(names) if receiver_is_trait => {
            ProjectionFailure::AmbiguousImplementors {
                types: names.clone(),
            }
        }
        ConstResolution::Ambiguous(traits) => ProjectionFailure::Ambiguous {
            traits: traits.clone(),
        },
        ConstResolution::AmbiguousInstantiations {
            trait_name,
            constructor,
            instantiations,
        } => ProjectionFailure::AmbiguousInstantiations {
            trait_name: trait_name.clone(),
            constructor: constructor.clone(),
            instantiations: instantiations.clone(),
        },
        ConstResolution::NotComputable => ProjectionFailure::NotComputable,
        ConstResolution::NotConstant => ProjectionFailure::NotConstant {
            declared: non_integer,
        },
        ConstResolution::Cyclic => ProjectionFailure::Cyclic {
            path: vec![format!("{receiver}::{item}")],
            namespace: crate::constraint::ItemNamespace::Const,
        },
        ConstResolution::Missing | ConstResolution::Value(_) => ProjectionFailure::NoImpl,
    }
}

/// a method's own parameters under names the receiver does not use
pub(crate) fn own_params_apart(own: &[String], receiver: &InferType) -> HashMap<String, InferType> {
    own.iter()
        .map(|name| {
            let mut spelled = crate::infer::shadowed_method_param(name);
            while receiver.mentions_param(&spelled) {
                spelled = crate::infer::shadowed_method_param(&spelled);
            }
            (name.clone(), InferType::Param(spelled))
        })
        .collect()
}

pub(crate) fn respelled_own(own: &[String], apart: &HashMap<String, InferType>) -> Vec<String> {
    own.iter()
        .map(|name| match apart.get(name) {
            Some(InferType::Param(spelled)) => spelled.clone(),
            _ => name.clone(),
        })
        .collect()
}

/// a turbofish on a method binds its own parameters in the order they first appear after the receiver, which is the order the trait's
fn own_params_in_call_order(
    params: &[InferType],
    ret: &InferType,
    has_self: bool,
    own: &[String],
) -> Vec<String> {
    let callee = InferType::Function {
        params: params.iter().skip(usize::from(has_self)).cloned().collect(),
        ret: Box::new(ret.clone()),
    };
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    super::call::collect_generic_params(&callee, &mut names, &mut seen);
    names.retain(|name| own.contains(name));
    for name in own {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

/// one trait without parameters supplies the method, so its declaration types any call to it whichever impl runs
pub(crate) fn typed_through_declaration(
    type_table: &crate::types::TypeTable,
    nominal: &str,
    member: &str,
) -> bool {
    matches!(
        type_table.traits_supplying(nominal, member).as_slice(),
        [trait_name] if type_table
            .get_trait(trait_name)
            .is_some_and(|definition| definition.type_params.is_empty())
    )
}
