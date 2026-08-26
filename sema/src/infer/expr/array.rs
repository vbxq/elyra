use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind};
use crate::types::{InferType, ResolvedType};
use aelys_syntax::{BinaryOp, Expr, ExprKind, Span, TypeAnnotation, UnaryOp};

impl TypeInference {
    pub(super) fn infer_array_literal(
        &mut self,
        element_type: &Option<TypeAnnotation>,
        elements: &[Expr],
        repeat: Option<&Expr>,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_elements: Vec<TypedExpr> = elements.iter().map(|e| self.infer_expr(e)).collect();
        let typed_repeat = repeat.map(|count| self.infer_repeat_count(count));

        let (elem_ty, resolved_elem) = if let Some(ann) = element_type {
            let ty = self.type_from_annotation(ann);
            let resolved = ResolvedType::from_infer_type(&ty);
            (ty, Some(resolved))
        } else if typed_elements.is_empty() {
            (self.type_gen.fresh(), None)
        } else {
            let first_ty = typed_elements[0].ty.clone();
            for elem in typed_elements.iter().skip(1) {
                self.constraints.push(Constraint::equal(
                    elem.ty.clone(),
                    first_ty.clone(),
                    elem.span,
                    ConstraintReason::ArrayElement,
                ));
            }
            (first_ty, None)
        };

        for element in &typed_elements {
            let reason = ConstraintReason::ArrayElement;
            if self.reject_dynamic(&element.ty, &elem_ty, element.span, reason.clone()) {
                continue;
            }
            if !self.reject_untyped_native(&element.ty, &elem_ty, element.span, reason.clone()) {
                self.constraints.push(Constraint::equal(
                    element.ty.clone(),
                    elem_ty.clone(),
                    element.span,
                    reason,
                ));
            }
        }

        let result_type = match repeat.and_then(constant_int_value) {
            Some(value) if value >= 0 => usize::try_from(value)
                .map(|length| InferType::FixedArray(Box::new(elem_ty.clone()), length))
                .unwrap_or_else(|_| InferType::Array(Box::new(elem_ty.clone()))),
            Some(_) => InferType::Array(Box::new(elem_ty.clone())),
            None if repeat.is_some() => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NonConstantArrayRepeat,
                    span: repeat.map(|expr| expr.span).unwrap_or(_span),
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Array(Box::new(elem_ty.clone()))
            }
            None => InferType::FixedArray(Box::new(elem_ty.clone()), elements.len()),
        };

