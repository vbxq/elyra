use super::functions::{struct_method_symbol, trait_method_symbol};
use super::{AssociatedBindings, AssociatedTypeDefinition, ImplMethodSignature, TypeInference};
use crate::types::{InferType, StructMethod, TraitDef, TraitImplDef, TraitMethod};
use aelys_syntax::{Function, Stmt, StmtKind, TraitMethod as SyntaxTraitMethod};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

struct ImplBlock<'a> {
    self_type: &'a aelys_syntax::TypeAnnotation,
    impl_type_params: &'a [String],
    methods: &'a [Function],
    trait_path: Option<&'a aelys_syntax::TypeAnnotation>,
    where_clauses: &'a [aelys_syntax::WhereClause],
    associated_types: &'a [aelys_syntax::AssociatedTypeDef],
    associated_consts: &'a [aelys_syntax::AssociatedConstDef],
    definition_module: Option<&'a aelys_syntax::ModuleId>,
}

struct ImplName {
    target: String,
    self_ty: InferType,
    trait_name: Option<String>,
    slot: Option<usize>,
    definitions: (usize, usize),
}

pub(super) struct DeclaredImpls {
    names: Vec<ImplName>,
    live: Vec<usize>,
}

struct BlockModule {
    saved: aelys_syntax::ModuleId,
    name: String,
    errors: usize,
    constraints: usize,
}

struct ImplHeader {
    target: String,
    target_ty: InferType,
    trait_name: Option<String>,
    trait_args: Vec<InferType>,
    bounds: Vec<(String, String, Vec<InferType>)>,
    bindings: crate::infer::AssociatedBindings,
}

// `resolve_associated_projection` answers unresolved on this, but a comparison
fn bound_subjects(clauses: &[aelys_syntax::WhereClause]) -> Vec<(String, String, Vec<InferType>)> {
    let mut out = Vec::new();
    for clause in clauses {
        for bound in &clause.bounds {
            out.push((
                clause.type_annotation.name.clone(),
                bound.path.join("::"),
                Vec::new(),
            ));
        }
    }
    out
}

fn unresolved_associated_item(trait_name: &str, item: &str, self_ty: &InferType) -> InferType {
    InferType::Projection {
        trait_name: Some(trait_name.to_string()),
        item: item.to_string(),
        self_ty: Box::new(self_ty.clone()),
    }
}

fn gather_impl_blocks<'a>(stmts: &'a [Stmt], out: &mut Vec<ImplBlock<'a>>) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::ImplDecl {
                type_params,
                self_type,
                trait_path,
                where_clauses,
                methods,
                associated_types,
                associated_consts,
            } => {
                out.push(ImplBlock {
                    self_type,
                    impl_type_params: type_params,
                    methods,
                    trait_path: trait_path.as_ref(),
                    where_clauses,
                    associated_types,
                    associated_consts,
                    definition_module: stmt.definition_module.as_ref(),
                });
            }
            StmtKind::Block(inner_stmts) => gather_impl_blocks(inner_stmts, out),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                gather_impl_blocks_in_stmt(then_branch, out);
                if let Some(else_branch) = else_branch {
                    gather_impl_blocks_in_stmt(else_branch, out);
                }
            }
            StmtKind::While { body, .. } => gather_impl_blocks_in_stmt(body, out),
            StmtKind::For { body, .. } => gather_impl_blocks_in_stmt(body, out),
            StmtKind::ForEach { body, .. } => gather_impl_blocks_in_stmt(body, out),
            _ => {}
        }
    }
}

