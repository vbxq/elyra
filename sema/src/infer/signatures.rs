use super::functions::{struct_method_symbol, trait_method_symbol};
use super::{AssociatedTypeDefinition, TypeInference};
use crate::types::{InferType, StructMethod, TraitDef, TraitImplDef, TraitMethod};
use aelys_syntax::{Function, Stmt, StmtKind, TraitMethod as SyntaxTraitMethod};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

fn is_foreign_impl_target(ty: &InferType, type_table: &crate::types::TypeTable) -> bool {
    match ty {
        InferType::Struct(name) | InferType::Applied { name, .. } => !type_table.has_nominal(name),
        InferType::Option(_)
        | InferType::Result(_, _)
        | InferType::Array(_)
        | InferType::FixedArray(_, _)
        | InferType::Vec(_)
        | InferType::Tuple(_)
        | InferType::Function { .. }
        | InferType::Param(_)
        | InferType::Var(_)
        | InferType::Dynamic
        | InferType::I8
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
        | InferType::String
        | InferType::Unit
        | InferType::Null
        | InferType::Error
        | InferType::Never
        | InferType::Numeric
        | InferType::UntypedNative(_)
        | InferType::Poison
        | InferType::Range
        | InferType::Projection { .. } => true,
    }
}

impl TypeInference {
    pub(super) fn collect_traits(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if let StmtKind::TraitDecl {
                name,
                type_params,
                super_bounds,
                methods,
                associated_types,
                associated_consts,
                ..
            } = &stmt.kind
            {
                for method in methods.iter().filter(|method| method.has_body) {
                    self.trait_defaults.insert(
                        (name.clone(), method.function.name.clone()),
                        method.function.clone(),
                    );
                }
                self.collect_trait_signature(
                    name,
                    type_params,
                    super_bounds,
                    methods,
                    associated_types,
                    associated_consts,
                );
            }
        }
    }

    pub(super) fn collect_signatures(&mut self, stmts: &[Stmt], prefix: &str) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Function(func) => {
                    self.collect_function_signature(func, prefix);
                }
                StmtKind::ImplDecl {
                    type_params,
                    self_type,
                    trait_path,
                    where_clauses,
                    methods,
                    associated_types,
                    associated_consts,
                } => {
                    self.collect_impl_signatures(
                        self_type,
                        type_params,
                        methods,
                        trait_path.as_ref(),
                        where_clauses,
                        associated_types,
                        associated_consts,
                    );
                }
                StmtKind::TraitDecl { .. } => {}
                StmtKind::Block(inner_stmts) => {
                    self.collect_signatures(inner_stmts, prefix);
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.collect_signatures_from_stmt(then_branch, prefix);
                    if let Some(else_branch) = else_branch {
                        self.collect_signatures_from_stmt(else_branch, prefix);
                    }
                }
                StmtKind::While { body, .. } => {
                    self.collect_signatures_from_stmt(body, prefix);
                }
                StmtKind::For { body, .. } => {
                    self.collect_signatures_from_stmt(body, prefix);
                }
                StmtKind::ForEach { body, .. } => {
                    self.collect_signatures_from_stmt(body, prefix);
                }
                _ => {}
            }
        }
    }

    fn collect_trait_signature(
        &mut self,
        name: &str,
        type_params: &[String],
        super_bounds: &[aelys_syntax::TypeAnnotation],
        methods: &[SyntaxTraitMethod],
        associated_types: &[aelys_syntax::AssociatedTypeDecl],
        associated_consts: &[aelys_syntax::AssociatedConstDecl],
    ) {
        if self.type_table.get_trait(name).is_some() {
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::DuplicateStruct {
                    name: name.to_string(),
                },
                span: methods
                    .first()
                    .map_or_else(aelys_syntax::Span::dummy, |method| method.function.span),
                reason: crate::constraint::ConstraintReason::Other("trait declaration".to_string()),
            });
            return;
        }

        let mut seen_methods = std::collections::HashSet::new();
        for method in methods {
            if !seen_methods.insert(method.function.name.clone()) {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::DuplicateTraitMethod {
                        trait_name: name.to_string(),
                        method: method.function.name.clone(),
                    },
                    span: method.function.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait method set".to_string(),
                    ),
                });
            }
        }
        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, type_params.to_vec());
        let saved_trait = std::mem::replace(&mut self.current_trait_name, Some(name.to_string()));
        let mut trait_items = Vec::new();
        trait_items.extend(associated_types.iter().map(|item| item.name.clone()));
        trait_items.extend(associated_consts.iter().map(|item| item.name.clone()));
        let saved_trait_items =
            std::mem::replace(&mut self.current_trait_associated_items, trait_items);
        let typed_methods = methods
            .iter()
            .map(|method| {
                self.trait_method_signature(&method.function, type_params, method.has_body)
            })
            .collect();
        let typed_associated_consts = associated_consts
            .iter()
            .map(|item| {
                let ty = self.type_from_annotation(&item.type_annotation);
                (item.name.clone(), ty)
            })
            .collect();
        self.type_params_in_scope = saved_type_params;
        self.current_trait_name = saved_trait;
        self.current_trait_associated_items = saved_trait_items;
        self.type_table.register_trait(TraitDef {
            name: name.to_string(),
            type_params: type_params.to_vec(),
            super_bounds: super_bounds
                .iter()
                .map(|bound| bound.path.join("::"))
                .collect(),
            methods: typed_methods,
            associated_types: associated_types
                .iter()
                .map(|item| item.name.clone())
                .collect(),
            associated_consts: typed_associated_consts,
        });
    }

    fn trait_method_signature(
        &mut self,
        method: &Function,
        trait_type_params: &[String],
        has_body: bool,
    ) -> TraitMethod {
        let has_self = method
            .params
            .first()
            .is_some_and(|param| param.name == "self");
        let mut method_type_params = trait_type_params.to_vec();
        method_type_params.extend(method.type_params.iter().cloned());
        method_type_params.push("Self".to_string());
        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, method_type_params);
        let params = method
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                if has_self && index == 0 {
                    InferType::Param("Self".to_string())
                } else {
                    param
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.type_from_parameter_annotation(annotation))
                        .unwrap_or_else(|| self.type_gen.fresh())
                }
            })
            .collect();
        let return_type = method
            .return_type
            .as_ref()
            .map(|annotation| self.type_from_annotation(annotation))
            .unwrap_or(InferType::Unit);
        self.type_params_in_scope = saved_type_params;
        TraitMethod {
            name: method.name.clone(),
            symbol: String::new(),
            params,
            return_type,
            has_self,
            mutable_self: has_self && method.params[0].mutable,
            has_body,
        }
    }

    fn collect_signatures_from_stmt(&mut self, stmt: &Stmt, prefix: &str) {
        match &stmt.kind {
            StmtKind::Function(func) => {
                self.collect_function_signature(func, prefix);
            }
            StmtKind::Block(stmts) => {
                self.collect_signatures(stmts, prefix);
            }
            _ => {}
        }
    }

    fn collect_function_signature(&mut self, func: &Function, prefix: &str) {
        let full_name = if prefix.is_empty() {
            func.name.clone()
        } else {
            format!("{}::{}", prefix, func.name)
        };

        let bounds = self.bounds_from_where_clauses(&func.where_clauses);
        let bindings = self.bindings_from_where_clauses(&func.where_clauses);

        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, func.type_params.clone());
        let saved_bounds = std::mem::replace(&mut self.current_function_bounds, bounds.clone());
        let saved_bindings =
            std::mem::replace(&mut self.current_function_bindings, bindings.clone());

        let mut param_types = Vec::with_capacity(func.params.len());
        for p in &func.params {
            let ty = match &p.type_annotation {
                Some(ann) => self.type_from_parameter_annotation(ann),
                None => self.type_gen.fresh(),
            };
            param_types.push(ty);
        }

        let ret_type = match &func.return_type {
            Some(ann) => self.type_from_annotation(ann),
            None => self.type_gen.fresh(),
        };

        let reference_modes: Vec<_> = func.params.iter().map(|param| param.reference).collect();

        if ret_type.contains_dynamic() {
            self.explicit_dynamic_functions.insert(full_name.clone());
            if !prefix.is_empty() {
                self.explicit_dynamic_functions.insert(func.name.clone());
            }
        }

        self.type_params_in_scope = saved_type_params;
        self.current_function_bounds = saved_bounds;
        self.current_function_bindings = saved_bindings;

        let fn_type = Rc::new(InferType::Function {
            params: param_types,
            ret: Box::new(ret_type),
        });

        if !bounds.is_empty() {
            self.generic_function_bounds
                .insert(full_name.clone(), bounds.clone());
            if !prefix.is_empty() {
                self.generic_function_bounds
                    .insert(func.name.clone(), bounds);
            }
        }
        if !bindings.is_empty() {
            self.generic_function_bindings
                .insert(full_name.clone(), bindings.clone());
            if !prefix.is_empty() {
                self.generic_function_bindings
                    .insert(func.name.clone(), bindings);
            }
        }

        self.function_type_params
            .insert(full_name.clone(), func.type_params.clone());
        self.function_reference_modes
            .insert(full_name.clone(), reference_modes.clone());
        if !prefix.is_empty() {
            self.function_type_params
                .insert(func.name.clone(), func.type_params.clone());
            self.function_reference_modes
                .insert(func.name.clone(), reference_modes);
        }

        self.env.define_function(full_name, Rc::clone(&fn_type));
        if !prefix.is_empty() {
            self.env.define_function(func.name.clone(), fn_type);
        }
    }

    fn bounds_from_where_clauses(
        &mut self,
        clauses: &[aelys_syntax::WhereClause],
    ) -> Vec<(String, String, Vec<InferType>)> {
        let mut bounds = Vec::new();
        for clause in clauses {
            for bound in &clause.bounds {
                let trait_args = bound
                    .type_params
                    .iter()
                    .map(|argument| self.type_from_annotation(argument))
                    .collect();
                let trait_name = bound.path.join("::");
                if let Some(module) = self.withholding_module(&trait_name).map(str::to_string) {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::TypeNotImported {
                            name: trait_name.clone(),
                            module,
                        },
                        span: clause.type_annotation.span,
                        reason: crate::constraint::ConstraintReason::UnknownType {
                            name: trait_name.clone(),
                        },
                    });
                }
                bounds.push((clause.type_annotation.name.clone(), trait_name, trait_args));
            }
        }
        bounds
    }

    fn bindings_from_where_clauses(
        &mut self,
        clauses: &[aelys_syntax::WhereClause],
    ) -> Vec<(String, String, Vec<(String, InferType)>)> {
        let mut bindings = Vec::new();
        for clause in clauses {
            for bound in &clause.bounds {
                if bound.associated_bindings.is_empty() {
                    continue;
                }
                let trait_name = bound.path.join("::");
                let items = bound
                    .associated_bindings
                    .iter()
                    .map(|(name, annotation)| {
                        let ty = self.type_from_annotation(annotation);
                        (name.clone(), ty)
                    })
                    .collect();
                bindings.push((clause.type_annotation.name.clone(), trait_name, items));
            }
        }
        bindings
    }

    fn collect_impl_signatures(
        &mut self,
        self_type: &aelys_syntax::TypeAnnotation,
        impl_type_params: &[String],
        methods: &[Function],
        trait_path: Option<&aelys_syntax::TypeAnnotation>,
        where_clauses: &[aelys_syntax::WhereClause],
        associated_types: &[aelys_syntax::AssociatedTypeDef],
        associated_consts: &[aelys_syntax::AssociatedConstDef],
    ) {
        let trait_name = trait_path.map(|path| path.path.join("::"));
        let target = self_type
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| self_type.name.clone());
        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, impl_type_params.to_vec());
        let target_ty = self.type_from_annotation(self_type);
        let trait_args = trait_path
            .map(|path| {
                path.type_params
                    .iter()
                    .map(|argument| self.type_from_annotation(argument))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let impl_bounds = self.bounds_from_where_clauses(where_clauses);
        self.type_params_in_scope = saved_type_params;
        let saved_impl_self = self.current_impl_self.replace(target_ty.clone());
        let effective_methods = self.effective_impl_methods(methods, trait_name.as_deref());
        let methods = effective_methods.as_slice();
        if trait_name.is_some() && is_foreign_impl_target(&target_ty, &self.type_table) {
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::OrphanTraitImpl {
                    trait_name: trait_name.clone().unwrap_or_default(),
                    target: target_ty.clone(),
                },
                span: methods
                    .first()
                    .map_or_else(aelys_syntax::Span::dummy, |m| m.span),
                reason: crate::constraint::ConstraintReason::Other("trait coherence".to_string()),
            });
            return;
        }
        if !self.type_table.has_nominal(&target) {
            self.errors.push(crate::constraint::TypeError {
                kind: self.nominal_error_kind(
                    &target,
                    crate::constraint::TypeErrorKind::UnknownStruct {
                        name: target.clone(),
                    },
                ),
                span: methods
                    .first()
                    .map_or_else(aelys_syntax::Span::dummy, |m| m.span),
                reason: crate::constraint::ConstraintReason::UnknownType {
                    name: target.clone(),
                },
            });
            return;
        }

        if let Some(trait_name) = trait_name.as_deref() {
            let Some(trait_def) = self.type_table.get_trait(trait_name).cloned() else {
                self.errors.push(crate::constraint::TypeError {
                    kind: self.nominal_error_kind(
                        trait_name,
                        crate::constraint::TypeErrorKind::UnknownTrait {
                            name: trait_name.to_string(),
                        },
                    ),
                    span: methods
                        .first()
                        .map_or_else(aelys_syntax::Span::dummy, |method| method.span),
                    reason: crate::constraint::ConstraintReason::UnknownType {
                        name: trait_name.to_string(),
                    },
                });
                return;
            };
            if crate::prelude::reserves_header(trait_name, &target_ty, &trait_args) {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::ReservedIdentityConversion {
                        ty: target_ty.clone(),
                    },
                    span: methods
                        .first()
                        .map_or_else(aelys_syntax::Span::dummy, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return;
            }
            if !self.type_table.register_trait_impl_with_args(
                trait_name.to_string(),
                target_ty.to_string(),
                &trait_args,
            ) {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::DuplicateTraitImpl {
                        trait_name: trait_name.to_string(),
                        target: target.to_string(),
                    },
                    span: methods
                        .first()
                        .map_or_else(aelys_syntax::Span::dummy, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return;
            }
            if self
                .type_table
                .trait_impl_overlaps(trait_name, &trait_args, &target_ty)
            {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::OverlappingTraitImpl {
                        trait_name: trait_name.to_string(),
                        target: target_ty.clone(),
                    },
                    span: methods
                        .first()
                        .map_or_else(aelys_syntax::Span::dummy, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return;
            }
            for method in methods {
                let Some(required) = trait_def
                    .methods
                    .iter()
                    .find(|candidate| candidate.name == method.name)
                else {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::TraitMethodNotInTrait {
                            trait_name: trait_name.to_string(),
                            method: method.name.clone(),
                        },
                        span: method.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method set".to_string(),
                        ),
                    });
                    continue;
                };
                let has_self = method
                    .params
                    .first()
                    .is_some_and(|param| param.name == "self");
                if has_self != required.has_self || method.params.len() != required.params.len() {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::TraitMethodSignatureMismatch {
                            trait_name: trait_name.to_string(),
                            method: method.name.clone(),
                        },
                        span: method.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method signature".to_string(),
                        ),
                    });
                }
            }
            for required in &trait_def.methods {
                if !required.has_body && !methods.iter().any(|method| method.name == required.name)
                {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::MissingTraitMethod {
                            trait_name: trait_name.to_string(),
                            method: required.name.clone(),
                        },
                        span: methods
                            .first()
                            .map_or_else(aelys_syntax::Span::dummy, |method| method.span),
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method set".to_string(),
                        ),
                    });
                }
            }
            for required_type in &trait_def.associated_types {
                let mut declarations = associated_types
                    .iter()
                    .filter(|item| &item.name == required_type);
                let first = declarations.next();
                let second = declarations.next();
                if first.is_none() {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::MissingAssociatedItem {
                            trait_name: trait_name.to_string(),
                            item: required_type.clone(),
                        },
                        span: self_type.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "associated item set".to_string(),
                        ),
                    });
                } else if let Some(duplicate) = second {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::DuplicateAssociatedItem {
                            trait_name: trait_name.to_string(),
                            item: required_type.clone(),
                        },
                        span: duplicate.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "associated item set".to_string(),
                        ),
                    });
                }
            }
            for (required_name, _required_ty) in &trait_def.associated_consts {
                let mut declarations = associated_consts
                    .iter()
                    .filter(|item| &item.name == required_name);
                let first = declarations.next();
                let second = declarations.next();
                if first.is_none() {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::MissingAssociatedItem {
                            trait_name: trait_name.to_string(),
                            item: required_name.clone(),
                        },
                        span: self_type.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "associated item set".to_string(),
                        ),
                    });
                } else if let Some(duplicate) = second {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::DuplicateAssociatedItem {
                            trait_name: trait_name.to_string(),
                            item: required_name.clone(),
                        },
                        span: duplicate.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "associated item set".to_string(),
                        ),
                    });
                }
            }
            for item in associated_consts {
                if let Some((_, required_ty)) = trait_def
                    .associated_consts
                    .iter()
                    .find(|(name, _)| name == &item.name)
                {
                    let declared_ty = self.type_from_annotation(&item.type_annotation);
                    if !self.type_table.types_match(required_ty, &declared_ty) {
                        self.errors.push(crate::constraint::TypeError {
                            kind: crate::constraint::TypeErrorKind::AssociatedItemTypeMismatch {
                                trait_name: trait_name.to_string(),
                                item: item.name.clone(),
                            },
                            span: item.span,
                            reason: crate::constraint::ConstraintReason::Other(
                                "associated constant type".to_string(),
                            ),
                        });
                        continue;
                    }
                    // constant is evaluated, so `const l: int = "ten"` cannot
                    let value_ty = self.infer_const_expr_type(&item.value);
                    if !matches!(value_ty, InferType::Poison)
                        && !self.type_table.types_match(&declared_ty, &value_ty)
                    {
                        self.errors.push(crate::constraint::TypeError {
                            kind: crate::constraint::TypeErrorKind::AssociatedItemTypeMismatch {
                                trait_name: trait_name.to_string(),
                                item: item.name.clone(),
                            },
                            span: item.span,
                            reason: crate::constraint::ConstraintReason::Other(
                                "associated constant value".to_string(),
                            ),
                        });
                    }
                }
            }
        }

        let mut registered_trait_methods = Vec::new();
        for method in methods {
            let has_self = method.params.first().is_some_and(|p| p.name == "self");
            if method
                .params
                .iter()
                .skip(usize::from(has_self))
                .any(|p| p.name == "self")
            {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::InvalidStructMethod {
                        method: method.name.clone(),
                        structure: target.to_string(),
                    },
                    span: method.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "struct method receiver".to_string(),
                    ),
                });
                continue;
            }
            if trait_name.is_none() && self.type_table.method(&target, &method.name).is_some() {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::InvalidStructMethod {
                        method: method.name.clone(),
                        structure: target.to_string(),
                    },
                    span: method.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "duplicate struct method".to_string(),
                    ),
                });
                continue;
            }

            let mut method_type_params = impl_type_params.to_vec();
            method_type_params.extend(method.type_params.iter().cloned());
            method_type_params.push("Self".to_string());
            let saved_type_params =
                std::mem::replace(&mut self.type_params_in_scope, method_type_params);
            let mut params = Vec::with_capacity(method.params.len());
            for (index, param) in method.params.iter().enumerate() {
                let ty = if has_self && index == 0 {
                    target_ty.clone()
                } else {
                    param
                        .type_annotation
                        .as_ref()
                        .map(|ann| self.type_from_annotation(ann))
                        .unwrap_or_else(|| self.type_gen.fresh())
                };
                params.push(ty);
            }
            let mut ret = method
                .return_type
                .as_ref()
                .map(|ann| self.type_from_annotation(ann))
                .unwrap_or_else(|| self.type_gen.fresh());
            let self_substitution = HashMap::from([("Self".to_string(), target_ty.clone())]);
            params = params
                .into_iter()
                .map(|param| param.substitute_params(&self_substitution))
                .collect();
            ret = ret.substitute_params(&self_substitution);
            let mut method_bounds = impl_bounds.clone();
            method_bounds.extend(self.bounds_from_where_clauses(&method.where_clauses));
            self.type_params_in_scope = saved_type_params;
            if let Some(trait_name) = trait_name.as_deref()
                && let Some(required) =
                    self.type_table
                        .get_trait(trait_name)
                        .and_then(|definition| {
                            definition
                                .methods
                                .iter()
                                .find(|candidate| candidate.name == method.name)
                        })
            {
                let mut substitutions = HashMap::from([("Self".to_string(), target_ty.clone())]);
                if let Some(trait_def) = self.type_table.get_trait(trait_name) {
                    for (parameter, argument) in trait_def.type_params.iter().zip(&trait_args) {
                        substitutions.insert(parameter.clone(), argument.clone());
                    }
                }
                let expected_params: Vec<_> = required
                    .params
                    .iter()
                    .map(|param| param.substitute_params(&substitutions))
                    .collect();
                let expected_return = required.return_type.substitute_params(&substitutions);
                let expected_return =
                    self.normalize_projection_in_signature(&expected_return, associated_types);
                let signature_matches = params.len() == expected_params.len()
                    && params
                        .iter()
                        .zip(expected_params.iter())
                        .all(|(actual, expected)| self.type_table.types_match(expected, actual))
                    && self.type_table.types_match(&expected_return, &ret);
                if !signature_matches {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::TraitMethodSignatureMismatch {
                            trait_name: trait_name.to_string(),
                            method: method.name.clone(),
                        },
                        span: method.span,
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method signature".to_string(),
                        ),
                    });
                }
            }
            let symbol = trait_name
                .as_deref()
                .map(|name| trait_method_symbol(name, &target, &method.name, &trait_args))
                .unwrap_or_else(|| struct_method_symbol(&target, &method.name));
            self.function_reference_modes.insert(
                symbol.clone(),
                method.params.iter().map(|param| param.reference).collect(),
            );
            if !method_bounds.is_empty() {
                self.generic_function_bounds
                    .insert(symbol.clone(), method_bounds);
            }
            self.env.define_function(
                symbol.clone(),
                Rc::new(InferType::Function {
                    params: params.clone(),
                    ret: Box::new(ret.clone()),
                }),
            );
            if trait_name.is_some() {
                let trait_method = TraitMethod {
                    name: method.name.clone(),
                    symbol,
                    params,
                    return_type: ret,
                    has_self,
                    mutable_self: has_self && method.params[0].mutable,
                    has_body: false,
                };
                self.type_table
                    .register_trait_method(target.clone(), trait_method.clone());
                registered_trait_methods.push(trait_method);
            } else {
                self.type_table.register_method(
                    target.to_string(),
                    StructMethod {
                        name: method.name.clone(),
                        symbol,
                        params,
                        return_type: ret,
                        has_self,
                        mutable_self: has_self && method.params[0].mutable,
                    },
                );
            }
        }
        if trait_name.is_none() {
            // rejecting here keeps them from being silently dropped and later
            for item in associated_types {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                        target: target.clone(),
                        item: item.name.clone(),
                        keyword: "type",
                    },
                    span: item.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "associated item set".to_string(),
                    ),
                });
            }
            for item in associated_consts {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                        target: target.clone(),
                        item: item.name.clone(),
                        keyword: "const",
                    },
                    span: item.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "associated item set".to_string(),
                    ),
                });
            }
        }
        if let Some(trait_name) = trait_name.as_deref() {
            let self_binding = HashMap::from([("Self".to_string(), target_ty.clone())]);
            let saved_defer = std::mem::replace(&mut self.defer_projection_resolution, true);
            let typed_associated_types: Vec<(String, InferType)> = associated_types
                .iter()
                .map(|item| {
                    let ty = self.type_from_annotation(&item.value);
                    (item.name.clone(), ty.substitute_params(&self_binding))
                })
                .collect();
            self.defer_projection_resolution = saved_defer;
            let receiver = target_ty.to_string();
            for definition in associated_types {
                let mut edges = Vec::new();
                collect_annotation_edges(&definition.value, &receiver, &mut edges);
                self.associated_type_definitions
                    .push(crate::infer::AssociatedTypeDefinition {
                        receiver: receiver.clone(),
                        item: definition.name.clone(),
                        edges,
                        span: definition.span,
                    });
            }
            for definition in associated_consts {
                self.associated_const_exprs.insert(
                    (receiver.clone(), definition.name.clone()),
                    (receiver.clone(), definition.value.clone()),
                );
                let mut edges = Vec::new();
                collect_expr_edges(&definition.value, &receiver, &mut edges);
                self.associated_type_definitions
                    .push(crate::infer::AssociatedTypeDefinition {
                        receiver: receiver.clone(),
                        item: definition.name.clone(),
                        edges,
                        span: definition.span,
                    });
            }
            let typed_associated_consts = associated_consts
                .iter()
                .map(|item| {
                    let declared = self.type_from_annotation(&item.type_annotation);
                    let value_ty = self.infer_const_expr_type(&item.value);
                    let value = constant_int_literal(&item.value);
                    (item.name.clone(), declared, value_ty, value)
                })
                .collect();
            self.type_table.register_trait_impl_def(TraitImplDef {
                trait_name: trait_name.to_string(),
                trait_args,
                self_type: target_ty,
                methods: registered_trait_methods,
                associated_types: typed_associated_types,
                associated_consts: typed_associated_consts,
            });
        }
        self.current_impl_self = saved_impl_self;
    }

    pub(super) fn infer_const_expr_type(&mut self, expr: &aelys_syntax::Expr) -> InferType {
        use aelys_syntax::ExprKind;
        match &expr.kind {
            ExprKind::Int(_) => InferType::I64,
            ExprKind::Float(_) => InferType::F64,
            ExprKind::Bool(_) => InferType::Bool,
            ExprKind::String(_) => InferType::String,
            ExprKind::Unary { op, operand } => {
                use aelys_syntax::UnaryOp;
                match op {
                    UnaryOp::Neg => self.infer_const_expr_type(operand),
                    UnaryOp::Not => self.infer_const_expr_type(operand),
                    _ => InferType::I64,
                }
            }
            ExprKind::Binary { left, right, .. } => {
                let left_ty = self.infer_const_expr_type(left);
                let right_ty = self.infer_const_expr_type(right);
                if left_ty.is_integer() && right_ty.is_integer() {
                    InferType::I64
                } else {
                    InferType::F64
                }
            }
            ExprKind::Grouping(inner) => self.infer_const_expr_type(inner),
            _ => InferType::I64,
        }
    }

    fn normalize_projection_in_signature(
        &mut self,
        ty: &InferType,
        associated_types: &[aelys_syntax::AssociatedTypeDef],
    ) -> InferType {
        match ty {
            InferType::Projection {
                trait_name: _,
                item,
                self_ty,
            } => {
                if let Some(definition) = associated_types
                    .iter()
                    .find(|candidate| &candidate.name == item)
                {
                    return self.type_from_annotation(&definition.value);
                }
                InferType::Projection {
                    trait_name: None,
                    item: item.clone(),
                    self_ty: Box::new(
                        self.normalize_projection_in_signature(self_ty, associated_types),
                    ),
                }
            }
            InferType::Function { params, ret } => InferType::Function {
                params: params
                    .iter()
                    .map(|param| self.normalize_projection_in_signature(param, associated_types))
                    .collect(),
                ret: Box::new(self.normalize_projection_in_signature(ret, associated_types)),
            },
            InferType::Array(inner) => InferType::Array(Box::new(
                self.normalize_projection_in_signature(inner, associated_types),
            )),
            InferType::FixedArray(inner, length) => InferType::FixedArray(
                Box::new(self.normalize_projection_in_signature(inner, associated_types)),
                *length,
            ),
            InferType::Vec(inner) => InferType::Vec(Box::new(
                self.normalize_projection_in_signature(inner, associated_types),
            )),
            InferType::Option(inner) => InferType::Option(Box::new(
                self.normalize_projection_in_signature(inner, associated_types),
            )),
            InferType::Result(ok, err) => InferType::Result(
                Box::new(self.normalize_projection_in_signature(ok, associated_types)),
                Box::new(self.normalize_projection_in_signature(err, associated_types)),
            ),
            InferType::Tuple(elements) => InferType::Tuple(
                elements
                    .iter()
                    .map(|element| {
                        self.normalize_projection_in_signature(element, associated_types)
                    })
                    .collect(),
            ),
            InferType::Applied { name, args } => InferType::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.normalize_projection_in_signature(arg, associated_types))
                    .collect(),
            },
            _ => ty.clone(),
        }
    }

    pub(super) fn effective_impl_methods(
        &self,
        methods: &[Function],
        trait_name: Option<&str>,
    ) -> Vec<Function> {
        let mut effective = methods.to_vec();
        let Some(trait_name) = trait_name else {
            return effective;
        };
        let Some(trait_def) = self.type_table.get_trait(trait_name) else {
            return effective;
        };
        for required in trait_def.methods.iter().filter(|method| method.has_body) {
            if effective.iter().any(|method| method.name == required.name) {
                continue;
            }
            if let Some(default) = self
                .trait_defaults
                .get(&(trait_name.to_string(), required.name.clone()))
            {
                effective.push(default.clone());
            }
        }
        effective
    }
}

