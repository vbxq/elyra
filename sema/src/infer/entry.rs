use super::{ConstEvalState, ConstResolution, ProjectionNamespace};
use super::{KNOWN_TYPE_NAMES, TypeInference};
use crate::constraint::{
    ConstraintReason, ItemNamespace, ProjectionFailure, TypeError, TypeErrorKind,
};
use crate::typed_ast::TypedProgram;
use crate::types::{InferType, TypeTable};
use aelys_common::Warning;
use aelys_syntax::{Expr, ExprKind, ModuleId, Source, Stmt, StmtKind, TypeAnnotation};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct InferenceInputs {
    pub stmts: Vec<Stmt>,
    pub source: Arc<Source>,
    pub module_aliases: HashSet<String>,
    pub known_globals: HashSet<String>,
    pub known_native_globals: HashSet<String>,
    pub known_native_signatures: HashMap<String, InferType>,
    pub imported_types: crate::infer::imports::ImportedTypes,
    pub current_module: ModuleId,
    pub scope_own_globals: bool,
}

pub struct InferenceResult {
    pub program: TypedProgram,
    pub warnings: Vec<Warning>,
    pub type_table: TypeTable,
    pub generic_structs: Vec<crate::types::StructDef>,
    pub generic_enums: Vec<crate::types::EnumDef>,
}

