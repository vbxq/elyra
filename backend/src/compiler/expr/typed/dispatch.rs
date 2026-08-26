use super::super::Compiler;
use aelys_common::Result;

impl Compiler {
    pub fn compile_typed_expr(&mut self, expr: &aelys_sema::TypedExpr, dest: u16) -> Result<()> {
        use aelys_sema::TypedExprKind;

        match &expr.kind {
            TypedExprKind::Int(n) => self.compile_literal_int(*n, dest, expr.span),
            TypedExprKind::Float(f) => self.compile_literal_float(*f, dest, expr.span),
            TypedExprKind::String(s) => self.compile_literal_string(s, dest, expr.span),
            TypedExprKind::FmtString(parts) => {
                self.compile_typed_fmt_string(parts, &[], dest, expr.span)
            }
            TypedExprKind::Bool(b) => self.compile_literal_bool(*b, dest, expr.span),
            TypedExprKind::Unit => self.compile_literal_unit(dest, expr.span),
            TypedExprKind::AssociatedConst {
                param,
                trait_name,
                item,
            } => Err(aelys_common::error::CompileError::new(
                aelys_common::error::CompileErrorKind::TypeInferenceError(format!(
                    "associated constant '{param}::{item}' of trait '{trait_name}' reached code generation unresolved"
                )),
                expr.span,
                self.source.clone(),
            )
            .into()),
            TypedExprKind::Null => self.compile_literal_null(dest, expr.span),
            TypedExprKind::Identifier(name) => self.compile_typed_identifier(name, dest, expr.span),
            TypedExprKind::Binary { left, op, right } => {
                self.compile_typed_binary(left, *op, right, dest, expr.span)
            }
            TypedExprKind::Unary { op, operand } => {
                self.compile_typed_unary(*op, operand, dest, expr.span)
            }
            TypedExprKind::And { left, right } => {
                self.compile_typed_and(left, right, dest, expr.span)
            }
            TypedExprKind::Or { left, right } => {
                self.compile_typed_or(left, right, dest, expr.span)
            }
            TypedExprKind::Call { callee, args } => {
                self.compile_typed_call(callee, args, dest, expr.span)
            }
            TypedExprKind::Assign { name, value } => {
                self.compile_typed_assign(name, value, dest, expr.span)
            }
            TypedExprKind::Grouping(inner) => self.compile_typed_expr(inner, dest),
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.compile_typed_if_expr(condition, then_branch, else_branch, dest),
            TypedExprKind::Lambda(inner) => self.compile_typed_expr(inner, dest),
            TypedExprKind::LambdaInner {
                params,
                return_type: _,
                body,
                captures,
            } => self.compile_typed_lambda_with_stmts(params, body, captures, dest, expr.span),
            TypedExprKind::Member {
                object,
                member,
                separator,
            } => self.compile_typed_member_access(object, member, *separator, dest, expr.span),
            TypedExprKind::StructField {
                object,
                offset,
                schema_index,
                ..
            } => self.compile_typed_struct_field(object, *offset, *schema_index, dest, expr.span),
            TypedExprKind::StructMethod { symbol, .. } => {
                let global_idx = self.get_or_create_global_index(symbol);
                self.emit_get_global_index(dest, global_idx, expr.span);
                self.accessed_globals.insert(symbol.clone());
                Ok(())
            }
            TypedExprKind::MemberAssign {
                object,
                offset,
                schema_index,
                value,
                ..
            } => self.compile_typed_struct_field_assign(
                object,
                value,
                *offset,
                *schema_index,
                dest,
                expr.span,
            ),
            TypedExprKind::ArrayLiteral {
                elements, repeat, ..
            } => {
                if let Some(repeat) = repeat {
                    let value = elements.first().ok_or_else(|| {
                        aelys_common::error::AelysError::Compile(
                            aelys_common::error::CompileError::new(
                                aelys_common::error::CompileErrorKind::TypeInferenceError(
                                    "array repeat requires an element".to_string(),
                                ),
                                expr.span,
                                self.source.clone(),
                            ),
                        )
                    })?;
                    self.compile_typed_array_repeat(&expr.ty, value, repeat, dest, expr.span)
                } else {
                    self.compile_typed_array_literal(&expr.ty, elements, dest, expr.span)
                }
            }
            TypedExprKind::ArraySized { element_type, size } => {
                self.compile_typed_array_sized(element_type, size, dest, expr.span)
            }
            TypedExprKind::VecLiteral {
                elements, repeat, ..
            } => {
                if let Some(repeat) = repeat {
                    let value = elements.first().ok_or_else(|| {
                        aelys_common::error::AelysError::Compile(
                            aelys_common::error::CompileError::new(
                                aelys_common::error::CompileErrorKind::TypeInferenceError(
                                    "vec repeat requires an element".to_string(),
                                ),
                                expr.span,
                                self.source.clone(),
                            ),
                        )
                    })?;
                    self.compile_typed_vec_repeat(&expr.ty, value, repeat, dest, expr.span)
                } else {
                    self.compile_typed_vec_literal(&expr.ty, elements, dest, expr.span)
                }
            }
            TypedExprKind::Index { object, index } => {
                self.compile_typed_index_access(object, index, dest, expr.span)
            }
            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => self.compile_typed_index_assign(object, index, value, dest, expr.span),
            TypedExprKind::Range {
                start,
                end,
                inclusive,
            } => self.compile_typed_range(start, end, *inclusive, dest, expr.span),
            TypedExprKind::Slice { object, range } => {
                self.compile_typed_slice(object, range, dest, expr.span)
            }
            TypedExprKind::StructLiteral {
                fields,
                schema_index,
                field_offsets,
                ..
            } => self.compile_typed_struct_literal(
                *schema_index,
                fields,
                field_offsets,
                dest,
                expr.span,
            ),
            TypedExprKind::EnumConstruct {
                fields,
                schema_index,
                variant_index,
                ..
            } => self.compile_typed_enum_construct(
                *schema_index,
                *variant_index,
                fields,
                dest,
                expr.span,
            ),
            TypedExprKind::Cast {
                expr: inner,
                target,
            } => self.compile_typed_cast(inner, target, dest, expr.span),
            TypedExprKind::Try {
                operand,
                conversion,
            } => self.compile_typed_try(operand, conversion.as_deref(), dest, expr.span),
            TypedExprKind::Match { scrutinee, arms } => {
                self.compile_typed_match(scrutinee, arms, dest, expr.span)
            }
        }
    }

