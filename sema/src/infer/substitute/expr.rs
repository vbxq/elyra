use super::super::TypeInference;
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedFmtStringPart, TypedMatchArm, TypedMatchArmBody, TypedParam,
    TypedPattern, TypedPatternKind,
};
use crate::types::InferType;
use crate::unify::Substitution;
use aelys_syntax::{BinaryOp, MemberSeparator, UnaryOp};

impl TypeInference {
    pub(super) fn apply_substitution_expr(
        &self,
        expr: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExpr {
        let depth = self.substitution_depth.get() + 1;
        if depth > super::super::MAX_INFERENCE_DEPTH {
            self.substitution_overflowed.set(Some(expr.span));
            return TypedExpr {
                kind: TypedExprKind::Null,
                ty: InferType::Poison,
                span: expr.span,
            };
        }
        self.substitution_depth.set(depth);
        let kind = self.apply_substitution_expr_kind(expr, subst);
        self.substitution_depth.set(depth - 1);
        TypedExpr {
            kind,
            ty: subst.apply(&expr.ty),
            span: expr.span,
        }
    }

    // bounds the per-level stack cost, which `max_inference_depth` levels of a
    fn apply_substitution_expr_kind(
        &self,
        expr: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        match &expr.kind {
            TypedExprKind::Int(n) => TypedExprKind::Int(*n),
            TypedExprKind::Float(f) => TypedExprKind::Float(*f),
            TypedExprKind::Bool(b) => TypedExprKind::Bool(*b),
            TypedExprKind::String(s) => TypedExprKind::String(s.clone()),
            TypedExprKind::FmtString(parts) => self.substitute_fmt_string(parts, subst),
            TypedExprKind::Null => TypedExprKind::Null,
            TypedExprKind::Unit => TypedExprKind::Unit,
            TypedExprKind::Identifier(name) => TypedExprKind::Identifier(name.clone()),
            TypedExprKind::Binary { left, op, right } => {
                self.substitute_binary(left, *op, right, subst)
            }
            TypedExprKind::Unary { op, operand } => self.substitute_unary(*op, operand, subst),
            TypedExprKind::And { left, right } => self.substitute_and(left, right, subst),
            TypedExprKind::Or { left, right } => self.substitute_or(left, right, subst),
            TypedExprKind::Call { callee, args } => self.substitute_call(callee, args, subst),
            TypedExprKind::Assign { name, value } => self.substitute_assign(name, value, subst),
            TypedExprKind::Grouping(inner) => {
                TypedExprKind::Grouping(Box::new(self.apply_substitution_expr(inner, subst)))
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.substitute_if(condition, then_branch, else_branch, subst),
            TypedExprKind::Try { operand, .. } => self.substitute_try(operand, expr.span, subst),
            TypedExprKind::Match { scrutinee, arms } => {
                self.substitute_match(scrutinee, arms, subst)
            }
            TypedExprKind::Lambda(inner) => {
                TypedExprKind::Lambda(Box::new(self.apply_substitution_expr(inner, subst)))
            }
            TypedExprKind::LambdaInner {
                params,
                return_type,
                body,
                captures,
            } => self.substitute_lambda_inner(params, return_type, body, captures, subst),
            TypedExprKind::Member {
                object,
                member,
                separator,
            } => self.substitute_member(object, member, *separator, subst),
            TypedExprKind::StructField {
                object,
                member,
                offset,
                schema_index,
            } => self.substitute_struct_field(object, member, *offset, *schema_index, subst),
            TypedExprKind::StructMethod {
                object,
                symbol,
                method,
                separator,
            } => self.substitute_struct_method(object, symbol, method, *separator, subst),
            TypedExprKind::MemberAssign {
                object,
                member,
                offset,
                schema_index,
                value,
            } => {
                self.substitute_member_assign(object, member, *offset, *schema_index, value, subst)
            }
            TypedExprKind::ArrayLiteral {
                element_type,
                elements,
                repeat,
            } => self.substitute_array_literal(element_type, elements, repeat.as_deref(), subst),
            TypedExprKind::ArraySized { element_type, size } => {
                self.substitute_array_sized(element_type, size, subst)
            }
            TypedExprKind::VecLiteral {
                element_type,
                elements,
                repeat,
            } => self.substitute_vec_literal(element_type, elements, repeat.as_deref(), subst),
            TypedExprKind::Index { object, index } => self.substitute_index(object, index, subst),
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => self.substitute_index_assign(object, index, value, subst),
            TypedExprKind::Range {
                start,
                end,
                inclusive,
            } => self.substitute_range(start.as_deref(), end.as_deref(), *inclusive, subst),
            TypedExprKind::Slice { object, range } => self.substitute_slice(object, range, subst),
            TypedExprKind::StructLiteral {
                name,
                schema_index,
                fields,
                field_offsets,
            } => self.substitute_struct_literal(name, *schema_index, fields, field_offsets, subst),
            TypedExprKind::EnumConstruct {
                enum_name,
                variant,
                schema_index,
                variant_index,
                fields,
            } => self.substitute_enum_construct(
                enum_name,
                variant,
                *schema_index,
                *variant_index,
                fields,
                subst,
            ),
            TypedExprKind::Cast { expr, target } => self.substitute_cast(expr, target, subst),
            TypedExprKind::AssociatedConst {
                param,
                trait_name,
                item,
            } => TypedExprKind::AssociatedConst {
                param: param.clone(),
                trait_name: trait_name.clone(),
                item: item.clone(),
            },
        }
    }

    #[inline(never)]
    fn substitute_fmt_string(
        &self,
        parts: &[TypedFmtStringPart],
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::FmtString(
            parts
                .iter()
                .map(|p| match p {
                    TypedFmtStringPart::Literal(s) => TypedFmtStringPart::Literal(s.clone()),
                    TypedFmtStringPart::Expr(e) => {
                        TypedFmtStringPart::Expr(Box::new(self.apply_substitution_expr(e, subst)))
                    }
                    TypedFmtStringPart::Placeholder => TypedFmtStringPart::Placeholder,
                })
                .collect(),
        )
    }

    #[inline(never)]
    fn substitute_binary(
        &self,
        left: &TypedExpr,
        op: BinaryOp,
        right: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Binary {
            left: Box::new(self.apply_substitution_expr(left, subst)),
            op,
            right: Box::new(self.apply_substitution_expr(right, subst)),
        }
    }

    #[inline(never)]
    fn substitute_unary(
        &self,
        op: UnaryOp,
        operand: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Unary {
            op,
            operand: Box::new(self.apply_substitution_expr(operand, subst)),
        }
    }

    #[inline(never)]
    fn substitute_and(
        &self,
        left: &TypedExpr,
        right: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::And {
            left: Box::new(self.apply_substitution_expr(left, subst)),
            right: Box::new(self.apply_substitution_expr(right, subst)),
        }
    }

    #[inline(never)]
    fn substitute_or(
        &self,
        left: &TypedExpr,
        right: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Or {
            left: Box::new(self.apply_substitution_expr(left, subst)),
            right: Box::new(self.apply_substitution_expr(right, subst)),
        }
    }

    #[inline(never)]
    fn substitute_call(
        &self,
        callee: &TypedExpr,
        args: &[TypedExpr],
        subst: &Substitution,
    ) -> TypedExprKind {
        let mut callee = self.apply_substitution_expr(callee, subst);
        let args: Vec<TypedExpr> = args
            .iter()
            .map(|a| self.apply_substitution_expr(a, subst))
            .collect();
        // a qualified call carries its receiver as the first argument, so the instance chooses its impl, or refuses it, from there
        if let TypedExprKind::StructMethod {
            symbol,
            separator: MemberSeparator::Path,
            ..
        } = &mut callee.kind
            && let Some(receiver) = args.first()
            && self
                .type_table
                .trait_impl_defs()
                .iter()
                .flat_map(|definition| definition.methods.iter())
                .any(|entry| entry.symbol == *symbol && entry.has_self)
        {
            let verdict = self.type_table.select_specialization(&receiver.ty, symbol);
            match verdict {
                crate::types::SpecializationChoice::Redirect(chosen) => *symbol = chosen,
                crate::types::SpecializationChoice::Keep => {}
                verdict => self.specialization_verdicts.borrow_mut().push((
                    verdict,
                    receiver.ty.clone(),
                    receiver.span,
                )),
            }
        }
        TypedExprKind::Call {
            callee: Box::new(callee),
            args,
        }
    }

    #[inline(never)]
    fn substitute_assign(
        &self,
        name: &str,
        value: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Assign {
            name: name.to_string(),
            value: Box::new(self.apply_substitution_expr(value, subst)),
        }
    }

    #[inline(never)]
    fn substitute_if(
        &self,
        condition: &TypedExpr,
        then_branch: &TypedExpr,
        else_branch: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::If {
            condition: Box::new(self.apply_substitution_expr(condition, subst)),
            then_branch: Box::new(self.apply_substitution_expr(then_branch, subst)),
            else_branch: Box::new(self.apply_substitution_expr(else_branch, subst)),
        }
    }

    #[inline(never)]
    fn substitute_try(
        &self,
        operand: &TypedExpr,
        span: aelys_syntax::Span,
        subst: &Substitution,
    ) -> TypedExprKind {
        let operand = Box::new(self.apply_substitution_expr(operand, subst));
        let Some((symbol, source, target)) = self.try_conversions.get(&(span.start, span.end))
        else {
            return TypedExprKind::Try {
                operand,
                conversion: None,
                conversion_target: None,
            };
        };
        let applied_source = subst.apply(source);
        let applied_target = subst.apply(target);
        let (Some(source_error), Some(target_error)) =
            (error_of(&applied_source), error_of(&applied_target))
        else {
            return TypedExprKind::Try {
                operand,
                conversion: Some(symbol.clone()),
                conversion_target: error_of(&applied_target),
            };
        };
        let verdict = self
            .type_table
            .select_from_conversion(&source_error, &target_error);
        let conversion = match &verdict {
            crate::types::FromSelection::Identity => None,
            crate::types::FromSelection::Selected(chosen) => Some(chosen.clone()),
            _ => {
                self.conversion_verdicts.borrow_mut().push((
                    verdict.clone(),
                    applied_source.clone(),
                    applied_target.clone(),
                    span,
                ));
                None
            }
        };
        TypedExprKind::Try {
            operand,
            conversion,
            conversion_target: Some(target_error),
        }
    }

    #[inline(never)]
    fn substitute_match(
        &self,
        scrutinee: &TypedExpr,
        arms: &[TypedMatchArm],
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Match {
            scrutinee: Box::new(self.apply_substitution_expr(scrutinee, subst)),
            arms: arms
                .iter()
                .map(|arm| TypedMatchArm {
                    pattern: self.apply_substitution_pattern(&arm.pattern, subst),
                    guard: arm
                        .guard
                        .as_ref()
                        .map(|guard| self.apply_substitution_expr(guard, subst)),
                    body: match &arm.body {
                        TypedMatchArmBody::Expr(expr) => {
                            TypedMatchArmBody::Expr(self.apply_substitution_expr(expr, subst))
                        }
                        TypedMatchArmBody::Block(stmts) => TypedMatchArmBody::Block(
                            stmts
                                .iter()
                                .map(|stmt| self.apply_substitution_stmt(stmt, subst))
                                .collect(),
                        ),
                    },
                    explicit_dynamic: arm.explicit_dynamic,
                    span: arm.span,
                })
                .collect(),
        }
    }

    #[inline(never)]
    fn substitute_lambda_inner(
        &self,
        params: &[TypedParam],
        return_type: &InferType,
        body: &[crate::typed_ast::TypedStmt],
        captures: &[(String, InferType)],
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::LambdaInner {
            params: params
                .iter()
                .map(|p| TypedParam {
                    name: p.name.clone(),
                    mutable: p.mutable,
                    ty: subst.apply(&p.ty),
                    span: p.span,
                })
                .collect(),
            return_type: subst.apply(return_type),
            body: body
                .iter()
                .map(|s| self.apply_substitution_stmt(s, subst))
                .collect(),
            captures: captures
                .iter()
                .map(|(name, ty)| (name.clone(), subst.apply(ty)))
                .collect(),
        }
    }

    #[inline(never)]
    fn substitute_member(
        &self,
        object: &TypedExpr,
        member: &str,
        separator: MemberSeparator,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Member {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            member: member.to_string(),
            separator,
        }
    }

    #[inline(never)]
    fn substitute_struct_field(
        &self,
        object: &TypedExpr,
        member: &str,
        offset: u16,
        schema_index: u16,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::StructField {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            member: member.to_string(),
            offset,
            schema_index,
        }
    }

    #[inline(never)]
    fn substitute_struct_method(
        &self,
        object: &TypedExpr,
        symbol: &str,
        method: &str,
        separator: MemberSeparator,
        subst: &Substitution,
    ) -> TypedExprKind {
        // sema picked the root of a specialization chain because the receiver
        let object = self.apply_substitution_expr(object, subst);
        let verdict = self.type_table.select_specialization(&object.ty, symbol);
        let symbol = match &verdict {
            crate::types::SpecializationChoice::Redirect(chosen) => chosen.clone(),
            crate::types::SpecializationChoice::Keep => symbol.to_string(),
            _ => {
                self.specialization_verdicts.borrow_mut().push((
                    verdict,
                    object.ty.clone(),
                    object.span,
                ));
                symbol.to_string()
            }
        };
        TypedExprKind::StructMethod {
            object: Box::new(object),
            symbol,
            method: method.to_string(),
            separator,
        }
    }

    #[inline(never)]
    fn substitute_member_assign(
        &self,
        object: &TypedExpr,
        member: &str,
        offset: u16,
        schema_index: u16,
        value: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::MemberAssign {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            member: member.to_string(),
            offset,
            schema_index,
            value: Box::new(self.apply_substitution_expr(value, subst)),
        }
    }

    #[inline(never)]
    fn substitute_array_literal(
        &self,
        element_type: &Option<crate::types::ResolvedType>,
        elements: &[TypedExpr],
        repeat: Option<&TypedExpr>,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::ArrayLiteral {
            element_type: element_type.clone(),
            elements: elements
                .iter()
                .map(|e| self.apply_substitution_expr(e, subst))
                .collect(),
            repeat: repeat.map(|count| Box::new(self.apply_substitution_expr(count, subst))),
        }
    }

    #[inline(never)]
    fn substitute_array_sized(
        &self,
        element_type: &Option<crate::types::ResolvedType>,
        size: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::ArraySized {
            element_type: element_type.clone(),
            size: Box::new(self.apply_substitution_expr(size, subst)),
        }
    }

    #[inline(never)]
    fn substitute_vec_literal(
        &self,
        element_type: &Option<crate::types::ResolvedType>,
        elements: &[TypedExpr],
        repeat: Option<&TypedExpr>,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::VecLiteral {
            element_type: element_type.clone(),
            elements: elements
                .iter()
                .map(|e| self.apply_substitution_expr(e, subst))
                .collect(),
            repeat: repeat.map(|count| Box::new(self.apply_substitution_expr(count, subst))),
        }
    }

    #[inline(never)]
    fn substitute_index(
        &self,
        object: &TypedExpr,
        index: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Index {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            index: Box::new(self.apply_substitution_expr(index, subst)),
        }
    }

    #[inline(never)]
    fn substitute_index_assign(
        &self,
        object: &TypedExpr,
        index: &TypedExpr,
        value: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::IndexAssign {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            index: Box::new(self.apply_substitution_expr(index, subst)),
            value: Box::new(self.apply_substitution_expr(value, subst)),
        }
    }

    #[inline(never)]
    fn substitute_range(
        &self,
        start: Option<&TypedExpr>,
        end: Option<&TypedExpr>,
        inclusive: bool,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Range {
            start: start.map(|s| Box::new(self.apply_substitution_expr(s, subst))),
            end: end.map(|e| Box::new(self.apply_substitution_expr(e, subst))),
            inclusive,
        }
    }

    #[inline(never)]
    fn substitute_slice(
        &self,
        object: &TypedExpr,
        range: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Slice {
            object: Box::new(self.apply_substitution_expr(object, subst)),
            range: Box::new(self.apply_substitution_expr(range, subst)),
        }
    }

    #[inline(never)]
    fn substitute_struct_literal(
        &self,
        name: &str,
        schema_index: u16,
        fields: &[(String, Box<TypedExpr>)],
        field_offsets: &[u16],
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::StructLiteral {
            name: name.to_string(),
            schema_index,
            fields: fields
                .iter()
                .map(|(n, v)| (n.clone(), Box::new(self.apply_substitution_expr(v, subst))))
                .collect(),
            field_offsets: field_offsets.to_vec(),
        }
    }

    #[inline(never)]
    fn substitute_enum_construct(
        &self,
        enum_name: &str,
        variant: &str,
        schema_index: u16,
        variant_index: u16,
        fields: &[(Option<String>, Box<TypedExpr>)],
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::EnumConstruct {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
            schema_index,
            variant_index,
            fields: fields
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        Box::new(self.apply_substitution_expr(value, subst)),
                    )
                })
                .collect(),
        }
    }

    #[inline(never)]
    fn substitute_cast(
        &self,
        expr: &TypedExpr,
        target: &InferType,
        subst: &Substitution,
    ) -> TypedExprKind {
        TypedExprKind::Cast {
            expr: Box::new(self.apply_substitution_expr(expr, subst)),
            target: subst.apply(target),
        }
    }

    fn apply_substitution_pattern(
        &self,
        pattern: &TypedPattern,
        subst: &Substitution,
    ) -> TypedPattern {
        let kind = match &pattern.kind {
            TypedPatternKind::Wildcard => TypedPatternKind::Wildcard,
            TypedPatternKind::Binding(name) => TypedPatternKind::Binding(name.clone()),
            TypedPatternKind::Int(value) => TypedPatternKind::Int(*value),
            TypedPatternKind::String(value) => TypedPatternKind::String(value.clone()),
            TypedPatternKind::Bool(value) => TypedPatternKind::Bool(*value),
            TypedPatternKind::Variant {
                path,
                enum_schema_index,
                enum_variant_index,
                fields,
                field_offsets,
            } => TypedPatternKind::Variant {
                path: path.clone(),
                enum_schema_index: *enum_schema_index,
                enum_variant_index: *enum_variant_index,
                fields: fields
                    .iter()
                    .map(|field| self.apply_substitution_pattern(field, subst))
                    .collect(),
                field_offsets: field_offsets.clone(),
            },
            TypedPatternKind::Struct {
                name,
                schema_index,
                fields,
                has_rest,
            } => TypedPatternKind::Struct {
                name: name.clone(),
                schema_index: *schema_index,
                fields: fields
                    .iter()
                    .map(|(field, pattern, offset)| {
                        (
                            field.clone(),
                            self.apply_substitution_pattern(pattern, subst),
                            *offset,
                        )
                    })
                    .collect(),
                has_rest: *has_rest,
            },
            TypedPatternKind::Or(alternatives) => TypedPatternKind::Or(
                alternatives
                    .iter()
                    .map(|alternative| self.apply_substitution_pattern(alternative, subst))
                    .collect(),
            ),
        };
        TypedPattern {
            kind,
            ty: subst.apply(&pattern.ty),
            span: pattern.span,
        }
    }
}

fn error_of(ty: &InferType) -> Option<InferType> {
    match ty {
        InferType::Result(_, error) => Some(error.as_ref().clone()),
        _ => None,
    }
}