impl Default for TypeInference {
    fn default() -> Self {
        Self {
            type_gen: crate::types::TypeVarGen::new(),
            constraints: Vec::new(),
            env: crate::env::TypeEnv::new(),
            errors: Vec::new(),
            return_type_stack: Vec::new(),
            depth: 0,
            warnings: Vec::new(),
            type_table: TypeTable::new(),
            type_params_in_scope: Vec::new(),
            method_param_renames: std::collections::HashMap::new(),
            trait_defaults: HashMap::new(),
            generic_function_bounds: HashMap::new(),
            function_type_params: HashMap::new(),
            function_reference_modes: HashMap::new(),
            allow_reference_annotation: false,
            allow_direct_borrow: false,
            callee_position: false,
            borrow_call_scopes: Vec::new(),
            forwarded_mutable_borrows: HashSet::new(),
            try_residuals: Vec::new(),
            try_conversions: HashMap::new(),
            must_use_values: Vec::new(),
            sum_method_residuals: Vec::new(),
            match_exhaustivity_residuals: Vec::new(),
            dynamic_residuals: Vec::new(),
            bound_residuals: Vec::new(),
            surface_dynamic_spans: HashSet::new(),
            sum_type_residuals: Vec::new(),
            explicit_dynamic_functions: HashSet::new(),
            module_aliases: HashSet::new(),
            known_globals: HashSet::new(),
            globals_without_signature: HashSet::new(),
            module_globals: std::collections::BTreeMap::new(),
            module_scoped_globals: std::collections::BTreeMap::new(),
            scope_own_globals: false,
            known_native_globals: HashSet::new(),
            known_native_signatures: HashMap::new(),
            current_module: ModuleId::new("<unknown>"),
            collection_iter_allowed: false,
            withheld_nominals: std::collections::BTreeMap::new(),
            private_nominals: std::collections::BTreeMap::new(),
            unexported_nominals: std::collections::BTreeSet::new(),
            root_module: ModuleId::new("<unknown>"),
            monomorphization_active: Vec::new(),
            current_trait_name: None,
            current_trait_associated_items: Vec::new(),
            projected_associated_const_types: Vec::new(),
            nominal_parameter_scope: false,
            current_function_bounds: Vec::new(),
            current_function_bindings: Vec::new(),
            generic_function_bindings: HashMap::new(),
            impl_method_signatures: HashMap::new(),
            impl_method_symbol_targets: HashMap::new(),
            associated_type_definitions: Vec::new(),
            associated_const_resolution_cache: std::cell::RefCell::new(None),
            trait_qualified_items: HashMap::new(),
            associated_const_exprs: HashMap::new(),
            defer_projection_resolution: false,
            annotation_namespace: ItemNamespace::Type,
            occurrence_role: None,
            current_impl_self: None,
            in_trait_default_body: false,
            impls_missing_supertraits: std::collections::BTreeMap::new(),
            projection_cycle_escaped: std::cell::Cell::new(false),
            substitution_depth: std::cell::Cell::new(0),
            substitution_overflowed: std::cell::Cell::new(None),
            specialization_verdicts: std::cell::RefCell::new(Vec::new()),
            conversion_verdicts: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl TypeInference {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn type_from_annotation(&mut self, ann: &TypeAnnotation) -> InferType {
        self.validate_reference_annotation(ann, self.allow_reference_annotation);
        self.check_type_annotation(ann);
        self.type_from_annotation_inner(ann)
    }

    pub(super) fn type_from_annotation_as(
        &mut self,
        role: crate::infer::OccurrenceRole,
        ann: &TypeAnnotation,
    ) -> InferType {
        let saved = self.occurrence_role.replace(role);
        let ty = self.type_from_annotation(ann);
        self.occurrence_role = saved;
        ty
    }

    pub(super) fn occurrence_reason(&self, fallback: &'static str) -> ConstraintReason {
        ConstraintReason::Other(
            self.occurrence_role
                .map_or(fallback, crate::infer::OccurrenceRole::describe)
                .to_string(),
        )
    }

    pub(super) fn annotation_has_invalid_generic_arity(&self, ann: &TypeAnnotation) -> bool {
        if ann.is_function_type() {
            return ann.fn_params.as_ref().is_some_and(|params| {
                params
                    .iter()
                    .any(|param| self.annotation_has_invalid_generic_arity(param))
            }) || ann
                .fn_ret
                .as_ref()
                .is_some_and(|ret| self.annotation_has_invalid_generic_arity(ret));
        }

        let expected = match ann.name.to_lowercase().as_str() {
            "array" | "vec" | "option" => Some(1),
            "result" => Some(2),
            name if KNOWN_TYPE_NAMES.contains(&name) => Some(0),
            _ => self
                .type_table
                .get_struct(&ann.name)
                .map(|def| def.type_params.len())
                .or_else(|| {
                    self.type_table
                        .get_enum(&ann.name)
                        .map(|def| def.type_params.len())
                }),
        };

        expected.is_some_and(|expected| {
            ann.type_params.len() != expected
                || ann
                    .type_params
                    .iter()
                    .any(|param| self.annotation_has_invalid_generic_arity(param))
        })
    }

    fn type_from_annotation_argument(&mut self, ann: &TypeAnnotation) -> InferType {
        let saved = std::mem::replace(&mut self.annotation_namespace, ItemNamespace::Type);
        let ty = self.type_from_annotation_inner(ann);
        self.annotation_namespace = saved;
        ty
    }

    fn type_from_annotation_inner(&mut self, ann: &TypeAnnotation) -> InferType {
        if ann.is_function_type() {
            let params = ann
                .fn_params
                .as_ref()
                .map(|params| {
                    params
                        .iter()
                        .map(|param| self.type_from_annotation_argument(param))
                        .collect()
                })
                .unwrap_or_default();
            let ret = ann
                .fn_ret
                .as_ref()
                .map(|ret| self.type_from_annotation_argument(ret))
                .unwrap_or(InferType::Unit);
            return InferType::Function {
                params,
                ret: Box::new(ret),
            };
        }

        if ann.path.len() == 1
            && ann.type_params.is_empty()
            && self
                .type_params_in_scope
                .iter()
                .any(|param| param == &ann.name)
        {
            return InferType::Param(
                self.method_param_renames
                    .get(&ann.name)
                    .cloned()
                    .unwrap_or_else(|| ann.name.clone()),
            );
        }

        if ann.path.len() >= 2 && ann.type_params.is_empty() {
            let self_segment = &ann.path[0];
            let item = ann.path.last().cloned().unwrap_or_default();
            if let Some(ProjectionNamespace::WrongNamespace { found }) =
                self.associated_item_namespace(self_segment, &item, self.annotation_namespace)
            {
                let reason = self.occurrence_reason("a type annotation");
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver: self.projection_receiver(self_segment),
                        item: item.clone(),
                        cause: ProjectionFailure::WrongNamespace { found },
                    },
                    span: ann.span,
                    reason,
                });
                return InferType::Poison;
            }
            if self_segment == "Self"
                && self.current_impl_self.is_none()
                && self.current_trait_name.is_none()
            {
                let reason = self.occurrence_reason("a type annotation");
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver: self_segment.clone(),
                        item: item.clone(),
                        cause: ProjectionFailure::SelfOutsideImpl,
                    },
                    span: ann.span,
                    reason,
                });
                return InferType::Poison;
            }
            let self_ty = if self_segment == "Self" {
                self.current_impl_self
                    .clone()
                    .unwrap_or_else(|| InferType::Param("Self".to_string()))
            } else if self
                .type_params_in_scope
                .iter()
                .any(|param| param == self_segment)
            {
                InferType::Param(self_segment.clone())
            } else if self.type_table.has_nominal(self_segment) {
                InferType::Struct(self_segment.clone())
            } else if self.type_table.get_trait(self_segment).is_some() {
                let item_name = ann.path.last().cloned().unwrap_or_default();
                return self.resolve_trait_qualified_projection(self_segment, &item_name, ann.span);
            } else {
                return InferType::Poison;
            };
            let trait_name = if self_segment == "Self" {
                self.current_trait_name.clone()
            } else {
                None
            };
            let projection = InferType::Projection {
                trait_name: trait_name.clone(),
                item,
                self_ty: Box::new(self_ty),
            };
            if !self.defer_projection_resolution
                && let Some(resolved) = self.resolve_associated_projection(&projection)
            {
                let reason = self.occurrence_reason("a type annotation");
                self.report_oversized_projection(&projection, ann.span, &reason);
                return resolved;
            }
            let item_name = projection.item_name().to_string();
            let failure_reason = self.occurrence_reason("a type annotation");
            if matches!(projection.self_ty(), InferType::Param(_)) {
                if self.projection_declaring_trait(&projection).is_none() {
                    let cause = if self.nominal_parameter_scope {
                        ProjectionFailure::NominalParameter
                    } else {
                        ProjectionFailure::Unbound
                    };
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::AmbiguousAssociatedProjection {
                            receiver: self_segment.clone(),
                            item: item_name,
                            cause,
                        },
                        span: ann.span,
                        reason: failure_reason,
                    });
                    return InferType::Poison;
                }
                return projection;
            }
            // projection left to leak into a later mismatch.
            let candidates = self.associated_projection_candidates(
                trait_name.as_deref(),
                &item_name,
                projection.self_ty(),
            );
            if candidates.len() > 1 {
                let mut traits: Vec<String> =
                    candidates.into_iter().map(|(name, _)| name).collect();
                traits.sort();
                traits.dedup();
                let receiver = self.projection_receiver(self_segment);
                let cause = match traits.as_slice() {
                    [only] => ProjectionFailure::AmbiguousInstantiations {
                        trait_name: only.clone(),
                        constructor: receiver.clone(),
                        instantiations: self.rival_instantiations(
                            &receiver,
                            &item_name,
                            ItemNamespace::Type,
                        ),
                    },
                    _ => ProjectionFailure::Ambiguous {
                        traits: self.traits_by_nameability(traits),
                    },
                };
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver,
                        item: item_name,
                        cause,
                    },
                    span: ann.span,
                    reason: failure_reason,
                });
                return InferType::Poison;
            }
            if matches!(
                self.associated_item_namespace(self_segment, &item_name, self.annotation_namespace),
                Some(ProjectionNamespace::Absent)
            ) {
                let receiver = self.projection_receiver(self_segment);
                let cause = match self.bare_receiver_definition(&receiver, &item_name) {
                    Some(definition) => ProjectionFailure::ReceiverArguments { param: definition },
                    None => ProjectionFailure::NoImpl,
                };
                self.errors.push(TypeError {
                    kind: TypeErrorKind::AmbiguousAssociatedProjection {
                        receiver,
                        item: item_name,
                        cause,
                    },
                    span: ann.span,
                    reason: failure_reason,
                });
                return InferType::Poison;
            }
            return projection;
        }

        let name_lower = ann.name.to_lowercase();
        match name_lower.as_str() {
            "int" | "i64" | "int64" => InferType::I64,
            "i8" | "int8" => InferType::I8,
            "i16" | "int16" => InferType::I16,
            "i32" | "int32" => InferType::I32,
            "u8" | "uint8" => InferType::U8,
            "u16" | "uint16" => InferType::U16,
            "u32" | "uint32" => InferType::U32,
            "u64" | "uint64" => InferType::U64,
            "float" | "f64" | "float64" => InferType::F64,
            "f32" | "float32" => InferType::F32,
            "bool" => InferType::Bool,
            "string" => InferType::String,
            "unit" | "void" => InferType::Unit,
            "dynamic" => InferType::Dynamic,
            "array" => {
                let inner = ann
                    .type_params
                    .first()
                    .map(|param| self.type_from_annotation_argument(param))
                    .unwrap_or(InferType::Poison);
                if let Some(length) = ann.array_length {
                    return InferType::FixedArray(Box::new(inner), length as usize);
                }
                if let Some(path) = &ann.array_length_path {
                    // `[t; bounds::limit]` resolve the associated constant.
                    match self.resolve_array_length_path(path, ann.span) {
                        Some(length) => InferType::FixedArray(Box::new(inner), length),
                        None => {
                            let path = path.clone();
                            self.report_unresolved_array_length(&path, ann.span);
                            InferType::Poison
                        }
                    }
                } else {
                    InferType::Array(Box::new(inner))
                }
            }
            "vec" => InferType::Vec(Box::new(
                ann.type_params
                    .first()
                    .map(|param| self.type_from_annotation_argument(param))
                    .unwrap_or(InferType::Poison),
            )),
            "option" => InferType::Option(Box::new(
                ann.type_params
                    .first()
                    .map(|param| self.type_from_annotation_argument(param))
                    .unwrap_or(InferType::Poison),
            )),
            "result" => InferType::Result(
                Box::new(
                    ann.type_params
                        .first()
                        .map(|param| self.type_from_annotation_argument(param))
                        .unwrap_or(InferType::Poison),
                ),
                Box::new(
                    ann.type_params
                        .get(1)
                        .map(|param| self.type_from_annotation_argument(param))
                        .unwrap_or(InferType::Poison),
                ),
            ),
            "error" => InferType::Error,
            _ if self.type_table.has_nominal(&ann.name) && !ann.type_params.is_empty() => {
                InferType::Applied {
                    name: ann.name.clone(),
                    args: ann
                        .type_params
                        .iter()
                        .map(|param| self.type_from_annotation_argument(param))
                        .collect(),
                }
            }
            _ if self.type_table.has_nominal(&ann.name) => InferType::Struct(ann.name.clone()),
            _ if ann.name.chars().next().is_some_and(|c| c.is_uppercase()) => {
                InferType::Struct(ann.name.clone())
            }
            _ => InferType::Poison,
        }
    }

    fn check_type_annotation(&mut self, ann: &TypeAnnotation) {
        if ann.is_function_type() {
            if let Some(params) = &ann.fn_params {
                for param in params {
                    self.check_type_annotation(param);
                }
            }
            if let Some(ret) = &ann.fn_ret {
                self.check_type_annotation(ret);
            }
            return;
        }
        if self.type_params_in_scope.iter().any(|tp| tp == &ann.name) {
            return;
        }
        let head = ann.path.first().unwrap_or(&ann.name);
        if let Some(error) = self.private_nominal_error(head, ann.span) {
            self.errors.push(error);
            return;
        }

        // standalone type, so the nominal fallback below must not fire.
        if ann.path.len() >= 2 && ann.type_params.is_empty() {
            let self_segment = &ann.path[0];
            if self_segment != "Self"
                && !self
                    .type_params_in_scope
                    .iter()
                    .any(|tp| tp == self_segment)
                && !self.type_table.has_nominal(self_segment)
                && self.type_table.get_trait(self_segment).is_none()
            {
                if KNOWN_TYPE_NAMES.contains(&self_segment.to_lowercase().as_str()) {
                    let item = ann.path.last().cloned().unwrap_or_default();
                    let reason = self.occurrence_reason("a type annotation");
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::AmbiguousAssociatedProjection {
                            receiver: self_segment.clone(),
                            item,
                            cause: ProjectionFailure::BuiltinReceiver,
                        },
                        span: ann.span,
                        reason,
                    });
                    return;
                }
                self.errors.push(TypeError {
                    kind: self.nominal_error_kind(
                        self_segment,
                        TypeErrorKind::UnknownTypeName {
                            name: self_segment.clone(),
                        },
                    ),
                    span: ann.span,
                    reason: ConstraintReason::UnknownType {
                        name: self_segment.clone(),
                    },
                });
            }
            for param in &ann.type_params {
                self.check_type_annotation(param);
            }
            return;
        }

        let name_lower = ann.name.to_lowercase();

        if name_lower == "dynamic" {
            if self
                .surface_dynamic_spans
                .insert((ann.span.start, ann.span.end))
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::DynamicIsNotInSurface,
                    span: ann.span,
                    reason: ConstraintReason::TypeAnnotation {
                        var_name: ann.name.clone(),
                    },
                });
            }
            for param in &ann.type_params {
                self.check_type_annotation(param);
            }
            return;
        }

        if KNOWN_TYPE_NAMES.contains(&name_lower.as_str()) {
            let expected = match name_lower.as_str() {
                "array" | "vec" | "option" => Some(1),
                "result" => Some(2),
                _ => Some(0),
            };
            if let Some(expected) = expected
                && ann.type_params.len() != expected
            {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::GenericArityMismatch {
                        name: ann.name.clone(),
                        expected,
                        found: ann.type_params.len(),
                    },
                    span: ann.span,
                    reason: ConstraintReason::TypeAnnotation {
                        var_name: ann.name.clone(),
                    },
                });
            }
            for param in &ann.type_params {
                self.check_type_annotation(param);
            }
            return;
        }

        if ann.name.chars().next().is_some_and(|c| c.is_uppercase()) {
            if let Some(def) = self.type_table.get_struct(&ann.name) {
                if ann.type_params.len() != def.type_params.len() {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::GenericArityMismatch {
                            name: ann.name.clone(),
                            expected: def.type_params.len(),
                            found: ann.type_params.len(),
                        },
                        span: ann.span,
                        reason: ConstraintReason::TypeAnnotation {
                            var_name: ann.name.clone(),
                        },
                    });
                }
                for param in &ann.type_params {
                    self.check_type_annotation(param);
                }
                return;
            }
            if let Some(def) = self.type_table.get_enum(&ann.name) {
                if ann.type_params.len() != def.type_params.len() {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::GenericArityMismatch {
                            name: ann.name.clone(),
                            expected: def.type_params.len(),
                            found: ann.type_params.len(),
                        },
                        span: ann.span,
                        reason: ConstraintReason::TypeAnnotation {
                            var_name: ann.name.clone(),
                        },
                    });
                }
                for param in &ann.type_params {
                    self.check_type_annotation(param);
                }
                return;
            }
            if self.env.contains(&ann.name) && ann.type_params.is_empty() {
                return;
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownTypeName {
                    name: ann.name.clone(),
                },
                span: ann.span,
                reason: ConstraintReason::UnknownType {
                    name: ann.name.clone(),
                },
            });
            return;
        }

        self.errors.push(TypeError {
            kind: self.nominal_error_kind(
                &ann.name,
                TypeErrorKind::UnknownTypeName {
                    name: ann.name.clone(),
                },
            ),
            span: ann.span,
            reason: ConstraintReason::UnknownType {
                name: ann.name.clone(),
            },
        });
    }

    pub fn infer_program(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
    ) -> Result<TypedProgram, Vec<crate::constraint::TypeError>> {
        let result =
            Self::infer_program_full(stmts, source, Default::default(), Default::default())?;
        Ok(result.program)
    }

    pub fn infer_program_with_imports(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
    ) -> Result<TypedProgram, Vec<crate::constraint::TypeError>> {
        let result = Self::infer_program_full(stmts, source, module_aliases, known_globals)?;
        Ok(result.program)
    }

    pub fn infer_program_with_imports_and_natives(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
    ) -> Result<TypedProgram, Vec<crate::constraint::TypeError>> {
        let result = Self::infer_program_full_with_native_signatures(
            stmts,
            source,
            module_aliases,
            known_globals,
            known_native_globals,
            HashMap::new(),
            crate::infer::imports::ImportedTypes::default(),
        )?;
        Ok(result.program)
    }

    pub fn infer_program_with_imports_and_native_signatures(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
        known_native_signatures: HashMap<String, InferType>,
    ) -> Result<TypedProgram, Vec<crate::constraint::TypeError>> {
        let result = Self::infer_program_full_with_native_signatures(
            stmts,
            source,
            module_aliases,
            known_globals,
            known_native_globals,
            known_native_signatures,
            crate::infer::imports::ImportedTypes::default(),
        )?;
        Ok(result.program)
    }

    /// projection. the chain is followed directly, never through composite
    pub(super) fn resolve_associated_projection(
        &self,
        projection: &InferType,
    ) -> Option<InferType> {
        let mut resolved = self.resolve_associated_projection_once(projection)?;
        let mut visited = vec![projection.clone()];
        while matches!(resolved, InferType::Projection { .. }) {
            if visited.contains(&resolved) {
                return None;
            }
            visited.push(resolved.clone());
            match self.resolve_associated_projection_once(&resolved) {
                Some(next) => resolved = next,
                None => return Some(resolved),
            }
        }
        Some(resolved)
    }

    fn resolve_associated_projection_once(&self, projection: &InferType) -> Option<InferType> {
        let InferType::Projection {
            trait_name,
            item,
            self_ty,
        } = projection
        else {
            return None;
        };
        if let InferType::Param(param) = self_ty.as_ref() {
            for (bound_param, _trait_name, items) in &self.current_function_bindings {
                if bound_param != param {
                    continue;
                }
                if let Some((_, crate::infer::BoundItem::Type(ty))) =
                    items.iter().find(|(name, _)| name == item)
                {
                    return Some(ty.clone());
                }
            }
            return None;
        }
        if !self_ty.is_concrete() {
            return None;
        }
        let mut found = self.associated_projection_candidates(trait_name.as_deref(), item, self_ty);
        if found.len() == 1 {
            Some(found.swap_remove(0).1)
        } else {
            None
        }
    }

    fn resolve_trait_qualified_projection(
        &mut self,
        trait_name: &str,
        item: &str,
        span: aelys_syntax::Span,
    ) -> InferType {
        let targets = self
            .trait_qualified_items
            .get(&(trait_name.to_string(), item.to_string()))
            .cloned()
            .unwrap_or_default();
        if targets.len() == 1 {
            let target = InferType::Struct(targets[0].clone());
            let projection = InferType::Projection {
                trait_name: Some(trait_name.to_string()),
                item: item.to_string(),
                self_ty: Box::new(target),
            };
            if let Some(resolved) = self.resolve_associated_projection(&projection) {
                let reason = self.occurrence_reason("a type annotation");
                self.report_oversized_projection(&projection, span, &reason);
                return resolved;
            }
            return projection;
        }
        let mut types = targets.clone();
        types.sort();
        types.dedup();
        let rivals = match types.as_slice() {
            [only] => self.rival_instantiations(only, item, ItemNamespace::Type),
            _ => Vec::new(),
        };
        let cause = if targets.is_empty() {
            ProjectionFailure::NoImpl
        } else if rivals.len() > 1 {
            ProjectionFailure::AmbiguousInstantiations {
                trait_name: trait_name.to_string(),
                constructor: types[0].clone(),
                instantiations: rivals,
            }
        } else {
            ProjectionFailure::AmbiguousImplementors { types }
        };
        let reason = self.occurrence_reason("a type annotation");
        self.errors.push(TypeError {
            kind: TypeErrorKind::AmbiguousAssociatedProjection {
                receiver: trait_name.to_string(),
                item: item.to_string(),
                cause,
            },
            span,
            reason,
        });
        InferType::Poison
    }

    pub(super) fn associated_projection_candidates(
        &self,
        trait_name: Option<&str>,
        item: &str,
        self_ty: &InferType,
    ) -> Vec<(String, InferType)> {
        let closure = trait_name.map(|name| self.type_table.supertrait_closure(name));
        let mut found = Vec::new();
        for implementation in self.type_table.trait_impl_defs() {
            if closure
                .as_deref()
                .is_some_and(|names| !names.contains(&implementation.trait_name))
            {
                continue;
            }
            let matched = self
                .type_table
                .types_match(&implementation.self_type, self_ty);
            let bare = !matched && bare_receiver_names_header(&implementation.self_type, self_ty);
            if !matched && !bare {
                continue;
            }
            if let Some((_, ty)) = implementation
                .associated_types
                .iter()
                .find(|(name, _)| name == item)
            {
                if bare && definition_needs_arguments(&implementation.self_type, ty) {
                    continue;
                }
                found.push((
                    implementation.trait_name.clone(),
                    crate::types::instantiate_impl_definition(
                        &implementation.self_type,
                        self_ty,
                        ty,
                    ),
                ));
            }
        }
        found
    }

    pub(super) fn rival_instantiations(
        &self,
        receiver: &str,
        item: &str,
        namespace: ItemNamespace,
    ) -> Vec<String> {
        let written = InferType::Struct(receiver.to_string());
        let mut names: Vec<String> = self
            .type_table
            .trait_impl_defs()
            .iter()
            .filter(|implementation| {
                bare_receiver_names_header(&implementation.self_type, &written)
                    && match namespace {
                        ItemNamespace::Type => implementation
                            .associated_types
                            .iter()
                            .any(|(name, _)| name == item),
                        ItemNamespace::Const => implementation
                            .associated_consts
                            .iter()
                            .any(|(name, _, _, _)| name == item),
                    }
            })
            .map(|implementation| implementation.self_type.source_spelling())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub(super) fn bare_receiver_definition(&self, receiver: &str, item: &str) -> Option<String> {
        let written = InferType::Struct(receiver.to_string());
        for implementation in self.type_table.trait_impl_defs() {
            if !bare_receiver_names_header(&implementation.self_type, &written) {
                continue;
            }
            let Some((_, ty)) = implementation
                .associated_types
                .iter()
                .find(|(name, _)| name == item)
            else {
                continue;
            };
            if definition_needs_arguments(&implementation.self_type, ty) {
                return Some(ty.source_spelling());
            }
        }
        None
    }

    /// resolves a symbolic fixed-array length `[t; bounds::limit]` by looking
    fn resolve_array_length_path(
        &self,
        path: &[String],
        span: aelys_syntax::Span,
    ) -> Option<usize> {
        if path.len() != 2 {
            return None;
        }
        debug_assert!(span.end >= span.start);
        match self.resolve_constant(&path[0], &path[1]) {
            ConstResolution::Value(value) => usize::try_from(value).ok(),
            _ => None,
        }
    }

    fn report_unresolved_array_length(&mut self, path: &[String], span: aelys_syntax::Span) {
        // a path longer than `receiver::item` rooted at a module alias has no
        if path.len() > 2
            && let Some(root) = path.first()
            && self.module_aliases.contains(root)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnknownTypeName { name: root.clone() },
                span,
                reason: ConstraintReason::Other("an array length".to_string()),
            });
            return;
        }
        let (receiver, item) = match path {
            [receiver, item] => (receiver.clone(), item.clone()),
            _ => (path.join("::"), String::new()),
        };
        if let Some(ProjectionNamespace::WrongNamespace { found }) =
            self.associated_item_namespace(&receiver, &item, ItemNamespace::Const)
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver: self.projection_receiver(&receiver),
                    item: item.clone(),
                    cause: ProjectionFailure::WrongNamespace { found },
                },
                span,
                reason: ConstraintReason::Other("an array length".to_string()),
            });
            return;
        }
        if receiver == "Self"
            && self.current_impl_self.is_none()
            && self.current_trait_name.is_none()
        {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver: receiver.clone(),
                    item: item.clone(),
                    cause: ProjectionFailure::SelfOutsideImpl,
                },
                span,
                reason: ConstraintReason::Other("an array length".to_string()),
            });
            return;
        }
        if self.type_params_in_scope.contains(&receiver) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver: receiver.clone(),
                    item: item.clone(),
                    cause: ProjectionFailure::LengthFromTypeParameter,
                },
                span,
                reason: ConstraintReason::Other("an array length".to_string()),
            });
            return;
        }
        let resolution = self.resolve_constant(&receiver, &item);
        if let ConstResolution::Value(value) = resolution {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NegativeArraySize {
                    size: value,
                    constant: Some(format!("{}::{item}", self.projection_receiver(&receiver))),
                },
                span,
                reason: ConstraintReason::Other("an array length".to_string()),
            });
            return;
        }
        let cause = match resolution {
            ConstResolution::Value(_) => unreachable!("returned above"),
            ConstResolution::Ambiguous(names) => self.projection_ambiguity(&receiver, names),
            ConstResolution::AmbiguousInstantiations {
                trait_name,
                constructor,
                instantiations,
            } => ProjectionFailure::AmbiguousInstantiations {
                trait_name,
                constructor,
                instantiations,
            },
            ConstResolution::NotComputable => ProjectionFailure::NotComputable,
            ConstResolution::NotConstant => ProjectionFailure::NotConstant {
                declared: self.non_integer_const_type(&receiver, &item),
            },
            ConstResolution::Cyclic => ProjectionFailure::Cyclic {
                path: vec![format!("{receiver}::{item}")],
                namespace: crate::constraint::ItemNamespace::Const,
            },
            ConstResolution::Missing => ProjectionFailure::NoImpl,
        };
        // path the source never wrote. `lengthfromtypeparameter` is pushed above
        let named = match cause {
            ProjectionFailure::Cyclic { .. } => receiver.clone(),
            _ => self.projection_receiver(&receiver),
        };
        self.errors.push(TypeError {
            kind: TypeErrorKind::AmbiguousAssociatedProjection {
                receiver: named,
                item: item.clone(),
                cause,
            },
            span,
            reason: ConstraintReason::Other("an array length".to_string()),
        });
    }

    pub(super) fn projection_ambiguity(
        &self,
        receiver: &str,
        mut names: Vec<String>,
    ) -> ProjectionFailure {
        names.sort();
        names.dedup();
        if self.type_table.get_trait(receiver).is_some() && !self.type_table.has_nominal(receiver) {
            return ProjectionFailure::AmbiguousImplementors { types: names };
        }
        ProjectionFailure::Ambiguous {
            traits: self.traits_by_nameability(names),
        }
    }

    /// the offered repair is written in the file the caret is in, and a carried body cannot
    pub(super) fn trait_nameable_here(&self, name: &str) -> bool {
        if self.current_module == self.root_module {
            return true;
        }
        self.type_table
            .get_trait(name)
            .is_none_or(|definition| definition.owner != self.root_module)
    }

    pub(super) fn traits_by_nameability(&self, mut names: Vec<String>) -> Vec<String> {
        names.sort();
        names.dedup();
        let (here, elsewhere): (Vec<String>, Vec<String>) = names
            .into_iter()
            .partition(|name| self.trait_nameable_here(name));
        here.into_iter().chain(elsewhere).collect()
    }

    pub(super) fn projection_receiver(&self, receiver: &str) -> String {
        match (receiver, self.current_impl_self.as_ref()) {
            ("Self", Some(target)) if !self.in_trait_default_body => target.source_spelling(),
            _ => receiver.to_string(),
        }
    }

    // a default body resolves against the impl that adopted it, so no `in_trait_default_body` guard
    pub(super) fn associated_lookup_receiver(&self, receiver: &str) -> String {
        match (receiver, self.current_impl_self.as_ref()) {
            ("Self", Some(target)) => {
                crate::types::nominal_name(target).unwrap_or_else(|| receiver.to_string())
            }
            _ => receiver.to_string(),
        }
    }

    /// its own position requires. `none` means this point cannot classify the
    pub(super) fn associated_item_namespace(
        &self,
        receiver: &str,
        item: &str,
        requested: ItemNamespace,
    ) -> Option<ProjectionNamespace> {
        if receiver != "Self"
            && self
                .type_params_in_scope
                .iter()
                .any(|param| param == receiver)
        {
            if self.param_bound_declares(receiver, item, requested) {
                return Some(ProjectionNamespace::Opaque);
            }
            if self.param_bound_declares(receiver, item, requested.other()) {
                return Some(ProjectionNamespace::WrongNamespace {
                    found: requested.other(),
                });
            }
            return Some(ProjectionNamespace::Absent);
        }
        let (target, self_ty) = if receiver == "Self" {
            let ty = self.current_impl_self.as_ref()?;
            (crate::types::nominal_name(ty)?, ty.clone())
        } else {
            (
                receiver.to_string(),
                InferType::Struct(receiver.to_string()),
            )
        };
        if !self.type_table.has_nominal(&target) && self.type_table.get_trait(&target).is_none() {
            return None;
        }
        if self.concrete_defines_item(&target, &self_ty, item, requested) {
            return Some(ProjectionNamespace::Found);
        }
        if self.concrete_defines_item(&target, &self_ty, item, requested.other()) {
            return Some(ProjectionNamespace::WrongNamespace {
                found: requested.other(),
            });
        }
        Some(ProjectionNamespace::Absent)
    }

    fn concrete_defines_item(
        &self,
        target: &str,
        self_ty: &InferType,
        item: &str,
        namespace: ItemNamespace,
    ) -> bool {
        if self.type_table.has_nominal(target) {
            return match namespace {
                ItemNamespace::Type => !self
                    .associated_projection_candidates(None, item, self_ty)
                    .is_empty(),
                ItemNamespace::Const => {
                    !self.associated_const_candidates(target, item).is_empty()
                        || self
                            .associated_const_exprs
                            .contains_key(&(target.to_string(), item.to_string()))
                }
            };
        }
        self.type_table
            .trait_declaring_item_in(target, item, Some(namespace))
            .is_some()
    }

    fn param_bound_declares(&self, param: &str, item: &str, namespace: ItemNamespace) -> bool {
        self.bounds_in_scope_for_param(param)
            .iter()
            .any(|(trait_name, _)| {
                self.type_table
                    .trait_declaring_item_in(trait_name, item, Some(namespace))
                    .is_some()
            })
    }

    pub(super) fn resolve_constant(&self, receiver: &str, item: &str) -> ConstResolution {
        if receiver == "Self" {
            let Some(target) = self.current_impl_self.as_ref() else {
                return ConstResolution::Missing;
            };
            return self.resolve_associated_const_value(&target.to_string(), item);
        }
        if self
            .associated_const_exprs
            .contains_key(&(receiver.to_string(), item.to_string()))
            || self.type_table.has_nominal(receiver)
        {
            return self.resolve_associated_const_value(receiver, item);
        }
        if self.type_table.get_trait(receiver).is_some() {
            let mut targets = self
                .trait_qualified_items
                .get(&(receiver.to_string(), item.to_string()))
                .cloned()
                .unwrap_or_default();
            targets.sort();
            targets.dedup();
            return match targets.as_slice() {
                [] => ConstResolution::Missing,
                [only] => self.resolve_associated_const_value_in(only, item, Some(receiver)),
                _ => ConstResolution::Ambiguous(targets),
            };
        }
        ConstResolution::Missing
    }

    pub(super) fn resolve_associated_const_value(
        &self,
        self_name: &str,
        item: &str,
    ) -> ConstResolution {
        self.resolve_associated_const_value_in(self_name, item, None)
    }

    fn resolve_associated_const_value_in(
        &self,
        self_name: &str,
        item: &str,
        only_trait: Option<&str>,
    ) -> ConstResolution {
        let mut candidates = self.associated_const_candidates(self_name, item);
        if let Some(trait_name) = only_trait {
            let closure = self.type_table.supertrait_closure(trait_name);
            candidates.retain(|(declaring, _, _)| closure.contains(declaring));
        }
        if candidates.len() > 1 {
            let mut traits: Vec<String> =
                candidates.iter().map(|(name, _, _)| name.clone()).collect();
            traits.sort();
            traits.dedup();
            if let [only] = traits.as_slice() {
                let mut instantiations: Vec<String> = candidates
                    .into_iter()
                    .map(|(_, instantiation, _)| instantiation)
                    .collect();
                instantiations.sort();
                instantiations.dedup();
                return ConstResolution::AmbiguousInstantiations {
                    trait_name: only.clone(),
                    constructor: self_name.to_string(),
                    instantiations,
                };
            }
            return ConstResolution::Ambiguous(self.traits_by_nameability(traits));
        }
        if candidates.is_empty() {
            let key = (self_name.to_string(), item.to_string());
            let Some((owner, expr)) = self.associated_const_exprs.get(&key).cloned() else {
                return ConstResolution::Missing;
            };
            let mut state = ConstEvalState::new(key);
            return self.eval_const_expr_value(&expr, &owner, &mut state);
        }
        if let Some(value) = candidates.swap_remove(0).2 {
            return ConstResolution::Value(value);
        }
        let key = (self_name.to_string(), item.to_string());
        let Some((owner, expr)) = self.associated_const_exprs.get(&key).cloned() else {
            return ConstResolution::Missing;
        };
        let mut state = ConstEvalState::new(key);
        self.eval_const_expr_value(&expr, &owner, &mut state)
    }

    /// budget on this path rejected legitimate programs at 256. recursion is
    pub(super) fn eval_const_expr_value(
        &self,
        expr: &aelys_syntax::Expr,
        owner: &str,
        state: &mut ConstEvalState,
    ) -> ConstResolution {
        let mut stack = Vec::new();
        collect_const_references(expr, owner, &mut stack);
        while let Some(key) = stack.last().cloned() {
            if state.cache.contains_key(&key) {
                stack.pop();
                state.visiting.retain(|entry| *entry != key);
                continue;
            }
            let Some((next_owner, value)) = self.associated_const_exprs.get(&key).cloned() else {
                state.cache.insert(key, ConstResolution::Missing);
                stack.pop();
                continue;
            };
            let mut dependencies = Vec::new();
            collect_const_references(&value, &next_owner, &mut dependencies);
            let unresolved: Vec<(String, String)> = dependencies
                .into_iter()
                .filter(|dependency| !state.cache.contains_key(dependency))
                .collect();
            if unresolved.is_empty() {
                let resolved = self.eval_const_shape(&value, &next_owner, state);
                state.cache.insert(key.clone(), resolved);
                state.visiting.retain(|entry| *entry != key);
                stack.pop();
                continue;
            }
            if state.visiting.contains(&key) {
                state.cache.insert(key.clone(), ConstResolution::Cyclic);
                state.visiting.retain(|entry| *entry != key);
                stack.pop();
                continue;
            }
            state.visiting.push(key);
            for dependency in unresolved {
                stack.push(dependency);
            }
        }
        self.eval_const_shape(expr, owner, state)
    }

    fn eval_const_shape(
        &self,
        expr: &aelys_syntax::Expr,
        owner: &str,
        state: &ConstEvalState,
    ) -> ConstResolution {
        use aelys_syntax::{BinaryOp, ExprKind, UnaryOp};
        match &expr.kind {
            ExprKind::Int(value) => ConstResolution::Value(*value),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                operand,
            } => match self.eval_const_shape(operand, owner, state) {
                ConstResolution::Value(value) => value
                    .checked_neg()
                    .map_or(ConstResolution::NotComputable, ConstResolution::Value),
                other => other,
            },
            ExprKind::Grouping(inner) => self.eval_const_shape(inner, owner, state),
            ExprKind::Binary { left, op, right } => {
                let left = match self.eval_const_shape(left, owner, state) {
                    ConstResolution::Value(value) => value,
                    other => return other,
                };
                let right = match self.eval_const_shape(right, owner, state) {
                    ConstResolution::Value(value) => value,
                    other => return other,
                };
                let computed = match op {
                    BinaryOp::Add => left.checked_add(right),
                    BinaryOp::Sub => left.checked_sub(right),
                    BinaryOp::Mul => left.checked_mul(right),
                    BinaryOp::Div => left.checked_div(right),
                    BinaryOp::Mod => left.checked_rem(right),
                    _ => return ConstResolution::NotConstant,
                };
                computed.map_or(ConstResolution::NotComputable, ConstResolution::Value)
            }
            ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Path,
            } => {
                let ExprKind::Identifier(name) = &object.kind else {
                    return ConstResolution::NotConstant;
                };
                let receiver = if name == "Self" {
                    owner.to_string()
                } else {
                    name.clone()
                };
                state
                    .cache
                    .get(&(receiver, member.clone()))
                    .cloned()
                    .unwrap_or(ConstResolution::Missing)
            }
            _ => ConstResolution::NotConstant,
        }
    }

    /// item. built once so the monomorphizer can resolve `t::limit` nodes
    pub(super) fn associated_const_resolutions(
        &self,
    ) -> HashMap<(String, String), ConstResolution> {
        if let Some(cached) = self.associated_const_resolution_cache.borrow().as_ref() {
            return cached.clone();
        }
        let mut table = HashMap::new();
        for (receiver, item) in self.associated_const_exprs.keys() {
            table.insert(
                (receiver.clone(), item.clone()),
                self.resolve_associated_const_value(receiver, item),
            );
        }
        *self.associated_const_resolution_cache.borrow_mut() = Some(table.clone());
        table
    }

    pub(super) fn associated_const_candidates(
        &self,
        self_name: &str,
        item: &str,
    ) -> Vec<(String, String, Option<i64>)> {
        let self_ty = InferType::Struct(self_name.to_string());
        let mut found = Vec::new();
        for implementation in self.type_table.trait_impl_defs() {
            if !self
                .type_table
                .types_match(&implementation.self_type, &self_ty)
                && !bare_receiver_names_header(&implementation.self_type, &self_ty)
            {
                continue;
            }
            if let Some((_, _, _, value)) = implementation
                .associated_consts
                .iter()
                .find(|(name, _, _, _)| name == item)
            {
                found.push((
                    implementation.trait_name.clone(),
                    implementation.self_type.source_spelling(),
                    *value,
                ));
            }
        }
        found
    }

    fn projection_declaring_trait(&self, projection: &InferType) -> Option<String> {
        let InferType::Projection {
            trait_name,
            item,
            self_ty,
        } = projection
        else {
            return None;
        };
        if let Some(trait_name) = trait_name {
            if self.current_trait_associated_items.contains(item) {
                return Some(trait_name.clone());
            }
            // super bounds and item names, so this walk answers the same whatever
            return self.type_table.trait_declaring_item(trait_name, item);
        }
        let InferType::Param(param) = self_ty.as_ref() else {
            return None;
        };
        for (bound_param, bound_trait, _) in &self.current_function_bounds {
            if bound_param != param {
                continue;
            }
            if let Some(declaring) = self.type_table.trait_declaring_item(bound_trait, item) {
                return Some(declaring);
            }
        }
        None
    }

    pub fn infer_program_full(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
    ) -> Result<InferenceResult, Vec<crate::constraint::TypeError>> {
        Self::infer_program_full_with_natives(
            stmts,
            source,
            module_aliases,
            known_globals,
            HashSet::new(),
        )
    }

    pub fn infer_program_full_with_natives(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
    ) -> Result<InferenceResult, Vec<crate::constraint::TypeError>> {
        Self::infer_program_full_with_native_signatures(
            stmts,
            source,
            module_aliases,
            known_globals,
            known_native_globals,
            HashMap::new(),
            crate::infer::imports::ImportedTypes::default(),
        )
    }

    pub fn infer_program_full_with_native_signatures(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
        known_native_signatures: HashMap<String, InferType>,
        imported_types: crate::infer::imports::ImportedTypes,
    ) -> Result<InferenceResult, Vec<crate::constraint::TypeError>> {
        let current_module = ModuleId::new(source.name.clone());
        Self::infer_program_full_with_native_signatures_in_module(InferenceInputs {
            stmts,
            source,
            module_aliases,
            known_globals,
            known_native_globals,
            known_native_signatures,
            imported_types,
            current_module,
            scope_own_globals: false,
        })
    }

    pub fn infer_program_full_with_native_signatures_in_module(
        inputs: InferenceInputs,
    ) -> Result<InferenceResult, Vec<crate::constraint::TypeError>> {
        let InferenceInputs {
            stmts,
            source,
            module_aliases,
            known_globals,
            known_native_globals,
            known_native_signatures,
            imported_types,
            current_module,
            scope_own_globals,
        } = inputs;
        let mut inf = TypeInference::new();
        inf.root_module = current_module.clone();
        inf.current_module = current_module;
        inf.scope_own_globals = scope_own_globals;
        inf.module_aliases = module_aliases.clone();
        inf.known_globals = known_globals.clone();
        inf.known_native_globals = known_native_globals;
        inf.known_native_signatures = known_native_signatures;

        // a module alias names a namespace, so it gets a binding kind and never a type
        for alias in &module_aliases {
            inf.env.define_namespace(alias.clone());
        }

        for global in &known_globals {
            let ty = inf
                .known_native_signatures
                .get(global)
                .cloned()
                .or_else(|| crate::native::function_signature(global))
                .or_else(|| crate::native::constant_signature(global))
                .or_else(|| {
                    inf.known_native_globals
                        .contains(global)
                        .then(|| InferType::UntypedNative(global.clone()))
                });
            match ty {
                Some(ty) => inf.env.define_function_owned(global.clone(), ty),
                None => {
                    inf.globals_without_signature.insert(global.clone());
                }
            }
        }

        crate::prelude::register(&mut inf.type_table);

        inf.install_imported_types(imported_types);
        let declared_nominals = inf.declare_nominals(&stmts);
        let declared_traits = inf.declare_traits(&stmts);
        inf.collect_trait_qualified_items(&stmts);
        inf.type_table.finalize_schema_indices();
        let declared_impls = inf.declare_impl_definitions(&stmts);
        inf.collect_traits(&stmts, declared_traits);
        inf.collect_declared_impls(&stmts, declared_impls);
        inf.validate_projected_associated_const_types();
        inf.resolve_nominal_bodies(&stmts, declared_nominals);
        inf.validate_nominal_inhabitation(&stmts);
        inf.collect_function_signatures(&stmts, "");
        inf.validate_associated_type_cycles();
        inf.collect_global_bindings(&stmts);

        let typed_stmts = inf.infer_stmts(&stmts);

        let generic_templates = inf.open_nominal_templates();
        let mut subst = inf.solve_constraints();

        inf.validate_sum_method_residuals(&mut subst);
        inf.validate_dynamic_residuals(&subst);
        inf.validate_bound_residuals(&subst);
        inf.validate_try_residuals(&subst);
        inf.validate_must_use_values(&subst);
        inf.validate_match_reachability(&subst);
        inf.validate_match_exhaustivity(&subst);
        inf.validate_sum_types(&subst);

        let mut resolved_stmts = inf.apply_substitution_stmts(&typed_stmts, &subst);
        inf.resolve_deferred_members(&mut resolved_stmts);
        let resolved_stmts = inf.monomorphize_program(resolved_stmts);

        inf.validate_unused_sum_bindings(&resolved_stmts);
        inf.validate_surface_type_boundaries(&resolved_stmts);

        let final_stmts = inf.finalize_stmts(resolved_stmts);

        if inf.projection_cycle_escaped.get() && inf.errors.is_empty() {
            // escaped, and a poisoned type must never pass for a valid one.
            inf.errors.push(TypeError {
                kind: TypeErrorKind::PoisonedType,
                span: aelys_syntax::Span::dummy(),
                reason: ConstraintReason::Other(
                    "associated projection cycle escaped collection".to_string(),
                ),
            });
        }
        for (verdict, source, target, span) in inf.conversion_verdicts.borrow_mut().drain(..) {
            let source_error = match &source {
                InferType::Result(_, error) => error.as_ref().clone(),
                other => other.clone(),
            };
            let target_error = match &target {
                InferType::Result(_, error) => error.as_ref().clone(),
                other => other.clone(),
            };
            let kind = match verdict {
                crate::types::FromSelection::Denied => {
                    crate::constraint::TypeErrorKind::UnsatisfiedTraitBound {
                        trait_name: crate::prelude::FROM_TRAIT.to_string(),
                        trait_args: vec![source_error.clone()],
                        ty: target_error,
                        denied: true,
                    }
                }
                crate::types::FromSelection::Unresolved(candidates) => {
                    crate::constraint::TypeErrorKind::UnsatisfiedTryConversion {
                        source_error,
                        target_error,
                        source,
                        target,
                        candidates,
                    }
                }
                _ => continue,
            };
            inf.errors.push(crate::constraint::TypeError {
                kind,
                span,
                reason: crate::constraint::ConstraintReason::Other(
                    "question mark conversion".to_string(),
                ),
            });
        }
        for (verdict, ty, span) in inf.specialization_verdicts.borrow_mut().drain(..) {
            let kind = match verdict {
                crate::types::SpecializationChoice::Ambiguous(trait_name) => {
                    crate::constraint::TypeErrorKind::AmbiguousSpecialization {
                        trait_name,
                        target: ty,
                    }
                }
                crate::types::SpecializationChoice::TooMany(trait_name) => {
                    crate::constraint::TypeErrorKind::SpecializationLimit {
                        trait_name,
                        target: ty,
                    }
                }
                crate::types::SpecializationChoice::Denied(trait_name) => {
                    crate::constraint::TypeErrorKind::UnsatisfiedTraitBound {
                        trait_name,
                        trait_args: Vec::new(),
                        ty,
                        denied: true,
                    }
                }
                _ => continue,
            };
            inf.errors.push(crate::constraint::TypeError {
                kind,
                span,
                reason: crate::constraint::ConstraintReason::Other(
                    "specialization selection".to_string(),
                ),
            });
        }
        if let Some(span) = inf.substitution_overflowed.get() {
            inf.errors.push(TypeError::recursion_limit(span));
        }

        inf.errors.sort_by_key(|error| {
            (
                error_priority(&error.kind),
                error.span.start,
                error.span.end,
                same_span_priority(&error.kind),
            )
        });

        if !inf.errors.is_empty() {
            return Err(inf.errors);
        }

        let type_table = inf.type_table;

        Ok(InferenceResult {
            program: TypedProgram {
                stmts: final_stmts,
                source,
                type_table: type_table.clone(),
            },
            warnings: inf.warnings,
            type_table,
            generic_structs: generic_templates.0,
            generic_enums: generic_templates.1,
        })
    }

    fn collect_global_bindings(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            let StmtKind::Let {
                name,
                mutable,
                type_annotation,
                initializer,
                ..
            } = &stmt.kind
            else {
                continue;
            };

            let ty = type_annotation
                .as_ref()
                .map(|annotation| self.type_from_annotation(annotation))
                .unwrap_or_else(|| self.global_initializer_type(initializer));
            let known_length = self.constant_collection_length(initializer).or(match &ty {
                InferType::FixedArray(_, length) => Some(*length),
                _ => None,
            });
            if type_annotation.is_some() && ty.contains_dynamic() {
                self.env.define_explicit_dynamic_local(name.clone(), ty);
            } else if let Some(length) = known_length {
                self.env
                    .define_local_with_collection_length(name.clone(), ty, length);
            } else {
                self.env.define_local(name.clone(), ty);
            }
            self.env.set_mutable(name, *mutable);
            if self.scope_own_globals {
                let scoped = crate::infer::module_scoped_global(self.current_module.as_str(), name);
                self.env.define_local_alias(name.clone(), scoped);
            }
        }
    }

    fn global_initializer_type(&mut self, expr: &Expr) -> InferType {
        match &expr.kind {
            ExprKind::Int(_) => InferType::I64,
            ExprKind::Float(_) => InferType::F64,
            ExprKind::Bool(_) => InferType::Bool,
            ExprKind::String(_) | ExprKind::FmtString(_) => InferType::String,
            ExprKind::Unit => InferType::Unit,
            ExprKind::ArrayLiteral {
                element_type,
                elements,
            } => {
                let inner = element_type
                    .as_ref()
                    .map(|annotation| self.type_from_annotation(annotation))
                    .or_else(|| {
                        elements
                            .first()
                            .map(|element| self.global_initializer_type(element))
                    })
                    .unwrap_or(InferType::Poison);
                if expr.repeat.is_some() {
                    if let Some(length) = self.constant_collection_length(expr) {
                        InferType::FixedArray(Box::new(inner), length)
                    } else {
                        InferType::Array(Box::new(inner))
                    }
                } else {
                    InferType::FixedArray(Box::new(inner), elements.len())
                }
            }
            ExprKind::VecLiteral {
                element_type,
                elements,
            } => InferType::Vec(Box::new(
                element_type
                    .as_ref()
                    .map(|annotation| self.type_from_annotation(annotation))
                    .or_else(|| {
                        elements
                            .first()
                            .map(|element| self.global_initializer_type(element))
                    })
                    .unwrap_or(InferType::Poison),
            )),
            ExprKind::ArraySized { element_type, .. } => InferType::Array(Box::new(
                element_type
                    .as_ref()
                    .map(|annotation| self.type_from_annotation(annotation))
                    .unwrap_or(InferType::Poison),
            )),
            ExprKind::StructLiteral { name, .. } => InferType::Struct(name.clone()),
            _ => InferType::Poison,
        }
    }
}

