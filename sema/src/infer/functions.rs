use super::{GENERATED_SYMBOL_PREFIX, TypeInference};
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedFunction, TypedParam};
use crate::types::InferType;
use aelys_syntax::{Function, Stmt, StmtKind, TypeAnnotation};

pub(crate) fn struct_method_symbol(structure: &str, method: &str) -> String {
    format!(
        "{GENERATED_SYMBOL_PREFIX}struct::{:08x}:{}{:08x}:{}",
        structure.len(),
        structure,
        method.len(),
        method
    )
}

pub(crate) fn trait_method_symbol(
    trait_name: &str,
    structure: &str,
    method: &str,
    trait_args: &[InferType],
) -> String {
    let mut symbol = format!(
        "{GENERATED_SYMBOL_PREFIX}trait::{:08x}:{}{:08x}:{}{:08x}:{}",
        trait_name.len(),
        trait_name,
        structure.len(),
        structure,
        method.len(),
        method
    );
    if !trait_args.is_empty() {
        use std::fmt::Write;
        for argument in trait_args {
            let rendered = argument.to_string();
            let _ = write!(symbol, "{:08x}:{}", rendered.len(), rendered);
        }
    }
    symbol
}

impl TypeInference {
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

        // bounds of this function, kept for the whole body so a `t::limit`
        let saved_function_bounds = std::mem::replace(
            &mut self.current_function_bounds,
            self.generic_function_bounds
                .get(&func.name)
                .cloned()
                .unwrap_or_default(),
        );
        let saved_function_bindings = std::mem::replace(
            &mut self.current_function_bindings,
            self.generic_function_bindings
                .get(&func.name)
                .cloned()
                .unwrap_or_default(),
        );

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
                        .map(|ann| self.type_from_parameter_annotation(ann))
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
                func.return_type.as_ref().map(|ann| {
                    self.type_from_annotation_as(crate::infer::OccurrenceRole::ReturnType, ann)
                })
            })
            .unwrap_or_else(|| {
                if func.body.is_empty() {
                    InferType::Unit
                } else {
                    self.type_gen.fresh()
                }
            });

        if func.return_type.is_none() && !body_yields_value(&func.body) {
            self.constraints.push(crate::constraint::Constraint::equal(
                return_type.clone(),
                InferType::Unit,
                func.span,
                ConstraintReason::Return {
                    func_name: func.name.clone(),
                },
            ));
        }

        let saved_forwarded_borrows = std::mem::take(&mut self.forwarded_mutable_borrows);
        let mut func_env = self.env.for_closure();
        func_env.set_current_function(Some(func.name.clone()));

        for (param, syntax_param) in typed_params.iter().zip(&func.params) {
            if param.ty.contains_dynamic() {
                func_env.define_explicit_dynamic_local(param.name.clone(), param.ty.clone());
            } else {
                func_env.define_local(param.name.clone(), param.ty.clone());
            }
            func_env.set_mutable(&param.name, param.mutable);
            if let Some(reference) = syntax_param.reference {
                func_env.define_borrow_binding(param.name.clone(), reference);
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
        self.current_function_bindings = saved_function_bindings;
        self.current_function_bounds = saved_function_bounds;
        self.forwarded_mutable_borrows = saved_forwarded_borrows;

        TypedFunction {
            name: func.name.clone(),
            type_params: func.type_params.clone(),
            own_type_params: func.type_params.clone(),
            params: typed_params,
            return_type,
            body: typed_body,
            decorators: func.decorators.clone(),
            is_pub: func.is_pub,
            span: func.span,
            captures,
        }
    }

    pub(super) fn infer_impl_decl(
        &mut self,
        self_type: &TypeAnnotation,
        impl_type_params: &[String],
        methods: &[Function],
        trait_path: Option<&TypeAnnotation>,
        negative: bool,
    ) -> crate::typed_ast::TypedStmtKind {
        let target_type = {
            let saved =
                std::mem::replace(&mut self.type_params_in_scope, impl_type_params.to_vec());
            let ty =
                self.type_from_annotation_as(crate::infer::OccurrenceRole::ImplHeader, self_type);
            self.type_params_in_scope = saved;
            ty
        };
        let written = self_type
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| self_type.name.clone());
        let target = match self_type.path.len() >= 2 {
            true => crate::types::nominal_name(&target_type).unwrap_or(written),
            false => written,
        };
        let header_spelling = crate::types::positional_spelling(&target_type);
        let trait_name = trait_path.map(|path| path.path.join("::"));
        let trait_args = self.impl_trait_args(trait_path, impl_type_params);
        // a negative impl supplies nothing, so no default body is typed on its header
        let effective_methods = match negative {
            true => methods.to_vec(),
            false => self.adopted_impl_methods(methods, trait_name.as_deref(), &target),
        };
        let nominal_params: Vec<String> = self
            .type_table
            .get_struct(&target)
            .map(|definition| definition.type_params.clone())
            .or_else(|| {
                self.type_table
                    .get_enum(&target)
                    .map(|definition| definition.type_params.clone())
            })
            .unwrap_or_default();
        let saved_impl_self = self
            .current_impl_self
            .replace(InferType::Struct(target.clone()));
        let mut typed_methods = Vec::with_capacity(effective_methods.len());
        let saved_default_body = self.in_trait_default_body;
        let saved_adoption = self.adopted_instantiation.take();
        let saved_header = self.current_impl_header.replace(target_type.clone());
        for (index, method) in effective_methods.iter().enumerate() {
            self.in_trait_default_body = index >= methods.len();
            self.adopted_instantiation = trait_name
                .clone()
                .filter(|_| self.in_trait_default_body)
                .map(|name| (name, target_type.clone(), trait_args.clone()));
            let mut normalized = method.clone();
            // a method that redeclares a spelling its impl or its nominal already
            let renames: std::collections::HashMap<String, String> = method
                .type_params
                .iter()
                .filter(|name| impl_type_params.contains(name) || nominal_params.contains(name))
                .map(|name| (name.clone(), crate::infer::shadowed_method_param(name)))
                .collect();
            normalized.type_params = impl_type_params
                .iter()
                .cloned()
                .chain(method.type_params.iter().cloned())
                .chain(renames.values().cloned())
                .collect();
            normalized.name = trait_name
                .as_deref()
                .map(|name| trait_method_symbol(name, &header_spelling, &method.name, &trait_args))
                .unwrap_or_else(|| struct_method_symbol(&target, &method.name));
            if let Some(first) = normalized.params.first_mut()
                && first.name == "self"
                && first.type_annotation.is_none()
            {
                first.type_annotation = Some(self_type.clone());
            }
            let saved_renames = std::mem::replace(&mut self.method_param_renames, renames.clone());
            let mut typed = self.infer_function(&normalized);
            self.method_param_renames = saved_renames;
            typed.own_type_params = method
                .type_params
                .iter()
                .map(|name| renames.get(name).cloned().unwrap_or_else(|| name.clone()))
                .collect();
            typed_methods.push(typed);
        }
        self.in_trait_default_body = saved_default_body;
        self.adopted_instantiation = saved_adoption;
        self.current_impl_header = saved_header;
        self.current_impl_self = saved_impl_self;
        crate::typed_ast::TypedStmtKind::ImplDecl {
            target,
            trait_name,
            type_params: impl_type_params.to_vec(),
            target_type,
            trait_args,
            methods: typed_methods,
        }
    }

    fn impl_trait_args(
        &mut self,
        trait_path: Option<&TypeAnnotation>,
        impl_type_params: &[String],
    ) -> Vec<InferType> {
        let Some(path) = trait_path else {
            return Vec::new();
        };
        let saved = std::mem::replace(&mut self.type_params_in_scope, impl_type_params.to_vec());
        let args = path
            .type_params
            .iter()
            .map(|argument| {
                self.type_from_annotation_as(crate::infer::OccurrenceRole::ImplHeader, argument)
            })
            .collect();
        self.type_params_in_scope = saved;
        args
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

fn body_yields_value(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_returns_value) || stmts.last().is_some_and(tail_yields_value)
}

fn stmt_returns_value(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(value) => value.is_some(),
        StmtKind::Block(stmts) => stmts.iter().any(stmt_returns_value),
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_value(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| stmt_returns_value(branch))
        }
        StmtKind::While { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForEach { body, .. } => stmt_returns_value(body),
        _ => false,
    }
}

fn tail_yields_value(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expression(_) => true,
        StmtKind::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => tail_yields_value(then_branch) || tail_yields_value(else_branch),
        StmtKind::Block(stmts) => stmts.last().is_some_and(tail_yields_value),
        _ => false,
    }
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
