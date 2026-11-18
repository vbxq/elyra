use super::TypeInference;
use super::expr::member::FieldVisibilityCheck;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedMatchArm, TypedMatchArmBody, TypedPattern, TypedPatternKind,
};
use crate::types::InferType;
use crate::unify::{Substitution, unify, unify_error_to_type_error};
use aelys_syntax::{Expr, ExprKind, MatchArm, MatchArmBody, Pattern, PatternKind, Span};

pub(super) struct TryResidual {
    pub source: InferType,
    pub target: InferType,
    pub span: Span,
}

pub(super) struct MustUseResidual {
    pub ty: InferType,
    pub span: Span,
}

#[derive(Clone)]
pub(super) struct SumMethodResidual {
    pub receiver: InferType,
    pub result: InferType,
    pub value: InferType,
    pub error: InferType,
    pub mapped: Option<InferType>,
    pub callback_result: Option<InferType>,
    pub callback: Option<InferType>,
    pub preferred_family: Option<SumFamily>,
    pub method: String,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SumFamily {
    Option,
    Result,
}

pub(super) struct SumTypeResidual {
    pub constructor: String,
    pub ty: InferType,
    pub span: Span,
}

pub(super) struct MatchArmCoverage {
    pub pattern: Pattern,
    pub guarded: bool,
    pub synthetic: bool,
    pub span: Span,
}

pub(super) struct MatchExhaustivityResidual {
    pub ty: InferType,
    pub arms: Vec<MatchArmCoverage>,
    pub span: Span,
}

impl TypeInference {
    pub(super) fn record_sum_type(
        &mut self,
        constructor: impl Into<String>,
        ty: InferType,
        span: Span,
    ) {
        if matches!(ty, InferType::Option(_) | InferType::Result(_, _)) {
            self.sum_type_residuals.push(SumTypeResidual {
                constructor: constructor.into(),
                ty,
                span,
            });
        }
    }

