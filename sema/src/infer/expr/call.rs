use super::super::functions::trait_method_symbol;
use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedFmtStringPart};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, Span};
use std::collections::{HashMap, HashSet};

fn call_path_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.clone()),
        ExprKind::GenericApply { callee, .. } => call_path_name(callee),
        ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } => {
            let mut path = call_path_name(object)?;
            path.push_str("::");
            path.push_str(member);
            Some(path)
        }
        _ => None,
    }
}

impl TypeInference {
    pub(super) fn infer_generic_apply(
        &mut self,
        callee: &Expr,
        type_args: &[aelys_syntax::TypeAnnotation],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_callee = self.infer_expr(callee);
        let names = match call_path_name(callee)
            .and_then(|name| self.function_type_params.get(&name).cloned())
        {
            Some(declared) => declared,
            None => {
                let mut names = Vec::new();
                let mut seen = HashSet::new();
                collect_generic_params(&typed_callee.ty, &mut names, &mut seen);
                names
            }
        };
        if names.len() != type_args.len() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::GenericArityMismatch {
                    name: generic_callee_name(callee),
                    expected: names.len(),
                    found: type_args.len(),
                },
                span,
                reason: ConstraintReason::Other("explicit generic arguments".to_string()),
            });
        }
        let explicit_types = type_args
            .iter()
            .map(|arg| self.type_from_annotation(arg))
            .collect::<Vec<_>>();
        let substitutions = names
            .into_iter()
            .zip(explicit_types)
            .collect::<HashMap<_, _>>();
        let ty = substitute_generic_params(&typed_callee.ty, &substitutions);
        (typed_callee.kind, ty)
    }

    pub(super) fn infer_call_expr(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        self.with_borrow_call_scope(|this| this.infer_call_expr_inner(callee, args, span))
    }

    fn infer_call_expr_inner(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        // a '.' on a namespace has its own diagnostic, so the speculative method handlers must not run
        let dot_on_namespace = matches!(
            &callee.kind,
            ExprKind::Member {
                object,
                separator: aelys_syntax::MemberSeparator::Dot,
                ..
            } if call_path_name(object).is_some_and(|path| self.env.is_namespace(&path))
        );

        if !dot_on_namespace {
            if let Some(result) = self.infer_sum_constructor_call(callee, args) {
                return result;
            }

            if let Some(result) = self.infer_enum_constructor_call(callee, args) {
                return result;
            }

            if let Some(result) = self.infer_qualified_trait_call(callee, args, span) {
                return result;
            }

            if let Some(result) = self.infer_sum_method_call(callee, args, span) {
                return result;
            }

            if let Some(result) = self.infer_string_method_call(callee, args) {
                return result;
            }

            if let Some(result) = self.infer_collection_method_call(callee, args) {
                return result;
            }
        }

        self.callee_position = true;
        let inferred_callee = self.infer_expr(callee);
        let native_obligations = self.native_obligations_for(callee, &inferred_callee.ty);
        let typed_callee = self.instantiate_generic_signature(inferred_callee);
        let typed_callee = self.specialize_numeric_signature(typed_callee);
        let reference_modes = self.reference_modes_for_call(callee, &typed_callee);
        if let ExprKind::Member {
            object,
            separator: aelys_syntax::MemberSeparator::Dot,
            ..
        } = &callee.kind
            && let Some(reference) = reference_modes.first().copied().flatten()
        {
            self.register_receiver_borrow(object, reference, callee.span);
        }
        let mut typed_args: Vec<TypedExpr> = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let expected = if matches!(&typed_callee.kind, TypedExprKind::StructMethod { .. }) {
                    reference_modes.get(index + 1).copied().flatten()
                } else {
                    reference_modes.get(index).copied().flatten()
                };
                self.infer_call_argument(arg, expected)
            })
            .collect();
        let argument_indices = effective_argument_indices(&typed_args);
        self.record_native_obligations(
            &native_obligations,
            &typed_callee.ty,
            &typed_args,
            &argument_indices,
            span,
        );

        let ret_type = if let InferType::UntypedNative(name) = &typed_callee.ty {
            InferType::UntypedNative(name.clone())
        } else if matches!(typed_callee.ty, InferType::Poison) {
            InferType::Poison
        } else if matches!(typed_callee.ty, InferType::Dynamic) {
            InferType::Dynamic
        } else {
            if let InferType::Function { params, .. } = &typed_callee.ty
                && params.len() == argument_indices.len()
            {
                for (index, param_ty) in argument_indices.iter().zip(params.iter()) {
                    let arg = &mut typed_args[*index];
                    let reason = ConstraintReason::Argument {
                        func_name: "typed function".to_string(),
                        arg_index: 0,
                    };
                    if self.reject_dynamic(&arg.ty, param_ty, arg.span, reason.clone()) {
                    } else if let InferType::UntypedNative(name) = &arg.ty {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UntypedNativeBoundary { name: name.clone() },
                            span: arg.span,
                            reason: reason.clone(),
                        });
                    }
                    if let TypedExprKind::Int(value) = &arg.kind
                        && param_ty.is_integer()
                        && *param_ty != InferType::I64
                    {
                        if InferType::int_fits(*value, param_ty) {
                            arg.ty = param_ty.clone();
                        } else {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::Mismatch {
                                    expected: param_ty.clone(),
                                    found: InferType::I64,
                                },
                                span: arg.span,
                                reason: ConstraintReason::IntLiteralOverflow {
                                    value: *value,
                                    target: param_ty.clone(),
                                },
                            });
                        }
                    }
                    if let TypedExprKind::Float(value) = &arg.kind
                        && *param_ty == InferType::F32
                    {
                        if value.is_finite() && value.abs() <= f32::MAX as f64 {
                            arg.ty = InferType::F32;
                        } else {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::Mismatch {
                                    expected: InferType::F32,
                                    found: InferType::F64,
                                },
                                span: arg.span,
                                reason: reason.clone(),
                            });
                        }
                    }
                }
            }

            let ret = match &typed_callee.ty {
                InferType::Function { ret, .. } => ret.as_ref().clone(),
                _ => self.type_gen.fresh(),
            };

            let arg_types: Vec<InferType> = argument_indices
                .iter()
                .map(|index| typed_args[*index].ty.clone())
                .collect();
            let expected_fn_type = InferType::Function {
                params: arg_types,
                ret: Box::new(ret.clone()),
            };

            self.constraints.push(Constraint::equal(
                typed_callee.ty.clone(),
                expected_fn_type,
                span,
                ConstraintReason::Other("function call".to_string()),
            ));

            ret
        };

        let callee_ty = typed_callee.ty.clone();
        self.mark_display_arguments(
            &native_obligations,
            &callee_ty,
            &mut typed_args,
            &argument_indices,
        );

        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            ret_type,
        )
    }

    fn mark_display_arguments(
        &mut self,
        obligations: &[Option<crate::native::NativeObligation>],
        callee_ty: &InferType,
        typed_args: &mut [TypedExpr],
        argument_indices: &[usize],
    ) {
        let InferType::Function { params, .. } = callee_ty else {
            return;
        };
        for (index, obligation) in obligations.iter().enumerate().take(params.len()) {
            if !matches!(
                obligation,
                Some(crate::native::NativeObligation::Bound(trait_name))
                    if *trait_name == crate::prelude::DISPLAY_TRAIT
            ) {
                continue;
            }
            let Some(slot) = argument_indices.get(index).copied() else {
                continue;
            };
            let placeholders = typed_args.get(slot).map_or(0, placeholder_count);
            if placeholders == 0 {
                if let Some(arg) = typed_args.get_mut(slot) {
                    crate::infer::monomorphize::mark_display_argument(arg);
                }
                continue;
            }
            for offset in 1..=placeholders {
                let Some(arg) = typed_args.get_mut(slot + offset) else {
                    break;
                };
                self.require_display(arg, "format string placeholder");
            }
        }
    }

    // a user declaration of the same name shadows the native and brings its own declared bounds
    fn native_obligations_for(
        &self,
        callee: &Expr,
        callee_ty: &InferType,
    ) -> Vec<Option<crate::native::NativeObligation>> {
        let Some(path) = call_path_name(callee) else {
            return Vec::new();
        };
        let bare = path
            .rsplit("::")
            .next()
            .unwrap_or(path.as_str())
            .to_string();
        for candidate in [path, bare] {
            let obligations = crate::native::parameter_obligations(&candidate);
            if obligations.is_empty() || self.function_type_params.contains_key(&candidate) {
                continue;
            }
            let resolved = crate::native::builtin_signature(&candidate)
                .or_else(|| self.known_native_signatures.get(&candidate).cloned())
                .or_else(|| crate::native::function_signature(&candidate));
            if resolved.as_ref() == Some(callee_ty) {
                return obligations;
            }
        }
        Vec::new()
    }

    fn record_native_obligations(
        &mut self,
        obligations: &[Option<crate::native::NativeObligation>],
        callee_ty: &InferType,
        typed_args: &[TypedExpr],
        argument_indices: &[usize],
        span: Span,
    ) {
        if obligations.is_empty() {
            return;
        }
        let InferType::Function { params, .. } = callee_ty else {
            return;
        };
        for (index, (param, obligation)) in params.iter().zip(obligations).enumerate() {
            let Some(obligation) = obligation else {
                continue;
            };
            let span = argument_indices
                .get(index)
                .and_then(|index| typed_args.get(*index))
                .map_or(span, |arg| arg.span);
            match obligation {
                crate::native::NativeObligation::Bound(trait_name) => {
                    self.bound_residuals.push(crate::infer::BoundResidual {
                        ty: param.clone(),
                        trait_name: (*trait_name).to_string(),
                        trait_args: Vec::new(),
                        span,
                        reason: ConstraintReason::Other("native parameter bound".to_string()),
                        nominal_only: false,
                    });
                }
                crate::native::NativeObligation::OneOf(options) => {
                    self.constraints.push(Constraint::one_of(
                        param.clone(),
                        options.clone(),
                        span,
                        ConstraintReason::Other("native conversion source".to_string()),
                    ));
                }
            }
        }
    }

    fn infer_enum_constructor_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } = &callee.kind
        else {
            return None;
        };
        let (enum_name, explicit_type_args) = match &object.kind {
            ExprKind::Identifier(enum_name) => (enum_name.clone(), Vec::new()),
            ExprKind::GenericApply { callee, type_args } => {
                let ExprKind::Identifier(enum_name) = &callee.kind else {
                    return None;
                };
                (
                    enum_name.clone(),
                    type_args
                        .iter()
                        .map(|arg| self.type_from_annotation(arg))
                        .collect(),
                )
            }
            _ => return None,
        };
        if let Some(error) = self.private_nominal_error(&enum_name, callee.span) {
            self.errors.push(error);
            return Some((TypedExprKind::Null, InferType::Poison));
        }
        let enum_def = self.type_table.get_enum(&enum_name).cloned()?;
        if !explicit_type_args.is_empty() && explicit_type_args.len() != enum_def.type_params.len()
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::GenericArityMismatch {
                    name: enum_name.clone(),
                    expected: enum_def.type_params.len(),
                    found: explicit_type_args.len(),
                },
                span: callee.span,
                reason: ConstraintReason::Other("enum type arguments".to_string()),
            });
        }
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
        let (variant_index, variant) = enum_def
            .variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.name == *member)?;
        let fields = match &variant.fields {
            crate::types::EnumVariantFieldsDef::Unit => {
                if !args.is_empty() {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidSumMethod {
                            method: format!("{enum_name}::{member}"),
                            receiver: InferType::Struct(enum_name.clone()),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("enum constructor arity".to_string()),
                    });
                }
                Vec::new()
            }
            crate::types::EnumVariantFieldsDef::Tuple(expected) => {
                if expected.len() != args.len() {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidSumMethod {
                            method: format!("{enum_name}::{member}"),
                            receiver: InferType::Struct(enum_name.clone()),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("enum constructor arity".to_string()),
                    });
                }
                args.iter()
                    .zip(expected.iter())
                    .map(|(arg, expected)| {
                        let expected = expected.substitute_params(&substitutions);
                        let typed = self.infer_expr(arg);
                        if !self.reject_dynamic(
                            &typed.ty,
                            &expected,
                            typed.span,
                            ConstraintReason::Other("enum constructor field".to_string()),
                        ) {
                            self.constraints.push(Constraint::equal(
                                expected.clone(),
                                typed.ty.clone(),
                                typed.span,
                                ConstraintReason::Other("enum constructor field".to_string()),
                            ));
                        }
                        (None, Box::new(typed))
                    })
                    .collect()
            }
            crate::types::EnumVariantFieldsDef::Named(_) => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidSumMethod {
                        method: format!("{enum_name}::{member}"),
                        receiver: InferType::Struct(enum_name.clone()),
                    },
                    span: callee.span,
                    reason: ConstraintReason::Other(
                        "named enum constructor requires fields".to_string(),
                    ),
                });
                Vec::new()
            }
        };
        Some((
            TypedExprKind::EnumConstruct {
                enum_name: enum_name.clone(),
                variant: member.clone(),
                schema_index: self.type_table.enum_schema_index(&enum_name).unwrap_or(0),
                variant_index: u16::try_from(variant_index).unwrap_or(0),
                fields,
            },
            enum_type,
        ))
    }

    fn infer_qualified_trait_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } = &callee.kind
        else {
            return None;
        };
        let trait_name = call_path_name(object)?;
        let trait_def = self.type_table.get_trait(&trait_name).cloned()?;
        if let Some(error) = self.private_nominal_error(&trait_name, object.span) {
            self.errors.push(error);
            return Some((TypedExprKind::Null, InferType::Poison));
        }
        let required = trait_def
            .methods
            .iter()
            .find(|method| method.name == *member)?;
        let receiver = args.first()?;
        let typed_receiver = self.infer_expr(receiver);
        let target = match &typed_receiver.ty {
            InferType::Struct(name) | InferType::Applied { name, .. } => name.clone(),
            _ => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidStructMethod {
                        method: member.clone(),
                        structure: typed_receiver.ty.to_string(),
                    },
                    span,
                    reason: ConstraintReason::Other("qualified trait receiver".to_string()),
                });
                return Some((
                    TypedExprKind::Member {
                        object: Box::new(TypedExpr::new(
                            TypedExprKind::Identifier(trait_name),
                            InferType::Poison,
                            object.span,
                        )),
                        member: member.clone(),
                        separator: aelys_syntax::MemberSeparator::Path,
                    },
                    InferType::Poison,
                ));
            }
        };
        if !self.type_table.has_trait_impl(&trait_name, &target) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnsatisfiedTraitBound {
                    trait_name: trait_name.clone(),
                    ty: typed_receiver.ty.clone(),
                },
                span,
                reason: ConstraintReason::Other("qualified trait method".to_string()),
            });
            return Some((
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier(trait_name),
                        InferType::Poison,
                        object.span,
                    )),
                    member: member.clone(),
                    separator: aelys_syntax::MemberSeparator::Path,
                },
                InferType::Poison,
            ));
        }
        let trait_args = self.type_table.sole_trait_impl_args(&trait_name, &target);
        let symbol = trait_method_symbol(&trait_name, &target, member, &trait_args);
        let method = self
            .type_table
            .trait_methods(&target, member)
            .iter()
            .find(|candidate| candidate.symbol == symbol)
            .cloned()
            .unwrap_or_else(|| crate::types::TraitMethod {
                name: required.name.clone(),
                symbol: symbol.clone(),
                params: required
                    .params
                    .iter()
                    .map(|param| {
                        if matches!(param, InferType::Param(name) if name == "Self") {
                            typed_receiver.ty.clone()
                        } else {
                            param.clone()
                        }
                    })
                    .collect(),
                return_type: required.return_type.clone(),
                has_self: required.has_self,
                mutable_self: required.mutable_self,
                has_body: required.has_body,
            });
        let mut typed_args = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            let typed = if index == 0 {
                typed_receiver.clone()
            } else {
                self.infer_expr(arg)
            };
            if let Some(expected) = method.params.get(index)
                && !self.reject_dynamic(
                    &typed.ty,
                    expected,
                    typed.span,
                    ConstraintReason::Argument {
                        func_name: symbol.clone(),
                        arg_index: index,
                    },
                )
            {
                self.constraints.push(Constraint::equal(
                    expected.clone(),
                    typed.ty.clone(),
                    typed.span,
                    ConstraintReason::Argument {
                        func_name: symbol.clone(),
                        arg_index: index,
                    },
                ));
            }
            typed_args.push(typed);
        }
        if method.params.len() != typed_args.len() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ArityMismatch {
                    expected: method.params.len(),
                    found: typed_args.len(),
                },
                span,
                reason: ConstraintReason::Other("qualified trait method call".to_string()),
            });
        }
        let method_type = InferType::Function {
            params: method.params.clone(),
            ret: Box::new(method.return_type.clone()),
        };
        let typed_callee = TypedExpr::new(
            TypedExprKind::StructMethod {
                object: Box::new(TypedExpr::new(
                    TypedExprKind::Identifier(trait_name),
                    InferType::Poison,
                    object.span,
                )),
                symbol,
                method: member.clone(),
                separator: aelys_syntax::MemberSeparator::Path,
            },
            method_type,
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            method.return_type,
        ))
    }

    fn infer_string_method_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Dot,
        } = &callee.kind
        else {
            return None;
        };
        let collection_pipeline = matches!(
            member.as_str(),
            "map" | "filter" | "fold" | "iter" | "collect"
        );
        let previous_iterator_context = self.collection_iter_allowed;
        if collection_pipeline {
            self.collection_iter_allowed = true;
        }
        let typed_object = self.infer_expr(object);
        self.collection_iter_allowed = previous_iterator_context;
        let signature = crate::native::function_signature(&format!("string::{member}"))?;
        let InferType::Function { params, ret } = signature else {
            return None;
        };
        let receiver = params.first()?;
        if !matches!(receiver, InferType::String) || params.len() != args.len() + 1 {
            return None;
        }

        if matches!(member.as_str(), "len" | "is_empty")
            && matches!(
                &typed_object.ty,
                InferType::Array(_)
                    | InferType::FixedArray(_, _)
                    | InferType::Vec(_)
                    | InferType::Var(_)
            )
        {
            return None;
        }

        let valid_receiver = match &typed_object.ty {
            InferType::String => true,
            InferType::Var(_) => {
                self.constraints.push(Constraint::one_of(
                    typed_object.ty.clone(),
                    vec![InferType::String],
                    object.span,
                    ConstraintReason::Other("string method receiver".to_string()),
                ));
                true
            }
            _ => false,
        };
        let expected_args = params[1..].to_vec();
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();

        if !valid_receiver {
            self.errors.push(TypeError {
                kind: TypeErrorKind::InvalidStringMethod {
                    method: member.clone(),
                    receiver: typed_object.ty.clone(),
                },
                span: object.span,
                reason: ConstraintReason::Other("string method receiver".to_string()),
            });
            let typed_callee = TypedExpr::new(
                TypedExprKind::Member {
                    object: Box::new(typed_object),
                    member: member.clone(),
                    separator: aelys_syntax::MemberSeparator::Dot,
                },
                InferType::Function {
                    params: vec![InferType::Poison; typed_args.len()],
                    ret: Box::new(InferType::Poison),
                },
                callee.span,
            );
            return Some((
                TypedExprKind::Call {
                    callee: Box::new(typed_callee),
                    args: typed_args,
                },
                InferType::Poison,
            ));
        }

        for (index, (arg, expected)) in typed_args.iter().zip(expected_args.iter()).enumerate() {
            let reason = ConstraintReason::Argument {
                func_name: format!("string::{member}"),
                arg_index: index,
            };
            if self.reject_dynamic(&arg.ty, expected, arg.span, reason.clone()) {
                continue;
            }
            if let InferType::UntypedNative(name) = &arg.ty {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UntypedNativeBoundary { name: name.clone() },
                    span: arg.span,
                    reason,
                });
                continue;
            }
            self.constraints.push(Constraint::equal(
                expected.clone(),
                arg.ty.clone(),
                arg.span,
                reason,
            ));
        }

        let output = ret.as_ref().clone();
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.clone(),
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            InferType::Function {
                params: expected_args,
                ret: Box::new(output.clone()),
            },
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    fn specialize_numeric_signature(&mut self, callee: TypedExpr) -> TypedExpr {
        let InferType::Function { params, ret } = &callee.ty else {
            return callee;
        };
        if !params.iter().any(contains_numeric) && !contains_numeric(ret) {
            return callee;
        }

        let numeric = self.type_gen.fresh();
        self.constraints.push(Constraint::one_of(
            numeric.clone(),
            InferType::all_numeric_types(),
            callee.span,
            ConstraintReason::Other("numeric function signature".to_string()),
        ));
        let specialized = InferType::Function {
            params: params
                .iter()
                .map(|param| replace_numeric(param, &numeric))
                .collect(),
            ret: Box::new(replace_numeric(ret, &numeric)),
        };
        TypedExpr::new(callee.kind, specialized, callee.span)
    }

    fn instantiate_generic_signature(&mut self, callee: TypedExpr) -> TypedExpr {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        collect_generic_params(&callee.ty, &mut names, &mut seen);
        if names.is_empty() {
            return callee;
        }

        let substitutions = names
            .into_iter()
            .map(|name| (name, self.type_gen.fresh()))
            .collect::<HashMap<_, _>>();
        let ty = substitute_generic_params(&callee.ty, &substitutions);
        TypedExpr::new(callee.kind, ty, callee.span)
    }

    fn infer_collection_method_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Dot,
        } = &callee.kind
        else {
            return None;
        };
        let consumes_iterator = matches!(member.as_str(), "map" | "filter" | "fold" | "collect");
        let previous_iterator_context = self.collection_iter_allowed;
        if consumes_iterator {
            self.collection_iter_allowed = true;
        }
        let typed_object = self.infer_expr(object);
        self.collection_iter_allowed = previous_iterator_context;
        if member == "iter" && !previous_iterator_context {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnconsumedCollectionIterator,
                span: callee.span,
                reason: ConstraintReason::Other("iterator consumption".to_string()),
            });
        }
        if member == "collect"
            && matches!(
                &typed_object.ty,
                InferType::Array(_) | InferType::FixedArray(_, _) | InferType::Vec(_)
            )
            && !is_pipeline_source(object)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::CollectionCollectRequiresPipeline,
                span: callee.span,
                reason: ConstraintReason::Other("collection pipeline terminal".to_string()),
            });
        }
        if is_collection_method(member, args.len())
            && matches!(&typed_object.ty, InferType::Dynamic)
        {
            return Some(self.invalid_collection_method(typed_object, member, args, callee));
        }
        let (element, is_vec) = match &typed_object.ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) => {
                (inner.as_ref().clone(), false)
            }
            InferType::Vec(inner) => (inner.as_ref().clone(), true),
            InferType::Var(_) => {
                let element = self.type_gen.fresh();
                let (options, is_vec) = match member.as_str() {
                    "len" | "is_empty" => (
                        vec![
                            InferType::String,
                            InferType::Array(Box::new(element.clone())),
                            InferType::Vec(Box::new(element.clone())),
                        ],
                        false,
                    ),
                    "get" => (
                        vec![
                            InferType::Array(Box::new(element.clone())),
                            InferType::Vec(Box::new(element.clone())),
                        ],
                        false,
                    ),
                    "capacity" | "pop" | "push" | "reserve" => {
                        (vec![InferType::Vec(Box::new(element.clone()))], true)
                    }
                    "map" | "filter" | "fold" | "iter" | "collect" => return None,
                    _ => return None,
                };
                self.constraints.push(Constraint::one_of(
                    typed_object.ty.clone(),
                    options,
                    object.span,
                    ConstraintReason::CollectionMethodReceiver {
                        method: member.clone(),
                    },
                ));
                (element, is_vec)
            }
            _ if is_collection_method(member, args.len()) => {
                return Some(self.invalid_collection_method(typed_object, member, args, callee));
            }
            _ => return None,
        };
        if matches!(member.as_str(), "push" | "pop" | "reserve") {
            let binding = mutable_collection_binding(object);
            if binding.is_some_and(|name| self.env.is_read_only(name)) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::ReadOnlyCollectionRequired {
                        method: member.clone(),
                        receiver: typed_object.ty.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::Other("read-only collection receiver".to_string()),
                });
            } else if binding.is_none_or(|name| !self.env.is_mutable(name)) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::MutableCollectionRequired {
                        method: member.clone(),
                        receiver: typed_object.ty.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::Other("mutable collection receiver".to_string()),
                });
            }
            if let Some(name) = binding
                && matches!(&object.kind, ExprKind::Identifier(_))
            {
                self.env.invalidate_collection_length(name);
            }
        }
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        let (output, expected_args) = match (member.as_str(), is_vec, args.len()) {
            ("len", _, 0) | ("capacity", true, 0) => (InferType::I64, Vec::new()),
            ("is_empty", _, 0) => (InferType::Bool, Vec::new()),
            ("pop", true, 0) | ("get", _, 1) => (
                InferType::Option(Box::new(element.clone())),
                if member == "get" {
                    vec![InferType::I64]
                } else {
                    Vec::new()
                },
            ),
            ("push", true, 1) => (InferType::Unit, vec![element.clone()]),
            ("reserve", true, 1) => (InferType::Unit, vec![InferType::I64]),
            ("iter", _, 0) => (
                if is_vec {
                    InferType::Vec(Box::new(element.clone()))
                } else {
                    InferType::Array(Box::new(element.clone()))
                },
                Vec::new(),
            ),
            ("collect", _, 0) => (InferType::Vec(Box::new(element.clone())), Vec::new()),
            ("map", _, 1) => {
                let mapped = self.type_gen.fresh();
                (
                    InferType::Vec(Box::new(mapped.clone())),
                    vec![InferType::Function {
                        params: vec![element.clone()],
                        ret: Box::new(mapped),
                    }],
                )
            }
            ("filter", _, 1) => (
                InferType::Vec(Box::new(element.clone())),
                vec![InferType::Function {
                    params: vec![element.clone()],
                    ret: Box::new(InferType::Bool),
                }],
            ),
            ("fold", _, 2) => {
                let accumulator = typed_args[0].ty.clone();
                (
                    accumulator.clone(),
                    vec![
                        accumulator.clone(),
                        InferType::Function {
                            params: vec![accumulator, element.clone()],
                            ret: Box::new(typed_args[0].ty.clone()),
                        },
                    ],
                )
            }
            _ => return None,
        };
        let expected_args = match member.as_str() {
            "get" => vec![InferType::I64],
            "push" => vec![element.clone()],
            "reserve" => vec![InferType::I64],
            _ => expected_args,
        };
        for (index, (arg, expected)) in typed_args.iter().zip(expected_args.iter()).enumerate() {
            let reason = ConstraintReason::Argument {
                func_name: format!("collection::{member}"),
                arg_index: index,
            };
            if !self.reject_dynamic(&arg.ty, expected, arg.span, reason.clone())
                && !self.reject_untyped_native(&arg.ty, expected, arg.span, reason.clone())
            {
                self.constraints.push(Constraint::equal(
                    expected.clone(),
                    arg.ty.clone(),
                    arg.span,
                    reason,
                ));
            }
        }
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.clone(),
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            InferType::Function {
                params: expected_args,
                ret: Box::new(output.clone()),
            },
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    fn invalid_collection_method(
        &mut self,
        typed_object: TypedExpr,
        member: &str,
        args: &[Expr],
        callee: &Expr,
    ) -> (TypedExprKind, InferType) {
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        self.errors.push(TypeError {
            kind: TypeErrorKind::InvalidCollectionMethod {
                method: member.to_string(),
                receiver: typed_object.ty.clone(),
            },
            span: typed_object.span,
            reason: ConstraintReason::Other("collection method receiver".to_string()),
        });
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.to_string(),
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            InferType::Function {
                params: vec![InferType::Poison; typed_args.len()],
                ret: Box::new(InferType::Poison),
            },
            callee.span,
        );
        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            InferType::Poison,
        )
    }
}