fn gather_impl_blocks_in_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<ImplBlock<'a>>) {
    if let StmtKind::Block(stmts) = &stmt.kind {
        gather_impl_blocks(stmts, out);
    }
}

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
    pub(super) fn declare_traits(&mut self, stmts: &[Stmt]) -> HashSet<String> {
        let mut accepted = HashSet::new();
        for stmt in stmts {
            let StmtKind::TraitDecl {
                name,
                type_params,
                super_bounds,
                associated_types,
                associated_consts,
                ..
            } = &stmt.kind
            else {
                continue;
            };
            // silently and in both namespaces, so the collision is refused here.
            if self.type_table.get_trait(name).is_some() || self.type_table.has_nominal(name) {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::DuplicateNominal {
                        name: name.clone(),
                        keyword: "trait",
                        collides_with: self.type_table.nominal_keyword(name),
                    },
                    span: stmt.span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait declaration".to_string(),
                    ),
                });
                continue;
            }
            self.type_table.register_trait(TraitDef {
                owner: self.current_module.clone(),
                name: name.clone(),
                type_params: type_params.clone(),
                super_bounds: super_bounds
                    .iter()
                    .map(|bound| bound.path.join("::"))
                    .collect(),
                methods: Vec::new(),
                associated_types: associated_types
                    .iter()
                    .map(|item| item.name.clone())
                    .collect(),
                associated_consts: associated_consts
                    .iter()
                    .map(|item| (item.name.clone(), InferType::Poison))
                    .collect(),
            });
            accepted.insert(name.clone());
        }
        accepted
    }

    pub(super) fn collect_traits(&mut self, stmts: &[Stmt], mut declared: HashSet<String>) {
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
                if !declared.remove(name) {
                    continue;
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

    // the imported impls, never against itself.
    pub(super) fn declare_impl_definitions(&mut self, stmts: &[Stmt]) -> DeclaredImpls {
        let mut blocks = Vec::new();
        gather_impl_blocks(stmts, &mut blocks);
        let live: Vec<usize> = (0..self.type_table.trait_impl_defs().len()).collect();
        let mut names = Vec::with_capacity(blocks.len());
        for block in &blocks {
            let saved = self.enter_block_module(block);
            let name = self.declare_impl_names(block);
            self.leave_block_module(saved);
            names.push(name);
        }
        DeclaredImpls { names, live }
    }

    pub(super) fn collect_impl_definitions(&mut self, stmts: &[Stmt]) {
        let declared = self.declare_impl_definitions(stmts);
        self.collect_declared_impls(stmts, declared);
    }

    pub(super) fn collect_declared_impls(&mut self, stmts: &[Stmt], declared: DeclaredImpls) {
        let mut blocks = Vec::new();
        gather_impl_blocks(stmts, &mut blocks);
        let DeclaredImpls {
            mut names,
            mut live,
        } = declared;
        debug_assert_eq!(blocks.len(), names.len());
        for (block, name) in blocks.iter().zip(names.iter_mut()) {
            let saved = self.enter_block_module(block);
            self.resolve_impl_item_definitions(block, name);
            self.leave_block_module(saved);
        }
        let mut headers = Vec::with_capacity(blocks.len());
        for (block, name) in blocks.iter().zip(&names) {
            let saved = self.enter_block_module(block);
            let header = self.resolve_impl_header(block, name);
            self.leave_block_module(saved);
            headers.push(header);
        }
        let mut accepted = Vec::with_capacity(blocks.len());
        for ((block, header), name) in blocks.iter().zip(&headers).zip(&names) {
            let saved = self.enter_block_module(block);
            let kept = self.check_impl_header(block, header, name, &mut live);
            self.leave_block_module(saved);
            accepted.push(kept);
        }
        self.retract_rejected_impls(&blocks, &mut names, &accepted);
        self.check_supertrait_obligations(&blocks, &headers, &accepted);
        for (((block, header), name), kept) in
            blocks.iter().zip(&headers).zip(&names).zip(&accepted)
        {
            if *kept {
                let saved = self.enter_block_module(block);
                self.collect_impl_associated_consts(block, header, name);
                self.leave_block_module(saved);
            }
        }
        for (((block, header), name), kept) in
            blocks.iter().zip(&headers).zip(&names).zip(&accepted)
        {
            if *kept {
                let saved = self.enter_block_module(block);
                self.collect_impl_method_signatures(block, header, name);
                self.leave_block_module(saved);
            }
        }
    }

    fn enter_block_module(&mut self, block: &ImplBlock<'_>) -> Option<BlockModule> {
        let module = block.definition_module?.clone();
        let name = module.as_str().to_string();
        let errors = self.errors.len();
        let constraints = self.constraints.len();
        Some(BlockModule {
            saved: std::mem::replace(&mut self.current_module, module),
            name,
            errors,
            constraints,
        })
    }

    fn leave_block_module(&mut self, saved: Option<BlockModule>) {
        let Some(BlockModule {
            saved,
            name,
            errors,
            constraints,
        }) = saved
        else {
            return;
        };
        for error in &mut self.errors[errors..] {
            error.reason.wrap_in_module(&name);
        }
        for constraint in &mut self.constraints[constraints..] {
            constraint.reason_mut().wrap_in_module(&name);
        }
        self.current_module = saved;
    }

    pub(super) fn collect_function_signatures(&mut self, stmts: &[Stmt], prefix: &str) {
        self.walk_signatures(stmts, prefix);
    }

    pub(super) fn collect_signatures(&mut self, stmts: &[Stmt], prefix: &str) {
        self.collect_impl_definitions(stmts);
        self.walk_signatures(stmts, prefix);
    }

    fn walk_signatures(&mut self, stmts: &[Stmt], prefix: &str) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Function(func) => {
                    self.collect_function_signature(func, prefix);
                }
                StmtKind::ImplDecl { .. } => {}
                StmtKind::TraitDecl { .. } => {}
                StmtKind::Block(inner_stmts) => {
                    self.walk_signatures(inner_stmts, prefix);
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.walk_signatures_in_stmt(then_branch, prefix);
                    if let Some(else_branch) = else_branch {
                        self.walk_signatures_in_stmt(else_branch, prefix);
                    }
                }
                StmtKind::While { body, .. } => {
                    self.walk_signatures_in_stmt(body, prefix);
                }
                StmtKind::For { body, .. } => {
                    self.walk_signatures_in_stmt(body, prefix);
                }
                StmtKind::ForEach { body, .. } => {
                    self.walk_signatures_in_stmt(body, prefix);
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
        let saved_trait = self.current_trait_name.replace(name.to_string());
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
                let ty = self.type_from_annotation_as(
                    crate::infer::OccurrenceRole::ItemDefinition,
                    &item.type_annotation,
                );
                let names_a_type = match &ty {
                    InferType::Struct(nominal) | InferType::Applied { name: nominal, .. } => {
                        self.type_table.has_nominal(nominal)
                    }
                    _ => true,
                };
                if names_a_type && matches!(ty, InferType::Projection { .. }) {
                    self.projected_associated_const_types.push(
                        crate::infer::ProjectedAssociatedConstType {
                            trait_name: name.to_string(),
                            item: item.name.clone(),
                            declared: ty.clone(),
                            span: item.span,
                        },
                    );
                } else if names_a_type
                    && let Some(settled) = self.settled_associated_const_type(&ty)
                    && !settled.folds_an_associated_constant()
                {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::UnfoldableAssociatedConstType {
                            trait_name: name.to_string(),
                            item: item.name.clone(),
                            declared: settled.source_spelling(),
                        },
                        span: item.span,
                        reason: crate::constraint::ConstraintReason::Other(format!(
                            "declared type in trait '{name}'"
                        )),
                    });
                }
                (item.name.clone(), ty)
            })
            .collect();
        self.type_params_in_scope = saved_type_params;
        self.current_trait_name = saved_trait;
        self.current_trait_associated_items = saved_trait_items;
        self.type_table.register_trait(TraitDef {
            owner: self.current_module.clone(),
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
        // bound argument reads its sibling bounds.
        let saved_bounds = std::mem::replace(
            &mut self.current_function_bounds,
            bound_subjects(&method.where_clauses),
        );
        let bounds = self.bounds_from_where_clauses(&method.where_clauses);
        self.current_function_bounds = bounds;
        let bindings = self.bindings_from_where_clauses(&method.where_clauses);
        let saved_bindings = std::mem::replace(&mut self.current_function_bindings, bindings);
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
            .map(|annotation| {
                self.type_from_annotation_as(crate::infer::OccurrenceRole::ReturnType, annotation)
            })
            .unwrap_or(InferType::Unit);
        self.current_function_bounds = saved_bounds;
        self.current_function_bindings = saved_bindings;
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

    fn walk_signatures_in_stmt(&mut self, stmt: &Stmt, prefix: &str) {
        match &stmt.kind {
            StmtKind::Function(func) => {
                self.collect_function_signature(func, prefix);
            }
            StmtKind::Block(stmts) => {
                self.walk_signatures(stmts, prefix);
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

        // a where clause reads the type parameters and the bounds declared
        let saved_type_params =
            std::mem::replace(&mut self.type_params_in_scope, func.type_params.clone());
        let saved_bounds = std::mem::replace(
            &mut self.current_function_bounds,
            bound_subjects(&func.where_clauses),
        );
        let bounds = self.bounds_from_where_clauses(&func.where_clauses);
        self.current_function_bounds = bounds.clone();
        let bindings = self.bindings_from_where_clauses(&func.where_clauses);
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
            Some(ann) => {
                self.type_from_annotation_as(crate::infer::OccurrenceRole::ReturnType, ann)
            }
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
                    .map(|argument| {
                        self.type_from_annotation_as(crate::infer::OccurrenceRole::Bound, argument)
                    })
                    .collect();
                let trait_name = bound.path.join("::");
                if let Some(module) = self
                    .withholding_module(&trait_name)
                    .or_else(|| self.refused_private_nominal(&trait_name))
                    .map(str::to_string)
                {
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
    ) -> AssociatedBindings {
        let mut bindings = Vec::new();
        for clause in clauses {
            for bound in &clause.bounds {
                if bound.associated_bindings.is_empty() {
                    continue;
                }
                let trait_name = bound.path.join("::");
                let mut items = Vec::with_capacity(bound.associated_bindings.len());
                for (name, value, binding_span) in &bound.associated_bindings {
                    if self.reject_undeclared_binding(
                        &trait_name,
                        name,
                        &clause.type_annotation.name,
                        *binding_span,
                    ) {
                        continue;
                    }
                    let item = match value {
                        aelys_syntax::AssociatedBinding::Const(value) => {
                            self.reject_binding_in_the_wrong_namespace(
                                &trait_name,
                                name,
                                &value.to_string(),
                                crate::constraint::ItemNamespace::Type,
                                *binding_span,
                            );
                            crate::infer::BoundItem::Const(*value)
                        }
                        aelys_syntax::AssociatedBinding::Type(annotation) => {
                            if is_a_bare_type_name(annotation)
                                && self.reject_binding_in_the_wrong_namespace(
                                    &trait_name,
                                    name,
                                    &annotation_spelling(annotation),
                                    crate::constraint::ItemNamespace::Const,
                                    *binding_span,
                                )
                            {
                                continue;
                            }
                            let namespace = if self.binding_reads_the_constant(&trait_name, name) {
                                crate::constraint::ItemNamespace::Const
                            } else {
                                crate::constraint::ItemNamespace::Type
                            };
                            let saved =
                                std::mem::replace(&mut self.annotation_namespace, namespace);
                            let ty = self.type_from_annotation_as(
                                crate::infer::OccurrenceRole::Bound,
                                annotation,
                            );
                            self.annotation_namespace = saved;
                            crate::infer::BoundItem::Type(ty)
                        }
                    };
                    items.push((name.clone(), item));
                }
                bindings.push((clause.type_annotation.name.clone(), trait_name, items));
            }
        }
        bindings
    }

    // the binding decides which is meant, so only a const-only name reads as one
    fn binding_reads_the_constant(&self, trait_name: &str, item: &str) -> bool {
        self.type_table
            .trait_declaring_item_in(
                trait_name,
                item,
                Some(crate::constraint::ItemNamespace::Type),
            )
            .is_none()
            && self
                .type_table
                .trait_declaring_item_in(
                    trait_name,
                    item,
                    Some(crate::constraint::ItemNamespace::Const),
                )
                .is_some()
    }

    // trait never declared is refused, whether or not anything instantiates the function.
    fn reject_undeclared_binding(
        &mut self,
        trait_name: &str,
        item: &str,
        subject: &str,
        span: aelys_syntax::Span,
    ) -> bool {
        if self.type_table.get_trait(trait_name).is_none() {
            return false;
        }
        if self
            .type_table
            .trait_declaring_item(trait_name, item)
            .is_some()
        {
            return false;
        }
        self.errors.push(crate::constraint::TypeError {
            kind: crate::constraint::TypeErrorKind::UndeclaredAssociatedBinding {
                trait_name: trait_name.to_string(),
                item: item.to_string(),
            },
            span,
            reason: crate::constraint::ConstraintReason::Other(format!("a bound on '{subject}'")),
        });
        true
    }

    // e0424. returns whether the binding was rejected.
    fn reject_binding_in_the_wrong_namespace(
        &mut self,
        trait_name: &str,
        item: &str,
        value: &str,
        declared: crate::constraint::ItemNamespace,
        span: aelys_syntax::Span,
    ) -> bool {
        if self
            .type_table
            .trait_declaring_item_in(trait_name, item, Some(declared))
            .is_none()
        {
            return false;
        }
        if self
            .type_table
            .trait_declaring_item_in(trait_name, item, Some(declared.other()))
            .is_some()
        {
            return false;
        }
        self.errors.push(crate::constraint::TypeError {
            kind: crate::constraint::TypeErrorKind::AssociatedBindingNamespaceMismatch {
                trait_name: trait_name.to_string(),
                item: item.to_string(),
                value: value.to_string(),
                declared,
            },
            span,
            reason: crate::constraint::ConstraintReason::Other("a bound".to_string()),
        });
        true
    }

    fn trait_owns_item(
        &self,
        trait_name: &str,
        item: &str,
        namespace: crate::constraint::ItemNamespace,
    ) -> bool {
        let Some(definition) = self.type_table.get_trait(trait_name) else {
            return true;
        };
        match namespace {
            crate::constraint::ItemNamespace::Type => {
                definition.associated_types.iter().any(|name| name == item)
            }
            crate::constraint::ItemNamespace::Const => definition
                .associated_consts
                .iter()
                .any(|(name, _)| name == item),
        }
    }

    fn declare_impl_names(&mut self, block: &ImplBlock<'_>) -> ImplName {
        let trait_name = block.trait_path.map(|path| path.path.join("::"));
        let target = block
            .self_type
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| block.self_type.name.clone());
        let saved_type_params = std::mem::replace(
            &mut self.type_params_in_scope,
            block.impl_type_params.to_vec(),
        );
        let saved_defer = std::mem::replace(&mut self.defer_projection_resolution, true);
        let mark = self.errors.len();
        let self_ty =
            self.type_from_annotation_as(crate::infer::OccurrenceRole::ImplHeader, block.self_type);
        self.errors.truncate(mark);
        self.defer_projection_resolution = saved_defer;
        self.type_params_in_scope = saved_type_params;
        let slot = trait_name.as_deref().map(|trait_name| {
            let associated_types = block
                .associated_types
                .iter()
                .filter(|item| {
                    self.trait_owns_item(
                        trait_name,
                        &item.name,
                        crate::constraint::ItemNamespace::Type,
                    )
                })
                .map(|item| {
                    (
                        item.name.clone(),
                        unresolved_associated_item(trait_name, &item.name, &self_ty),
                    )
                })
                .collect();
            let associated_consts = block
                .associated_consts
                .iter()
                .filter(|item| {
                    self.trait_owns_item(
                        trait_name,
                        &item.name,
                        crate::constraint::ItemNamespace::Const,
                    )
                })
                .map(|item| {
                    let placeholder = unresolved_associated_item(trait_name, &item.name, &self_ty);
                    (item.name.clone(), placeholder.clone(), placeholder, None)
                })
                .collect();
            self.type_table.push_trait_impl_def(TraitImplDef {
                trait_name: trait_name.to_string(),
                trait_args: Vec::new(),
                self_type: self_ty.clone(),
                methods: Vec::new(),
                associated_types,
                associated_consts,
            })
        });
        ImplName {
            target,
            self_ty,
            trait_name,
            slot,
            definitions: (0, 0),
        }
    }

    fn resolve_impl_item_definitions(&mut self, block: &ImplBlock<'_>, name: &mut ImplName) {
        if name.trait_name.is_none() {
            return;
        }
        let associated_types = block.associated_types;
        let associated_consts = block.associated_consts;
        let owner = name.trait_name.clone().unwrap_or_default();
        let owner = owner.as_str();
        let target_ty = name.self_ty.clone();
        let saved_impl_self = self.current_impl_self.replace(target_ty.clone());
        let self_binding = HashMap::from([("Self".to_string(), target_ty.clone())]);
        let saved_defer = std::mem::replace(&mut self.defer_projection_resolution, true);
        let saved_type_params = std::mem::replace(
            &mut self.type_params_in_scope,
            block.impl_type_params.to_vec(),
        );
        let owned: Vec<bool> = associated_types
            .iter()
            .map(|item| {
                self.trait_owns_item(owner, &item.name, crate::constraint::ItemNamespace::Type)
            })
            .collect();
        let typed_associated_types: Vec<(String, InferType)> = associated_types
            .iter()
            .zip(&owned)
            .filter_map(|(item, owned)| {
                let ty = self.type_from_annotation_as(
                    crate::infer::OccurrenceRole::ItemDefinition,
                    &item.value,
                );
                (*owned).then(|| (item.name.clone(), ty.substitute_params(&self_binding)))
            })
            .collect();
        self.type_params_in_scope = saved_type_params;
        self.defer_projection_resolution = saved_defer;
        let receiver = target_ty.to_string();
        let first_definition = self.associated_type_definitions.len();
        for definition in associated_types {
            let mut edges = Vec::new();
            collect_annotation_edges(&definition.value, &receiver, &mut edges);
            self.associated_type_definitions
                .push(crate::infer::AssociatedTypeDefinition {
                    receiver: receiver.clone(),
                    item: definition.name.clone(),
                    namespace: crate::constraint::ItemNamespace::Type,
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
                    namespace: crate::constraint::ItemNamespace::Const,
                    edges,
                    span: definition.span,
                });
        }
        name.definitions = (first_definition, self.associated_type_definitions.len());
        if let Some(slot) = name.slot
            && let Some(definition) = self.type_table.trait_impl_def_at_mut(slot)
        {
            definition.associated_types = typed_associated_types;
        }
        self.current_impl_self = saved_impl_self;
    }

    fn resolve_impl_header(&mut self, block: &ImplBlock<'_>, name: &ImplName) -> ImplHeader {
        let saved_type_params = std::mem::replace(
            &mut self.type_params_in_scope,
            block.impl_type_params.to_vec(),
        );
        let target_ty =
            self.type_from_annotation_as(crate::infer::OccurrenceRole::ImplHeader, block.self_type);
        let trait_args = block
            .trait_path
            .map(|path| {
                path.type_params
                    .iter()
                    .map(|argument| {
                        self.type_from_annotation_as(
                            crate::infer::OccurrenceRole::ImplHeader,
                            argument,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let bounds = self.bounds_from_where_clauses(block.where_clauses);
        let bindings = self.bindings_from_where_clauses(block.where_clauses);
        self.type_params_in_scope = saved_type_params;
        let target = match block.self_type.path.len() >= 2 {
            true => crate::types::nominal_name(&target_ty).unwrap_or_else(|| name.target.clone()),
            false => name.target.clone(),
        };
        ImplHeader {
            target,
            target_ty,
            trait_name: name.trait_name.clone(),
            trait_args,
            bounds,
            bindings,
        }
    }

    fn check_impl_header(
        &mut self,
        block: &ImplBlock<'_>,
        header: &ImplHeader,
        name: &ImplName,
        live: &mut Vec<usize>,
    ) -> bool {
        let self_type = block.self_type;
        let header_span = self_type.span;
        let trait_path_span = block.trait_path.map_or(header_span, |path| path.span);
        let associated_types = block.associated_types;
        let associated_consts = block.associated_consts;
        let trait_name = header.trait_name.clone();
        let target = header.target.clone();
        let target_ty = header.target_ty.clone();
        let trait_args = header.trait_args.clone();
        let effective_methods = self.effective_impl_methods(block.methods, trait_name.as_deref());
        let methods = effective_methods.as_slice();
        if trait_name.is_some() && is_foreign_impl_target(&target_ty, &self.type_table) {
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::OrphanTraitImpl {
                    trait_name: trait_name.clone().unwrap_or_default(),
                    target: target_ty.clone(),
                },
                span: methods.first().map_or(header_span, |m| m.span),
                reason: crate::constraint::ConstraintReason::Other("trait coherence".to_string()),
            });
            return false;
        }
        if let Some(error) =
            self.private_nominal_error(&target, methods.first().map_or(header_span, |m| m.span))
        {
            self.errors.push(error);
            return false;
        }
        if !self.type_table.has_nominal(&target) {
            self.errors.push(crate::constraint::TypeError {
                kind: self.nominal_error_kind(
                    &target,
                    crate::constraint::TypeErrorKind::UnknownStruct {
                        name: target.clone(),
                    },
                ),
                span: methods.first().map_or(header_span, |m| m.span),
                reason: crate::constraint::ConstraintReason::UnknownType {
                    name: target.clone(),
                },
            });
            return false;
        }

        if let Some(unconstrained) = block
            .impl_type_params
            .iter()
            .find(|param| !target_ty.mentions_param(param))
        {
            let call_site_binds = call_site_binds_param(methods, &trait_args, unconstrained);
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::UnconstrainedImplTypeParam {
                    param: unconstrained.clone(),
                    target: target_ty.source_spelling(),
                    call_site_binds,
                },
                span: self_type.span,
                reason: impl_site_reason(trait_name.as_deref(), &target_ty),
            });
            return false;
        }

        if let Some(trait_name) = trait_name.as_deref() {
            if let Some(error) = self.private_nominal_error(
                trait_name,
                methods
                    .first()
                    .map_or(trait_path_span, |method| method.span),
            ) {
                self.errors.push(error);
                return false;
            }
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
                        .map_or(trait_path_span, |method| method.span),
                    reason: crate::constraint::ConstraintReason::UnknownType {
                        name: trait_name.to_string(),
                    },
                });
                return false;
            };
            if crate::prelude::reserves_header(trait_name, &target_ty, &trait_args) {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::ReservedIdentityConversion {
                        ty: target_ty.clone(),
                    },
                    span: methods
                        .first()
                        .map_or(trait_path_span, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return false;
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
                    span: methods.first().map_or(header_span, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return false;
            }
            if self
                .type_table
                .trait_impl_overlaps_among(live, trait_name, &trait_args, &target_ty)
            {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::OverlappingTraitImpl {
                        trait_name: trait_name.to_string(),
                        target: target_ty.clone(),
                    },
                    span: methods.first().map_or(header_span, |method| method.span),
                    reason: crate::constraint::ConstraintReason::Other(
                        "trait coherence".to_string(),
                    ),
                });
                return false;
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
                        span: methods.first().map_or(header_span, |method| method.span),
                        reason: crate::constraint::ConstraintReason::Other(
                            "trait method set".to_string(),
                        ),
                    });
                }
            }
            for item in associated_types {
                if !trait_def.associated_types.contains(&item.name) {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                            target: target.clone(),
                            item: item.name.clone(),
                            keyword: "type",
                            trait_name: Some(trait_name.to_string()),
                        },
                        span: item.span,
                        reason: impl_site_reason(Some(trait_name), &target_ty),
                    });
                }
            }
            for item in associated_consts {
                if !trait_def
                    .associated_consts
                    .iter()
                    .any(|(name, _)| *name == item.name)
                {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                            target: target.clone(),
                            item: item.name.clone(),
                            keyword: "const",
                            trait_name: Some(trait_name.to_string()),
                        },
                        span: item.span,
                        reason: impl_site_reason(Some(trait_name), &target_ty),
                    });
                }
            }
            for namespace in [
                crate::constraint::ItemNamespace::Type,
                crate::constraint::ItemNamespace::Const,
            ] {
                let written: Vec<(&str, aelys_syntax::Span)> = match namespace {
                    crate::constraint::ItemNamespace::Type => associated_types
                        .iter()
                        .map(|item| (item.name.as_str(), item.span))
                        .collect(),
                    crate::constraint::ItemNamespace::Const => associated_consts
                        .iter()
                        .map(|item| (item.name.as_str(), item.span))
                        .collect(),
                };
                for required in self
                    .type_table
                    .declared_associated_items(trait_name, namespace)
                {
                    let mut declarations = written.iter().filter(|(name, _)| *name == required);
                    let first = declarations.next();
                    let second = declarations.next();
                    if first.is_none() {
                        self.errors.push(crate::constraint::TypeError {
                            kind: crate::constraint::TypeErrorKind::MissingAssociatedItem {
                                trait_name: trait_name.to_string(),
                                item: required.clone(),
                                namespace,
                                declared_type: self
                                    .type_table
                                    .declared_associated_consts(trait_name)
                                    .into_iter()
                                    .find(|(name, _)| *name == *required)
                                    .map(|(_, ty)| ty.source_spelling()),
                            },
                            span: self_type.span,
                            reason: impl_site_reason(Some(trait_name), &target_ty),
                        });
                    } else if let Some(duplicate) = second {
                        self.errors.push(crate::constraint::TypeError {
                            kind: crate::constraint::TypeErrorKind::DuplicateAssociatedItem {
                                trait_name: trait_name.to_string(),
                                item: required.clone(),
                            },
                            span: duplicate.1,
                            reason: impl_site_reason(Some(trait_name), &target_ty),
                        });
                    }
                }
            }
            let Some(slot) = name.slot else {
                return true;
            };
            if let Some(definition) = self.type_table.trait_impl_def_at_mut(slot) {
                definition.self_type = target_ty.clone();
                definition.trait_args = trait_args.clone();
            }
            live.push(slot);
        } else {
            // rejecting here keeps them from being silently dropped and later
            for item in associated_types {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                        target: target.clone(),
                        item: item.name.clone(),
                        keyword: "type",
                        trait_name: None,
                    },
                    span: item.span,
                    reason: impl_site_reason(None, &target_ty),
                });
            }
            for item in associated_consts {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::AssociatedItemOutsideTraitImpl {
                        target: target.clone(),
                        item: item.name.clone(),
                        keyword: "const",
                        trait_name: None,
                    },
                    span: item.span,
                    reason: impl_site_reason(None, &target_ty),
                });
            }
        }
        true
    }

    fn check_supertrait_obligations(
        &mut self,
        blocks: &[ImplBlock<'_>],
        headers: &[ImplHeader],
        accepted: &[bool],
    ) {
        for ((block, header), kept) in blocks.iter().zip(headers).zip(accepted) {
            if !*kept {
                continue;
            }
            let Some(trait_name) = header.trait_name.as_deref() else {
                continue;
            };
            let target_ty = header.target_ty.clone();
            let mut missing: Vec<String> = self
                .type_table
                .supertrait_closure(trait_name)
                .into_iter()
                .filter(|name| name != trait_name)
                .filter(|name| !self.type_table.satisfies_bound(name, &target_ty, &[]))
                .collect();
            missing.sort_unstable();
            if !missing.is_empty() {
                self.impls_missing_supertraits.insert(
                    (trait_name.to_string(), header.target.clone()),
                    (target_ty.clone(), missing.clone()),
                );
            }
            for name in missing {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::MissingSupertraitImpl {
                        trait_name: trait_name.to_string(),
                        supertrait: name,
                        ty: target_ty.clone(),
                    },
                    span: block.self_type.span,
                    reason: impl_site_reason(Some(trait_name), &target_ty),
                });
            }
        }
    }

    // a rejected impl contributes nothing, so its registration and the cycle
    fn retract_rejected_impls(
        &mut self,
        blocks: &[ImplBlock<'_>],
        names: &mut [ImplName],
        accepted: &[bool],
    ) {
        if accepted.iter().all(|kept| *kept) {
            return;
        }
        let mut dead: Vec<usize> = names
            .iter()
            .zip(accepted)
            .filter(|(_, kept)| !**kept)
            .filter_map(|(name, _)| name.slot)
            .collect();
        dead.sort_unstable();
        self.type_table.remove_trait_impl_defs(&dead);
        let mut dropped = vec![false; self.associated_type_definitions.len()];
        for (name, kept) in names.iter().zip(accepted) {
            if *kept {
                continue;
            }
            let (first, last) = name.definitions;
            for slot in dropped.iter_mut().take(last).skip(first) {
                *slot = true;
            }
        }
        let mut index = 0;
        self.associated_type_definitions.retain(|_| {
            let keep = !dropped[index];
            index += 1;
            keep
        });
        for (name, kept) in names.iter_mut().zip(accepted) {
            match kept {
                true => {
                    if let Some(slot) = name.slot {
                        let shift = dead.iter().filter(|index| **index < slot).count();
                        name.slot = Some(slot - shift);
                    }
                }
                false => name.slot = None,
            }
        }
        for (name, kept) in names.iter().zip(accepted) {
            if *kept {
                continue;
            }
            let receiver = name.self_ty.to_string();
            self.associated_const_exprs
                .retain(|(candidate, _), _| candidate != &receiver);
        }
        // an accepted impl can share the receiver of a rejected one
        for ((block, name), kept) in blocks.iter().zip(names.iter()).zip(accepted) {
            if !*kept || name.trait_name.is_none() {
                continue;
            }
            let receiver = name.self_ty.to_string();
            for definition in block.associated_consts {
                self.associated_const_exprs.insert(
                    (receiver.clone(), definition.name.clone()),
                    (receiver.clone(), definition.value.clone()),
                );
            }
        }
    }

    fn collect_impl_associated_consts(
        &mut self,
        block: &ImplBlock<'_>,
        header: &ImplHeader,
        name: &ImplName,
    ) {
        let Some(trait_name) = header.trait_name.as_deref() else {
            return;
        };
        let associated_consts = block.associated_consts;
        if associated_consts.is_empty() {
            return;
        }
        let target_ty = header.target_ty.clone();
        let saved_impl_self = self.current_impl_self.replace(target_ty.clone());
        let required = self.type_table.declared_associated_consts(trait_name);
        let instantiation = self.trait_instantiation(trait_name, &header.trait_args, &target_ty);
        let saved_type_params = std::mem::replace(
            &mut self.type_params_in_scope,
            block.impl_type_params.to_vec(),
        );
        let mut typed_associated_consts = Vec::with_capacity(associated_consts.len());
        for item in associated_consts {
            let declared = self.type_from_annotation_as(
                crate::infer::OccurrenceRole::ItemDefinition,
                &item.type_annotation,
            );
            let value_ty = self.infer_const_expr_type(&item.value);
            let value = constant_int_literal(&item.value);
            // the trait declaration cannot name an impl's parameter, so a declared
            let declares_impl_param = block
                .impl_type_params
                .iter()
                .any(|param| declared.mentions_param(param));
            // a name defined twice is refused as a duplicate, and neither of the two
            let defined_once = associated_consts
                .iter()
                .filter(|other| other.name == item.name)
                .count()
                == 1;
            if let Some((_, required_ty)) = required.iter().find(|(name, _)| name == &item.name)
                && defined_once
                && !declared.contains_poison()
            {
                let instantiated = required_ty.substitute_params(&instantiation);
                if declares_impl_param || !self.type_table.types_match(&instantiated, &declared) {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::AssociatedItemTypeMismatch {
                            trait_name: trait_name.to_string(),
                            item: item.name.clone(),
                            disagreement:
                                crate::constraint::AssociatedItemDisagreement::DeclaredType {
                                    declared: self.settled_spelling(&instantiated),
                                    found: self.settled_spelling(&declared),
                                },
                        },
                        span: item.span,
                        reason: crate::constraint::ConstraintReason::Other(format!(
                            "declared type in the impl for '{}'",
                            target_ty.source_spelling()
                        )),
                    });
                } else if self.settled_associated_const_type(required_ty).is_none()
                    && let Some(settled) = self.settled_associated_const_type(&instantiated)
                    && !settled.folds_an_associated_constant()
                {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::UnfoldableAssociatedConstType {
                            trait_name: trait_name.to_string(),
                            item: item.name.clone(),
                            declared: settled.source_spelling(),
                        },
                        span: item.span,
                        reason: crate::constraint::ConstraintReason::Other(format!(
                            "declared type in the impl for '{}'",
                            target_ty.source_spelling()
                        )),
                    });
                } else if !matches!(value_ty, Some(InferType::Poison))
                    && !value_ty
                        .as_ref()
                        .is_some_and(|found| self.type_table.types_match(&declared, found))
                {
                    self.errors.push(crate::constraint::TypeError {
                        kind: crate::constraint::TypeErrorKind::AssociatedItemTypeMismatch {
                            trait_name: trait_name.to_string(),
                            item: item.name.clone(),
                            disagreement:
                                crate::constraint::AssociatedItemDisagreement::ConstantValue {
                                    declared: self.settled_spelling(&declared),
                                    found: value_ty
                                        .as_ref()
                                        .map(|found| self.settled_spelling(found)),
                                },
                        },
                        span: item.span,
                        reason: crate::constraint::ConstraintReason::Other(format!(
                            "constant value in the impl for '{}'",
                            target_ty.source_spelling()
                        )),
                    });
                }
            }
            if self.trait_owns_item(
                trait_name,
                &item.name,
                crate::constraint::ItemNamespace::Const,
            ) {
                let recorded = value_ty.unwrap_or_else(|| declared.clone());
                typed_associated_consts.push((item.name.clone(), declared, recorded, value));
            }
        }
        self.type_params_in_scope = saved_type_params;
        if let Some(slot) = name.slot
            && let Some(definition) = self.type_table.trait_impl_def_at_mut(slot)
        {
            definition.associated_consts = typed_associated_consts;
        }
        self.current_impl_self = saved_impl_self;
    }

    fn collect_impl_method_signatures(
        &mut self,
        block: &ImplBlock<'_>,
        header: &ImplHeader,
        name: &ImplName,
    ) {
        let impl_type_params = block.impl_type_params;
        let associated_types = block.associated_types;
        let trait_name = header.trait_name.clone();
        let target = header.target.clone();
        let target_ty = header.target_ty.clone();
        let trait_args = header.trait_args.clone();
        let impl_bounds = header.bounds.clone();
        let impl_bindings = header.bindings.clone();
        let saved_impl_self = self.current_impl_self.replace(target_ty.clone());
        let effective_methods =
            self.adopted_impl_methods(block.methods, trait_name.as_deref(), &target);
        let methods = effective_methods.as_slice();
        let mut registered_trait_methods = Vec::new();
        let saved_default_body = self.in_trait_default_body;
        for (index, method) in methods.iter().enumerate() {
            self.in_trait_default_body = index >= block.methods.len();
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
            let self_substitution = HashMap::from([("Self".to_string(), target_ty.clone())]);
            let visible_impl_bounds: Vec<_> = impl_bounds
                .iter()
                .filter(|(subject, _, _)| !method.type_params.contains(subject))
                .cloned()
                .collect();
            let mut subjects = visible_impl_bounds.clone();
            subjects.extend(bound_subjects(&method.where_clauses));
            let saved_bounds = std::mem::replace(&mut self.current_function_bounds, subjects);
            let mut method_bounds = visible_impl_bounds;
            method_bounds.extend(self.bounds_from_where_clauses(&method.where_clauses));
            self.current_function_bounds = method_bounds.clone();
            let mut method_bindings: AssociatedBindings = impl_bindings
                .iter()
                .filter(|(subject, _, _)| !method.type_params.contains(subject))
                .cloned()
                .collect();
            method_bindings.extend(self.bindings_from_where_clauses(&method.where_clauses));
            for (_, _, items) in &mut method_bindings {
                for (_, item) in items.iter_mut() {
                    if let crate::infer::BoundItem::Type(ty) = item {
                        *ty = ty.substitute_params(&self_substitution);
                    }
                }
            }
            let saved_bindings =
                std::mem::replace(&mut self.current_function_bindings, method_bindings.clone());
            let mut params = Vec::with_capacity(method.params.len());
            for (index, param) in method.params.iter().enumerate() {
                let ty = if has_self && index == 0 {
                    target_ty.clone()
                } else {
                    param
                        .type_annotation
                        .as_ref()
                        .map(|ann| {
                            self.type_from_annotation_as(
                                crate::infer::OccurrenceRole::Parameter,
                                ann,
                            )
                        })
                        .unwrap_or_else(|| self.type_gen.fresh())
                };
                params.push(ty);
            }
            let mut ret = method
                .return_type
                .as_ref()
                .map(|ann| {
                    self.type_from_annotation_as(crate::infer::OccurrenceRole::ReturnType, ann)
                })
                .unwrap_or_else(|| self.type_gen.fresh());
            params = params
                .into_iter()
                .map(|param| param.substitute_params(&self_substitution))
                .collect();
            ret = ret.substitute_params(&self_substitution);
            params = params
                .iter()
                .map(|param| {
                    self.normalize_projection_in_signature(param, associated_types, &target_ty)
                })
                .collect();
            ret = self.normalize_projection_in_signature(&ret, associated_types, &target_ty);
            self.current_function_bounds = saved_bounds;
            self.current_function_bindings = saved_bindings;
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
                let saved_type_params =
                    std::mem::replace(&mut self.type_params_in_scope, impl_type_params.to_vec());
                let expected_params: Vec<_> = expected_params
                    .iter()
                    .map(|param| {
                        self.normalize_projection_in_signature(param, associated_types, &target_ty)
                    })
                    .collect();
                let expected_return = self.normalize_projection_in_signature(
                    &expected_return,
                    associated_types,
                    &target_ty,
                );
                self.type_params_in_scope = saved_type_params;
                let signature_matches = params.len() == expected_params.len()
                    && params
                        .iter()
                        .zip(expected_params.iter())
                        .all(|(actual, expected)| self.type_table.types_match(expected, actual))
                    && self.type_table.types_match(&expected_return, &ret);
                // a poisoned annotation already reported why it could not resolve
                let annotation_reported =
                    params.iter().chain([&ret]).any(InferType::contains_poison);
                if !signature_matches
                    && !annotation_reported
                    && !self.report_ambiguous_expected_projection(
                        expected_params.iter().chain([&expected_return]),
                        method.span,
                        &target_ty,
                        trait_name,
                    )
                {
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
            if impl_type_params.is_empty() {
                let written_here = index < block.methods.len();
                let span = if written_here {
                    method.span
                } else {
                    block.self_type.span
                };
                self.report_impl_symbol_collision(&symbol, &target, &method.name, span, &target_ty);
            }
            self.function_reference_modes.insert(
                symbol.clone(),
                method.params.iter().map(|param| param.reference).collect(),
            );
            let carries_obligation = !method_bounds.is_empty() || !method_bindings.is_empty();
            if !method_bounds.is_empty() {
                self.generic_function_bounds
                    .insert(symbol.clone(), method_bounds);
            }
            if !method_bindings.is_empty() {
                self.generic_function_bindings
                    .insert(symbol.clone(), method_bindings);
            }
            // the impl block's own bounds reach a method that declares no type
            if carries_obligation {
                self.impl_method_signatures.insert(
                    symbol.clone(),
                    ImplMethodSignature {
                        params: params.clone(),
                        return_type: ret.clone(),
                    },
                );
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
        if let Some(slot) = name.slot
            && let Some(definition) = self.type_table.trait_impl_def_at_mut(slot)
        {
            definition.methods = registered_trait_methods;
        }
        self.in_trait_default_body = saved_default_body;
        self.current_impl_self = saved_impl_self;
    }

    // a generic impl mints one symbol per instance in the monomorphizer, which
    fn report_impl_symbol_collision(
        &mut self,
        symbol: &str,
        target: &str,
        method: &str,
        span: aelys_syntax::Span,
        target_ty: &InferType,
    ) {
        match self.impl_method_symbol_targets.get(symbol) {
            Some(previous) if previous == target_ty => {}
            Some(_) => {
                self.errors.push(crate::constraint::TypeError {
                    kind: crate::constraint::TypeErrorKind::MangledSymbolCollision {
                        name: format!("{target}::{method}"),
                    },
                    span,
                    reason: crate::constraint::ConstraintReason::Other(
                        "impl method symbol".to_string(),
                    ),
                });
            }
            None => {
                self.impl_method_symbol_targets
                    .insert(symbol.to_string(), target_ty.clone());
            }
        }
    }

    pub(super) fn infer_const_expr_type(&mut self, expr: &aelys_syntax::Expr) -> Option<InferType> {
        use aelys_syntax::ExprKind;
        match &expr.kind {
            ExprKind::Int(_) => Some(InferType::I64),
            ExprKind::Float(_) => Some(InferType::F64),
            ExprKind::Bool(_) => Some(InferType::Bool),
            ExprKind::String(_) => Some(InferType::String),
            ExprKind::Unary { op, operand } => {
                use aelys_syntax::UnaryOp;
                match op {
                    UnaryOp::Neg | UnaryOp::Not => self.infer_const_expr_type(operand),
                    _ => None,
                }
            }
            ExprKind::Binary { left, right, .. } => {
                let left_ty = self.infer_const_expr_type(left)?;
                let right_ty = self.infer_const_expr_type(right)?;
                if left_ty.is_integer() && right_ty.is_integer() {
                    Some(InferType::I64)
                } else if left_ty.is_numeric() && right_ty.is_numeric() {
                    Some(InferType::F64)
                } else {
                    None
                }
            }
            ExprKind::Grouping(inner) => self.infer_const_expr_type(inner),
            ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Path,
            } => {
                let ExprKind::Identifier(name) = &object.kind else {
                    return None;
                };
                let receiver = match name.as_str() {
                    "Self" => crate::types::nominal_name(self.current_impl_self.as_ref()?)?,
                    _ => name.clone(),
                };
                self.referenced_const_type(&receiver, member)
            }
            _ => None,
        }
    }

    pub(super) fn referenced_const_type(&self, receiver: &str, item: &str) -> Option<InferType> {
        let declared_by = |trait_name: &str| {
            self.type_table
                .declared_associated_consts(trait_name)
                .into_iter()
                .find(|(name, _)| name == item)
                .map(|(_, ty)| ty)
        };
        if let Some(ty) = declared_by(receiver) {
            return Some(ty);
        }
        let mut traits: Vec<String> = self
            .associated_const_candidates(receiver, item)
            .into_iter()
            .map(|(trait_name, _, _)| trait_name)
            .collect();
        traits.sort();
        traits.dedup();
        let [only] = traits.as_slice() else {
            return None;
        };
        declared_by(only)
    }

    pub(super) fn validate_projected_associated_const_types(&mut self) {
        let pending = std::mem::take(&mut self.projected_associated_const_types);
        for entry in &pending {
            let Some(settled) = self.settled_associated_const_type(&entry.declared) else {
                continue;
            };
            if settled.folds_an_associated_constant() {
                continue;
            }
            let trait_name = &entry.trait_name;
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::UnfoldableAssociatedConstType {
                    trait_name: trait_name.clone(),
                    item: entry.item.clone(),
                    declared: settled.source_spelling(),
                },
                span: entry.span,
                reason: crate::constraint::ConstraintReason::Other(format!(
                    "declared type in trait '{trait_name}'"
                )),
            });
        }
    }

    fn trait_instantiation(
        &self,
        trait_name: &str,
        trait_args: &[InferType],
        target_ty: &InferType,
    ) -> HashMap<String, InferType> {
        let mut instantiation = HashMap::from([("Self".to_string(), target_ty.clone())]);
        let Some(definition) = self.type_table.get_trait(trait_name) else {
            return instantiation;
        };
        for (param, argument) in definition.type_params.iter().zip(trait_args) {
            instantiation.insert(param.clone(), argument.clone());
        }
        instantiation
    }

    /// none while a parameter or an unresolved projection leaves it open
    pub(super) fn settled_associated_const_type(&self, ty: &InferType) -> Option<InferType> {
        let settled = match ty {
            InferType::Projection { .. } => self.resolve_associated_projection(ty)?,
            other => other.clone(),
        };
        settled.is_concrete().then_some(settled)
    }

    pub(super) fn settled_spelling(&self, ty: &InferType) -> String {
        self.settled_associated_const_type(ty)
            .unwrap_or_else(|| ty.clone())
            .source_spelling()
    }

    fn impl_declared_const_type(&self, receiver: &str, item: &str) -> Option<InferType> {
        let self_ty = InferType::Struct(receiver.to_string());
        let mut found = Vec::new();
        for implementation in self.type_table.trait_impl_defs() {
            if !self
                .type_table
                .types_match(&implementation.self_type, &self_ty)
            {
                continue;
            }
            if let Some((_, declared, _, _)) = implementation
                .associated_consts
                .iter()
                .find(|(name, _, _, _)| name == item)
            {
                found.push(declared.clone());
            }
        }
        let [only] = found.as_slice() else {
            return None;
        };
        Some(only.clone())
    }

    pub(super) fn non_integer_const_type(&self, receiver: &str, item: &str) -> Option<String> {
        let declared = self
            .impl_declared_const_type(receiver, item)
            .or_else(|| self.referenced_const_type(receiver, item))?;
        match declared.is_integer() {
            true => None,
            false => Some(self.settled_spelling(&declared)),
        }
    }

    fn report_ambiguous_expected_projection<'e>(
        &mut self,
        expected: impl Iterator<Item = &'e InferType>,
        span: aelys_syntax::Span,
        target_ty: &InferType,
        trait_name: &str,
    ) -> bool {
        let mut projections = Vec::new();
        for ty in expected {
            collect_projections(ty, &mut projections);
        }
        for projection in projections {
            let InferType::Projection {
                trait_name: written,
                item,
                self_ty,
            } = &projection
            else {
                continue;
            };
            if !self_ty.is_concrete() || self.resolve_associated_projection(&projection).is_some() {
                continue;
            }
            let candidates =
                self.associated_projection_candidates(written.as_deref(), item, self_ty);
            if candidates.len() < 2 {
                continue;
            }
            let traits: Vec<String> = candidates.into_iter().map(|(name, _)| name).collect();
            let traits = self.traits_by_nameability(traits);
            self.errors.push(crate::constraint::TypeError {
                kind: crate::constraint::TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver: self_ty.source_spelling(),
                    item: item.clone(),
                    cause: crate::constraint::ProjectionFailure::Ambiguous { traits },
                },
                span,
                reason: impl_site_reason(Some(trait_name), target_ty),
            });
            return true;
        }
        false
    }

    fn normalize_projection_in_signature(
        &mut self,
        ty: &InferType,
        associated_types: &[aelys_syntax::AssociatedTypeDef],
        target_ty: &InferType,
    ) -> InferType {
        match ty {
            InferType::Projection {
                trait_name,
                item,
                self_ty,
            } => {
                if self_ty.as_ref() == target_ty
                    && let Some(definition) = associated_types
                        .iter()
                        .find(|candidate| &candidate.name == item)
                {
                    return self.type_from_annotation(&definition.value);
                }
                InferType::Projection {
                    trait_name: trait_name.clone(),
                    item: item.clone(),
                    self_ty: Box::new(self.normalize_projection_in_signature(
                        self_ty,
                        associated_types,
                        target_ty,
                    )),
                }
            }
            InferType::Function { params, ret } => InferType::Function {
                params: params
                    .iter()
                    .map(|param| {
                        self.normalize_projection_in_signature(param, associated_types, target_ty)
                    })
                    .collect(),
                ret: Box::new(self.normalize_projection_in_signature(
                    ret,
                    associated_types,
                    target_ty,
                )),
            },
            InferType::Array(inner) => InferType::Array(Box::new(
                self.normalize_projection_in_signature(inner, associated_types, target_ty),
            )),
            InferType::FixedArray(inner, length) => InferType::FixedArray(
                Box::new(self.normalize_projection_in_signature(
                    inner,
                    associated_types,
                    target_ty,
                )),
                *length,
            ),
            InferType::Vec(inner) => InferType::Vec(Box::new(
                self.normalize_projection_in_signature(inner, associated_types, target_ty),
            )),
            InferType::Option(inner) => InferType::Option(Box::new(
                self.normalize_projection_in_signature(inner, associated_types, target_ty),
            )),
            InferType::Result(ok, err) => InferType::Result(
                Box::new(self.normalize_projection_in_signature(ok, associated_types, target_ty)),
                Box::new(self.normalize_projection_in_signature(err, associated_types, target_ty)),
            ),
            InferType::Tuple(elements) => InferType::Tuple(
                elements
                    .iter()
                    .map(|element| {
                        self.normalize_projection_in_signature(element, associated_types, target_ty)
                    })
                    .collect(),
            ),
            InferType::Applied { name, args } => InferType::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| {
                        self.normalize_projection_in_signature(arg, associated_types, target_ty)
                    })
                    .collect(),
            },
            _ => ty.clone(),
        }
    }

    pub(super) fn supertrait_gate_error(
        &self,
        target: &str,
        member: &str,
        span: aelys_syntax::Span,
    ) -> Option<crate::constraint::TypeError> {
        supertrait_gate_error_for(
            &self.type_table,
            &self.impls_missing_supertraits,
            target,
            member,
            span,
        )
    }

    pub(super) fn adopted_impl_methods(
        &self,
        methods: &[Function],
        trait_name: Option<&str>,
        target: &str,
    ) -> Vec<Function> {
        if let Some(name) = trait_name
            && self
                .impls_missing_supertraits
                .contains_key(&(name.to_string(), target.to_string()))
        {
            return methods.to_vec();
        }
        self.effective_impl_methods(methods, trait_name)
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

fn annotation_mentions(annotation: &aelys_syntax::TypeAnnotation, param: &str) -> bool {
    annotation.name == param
        || annotation.path.iter().any(|segment| segment == param)
        || annotation
            .type_params
            .iter()
            .any(|inner| annotation_mentions(inner, param))
        || annotation
            .fn_params
            .iter()
            .flatten()
            .any(|inner| annotation_mentions(inner, param))
        || annotation
            .fn_ret
            .as_deref()
            .is_some_and(|inner| annotation_mentions(inner, param))
}

// measured with the refusal disabled: a parameter carried by a trait argument or
fn call_site_binds_param(methods: &[Function], trait_args: &[InferType], param: &str) -> bool {
    trait_args.iter().any(|arg| arg.mentions_param(param))
        || methods.iter().any(|method| {
            !method.type_params.iter().any(|own| own == param)
                && (method
                    .params
                    .iter()
                    .filter_map(|parameter| parameter.type_annotation.as_ref())
                    .any(|annotation| annotation_mentions(annotation, param))
                    || method
                        .return_type
                        .as_ref()
                        .is_some_and(|annotation| annotation_mentions(annotation, param)))
        })
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
        let mut first: HashMap<(&str, &str), usize> = HashMap::new();
        for (position, definition) in definitions.iter().enumerate() {
            first
                .entry((definition.receiver.as_str(), definition.item.as_str()))
                .or_insert(position);
        }
        let mut settled: HashSet<(String, String)> = HashSet::new();
        let mut reported: HashSet<(String, String)> = HashSet::new();
        for definition in &definitions {
            let origin = (definition.receiver.clone(), definition.item.clone());
            if settled.contains(&origin) {
                continue;
            }
            if let Some(cycle) =
                Self::find_projection_cycle(&definitions, &first, &origin, &mut settled)
            {
                let opener = cycle[0].clone();
                if !reported.insert(opener.clone()) {
                    continue;
                }
                let Some(culprit) = first
                    .get(&(opener.0.as_str(), opener.1.as_str()))
                    .map(|&position| &definitions[position])
                else {
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
                        cause: crate::constraint::ProjectionFailure::Cyclic {
                            path,
                            namespace: culprit.namespace,
                        },
                    },
                    span: culprit.span,
                    reason: crate::constraint::ConstraintReason::Other(format!(
                        "an associated item definition in the impl for '{}'",
                        culprit.receiver
                    )),
                });
            }
        }
        self.associated_type_definitions = definitions;
    }

    fn find_projection_cycle(
        definitions: &[AssociatedTypeDefinition],
        first: &HashMap<(&str, &str), usize>,
        origin: &(String, String),
        settled: &mut HashSet<(String, String)>,
    ) -> Option<Vec<(String, String)>> {
        let mut path: Vec<(String, String)> = Vec::new();
        let mut on_path: HashSet<(String, String)> = HashSet::new();
        let mut frames: Vec<(usize, usize)> = Vec::new();
        let mut pending = Some(origin.clone());
        loop {
            if let Some(node) = pending.take() {
                if on_path.contains(&node) {
                    let position = path
                        .iter()
                        .position(|entry| *entry == node)
                        .expect("a node on the path has a position in it");
                    return Some(path[position..].to_vec());
                }
                if !settled.contains(&node) {
                    match first.get(&(node.0.as_str(), node.1.as_str())) {
                        Some(&position) => {
                            on_path.insert(node.clone());
                            path.push(node);
                            frames.push((position, 0));
                            continue;
                        }
                        None => {
                            settled.insert(node);
                        }
                    }
                }
            }
            let (definition, cursor) = frames.last_mut()?;
            let edges = &definitions[*definition].edges;
            if let Some(edge) = edges.get(*cursor) {
                let edge = edge.clone();
                *cursor += 1;
                pending = Some(edge);
                continue;
            }
            frames.pop();
            let node = path.pop().expect("a frame owns one path entry");
            on_path.remove(&node);
            settled.insert(node);
        }
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
    if let Some(segments) = annotation.array_length_path.as_ref()
        && segments.len() >= 2
    {
        let receiver = &segments[0];
        let receiver = if receiver == "Self" { target } else { receiver };
        if let Some(item) = segments.last() {
            out.push((receiver.to_string(), item.clone()));
        }
    }
    for param in &annotation.type_params {
        collect_annotation_edges(param, target, out);
    }
    for (_, bound, _) in &annotation.associated_bindings {
        if let aelys_syntax::AssociatedBinding::Type(bound) = bound {
            collect_annotation_edges(bound, target, out);
        }
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
            if matches!(separator, aelys_syntax::MemberSeparator::Path)
                && let ExprKind::Identifier(name) = &object.kind
            {
                let receiver = if name == "Self" { target } else { name };
                out.push((receiver.to_string(), member.clone()));
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

// rejected there as e0423. a generic argument list, `wrap<counter::item>`,
fn is_a_bare_type_name(annotation: &aelys_syntax::TypeAnnotation) -> bool {
    annotation.path.len() == 1 && annotation.type_params.is_empty()
}

fn annotation_spelling(annotation: &aelys_syntax::TypeAnnotation) -> String {
    if annotation.path.is_empty() {
        annotation.name.clone()
    } else {
        annotation.path.join("::")
    }
}

fn collect_projections(ty: &InferType, out: &mut Vec<InferType>) {
    match ty {
        InferType::Projection { self_ty, .. } => {
            out.push(ty.clone());
            collect_projections(self_ty, out);
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_projections(param, out);
            }
            collect_projections(ret, out);
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => collect_projections(inner, out),
        InferType::Result(ok, err) => {
            collect_projections(ok, out);
            collect_projections(err, out);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_projections(element, out);
            }
        }
        InferType::Applied { args, .. } => {
            for arg in args {
                collect_projections(arg, out);
            }
        }
        _ => {}
    }
}

fn impl_site_reason(
    trait_name: Option<&str>,
    target: &InferType,
) -> crate::constraint::ConstraintReason {
    let target = target.source_spelling();
    crate::constraint::ConstraintReason::Other(match trait_name {
        Some(trait_name) => format!("impl of trait '{trait_name}' for '{target}'"),
        None => format!("inherent impl of '{target}'"),
    })
}

// monomorphization meets the same withheld default through a bound on a type
pub(super) fn supertrait_gate_error_for(
    type_table: &crate::types::TypeTable,
    gate: &std::collections::BTreeMap<(String, String), (crate::types::InferType, Vec<String>)>,
    target: &str,
    member: &str,
    span: aelys_syntax::Span,
) -> Option<crate::constraint::TypeError> {
    for ((trait_name, impl_target), (target_ty, missing)) in gate {
        if impl_target != target {
            continue;
        }
        if !type_table.get_trait(trait_name).is_some_and(|definition| {
            definition
                .methods
                .iter()
                .any(|method| method.name == member)
        }) {
            continue;
        }
        let supertrait = missing.first()?.clone();
        return Some(crate::constraint::TypeError {
            kind: crate::constraint::TypeErrorKind::MissingSupertraitImpl {
                trait_name: trait_name.clone(),
                supertrait,
                ty: target_ty.clone(),
            },
            span,
            reason: impl_site_reason(Some(trait_name), target_ty),
        });
    }
    None
}
