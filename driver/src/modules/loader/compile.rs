use super::types::{ModuleInfo, ModuleLoader};
use aelys_backend::Compiler;
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::Optimizer;
use aelys_runtime::VM;
use aelys_sema::{InferType, TypeInference, TypedStmtKind};
use aelys_syntax::{ModuleId, Source, StmtKind};
use std::path::Path;
use std::sync::Arc;

impl ModuleLoader {
    pub(crate) fn compile_module(
        &mut self,
        file_path: &Path,
        module_path_str: &str,
        needs: &aelys_syntax::NeedsStmt,
        vm: &mut VM,
    ) -> Result<()> {
        let content = std::fs::read_to_string(file_path).map_err(|_| {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: module_path_str.to_string(),
                    searched_paths: vec![file_path.display().to_string()],
                },
                needs.span,
                self.source.clone(),
            ))
        })?;

        let module_source = Source::new(file_path.display().to_string(), &content);
        let tokens = Lexer::with_source(module_source.clone()).scan()?;
        let stmts = Parser::new_rust_collections(tokens, module_source.clone()).parse()?;

        let exports = self.collect_exports(&stmts, module_path_str)?;

        let module_name = needs
            .path
            .last()
            .cloned()
            .expect("needs.path validated as non-empty");
        let mut export_signatures = std::collections::HashMap::new();
        for stmt in &stmts {
            let StmtKind::Function(func) = &stmt.kind else {
                continue;
            };
            if !func.is_pub {
                continue;
            }
            let signature = InferType::Function {
                params: func
                    .params
                    .iter()
                    .map(|param| {
                        param
                            .type_annotation
                            .as_ref()
                            .map(InferType::from_annotation)
                            .unwrap_or(InferType::Dynamic)
                    })
                    .collect(),
                ret: Box::new(
                    func.return_type
                        .as_ref()
                        .map(InferType::from_annotation)
                        .unwrap_or(InferType::Dynamic),
                ),
            };
            export_signatures.insert(func.name.clone(), signature.clone());
            export_signatures.insert(format!("{}::{}", module_name, func.name), signature);
        }
        let module_info = ModuleInfo {
            name: module_name.clone(),
            path: module_path_str.to_string(),
            file_path: file_path.to_path_buf(),
            version: None,
            exports: exports.clone(),
            native_functions: Vec::new(),
            native_signatures: export_signatures,
            exported_types: super::exported_types::ExportedTypes::default(),
        };
        self.loaded_modules
            .insert(module_path_str.to_string(), module_info);

        let original_base_dir = self.base_dir.clone();
        let original_base_root = self.base_root.clone();
        self.base_dir = file_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(original_base_dir.clone());
        self.base_root = self
            .base_dir
            .canonicalize()
            .unwrap_or_else(|_| self.base_dir.clone());

        let mut module_aliases = std::collections::HashSet::new();
        let mut known_globals = std::collections::HashSet::new();
        let mut known_native_globals = std::collections::HashSet::new();
        let mut native_signatures = std::collections::HashMap::new();
        let mut imported_types = aelys_sema::infer::imports::ImportedTypes::default();
        let mut imported_impl_stmts: Vec<aelys_syntax::Stmt> = Vec::new();
        let mut nominal_scopes: Vec<super::exported_types::NominalScopeEntry> = Vec::new();
        let mut module_sources: std::collections::HashMap<String, Arc<Source>> =
            std::collections::HashMap::new();

        for stmt in &stmts {
            if let StmtKind::Needs(nested_needs) = &stmt.kind {
                let result = self.load_module(nested_needs, vm)?;

                match result {
                    super::types::LoadResult::Module(alias) => {
                        module_aliases.insert(alias.clone());
                    }
                    super::types::LoadResult::Symbol(symbol) => {
                        known_globals.insert(symbol);
                    }
                }

                let nested_module_path = nested_needs.path.join(".");
                if let Some(module_info) = self.get_module(&nested_module_path) {
                    for native_name in &module_info.native_functions {
                        known_native_globals.insert(native_name.clone());
                    }
                    native_signatures.extend(module_info.native_signatures.clone());
                    let wanted = match &nested_needs.kind {
                        aelys_syntax::ImportKind::Symbols(symbols) => Some(symbols.clone()),
                        _ => None,
                    };
                    super::exported_types::widen_nominal_scope(
                        &mut nominal_scopes,
                        &nested_module_path,
                        wanted,
                        nested_needs.span,
                    );

                    match &nested_needs.kind {
                        aelys_syntax::ImportKind::Module { alias: None }
                        | aelys_syntax::ImportKind::Wildcard => {
                            for name in module_info.exports.keys() {
                                let module_alias = self.get_module_alias(nested_needs);
                                known_globals.insert(format!("{}::{}", module_alias, name));
                                known_globals.insert(name.clone());
                                if let Some(signature) = module_info
                                    .native_signatures
                                    .get(&format!("{}::{}", module_info.name, name))
                                {
                                    native_signatures.insert(name.clone(), signature.clone());
                                }
                            }
                        }
                        aelys_syntax::ImportKind::Module { alias: Some(_) } => {
                            let module_alias = self.get_module_alias(nested_needs);
                            for name in module_info.exports.keys() {
                                let alias_qualified = format!("{}::{}", module_alias, name);
                                let internal_qualified = format!("{}::{}", module_info.name, name);
                                known_globals.insert(alias_qualified.clone());
                                if module_info.native_functions.contains(&alias_qualified)
                                    || module_info.native_functions.contains(&internal_qualified)
                                {
                                    known_native_globals.insert(alias_qualified);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        for entry in &nominal_scopes {
            let Some(module_info) = self.get_module(&entry.module_path) else {
                continue;
            };
            let (selected, selected_impls) = super::exported_types::select_exported_nominals(
                &module_info.exported_types,
                &entry.module_path,
                entry.scope(),
            );
            imported_types.extend(selected);
            if !selected_impls.is_empty() {
                if let Some(source) = &module_info.exported_types.source {
                    module_sources.insert(
                        ModuleId::new(entry.module_path.clone())
                            .as_str()
                            .to_string(),
                        source.clone(),
                    );
                }
                imported_types.module_globals.insert(
                    ModuleId::new(entry.module_path.clone())
                        .as_str()
                        .to_string(),
                    module_info.exported_types.globals.clone(),
                );
            }
            imported_impl_stmts.extend(
                selected_impls.into_iter().map(|stmt| {
                    stmt.with_definition_module(ModuleId::new(entry.module_path.clone()))
                }),
            );
        }

        self.base_dir = original_base_dir;
        self.base_root = original_base_root;

        let declaration_stmts: Vec<_> = stmts
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    StmtKind::EnumDecl { .. }
                        | StmtKind::StructDecl { .. }
                        | StmtKind::TraitDecl { .. }
                        | StmtKind::ImplDecl { .. }
                )
            })
            .cloned()
            .collect();

        let main_stmts: Vec<_> = imported_impl_stmts
            .into_iter()
            .chain(
                stmts
                    .into_iter()
                    .filter(|s| !matches!(s.kind, StmtKind::Needs(_))),
            )
            .collect();

        let inference_result = TypeInference::infer_program_full_with_native_signatures_in_module(
            aelys_sema::InferenceInputs {
                stmts: main_stmts,
                source: module_source.clone(),
                module_aliases: module_aliases.clone(),
                known_globals: known_globals.clone(),
                known_native_globals: known_native_globals.clone(),
                known_native_signatures: native_signatures.clone(),
                imported_types,
                current_module: ModuleId::new(module_path_str),
            },
        )
        .map_err(|errors| {
            if let Some(err) = errors.first() {
                AelysError::Compile(CompileError::new(
                    CompileErrorKind::NamedTypeError {
                        code: err.diagnostic_code(),
                        message: format!("{}", err),
                    },
                    err.span,
                    module_source.clone(),
                ))
            } else {
                AelysError::Compile(CompileError::new(
                    CompileErrorKind::TypeInferenceError("Unknown type error".to_string()),
                    aelys_syntax::Span::dummy(),
                    module_source.clone(),
                ))
            }
        })?;
        let typed_program = inference_result.program;

        let mut inferred_export_signatures = std::collections::HashMap::new();
        for stmt in &typed_program.stmts {
            match &stmt.kind {
                TypedStmtKind::Function(function) if function.is_pub => {
                    let signature = InferType::Function {
                        params: function
                            .params
                            .iter()
                            .map(|param| param.ty.clone())
                            .collect(),
                        ret: Box::new(function.return_type.clone()),
                    };
                    inferred_export_signatures.insert(function.name.clone(), signature.clone());
                    inferred_export_signatures
                        .insert(format!("{}::{}", module_name, function.name), signature);
                }
                TypedStmtKind::Let {
                    name,
                    is_pub: true,
                    var_type,
                    ..
                } => {
                    inferred_export_signatures.insert(name.clone(), var_type.clone());
                    inferred_export_signatures
                        .insert(format!("{}::{}", module_name, name), var_type.clone());
                }
                _ => {}
            }
        }
        super::exported_types::reject_nominal_boundary_signatures(
            &typed_program.stmts,
            &inference_result.type_table,
            module_path_str,
            &module_source,
        )?;
        let mut exported_types = super::exported_types::collect_exported_types(
            &declaration_stmts,
            &inference_result.type_table,
            module_path_str,
            module_source.clone(),
        )?;
        exported_types.source = Some(module_source.clone());
        for name in &known_globals {
            if let Some(signature) = native_signatures.get(name) {
                exported_types
                    .globals
                    .insert(name.clone(), signature.clone());
            }
        }
        for stmt in &typed_program.stmts {
            match &stmt.kind {
                TypedStmtKind::Let { name, var_type, .. } => {
                    exported_types
                        .globals
                        .insert(name.clone(), var_type.clone());
                }
                TypedStmtKind::Function(function) => {
                    exported_types.globals.insert(
                        function.name.clone(),
                        InferType::Function {
                            params: function.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: Box::new(function.return_type.clone()),
                        },
                    );
                }
                _ => {}
            }
        }
        if let Some(module_info) = self.loaded_modules.get_mut(module_path_str) {
            module_info.native_signatures = inferred_export_signatures;
            module_info.exported_types = exported_types;
        }

        let mut optimizer = Optimizer::new(aelys_opt::OptimizationLevel::Standard);
        let typed_program = optimizer.optimize(typed_program);

        let compiler = Compiler::with_modules(
            Some(module_path_str.to_string()),
            module_source.clone(),
            module_aliases,
            known_globals,
            known_native_globals,
            std::collections::HashMap::new(),
        );
        let (function, _globals) = compiler.compile_typed(&typed_program)?;

        let global_layout = Arc::clone(&function.global_layout);

        let func_ref = vm.alloc_function(function)?;
        vm.execute(func_ref)?;

        vm.sync_globals_to_hashmap(global_layout.names());

        self.register_exports(needs, &exports, vm)?;

        Ok(())
    }
}
