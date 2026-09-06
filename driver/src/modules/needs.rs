use crate::modules::loader::{
    LoadResult, ModuleImports, ModuleLoader, NominalScope, NominalScopeEntry,
    select_exported_nominals, widen_nominal_scope,
};
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_runtime::VM;
use aelys_sema::InferType;
use aelys_syntax::Source;
use aelys_syntax::{ImportKind, Stmt, StmtKind};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

fn resolve_loaded_module<'a>(
    loader: &'a ModuleLoader,
    needs: &aelys_syntax::NeedsStmt,
) -> (
    Option<&'a crate::modules::loader::ModuleInfo>,
    String,
    Option<String>,
) {
    let full_path = needs.path.join(".");
    if let Some(module_info) = loader.get_module(&full_path) {
        return (Some(module_info), full_path, None);
    }
    if needs.path.len() > 1 {
        let parent = needs.path[..needs.path.len() - 1].join(".");
        if let Some(module_info) = loader.get_module(&parent) {
            return (Some(module_info), parent, needs.path.last().cloned());
        }
    }
    (None, full_path, None)
}

#[derive(Default)]
struct NominalImports {
    types: aelys_sema::infer::imports::ImportedTypes,
    impl_stmts: Vec<Stmt>,
    origins: HashMap<String, String>,
    body_globals: std::collections::HashSet<String>,
    module_sources: HashMap<String, Arc<Source>>,
}

impl NominalImports {
    /// nominal declarations key on their short name all the way down to the mangled method
    fn absorb(
        &mut self,
        module_info: &crate::modules::loader::ModuleInfo,
        module_path: &str,
        scope: NominalScope<'_>,
        span: aelys_syntax::Span,
        source: &Arc<Source>,
    ) -> Result<()> {
        let (selected, impl_stmts) =
            select_exported_nominals(&module_info.exported_types, module_path, scope);
        for name in selected.nominal_names() {
            if let Some(existing) = self.origins.get(&name)
                && existing != module_path
            {
                return Err(AelysError::Compile(CompileError::new(
                    CompileErrorKind::SymbolConflict {
                        symbol: name,
                        modules: vec![existing.clone(), module_path.to_string()],
                    },
                    span,
                    source.clone(),
                )));
            }
            self.origins.insert(name, module_path.to_string());
        }
        self.types.extend(selected);
        if !impl_stmts.is_empty() {
            let module = aelys_syntax::ModuleId::new(module_path);
            self.body_globals
                .extend(module_info.exported_types.globals.keys().cloned());
            if let Some(defining) = &module_info.exported_types.source {
                self.module_sources
                    .insert(module.as_str().to_string(), defining.clone());
            }
            self.types.module_globals.insert(
                module.as_str().to_string(),
                module_info.exported_types.globals.clone(),
            );
            self.impl_stmts.extend(
                impl_stmts
                    .into_iter()
                    .map(|stmt| stmt.with_definition_module(module.clone())),
            );
        }
        Ok(())
    }
}

/// silently shadow it is rejected instead.
fn reject_local_nominal_conflicts(
    stmts: &[Stmt],
    nominal_origins: &HashMap<String, String>,
    source: &Arc<Source>,
) -> Result<()> {
    for stmt in stmts {
        let name = match &stmt.kind {
            StmtKind::EnumDecl { name, .. }
            | StmtKind::StructDecl { name, .. }
            | StmtKind::TraitDecl { name, .. } => name,
            _ => continue,
        };
        if let Some(origin) = nominal_origins.get(name) {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::SymbolConflict {
                    symbol: name.clone(),
                    modules: vec![origin.clone(), "this module".to_string()],
                },
                stmt.span,
                source.clone(),
            )));
        }
    }
    Ok(())
}

