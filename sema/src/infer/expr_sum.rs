use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedMatchArm, TypedMatchArmBody, TypedPattern, TypedPatternKind,
};
use crate::types::InferType;
use crate::unify::Substitution;
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

pub(super) struct SumTypeResidual {
    pub constructor: String,
    pub ty: InferType,
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
        let typed_object = self.infer_expr(object);
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
        let (family, value_type, error_type) = match &typed_object.ty {
            InferType::Option(value) => ("Option", value.as_ref().clone(), None),
            InferType::Result(value, error) => (
                "Result",
                value.as_ref().clone(),
                Some(error.as_ref().clone()),
            ),
            _ => return None,
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
                TypedExprKind::Try(Box::new(typed_inner)),
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
        (TypedExprKind::Try(Box::new(typed_inner)), output)
    }

    pub(super) fn validate_try_residuals(&mut self, subst: &Substitution) {
        for residual in &self.try_residuals {
            let source = subst.apply(&residual.source);
            let target = subst.apply(&residual.target);
            let valid = !contains_dynamic_type(&source)
                && !contains_dynamic_type(&target)
                && match (&source, &target) {
                    (InferType::Option(source_value), InferType::Option(target_value)) => {
                        source_value == target_value
                    }
                    (
                        InferType::Result(source_value, source_error),
                        InferType::Result(target_value, target_error),
                    ) => {
                        source_value == target_value
                            && (source_error == target_error
                                || (*source_error.as_ref() == InferType::String
                                    && *target_error.as_ref() == InferType::Error))
                    }
                    _ => false,
                };
            if !valid {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::QuestionMarkTypeMismatch { source, target },
                    span: residual.span,
                    reason: ConstraintReason::Other("question mark conversion".to_string()),
                });
            }
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
            self.env.pop_scope();
            typed_arms.push(TypedMatchArm {
                pattern: typed_pattern,
                guard: typed_guard,
                body: typed_body,
                span: arm.span,
            });
        }

        let unguarded: Vec<&Pattern> = arms
            .iter()
            .filter(|arm| arm.guard.is_none())
            .map(|arm| &arm.pattern)
            .collect();
        let missing = missing_patterns(&typed_scrutinee.ty, &unguarded);
        if !missing.is_empty() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NonExhaustiveMatch { missing },
                span,
                reason: ConstraintReason::Other("match exhaustivity".to_string()),
            });
        }

        (
            TypedExprKind::Match {
                scrutinee: Box::new(typed_scrutinee),
                arms: typed_arms,
            },
            result_type,
        )
    }

    fn infer_pattern(&mut self, pattern: &Pattern, ty: &InferType) -> TypedPattern {
        let kind = match &pattern.kind {
            PatternKind::Wildcard => TypedPatternKind::Wildcard,
            PatternKind::Binding(name) => {
                self.env.define_local(name.clone(), ty.clone());
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
            PatternKind::Variant { path, fields } => {
                let variant = path.last().cloned().unwrap_or_default();
                let expected_family = match ty {
                    InferType::Option(_) => Some("Option"),
                    InferType::Result(_, _) => Some("Result"),
                    InferType::Error => Some("Error"),
                    _ => None,
                };
                if path.len() > 1 && expected_family.is_some_and(|family| path[0] != family) {
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
                    fields: typed_fields,
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

fn contains_dynamic_type(ty: &InferType) -> bool {
    match ty {
        InferType::Dynamic => true,
        InferType::Option(inner) | InferType::Array(inner) | InferType::Vec(inner) => {
            contains_dynamic_type(inner)
        }
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

fn pattern_covers_variant(
    pattern: &Pattern,
    variant: &str,
    field: Option<&Pattern>,
    field_ty: &InferType,
) -> bool {
    match &pattern.kind {
        PatternKind::Variant { path, fields }
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
        InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
            type_is_unresolved(inner)
        }
        InferType::Result(ok, error) => type_is_unresolved(ok) || type_is_unresolved(error),
        InferType::Tuple(elements) => elements.iter().any(type_is_unresolved),
        _ => false,
    }
}
