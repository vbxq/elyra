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
        let mut own_nominals: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut inherited_nominals: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut nominal_origins: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut module_sources: std::collections::HashMap<String, Arc<Source>> =
            std::collections::HashMap::new();
        let mut scoped_globals: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut qualified_imports: std::collections::BTreeMap<String, InferType> =
            std::collections::BTreeMap::new();

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
                                let qualified = format!("{}::{}", module_alias, name);
                                if let Some(ty) = module_info.exported_types.globals.get(name) {
                                    qualified_imports.insert(qualified.clone(), ty.clone());
                                }
                                known_globals.insert(qualified);
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
                                if let Some(ty) = module_info.exported_types.globals.get(name) {
                                    qualified_imports.insert(alias_qualified.clone(), ty.clone());
                                }
                                known_globals.insert(alias_qualified.clone());
                                if module_info.native_functions.contains(&alias_qualified)
                                    || module_info.native_functions.contains(&internal_qualified)
                                {
                                    known_native_globals.insert(alias_qualified.clone());
                                }
                                if let Some(signature) = module_info
                                    .native_signatures
                                    .get(&alias_qualified)
                                    .or_else(|| {
                                        module_info.native_signatures.get(&internal_qualified)
                                    })
                                    .or_else(|| module_info.native_signatures.get(name))
                                {
                                    native_signatures.insert(alias_qualified, signature.clone());
                                }
                            }
                        }
                        aelys_syntax::ImportKind::Symbols(symbols) => {
                            for symbol in symbols {
                                if !module_info.exports.contains_key(symbol) {
                                    continue;
                                }
                                let internal_qualified =
                                    format!("{}::{}", module_info.name, symbol);
                                if let Some(ty) = module_info.exported_types.globals.get(symbol) {
                                    qualified_imports.insert(symbol.clone(), ty.clone());
                                }
                                known_globals.insert(symbol.clone());
                                if module_info.native_functions.contains(&internal_qualified) {
                                    known_native_globals.insert(symbol.clone());
                                }
                                if let Some(signature) = module_info
                                    .native_signatures
                                    .get(&internal_qualified)
                                    .or_else(|| module_info.native_signatures.get(symbol))
                                {
                                    native_signatures.insert(symbol.clone(), signature.clone());
                                }
                            }
                        }
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
            for name in selected.nominal_names() {
                match selected.private_nominals.get(&name) {
                    Some(origin) => {
                        inherited_nominals.insert(name.clone());
                        nominal_origins.insert(name, origin.clone());
                    }
                    None => {
                        own_nominals.insert(name.clone());
                        nominal_origins.insert(name, entry.module_path.replace('.', "::"));
                    }
                }
            }
            let inherited_types = module_info.exported_types.private_types.clone();
            let inherited_impls = module_info.exported_types.private_impls.clone();
            module_sources.extend(
                module_info
                    .exported_types
                    .private_module_sources
                    .iter()
                    .map(|(module, source)| (module.clone(), source.clone())),
            );
            for (name, origin) in &module_info.exported_types.private_origins {
                nominal_origins.insert(name.clone(), origin.clone());
                inherited_nominals.insert(name.clone());
            }
            imported_types.extend(inherited_types);
            imported_impl_stmts.extend(inherited_impls);
            for name in selected.nominal_names() {
                if let Some(ordinal) = module_info.exported_types.struct_def_ordinals.get(&name) {
                    imported_types
                        .struct_def_ordinals
                        .insert(name.clone(), *ordinal);
                }
                if let Some(ordinal) = module_info.exported_types.enum_def_ordinals.get(&name) {
                    imported_types
                        .enum_def_ordinals
                        .insert(name.clone(), *ordinal);
                }
            }
            imported_types.extend(selected);
            {
                if let Some(source) = &module_info.exported_types.source {
                    module_sources.insert(
                        ModuleId::new(entry.module_path.clone())
                            .as_str()
                            .to_string(),
                        source.clone(),
                    );
                }
                let module_id = ModuleId::new(entry.module_path.clone())
                    .as_str()
                    .to_string();
                imported_types.module_globals.insert(
                    module_id.clone(),
                    module_info.exported_types.globals.clone(),
                );
                for name in &module_info.exported_types.scoped_globals {
                    scoped_globals.insert(aelys_sema::module_scoped_global(&module_id, name));
                }
                imported_types
                    .module_scoped_globals
                    .insert(module_id, module_info.exported_types.scoped_globals.clone());
            }
            if !selected_impls.is_empty() {
                for name in module_info.exported_types.own_private_types.nominal_names() {
                    inherited_nominals.insert(name.clone());
                    nominal_origins.insert(name.clone(), entry.module_path.replace('.', "::"));
                    imported_types.unexported_nominals.insert(name);
                }
                for original in module_info.exported_types.renamed_privates.keys() {
                    inherited_nominals.insert(original.clone());
                    nominal_origins.insert(original.clone(), entry.module_path.replace('.', "::"));
                    imported_types.unexported_nominals.insert(original.clone());
                }
                for (name, ordinal) in &module_info.exported_types.struct_def_ordinals {
                    imported_types
                        .struct_def_ordinals
                        .insert(name.clone(), *ordinal);
                }
                for (name, ordinal) in &module_info.exported_types.enum_def_ordinals {
                    imported_types
                        .enum_def_ordinals
                        .insert(name.clone(), *ordinal);
                }
                imported_types.extend(module_info.exported_types.own_private_types.clone());
                imported_impl_stmts.extend(
                    module_info
                        .exported_types
                        .own_private_impls
                        .iter()
                        .cloned()
                        .map(|stmt| {
                            stmt.with_definition_module(ModuleId::new(entry.module_path.clone()))
                        }),
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

        // reach this module by two routes, so the same body must not register twice
        let mut seen_impls: std::collections::HashSet<(String, usize, usize)> =
            std::collections::HashSet::new();
        imported_impl_stmts.retain(|stmt| {
            let module = stmt
                .definition_module
                .as_ref()
                .map(|module| module.as_str().to_string())
                .unwrap_or_default();
            seen_impls.insert((module, stmt.span.start, stmt.span.end))
        });
        let private_types = imported_types.clone();
        let private_impls = imported_impl_stmts.clone();
        let private_nominals: std::collections::BTreeMap<String, String> = inherited_nominals
            .iter()
            .filter(|name| !own_nominals.contains(*name))
            .filter_map(|name| {
                nominal_origins
                    .get(name)
                    .map(|origin| (name.clone(), origin.clone()))
            })
            .collect();
        imported_types.private_nominals = private_nominals;

        // mangled method symbols are minted there and cannot be rewritten afterwards
        let applied_renames =
            super::exported_types::module_private_nominal_renames(&stmts, module_path_str);

        let mut declaration_stmts: Vec<_> = stmts
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
        super::rename::rename_stmts(&mut declaration_stmts, &applied_renames);

        let mut own_stmts: Vec<_> = stmts
            .into_iter()
            .filter(|s| !matches!(s.kind, StmtKind::Needs(_)))
            .collect();
        super::rename::rename_stmts(&mut own_stmts, &applied_renames);

        let main_stmts: Vec<_> = imported_impl_stmts.into_iter().chain(own_stmts).collect();

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
                scope_own_globals: true,
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
                    let name = aelys_sema::unscoped_global_name(name);
                    inferred_export_signatures.insert(name.to_string(), var_type.clone());
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
            &inference_result.generic_structs,
            &inference_result.generic_enums,
            module_path_str,
            module_source.clone(),
            &applied_renames,
        )?;
        exported_types.source = Some(module_source.clone());
        exported_types.private_types = private_types;
        exported_types.private_impls = private_impls;
        exported_types.private_origins = nominal_origins;
        for entry in &nominal_scopes {
            if let Some(module_info) = self.get_module(&entry.module_path) {
                exported_types
                    .renamed_privates
                    .extend(module_info.exported_types.renamed_privates.clone());
                exported_types
                    .renamed_private_traits
                    .extend(module_info.exported_types.renamed_private_traits.clone());
            }
        }
        exported_types.private_module_sources = module_sources.clone();
        exported_types.globals.extend(qualified_imports);
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
                    exported_types.globals.insert(
                        aelys_sema::unscoped_global_name(name).to_string(),
                        var_type.clone(),
                    );
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
        let module_global_names: Vec<String> = exported_types.globals.keys().cloned().collect();
        if let Some(module_info) = self.loaded_modules.get_mut(module_path_str) {
            module_info.native_signatures = inferred_export_signatures;
            module_info.exported_types = exported_types;
        }

        let mut optimizer = Optimizer::new(aelys_opt::OptimizationLevel::Standard);
        let typed_program = optimizer.optimize(typed_program);

        let mut codegen_globals = known_globals.clone();
        codegen_globals.extend(scoped_globals.iter().cloned());
        let compiler = Compiler::with_modules(
            Some(module_path_str.to_string()),
            module_source.clone(),
            module_aliases,
            codegen_globals,
            known_native_globals,
            std::collections::HashMap::new(),
        )
        .with_module_sources(module_sources.clone());
        let (function, _globals) = compiler.compile_typed(&typed_program)?;

        let global_layout = Arc::clone(&function.global_layout);

        let func_ref = vm.alloc_function(function)?;
        vm.execute(func_ref)?;

        vm.sync_globals_to_hashmap(global_layout.names());

        let module_id = ModuleId::new(module_path_str).as_str().to_string();
        let mut published = std::collections::BTreeSet::new();
        for name in &module_global_names {
            if name.contains("::") {
                continue;
            }
            let scoped = aelys_sema::module_scoped_global(&module_id, name);
            if vm.get_global(&scoped).is_some() {
                published.insert(name.clone());
                continue;
            }
            let Some(value) = vm.get_global(name) else {
                continue;
            };
            vm.set_global(scoped, value);
            published.insert(name.clone());
        }
        if let Some(module_info) = self.loaded_modules.get_mut(module_path_str) {
            module_info.exported_types.scoped_globals = published;
        }

        self.register_exports(needs, &exports, vm)?;

        Ok(())
    }
}