    pub(super) fn typed_expr_may_have_side_effects(expr: &aelys_sema::TypedExpr) -> bool {
        use aelys_sema::TypedExprKind;

        match &expr.kind {
            TypedExprKind::Call { .. } => true,
            TypedExprKind::Assign { .. } => true,
            TypedExprKind::Binary { left, right, .. } => {
                Self::typed_expr_may_have_side_effects(left)
                    || Self::typed_expr_may_have_side_effects(right)
            }
            TypedExprKind::Unary { operand, .. } => Self::typed_expr_may_have_side_effects(operand),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                Self::typed_expr_may_have_side_effects(left)
                    || Self::typed_expr_may_have_side_effects(right)
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                Self::typed_expr_may_have_side_effects(condition)
                    || Self::typed_expr_may_have_side_effects(then_branch)
                    || Self::typed_expr_may_have_side_effects(else_branch)
            }
            TypedExprKind::Grouping(inner) | TypedExprKind::Lambda(inner) => {
                Self::typed_expr_may_have_side_effects(inner)
            }
            TypedExprKind::Member { object, .. } => Self::typed_expr_may_have_side_effects(object),
            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => {
                Self::typed_expr_may_have_side_effects(object)
            }
            TypedExprKind::MemberAssign { .. } => true,
            TypedExprKind::LambdaInner { .. } => false,
            TypedExprKind::ArrayLiteral { elements, .. }
            | TypedExprKind::VecLiteral { elements, .. } => {
                elements.iter().any(Self::typed_expr_may_have_side_effects)
            }
            TypedExprKind::ArraySized { size, .. } => Self::typed_expr_may_have_side_effects(size),
            TypedExprKind::Index { object, index } => {
                Self::typed_expr_may_have_side_effects(object)
                    || Self::typed_expr_may_have_side_effects(index)
            }
            TypedExprKind::IndexAssign { .. } => true, // assignment has side effects
            TypedExprKind::Range { start, end, .. } => {
                start
                    .as_ref()
                    .is_some_and(|s| Self::typed_expr_may_have_side_effects(s))
                    || end
                        .as_ref()
                        .is_some_and(|e| Self::typed_expr_may_have_side_effects(e))
            }
            TypedExprKind::Slice { object, range } => {
                Self::typed_expr_may_have_side_effects(object)
                    || Self::typed_expr_may_have_side_effects(range)
            }
            TypedExprKind::FmtString(parts) => parts.iter().any(|p| match p {
                aelys_sema::TypedFmtStringPart::Expr(e) => {
                    Self::typed_expr_may_have_side_effects(e)
                }
                _ => false,
            }),
            TypedExprKind::StructLiteral { fields, .. } => fields
                .iter()
                .any(|(_, v)| Self::typed_expr_may_have_side_effects(v)),
            TypedExprKind::EnumConstruct { fields, .. } => fields
                .iter()
                .any(|(_, v)| Self::typed_expr_may_have_side_effects(v)),
            TypedExprKind::Cast { expr, .. } => Self::typed_expr_may_have_side_effects(expr),
            TypedExprKind::Unit => false,
            TypedExprKind::Try { operand, .. } => Self::typed_expr_may_have_side_effects(operand),
            TypedExprKind::Match { .. } => true,
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::AssociatedConst { .. }
            | TypedExprKind::Null
            | TypedExprKind::Identifier(_) => false,
        }
    }
}
