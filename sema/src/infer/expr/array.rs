use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind};
use crate::types::{InferType, ResolvedType};
use aelys_syntax::{Expr, ExprKind, Span, TypeAnnotation};

impl TypeInference {
    pub(super) fn infer_array_literal(
        &mut self,
        element_type: &Option<TypeAnnotation>,
        elements: &[Expr],
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_elements: Vec<TypedExpr> = elements.iter().map(|e| self.infer_expr(e)).collect();

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
            TypedExprKind::ArrayLiteral {
                element_type: resolved_elem,
                elements: typed_elements,
            },
            InferType::Array(Box::new(elem_ty)),
        )
    }

    pub(super) fn infer_array_sized(
        &mut self,
        element_type: &Option<TypeAnnotation>,
        size: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_size = self.infer_expr(size);

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
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_elements: Vec<TypedExpr> = elements.iter().map(|e| self.infer_expr(e)).collect();

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
            },
            InferType::Vec(Box::new(elem_ty)),
        )
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
            InferType::Array(inner) => (**inner).clone(),
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
                InferType::Dynamic
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
                InferType::Dynamic
            }
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: receiver.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Dynamic
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
        _span: Span,
    ) -> (TypedExprKind, InferType) {
        self.reject_constant_index(object, index);
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
            InferType::Array(inner) | InferType::Vec(inner) => {
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
        let ExprKind::Int(index_value) = index.kind else {
            return;
        };
        let length = match &object.kind {
            ExprKind::ArrayLiteral { elements, .. } | ExprKind::VecLiteral { elements, .. } => {
                Some(elements.len())
            }
            ExprKind::ArraySized { size, .. } => match &size.kind {
                ExprKind::Int(size) if *size >= 0 => usize::try_from(*size).ok(),
                _ => None,
            },
            ExprKind::String(value) => Some(value.chars().count()),
            _ => None,
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
        let typed_object = self.infer_expr(object);
        let typed_range = self.infer_expr(range);
        let result_ty = match &typed_object.ty {
            InferType::Array(_) | InferType::Vec(_) => typed_object.ty.clone(),
            receiver => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::InvalidIndex {
                        receiver: receiver.clone(),
                    },
                    span: object.span,
                    reason: ConstraintReason::ArrayIndex,
                });
                InferType::Dynamic
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
