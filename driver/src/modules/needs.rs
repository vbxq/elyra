use crate::modules::loader::{
    LoadResult, ModuleImports, ModuleLoader, NominalScope, NominalScopeEntry,
    select_exported_nominals, widen_nominal_scope,
};
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind, SymbolConflictRepair};
use aelys_runtime::VM;
use aelys_sema::InferType;
use aelys_syntax::Source;
use aelys_syntax::{ImportKind, Stmt, StmtKind};
use std::collections::{BTreeMap, HashMap};
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

fn surface_module(module_path: &str) -> String {
    module_path.replace('.', "::")
}

type NominalCarriers =
    std::collections::BTreeMap<String, BTreeMap<String, std::collections::BTreeSet<String>>>;

struct NominalClash {
    symbol: String,
    modules: Vec<String>,
    carried_by: Vec<String>,
}

#[derive(Default)]
struct NominalImports {
    types: aelys_sema::infer::imports::ImportedTypes,
    impl_stmts: Vec<Stmt>,
    origins: HashMap<String, String>,
    body_globals: std::collections::HashSet<String>,
    module_sources: HashMap<String, Arc<Source>>,
    inherited: std::collections::BTreeMap<String, String>,
    carriers: NominalCarriers,
    private_decls: std::collections::HashSet<(String, String)>,
    renamed_originals: std::collections::HashSet<String>,
    needs_spans: BTreeMap<String, aelys_syntax::Span>,
}

impl NominalImports {
    /// nominal declarations key on their short name all the way down to the mangled method
    fn absorb(
        &mut self,
        module_info: &crate::modules::loader::ModuleInfo,
        module_path: &str,
        scope: NominalScope<'_>,
        span: aelys_syntax::Span,
    ) {
        let (selected, impl_stmts) =
            select_exported_nominals(&module_info.exported_types, module_path, scope);
        let exported = &module_info.exported_types;
        let via = surface_module(module_path);
        self.needs_spans.entry(via.clone()).or_insert(span);
        for (name, origin) in &exported.private_origins {
            self.inherited
                .entry(name.clone())
                .or_insert_with(|| origin.clone());
            if exported.private_types.unexported_nominals.contains(name) {
                self.private_decls.insert((name.clone(), origin.clone()));
            }
            self.carriers
                .entry(name.clone())
                .or_default()
                .entry(origin.clone())
                .or_default()
                .insert(via.clone());
        }
        self.types.extend(exported.private_types.clone());
        self.impl_stmts.extend(exported.private_impls.clone());
        self.module_sources.extend(
            exported
                .private_module_sources
                .iter()
                .map(|(module, source)| (module.clone(), source.clone())),
        );
        self.body_globals.extend(
            exported
                .private_types
                .module_globals
                .values()
                .flat_map(|globals| globals.keys().cloned()),
        );
        // rewrite gave it, never under the bare one
        for (module, names) in &exported.private_types.module_scoped_globals {
            for name in names {
                self.body_globals
                    .insert(aelys_sema::module_scoped_global(module, name));
            }
        }
        for (name, origin) in &selected.private_nominals {
            self.inherited
                .entry(name.clone())
                .or_insert_with(|| origin.clone());
            self.carriers
                .entry(name.clone())
                .or_default()
                .entry(origin.clone())
                .or_default()
                .insert(via.clone());
        }
        for name in selected.nominal_names() {
            // file imported, so it never becomes one of its origins
            if selected.private_nominals.contains_key(&name) {
                continue;
            }
            self.carriers
                .entry(name.clone())
                .or_default()
                .entry(via.clone())
                .or_default()
                .insert(via.clone());
            self.origins.insert(name, module_path.to_string());
        }
        for name in selected.nominal_names() {
            if let Some(ordinal) = exported.struct_def_ordinals.get(&name) {
                self.types
                    .struct_def_ordinals
                    .insert(name.clone(), *ordinal);
            }
            if let Some(ordinal) = exported.enum_def_ordinals.get(&name) {
                self.types.enum_def_ordinals.insert(name.clone(), *ordinal);
            }
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
            for name in &module_info.exported_types.scoped_globals {
                self.body_globals
                    .insert(aelys_sema::module_scoped_global(module.as_str(), name));
            }
            self.types.module_globals.insert(
                module.as_str().to_string(),
                module_info.exported_types.globals.clone(),
            );
            self.types.module_scoped_globals.insert(
                module.as_str().to_string(),
                module_info.exported_types.scoped_globals.clone(),
            );
            for name in exported.own_private_types.nominal_names() {
                self.inherited
                    .entry(name.clone())
                    .or_insert_with(|| via.clone());
                self.private_decls.insert((name.clone(), via.clone()));
                self.carriers
                    .entry(name.clone())
                    .or_default()
                    .entry(via.clone())
                    .or_default()
                    .insert(via.clone());
                self.types.unexported_nominals.insert(name);
            }
            for original in exported.renamed_privates.keys() {
                self.inherited
                    .entry(original.clone())
                    .or_insert_with(|| via.clone());
                self.types.unexported_nominals.insert(original.clone());
                if exported.renamed_private_traits.contains(original) {
                    self.private_decls.insert((original.clone(), via.clone()));
                    self.carriers
                        .entry(original.clone())
                        .or_default()
                        .entry(via.clone())
                        .or_default()
                        .insert(via.clone());
                    continue;
                }
                self.renamed_originals.insert(original.clone());
            }
            self.types.struct_def_ordinals.extend(
                exported
                    .struct_def_ordinals
                    .iter()
                    .map(|(k, v)| (k.clone(), *v)),
            );
            self.types.enum_def_ordinals.extend(
                exported
                    .enum_def_ordinals
                    .iter()
                    .map(|(k, v)| (k.clone(), *v)),
            );
            self.types.extend(exported.own_private_types.clone());
            self.impl_stmts.extend(
                exported
                    .own_private_impls
                    .iter()
                    .cloned()
                    .map(|stmt| stmt.with_definition_module(module.clone())),
            );
            self.impl_stmts.extend(
                impl_stmts
                    .into_iter()
                    .map(|stmt| stmt.with_definition_module(module.clone())),
            );
        }
    }