/// overflowing or dividing-by-zero constant stays unresolved and is diagnosed
fn constant_int_literal(expr: &aelys_syntax::Expr) -> Option<i64> {
    use aelys_syntax::{BinaryOp, ExprKind, UnaryOp};
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => constant_int_literal(operand).and_then(i64::checked_neg),
        ExprKind::Grouping(inner) => constant_int_literal(inner),
        ExprKind::Binary { left, op, right } => {
            let left = constant_int_literal(left)?;
            let right = constant_int_literal(right)?;
            match op {
                BinaryOp::Add => left.checked_add(right),
                BinaryOp::Sub => left.checked_sub(right),
                BinaryOp::Mul => left.checked_mul(right),
                BinaryOp::Div => left.checked_div(right),
                BinaryOp::Mod => left.checked_rem(right),
                _ => None,
            }
        }
        _ => None,
    }
}

impl TypeInference {
    /// used: an unused cycle would otherwise compile silently, and a used one
    pub(super) fn validate_associated_type_cycles(&mut self) {
        let definitions = std::mem::take(&mut self.associated_type_definitions);
        let mut settled: HashSet<(String, String)> = HashSet::new();
        let mut reported: HashSet<(String, String)> = HashSet::new();
        for definition in &definitions {
            let origin = (definition.receiver.clone(), definition.item.clone());
            if settled.contains(&origin) {
                continue;
            }
            let mut stack = Vec::new();
            if let Some(cycle) =
                Self::find_projection_cycle(&definitions, &origin, &mut stack, &mut settled)
            {
                let opener = cycle[0].clone();
                if !reported.insert(opener.clone()) {
                    continue;
                }
                let Some(culprit) = definitions.iter().find(|candidate| {
                    (candidate.receiver.clone(), candidate.item.clone()) == opener
                }) else {
                    continue;
                };
                let path = cycle
                    .iter()
                    .chain(std::iter::once(&cycle[0]))
                    .map(|(receiver, item)| format!("{receiver}::{item}"))
                    .collect();
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver: culprit.receiver.clone(),
                        item: culprit.item.clone(),
                        cause: crate::constraint::ProjectionFailure::Cyclic { path },
                    },
                    span: culprit.span,
                    reason: crate::constraint::ConstraintReason::UnknownType {
                        name: format!("{}::{}", culprit.receiver, culprit.item),
                    },
                });
            }
        }
        self.associated_type_definitions = definitions;
    }

    fn find_projection_cycle(
        definitions: &[AssociatedTypeDefinition],
        node: &(String, String),
        stack: &mut Vec<(String, String)>,
        settled: &mut HashSet<(String, String)>,
    ) -> Option<Vec<(String, String)>> {
        if let Some(position) = stack.iter().position(|entry| entry == node) {
            return Some(stack[position..].to_vec());
        }
        if settled.contains(node) {
            return None;
        }
        let Some(definition) = definitions
            .iter()
            .find(|candidate| candidate.receiver == node.0 && candidate.item == node.1)
        else {
            settled.insert(node.clone());
            return None;
        };
        stack.push(node.clone());
        for edge in definition.edges.clone() {
            if let Some(cycle) = Self::find_projection_cycle(definitions, &edge, stack, settled) {
                stack.pop();
                return Some(cycle);
            }
        }
        stack.pop();
        settled.insert(node.clone());
        None
    }
}