    pub(super) fn validate_sum_types(&mut self, subst: &Substitution) {
        for residual in &self.sum_type_residuals {
            let ty = subst.apply(&residual.ty);
            if sum_type_is_unresolved(&ty) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnresolvedSumType {
                        constructor: residual.constructor.clone(),
                    },
                    span: residual.span,
                    reason: ConstraintReason::Other("sum type resolution".to_string()),
                });
            }
        }
    }

    pub(super) fn validate_match_reachability(&mut self, subst: &Substitution) {
        let mut reported = Vec::new();
        for (result, spans) in &self.match_unit_residuals {
            let resolved = subst.apply(result);
            if !matches!(
                resolved,
                InferType::Unit
                    | InferType::Var(_)
                    | InferType::Dynamic
                    | InferType::Poison
                    | InferType::Never
            ) {
                for span in spans {
                    reported.push(TypeError {
                        kind: TypeErrorKind::MatchArmValueRequired,
                        span: *span,
                        reason: ConstraintReason::Other("match arm tail value".to_string()),
                    });
                }
            }
        }
        let checker = super::exhaustive::Checker::new(&self.type_table);
        for residual in &self.match_exhaustivity_residuals {
            let ty = subst.apply(&residual.ty);
            if ty.contains_poison() {
                continue;
            }
            let mut capitalized = Vec::new();
            for arm in &residual.arms {
                checker.capitalized_bindings(&arm.pattern, &ty, &mut capitalized);
            }
            let rejected_bindings = !capitalized.is_empty();
            for (name, suggestion, span) in capitalized {
                reported.push(TypeError {
                    kind: TypeErrorKind::CapitalizedBindingPattern { name, suggestion },
                    span,
                    reason: ConstraintReason::Other("match arm pattern".to_string()),
                });
            }
            if rejected_bindings {
                continue;
            }
            let arms: Vec<(&Pattern, bool)> = residual
                .arms
                .iter()
                .map(|arm| (&arm.pattern, arm.guarded))
                .collect();
            let analysis = checker.analyze(&ty, &arms);
            for index in analysis.unreachable {
                let arm = &residual.arms[index];
                if arm.synthetic {
                    continue;
                }
                reported.push(TypeError {
                    kind: TypeErrorKind::UnreachablePattern {
                        pattern: describe_pattern(&arm.pattern),
                    },
                    span: arm.span,
                    reason: ConstraintReason::Other("match arm reachability".to_string()),
                });
            }
            if analysis.missing.is_empty() {
                continue;
            }
            if let Some(structure) = checker.is_struct(&ty) {
                reported.push(TypeError {
                    kind: TypeErrorKind::NonExhaustiveStruct { structure },
                    span: residual.span,
                    reason: ConstraintReason::Other("struct match exhaustivity".to_string()),
                });
            } else {
                reported.push(TypeError {
                    kind: TypeErrorKind::NonExhaustiveMatch {
                        missing: analysis.missing,
                    },
                    span: residual.span,
                    reason: ConstraintReason::Other("match exhaustivity".to_string()),
                });
            }
        }
        self.errors.extend(reported);
    }

    pub(super) fn validate_match_exhaustivity(&mut self, _subst: &Substitution) {}

    pub(super) fn infer_sum_method_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        _span: Span,
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
        if collection_pipeline
            && matches!(
                &typed_object.ty,
                InferType::Array(_) | InferType::FixedArray(_, _) | InferType::Vec(_)
            )
        {
            return None;
        }
        if let InferType::UntypedNative(name) = &typed_object.ty
            && is_sum_method(member)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UntypedSumValue { name: name.clone() },
                span: callee.span,
                reason: ConstraintReason::Other("untyped native sum method".to_string()),
            });
            return None;
        }
        if matches!(typed_object.ty, InferType::Dynamic) && is_sum_method(member) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::DynamicSumMethod {
                    method: member.clone(),
                },
                span: callee.span,
                reason: ConstraintReason::Other("dynamic sum method".to_string()),
            });
            return None;
        }
        if !is_sum_method(member) {
            return None;
        }
        if matches!(typed_object.ty, InferType::Var(_)) {
            return Some(self.infer_inferred_sum_method_call(callee, typed_object, member, args));
        }
        let (family, value_type, error_type) = match &typed_object.ty {
            InferType::Option(value) => ("Option", value.as_ref().clone(), None),
            InferType::Result(value, error) => (
                "Result",
                value.as_ref().clone(),
                Some(error.as_ref().clone()),
            ),
            _ => return Some(self.invalid_sum_method_call(typed_object, member, args, callee)),
        };
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        let mut expected_args = Vec::new();
        let output = match member.as_str() {
            "unwrap" | "expect" => {
                if member == "expect" {
                    expected_args.push(InferType::String);
                }
                value_type.clone()
            }
            "unwrap_or" => {
                expected_args.push(value_type.clone());
                value_type.clone()
            }
            "unwrap_or_else" => {
                let params = if family == "Result" {
                    vec![error_type.clone().unwrap_or(InferType::Dynamic)]
                } else {
                    Vec::new()
                };
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(params.len()),
                    params,
                    ret: Box::new(value_type.clone()),
                });
                value_type.clone()
            }
            "is_ok" | "is_err" if family == "Result" => InferType::Bool,
            "is_some" | "is_none" if family == "Option" => InferType::Bool,
            "ok_or" if family == "Option" => {
                let error = typed_args
                    .first()
                    .map(|arg| arg.ty.clone())
                    .unwrap_or_else(|| self.type_gen.fresh());
                expected_args.push(error.clone());
                InferType::Result(Box::new(value_type.clone()), Box::new(error))
            }
            "ok" if family == "Result" => InferType::Option(Box::new(value_type.clone())),
            "err" if family == "Result" => {
                InferType::Option(Box::new(error_type.clone().unwrap_or(InferType::Dynamic)))
            }
            "map" => {
                let mapped = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![value_type.clone()],
                    ret: Box::new(mapped.clone()),
                });
                match family {
                    "Option" => InferType::Option(Box::new(mapped)),
                    _ => InferType::Result(
                        Box::new(mapped),
                        Box::new(error_type.clone().unwrap_or(InferType::Dynamic)),
                    ),
                }
            }
            "map_err" if family == "Result" => {
                let mapped = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![error_type.clone().unwrap_or(InferType::Dynamic)],
                    ret: Box::new(mapped.clone()),
                });
                InferType::Result(Box::new(value_type.clone()), Box::new(mapped))
            }
            "and_then" => {
                let mapped = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![value_type.clone()],
                    ret: Box::new(match family {
                        "Option" => InferType::Option(Box::new(mapped.clone())),
                        _ => InferType::Result(
                            Box::new(mapped.clone()),
                            Box::new(error_type.clone().unwrap_or(InferType::Dynamic)),
                        ),
                    }),
                });
                match family {
                    "Option" => InferType::Option(Box::new(mapped)),
                    _ => InferType::Result(
                        Box::new(mapped),
                        Box::new(error_type.clone().unwrap_or(InferType::Dynamic)),
                    ),
                }
            }
            "or_else" => {
                let mapped = self.type_gen.fresh();
                if family == "Option" {
                    expected_args.push(InferType::Function {
                        modes: self.fresh_param_modes(0),
                        params: Vec::new(),
                        ret: Box::new(InferType::Option(Box::new(value_type.clone()))),
                    });
                    InferType::Option(Box::new(value_type.clone()))
                } else {
                    expected_args.push(InferType::Function {
                        modes: self.fresh_param_modes(1),
                        params: vec![error_type.clone().unwrap_or(InferType::Dynamic)],
                        ret: Box::new(InferType::Result(
                            Box::new(value_type.clone()),
                            Box::new(mapped.clone()),
                        )),
                    });
                    InferType::Result(Box::new(value_type.clone()), Box::new(mapped))
                }
            }
            _ => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidSumMethod {
                        method: member.clone(),
                        receiver: typed_object.ty.clone(),
                    },
                    span: callee.span,
                    reason: ConstraintReason::Other("sum method".to_string()),
                });
                InferType::Dynamic
            }
        };

        if expected_args.len() != typed_args.len() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::InvalidSumMethod {
                    method: member.clone(),
                    receiver: typed_object.ty.clone(),
                },
                span: callee.span,
                reason: ConstraintReason::Other("sum method arity".to_string()),
            });
        }
        for (index, (arg, expected)) in typed_args.iter().zip(expected_args.iter()).enumerate() {
            let reason = ConstraintReason::Argument {
                func_name: format!("{family}::{member}"),
                arg_index: index,
            };
            if self.reject_sum_callback_dynamic_boundary(
                &arg.ty,
                expected,
                arg.span,
                reason.clone(),
            ) || self.reject_dynamic(&arg.ty, expected, arg.span, reason.clone())
            {
                continue;
            }
            if !self.reject_untyped_native(&arg.ty, expected, arg.span, reason.clone()) {
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
            InferType::function(expected_args, output.clone()),
            callee.span,
        );
        self.record_sum_type(format!("{family}::{member}"), output.clone(), callee.span);
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    fn reject_sum_callback_dynamic_boundary(
        &mut self,
        found: &InferType,
        expected: &InferType,
        span: Span,
        reason: ConstraintReason,
    ) -> bool {
        let (
            InferType::Function {
                params: found_params,
                ..
            },
            InferType::Function {
                params: expected_params,
                ..
            },
        ) = (found, expected)
        else {
            return false;
        };
        for (found_param, expected_param) in found_params.iter().zip(expected_params) {
            if expected_param.contains_dynamic() && !found_param.contains_dynamic() {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::Mismatch {
                        expected: expected_param.clone(),
                        found: found_param.clone(),
                    },
                    span,
                    reason,
                });
                return true;
            }
        }
        false
    }

    fn infer_inferred_sum_method_call(
        &mut self,
        callee: &Expr,
        typed_object: TypedExpr,
        member: &str,
        args: &[Expr],
    ) -> (TypedExprKind, InferType) {
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        let value = self.type_gen.fresh();
        let error = self.type_gen.fresh();
        let mut mapped = None;
        let mut callback_result = None;
        let mut callback = None;
        let mut preferred_family = None;
        let mut expected_args = Vec::new();
        let output = match member {
            "unwrap" => value.clone(),
            "expect" => {
                expected_args.push(InferType::String);
                value.clone()
            }
            "unwrap_or" => {
                expected_args.push(value.clone());
                value.clone()
            }
            "unwrap_or_else" => {
                callback = typed_args.first().map(|arg| arg.ty.clone());
                let params = match typed_args.first().and_then(|arg| function_arity(&arg.ty)) {
                    Some(1) => {
                        preferred_family = Some(SumFamily::Result);
                        vec![error.clone()]
                    }
                    Some(0) => {
                        preferred_family = Some(SumFamily::Option);
                        Vec::new()
                    }
                    _ => Vec::new(),
                };
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(params.len()),
                    params,
                    ret: Box::new(value.clone()),
                });
                value.clone()
            }
            "is_ok" | "is_err" => {
                preferred_family = Some(SumFamily::Result);
                InferType::Bool
            }
            "is_some" | "is_none" => {
                preferred_family = Some(SumFamily::Option);
                InferType::Bool
            }
            "ok_or" => {
                preferred_family = Some(SumFamily::Option);
                expected_args.push(error.clone());
                InferType::Result(Box::new(value.clone()), Box::new(error.clone()))
            }
            "ok" => {
                preferred_family = Some(SumFamily::Result);
                InferType::Option(Box::new(value.clone()))
            }
            "err" => {
                preferred_family = Some(SumFamily::Result);
                InferType::Option(Box::new(error.clone()))
            }
            "map" => {
                callback = typed_args.first().map(|arg| arg.ty.clone());
                let mapped_type = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![value.clone()],
                    ret: Box::new(mapped_type.clone()),
                });
                mapped = Some(mapped_type);
                let result = self.type_gen.fresh();
                callback_result = Some(result.clone());
                result
            }
            "map_err" => {
                callback = typed_args.first().map(|arg| arg.ty.clone());
                preferred_family = Some(SumFamily::Result);
                let mapped_type = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![error.clone()],
                    ret: Box::new(mapped_type.clone()),
                });
                mapped = Some(mapped_type);
                let result = self.type_gen.fresh();
                callback_result = Some(result.clone());
                result
            }
            "and_then" => {
                callback = typed_args.first().map(|arg| arg.ty.clone());
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(1),
                    params: vec![value.clone()],
                    ret: Box::new({
                        let result = self.type_gen.fresh();
                        callback_result = Some(result.clone());
                        result
                    }),
                });
                self.type_gen.fresh()
            }
            "or_else" => {
                callback = typed_args.first().map(|arg| arg.ty.clone());
                let params = match typed_args.first().and_then(|arg| function_arity(&arg.ty)) {
                    Some(1) => {
                        preferred_family = Some(SumFamily::Result);
                        vec![error.clone()]
                    }
                    Some(0) => {
                        preferred_family = Some(SumFamily::Option);
                        Vec::new()
                    }
                    _ => Vec::new(),
                };
                let result = self.type_gen.fresh();
                expected_args.push(InferType::Function {
                    modes: self.fresh_param_modes(params.len()),
                    params,
                    ret: Box::new(result.clone()),
                });
                callback_result = Some(result.clone());
                result
            }
            _ => unreachable!(),
        };

        let valid_arity = expected_args.len() == typed_args.len();
        if valid_arity
            && matches!(member, "unwrap_or_else" | "or_else")
            && typed_args
                .first()
                .is_some_and(|arg| function_arity(&arg.ty).is_none())
        {
            expected_args[0] = typed_args[0].ty.clone();
        }
        if !valid_arity {
            self.errors.push(TypeError {
                kind: TypeErrorKind::InvalidSumMethod {
                    method: member.to_string(),
                    receiver: typed_object.ty.clone(),
                },
                span: callee.span,
                reason: ConstraintReason::Other("sum method arity".to_string()),
            });
        }
        for (index, (arg, expected)) in typed_args.iter().zip(expected_args.iter()).enumerate() {
            let reason = ConstraintReason::Argument {
                func_name: format!("sum::{member}"),
                arg_index: index,
            };
            if self.reject_dynamic(&arg.ty, expected, arg.span, reason.clone()) {
                continue;
            }
            if !self.reject_untyped_native(&arg.ty, expected, arg.span, reason.clone()) {
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
                object: Box::new(typed_object.clone()),
                member: member.to_string(),
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            InferType::function(expected_args, output.clone()),
            callee.span,
        );
        if valid_arity {
            self.sum_method_residuals.push(SumMethodResidual {
                receiver: typed_object.ty,
                result: output.clone(),
                value,
                error,
                mapped,
                callback_result,
                callback,
                preferred_family,
                method: member.to_string(),
                span: callee.span,
            });
        }
        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        )
    }

    fn invalid_sum_method_call(
        &mut self,
        typed_object: TypedExpr,
        member: &str,
        args: &[Expr],
        callee: &Expr,
    ) -> (TypedExprKind, InferType) {
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        self.errors.push(TypeError {
            kind: TypeErrorKind::InvalidSumMethod {
                method: member.to_string(),
                receiver: typed_object.ty.clone(),
            },
            span: typed_object.span,
            reason: ConstraintReason::Other("sum method receiver".to_string()),
        });
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.to_string(),
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            InferType::function(
                vec![InferType::Dynamic; typed_args.len()],
                InferType::Dynamic,
            ),
            callee.span,
        );
        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            InferType::Dynamic,
        )
    }

    pub(super) fn validate_sum_method_residuals(&mut self, subst: &mut Substitution) {
        let mut pending = self.sum_method_residuals.clone();
        loop {
            let mut deferred = Vec::new();
            let mut progress = false;
            for residual in pending {
                let mut receiver = subst.apply(&residual.receiver);
                let family = sum_family(&receiver)
                    .or(residual.preferred_family)
                    .or_else(|| {
                        residual
                            .callback_result
                            .as_ref()
                            .and_then(|ty| sum_family(&subst.apply(ty)))
                    });
                let Some(family) = family else {
                    if matches!(receiver, InferType::Var(_)) {
                        deferred.push(residual);
                    } else {
                        self.push_sum_method_receiver_error(&residual, &receiver);
                    }
                    continue;
                };

                if matches!(receiver, InferType::Var(_)) {
                    let shape = match family {
                        SumFamily::Option => InferType::Option(Box::new(residual.value.clone())),
                        SumFamily::Result => InferType::Result(
                            Box::new(residual.value.clone()),
                            Box::new(residual.error.clone()),
                        ),
                    };
                    if let Err(error) = unify(&receiver, &shape, subst) {
                        self.errors.push(unify_error_to_type_error(
                            error,
                            residual.span,
                            ConstraintReason::Other(format!("sum method '{}'", residual.method)),
                        ));
                        continue;
                    }
                    receiver = subst.apply(&receiver);
                }

                let Some(expected) =
                    self.sum_method_result_type(&residual, family, &receiver, subst)
                else {
                    progress = true;
                    continue;
                };
                if let Err(error) = unify(&residual.result, &expected, subst) {
                    self.errors.push(unify_error_to_type_error(
                        error,
                        residual.span,
                        ConstraintReason::Other(format!("sum method '{}'", residual.method)),
                    ));
                }
                progress = true;
            }
            if deferred.is_empty() {
                break;
            }
            if !progress {
                break;
            }
            pending = deferred;
        }
        for residual in &self.sum_method_residuals {
            if subst.apply(&residual.receiver).has_vars()
                || subst.apply(&residual.result).has_vars()
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnresolvedSumType {
                        constructor: residual.method.clone(),
                    },
                    span: residual.span,
                    reason: ConstraintReason::Other("sum method resolution".to_string()),
                });
            }
        }
    }

    fn sum_method_result_type(
        &mut self,
        residual: &SumMethodResidual,
        family: SumFamily,
        receiver: &InferType,
        subst: &mut Substitution,
    ) -> Option<InferType> {
        let (value_type, error_type) = match receiver {
            InferType::Option(value) => (value.as_ref().clone(), None),
            InferType::Result(value, error) => {
                (value.as_ref().clone(), Some(error.as_ref().clone()))
            }
            _ => {
                self.push_sum_method_receiver_error(residual, receiver);
                return None;
            }
        };

        let mut unify_field = |this: &mut Self, found: &InferType, expected: &InferType| {
            let found_resolved = subst.apply(found);
            let expected_resolved = subst.apply(expected);
            if dynamic_type_mismatch(&found_resolved, &expected_resolved) {
                this.errors.push(TypeError {
                    kind: TypeErrorKind::Mismatch {
                        expected: expected_resolved,
                        found: found_resolved,
                    },
                    span: residual.span,
                    reason: ConstraintReason::Other(format!("sum method '{}'", residual.method)),
                });
                return;
            }
            if let Err(error) = unify(found, expected, subst) {
                this.errors.push(unify_error_to_type_error(
                    error,
                    residual.span,
                    ConstraintReason::Other(format!("sum method '{}'", residual.method)),
                ));
            }
        };

        match residual.method.as_str() {
            "unwrap" | "expect" | "unwrap_or" | "unwrap_or_else" => {
                unify_field(self, &residual.value, &value_type);
                if residual.method == "unwrap_or_else"
                    && let Some(callback) = &residual.callback
                {
                    let params = if family == SumFamily::Result {
                        vec![error_type.clone().unwrap_or(InferType::Dynamic)]
                    } else {
                        Vec::new()
                    };
                    let modes = self.fresh_param_modes(params.len());
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            modes,
                            params,
                            ret: Box::new(residual.value.clone()),
                        },
                    );
                }
                Some(residual.value.clone())
            }
            "is_ok" | "is_err" | "is_some" | "is_none" => {
                let expected = if matches!(residual.method.as_str(), "is_ok" | "is_err") {
                    SumFamily::Result
                } else {
                    SumFamily::Option
                };
                if family != expected {
                    self.push_sum_method_receiver_error(residual, receiver);
                    return None;
                }
                Some(InferType::Bool)
            }
            "ok_or" => {
                if family != SumFamily::Option {
                    self.push_sum_method_receiver_error(residual, receiver);
                    return None;
                }
                unify_field(self, &residual.value, &value_type);
                Some(InferType::Result(
                    Box::new(value_type),
                    Box::new(residual.error.clone()),
                ))
            }
            "ok" => {
                if family != SumFamily::Result {
                    self.push_sum_method_receiver_error(residual, receiver);
                    return None;
                }
                Some(InferType::Option(Box::new(value_type)))
            }
            "err" => {
                if family != SumFamily::Result {
                    self.push_sum_method_receiver_error(residual, receiver);
                    return None;
                }
                Some(InferType::Option(Box::new(
                    error_type.unwrap_or(InferType::Dynamic),
                )))
            }
            "map" => {
                unify_field(self, &residual.value, &value_type);
                let mapped = residual.mapped.clone().unwrap_or(InferType::Dynamic);
                if let Some(callback) = &residual.callback {
                    let modes = self.fresh_param_modes(1);
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            modes,
                            params: vec![value_type.clone()],
                            ret: Box::new(mapped.clone()),
                        },
                    );
                }
                Some(match family {
                    SumFamily::Option => InferType::Option(Box::new(mapped)),
                    SumFamily::Result => InferType::Result(
                        Box::new(mapped),
                        Box::new(error_type.unwrap_or(InferType::Dynamic)),
                    ),
                })
            }
            "map_err" => {
                if family != SumFamily::Result {
                    self.push_sum_method_receiver_error(residual, receiver);
                    return None;
                }
                let error_type = error_type.unwrap_or(InferType::Dynamic);
                unify_field(self, &residual.error, &error_type);
                let mapped = residual.mapped.clone().unwrap_or(InferType::Dynamic);
                if let Some(callback) = &residual.callback {
                    let modes = self.fresh_param_modes(1);
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            modes,
                            params: vec![error_type.clone()],
                            ret: Box::new(mapped.clone()),
                        },
                    );
                }
                Some(InferType::Result(Box::new(value_type), Box::new(mapped)))
            }
            "and_then" => {
                unify_field(self, &residual.value, &value_type);
                let callback_result = residual
                    .callback_result
                    .clone()
                    .unwrap_or(InferType::Dynamic);
                let expected_callback = match family {
                    SumFamily::Option => InferType::Option(Box::new(self.type_gen.fresh())),
                    SumFamily::Result => InferType::Result(
                        Box::new(self.type_gen.fresh()),
                        Box::new(error_type.unwrap_or(InferType::Dynamic)),
                    ),
                };
                if let Some(callback) = &residual.callback {
                    let modes = self.fresh_param_modes(1);
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            modes,
                            params: vec![value_type.clone()],
                            ret: Box::new(expected_callback.clone()),
                        },
                    );
                }
                unify_field(self, &callback_result, &expected_callback);
                Some(callback_result)
            }
            "or_else" => {
                let callback_result = residual
                    .callback_result
                    .clone()
                    .unwrap_or(InferType::Dynamic);
                let expected_callback = match family {
                    SumFamily::Option => InferType::Option(Box::new(value_type.clone())),
                    SumFamily::Result => InferType::Result(
                        Box::new(value_type.clone()),
                        Box::new(self.type_gen.fresh()),
                    ),
                };
                if let Some(callback) = &residual.callback {
                    let modes = self.fresh_param_modes(usize::from(family == SumFamily::Result));
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            modes,
                            params: if family == SumFamily::Result {
                                vec![error_type.clone().unwrap_or(InferType::Dynamic)]
                            } else {
                                Vec::new()
                            },
                            ret: Box::new(callback_result.clone()),
                        },
                    );
                }
                unify_field(self, &callback_result, &expected_callback);
                Some(callback_result)
            }
            _ => None,
        }
    }

    fn push_sum_method_receiver_error(
        &mut self,
        residual: &SumMethodResidual,
        receiver: &InferType,
    ) {
        let kind = match receiver {
            InferType::Dynamic => TypeErrorKind::DynamicSumMethod {
                method: residual.method.clone(),
            },
            InferType::UntypedNative(name) => TypeErrorKind::UntypedSumValue { name: name.clone() },
            _ => TypeErrorKind::InvalidSumMethod {
                method: residual.method.clone(),
                receiver: receiver.clone(),
            },
        };
        self.errors.push(TypeError {
            kind,
            span: residual.span,
            reason: ConstraintReason::Other("sum method receiver".to_string()),
        });
    }

    pub(super) fn record_must_use_value(&mut self, expr: &TypedExpr) {
        if is_assignment(expr) {
            return;
        }
        self.must_use_values.push(MustUseResidual {
            ty: expr.ty.clone(),
            span: expr.span,
        });
    }

    pub(super) fn validate_must_use_values(&mut self, subst: &Substitution) {
        for residual in &self.must_use_values {
            match subst.apply(&residual.ty) {
                InferType::Result(_, _) => self.errors.push(TypeError {
                    kind: TypeErrorKind::IgnoredResult,
                    span: residual.span,
                    reason: ConstraintReason::Other("must use Result".to_string()),
                }),
                InferType::Option(_) => self.errors.push(TypeError {
                    kind: TypeErrorKind::IgnoredOption,
                    span: residual.span,
                    reason: ConstraintReason::Other("must use Option".to_string()),
                }),
                _ => {}
            }
        }
    }

    pub(super) fn infer_sum_constructor_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let (name, qualified) = match &callee.kind {
            ExprKind::Identifier(name) => (name.as_str(), false),
            ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Path,
            } => {
                let ExprKind::Identifier(prefix) = &object.kind else {
                    return None;
                };
                if !matches!(prefix.as_str(), "Option" | "Result" | "Error") {
                    return None;
                }
                (member.as_str(), true)
            }
            _ => return None,
        };

        let family = match (qualified, name) {
            (false, "Some") | (true, "Some") if qualified_family(callee) == Some("Option") => {
                "Some"
            }
            (false, "Some") => "Some",
            (false, "Ok") | (true, "Ok") if qualified_family(callee) == Some("Result") => "Ok",
            (false, "Ok") => "Ok",
            (false, "Err") | (true, "Err") if qualified_family(callee) == Some("Result") => "Err",
            (false, "Err") => "Err",
            (true, "Message") if qualified_family(callee) == Some("Error") => "Message",
            _ => return None,
        };

        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        let arg_types: Vec<InferType> = typed_args.iter().map(|arg| arg.ty.clone()).collect();
        let output = match family {
            "Some" => {
                if arg_types.len() != 1 {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidSumMethod {
                            method: "Some".to_string(),
                            receiver: InferType::Option(Box::new(InferType::Dynamic)),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("sum constructor arity".to_string()),
                    });
                    InferType::Option(Box::new(InferType::Dynamic))
                } else {
                    InferType::Option(Box::new(arg_types[0].clone()))
                }
            }
            "Ok" => {
                let error = self.type_gen.fresh();
                if arg_types.len() != 1 {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidSumMethod {
                            method: "Ok".to_string(),
                            receiver: InferType::Result(
                                Box::new(InferType::Dynamic),
                                Box::new(error.clone()),
                            ),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("sum constructor arity".to_string()),
                    });
                    InferType::Result(Box::new(InferType::Dynamic), Box::new(error))
                } else {
                    InferType::Result(Box::new(arg_types[0].clone()), Box::new(error))
                }
            }
            "Err" => {
                let value = self.type_gen.fresh();
                if arg_types.len() != 1 {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidSumMethod {
                            method: "Err".to_string(),
                            receiver: InferType::Result(
                                Box::new(value.clone()),
                                Box::new(InferType::Dynamic),
                            ),
                        },
                        span: callee.span,
                        reason: ConstraintReason::Other("sum constructor arity".to_string()),
                    });
                    InferType::Result(Box::new(value), Box::new(InferType::Dynamic))
                } else {
                    InferType::Result(Box::new(value), Box::new(arg_types[0].clone()))
                }
            }
            "Message" => {
                if arg_types.len() == 1 {
                    let reason = ConstraintReason::Argument {
                        func_name: "Error::message".to_string(),
                        arg_index: 0,
                    };
                    if !self.reject_dynamic(
                        &arg_types[0],
                        &InferType::String,
                        args[0].span,
                        reason.clone(),
                    ) && !self.reject_untyped_native(
                        &arg_types[0],
                        &InferType::String,
                        args[0].span,
                        reason.clone(),
                    ) {
                        self.constraints.push(Constraint::equal(
                            InferType::String,
                            arg_types[0].clone(),
                            callee.span,
                            reason,
                        ));
                    }
                }
                InferType::Error
            }
            _ => return None,
        };
        self.record_sum_type(family, output.clone(), callee.span);

        let callee_type = InferType::function(arg_types, output.clone());
        let typed_callee = match &callee.kind {
            ExprKind::Identifier(name) => TypedExpr::new(
                TypedExprKind::Identifier(name.clone()),
                callee_type,
                callee.span,
            ),
            ExprKind::Member {
                object,
                member,
                separator,
            } => TypedExpr::new(
                TypedExprKind::Member {
                    object: Box::new(TypedExpr::new(
                        TypedExprKind::Identifier(match &object.kind {
                            ExprKind::Identifier(name) => name.clone(),
                            _ => String::new(),
                        }),
                        InferType::Error,
                        object.span,
                    )),
                    member: member.clone(),
                    separator: *separator,
                },
                callee_type,
                callee.span,
            ),
            _ => return None,
        };

        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    pub(super) fn infer_try_expr(
        &mut self,
        inner: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_inner = self.infer_expr(inner);
        let source = typed_inner.ty.clone();
        let Some(current_return) = self.current_return_type().cloned() else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::QuestionMarkOutsideResult { top_level: true },
                span,
                reason: ConstraintReason::Other("question mark".to_string()),
            });
            let output = match &source {
                InferType::Option(value) | InferType::Result(value, _) => value.as_ref().clone(),
                _ => self.type_gen.fresh(),
            };
            return (
                TypedExprKind::Try {
                    operand: Box::new(typed_inner),
                    conversion: None,
                    conversion_target: None,
                },
                output,
            );
        };

        let output = self.type_gen.fresh();
        let target = match (&source, &current_return) {
            (InferType::Option(source_value), InferType::Var(_)) => {
                let target = InferType::Option(Box::new(self.type_gen.fresh()));
                self.constraints.push(Constraint::equal(
                    source_value.as_ref().clone(),
                    output.clone(),
                    span,
                    ConstraintReason::Other("question mark source value".to_string()),
                ));
                self.constraints.push(Constraint::equal(
                    current_return.clone(),
                    target.clone(),
                    span,
                    ConstraintReason::Other("question mark return".to_string()),
                ));
                target
            }
            (InferType::Result(source_value, source_error), InferType::Var(_)) => {
                let target = InferType::Result(
                    Box::new(self.type_gen.fresh()),
                    Box::new(source_error.as_ref().clone()),
                );
                self.constraints.push(Constraint::equal(
                    source_value.as_ref().clone(),
                    output.clone(),
                    span,
                    ConstraintReason::Other("question mark source value".to_string()),
                ));
                self.constraints.push(Constraint::equal(
                    current_return.clone(),
                    target.clone(),
                    span,
                    ConstraintReason::Other("question mark return".to_string()),
                ));
                target
            }
            (InferType::Option(source_value), InferType::Option(_))
            | (InferType::Result(source_value, _), InferType::Result(_, _)) => {
                self.constraints.push(Constraint::equal(
                    source_value.as_ref().clone(),
                    output.clone(),
                    span,
                    ConstraintReason::Other("question mark source value".to_string()),
                ));
                current_return.clone()
            }
            (InferType::Option(_), InferType::Result(_, _))
            | (InferType::Result(_, _), InferType::Option(_)) => current_return.clone(),
            (InferType::UntypedNative(name), _) => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UntypedSumValue { name: name.clone() },
                    span,
                    reason: ConstraintReason::Other("question mark".to_string()),
                });
                current_return.clone()
            }
            (InferType::Option(source_value) | InferType::Result(source_value, _), _) => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkOutsideResult { top_level: false },
                    span,
                    reason: ConstraintReason::Other("question mark".to_string()),
                });
                self.constraints.push(Constraint::equal(
                    source_value.as_ref().clone(),
                    output.clone(),
                    span,
                    ConstraintReason::Other("question mark source value".to_string()),
                ));
                return (
                    TypedExprKind::Try {
                        operand: Box::new(typed_inner),
                        conversion: None,
                        conversion_target: None,
                    },
                    output,
                );
            }
            (InferType::Var(_), InferType::Option(_) | InferType::Result(_, _)) => {
                let expected_source = match &current_return {
                    InferType::Option(_) => InferType::Option(Box::new(output.clone())),
                    _ => {
                        InferType::Result(Box::new(output.clone()), Box::new(self.type_gen.fresh()))
                    }
                };
                self.constraints.push(Constraint::equal(
                    expected_source.clone(),
                    source.clone(),
                    span,
                    ConstraintReason::Other("question mark".to_string()),
                ));
                self.try_residuals.push(TryResidual {
                    source: expected_source,
                    target: current_return.clone(),
                    span,
                });
                return (
                    TypedExprKind::Try {
                        operand: Box::new(typed_inner),
                        conversion: None,
                        conversion_target: None,
                    },
                    output,
                );
            }
            (_, _) => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkTypeMismatch {
                        source: source.clone(),
                        target: current_return.clone(),
                    },
                    span,
                    reason: ConstraintReason::Other("question mark".to_string()),
                });
                return (
                    TypedExprKind::Try {
                        operand: Box::new(typed_inner),
                        conversion: None,
                        conversion_target: None,
                    },
                    output,
                );
            }
        };

        self.try_residuals.push(TryResidual {
            source,
            target: target.clone(),
            span,
        });
        (
            TypedExprKind::Try {
                operand: Box::new(typed_inner),
                conversion: None,
                conversion_target: None,
            },
            output,
        )
    }

    pub(super) fn validate_try_residuals(&mut self, subst: &Substitution) {
        let mut errors = Vec::new();
        let mut conversions = Vec::new();
        for residual in &self.try_residuals {
            let source = subst.apply(&residual.source);
            let target = subst.apply(&residual.target);
            if contains_dynamic_type(&source) || contains_dynamic_type(&target) {
                errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkTypeMismatch { source, target },
                    span: residual.span,
                    reason: ConstraintReason::Other("question mark conversion".to_string()),
                });
                continue;
            }
            match (&source, &target) {
                (InferType::Option(_), InferType::Option(_)) => {}
                (InferType::Result(_, source_error), InferType::Result(_, target_error)) => {
                    match self
                        .type_table
                        .select_from_conversion(source_error, target_error)
                    {
                        crate::types::FromSelection::Identity => {}
                        crate::types::FromSelection::Selected(symbol) => {
                            conversions.push((
                                residual.span,
                                symbol,
                                source.clone(),
                                target.clone(),
                            ));
                        }
                        crate::types::FromSelection::Denied => {
                            errors.push(TypeError {
                                kind: TypeErrorKind::UnsatisfiedTraitBound {
                                    trait_name: crate::prelude::FROM_TRAIT.to_string(),
                                    trait_args: vec![source_error.as_ref().clone()],
                                    ty: target_error.as_ref().clone(),
                                    denied: true,
                                },
                                span: residual.span,
                                reason: ConstraintReason::Other(
                                    "question mark conversion".to_string(),
                                ),
                            });
                        }
                        crate::types::FromSelection::Unresolved(candidates) => {
                            errors.push(TypeError {
                                kind: TypeErrorKind::UnsatisfiedTryConversion {
                                    source_error: source_error.as_ref().clone(),
                                    target_error: target_error.as_ref().clone(),
                                    source: source.clone(),
                                    target: target.clone(),
                                    candidates,
                                },
                                span: residual.span,
                                reason: ConstraintReason::Other(
                                    "question mark conversion".to_string(),
                                ),
                            });
                        }
                    }
                }
                (InferType::Option(_), InferType::Result(_, _))
                | (InferType::Result(_, _), InferType::Option(_)) => {
                    errors.push(TypeError {
                        kind: TypeErrorKind::InvalidTryResidual { source, target },
                        span: residual.span,
                        reason: ConstraintReason::Other("question mark residual".to_string()),
                    });
                }
                _ => errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkTypeMismatch { source, target },
                    span: residual.span,
                    reason: ConstraintReason::Other("question mark conversion".to_string()),
                }),
            }
        }
        self.errors.extend(errors);
        for (span, symbol, source, target) in conversions {
            self.try_conversions
                .insert((span.start, span.end), (symbol, source, target));
        }
    }

    pub(super) fn infer_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let discarded = std::mem::take(&mut self.statement_match);
        let typed_scrutinee = self.infer_expr(scrutinee);
        let result_type = if discarded {
            InferType::Unit
        } else {
            self.type_gen.fresh()
        };
        let mut dynamic_result = false;
        let mut typed_arms = Vec::with_capacity(arms.len());
        let mut implicit_unit_arms = Vec::new();
        let mut value_arms = 0usize;

        let patterns: Vec<Pattern> = arms
            .iter()
            .map(|arm| self.resolve_constant_patterns(&arm.pattern))
            .collect();
        for (arm, pattern) in arms.iter().zip(&patterns) {
            self.env.push_scope();
            let typed_pattern = self.infer_pattern(pattern, &typed_scrutinee.ty);
            let mut bindings = Vec::new();
            collect_pattern_bindings(&typed_pattern, &mut bindings);
            let names: Vec<String> = bindings.into_iter().map(|(name, _)| name).collect();
            self.record_binding_origins(scrutinee, &names);
            let typed_guard = arm.guard.as_ref().map(|guard| {
                let typed_guard = self.infer_expr(guard);
                if !self.reject_dynamic(
                    &typed_guard.ty,
                    &InferType::Bool,
                    guard.span,
                    ConstraintReason::IfCondition,
                ) && !self.reject_untyped_native(
                    &typed_guard.ty,
                    &InferType::Bool,
                    guard.span,
                    ConstraintReason::IfCondition,
                ) {
                    self.constraints.push(Constraint::equal(
                        InferType::Bool,
                        typed_guard.ty.clone(),
                        guard.span,
                        ConstraintReason::IfCondition,
                    ));
                }
                typed_guard
            });
            let typed_body = match &arm.body {
                MatchArmBody::Expr(expr) if discarded => {
                    let typed_expr = self.infer_expr(expr);
                    self.record_must_use_value(&typed_expr);
                    TypedMatchArmBody::Expr(typed_expr)
                }
                MatchArmBody::Block(stmts) if discarded => {
                    let (typed_stmts, _) = self.infer_stmts_value(stmts);
                    if let Some(crate::typed_ast::TypedStmt {
                        kind: crate::typed_ast::TypedStmtKind::Expression(tail),
                        ..
                    }) = typed_stmts.last()
                    {
                        self.record_must_use_value(tail);
                    }
                    TypedMatchArmBody::Block(typed_stmts)
                }
                MatchArmBody::Expr(expr) => {
                    let typed_expr = self.infer_expr(expr);
                    value_arms += 1;
                    dynamic_result |= matches!(typed_expr.ty, InferType::Dynamic);
                    TypedMatchArmBody::Expr(typed_expr)
                }
                MatchArmBody::Block(stmts) => {
                    let (typed_stmts, body_type) = self.infer_stmts_value(stmts);
                    dynamic_result |= matches!(body_type, Some(InferType::Dynamic));
                    if let Some(body_type) = body_type {
                        if tail_is_implicit_unit(stmts) {
                            implicit_unit_arms.push(arm.span);
                        } else {
                            value_arms += 1;
                            self.constraints.push(Constraint::equal(
                                result_type.clone(),
                                body_type,
                                arm.span,
                                ConstraintReason::IfBranches,
                            ));
                        }
                    }
                    TypedMatchArmBody::Block(typed_stmts)
                }
            };
            let explicit_dynamic = match &typed_body {
                TypedMatchArmBody::Expr(expr) => self.is_explicit_dynamic_expr(expr),
                TypedMatchArmBody::Block(stmts) => self.is_explicit_dynamic_stmt_tail(stmts),
            };
            self.env.pop_scope();
            typed_arms.push(TypedMatchArm {
                pattern: typed_pattern,
                guard: typed_guard,
                body: typed_body,
                explicit_dynamic,
                span: arm.span,
            });
        }

        if !discarded {
            let mut bodies: Vec<&mut TypedExpr> = typed_arms
                .iter_mut()
                .filter_map(|arm| match &mut arm.body {
                    TypedMatchArmBody::Expr(expr) => Some(expr),
                    TypedMatchArmBody::Block(_) => None,
                })
                .collect();
            self.unify_branch_literals(&mut bodies);
            for arm in &typed_arms {
                if let TypedMatchArmBody::Expr(typed_expr) = &arm.body
                    && !self.reject_dynamic(
                        &typed_expr.ty,
                        &result_type,
                        typed_expr.span,
                        ConstraintReason::IfBranches,
                    )
                {
                    self.constraints.push(Constraint::equal(
                        result_type.clone(),
                        typed_expr.ty.clone(),
                        typed_expr.span,
                        ConstraintReason::IfBranches,
                    ));
                }
            }
        }

        if !implicit_unit_arms.is_empty() {
            if value_arms == 0 {
                self.constraints.push(Constraint::equal(
                    result_type.clone(),
                    InferType::Unit,
                    span,
                    ConstraintReason::IfBranches,
                ));
            } else {
                self.match_unit_residuals
                    .push((result_type.clone(), implicit_unit_arms));
            }
        }

        self.match_exhaustivity_residuals
            .push(MatchExhaustivityResidual {
                ty: typed_scrutinee.ty.clone(),
                arms: arms
                    .iter()
                    .zip(patterns)
                    .map(|(arm, pattern)| MatchArmCoverage {
                        pattern,
                        guarded: arm.guard.is_some(),
                        synthetic: arm.synthetic,
                        span: arm.span,
                    })
                    .collect(),
                span,
            });

        (
            TypedExprKind::Match {
                scrutinee: Box::new(typed_scrutinee),
                arms: typed_arms,
            },
            if dynamic_result {
                InferType::Dynamic
            } else {
                result_type
            },
        )
    }

    fn mismatched_tuple_pattern(
        &mut self,
        name: &str,
        ty: &InferType,
        pattern: &Pattern,
    ) -> Option<TypedPattern> {
        let InferType::Applied { name: actual, .. } = ty else {
            return None;
        };
        let expected = aelys_syntax::tuple_arity(actual)?;
        let found = aelys_syntax::tuple_arity(name)?;
        if expected == found {
            return None;
        }
        self.errors.push(TypeError {
            kind: TypeErrorKind::TuplePatternArity { expected, found },
            span: pattern.span,
            reason: ConstraintReason::Other("tuple pattern".to_string()),
        });
        Some(self.infer_pattern(
            pattern,
            &InferType::Applied {
                name: name.to_string(),
                args: vec![InferType::Poison; found],
            },
        ))
    }

    // a bare name that names a constant in scope matches its value instead of binding
    fn resolve_constant_patterns(&mut self, pattern: &Pattern) -> Pattern {
        let each = |this: &mut Self, patterns: &[Pattern]| -> Vec<Pattern> {
            patterns
                .iter()
                .map(|pattern| this.resolve_constant_patterns(pattern))
                .collect()
        };
        let kind = match &pattern.kind {
            PatternKind::Binding(name) => {
                let leaf = Expr::new(ExprKind::Identifier(name.clone()), pattern.span);
                match self.constant_leaf(&leaf) {
                    Some(super::constants::ConstValue::Int(value)) => PatternKind::Int(value),
                    Some(super::constants::ConstValue::Bool(value)) => PatternKind::Bool(value),
                    Some(super::constants::ConstValue::Str(value)) => PatternKind::String(value),
                    Some(super::constants::ConstValue::Float(_)) => {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::Mismatch {
                                expected: InferType::I64,
                                found: InferType::F64,
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other(format!(
                                "constant '{name}' is a float, and a float cannot be matched by a pattern"
                            )),
                        });
                        PatternKind::Wildcard
                    }
                    None => return pattern.clone(),
                }
            }
            PatternKind::Variant {
                path,
                type_args,
                fields,
            } => PatternKind::Variant {
                path: path.clone(),
                type_args: type_args.clone(),
                fields: each(self, fields),
            },
            PatternKind::Or(alternatives) => PatternKind::Or(each(self, alternatives)),
            PatternKind::Struct {
                path,
                type_args,
                fields,
                has_rest,
            } => PatternKind::Struct {
                path: path.clone(),
                type_args: type_args.clone(),
                fields: fields
                    .iter()
                    .map(|field| aelys_syntax::StructPatternField {
                        name: field.name.clone(),
                        pattern: self.resolve_constant_patterns(&field.pattern),
                        span: field.span,
                    })
                    .collect(),
                has_rest: *has_rest,
            },
            _ => return pattern.clone(),
        };
        Pattern {
            kind,
            span: pattern.span,
        }
    }

    fn constrain_integer_pattern(&mut self, ty: &InferType, bounds: &[Option<i64>], span: Span) {
        if ty.is_integer() {
            if let Some(value) = bounds
                .iter()
                .flatten()
                .find(|value| !InferType::int_fits(**value, ty))
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::Mismatch {
                        expected: ty.clone(),
                        found: InferType::I64,
                    },
                    span,
                    reason: ConstraintReason::IntLiteralOverflow {
                        value: *value,
                        target: ty.clone(),
                    },
                });
            }
            return;
        }
        self.constraints.push(Constraint::equal(
            ty.clone(),
            InferType::I64,
            span,
            ConstraintReason::Comparison,
        ));
    }

    fn infer_pattern(&mut self, pattern: &Pattern, ty: &InferType) -> TypedPattern {
        let kind = match &pattern.kind {
            PatternKind::Wildcard => TypedPatternKind::Wildcard,
            PatternKind::Binding(name) => {
                if ty.contains_dynamic() {
                    self.env
                        .define_explicit_dynamic_local(name.clone(), ty.clone());
                } else {
                    self.env.define_local(name.clone(), ty.clone());
                }
                TypedPatternKind::Binding(name.clone())
            }
            PatternKind::Int(value) => {
                self.constrain_integer_pattern(ty, &[Some(*value)], pattern.span);
                TypedPatternKind::Int(*value)
            }
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let end = end.map(|end| if *inclusive { end } else { end - 1 });
                self.constrain_integer_pattern(ty, &[*start, end], pattern.span);
                TypedPatternKind::Range { start: *start, end }
            }
            PatternKind::String(value) => {
                self.constraints.push(Constraint::equal(
                    ty.clone(),
                    InferType::String,
                    pattern.span,
                    ConstraintReason::Comparison,
                ));
                TypedPatternKind::String(value.clone())
            }
            PatternKind::Char(value) => {
                self.constraints.push(Constraint::equal(
                    ty.clone(),
                    InferType::Char,
                    pattern.span,
                    ConstraintReason::Comparison,
                ));
                TypedPatternKind::Char(*value)
            }
            PatternKind::Bool(value) => {
                self.constraints.push(Constraint::equal(
                    ty.clone(),
                    InferType::Bool,
                    pattern.span,
                    ConstraintReason::Comparison,
                ));
                TypedPatternKind::Bool(*value)
            }
            PatternKind::Variant {
                path,
                type_args,
                fields,
            } => {
                let variant = path.last().cloned().unwrap_or_default();
                if let Some(enum_name) = enum_type_name(ty)
                    && let Some(enum_def) = self.type_table.get_enum(&enum_name).cloned()
                {
                    if !enum_path_matches(path, &enum_name) {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownVariant {
                                variant: path.join("::"),
                                expected: enum_name.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("enum match path".to_string()),
                        });
                        return TypedPattern {
                            kind: TypedPatternKind::Variant {
                                path: path.clone(),
                                enum_schema_index: None,
                                enum_variant_index: None,
                                fields: Vec::new(),
                                field_offsets: Vec::new(),
                            },
                            ty: ty.clone(),
                            span: pattern.span,
                        };
                    }
                    if !type_args.is_empty() {
                        if type_args.len() != enum_def.type_params.len() {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::GenericArityMismatch {
                                    name: enum_name.clone(),
                                    expected: enum_def.type_params.len(),
                                    found: type_args.len(),
                                },
                                span: pattern.span,
                                reason: ConstraintReason::Other(
                                    "enum pattern type arguments".to_string(),
                                ),
                            });
                        }
                        if let InferType::Applied { args, .. } = &ty {
                            let explicit_types: Vec<_> = type_args
                                .iter()
                                .map(|annotation| self.type_from_annotation(annotation))
                                .collect();
                            for (explicit, actual) in explicit_types.into_iter().zip(args) {
                                self.constraints.push(Constraint::equal(
                                    explicit,
                                    actual.clone(),
                                    pattern.span,
                                    ConstraintReason::Other(
                                        "enum pattern type arguments".to_string(),
                                    ),
                                ));
                            }
                        }
                    }
                    let substitutions = enum_substitutions(ty, &enum_def);
                    let Some((variant_index, enum_variant)) = enum_def
                        .variants
                        .iter()
                        .enumerate()
                        .find(|(_, candidate)| candidate.name == variant)
                    else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownVariant {
                                variant: path.join("::"),
                                expected: enum_name.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("enum match pattern".to_string()),
                        });
                        return TypedPattern {
                            kind: TypedPatternKind::Variant {
                                path: path.clone(),
                                enum_schema_index: None,
                                enum_variant_index: None,
                                fields: Vec::new(),
                                field_offsets: Vec::new(),
                            },
                            ty: ty.clone(),
                            span: pattern.span,
                        };
                    };
                    let typed_fields = match &enum_variant.fields {
                        crate::types::EnumVariantFieldsDef::Unit => {
                            if !fields.is_empty() {
                                self.errors.push(TypeError {
                                    kind: TypeErrorKind::InvalidSumMethod {
                                        method: format!("{variant} pattern"),
                                        receiver: ty.clone(),
                                    },
                                    span: pattern.span,
                                    reason: ConstraintReason::Other(
                                        "enum pattern arity".to_string(),
                                    ),
                                });
                            }
                            Vec::new()
                        }
                        crate::types::EnumVariantFieldsDef::Tuple(expected) => {
                            if fields.len() != expected.len() {
                                self.errors.push(TypeError {
                                    kind: TypeErrorKind::InvalidSumMethod {
                                        method: format!("{variant} pattern"),
                                        receiver: ty.clone(),
                                    },
                                    span: pattern.span,
                                    reason: ConstraintReason::Other(
                                        "enum pattern arity".to_string(),
                                    ),
                                });
                            }
                            fields
                                .iter()
                                .zip(expected.iter())
                                .map(|(field, expected)| {
                                    let expected = expected.substitute_params(&substitutions);
                                    self.infer_pattern(field, &expected)
                                })
                                .collect()
                        }
                        crate::types::EnumVariantFieldsDef::Named(_) => {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::InvalidSumMethod {
                                    method: format!("{variant} pattern"),
                                    receiver: ty.clone(),
                                },
                                span: pattern.span,
                                reason: ConstraintReason::Other("enum pattern shape".to_string()),
                            });
                            Vec::new()
                        }
                    };
                    let field_offsets = (0..typed_fields.len())
                        .map(|offset| u16::try_from(offset).unwrap_or(u16::MAX))
                        .collect();
                    return TypedPattern {
                        kind: TypedPatternKind::Variant {
                            path: path.clone(),
                            enum_schema_index: self.type_table.enum_schema_index(&enum_name),
                            enum_variant_index: u16::try_from(variant_index).ok(),
                            fields: typed_fields,
                            field_offsets,
                        },
                        ty: ty.clone(),
                        span: pattern.span,
                    };
                }
                let expected_family = match ty {
                    InferType::Option(_) => Some("Option"),
                    InferType::Result(_, _) => Some("Result"),
                    InferType::Error => Some("Error"),
                    _ => None,
                };
                let qualified_family_mismatch = path.len() > 1
                    && (expected_family.is_some_and(|family| path[0] != family)
                        || (matches!(ty, InferType::Var(_))
                            && variant_family(&variant) != Some(path[0].as_str())));
                if qualified_family_mismatch {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::UnknownVariant {
                            variant: path.join("::"),
                            expected: ty.to_string(),
                        },
                        span: pattern.span,
                        reason: ConstraintReason::Other("match pattern family".to_string()),
                    });
                }
                let payload_ty = match (ty, variant.as_str()) {
                    (InferType::Option(inner), "Some") => Some(inner.as_ref().clone()),
                    (InferType::Option(_), "None") => None,
                    (InferType::Result(ok, _), "Ok") => Some(ok.as_ref().clone()),
                    (InferType::Result(_, err), "Err") => Some(err.as_ref().clone()),
                    (InferType::Error, "Message") => Some(InferType::String),
                    (InferType::Var(_), "Some") => {
                        let inner = self.type_gen.fresh();
                        self.constraints.push(Constraint::equal(
                            ty.clone(),
                            InferType::Option(Box::new(inner.clone())),
                            pattern.span,
                            ConstraintReason::Other("Some match pattern".to_string()),
                        ));
                        Some(inner)
                    }
                    (InferType::Var(_), "None") => {
                        let inner = self.type_gen.fresh();
                        self.constraints.push(Constraint::equal(
                            ty.clone(),
                            InferType::Option(Box::new(inner)),
                            pattern.span,
                            ConstraintReason::Other("None match pattern".to_string()),
                        ));
                        None
                    }
                    (InferType::Var(_), "Ok") => {
                        let ok = self.type_gen.fresh();
                        let error = self.type_gen.fresh();
                        self.constraints.push(Constraint::equal(
                            ty.clone(),
                            InferType::Result(Box::new(ok.clone()), Box::new(error)),
                            pattern.span,
                            ConstraintReason::Other("Ok match pattern".to_string()),
                        ));
                        Some(ok)
                    }
                    (InferType::Var(_), "Err") => {
                        let ok = self.type_gen.fresh();
                        let error = self.type_gen.fresh();
                        self.constraints.push(Constraint::equal(
                            ty.clone(),
                            InferType::Result(Box::new(ok), Box::new(error.clone())),
                            pattern.span,
                            ConstraintReason::Other("Err match pattern".to_string()),
                        ));
                        Some(error)
                    }
                    (InferType::Var(_), "Message") => {
                        self.constraints.push(Constraint::equal(
                            ty.clone(),
                            InferType::Error,
                            pattern.span,
                            ConstraintReason::Other("Error match pattern".to_string()),
                        ));
                        Some(InferType::String)
                    }
                    _ => {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownVariant {
                                variant: variant.clone(),
                                expected: ty.to_string(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("match pattern".to_string()),
                        });
                        None
                    }
                };
                let typed_fields = if let Some(payload_ty) = payload_ty {
                    if fields.len() != 1 {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::InvalidSumMethod {
                                method: format!("{variant} pattern"),
                                receiver: ty.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("match pattern arity".to_string()),
                        });
                        Vec::new()
                    } else {
                        vec![self.infer_pattern(&fields[0], &payload_ty)]
                    }
                } else {
                    if !fields.is_empty() {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::InvalidSumMethod {
                                method: format!("{variant} pattern"),
                                receiver: ty.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("match pattern arity".to_string()),
                        });
                    }
                    Vec::new()
                };
                TypedPatternKind::Variant {
                    path: path.clone(),
                    enum_schema_index: None,
                    enum_variant_index: None,
                    fields: typed_fields,
                    field_offsets: Vec::new(),
                }
            }
            PatternKind::Struct {
                path,
                fields,
                has_rest,
                ..
            } => {
                if let Some(enum_name) = enum_type_name(ty)
                    && self.type_table.get_enum(&enum_name).is_some()
                    && !enum_path_matches(path, &enum_name)
                {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::UnknownVariant {
                            variant: path.join("::"),
                            expected: enum_name,
                        },
                        span: pattern.span,
                        reason: ConstraintReason::Other("enum match pattern path".to_string()),
                    });
                    return TypedPattern {
                        kind: TypedPatternKind::Variant {
                            path: path.clone(),
                            enum_schema_index: None,
                            enum_variant_index: None,
                            fields: Vec::new(),
                            field_offsets: Vec::new(),
                        },
                        ty: ty.clone(),
                        span: pattern.span,
                    };
                }
                if let Some(enum_name) = enum_type_name(ty)
                    && let Some(enum_def) = self.type_table.get_enum(&enum_name).cloned()
                    && let Some((_, variant)) =
                        enum_def.variants.iter().enumerate().find(|(_, candidate)| {
                            path.first().is_some_and(|segment| segment == &enum_name)
                                && path
                                    .last()
                                    .is_some_and(|segment| segment == &candidate.name)
                        })
                {
                    let crate::types::EnumVariantFieldsDef::Named(expected) = &variant.fields
                    else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::InvalidSumMethod {
                                method: format!("{} pattern", variant.name),
                                receiver: ty.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("enum pattern shape".to_string()),
                        });
                        return TypedPattern {
                            kind: TypedPatternKind::Variant {
                                path: path.clone(),
                                enum_schema_index: None,
                                enum_variant_index: None,
                                fields: Vec::new(),
                                field_offsets: Vec::new(),
                            },
                            ty: ty.clone(),
                            span: pattern.span,
                        };
                    };
                    let mut seen = std::collections::HashSet::new();
                    let mut typed_fields = Vec::with_capacity(expected.len());
                    let mut field_offsets = Vec::with_capacity(expected.len());
                    for field in fields {
                        if !seen.insert(field.name.clone()) {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::DuplicateStructField {
                                    structure: format!("{}::{}", enum_name, variant.name),
                                    field: field.name.clone(),
                                },
                                span: field.span,
                                reason: ConstraintReason::Other("enum pattern field".to_string()),
                            });
                        }
                        if !expected
                            .iter()
                            .any(|candidate| candidate.name == field.name)
                        {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::UnknownField {
                                    structure: format!("{}::{}", enum_name, variant.name),
                                    field: field.name.clone(),
                                },
                                span: field.span,
                                reason: ConstraintReason::Other("enum pattern field".to_string()),
                            });
                        }
                    }
                    for expected_field in expected {
                        let Some(field) = fields
                            .iter()
                            .find(|candidate| candidate.name == expected_field.name)
                        else {
                            if !*has_rest {
                                if !self.check_field_visibility(FieldVisibilityCheck {
                                    structure: &format!("{}::{}", enum_name, variant.name),
                                    field: &expected_field.name,
                                    is_pub: expected_field.is_pub,
                                    owner: &enum_def.owner,
                                    span: pattern.span,
                                    operation: "an enum pattern",
                                    construction: true,
                                }) {
                                    continue;
                                }
                                self.errors.push(TypeError {
                                    kind: TypeErrorKind::MissingField {
                                        structure: format!("{}::{}", enum_name, variant.name),
                                        field: expected_field.name.clone(),
                                    },
                                    span: pattern.span,
                                    reason: ConstraintReason::Other(
                                        "enum pattern field".to_string(),
                                    ),
                                });
                            }
                            continue;
                        };
                        if !self.check_field_visibility(FieldVisibilityCheck {
                            structure: &format!("{}::{}", enum_name, variant.name),
                            field: &expected_field.name,
                            is_pub: expected_field.is_pub,
                            owner: &enum_def.owner,
                            span: field.span,
                            operation: "an enum pattern",
                            construction: true,
                        }) {
                            continue;
                        }
                        typed_fields.push(self.infer_pattern(&field.pattern, &expected_field.ty));
                        field_offsets.push(expected_field.ordinal);
                    }
                    return TypedPattern {
                        kind: TypedPatternKind::Variant {
                            path: path.clone(),
                            enum_schema_index: self.type_table.enum_schema_index(&enum_name),
                            enum_variant_index: enum_def
                                .variants
                                .iter()
                                .position(|candidate| candidate.name == variant.name)
                                .and_then(|index| u16::try_from(index).ok()),
                            fields: typed_fields,
                            field_offsets,
                        },
                        ty: ty.clone(),
                        span: pattern.span,
                    };
                }
                let name = path.last().cloned().unwrap_or_default();
                let refused = self.private_nominal_error(&name, pattern.span);
                let Some(def) = refused
                    .is_none()
                    .then(|| self.type_table.get_struct(&name).cloned())
                    .flatten()
                else {
                    if let Some(error) = refused {
                        self.errors.push(error);
                        return TypedPattern {
                            kind: TypedPatternKind::Struct {
                                name,
                                schema_index: 0,
                                fields: Vec::new(),
                                has_rest: true,
                            },
                            ty: InferType::Poison,
                            span: pattern.span,
                        };
                    }
                    self.errors.push(TypeError {
                        kind: self.nominal_error_kind(
                            &name,
                            TypeErrorKind::UnknownStruct { name: name.clone() },
                        ),
                        span: pattern.span,
                        reason: ConstraintReason::UnknownType { name: name.clone() },
                    });
                    return TypedPattern {
                        kind: TypedPatternKind::Struct {
                            name,
                            schema_index: 0,
                            fields: Vec::new(),
                            has_rest: *has_rest,
                        },
                        ty: ty.clone(),
                        span: pattern.span,
                    };
                };
                if let Some(typed) = self.mismatched_tuple_pattern(&name, ty, pattern) {
                    return typed;
                }
                let field_substitutions: std::collections::HashMap<String, InferType> =
                    if def.type_params.is_empty() {
                        if let InferType::Var(_) = ty {
                            self.constraints.push(Constraint::equal(
                                ty.clone(),
                                InferType::Struct(name.clone()),
                                pattern.span,
                                ConstraintReason::Other("struct pattern type".to_string()),
                            ));
                        } else if !matches!(ty, InferType::Struct(actual) if actual == &name) {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::Mismatch {
                                    expected: InferType::Struct(name.clone()),
                                    found: ty.clone(),
                                },
                                span: pattern.span,
                                reason: ConstraintReason::Other("struct pattern type".to_string()),
                            });
                        }
                        std::collections::HashMap::new()
                    } else {
                        let args: Vec<InferType> = match ty {
                            InferType::Applied { name: actual, args }
                                if actual == &name && args.len() == def.type_params.len() =>
                            {
                                args.clone()
                            }
                            _ => {
                                let fresh: Vec<InferType> = def
                                    .type_params
                                    .iter()
                                    .map(|_| self.type_gen.fresh())
                                    .collect();
                                let expected = InferType::Applied {
                                    name: name.clone(),
                                    args: fresh.clone(),
                                };
                                if let InferType::Var(_) = ty {
                                    self.constraints.push(Constraint::equal(
                                        ty.clone(),
                                        expected,
                                        pattern.span,
                                        ConstraintReason::Other("struct pattern type".to_string()),
                                    ));
                                } else {
                                    self.errors.push(TypeError {
                                        kind: TypeErrorKind::Mismatch {
                                            expected,
                                            found: ty.clone(),
                                        },
                                        span: pattern.span,
                                        reason: ConstraintReason::Other(
                                            "struct pattern type".to_string(),
                                        ),
                                    });
                                }
                                fresh
                            }
                        };
                        def.type_params.iter().cloned().zip(args).collect()
                    };
                let mut seen = std::collections::HashSet::new();
                let schema_index = self.type_table.schema_index(&name).unwrap_or(0);
                let mut typed_fields = Vec::with_capacity(fields.len());
                for field in fields {
                    if !seen.insert(field.name.clone()) {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::DuplicateStructField {
                                structure: name.clone(),
                                field: field.name.clone(),
                            },
                            span: field.span,
                            reason: ConstraintReason::Other("struct pattern field".to_string()),
                        });
                        continue;
                    }
                    let Some(field_def) = def
                        .fields
                        .iter()
                        .find(|candidate| candidate.name == field.name)
                    else {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UnknownField {
                                structure: name.clone(),
                                field: field.name.clone(),
                            },
                            span: field.span,
                            reason: ConstraintReason::Other("struct pattern field".to_string()),
                        });
                        continue;
                    };
                    if !self.check_field_visibility(FieldVisibilityCheck {
                        structure: &name,
                        field: &field.name,
                        is_pub: field_def.is_pub,
                        owner: &def.owner,
                        span: field.span,
                        operation: "a struct pattern",
                        construction: true,
                    }) {
                        continue;
                    }
                    let field_ty = field_def.ty.substitute_params(&field_substitutions);
                    typed_fields.push((
                        field.name.clone(),
                        self.infer_pattern(&field.pattern, &field_ty),
                        field_def.ordinal,
                    ));
                }
                if !*has_rest {
                    for field in &def.fields {
                        if fields.iter().any(|candidate| candidate.name == field.name) {
                            continue;
                        }
                        self.check_field_visibility(FieldVisibilityCheck {
                            structure: &name,
                            field: &field.name,
                            is_pub: field.is_pub,
                            owner: &def.owner,
                            span: pattern.span,
                            operation: "a struct pattern",
                            construction: true,
                        });
                    }
                    let required_fields = def
                        .fields
                        .iter()
                        .filter(|field| {
                            field.is_pub || self.current_module.is_same_or_descendant_of(&def.owner)
                        })
                        .count();
                    if typed_fields.len() != required_fields {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::NonExhaustiveStruct {
                                structure: name.clone(),
                            },
                            span: pattern.span,
                            reason: ConstraintReason::Other("struct pattern fields".to_string()),
                        });
                    }
                }
                TypedPatternKind::Struct {
                    name,
                    schema_index,
                    fields: typed_fields,
                    has_rest: *has_rest,
                }
            }
            PatternKind::Or(alternatives) => {
                let saved_env = self.env.clone();
                let mut typed_alternatives = Vec::with_capacity(alternatives.len());
                for alternative in alternatives {
                    self.env = saved_env.clone();
                    typed_alternatives.push(self.infer_pattern(alternative, ty));
                }
                self.env = saved_env;
                if let Some(first) = typed_alternatives.first() {
                    let mut bindings = Vec::new();
                    collect_pattern_bindings(first, &mut bindings);
                    let expected_names: Vec<String> =
                        bindings.iter().map(|(name, _)| name.clone()).collect();
                    for alternative in typed_alternatives.iter().skip(1) {
                        let mut alternative_bindings = Vec::new();
                        collect_pattern_bindings(alternative, &mut alternative_bindings);
                        let found_names: Vec<String> = alternative_bindings
                            .iter()
                            .map(|(name, _)| name.clone())
                            .collect();
                        if alternative_bindings != bindings {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::PatternBindingMismatch {
                                    expected: expected_names.clone(),
                                    found: found_names,
                                },
                                span: alternative.span,
                                reason: ConstraintReason::Other(
                                    "or-pattern binding consistency".to_string(),
                                ),
                            });
                        }
                    }
                    for (name, binding_ty) in bindings {
                        self.env.define_local(name, binding_ty);
                    }
                }
                TypedPatternKind::Or(typed_alternatives)
            }
        };

        TypedPattern {
            kind,
            ty: ty.clone(),
            span: pattern.span,
        }
    }
}