// so it must not outrank that rejection on an earlier span.
fn bare_receiver_names_header(header: &InferType, written: &InferType) -> bool {
    matches!(
        (header, written),
        (InferType::Applied { name, .. }, InferType::Struct(receiver)) if name == receiver
    )
}

fn definition_needs_arguments(header: &InferType, definition: &InferType) -> bool {
    let InferType::Applied { args, .. } = header else {
        return false;
    };
    let mut params = Vec::new();
    for arg in args {
        collect_type_params(arg, &mut params);
    }
    params.iter().any(|param| definition.mentions_param(param))
}

pub(crate) fn collect_type_params(ty: &InferType, out: &mut Vec<String>) {
    match ty {
        InferType::Param(name) => out.push(name.clone()),
        InferType::Applied { args, .. } | InferType::Tuple(args) => {
            for arg in args {
                collect_type_params(arg, out);
            }
        }
        InferType::Array(inner)
        | InferType::Vec(inner)
        | InferType::Option(inner)
        | InferType::FixedArray(inner, _) => collect_type_params(inner, out),
        InferType::Result(ok, error) => {
            collect_type_params(ok, out);
            collect_type_params(error, out);
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_type_params(param, out);
            }
            collect_type_params(ret, out);
        }
        InferType::Projection { self_ty, .. } => collect_type_params(self_ty, out),
        _ => {}
    }
}