fn collect_annotation_edges(
    annotation: &aelys_syntax::TypeAnnotation,
    target: &str,
    out: &mut Vec<(String, String)>,
) {
    if annotation.path.len() >= 2 {
        let receiver = &annotation.path[0];
        let receiver = if receiver == "Self" { target } else { receiver };
        if let Some(item) = annotation.path.last() {
            out.push((receiver.to_string(), item.clone()));
        }
    }
    if let Some(segments) = annotation.array_length_path.as_ref() {
        if segments.len() >= 2 {
            let receiver = &segments[0];
            let receiver = if receiver == "Self" { target } else { receiver };
            if let Some(item) = segments.last() {
                out.push((receiver.to_string(), item.clone()));
            }
        }
    }
    for param in &annotation.type_params {
        collect_annotation_edges(param, target, out);
    }
    for (_, bound) in &annotation.associated_bindings {
        collect_annotation_edges(bound, target, out);
    }
    if let Some(params) = annotation.fn_params.as_ref() {
        for param in params {
            collect_annotation_edges(param, target, out);
        }
    }
    if let Some(ret) = annotation.fn_ret.as_ref() {
        collect_annotation_edges(ret, target, out);
    }
}

fn collect_expr_edges(expr: &aelys_syntax::Expr, target: &str, out: &mut Vec<(String, String)>) {
    use aelys_syntax::ExprKind;
    match &expr.kind {
        ExprKind::Member {
            object,
            member,
            separator,
        } => {
            if matches!(separator, aelys_syntax::MemberSeparator::Path) {
                if let ExprKind::Identifier(name) = &object.kind {
                    let receiver = if name == "Self" { target } else { name };
                    out.push((receiver.to_string(), member.clone()));
                }
            }
            collect_expr_edges(object, target, out);
        }
        ExprKind::Binary { left, right, .. } => {
            collect_expr_edges(left, target, out);
            collect_expr_edges(right, target, out);
        }
        ExprKind::Unary { operand, .. } => collect_expr_edges(operand, target, out),
        ExprKind::Grouping(inner) => collect_expr_edges(inner, target, out),
        ExprKind::Call { callee, args } => {
            collect_expr_edges(callee, target, out);
            for arg in args {
                collect_expr_edges(arg, target, out);
            }
        }
        _ => {}
    }
}

impl TypeInference {
    pub(super) fn collect_trait_qualified_items(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::ImplDecl {
                    self_type,
                    trait_path,
                    associated_types,
                    associated_consts,
                    ..
                } => {
                    let Some(trait_path) = trait_path else {
                        continue;
                    };
                    let trait_name = trait_path.path.join("::");
                    let target = self_type
                        .path
                        .last()
                        .cloned()
                        .unwrap_or_else(|| self_type.name.clone());
                    for item in associated_types {
                        self.trait_qualified_items
                            .entry((trait_name.clone(), item.name.clone()))
                            .or_default()
                            .push(target.clone());
                    }
                    for item in associated_consts {
                        self.trait_qualified_items
                            .entry((trait_name.clone(), item.name.clone()))
                            .or_default()
                            .push(target.clone());
                        self.associated_const_exprs
                            .entry((target.clone(), item.name.clone()))
                            .or_insert_with(|| (target.clone(), item.value.clone()));
                    }
                }
                StmtKind::Block(inner) => self.collect_trait_qualified_items(inner),
                _ => {}
            }
        }
    }
}