fn load_modules(
    stmts: &[Stmt],
    mut loader: ModuleLoader,
    source: Arc<Source>,
    vm: &mut VM,
) -> Result<(ModuleImports, ModuleLoader)> {
    let mut module_aliases = std::collections::HashSet::new();
    let mut known_globals = std::collections::HashSet::new();
    let mut known_native_globals = std::collections::HashSet::new();
    let mut native_signatures: HashMap<String, InferType> = HashMap::new();
    let mut symbol_origins: HashMap<String, String> = HashMap::new();
    let mut nominal_imports = NominalImports::default();
    let mut nominal_scopes: Vec<NominalScopeEntry> = Vec::new();

    let needs_stmts: Vec<&aelys_syntax::NeedsStmt> = stmts
        .iter()
        .filter_map(|s| {
            if let StmtKind::Needs(needs) = &s.kind {
                Some(needs)
            } else {
                None
            }
        })
        .collect();

    for needs in &needs_stmts {
        let result = loader.load_module(needs, vm)?;

        match result {
            LoadResult::Module(alias) => {
                module_aliases.insert(alias.clone());
            }
            LoadResult::Symbol(symbol) => {
                known_globals.insert(symbol);
            }
        }

        let module_path = needs.path.join(".");
        let is_stdlib = module_path.starts_with("std.");
        let (module_info, module_path, fallback_symbol) = resolve_loaded_module(&loader, needs);
        if let Some(module_info) = module_info {
            // `needs a::b::c` resolved to module `a::b`, so it names one symbol and must not pull
            let effective_kind = match &fallback_symbol {
                Some(symbol) => ImportKind::Symbols(vec![symbol.clone()]),
                None => needs.kind.clone(),
            };
            // `needs a::b::c` names one member of `a::b` and never a type, so it
            let wanted = match (&fallback_symbol, &effective_kind) {
                (Some(_), _) => Some(Vec::new()),
                (None, ImportKind::Symbols(symbols)) => Some(symbols.clone()),
                (None, _) => None,
            };
            widen_nominal_scope(&mut nominal_scopes, &module_path, wanted, needs.span);
            for native_name in &module_info.native_functions {
                known_native_globals.insert(native_name.clone());
            }

            let module_alias = loader.get_module_alias(needs);
            match &effective_kind {
                ImportKind::Module { alias: None } => {
                    for name in module_info.exports.keys() {
                        let qualified = format!("{}::{}", module_alias, name);
                        if let Some(existing) = symbol_origins.get(name) {
                            return Err(AelysError::Compile(CompileError::new(
                                CompileErrorKind::SymbolConflict {
                                    symbol: name.clone(),
                                    modules: vec![existing.clone(), module_path.clone()],
                                },
                                needs.span,
                                source.clone(),
                            )));
                        }
                        symbol_origins.insert(
                            name.clone(),
                            if is_stdlib {
                                qualified.clone()
                            } else {
                                module_path.clone()
                            },
                        );
                        known_globals.insert(qualified.clone());
                        known_globals.insert(name.clone());
                        if module_info.native_functions.contains(&qualified) {
                            known_native_globals.insert(name.clone());
                        }
                        if let Some(signature) = module_info
                            .native_signatures
                            .get(&qualified)
                            .or_else(|| module_info.native_signatures.get(name))
                        {
                            native_signatures.insert(qualified.clone(), signature.clone());
                            native_signatures.insert(name.clone(), signature.clone());
                        }
                    }
                }
                ImportKind::Symbols(symbols) => {
                    for sym in symbols {
                        let qualified = format!("{}::{}", module_alias, sym);
                        symbol_origins.insert(
                            sym.clone(),
                            if is_stdlib {
                                qualified.clone()
                            } else {
                                module_path.clone()
                            },
                        );
                        known_globals.insert(sym.clone());
                        if module_info.native_functions.contains(&qualified) {
                            known_native_globals.insert(sym.clone());
                        }
                        if let Some(signature) = module_info
                            .native_signatures
                            .get(&qualified)
                            .or_else(|| module_info.native_signatures.get(sym))
                        {
                            native_signatures.insert(sym.clone(), signature.clone());
                        }
                    }
                }
                ImportKind::Wildcard => {
                    for name in module_info.exports.keys() {
                        let qualified = format!("{}::{}", module_alias, name);
                        symbol_origins.insert(
                            name.clone(),
                            if is_stdlib {
                                qualified.clone()
                            } else {
                                module_path.clone()
                            },
                        );
                        known_globals.insert(qualified.clone());
                        known_globals.insert(name.clone());
                        if module_info.native_functions.contains(&qualified) {
                            known_native_globals.insert(name.clone());
                        }
                        if let Some(signature) = module_info
                            .native_signatures
                            .get(&qualified)
                            .or_else(|| module_info.native_signatures.get(name))
                        {
                            native_signatures.insert(qualified.clone(), signature.clone());
                            native_signatures.insert(name.clone(), signature.clone());
                        }
                    }
                }
                ImportKind::Module { alias: Some(_) } => {
                    for name in module_info.exports.keys() {
                        let alias_qualified = format!("{}::{}", module_alias, name);
                        let internal_qualified = format!("{}::{}", module_info.name, name);
                        known_globals.insert(alias_qualified.clone());
                        if module_info.native_functions.contains(&alias_qualified)
                            || module_info.native_functions.contains(&internal_qualified)
                        {
                            known_native_globals.insert(alias_qualified.clone());
                        }
                        if let Some(signature) = module_info
                            .native_signatures
                            .get(&alias_qualified)
                            .or_else(|| module_info.native_signatures.get(&internal_qualified))
                            .or_else(|| module_info.native_signatures.get(name))
                        {
                            native_signatures.insert(alias_qualified, signature.clone());
                        }
                    }
                }
            }
        }
    }

    for entry in &nominal_scopes {
        let Some(module_info) = loader.get_module(&entry.module_path) else {
            continue;
        };
        let scope = entry.scope();
        nominal_imports.absorb(module_info, &entry.module_path, scope, entry.span, &source)?;
    }

    reject_local_nominal_conflicts(stmts, &nominal_imports.origins, &source)?;

    Ok((
        ModuleImports {
            module_aliases,
            known_globals,
            known_native_globals,
            native_signatures,
            symbol_origins,
            imported_types: nominal_imports.types,
            imported_impl_stmts: nominal_imports.impl_stmts,
            impl_body_globals: nominal_imports.body_globals,
            module_sources: nominal_imports.module_sources,
        },
        loader,
    ))
}

pub fn load_modules_for_program(
    stmts: &[Stmt],
    entry_file: &Path,
    source: Arc<Source>,
    vm: &mut VM,
) -> Result<ModuleImports> {
    let loader = ModuleLoader::new(entry_file, source.clone());
    load_modules(stmts, loader, source, vm).map(|(imports, _)| imports)
}

pub fn load_modules_with_loader(
    stmts: &[Stmt],
    entry_file: &Path,
    source: Arc<Source>,
    vm: &mut VM,
) -> Result<(ModuleImports, ModuleLoader)> {
    let loader = ModuleLoader::new(entry_file, source.clone());
    load_modules(stmts, loader, source, vm)
}

pub fn load_modules_in_dir(
    stmts: &[Stmt],
    base_dir: &Path,
    host_modules: std::collections::HashSet<String>,
    source: Arc<Source>,
    vm: &mut VM,
) -> Result<ModuleImports> {
    let mut loader = ModuleLoader::for_base_dir(base_dir, source.clone());
    loader.set_host_modules(host_modules);
    load_modules(stmts, loader, source, vm).map(|(imports, _)| imports)
}
