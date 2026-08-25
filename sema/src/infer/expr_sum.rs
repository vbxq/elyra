use super::TypeInference;
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
        for residual in &self.match_exhaustivity_residuals {
            let ty = subst.apply(&residual.ty);
            let mut covering: Vec<&Pattern> = Vec::new();
            for arm in &residual.arms {
                if !covering.is_empty() && self.pattern_is_unreachable(&ty, &covering, &arm.pattern)
                {
                    reported.push(TypeError {
                        kind: TypeErrorKind::UnreachablePattern {
                            pattern: describe_pattern(&arm.pattern),
                        },
                        span: arm.span,
                        reason: ConstraintReason::Other("match arm reachability".to_string()),
                    });
                }
                if !arm.guarded {
                    covering.push(&arm.pattern);
                }
            }
        }
        self.errors.extend(reported);
    }

    pub(super) fn validate_match_exhaustivity(&mut self, subst: &Substitution) {
        for residual in &self.match_exhaustivity_residuals {
            let ty = subst.apply(&residual.ty);
            if matches!(ty, InferType::Poison) {
                continue;
            }
            let patterns: Vec<&Pattern> = residual
                .arms
                .iter()
                .filter(|arm| !arm.guarded)
                .map(|arm| &arm.pattern)
                .collect();
            if enum_type_name(&ty).is_some_and(|name| self.type_table.get_enum(&name).is_some()) {
                let missing = self.enum_missing_patterns(&ty, &patterns);
                if !missing.is_empty() {
                    self.errors.insert(
                        0,
                        TypeError {
                            kind: TypeErrorKind::NonExhaustiveMatch { missing },
                            span: residual.span,
                            reason: ConstraintReason::Other("enum match exhaustivity".to_string()),
                        },
                    );
                }
                continue;
            }
            if let InferType::Struct(name) = &ty {
                if !self.struct_patterns_cover(name, &patterns) {
                    self.errors.insert(
                        0,
                        TypeError {
                            kind: TypeErrorKind::NonExhaustiveStruct {
                                structure: name.clone(),
                            },
                            span: residual.span,
                            reason: ConstraintReason::Other(
                                "struct match exhaustivity".to_string(),
                            ),
                        },
                    );
                }
                continue;
            }
            let missing = missing_patterns(&ty, &patterns);
            if !missing.is_empty() {
                self.errors.insert(
                    0,
                    TypeError {
                        kind: TypeErrorKind::NonExhaustiveMatch { missing },
                        span: residual.span,
                        reason: ConstraintReason::Other("match exhaustivity".to_string()),
                    },
                );
            }
        }
    }

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
                    params,
                    ret: Box::new(value_type.clone()),
                });
                value_type.clone()
            }
            "ok" if family == "Result" => InferType::Option(Box::new(value_type.clone())),
            "err" if family == "Result" => {
                InferType::Option(Box::new(error_type.clone().unwrap_or(InferType::Dynamic)))
            }
            "map" => {
                let mapped = self.type_gen.fresh();
                expected_args.push(InferType::Function {
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
                    params: vec![error_type.clone().unwrap_or(InferType::Dynamic)],
                    ret: Box::new(mapped.clone()),
                });
                InferType::Result(Box::new(value_type.clone()), Box::new(mapped))
            }
            "and_then" => {
                let mapped = self.type_gen.fresh();
                expected_args.push(InferType::Function {
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
                        params: Vec::new(),
                        ret: Box::new(InferType::Option(Box::new(value_type.clone()))),
                    });
                    InferType::Option(Box::new(value_type.clone()))
                } else {
                    expected_args.push(InferType::Function {
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
                    arg.ty.clone(),
                    expected.clone(),
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
                    params,
                    ret: Box::new(value.clone()),
                });
                value.clone()
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
                    arg.ty.clone(),
                    expected.clone(),
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
            InferType::Function {
                params: expected_args,
                ret: Box::new(output.clone()),
            },
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
            InferType::Function {
                params: vec![InferType::Dynamic; typed_args.len()],
                ret: Box::new(InferType::Dynamic),
            },
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
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
                            params,
                            ret: Box::new(residual.value.clone()),
                        },
                    );
                }
                Some(residual.value.clone())
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
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
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
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
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
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
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
                    unify_field(
                        self,
                        callback,
                        &InferType::Function {
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
                            arg_types[0].clone(),
                            InferType::String,
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

        let callee_type = InferType::Function {
            params: arg_types,
            ret: Box::new(output.clone()),
        };
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
                kind: TypeErrorKind::QuestionMarkOutsideResult,
                span,
                reason: ConstraintReason::Other("question mark".to_string()),
            });
            return (
                TypedExprKind::Try {
                    operand: Box::new(typed_inner),
                    conversion: None,
                },
                InferType::Dynamic,
            );
        };

        let output = self.type_gen.fresh();
        let target = match (&source, &current_return) {
            (InferType::Option(source_value), InferType::Var(_)) => {
                let target = InferType::Option(Box::new(output.clone()));
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
                    Box::new(output.clone()),
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
            (InferType::Option(_), InferType::Option(target_value)) => {
                self.constraints.push(Constraint::equal(
                    output.clone(),
                    target_value.as_ref().clone(),
                    span,
                    ConstraintReason::Other("question mark value".to_string()),
                ));
                current_return.clone()
            }
            (InferType::Result(_, _), InferType::Result(target_value, _)) => {
                self.constraints.push(Constraint::equal(
                    output.clone(),
                    target_value.as_ref().clone(),
                    span,
                    ConstraintReason::Other("question mark value".to_string()),
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
            (_, _) => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkOutsideResult,
                    span,
                    reason: ConstraintReason::Other("question mark".to_string()),
                });
                current_return.clone()
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
                (InferType::Option(source_value), InferType::Option(target_value)) => {
                    if source_value != target_value {
                        errors.push(TypeError {
                            kind: TypeErrorKind::QuestionMarkTypeMismatch { source, target },
                            span: residual.span,
                            reason: ConstraintReason::Other("question mark conversion".to_string()),
                        });
                    }
                }
                (
                    InferType::Result(source_value, source_error),
                    InferType::Result(target_value, target_error),
                ) => {
                    if source_value != target_value {
                        errors.push(TypeError {
                            kind: TypeErrorKind::QuestionMarkTypeMismatch {
                                source: source.clone(),
                                target: target.clone(),
                            },
                            span: residual.span,
                            reason: ConstraintReason::Other("question mark conversion".to_string()),
                        });
                        continue;
                    }
                    match self
                        .type_table
                        .select_from_conversion(source_error, target_error)
                    {
                        crate::types::FromSelection::Identity => {}
                        crate::types::FromSelection::Selected(symbol) => {
                            conversions.push((residual.span, symbol));
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
        for (span, symbol) in conversions {
            self.try_conversions.insert((span.start, span.end), symbol);
        }
    }

    pub(super) fn infer_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_scrutinee = self.infer_expr(scrutinee);
        let result_type = self.type_gen.fresh();
        let mut dynamic_result = false;
        let mut typed_arms = Vec::with_capacity(arms.len());

        for arm in arms {
            self.env.push_scope();
            let typed_pattern = self.infer_pattern(&arm.pattern, &typed_scrutinee.ty);
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
                        typed_guard.ty.clone(),
                        InferType::Bool,
                        guard.span,
                        ConstraintReason::IfCondition,
                    ));
                }
                typed_guard
            });
            let typed_body = match &arm.body {
                MatchArmBody::Expr(expr) => {
                    let typed_expr = self.infer_expr(expr);
                    dynamic_result |= matches!(typed_expr.ty, InferType::Dynamic);
                    if !self.reject_dynamic(
                        &typed_expr.ty,
                        &result_type,
                        expr.span,
                        ConstraintReason::IfBranches,
                    ) {
                        self.constraints.push(Constraint::equal(
                            typed_expr.ty.clone(),
                            result_type.clone(),
                            expr.span,
                            ConstraintReason::IfBranches,
                        ));
                    }
                    TypedMatchArmBody::Expr(typed_expr)
                }
                MatchArmBody::Block(stmts) => {
                    if stmts.is_empty() {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::MatchArmValueRequired,
                            span: arm.span,
                            reason: ConstraintReason::Other("empty match arm".to_string()),
                        });
                    }
                    let typed_stmts = if stmts.is_empty() {
                        Vec::new()
                    } else {
                        let mut typed_stmts: Vec<_> = stmts[..stmts.len() - 1]
                            .iter()
                            .map(|stmt| self.infer_stmt(stmt))
                            .collect();
                        typed_stmts.push(self.infer_stmt_with_implicit_return(
                            &stmts[stmts.len() - 1],
                            &result_type,
                        ));
                        typed_stmts
                    };
                    let body_type = typed_stmts.last().and_then(|stmt| match &stmt.kind {
                        crate::typed_ast::TypedStmtKind::Expression(expr) => Some(expr.ty.clone()),
                        crate::typed_ast::TypedStmtKind::Return(_)
                        | crate::typed_ast::TypedStmtKind::Break
                        | crate::typed_ast::TypedStmtKind::Continue => None,
                        _ => {
                            self.errors.push(TypeError {
                                kind: TypeErrorKind::MatchArmValueRequired,
                                span: stmt.span,
                                reason: ConstraintReason::Other("match arm tail value".to_string()),
                            });
                            None
                        }
                    });
                    dynamic_result |= matches!(body_type, Some(InferType::Dynamic));
                    if let Some(body_type) = body_type {
                        self.constraints.push(Constraint::equal(
                            body_type,
                            result_type.clone(),
                            arm.span,
                            ConstraintReason::IfBranches,
                        ));
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

        self.match_exhaustivity_residuals
            .push(MatchExhaustivityResidual {
                ty: typed_scrutinee.ty.clone(),
                arms: arms
                    .iter()
                    .map(|arm| MatchArmCoverage {
                        pattern: arm.pattern.clone(),
                        guarded: arm.guard.is_some(),
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
                self.constraints.push(Constraint::equal(
                    ty.clone(),
                    InferType::I64,
                    pattern.span,
                    ConstraintReason::Comparison,
                ));
                TypedPatternKind::Int(*value)
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
                    for (offset, expected_field) in expected.iter().enumerate() {
                        let Some(field) = fields
                            .iter()
                            .find(|candidate| candidate.name == expected_field.name)
                        else {
                            if !*has_rest {
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
                        typed_fields.push(self.infer_pattern(&field.pattern, &expected_field.ty));
                        field_offsets.push(u16::try_from(offset).unwrap_or(u16::MAX));
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
                let Some(def) = self.type_table.get_struct(&name).cloned() else {
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
                if !matches!(ty, InferType::Struct(actual) if actual == &name) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::Mismatch {
                            expected: InferType::Struct(name.clone()),
                            found: ty.clone(),
                        },
                        span: pattern.span,
                        reason: ConstraintReason::Other("struct pattern type".to_string()),
                    });
                }
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
                    typed_fields.push((
                        field.name.clone(),
                        self.infer_pattern(&field.pattern, &field_def.ty),
                        def.fields
                            .iter()
                            .position(|candidate| candidate.name == field.name)
                            .and_then(|offset| u16::try_from(offset).ok())
                            .unwrap_or(0),
                    ));
                }
                if !*has_rest && typed_fields.len() != def.fields.len() {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::NonExhaustiveStruct {
                            structure: name.clone(),
                        },
                        span: pattern.span,
                        reason: ConstraintReason::Other("struct pattern fields".to_string()),
                    });
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

    fn struct_patterns_cover(&self, name: &str, patterns: &[&Pattern]) -> bool {
        self.patterns_cover_type(&InferType::Struct(name.to_string()), patterns)
    }

    fn enum_missing_patterns(&self, ty: &InferType, patterns: &[&Pattern]) -> Vec<String> {
        let Some(name) = enum_type_name(ty) else {
            return vec!["_".to_string()];
        };
        let Some(def) = self.type_table.get_enum(&name) else {
            return vec!["_".to_string()];
        };
        let substitutions = enum_substitutions(ty, def);
        if patterns.iter().any(|pattern| pattern_covers_all(pattern)) {
            return Vec::new();
        }
        def.variants
            .iter()
            .filter_map(|variant| {
                if self.enum_patterns_cover_variant(&name, variant, patterns, &substitutions) {
                    None
                } else {
                    Some(enum_variant_pattern(&name, variant))
                }
            })
            .collect()
    }

    fn enum_patterns_cover_variant(
        &self,
        name: &str,
        variant: &crate::types::EnumVariantDef,
        patterns: &[&Pattern],
        substitutions: &std::collections::HashMap<String, InferType>,
    ) -> bool {
        if patterns.iter().any(|pattern| pattern_covers_all(pattern)) {
            return true;
        }
        match &variant.fields {
            crate::types::EnumVariantFieldsDef::Unit => patterns.iter().any(|pattern| {
                self.enum_pattern_covers_variant(name, variant, pattern, substitutions)
            }),
            crate::types::EnumVariantFieldsDef::Tuple(expected) => {
                let expected: Vec<InferType> = expected
                    .iter()
                    .map(|ty| ty.substitute_params(substitutions))
                    .collect();
                let mut rows = Vec::new();
                for pattern in patterns {
                    collect_enum_tuple_rows(pattern, name, &variant.name, &mut rows);
                }
                !rows.is_empty() && self.tuple_patterns_cover_product(&expected, &rows)
            }
            _ => patterns.iter().any(|pattern| {
                self.enum_pattern_covers_variant(name, variant, pattern, substitutions)
            }),
        }
    }

    fn enum_pattern_covers_variant(
        &self,
        name: &str,
        variant: &crate::types::EnumVariantDef,
        pattern: &Pattern,
        substitutions: &std::collections::HashMap<String, InferType>,
    ) -> bool {
        match &pattern.kind {
            PatternKind::Variant { path, fields, .. }
                if path.first().is_some_and(|segment| segment == name)
                    && path.last().is_some_and(|segment| segment == &variant.name) =>
            {
                match &variant.fields {
                    crate::types::EnumVariantFieldsDef::Unit => fields.is_empty(),
                    crate::types::EnumVariantFieldsDef::Tuple(expected) => {
                        fields.len() == expected.len()
                            && fields.iter().zip(expected).all(|(field, expected)| {
                                let expected = expected.substitute_params(substitutions);
                                self.pattern_covers_type(field, &expected)
                            })
                    }
                    crate::types::EnumVariantFieldsDef::Named(_) => false,
                }
            }
            PatternKind::Struct {
                path,
                fields,
                has_rest,
                ..
            } if path.first().is_some_and(|segment| segment == name)
                && path.last().is_some_and(|segment| segment == &variant.name) =>
            {
                let crate::types::EnumVariantFieldsDef::Named(expected) = &variant.fields else {
                    return false;
                };
                fields.len() <= expected.len()
                    && (*has_rest || fields.len() == expected.len())
                    && fields.iter().all(|field| {
                        expected
                            .iter()
                            .find(|candidate| candidate.name == field.name)
                            .is_some_and(|expected| {
                                let expected = expected.ty.substitute_params(substitutions);
                                self.pattern_covers_type(&field.pattern, &expected)
                            })
                    })
            }
            PatternKind::Or(alternatives) => alternatives.iter().any(|alternative| {
                self.enum_pattern_covers_variant(name, variant, alternative, substitutions)
            }),
            _ => false,
        }
    }

    fn pattern_is_unreachable(
        &self,
        ty: &InferType,
        covering: &[&Pattern],
        candidate: &Pattern,
    ) -> bool {
        if covering.iter().any(|pattern| pattern_covers_all(pattern)) {
            return true;
        }
        match &candidate.kind {
            PatternKind::Wildcard | PatternKind::Binding(_) => {
                self.patterns_cover_type(ty, covering)
            }
            PatternKind::Or(alternatives) => alternatives
                .iter()
                .all(|alternative| self.pattern_is_unreachable(ty, covering, alternative)),
            PatternKind::Bool(value) => covering
                .iter()
                .any(|pattern| pattern_covers_literal(pattern, *value)),
            PatternKind::Int(_) | PatternKind::String(_) => covering
                .iter()
                .any(|pattern| literal_patterns_match(pattern, candidate)),
            _ => self.constructor_is_covered(ty, covering, candidate),
        }
    }

    fn constructor_is_covered(
        &self,
        ty: &InferType,
        covering: &[&Pattern],
        candidate: &Pattern,
    ) -> bool {
        let Some(variant_name) = pattern_variant_name(candidate) else {
            return false;
        };
        if let Some(name) = enum_type_name(ty)
            && let Some(def) = self.type_table.get_enum(&name)
        {
            let Some(variant) = def
                .variants
                .iter()
                .find(|variant| variant.name == variant_name)
            else {
                return false;
            };
            let substitutions = enum_substitutions(ty, def);
            return self.enum_patterns_cover_variant(&name, variant, covering, &substitutions);
        }
        match ty {
            InferType::Option(inner) => match variant_name.as_str() {
                "None" => covering
                    .iter()
                    .any(|pattern| pattern_covers_variant(pattern, "None", None, inner)),
                "Some" => self.payload_is_covered(covering, "Some", inner),
                _ => false,
            },
            InferType::Result(ok, error) => match variant_name.as_str() {
                "Ok" => self.payload_is_covered(covering, "Ok", ok),
                "Err" => self.payload_is_covered(covering, "Err", error),
                _ => false,
            },
            InferType::Struct(_) => self.patterns_cover_type(ty, covering),
            _ => false,
        }
    }

    fn payload_is_covered(&self, covering: &[&Pattern], variant: &str, inner: &InferType) -> bool {
        let mut fields = Vec::new();
        for pattern in covering {
            collect_variant_fields(pattern, variant, &mut fields);
        }
        !fields.is_empty() && missing_patterns(inner, &fields).is_empty()
    }

    fn patterns_cover_type(&self, ty: &InferType, patterns: &[&Pattern]) -> bool {
        if patterns.iter().any(|pattern| pattern_covers_all(pattern)) {
            return true;
        }
        if let Some(name) = enum_type_name(ty)
            && let Some(def) = self.type_table.get_enum(&name)
        {
            let substitutions = enum_substitutions(ty, def);
            return def.variants.iter().all(|variant| {
                self.enum_patterns_cover_variant(&name, variant, patterns, &substitutions)
            });
        }
        patterns
            .iter()
            .any(|pattern| self.pattern_covers_type(pattern, ty))
    }

    fn pattern_covers_type(&self, pattern: &Pattern, ty: &InferType) -> bool {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Binding(_) => true,
            PatternKind::Or(alternatives) => {
                let alternatives: Vec<&Pattern> = alternatives.iter().collect();
                self.patterns_cover_type(ty, &alternatives)
            }
            _ => match ty {
                ty if enum_type_name(ty)
                    .is_some_and(|name| self.type_table.get_enum(&name).is_some()) =>
                {
                    let Some(name) = enum_type_name(ty) else {
                        return false;
                    };
                    let Some(def) = self.type_table.get_enum(&name) else {
                        return false;
                    };
                    let substitutions = enum_substitutions(ty, def);
                    def.variants.iter().all(|variant| {
                        self.enum_pattern_covers_variant(&name, variant, pattern, &substitutions)
                    })
                }
                InferType::Struct(name) => self.struct_pattern_covers_type(name, pattern),
                _ => self.pattern_is_irrefutable(pattern),
            },
        }
    }

    fn tuple_patterns_cover_product(&self, types: &[InferType], rows: &[Vec<&Pattern>]) -> bool {
        if types.is_empty() {
            return !rows.is_empty();
        }
        let Some(first_type) = types.first() else {
            return false;
        };
        if let Some(name) = enum_type_name(first_type)
            && let Some(definition) = self.type_table.get_enum(&name)
        {
            let substitutions = enum_substitutions(first_type, definition);
            return definition.variants.iter().all(|variant| {
                let matching_rows: Vec<Vec<&Pattern>> = rows
                    .iter()
                    .filter_map(|row| {
                        let first = row.first()?;
                        (pattern_covers_all(first)
                            || self.enum_pattern_covers_variant(
                                &name,
                                variant,
                                first,
                                &substitutions,
                            ))
                        .then(|| row[1..].to_vec())
                    })
                    .collect();
                self.tuple_patterns_cover_product(&types[1..], &matching_rows)
            });
        }
        let suffixes: Vec<Vec<&Pattern>> = rows
            .iter()
            .filter_map(|row| {
                let first = row.first()?;
                self.pattern_covers_type(first, first_type)
                    .then(|| row[1..].to_vec())
            })
            .collect();
        self.tuple_patterns_cover_product(&types[1..], &suffixes)
    }

    fn struct_pattern_covers_type(&self, name: &str, pattern: &Pattern) -> bool {
        let PatternKind::Struct {
            path,
            fields,
            has_rest,
            ..
        } = &pattern.kind
        else {
            return false;
        };
        if path.last().is_none_or(|candidate| candidate != name) {
            return false;
        }
        let Some(def) = self.type_table.get_struct(name) else {
            return false;
        };
        fields.len() <= def.fields.len()
            && (*has_rest || fields.len() == def.fields.len())
            && fields.iter().all(|field| {
                def.fields
                    .iter()
                    .find(|candidate| candidate.name == field.name)
                    .is_some_and(|expected| self.pattern_covers_type(&field.pattern, &expected.ty))
            })
    }

    fn pattern_is_irrefutable(&self, pattern: &Pattern) -> bool {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Binding(_) => true,
            PatternKind::Struct {
                path,
                fields,
                has_rest,
                ..
            } => {
                let Some(name) = path.last() else {
                    return false;
                };
                if let Some(enum_name) = path.first()
                    && let Some(enum_def) = self.type_table.get_enum(enum_name)
                    && let Some(variant) = enum_def
                        .variants
                        .iter()
                        .find(|variant| variant.name == *name)
                {
                    let crate::types::EnumVariantFieldsDef::Named(expected) = &variant.fields
                    else {
                        return false;
                    };
                    return fields.len() <= expected.len()
                        && (*has_rest || fields.len() == expected.len())
                        && fields
                            .iter()
                            .all(|field| self.pattern_is_irrefutable(&field.pattern));
                }
                let Some(def) = self.type_table.get_struct(name) else {
                    return false;
                };
                fields
                    .iter()
                    .all(|field| self.pattern_is_irrefutable(&field.pattern))
                    && (*has_rest || fields.len() == def.fields.len())
            }
            PatternKind::Or(alternatives) => alternatives
                .iter()
                .any(|alternative| self.pattern_is_irrefutable(alternative)),
            _ => false,
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

fn enum_variant_pattern(name: &str, variant: &crate::types::EnumVariantDef) -> String {
    match &variant.fields {
        crate::types::EnumVariantFieldsDef::Unit => format!("{name}::{}", variant.name),
        crate::types::EnumVariantFieldsDef::Tuple(_) => {
            format!("{name}::{}(..)", variant.name)
        }
        crate::types::EnumVariantFieldsDef::Named(_) => {
            format!("{name}::{} {{ .. }}", variant.name)
        }
    }
}

fn contains_dynamic_type(ty: &InferType) -> bool {
    match ty {
        InferType::Dynamic => true,
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => contains_dynamic_type(inner),
        InferType::Result(ok, error) => contains_dynamic_type(ok) || contains_dynamic_type(error),
        InferType::Tuple(elements) => elements.iter().any(contains_dynamic_type),
        InferType::Function { params, ret } => {
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
            },
            InferType::Function {
                params: right_params,
                ret: right_ret,
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

fn missing_patterns(ty: &InferType, patterns: &[&Pattern]) -> Vec<String> {
    if patterns.iter().any(|pattern| pattern_covers_all(pattern)) {
        return Vec::new();
    }

    match ty {
        InferType::Bool => {
            let true_covered = patterns
                .iter()
                .any(|pattern| pattern_covers_literal(pattern, true));
            let false_covered = patterns
                .iter()
                .any(|pattern| pattern_covers_literal(pattern, false));
            let mut missing = Vec::new();
            if !true_covered {
                missing.push("true".to_string());
            }
            if !false_covered {
                missing.push("false".to_string());
            }
            missing
        }
        InferType::Option(inner) => {
            let none_covered = patterns
                .iter()
                .any(|pattern| pattern_covers_variant(pattern, "None", None, inner));
            let mut some_patterns = Vec::new();
            for pattern in patterns {
                collect_variant_fields(pattern, "Some", &mut some_patterns);
            }
            let mut missing = Vec::new();
            if !none_covered {
                missing.push("None".to_string());
            }
            if !some_patterns.is_empty() {
                if !missing_patterns(inner, &some_patterns).is_empty() {
                    missing.push("Some(_)".to_string());
                }
            } else {
                missing.push("Some(_)".to_string());
            }
            missing
        }
        InferType::Result(ok, err) => {
            let mut ok_patterns = Vec::new();
            let mut err_patterns = Vec::new();
            for pattern in patterns {
                collect_variant_fields(pattern, "Ok", &mut ok_patterns);
                collect_variant_fields(pattern, "Err", &mut err_patterns);
            }
            let mut missing = Vec::new();
            if ok_patterns.is_empty() || !missing_patterns(ok, &ok_patterns).is_empty() {
                missing.push("Ok(_)".to_string());
            }
            if err_patterns.is_empty() || !missing_patterns(err, &err_patterns).is_empty() {
                missing.push("Err(_)".to_string());
            }
            missing
        }
        InferType::Error => {
            if patterns.iter().any(|pattern| {
                pattern_covers_variant(
                    pattern,
                    "Message",
                    Some(&Pattern {
                        kind: PatternKind::Wildcard,
                        span: Span::dummy(),
                    }),
                    &InferType::String,
                )
            }) {
                Vec::new()
            } else {
                vec!["Message(_)".to_string()]
            }
        }
        _ => vec!["_".to_string()],
    }
}

fn pattern_variant_name(pattern: &Pattern) -> Option<String> {
    match &pattern.kind {
        PatternKind::Variant { path, .. } | PatternKind::Struct { path, .. } => {
            path.last().cloned()
        }
        _ => None,
    }
}

fn literal_patterns_match(pattern: &Pattern, candidate: &Pattern) -> bool {
    match (&pattern.kind, &candidate.kind) {
        (PatternKind::Int(left), PatternKind::Int(right)) => left == right,
        (PatternKind::String(left), PatternKind::String(right)) => left == right,
        (PatternKind::Or(alternatives), _) => alternatives
            .iter()
            .any(|alternative| literal_patterns_match(alternative, candidate)),
        _ => false,
    }
}

fn describe_pattern(pattern: &Pattern) -> String {
    match &pattern.kind {
        PatternKind::Wildcard => "_".to_string(),
        PatternKind::Binding(name) => name.clone(),
        PatternKind::Int(value) => value.to_string(),
        PatternKind::String(value) => format!("\"{value}\""),
        PatternKind::Bool(value) => value.to_string(),
        PatternKind::Variant { path, fields, .. } => {
            if fields.is_empty() {
                path.join("::")
            } else {
                format!("{}(..)", path.join("::"))
            }
        }
        PatternKind::Struct { path, .. } => format!("{} {{ .. }}", path.join("::")),
        PatternKind::Or(alternatives) => alternatives
            .iter()
            .map(describe_pattern)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn pattern_covers_all(pattern: &Pattern) -> bool {
    match &pattern.kind {
        PatternKind::Wildcard | PatternKind::Binding(_) => true,
        PatternKind::Or(alternatives) => alternatives.iter().any(pattern_covers_all),
        _ => false,
    }
}

fn pattern_covers_literal(pattern: &Pattern, value: bool) -> bool {
    match &pattern.kind {
        PatternKind::Bool(other) => *other == value,
        PatternKind::Or(alternatives) => alternatives
            .iter()
            .any(|alternative| pattern_covers_literal(alternative, value)),
        _ => false,
    }
}

fn collect_variant_fields<'a>(
    pattern: &'a Pattern,
    variant: &str,
    collected: &mut Vec<&'a Pattern>,
) {
    match &pattern.kind {
        PatternKind::Variant {
            path,
            fields: payloads,
            ..
        } if path.last().is_some_and(|name| name == variant) => {
            if let Some(field) = payloads.first() {
                collected.push(field);
            }
        }
        PatternKind::Or(alternatives) => {
            for alternative in alternatives {
                collect_variant_fields(alternative, variant, collected);
            }
        }
        _ => {}
    }
}

fn collect_enum_tuple_rows<'a>(
    pattern: &'a Pattern,
    enum_name: &str,
    variant: &str,
    collected: &mut Vec<Vec<&'a Pattern>>,
) {
    match &pattern.kind {
        PatternKind::Variant { path, fields, .. }
            if path.first().is_some_and(|name| name == enum_name)
                && path.last().is_some_and(|name| name == variant) =>
        {
            collected.push(fields.iter().collect());
        }
        PatternKind::Or(alternatives) => {
            for alternative in alternatives {
                collect_enum_tuple_rows(alternative, enum_name, variant, collected);
            }
        }
        _ => {}
    }
}

fn pattern_covers_variant(
    pattern: &Pattern,
    variant: &str,
    field: Option<&Pattern>,
    field_ty: &InferType,
) -> bool {
    match &pattern.kind {
        PatternKind::Variant { path, fields, .. }
            if path.last().is_some_and(|name| name == variant) =>
        {
            match (field, fields.first()) {
                (None, None) => true,
                (Some(_), Some(inner)) => missing_patterns(field_ty, &[inner]).is_empty(),
                _ => false,
            }
        }
        PatternKind::Or(alternatives) => alternatives
            .iter()
            .any(|alternative| pattern_covers_variant(alternative, variant, field, field_ty)),
        _ => false,
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
        InferType::Function { params, ret } => {
            params.iter().any(type_is_unresolved) || type_is_unresolved(ret)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => type_is_unresolved(inner),
        InferType::Result(ok, error) => type_is_unresolved(ok) || type_is_unresolved(error),
        InferType::Tuple(elements) => elements.iter().any(type_is_unresolved),
        _ => false,
    }
}
