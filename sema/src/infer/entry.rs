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
            try_residuals: Vec::new(),
            must_use_values: Vec::new(),
            dynamic_residuals: Vec::new(),
            sum_type_residuals: Vec::new(),
            explicit_dynamic_functions: HashSet::new(),
            module_aliases: HashSet::new(),
            known_globals: HashSet::new(),
            known_native_globals: HashSet::new(),
            known_native_signatures: HashMap::new(),
        }
    }
}

impl TypeInference {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn type_from_annotation(&mut self, ann: &TypeAnnotation) -> InferType {
        self.check_type_annotation(ann);
        InferType::from_annotation(ann)
    }

    fn check_type_annotation(&mut self, ann: &TypeAnnotation) {
        if self.type_params_in_scope.iter().any(|tp| tp == &ann.name) {
            return;
        }

        let name_lower = ann.name.to_lowercase();

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
            if self.env.contains(&ann.name) && ann.type_params.is_empty() {
                return;
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::Mismatch {
                    expected: InferType::Dynamic,
                    found: InferType::Struct(ann.name.clone()),
                },
                span: ann.span,
                reason: ConstraintReason::UnknownType {
                    name: ann.name.clone(),
                },
            });
            return;
        }

        self.errors.push(TypeError {
            kind: TypeErrorKind::Mismatch {
                expected: InferType::Dynamic,
                found: InferType::Dynamic,
            },
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
        )
    }

    pub fn infer_program_full_with_native_signatures(
        stmts: Vec<Stmt>,
        source: Arc<Source>,
        module_aliases: HashSet<String>,
        known_globals: HashSet<String>,
        known_native_globals: HashSet<String>,
        known_native_signatures: HashMap<String, InferType>,
    ) -> Result<InferenceResult, Vec<crate::constraint::TypeError>> {
        let mut inf = TypeInference::new();
        inf.module_aliases = module_aliases.clone();
        inf.known_globals = known_globals.clone();
        inf.known_native_globals = known_native_globals;
        inf.known_native_signatures = known_native_signatures;

        for alias in &module_aliases {
            inf.env
                .define_function_owned(alias.clone(), InferType::Dynamic);
        }

        for global in &known_globals {
            let ty = inf
                .known_native_signatures
                .get(global)
                .cloned()
                .or_else(|| crate::native::function_signature(global))
                .or_else(|| crate::native::constant_signature(global))
                .unwrap_or_else(|| {
                    if inf.known_native_globals.contains(global) {
                        InferType::UntypedNative(global.clone())
                    } else {
                        InferType::Dynamic
                    }
                });
            inf.env.define_function_owned(global.clone(), ty);
        }

        inf.collect_structs(&stmts);
        inf.collect_signatures(&stmts, "");
        inf.collect_global_bindings(&stmts);

        let typed_stmts = inf.infer_stmts(&stmts);

        let subst = inf.solve_constraints();

        inf.validate_dynamic_residuals(&subst);
        inf.validate_try_residuals(&subst);
        inf.validate_must_use_values(&subst);
        inf.validate_sum_types(&subst);

        let resolved_stmts = inf.apply_substitution_stmts(&typed_stmts, &subst);

        inf.validate_unused_sum_bindings(&resolved_stmts);

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
            self.env.define_local(name.clone(), ty);
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
            } => InferType::Array(Box::new(
                element_type
                    .as_ref()
                    .map(|annotation| self.type_from_annotation(annotation))
                    .or_else(|| {
                        elements
                            .first()
                            .map(|element| self.global_initializer_type(element))
                    })
                    .unwrap_or(InferType::Dynamic),
            )),
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
                    .unwrap_or(InferType::Dynamic),
            )),
            ExprKind::ArraySized { element_type, .. } => InferType::Array(Box::new(
                element_type
                    .as_ref()
                    .map(|annotation| self.type_from_annotation(annotation))
                    .unwrap_or(InferType::Dynamic),
            )),
            ExprKind::StructLiteral { name, .. } => InferType::Struct(name.clone()),
            _ => InferType::Dynamic,
        }
    }
}