fn enum_type_name(ty: &InferType) -> Option<String> {
    match ty {
        InferType::Struct(name) | InferType::Applied { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn enum_path_matches(path: &[String], enum_name: &str) -> bool {
    path.len() >= 2 && path[..path.len() - 1].join("::") == enum_name
}

fn enum_substitutions(
    ty: &InferType,
    definition: &crate::types::EnumDef,
) -> std::collections::HashMap<String, InferType> {
    let InferType::Applied { args, .. } = ty else {
        return std::collections::HashMap::new();
    };
    definition
        .type_params
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect()
}

fn contains_dynamic_type(ty: &InferType) -> bool {
    match ty {
        InferType::Dynamic => true,
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => contains_dynamic_type(inner),
        InferType::Result(ok, error) | InferType::Map(ok, error) => {
            contains_dynamic_type(ok) || contains_dynamic_type(error)
        }
        InferType::Tuple(elements) => elements.iter().any(contains_dynamic_type),
        InferType::Function { params, ret, .. } => {
            params.iter().any(contains_dynamic_type) || contains_dynamic_type(ret)
        }
        _ => false,
    }
}

fn is_sum_method(name: &str) -> bool {
    matches!(
        name,
        "unwrap"
            | "expect"
            | "unwrap_or"
            | "unwrap_or_else"
            | "ok"
            | "err"
            | "map"
            | "map_err"
            | "and_then"
            | "or_else"
            | "is_ok"
            | "is_err"
            | "is_some"
            | "is_none"
            | "ok_or"
    )
}

fn function_arity(ty: &InferType) -> Option<usize> {
    match ty {
        InferType::Function { params, .. } => Some(params.len()),
        _ => None,
    }
}

fn sum_family(ty: &InferType) -> Option<SumFamily> {
    match ty {
        InferType::Option(_) => Some(SumFamily::Option),
        InferType::Result(_, _) => Some(SumFamily::Result),
        _ => None,
    }
}

fn dynamic_type_mismatch(left: &InferType, right: &InferType) -> bool {
    match (left, right) {
        (InferType::Dynamic, InferType::Dynamic)
        | (InferType::Dynamic, InferType::Var(_))
        | (InferType::Var(_), InferType::Dynamic) => false,
        (InferType::Dynamic, _) | (_, InferType::Dynamic) => true,
        (InferType::Option(left), InferType::Option(right))
        | (InferType::Array(left), InferType::Array(right))
        | (InferType::FixedArray(left, _), InferType::FixedArray(right, _))
        | (InferType::Vec(left), InferType::Vec(right)) => dynamic_type_mismatch(left, right),
        (InferType::Result(left_ok, left_err), InferType::Result(right_ok, right_err)) => {
            dynamic_type_mismatch(left_ok, right_ok) || dynamic_type_mismatch(left_err, right_err)
        }
        (
            InferType::Function {
                params: left_params,
                ret: left_ret,
                ..
            },
            InferType::Function {
                params: right_params,
                ret: right_ret,
                ..
            },
        ) => {
            left_params
                .iter()
                .zip(right_params)
                .any(|(left, right)| dynamic_type_mismatch(left, right))
                || dynamic_type_mismatch(left_ret, right_ret)
        }
        (InferType::Tuple(left), InferType::Tuple(right)) => left
            .iter()
            .zip(right)
            .any(|(left, right)| dynamic_type_mismatch(left, right)),
        _ => false,
    }
}

fn qualified_family(callee: &Expr) -> Option<&'static str> {
    let ExprKind::Member { object, .. } = &callee.kind else {
        return None;
    };
    let ExprKind::Identifier(name) = &object.kind else {
        return None;
    };
    match name.as_str() {
        "Option" => Some("Option"),
        "Result" => Some("Result"),
        "Error" => Some("Error"),
        _ => None,
    }
}

fn is_assignment(expr: &TypedExpr) -> bool {
    match &expr.kind {
        TypedExprKind::Assign { .. }
        | TypedExprKind::MemberAssign { .. }
        | TypedExprKind::IndexAssign { .. } => true,
        TypedExprKind::Grouping(inner) => is_assignment(inner),
        _ => false,
    }
}

fn collect_pattern_bindings(pattern: &TypedPattern, bindings: &mut Vec<(String, InferType)>) {
    match &pattern.kind {
        TypedPatternKind::Binding(name) => bindings.push((name.clone(), pattern.ty.clone())),
        TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
            for field in fields {
                collect_pattern_bindings(field, bindings);
            }
        }
        TypedPatternKind::Struct { fields, .. } => {
            for (_, field, _) in fields {
                collect_pattern_bindings(field, bindings);
            }
        }
        _ => {}
    }
}

fn describe_pattern(pattern: &Pattern) -> String {
    match &pattern.kind {
        PatternKind::Wildcard => "_".to_string(),
        PatternKind::Binding(name) => name.clone(),
        PatternKind::Int(value) => value.to_string(),
        PatternKind::String(value) => format!("\"{value}\""),
        PatternKind::Char(value) => format!("{value:?}"),
        PatternKind::Bool(value) => value.to_string(),
        PatternKind::Variant { path, fields, .. } => {
            if fields.is_empty() {
                path.join("::")
            } else {
                format!("{}(..)", path.join("::"))
            }
        }
        PatternKind::Struct { path, .. } => format!("{} {{ .. }}", path.join("::")),
        PatternKind::Range {
            start,
            end,
            inclusive,
        } => format!(
            "{}{}{}",
            start.map(|value| value.to_string()).unwrap_or_default(),
            if *inclusive { "..=" } else { ".." },
            end.map(|value| value.to_string()).unwrap_or_default()
        ),
        PatternKind::Or(alternatives) => alternatives
            .iter()
            .map(describe_pattern)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn variant_family(variant: &str) -> Option<&'static str> {
    match variant {
        "Some" | "None" => Some("Option"),
        "Ok" | "Err" => Some("Result"),
        "Message" => Some("Error"),
        _ => None,
    }
}

fn sum_type_is_unresolved(ty: &InferType) -> bool {
    match ty {
        InferType::Option(inner) => type_is_unresolved(inner),
        InferType::Result(ok, error) => type_is_unresolved(ok) || type_is_unresolved(error),
        _ => false,
    }
}

fn type_is_unresolved(ty: &InferType) -> bool {
    match ty {
        InferType::Var(_) | InferType::Dynamic | InferType::UntypedNative(_) => true,
        InferType::Function { params, ret, .. } => {
            params.iter().any(type_is_unresolved) || type_is_unresolved(ret)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => type_is_unresolved(inner),
        InferType::Result(ok, error) | InferType::Map(ok, error) => {
            type_is_unresolved(ok) || type_is_unresolved(error)
        }
        InferType::Tuple(elements) => elements.iter().any(type_is_unresolved),
        _ => false,
    }
}

fn tail_is_implicit_unit(stmts: &[aelys_syntax::Stmt]) -> bool {
    use aelys_syntax::StmtKind;
    let Some(last) = stmts.last() else {
        return true;
    };
    match &last.kind {
        StmtKind::Expression(_)
        | StmtKind::Return(_)
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. } => false,
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => {
            tail_is_implicit_unit(std::slice::from_ref(then_branch.as_ref()))
                && tail_is_implicit_unit(std::slice::from_ref(else_branch.as_ref()))
        }
        StmtKind::Block(inner) => tail_is_implicit_unit(inner),
        _ => true,
    }
}
