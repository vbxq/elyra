use super::super::TypeInference;
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedFmtStringPart, TypedMatchArm, TypedMatchArmBody, TypedParam,
    TypedPattern, TypedPatternKind,
};
use crate::unify::Substitution;

impl TypeInference {
    pub(super) fn apply_substitution_expr(
        &self,
        expr: &TypedExpr,
        subst: &Substitution,
    ) -> TypedExpr {
        let kind = match &expr.kind {
            TypedExprKind::Int(n) => TypedExprKind::Int(*n),
            TypedExprKind::Float(f) => TypedExprKind::Float(*f),
            TypedExprKind::Bool(b) => TypedExprKind::Bool(*b),
            TypedExprKind::String(s) => TypedExprKind::String(s.clone()),
            TypedExprKind::FmtString(parts) => TypedExprKind::FmtString(
                parts
                    .iter()
                    .map(|p| match p {
                        TypedFmtStringPart::Literal(s) => TypedFmtStringPart::Literal(s.clone()),
                        TypedFmtStringPart::Expr(e) => TypedFmtStringPart::Expr(Box::new(
                            self.apply_substitution_expr(e, subst),
                        )),
                        TypedFmtStringPart::Placeholder => TypedFmtStringPart::Placeholder,
                    })
                    .collect(),
            ),
            TypedExprKind::Null => TypedExprKind::Null,
            TypedExprKind::Unit => TypedExprKind::Unit,
            TypedExprKind::Identifier(name) => TypedExprKind::Identifier(name.clone()),
            TypedExprKind::Binary { left, op, right } => TypedExprKind::Binary {
                left: Box::new(self.apply_substitution_expr(left, subst)),
                op: *op,
                right: Box::new(self.apply_substitution_expr(right, subst)),
            },
            TypedExprKind::Unary { op, operand } => TypedExprKind::Unary {
                op: *op,
                operand: Box::new(self.apply_substitution_expr(operand, subst)),
            },
            TypedExprKind::And { left, right } => TypedExprKind::And {
                left: Box::new(self.apply_substitution_expr(left, subst)),
                right: Box::new(self.apply_substitution_expr(right, subst)),
            },
            TypedExprKind::Or { left, right } => TypedExprKind::Or {
                left: Box::new(self.apply_substitution_expr(left, subst)),
                right: Box::new(self.apply_substitution_expr(right, subst)),
            },
            TypedExprKind::Call { callee, args } => TypedExprKind::Call {
                callee: Box::new(self.apply_substitution_expr(callee, subst)),
                args: args
                    .iter()
                    .map(|a| self.apply_substitution_expr(a, subst))
                    .collect(),
            },
            TypedExprKind::Assign { name, value } => TypedExprKind::Assign {
                name: name.clone(),
                value: Box::new(self.apply_substitution_expr(value, subst)),
            },
            TypedExprKind::Grouping(inner) => {
                TypedExprKind::Grouping(Box::new(self.apply_substitution_expr(inner, subst)))
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => TypedExprKind::If {
                condition: Box::new(self.apply_substitution_expr(condition, subst)),
                then_branch: Box::new(self.apply_substitution_expr(then_branch, subst)),
                else_branch: Box::new(self.apply_substitution_expr(else_branch, subst)),
            },
            TypedExprKind::Try { operand, .. } => TypedExprKind::Try {
                operand: Box::new(self.apply_substitution_expr(operand, subst)),
                conversion: self
                    .try_conversions
                    .get(&(expr.span.start, expr.span.end))
                    .cloned(),
            },
            TypedExprKind::Match { scrutinee, arms } => TypedExprKind::Match {
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
            },
            TypedExprKind::Lambda(inner) => {
                TypedExprKind::Lambda(Box::new(self.apply_substitution_expr(inner, subst)))
            }
            TypedExprKind::LambdaInner {
                params,
                return_type,
                body,
                captures,
            } => TypedExprKind::LambdaInner {
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
            },
            TypedExprKind::Member {
                object,
                member,
                separator,
            } => TypedExprKind::Member {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                member: member.clone(),
                separator: *separator,
            },
            TypedExprKind::StructField {
                object,
                member,
                offset,
                schema_index,
            } => TypedExprKind::StructField {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                member: member.clone(),
                offset: *offset,
                schema_index: *schema_index,
            },
            TypedExprKind::StructMethod {
                object,
                symbol,
                method,
                separator,
            } => TypedExprKind::StructMethod {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                symbol: symbol.clone(),
                method: method.clone(),
                separator: *separator,
            },
            TypedExprKind::MemberAssign {
                object,
                member,
                offset,
                schema_index,
                value,
            } => TypedExprKind::MemberAssign {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                member: member.clone(),
                offset: *offset,
                schema_index: *schema_index,
                value: Box::new(self.apply_substitution_expr(value, subst)),
            },
            TypedExprKind::ArrayLiteral {
                element_type,
                elements,
                repeat,
            } => TypedExprKind::ArrayLiteral {
                element_type: element_type.clone(),
                elements: elements
                    .iter()
                    .map(|e| self.apply_substitution_expr(e, subst))
                    .collect(),
                repeat: repeat
                    .as_ref()
                    .map(|count| Box::new(self.apply_substitution_expr(count, subst))),
            },
            TypedExprKind::ArraySized { element_type, size } => TypedExprKind::ArraySized {
                element_type: element_type.clone(),
                size: Box::new(self.apply_substitution_expr(size, subst)),
            },
            TypedExprKind::VecLiteral {
                element_type,
                elements,
                repeat,
            } => TypedExprKind::VecLiteral {
                element_type: element_type.clone(),
                elements: elements
                    .iter()
                    .map(|e| self.apply_substitution_expr(e, subst))
                    .collect(),
                repeat: repeat
                    .as_ref()
                    .map(|count| Box::new(self.apply_substitution_expr(count, subst))),
            },
            TypedExprKind::Index { object, index } => TypedExprKind::Index {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                index: Box::new(self.apply_substitution_expr(index, subst)),
            },
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => TypedExprKind::IndexAssign {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                index: Box::new(self.apply_substitution_expr(index, subst)),
                value: Box::new(self.apply_substitution_expr(value, subst)),
            },
            TypedExprKind::Range {
                start,
                end,
                inclusive,
            } => TypedExprKind::Range {
                start: start
                    .as_ref()
                    .map(|s| Box::new(self.apply_substitution_expr(s, subst))),
                end: end
                    .as_ref()
                    .map(|e| Box::new(self.apply_substitution_expr(e, subst))),
                inclusive: *inclusive,
            },
            TypedExprKind::Slice { object, range } => TypedExprKind::Slice {
                object: Box::new(self.apply_substitution_expr(object, subst)),
                range: Box::new(self.apply_substitution_expr(range, subst)),
            },
            TypedExprKind::StructLiteral {
                name,
                schema_index,
                fields,
                field_offsets,
            } => TypedExprKind::StructLiteral {
                name: name.clone(),
                schema_index: *schema_index,
                fields: fields
                    .iter()
                    .map(|(n, v)| (n.clone(), Box::new(self.apply_substitution_expr(v, subst))))
                    .collect(),
                field_offsets: field_offsets.clone(),
            },
            TypedExprKind::EnumConstruct {
                enum_name,
                variant,
                schema_index,
                variant_index,
                fields,
            } => TypedExprKind::EnumConstruct {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                schema_index: *schema_index,
                variant_index: *variant_index,
                fields: fields
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            Box::new(self.apply_substitution_expr(value, subst)),
                        )
                    })
                    .collect(),
            },
            TypedExprKind::Cast { expr, target } => TypedExprKind::Cast {
                expr: Box::new(self.apply_substitution_expr(expr, subst)),
                target: subst.apply(target),
            },
        };

        TypedExpr {
            kind,
            ty: subst.apply(&expr.ty),
            span: expr.span,
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