fn error_priority(kind: &TypeErrorKind) -> u8 {
    match kind {
        TypeErrorKind::DuplicateNominal { .. } => 0,
        // a mangling collision is a symptom: a coherence verdict, a duplicate
        TypeErrorKind::MangledSymbolCollision { .. } => 3,
        TypeErrorKind::PoisonedType
        | TypeErrorKind::UnmaterializedAppliedType { .. }
        | TypeErrorKind::IgnoredResult
        | TypeErrorKind::IgnoredOption => 2,
        _ => 1,
    }
}

fn same_span_priority(kind: &TypeErrorKind) -> u8 {
    match kind {
        TypeErrorKind::NonExhaustiveMatch { .. } | TypeErrorKind::NonExhaustiveStruct { .. } => 0,
        _ => 1,
    }
}

fn collect_const_references(
    expr: &aelys_syntax::Expr,
    owner: &str,
    out: &mut Vec<(String, String)>,
) {
    use aelys_syntax::ExprKind;
    match &expr.kind {
        ExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Path,
        } => {
            if let ExprKind::Identifier(name) = &object.kind {
                let receiver = if name == "Self" {
                    owner.to_string()
                } else {
                    name.clone()
                };
                out.push((receiver, member.clone()));
            }
        }
        ExprKind::Unary { operand, .. } => collect_const_references(operand, owner, out),
        ExprKind::Grouping(inner) => collect_const_references(inner, owner, out),
        ExprKind::Binary { left, right, .. } => {
            collect_const_references(left, owner, out);
            collect_const_references(right, owner, out);
        }
        _ => {}
    }
}
