use super::{KNOWN_TYPE_NAMES, TypeInference};
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::TypedProgram;
use crate::types::{InferType, TypeTable};
use aelys_common::Warning;
use aelys_syntax::{Expr, ExprKind, Source, Stmt, StmtKind, TypeAnnotation};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct InferenceResult {
    pub program: TypedProgram,
    pub warnings: Vec<Warning>,
    pub type_table: TypeTable,
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
            trait_defaults: HashMap::new(),
            generic_function_bounds: HashMap::new(),
            function_type_params: HashMap::new(),
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
            known_native_globals: HashSet::new(),
            known_native_signatures: HashMap::new(),
            collection_iter_allowed: false,
            withheld_nominals: std::collections::BTreeMap::new(),
            monomorphization_active: Vec::new(),
        }
    }
}

impl TypeInference {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn type_from_annotation(&mut self, ann: &TypeAnnotation) -> InferType {
        self.check_type_annotation(ann);
        self.type_from_annotation_inner(ann)
    }

    fn type_from_annotation_inner(&mut self, ann: &TypeAnnotation) -> InferType {
        if ann.is_function_type() {
            let params = ann
                .fn_params
                .as_ref()
                .map(|params| {
                    params
                        .iter()
                        .map(|param| self.type_from_annotation_inner(param))
                        .collect()
                })
                .unwrap_or_default();
            let ret = ann
                .fn_ret
                .as_ref()
                .map(|ret| self.type_from_annotation_inner(ret))
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
            return InferType::Param(ann.name.clone());
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
                    .map(|param| self.type_from_annotation_inner(param))
                    .unwrap_or(InferType::Poison);
                match ann.array_length {
                    Some(length) => InferType::FixedArray(Box::new(inner), length as usize),
                    None => InferType::Array(Box::new(inner)),
                }
            }
            "vec" => InferType::Vec(Box::new(
                ann.type_params
                    .first()
                    .map(|param| self.type_from_annotation_inner(param))
                    .unwrap_or(InferType::Poison),
            )),
            "option" => InferType::Option(Box::new(
                ann.type_params
                    .first()
                    .map(|param| self.type_from_annotation_inner(param))
                    .unwrap_or(InferType::Poison),
            )),
            "result" => InferType::Result(
                Box::new(
                    ann.type_params
                        .first()
                        .map(|param| self.type_from_annotation_inner(param))
                        .unwrap_or(InferType::Poison),
                ),
                Box::new(
                    ann.type_params
                        .get(1)
                        .map(|param| self.type_from_annotation_inner(param))
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
                        .map(|param| self.type_from_annotation_inner(param))
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
        if self.type_params_in_scope.iter().any(|tp| tp == &ann.name) {
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
        let mut inf = TypeInference::new();
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
        inf.collect_enums(&stmts);
        inf.collect_structs(&stmts);
        inf.type_table.finalize_schema_indices();
        inf.collect_traits(&stmts);
        inf.collect_signatures(&stmts, "");
        inf.collect_global_bindings(&stmts);

        let typed_stmts = inf.infer_stmts(&stmts);

        let mut subst = inf.solve_constraints();

        inf.validate_sum_method_residuals(&mut subst);
        inf.validate_dynamic_residuals(&subst);
        inf.validate_bound_residuals(&subst);
        inf.validate_try_residuals(&subst);
        inf.validate_must_use_values(&subst);
        inf.validate_match_reachability(&subst);
        inf.validate_match_exhaustivity(&subst);
        inf.validate_sum_types(&subst);

        let resolved_stmts = inf.apply_substitution_stmts(&typed_stmts, &subst);
        let resolved_stmts = inf.monomorphize_program(resolved_stmts);

        inf.validate_unused_sum_bindings(&resolved_stmts);
        inf.validate_surface_type_boundaries(&resolved_stmts);

        let final_stmts = inf.finalize_stmts(resolved_stmts);

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
            let known_length = crate::infer::expr::array::constant_collection_length(initializer)
                .or(match &ty {
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
                    if let Some(length) =
                        crate::infer::expr::array::constant_collection_length(expr)
                    {
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