        (
            TypedExprKind::ArrayLiteral {
                element_type: resolved_elem,
                elements: typed_elements,
                repeat: typed_repeat.map(Box::new),
            },
            result_type,
        )
    }

    pub(super) fn infer_array_sized(
        &mut self,
        element_type: &Option<TypeAnnotation>,
        size: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_size = self.infer_expr(size);

        if let Some(value) = constant_int_value(size)
            && value < 0
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NegativeArraySize { size: value },
                span: size.span,
                reason: ConstraintReason::ArrayIndex,
            });
        }

        if !self.reject_dynamic(
            &typed_size.ty,
            &InferType::I64,
            size.span,
            ConstraintReason::ArrayIndex,
        ) && !self.reject_untyped_native(
            &typed_size.ty,
            &InferType::I64,
            size.span,
            ConstraintReason::ArrayIndex,
        ) {
            self.constraints.push(Constraint::equal(
                typed_size.ty.clone(),
                InferType::I64,
                span,
                ConstraintReason::ArrayIndex,
            ));
        }

        let (elem_ty, resolved_elem) = if let Some(ann) = element_type {
            let ty = self.type_from_annotation(ann);
            let resolved = ResolvedType::from_infer_type(&ty);
            (ty, Some(resolved))
        } else {
            (InferType::Dynamic, None)
        };

        if !matches!(
            elem_ty,
            InferType::I8
                | InferType::I16
                | InferType::I32
                | InferType::I64
                | InferType::U8
                | InferType::U16
                | InferType::U32
                | InferType::U64
                | InferType::F32
                | InferType::F64
                | InferType::Bool
        ) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::SizedArrayElementNotDefaultable {
                    element: elem_ty.clone(),
                },
                span,
                reason: ConstraintReason::ArrayElement,
            });
        }

        (
            TypedExprKind::ArraySized {
                element_type: resolved_elem,
                size: Box::new(typed_size),
            },
            InferType::Array(Box::new(elem_ty)),
        )
    }

    pub(super) fn infer_vec_literal(
        &mut self,
        element_type: &Option<TypeAnnotation>,
        elements: &[Expr],
        repeat: Option<&Expr>,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_elements: Vec<TypedExpr> = elements.iter().map(|e| self.infer_expr(e)).collect();
        let typed_repeat = repeat.map(|count| self.infer_repeat_count(count));

        let (elem_ty, resolved_elem) = if let Some(ann) = element_type {
            let ty = self.type_from_annotation(ann);
            let resolved = ResolvedType::from_infer_type(&ty);
            (ty, Some(resolved))
        } else if typed_elements.is_empty() {
            (self.type_gen.fresh(), None)
        } else {
            let first_ty = typed_elements[0].ty.clone();
            for elem in typed_elements.iter().skip(1) {
                self.constraints.push(Constraint::equal(
                    elem.ty.clone(),
                    first_ty.clone(),
                    elem.span,
                    ConstraintReason::ArrayElement,
                ));
            }
            (first_ty, None)
        };

        for element in &typed_elements {
            let reason = ConstraintReason::ArrayElement;
            if self.reject_dynamic(&element.ty, &elem_ty, element.span, reason.clone()) {
                continue;
            }
            if !self.reject_untyped_native(&element.ty, &elem_ty, element.span, reason.clone()) {
                self.constraints.push(Constraint::equal(
                    element.ty.clone(),
                    elem_ty.clone(),
                    element.span,
                    reason,
                ));
            }
        }

        (
            TypedExprKind::VecLiteral {
                element_type: resolved_elem,
                elements: typed_elements,
                repeat: typed_repeat.map(Box::new),
            },
            InferType::Vec(Box::new(elem_ty)),
        )
    }

    fn infer_repeat_count(&mut self, count: &Expr) -> TypedExpr {
        let typed_count = self.infer_expr(count);
        if let Some(value) = constant_int_value(count)
            && value < 0
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NegativeArraySize { size: value },
                span: count.span,
                reason: ConstraintReason::ArrayIndex,
            });
        }
        if !self.reject_dynamic(
            &typed_count.ty,
            &InferType::I64,
            count.span,
            ConstraintReason::ArrayIndex,
        ) && !self.reject_untyped_native(
            &typed_count.ty,
            &InferType::I64,
            count.span,
            ConstraintReason::ArrayIndex,
        ) {
            self.constraints.push(Constraint::equal(
                typed_count.ty.clone(),
                InferType::I64,
                count.span,
                ConstraintReason::ArrayIndex,
            ));
        }
        typed_count
    }

    pub(super) fn infer_index_expr(
        &mut self,
        object: &Expr,
        index: &Expr,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        self.reject_constant_index(object, index);
        let typed_object = self.infer_expr(object);
        let typed_index = self.infer_expr(index);

        if matches!(typed_index.ty, InferType::Range) {
            let result_ty = match &typed_object.ty {
                InferType::Array(inner)
                | InferType::FixedArray(inner, _)
                | InferType::Vec(inner) => InferType::Vec(inner.clone()),
                receiver => {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidIndex {
                            receiver: receiver.clone(),
                        },
                        span: object.span,
                        reason: ConstraintReason::ArrayIndex,
                    });
                    InferType::Poison
                }
            };
            return (
                TypedExprKind::Slice {
                    object: Box::new(typed_object),
                    range: Box::new(typed_index),
                },
                result_ty,
            );
        }

        if !self.reject_dynamic(
            &typed_index.ty,
            &InferType::I64,
            index.span,
            ConstraintReason::ArrayIndex,
        ) && !matches!(typed_index.ty, InferType::UntypedNative(_))
        {
            self.constraints.push(Constraint::equal(
                typed_index.ty.clone(),
                InferType::I64,
                index.span,
                ConstraintReason::ArrayIndex,
            ));
        } else {
            self.reject_untyped_native(
                &typed_index.ty,
                &InferType::I64,
                index.span,
                ConstraintReason::ArrayIndex,
            );
        }

        let elem_ty = match &typed_object.ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) => (**inner).clone(),
            InferType::Vec(inner) => (**inner).clone(),
            InferType::String => InferType::String,
            InferType::Dynamic => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: InferType::Dynamic,
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Poison
            }
            InferType::Var(_) => {
                let element = self.type_gen.fresh();
                self.constraints.push(Constraint::one_of(
                    typed_object.ty.clone(),
                    vec![
                        InferType::String,
                        InferType::Array(Box::new(element.clone())),
                        InferType::Vec(Box::new(element.clone())),
                    ],
                    object.span,
                    ConstraintReason::ArrayIndex,
                ));
                element
            }
            InferType::UntypedNative(_) => {
                self.reject_untyped_native(
                    &typed_object.ty,
                    &InferType::Dynamic,
                    object.span,
                    ConstraintReason::ArrayIndex,
                );
                InferType::Poison
            }
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: receiver.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Poison
            }
        };

        (
            TypedExprKind::Index {
                object: Box::new(typed_object),
                index: Box::new(typed_index),
            },
            elem_ty,
        )
    }

    pub(super) fn infer_index_assign_expr(
        &mut self,
        object: &Expr,
        index: &Expr,
        value: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        self.check_write_access(object, span, "index assignment");
        self.reject_constant_index(object, index);
        let binding = mutable_collection_binding(object);
        let binding_type = binding
            .and_then(|name| self.env.lookup(name).cloned())
            .unwrap_or(InferType::Dynamic);
        if binding.is_some_and(|name| self.env.is_read_only(name)) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ReadOnlyCollectionRequired {
                    method: "index assignment".to_string(),
                    receiver: binding_type.clone(),
                },
                span: object.span,
                reason: ConstraintReason::Other("read-only collection receiver".to_string()),
            });
        } else if binding
            .is_none_or(|name| self.env.borrow_kind(name).is_none() && !self.env.is_mutable(name))
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionRequired {
                    method: "index assignment".to_string(),
                    receiver: binding_type,
                },
                span: object.span,
                reason: ConstraintReason::Other("mutable collection receiver".to_string()),
            });
        }
        let typed_object = self.infer_expr(object);
        let typed_index = self.infer_expr(index);
        let typed_value = self.infer_expr(value);

        if !self.reject_dynamic(
            &typed_index.ty,
            &InferType::I64,
            index.span,
            ConstraintReason::ArrayIndex,
        ) && !matches!(typed_index.ty, InferType::UntypedNative(_))
        {
            self.constraints.push(Constraint::equal(
                typed_index.ty.clone(),
                InferType::I64,
                index.span,
                ConstraintReason::ArrayIndex,
            ));
        } else {
            self.reject_untyped_native(
                &typed_index.ty,
                &InferType::I64,
                index.span,
                ConstraintReason::ArrayIndex,
            );
        }

        let result_ty = match &typed_object.ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) | InferType::Vec(inner) => {
                let reason = ConstraintReason::ArrayElement;
                if !self.reject_dynamic(&typed_value.ty, inner, value.span, reason.clone())
                    && !self.reject_untyped_native(
                        &typed_value.ty,
                        inner,
                        value.span,
                        reason.clone(),
                    )
                {
                    self.constraints.push(Constraint::equal(
                        typed_value.ty.clone(),
                        inner.as_ref().clone(),
                        value.span,
                        reason,
                    ));
                }
                InferType::Unit
            }
            InferType::Dynamic => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: InferType::Dynamic,
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayElement,
                });
                InferType::Unit
            }
            InferType::Var(_) => {
                let element = self.type_gen.fresh();
                self.constraints.push(Constraint::one_of(
                    typed_object.ty.clone(),
                    vec![
                        InferType::Array(Box::new(element.clone())),
                        InferType::Vec(Box::new(element.clone())),
                    ],
                    object.span,
                    ConstraintReason::ArrayElement,
                ));
                let reason = ConstraintReason::ArrayElement;
                if !self.reject_dynamic(&typed_value.ty, &element, value.span, reason.clone())
                    && !self.reject_untyped_native(
                        &typed_value.ty,
                        &element,
                        value.span,
                        reason.clone(),
                    )
                {
                    self.constraints.push(Constraint::equal(
                        typed_value.ty.clone(),
                        element,
                        value.span,
                        reason,
                    ));
                }
                InferType::Unit
            }
            InferType::UntypedNative(_) => {
                self.reject_untyped_native(
                    &typed_object.ty,
                    &InferType::Dynamic,
                    object.span,
                    ConstraintReason::ArrayElement,
                );
                InferType::Unit
            }
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: receiver.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayElement,
                });
                InferType::Unit
            }
        };

        (
            TypedExprKind::IndexAssign {
                object: Box::new(typed_object),
                index: Box::new(typed_index),
                value: Box::new(typed_value),
            },
            result_ty,
        )
    }

    fn reject_constant_index(&mut self, object: &Expr, index: &Expr) {
        let Some(index_value) = constant_int_value(index) else {
            return;
        };
        let length = match &object.kind {
            ExprKind::Identifier(name) => self.env.collection_length(name),
            _ => constant_collection_length(object),
        };
        let Some(length) = length else {
            return;
        };
        if index_value < 0 || usize::try_from(index_value).map_or(true, |index| index >= length) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ConstantIndexOutOfBounds {
                    index: index_value,
                    length,
                },
                span: index.span,
                reason: ConstraintReason::ArrayIndex,
            });
        }
    }

    pub(super) fn infer_slice_expr(
        &mut self,
        object: &Expr,
        range: &Expr,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let known_length = match &object.kind {
            ExprKind::Identifier(name) => self.env.collection_length(name),
            _ => constant_collection_length(object),
        };
        if let Some(length) = known_length
            && let ExprKind::Range {
                start,
                end,
                inclusive,
            } = &range.kind
            && let Some((start_value, end_value)) =
                constant_slice_bounds(start.as_deref(), end.as_deref(), *inclusive, length)
            && (start_value < 0
                || end_value < 0
                || start_value > end_value
                || end_value > i64::try_from(length).unwrap_or(i64::MAX)
                || (*inclusive && length == 0)
                || (*inclusive
                    && matches!(
                        (
                            start.as_deref().and_then(constant_int_value),
                            end.as_deref().and_then(constant_int_value)
                        ),
                        (Some(start), Some(end)) if start > end
                    )))
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ConstantSliceOutOfBounds {
                    start: start.as_deref().and_then(constant_int_value),
                    end: end.as_deref().and_then(constant_int_value),
                    length,
                    inclusive: *inclusive,
                },
                span: range.span,
                reason: ConstraintReason::ArrayIndex,
            });
        }
        let typed_object = self.infer_expr(object);
        let typed_range = self.infer_expr(range);
        let result_ty = match &typed_object.ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) | InferType::Vec(inner) => {
                InferType::Vec(inner.clone())
            }
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: receiver.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Poison
            }
        };

        (
            TypedExprKind::Slice {
                object: Box::new(typed_object),
                range: Box::new(typed_range),
            },
            result_ty,
        )
    }

    pub(super) fn infer_range_expr(
        &mut self,
        start: &Option<Box<Expr>>,
        end: &Option<Box<Expr>>,
        inclusive: bool,
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_start = start.as_ref().map(|e| Box::new(self.infer_expr(e)));
        let typed_end = end.as_ref().map(|e| Box::new(self.infer_expr(e)));

        if let Some(ref s) = typed_start
            && !self.reject_dynamic(&s.ty, &InferType::I64, s.span, ConstraintReason::RangeBound)
            && !self.reject_untyped_native(
                &s.ty,
                &InferType::I64,
                s.span,
                ConstraintReason::RangeBound,
            )
        {
            self.constraints.push(Constraint::equal(
                s.ty.clone(),
                InferType::I64,
                s.span,
                ConstraintReason::RangeBound,
            ));
        }
        if let Some(ref e) = typed_end
            && !self.reject_dynamic(&e.ty, &InferType::I64, e.span, ConstraintReason::RangeBound)
            && !self.reject_untyped_native(
                &e.ty,
                &InferType::I64,
                e.span,
                ConstraintReason::RangeBound,
            )
        {
            self.constraints.push(Constraint::equal(
                e.ty.clone(),
                InferType::I64,
                e.span,
                ConstraintReason::RangeBound,
            ));
        }

        (
            TypedExprKind::Range {
                start: typed_start,
                end: typed_end,
                inclusive,
            },
            InferType::Range,
        )
    }
}

