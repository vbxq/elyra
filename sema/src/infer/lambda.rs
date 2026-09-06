use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedParam};
use crate::types::InferType;
use aelys_syntax::{Parameter, Span, Stmt, TypeAnnotation};

impl TypeInference {
    pub(super) fn infer_lambda(
        &mut self,
        params: &[Parameter],
        return_type_ann: Option<&TypeAnnotation>,
        body: &[Stmt],
        span: Span,
    ) -> TypedExpr {
        let closure_env = self.env.for_closure();

        let mut typed_params = Vec::with_capacity(params.len());
        for p in params {
            let ty = match &p.type_annotation {
                Some(ann) => self.type_from_parameter_annotation(ann),
                None => self.type_gen.fresh(),
            };
            typed_params.push(TypedParam {
                name: p.name.clone(),
                mutable: p.mutable,
                ty,
                span: p.span,
            });
        }

        let return_type = match return_type_ann {
            Some(ann) => {
                self.type_from_annotation_as(crate::infer::OccurrenceRole::ReturnType, ann)
            }
            None if body.is_empty() => InferType::Unit,
            None => self.type_gen.fresh(),
        };

        let saved_env = std::mem::replace(&mut self.env, closure_env);
        let saved_forwarded_borrows = std::mem::take(&mut self.forwarded_mutable_borrows);

        for (param, syntax_param) in typed_params.iter().zip(params) {
            if param.ty.contains_dynamic() {
                self.env
                    .define_explicit_dynamic_local(param.name.clone(), param.ty.clone());
            } else {
                self.env.define_local(param.name.clone(), param.ty.clone());
            }
            self.env.set_mutable(&param.name, param.mutable);
            if let Some(reference) = syntax_param.reference {
                self.env
                    .define_borrow_binding(param.name.clone(), reference);
            }
        }

        self.push_return_type(return_type.clone());

        let typed_stmts = if body.is_empty() {
            vec![]
        } else {
            let mut stmts = Vec::new();
            for stmt in &body[..body.len() - 1] {
                stmts.push(self.infer_stmt(stmt));
            }

            let last_stmt = &body[body.len() - 1];
            let typed_last = self.infer_stmt_with_implicit_return(last_stmt, &return_type);
            stmts.push(typed_last);

            stmts
        };

        if return_type_ann.is_some()
            && !matches!(return_type, InferType::Unit)
            && !super::functions::body_always_returns(body, true)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MissingReturnValue {
                    expected: return_type.clone(),
                },
                span,
                reason: ConstraintReason::Return {
                    func_name: "<lambda>".to_string(),
                },
            });
        }

        let captures = self.collect_captures_from_stmts(&typed_stmts, &typed_params);

        self.pop_return_type();
        self.env = saved_env;
        self.forwarded_mutable_borrows = saved_forwarded_borrows;

        let param_types: Vec<InferType> = typed_params.iter().map(|p| p.ty.clone()).collect();
        let fn_type = InferType::Function {
            params: param_types,
            ret: Box::new(return_type.clone()),
        };

        TypedExpr {
            kind: TypedExprKind::LambdaInner {
                params: typed_params,
                return_type,
                body: typed_stmts,
                captures,
            },
            ty: fn_type,
            span,
        }
    }
}