fn generic_callee_name(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Identifier(name) => name.clone(),
        ExprKind::Member { .. } => "qualified function".to_string(),
        _ => "function".to_string(),
    }
}

fn collect_generic_params(ty: &InferType, names: &mut Vec<String>, seen: &mut HashSet<String>) {
    match ty {
        InferType::Param(name) => {
            if seen.insert(name.clone()) {
                names.push(name.clone());
            }
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_generic_params(param, names, seen);
            }
            collect_generic_params(ret, names, seen);
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => collect_generic_params(inner, names, seen),
        InferType::Result(ok, err) => {
            collect_generic_params(ok, names, seen);
            collect_generic_params(err, names, seen);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_generic_params(element, names, seen);
            }
        }
        InferType::Applied { args, .. } => {
            for arg in args {
                collect_generic_params(arg, names, seen);
            }
        }
        _ => {}
    }
}

fn substitute_generic_params(
    ty: &InferType,
    substitutions: &HashMap<String, InferType>,
) -> InferType {
    match ty {
        InferType::Param(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        InferType::Function { params, ret } => InferType::Function {
            params: params
                .iter()
                .map(|param| substitute_generic_params(param, substitutions))
                .collect(),
            ret: Box::new(substitute_generic_params(ret, substitutions)),
        },
        InferType::Array(inner) => {
            InferType::Array(Box::new(substitute_generic_params(inner, substitutions)))
        }
        InferType::FixedArray(inner, length) => InferType::FixedArray(
            Box::new(substitute_generic_params(inner, substitutions)),
            *length,
        ),
        InferType::Vec(inner) => {
            InferType::Vec(Box::new(substitute_generic_params(inner, substitutions)))
        }
        InferType::Option(inner) => {
            InferType::Option(Box::new(substitute_generic_params(inner, substitutions)))
        }
        InferType::Result(ok, err) => InferType::Result(
            Box::new(substitute_generic_params(ok, substitutions)),
            Box::new(substitute_generic_params(err, substitutions)),
        ),
        InferType::Tuple(elements) => InferType::Tuple(
            elements
                .iter()
                .map(|element| substitute_generic_params(element, substitutions))
                .collect(),
        ),
        InferType::Applied { name, args } => InferType::Applied {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_generic_params(arg, substitutions))
                .collect(),
        },
        InferType::Projection {
            trait_name,
            item,
            self_ty,
        } => InferType::Projection {
            trait_name: trait_name.clone(),
            item: item.clone(),
            self_ty: Box::new(substitute_generic_params(self_ty, substitutions)),
        },
        _ => ty.clone(),
    }
}