fn constant_slice_bounds(
    start: Option<&Expr>,
    end: Option<&Expr>,
    inclusive: bool,
    length: usize,
) -> Option<(i64, i64)> {
    let start = match start {
        Some(expr) => Some(constant_int_value(expr)?),
        None => None,
    };
    let end = match end {
        Some(expr) => Some(constant_int_value(expr)?),
        None => None,
    };
    let start_value = start.unwrap_or(0);
    let end_value = match end {
        Some(value) if inclusive => value.checked_add(1)?,
        Some(value) => value,
        None => i64::try_from(length).ok()?,
    };
    Some((start_value, end_value))
}

fn constant_int_value(expr: &Expr) -> Option<i64> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Grouping(inner) => constant_int_value(inner),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => constant_int_value(operand)?.checked_neg(),
        ExprKind::Unary {
            op: UnaryOp::BitNot,
            operand,
        } => Some(!constant_int_value(operand)?),
        ExprKind::Binary { left, op, right } => {
            let left = constant_int_value(left)?;
            let right = constant_int_value(right)?;
            match op {
                BinaryOp::Add => left.checked_add(right),
                BinaryOp::Sub => left.checked_sub(right),
                BinaryOp::Mul => left.checked_mul(right),
                BinaryOp::Div => left.checked_div(right),
                BinaryOp::Mod => left.checked_rem(right),
                BinaryOp::Shl => left.checked_shl(u32::try_from(right).ok()?),
                BinaryOp::Shr => left.checked_shr(u32::try_from(right).ok()?),
                BinaryOp::BitAnd => Some(left & right),
                BinaryOp::BitOr => Some(left | right),
                BinaryOp::BitXor => Some(left ^ right),
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn constant_collection_length(expr: &Expr) -> Option<usize> {
    match &expr.kind {
        ExprKind::ArrayLiteral { elements, .. } | ExprKind::VecLiteral { elements, .. } => expr
            .repeat
            .as_deref()
            .and_then(constant_int_value)
            .and_then(|length| usize::try_from(length).ok())
            .or(Some(elements.len())),
        ExprKind::ArraySized { size, .. } => constant_int_value(size)
            .filter(|size| *size >= 0)
            .and_then(|size| usize::try_from(size).ok()),
        ExprKind::String(value) => Some(value.chars().count()),
        ExprKind::Grouping(inner) => constant_collection_length(inner),
        _ => None,
    }
}

fn mutable_collection_binding(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.as_str()),
        ExprKind::Grouping(inner) => mutable_collection_binding(inner),
        ExprKind::Index { object, .. } => mutable_collection_binding(object),
        _ => None,
    }
}
