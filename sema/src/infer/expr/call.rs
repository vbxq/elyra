use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedFmtStringPart};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, Span};

impl TypeInference {
    pub(super) fn infer_call_expr(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> (TypedExprKind, InferType) {
        if let Some(result) = self.infer_sum_constructor_call(callee, args) {
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

        let inferred_callee = self.infer_expr(callee);
        let typed_callee = self.specialize_numeric_signature(inferred_callee);
        let mut typed_args: Vec<TypedExpr> = args.iter().map(|a| self.infer_expr(a)).collect();
        let argument_indices = effective_argument_indices(&typed_args);

        let ret_type = if let InferType::UntypedNative(name) = &typed_callee.ty {
            InferType::UntypedNative(name.clone())
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
                    } else if let InferType::UntypedNative(name) = &arg.ty
                        && !matches!(param_ty, InferType::Dynamic)
                    {
                        self.errors.push(TypeError {
                            kind: TypeErrorKind::UntypedNativeTypeMismatch {
                                name: name.clone(),
                                expected: param_ty.clone(),
                            },
                            span: arg.span,
                            reason,
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

        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            ret_type,
        )
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
        let typed_object = self.infer_expr(object);
        let signature = crate::native::function_signature(&format!("string::{member}"))?;
        let InferType::Function { params, ret } = signature else {
            return None;
        };
        let receiver = params.first()?;
        if !matches!(receiver, InferType::String) || params.len() != args.len() + 1 {
            return None;
        }

        if member == "len"
            && matches!(
                &typed_object.ty,
                InferType::Array(_) | InferType::Vec(_) | InferType::Var(_)
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
                    params: vec![InferType::Dynamic; typed_args.len()],
                    ret: Box::new(InferType::Dynamic),
                },
                callee.span,
            );
            return Some((
                TypedExprKind::Call {
                    callee: Box::new(typed_callee),
                    args: typed_args,
                },
                InferType::Dynamic,
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
            if let InferType::UntypedNative(name) = &arg.ty
                && !matches!(expected, InferType::Dynamic)
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UntypedNativeTypeMismatch {
                        name: name.clone(),
                        expected: expected.clone(),
                    },
                    span: arg.span,
                    reason,
                });
                continue;
            }
            self.constraints.push(Constraint::equal(
                arg.ty.clone(),
                expected.clone(),
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
        let typed_object = self.infer_expr(object);
        if is_collection_method(member, args.len())
            && matches!(&typed_object.ty, InferType::Dynamic)
        {
            return Some(self.invalid_collection_method(typed_object, member, args, callee));
        }
        let (element, is_vec) = match &typed_object.ty {
            InferType::Array(inner) => (inner.as_ref().clone(), false),
            InferType::Vec(inner) => (inner.as_ref().clone(), true),
            InferType::Var(_) => {
                let element = self.type_gen.fresh();
                let (options, is_vec) = match member.as_str() {
                    "len" => (
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
        let output = match (member.as_str(), is_vec, args.len()) {
            ("len", _, 0) | ("capacity", true, 0) => InferType::I64,
            ("pop", true, 0) | ("get", _, 1) => InferType::Option(Box::new(element.clone())),
            ("push", true, 1) | ("reserve", true, 1) => InferType::Unit,
            _ => return None,
        };
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        let expected_args = match member.as_str() {
            "get" => vec![InferType::I64],
            "push" => vec![element.clone()],
            "reserve" => vec![InferType::I64],
            _ => Vec::new(),
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
}

fn is_collection_method(member: &str, argument_count: usize) -> bool {
    matches!(
        (member, argument_count),
        ("len" | "capacity" | "pop", 0) | ("push" | "get" | "reserve", 1)
    )
}

fn contains_numeric(ty: &InferType) -> bool {
    match ty {
        InferType::Numeric => true,
        InferType::Function { params, ret } => {
            params.iter().any(contains_numeric) || contains_numeric(ret)
        }
        InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
            contains_numeric(inner)
        }
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
