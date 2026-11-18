use crate::passes::effects::Effects;
use aelys_sema::{
    TypedExpr, TypedExprKind, TypedFunction, TypedMatchArm, TypedMatchArmBody, TypedPattern,
    TypedPatternKind, TypedStmt, TypedStmtKind,
};
use aelys_syntax::Span;
use std::collections::HashMap;

pub struct InlineExpander {
    next_temporary: usize,
}

impl InlineExpander {
    pub fn new() -> Self {
        Self { next_temporary: 0 }
    }

    pub fn expand_call(
        &mut self,
        func: &TypedFunction,
        args: &[TypedExpr],
        call_span: Span,
        effects: &Effects,
    ) -> Option<TypedExpr> {
        if func.params.len() != args.len() {
            return None;
        }

        let body = match func.body.as_slice() {
            [stmt] => match &stmt.kind {
                TypedStmtKind::Return(Some(expr)) | TypedStmtKind::Expression(expr) => expr,
                _ => return None,
            },
            _ => return None,
        };

        let param_names: HashMap<String, TypedExpr> = func
            .params
            .iter()
            .zip(args.iter())
            .map(|(p, a)| (p.name.clone(), a.clone()))
            .collect();
        if !self.expr_has_only_params_and_literals(body, &param_names) {
            return None;
        }

        let effectful: Vec<bool> = args.iter().map(|arg| effects.has_effects(arg)).collect();
        let last_effectful = effectful.iter().rposition(|e| *e);
        let mut substitutions = HashMap::new();
        let mut bindings = Vec::new();
        for (index, (param, arg)) in func.params.iter().zip(args.iter()).enumerate() {
            let uses = count_uses(body, &param.name);
            let later_effects = last_effectful.is_some_and(|last| last > index);
            let substitutable = is_literal(arg)
                || (!effectful[index]
                    && !later_effects
                    && (uses <= 1 || matches!(arg.kind, TypedExprKind::Identifier(_))));
            if substitutable {
                substitutions.insert(param.name.clone(), arg.clone());
                continue;
            }
            let temporary = format!("{}#inline{}", param.name, self.next_temporary);
            self.next_temporary += 1;
            substitutions.insert(
                param.name.clone(),
                TypedExpr::new(
                    TypedExprKind::Identifier(temporary.clone()),
                    arg.ty.clone(),
                    arg.span,
                ),
            );
            bindings.push((temporary, arg.clone()));
        }

        let mut result = self.substitute_expr(body, &substitutions, call_span);
        for (temporary, arg) in bindings.into_iter().rev() {
            let ty = result.ty.clone();
            let pattern = TypedPattern {
                kind: TypedPatternKind::Binding(temporary),
                ty: arg.ty.clone(),
                span: arg.span,
            };
            let arm = TypedMatchArm {
                pattern,
                guard: None,
                body: TypedMatchArmBody::Expr(result),
                explicit_dynamic: false,
                span: call_span,
            };
            result = TypedExpr::new(
                TypedExprKind::Match {
                    scrutinee: Box::new(arg),
                    arms: vec![arm],
                },
                ty,
                call_span,
            );
        }
        Some(result)
    }