    fn public_declaring(
        &self,
        symbol: &str,
        declaring: &BTreeMap<String, std::collections::BTreeSet<String>>,
    ) -> Vec<String> {
        declaring
            .keys()
            .filter(|module| {
                !self
                    .private_decls
                    .contains(&(symbol.to_string(), (*module).clone()))
            })
            .cloned()
            .collect()
    }

    fn first_clash(&self) -> Option<(NominalClash, aelys_syntax::Span)> {
        for (symbol, declaring) in &self.carriers {
            let public = self.public_declaring(symbol, declaring);
            if public.len() < 2 {
                continue;
            }
            let modules: Vec<String> = public;
            let carried_by: Vec<String> = declaring
                .iter()
                .filter(|(module, _)| {
                    !self
                        .private_decls
                        .contains(&(symbol.clone(), (*module).clone()))
                })
                .flat_map(|(module, vias)| vias.iter().filter(move |via| *via != module))
                .cloned()
                .collect::<std::collections::BTreeSet<String>>()
                .into_iter()
                .collect();
            // the caret follows the declaring module that sorts last, never the order the
            let span = declaring
                .iter()
                .filter(|(module, _)| {
                    !self
                        .private_decls
                        .contains(&(symbol.clone(), (*module).clone()))
                })
                .map(|(_, vias)| vias)
                .next_back()
                .and_then(|vias| vias.iter().next_back())
                .and_then(|via| self.needs_spans.get(via))
                .copied()
                .unwrap_or_else(aelys_syntax::Span::dummy);
            return Some((
                NominalClash {
                    symbol: symbol.clone(),
                    modules,
                    carried_by,
                },
                span,
            ));
        }
        None
    }

    fn local_clash(&self, name: &str) -> Option<NominalClash> {
        let declaring = self.carriers.get(name)?;
        let mut modules: Vec<String> = self.public_declaring(name, declaring);
        if modules.is_empty() {
            if self.renamed_originals.contains(name) {
                return None;
            }
            modules = declaring.keys().cloned().collect();
        }
        modules.push("this module".to_string());
        let carried_by: Vec<String> = declaring
            .iter()
            .filter(|(module, _)| {
                !self
                    .private_decls
                    .contains(&(name.to_string(), (*module).clone()))
            })
            .flat_map(|(module, vias)| vias.iter().filter(move |via| *via != module))
            .cloned()
            .collect::<std::collections::BTreeSet<String>>()
            .into_iter()
            .collect();
        Some(NominalClash {
            symbol: name.to_string(),
            modules,
            carried_by,
        })
    }
}

impl NominalClash {
    fn into_error(
        self,
        repair: SymbolConflictRepair,
        span: aelys_syntax::Span,
        source: &Arc<Source>,
    ) -> AelysError {
        AelysError::Compile(CompileError::new(
            CompileErrorKind::SymbolConflict {
                symbol: self.symbol,
                modules: self.modules,
                carried_by: self.carried_by,
                repair,
            },
            span,
            source.clone(),
        ))
    }
}

/// silently shadow it is rejected instead.
fn reject_local_nominal_conflicts(
    stmts: &[Stmt],
    imports: &NominalImports,
    source: &Arc<Source>,
) -> Result<()> {
    let first = stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::EnumDecl { name, .. }
            | StmtKind::StructDecl { name, .. }
            | StmtKind::TraitDecl { name, .. } => {
                imports.local_clash(name).map(|clash| (clash, stmt.span))
            }
            _ => None,
        })
        .min_by(|left, right| left.0.symbol.cmp(&right.0.symbol));
    match first {
        Some((clash, span)) => {
            Err(clash.into_error(SymbolConflictRepair::RenameLocal, span, source))
        }
        None => Ok(()),
    }
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
                                    carried_by: Vec::new(),
                                    repair: SymbolConflictRepair::Alias,
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
        nominal_imports.absorb(module_info, &entry.module_path, scope, entry.span);
    }

    if let Some((clash, span)) = nominal_imports.first_clash() {
        return Err(clash.into_error(SymbolConflictRepair::NameOne, span, &source));
    }

    reject_local_nominal_conflicts(stmts, &nominal_imports, &source)?;

    // reach a module by two routes, so the same imported body must not register twice
    let mut seen_impls: std::collections::HashSet<(String, usize, usize)> =
        std::collections::HashSet::new();
    nominal_imports.impl_stmts.retain(|stmt| {
        let module = stmt
            .definition_module
            .as_ref()
            .map(|module| module.as_str().to_string())
            .unwrap_or_default();
        seen_impls.insert((module, stmt.span.start, stmt.span.end))
    });
    nominal_imports.types.private_nominals = nominal_imports
        .inherited
        .iter()
        .filter(|(name, _)| !nominal_imports.origins.contains_key(*name))
        .map(|(name, origin)| (name.clone(), origin.clone()))
        .collect();

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
