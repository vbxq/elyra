use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedFunction, TypedParam};
use crate::types::InferType;
use aelys_syntax::{Function, Stmt, StmtKind};

impl TypeInference {
    /// Infer function type
    pub(super) fn infer_function(&mut self, func: &Function) -> TypedFunction {
        let fn_signature = self.env.lookup_function(&func.name).cloned();

        let (sig_params, sig_ret) = match fn_signature.as_deref() {
            Some(InferType::Function { params, ret }) => {
                (Some(params.clone()), Some((**ret).clone()))
            }
            _ => (None, None),
        };

        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, func.type_params.clone());

        for type_param in &func.type_params {
            let fresh_var = self.type_gen.fresh();
            self.env.define_local(type_param.clone(), fresh_var);
        }

        let mut typed_params = Vec::with_capacity(func.params.len());
        for (i, p) in func.params.iter().enumerate() {
            let ty = sig_params
                .as_ref()
                .and_then(|ps| ps.get(i).cloned())
                .or_else(|| {
                    p.type_annotation
                        .as_ref()
                        .map(|ann| self.type_from_annotation(ann))
                })
                .unwrap_or_else(|| self.type_gen.fresh());

            typed_params.push(TypedParam {
                name: p.name.clone(),
                mutable: p.mutable,
                ty,
                span: p.span,
            });
        }

        let return_type = sig_ret
            .or_else(|| {
                func.return_type
                    .as_ref()
                    .map(|ann| self.type_from_annotation(ann))
            })
            .unwrap_or_else(|| {
                if func.body.is_empty() {
                    InferType::Unit
                } else {
                    self.type_gen.fresh()
                }
            });

        let mut func_env = self.env.for_closure();
        func_env.set_current_function(Some(func.name.clone()));

        for (param, syntax_param) in typed_params.iter().zip(&func.params) {
            if syntax_param
                .type_annotation
                .as_ref()
                .is_some_and(|annotation| annotation.name.eq_ignore_ascii_case("dynamic"))
            {
                func_env.define_explicit_dynamic_local(param.name.clone(), param.ty.clone());
            } else {
                func_env.define_local(param.name.clone(), param.ty.clone());
            }
        }

        let saved_env = std::mem::replace(&mut self.env, func_env);

        self.collect_signatures(&func.body, &func.name);

        self.push_return_type(return_type.clone());

        let typed_body = if func.body.is_empty() {
            vec![]
        } else {
            let mut stmts: Vec<_> = func.body[..func.body.len() - 1]
                .iter()
                .map(|s| self.infer_stmt(s))
                .collect();

            let last_stmt = &func.body[func.body.len() - 1];
            let typed_last = self.infer_stmt_with_implicit_return(last_stmt, &return_type);
            stmts.push(typed_last);

            stmts
        };

        if func.return_type.is_some()
            && !matches!(return_type, InferType::Unit)
            && !body_always_returns(&func.body, true)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MissingReturnValue {
                    expected: return_type.clone(),
                },
                span: func.span,
                reason: ConstraintReason::Return {
                    func_name: func.name.clone(),
                },
            });
        }

        let captures = self.collect_captures_from_stmts(&typed_body, &typed_params);

        self.pop_return_type();
        self.env = saved_env;
        self.type_params_in_scope = saved_type_params;

        TypedFunction {
            name: func.name.clone(),
            type_params: func.type_params.clone(),
            params: typed_params,
            return_type,
            body: typed_body,
            decorators: func.decorators.clone(),
            is_pub: func.is_pub,
            span: func.span,
            captures,
        }
    }
}

pub(super) fn body_always_returns(stmts: &[Stmt], implicit_tail: bool) -> bool {
    let Some((last, prefix)) = stmts.split_last() else {
        return false;
    };

    for stmt in prefix {
        if stmt_always_returns(stmt, false) {
            return true;
        }
    }

    stmt_always_returns(last, implicit_tail)
}

fn stmt_always_returns(stmt: &Stmt, implicit_tail: bool) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Expression(_) => implicit_tail,
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => {
            stmt_always_returns(then_branch, implicit_tail)
                && stmt_always_returns(else_branch, implicit_tail)
        }
        StmtKind::Block(stmts) => body_always_returns(stmts, implicit_tail),
        _ => false,
    }
}