fn is_collection_method(member: &str, argument_count: usize) -> bool {
    matches!(
        (member, argument_count),
        ("len" | "is_empty" | "capacity" | "pop", 0)
            | ("iter" | "collect", 0)
            | ("push" | "get" | "reserve" | "map" | "filter", 1)
            | ("fold", 2)
    )
}

fn mutable_collection_binding(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.as_str()),
        ExprKind::Grouping(inner) => mutable_collection_binding(inner),
        ExprKind::Index { object, .. } => match &object.kind {
            ExprKind::Identifier(name) => Some(name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn is_pipeline_source(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Grouping(inner) => is_pipeline_source(inner),
        ExprKind::Call { callee, .. } => is_pipeline_source(callee),
        ExprKind::Member {
            member,
            separator: aelys_syntax::MemberSeparator::Dot,
            ..
        } => matches!(member.as_str(), "iter" | "map" | "filter"),
        _ => false,
    }
}

fn contains_numeric(ty: &InferType) -> bool {
    match ty {
        InferType::Numeric => true,
        InferType::Function { params, ret } => {
            params.iter().any(contains_numeric) || contains_numeric(ret)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => contains_numeric(inner),
        InferType::Result(ok, err) => contains_numeric(ok) || contains_numeric(err),
        InferType::Tuple(elements) => elements.iter().any(contains_numeric),
        _ => false,
    }
}

fn replace_numeric(ty: &InferType, replacement: &InferType) -> InferType {
    match ty {
        InferType::Numeric => replacement.clone(),
        InferType::Function { params, ret } => InferType::Function {
            params: params
                .iter()
                .map(|param| replace_numeric(param, replacement))
                .collect(),
            ret: Box::new(replace_numeric(ret, replacement)),
        },
        InferType::Array(inner) => InferType::Array(Box::new(replace_numeric(inner, replacement))),
        InferType::FixedArray(inner, length) => {
            InferType::FixedArray(Box::new(replace_numeric(inner, replacement)), *length)
        }
        InferType::Vec(inner) => InferType::Vec(Box::new(replace_numeric(inner, replacement))),
        InferType::Option(inner) => {
            InferType::Option(Box::new(replace_numeric(inner, replacement)))
        }
        InferType::Result(ok, err) => InferType::Result(
            Box::new(replace_numeric(ok, replacement)),
            Box::new(replace_numeric(err, replacement)),
        ),
        InferType::Tuple(elements) => InferType::Tuple(
            elements
                .iter()
                .map(|element| replace_numeric(element, replacement))
                .collect(),
        ),
        _ => ty.clone(),
    }
}

fn placeholder_count(arg: &TypedExpr) -> usize {
    let TypedExprKind::FmtString(parts) = &arg.kind else {
        return 0;
    };
    parts
        .iter()
        .filter(|part| matches!(part, TypedFmtStringPart::Placeholder))
        .count()
}

fn effective_argument_indices(args: &[TypedExpr]) -> Vec<usize> {
    let Some(TypedExpr {
        kind: TypedExprKind::FmtString(parts),
        ..
    }) = args.first()
    else {
        return (0..args.len()).collect();
    };
    let placeholders = parts
        .iter()
        .filter(|part| matches!(part, TypedFmtStringPart::Placeholder))
        .count();
    if placeholders == 0 || args.len() < placeholders + 1 {
        return (0..args.len()).collect();
    }
    std::iter::once(0)
        .chain((placeholders + 1)..args.len())
        .collect()
}