    fn expr_has_only_params_and_literals(
        &self,
        expr: &TypedExpr,
        params: &HashMap<String, TypedExpr>,
    ) -> bool {
        match &expr.kind {
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null => true,

            TypedExprKind::Identifier(name) => params.contains_key(name),

            TypedExprKind::Binary { left, right, .. } => {
                self.expr_has_only_params_and_literals(left, params)
                    && self.expr_has_only_params_and_literals(right, params)
            }
            TypedExprKind::Unary { operand, .. } => {
                self.expr_has_only_params_and_literals(operand, params)
            }
            TypedExprKind::Grouping(inner) => self.expr_has_only_params_and_literals(inner, params),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.expr_has_only_params_and_literals(left, params)
                    && self.expr_has_only_params_and_literals(right, params)
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_has_only_params_and_literals(condition, params)
                    && self.expr_has_only_params_and_literals(then_branch, params)
                    && self.expr_has_only_params_and_literals(else_branch, params)
            }
            _ => false,
        }
    }

    fn substitute_expr(
        &self,
        expr: &TypedExpr,
        params: &HashMap<String, TypedExpr>,
        span: Span,
    ) -> TypedExpr {
        let kind = match &expr.kind {
            TypedExprKind::Identifier(name) => {
                if let Some(replacement) = params.get(name) {
                    return replacement.clone();
                }
                TypedExprKind::Identifier(name.clone())
            }

            TypedExprKind::Binary { left, op, right } => TypedExprKind::Binary {
                left: Box::new(self.substitute_expr(left, params, span)),
                op: *op,
                right: Box::new(self.substitute_expr(right, params, span)),
            },

            TypedExprKind::Unary { op, operand } => TypedExprKind::Unary {
                op: *op,
                operand: Box::new(self.substitute_expr(operand, params, span)),
            },

            TypedExprKind::And { left, right } => TypedExprKind::And {
                left: Box::new(self.substitute_expr(left, params, span)),
                right: Box::new(self.substitute_expr(right, params, span)),
            },

            TypedExprKind::Or { left, right } => TypedExprKind::Or {
                left: Box::new(self.substitute_expr(left, params, span)),
                right: Box::new(self.substitute_expr(right, params, span)),
            },

            TypedExprKind::Call { callee, args } => TypedExprKind::Call {
                callee: Box::new(self.substitute_expr(callee, params, span)),
                args: args
                    .iter()
                    .map(|a| self.substitute_expr(a, params, span))
                    .collect(),
            },

            TypedExprKind::Grouping(inner) => {
                TypedExprKind::Grouping(Box::new(self.substitute_expr(inner, params, span)))
            }

            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => TypedExprKind::If {
                condition: Box::new(self.substitute_expr(condition, params, span)),
                then_branch: Box::new(self.substitute_expr(then_branch, params, span)),
                else_branch: Box::new(self.substitute_expr(else_branch, params, span)),
            },

            TypedExprKind::Assign { name, value } => TypedExprKind::Assign {
                name: name.clone(),
                value: Box::new(self.substitute_expr(value, params, span)),
            },

            TypedExprKind::Member {
                object,
                member,
                separator,
            } => TypedExprKind::Member {
                object: Box::new(self.substitute_expr(object, params, span)),
                member: member.clone(),
                separator: *separator,
            },

            TypedExprKind::ArrayLiteral {
                element_type,
                elements,
                repeat,
            } => TypedExprKind::ArrayLiteral {
                element_type: element_type.clone(),
                elements: elements
                    .iter()
                    .map(|e| self.substitute_expr(e, params, span))
                    .collect(),
                repeat: repeat
                    .as_ref()
                    .map(|count| Box::new(self.substitute_expr(count, params, span))),
            },

            TypedExprKind::VecLiteral {
                element_type,
                elements,
                repeat,
            } => TypedExprKind::VecLiteral {
                element_type: element_type.clone(),
                elements: elements
                    .iter()
                    .map(|e| self.substitute_expr(e, params, span))
                    .collect(),
                repeat: repeat
                    .as_ref()
                    .map(|count| Box::new(self.substitute_expr(count, params, span))),
            },

            TypedExprKind::ArraySized { element_type, size } => TypedExprKind::ArraySized {
                element_type: element_type.clone(),
                size: Box::new(self.substitute_expr(size, params, span)),
            },

            TypedExprKind::Index { object, index } => TypedExprKind::Index {
                object: Box::new(self.substitute_expr(object, params, span)),
                index: Box::new(self.substitute_expr(index, params, span)),
            },

            TypedExprKind::IndexAssign {
                object,
                index,
                value,
            } => TypedExprKind::IndexAssign {
                object: Box::new(self.substitute_expr(object, params, span)),
                index: Box::new(self.substitute_expr(index, params, span)),
                value: Box::new(self.substitute_expr(value, params, span)),
            },

            TypedExprKind::Range {
                start,
                end,
                inclusive,
            } => TypedExprKind::Range {
                start: start
                    .as_ref()
                    .map(|s| Box::new(self.substitute_expr(s, params, span))),
                end: end
                    .as_ref()
                    .map(|e| Box::new(self.substitute_expr(e, params, span))),
                inclusive: *inclusive,
            },

            TypedExprKind::Slice { object, range } => TypedExprKind::Slice {
                object: Box::new(self.substitute_expr(object, params, span)),
                range: Box::new(self.substitute_expr(range, params, span)),
            },

            TypedExprKind::Lambda(inner) => {
                TypedExprKind::Lambda(Box::new(self.substitute_expr(inner, params, span)))
            }

            TypedExprKind::LambdaInner {
                params: lparams,
                return_type,
                body,
                captures,
            } => {
                let mut filtered = params.clone();
                for p in lparams {
                    filtered.remove(&p.name);
                }
                TypedExprKind::LambdaInner {
                    params: lparams.clone(),
                    return_type: return_type.clone(),
                    body: body
                        .iter()
                        .map(|s| self.substitute_stmt(s, &filtered, span))
                        .collect(),
                    captures: captures.clone(),
                }
            }

            TypedExprKind::FmtString(parts) => TypedExprKind::FmtString(
                parts
                    .iter()
                    .map(|p| match p {
                        aelys_sema::TypedFmtStringPart::Literal(s) => {
                            aelys_sema::TypedFmtStringPart::Literal(s.clone())
                        }
                        aelys_sema::TypedFmtStringPart::Expr(e) => {
                            aelys_sema::TypedFmtStringPart::Expr(Box::new(
                                self.substitute_expr(e, params, span),
                            ))
                        }
                        aelys_sema::TypedFmtStringPart::Placeholder => {
                            aelys_sema::TypedFmtStringPart::Placeholder
                        }
                    })
                    .collect(),
            ),

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
                    .map(|(n, v)| (n.clone(), Box::new(self.substitute_expr(v, params, span))))
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
                            Box::new(self.substitute_expr(value, params, span)),
                        )
                    })
                    .collect(),
            },
            TypedExprKind::StructField {
                object,
                member,
                offset,
                schema_index,
            } => TypedExprKind::StructField {
                object: Box::new(self.substitute_expr(object, params, span)),
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
                object: Box::new(self.substitute_expr(object, params, span)),
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
                object: Box::new(self.substitute_expr(object, params, span)),
                member: member.clone(),
                offset: *offset,
                schema_index: *schema_index,
                value: Box::new(self.substitute_expr(value, params, span)),
            },
            TypedExprKind::Cast { expr, target } => TypedExprKind::Cast {
                expr: Box::new(self.substitute_expr(expr, params, span)),
                target: target.clone(),
            },
            TypedExprKind::Int(n) => TypedExprKind::Int(*n),
            TypedExprKind::Float(f) => TypedExprKind::Float(*f),
            TypedExprKind::Bool(b) => TypedExprKind::Bool(*b),
            TypedExprKind::String(s) => TypedExprKind::String(s.clone()),
            TypedExprKind::Char(s) => TypedExprKind::Char(*s),
            TypedExprKind::Unit => TypedExprKind::Unit,
            TypedExprKind::AssociatedConst {
                param,
                trait_name,
                item,
            } => TypedExprKind::AssociatedConst {
                param: param.clone(),
                trait_name: trait_name.clone(),
                item: item.clone(),
            },
            TypedExprKind::Null => TypedExprKind::Null,
            TypedExprKind::Try {
                operand,
                conversion,
                conversion_target,
            } => TypedExprKind::Try {
                operand: Box::new(self.substitute_expr(operand, params, span)),
                conversion: conversion.clone(),
                conversion_target: conversion_target.clone(),
            },
            TypedExprKind::Match { scrutinee, arms } => TypedExprKind::Match {
                scrutinee: Box::new(self.substitute_expr(scrutinee, params, span)),
                arms: arms.clone(),
            },
            TypedExprKind::Block(_) | TypedExprKind::Loop { .. } => expr.kind.clone(),
        };

        TypedExpr::new(kind, expr.ty.clone(), span)
    }

    fn substitute_stmt(
        &self,
        stmt: &TypedStmt,
        params: &HashMap<String, TypedExpr>,
        span: Span,
    ) -> TypedStmt {
        let kind = match &stmt.kind {
            TypedStmtKind::Expression(e) => {
                TypedStmtKind::Expression(self.substitute_expr(e, params, span))
            }
            TypedStmtKind::Let {
                name,
                mutable,
                initializer,
                var_type,
                is_pub,
            } => TypedStmtKind::Let {
                name: name.clone(),
                mutable: *mutable,
                initializer: self.substitute_expr(initializer, params, span),
                var_type: var_type.clone(),
                is_pub: *is_pub,
            },
            TypedStmtKind::Block(stmts) => TypedStmtKind::Block(
                stmts
                    .iter()
                    .map(|s| self.substitute_stmt(s, params, span))
                    .collect(),
            ),
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => TypedStmtKind::If {
                condition: self.substitute_expr(condition, params, span),
                then_branch: Box::new(self.substitute_stmt(then_branch, params, span)),
                else_branch: else_branch
                    .as_ref()
                    .map(|e| Box::new(self.substitute_stmt(e, params, span))),
            },
            TypedStmtKind::While {
                condition,
                body,
                label,
            } => TypedStmtKind::While {
                condition: self.substitute_expr(condition, params, span),
                body: Box::new(self.substitute_stmt(body, params, span)),
                label: label.clone(),
            },
            TypedStmtKind::For {
                iterator,
                start,
                end,
                inclusive,
                step,
                body,
                label,
            } => TypedStmtKind::For {
                iterator: iterator.clone(),
                start: self.substitute_expr(start, params, span),
                end: self.substitute_expr(end, params, span),
                inclusive: *inclusive,
                step: Box::new(
                    step.as_ref()
                        .as_ref()
                        .map(|s| self.substitute_expr(s, params, span)),
                ),
                body: Box::new(self.substitute_stmt(body, params, span)),
                label: label.clone(),
            },
            TypedStmtKind::ForEach {
                iterator,
                iterable,
                elem_type,
                read_only,
                body,
                label,
            } => TypedStmtKind::ForEach {
                iterator: iterator.clone(),
                iterable: self.substitute_expr(iterable, params, span),
                elem_type: elem_type.clone(),
                read_only: *read_only,
                body: Box::new(self.substitute_stmt(body, params, span)),
                label: label.clone(),
            },
            TypedStmtKind::Return(e) => {
                TypedStmtKind::Return(e.as_ref().map(|ex| self.substitute_expr(ex, params, span)))
            }
            other => other.clone(),
        };

        TypedStmt::new(kind, stmt.span)
    }
}

impl Default for InlineExpander {
    fn default() -> Self {
        Self::new()
    }
}

fn is_literal(expr: &TypedExpr) -> bool {
    matches!(
        expr.kind,
        TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null
    )
}

fn count_uses(expr: &TypedExpr, name: &str) -> usize {
    match &expr.kind {
        TypedExprKind::Identifier(n) => usize::from(n == name),
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => count_uses(left, name) + count_uses(right, name),
        TypedExprKind::Unary { operand, .. } | TypedExprKind::Grouping(operand) => {
            count_uses(operand, name)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_uses(condition, name)
                + count_uses(then_branch, name)
                + count_uses(else_branch, name)
        }
        _ => 0,
    }
}
