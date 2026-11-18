use super::types::{ExportInfo, ModuleInfo, ModuleLoader};
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_runtime::VM;
use aelys_syntax::{ImportKind, Stmt, StmtKind};

impl ModuleLoader {
    pub(crate) fn collect_exports(
        &self,
        stmts: &[Stmt],
        _module_path: &str,
    ) -> Result<std::collections::HashMap<String, ExportInfo>> {
        let mut exports = std::collections::HashMap::new();

        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Function(func) => {
                    if func.is_pub {
                        exports.insert(
                            func.name.clone(),
                            ExportInfo {
                                is_function: true,
                                is_mutable: false,
                            },
                        );
                    }
                }
                StmtKind::Let {
                    name,
                    mutable,
                    is_pub,
                    ..
                } => {
                    if *is_pub {
                        exports.insert(
                            name.clone(),
                            ExportInfo {
                                is_function: false,
                                is_mutable: *mutable,
                            },
                        );
                    }
                }
                StmtKind::Needs(_) => {}
                _ => {}
            }
        }

        Ok(exports)
    }

    pub(crate) fn register_exports(
        &self,
        needs: &aelys_syntax::NeedsStmt,
        module_key: &str,
        exports: &std::collections::HashMap<String, ExportInfo>,
        vm: &mut VM,
    ) -> Result<()> {
        let module_alias = self.get_module_alias(needs);
        let module_path_str = module_key.to_string();
        let module_id = aelys_syntax::ModuleId::new(module_key);
        // stdlib constant this module never wrote
        let read = |vm: &VM, name: &str| {
            vm.get_global(&aelys_sema::module_scoped_global(module_id.as_str(), name))
                .or_else(|| vm.get_global(name))
        };

        match &needs.kind {
            ImportKind::Module { alias } => {
                for name in exports.keys() {
                    let qualified_name = format!("{}::{}", module_alias, name);
                    let value = read(vm, name).ok_or_else(|| {
                        AelysError::Compile(CompileError::new(
                            CompileErrorKind::SymbolNotFound {
                                symbol: name.clone(),
                                module: module_path_str.clone(),
                            },
                            needs.span,
                            self.source.clone(),
                        ))
                    })?;
                    vm.set_global(qualified_name, value);
                    if alias.is_none() {
                        vm.set_global(name.clone(), value);
                    }
                }
            }
            ImportKind::Symbols(symbols) => {
                for symbol in symbols {
                    if !exports.contains_key(symbol) {
                        self.reject_unimportable_symbol(&module_path_str, symbol, needs.span)?;
                        continue;
                    }
                    let value = read(vm, symbol).ok_or_else(|| {
                        AelysError::Compile(CompileError::new(
                            CompileErrorKind::SymbolNotFound {
                                symbol: symbol.clone(),
                                module: module_path_str.clone(),
                            },
                            needs.span,
                            self.source.clone(),
                        ))
                    })?;
                    vm.set_global(symbol.clone(), value);
                }
            }
            ImportKind::Wildcard => {
                for name in exports.keys() {
                    let value = read(vm, name).ok_or_else(|| {
                        AelysError::Compile(CompileError::new(
                            CompileErrorKind::SymbolNotFound {
                                symbol: name.clone(),
                                module: module_path_str.clone(),
                            },
                            needs.span,
                            self.source.clone(),
                        ))
                    })?;
                    vm.set_global(name.clone(), value);
                }
            }
        }

        Ok(())
    }

    pub(crate) fn reject_unimportable_symbol(
        &self,
        module_path: &str,
        symbol: &str,
        span: aelys_syntax::Span,
    ) -> Result<()> {
        let exported_types = self
            .loaded_modules
            .get(module_path)
            .map(|module_info| &module_info.exported_types);
        if let Some(exported_types) = exported_types {
            if exported_types
                .types
                .nominal_names()
                .iter()
                .any(|name| name == symbol)
            {
                return Ok(());
            }
            if exported_types.private_names.contains(symbol) {
                return Err(AelysError::Compile(CompileError::new(
                    CompileErrorKind::SymbolNotPublic {
                        symbol: symbol.to_string(),
                        module: module_path.to_string(),
                    },
                    span,
                    self.source.clone(),
                )));
            }
        }
        Err(AelysError::Compile(CompileError::new(
            CompileErrorKind::SymbolNotFound {
                symbol: symbol.to_string(),
                module: module_path.to_string(),
            },
            span,
            self.source.clone(),
        )))
    }

    pub fn is_symbol_public(&self, module_path: &str, symbol: &str) -> bool {
        self.loaded_modules
            .get(module_path)
            .map(|m| m.exports.contains_key(symbol))
            .unwrap_or(false)
    }

    // an import spelling is local to one importer, so it reads the defining module's own slot
    pub(crate) fn import_renames(
        &self,
        needs: &aelys_syntax::NeedsStmt,
        kind: &ImportKind,
        module_key: &str,
    ) -> Vec<(String, String)> {
        let Some(module_info) = self.loaded_modules.get(module_key) else {
            return Vec::new();
        };
        if self.loaded_native_modules.contains_key(module_key) {
            return Vec::new();
        }
        let is_std = module_key.starts_with("std.");
        let target = |name: &str| {
            if is_std {
                format!("{}::{}", module_info.name, name)
            } else {
                aelys_sema::module_scoped_global(
                    aelys_syntax::ModuleId::new(module_key).as_str(),
                    name,
                )
            }
        };
        let alias = self.get_module_alias(needs);
        let mut renames = Vec::new();
        match kind {
            ImportKind::Module { alias: None } | ImportKind::Wildcard => {
                for name in module_info.exports.keys() {
                    renames.push((format!("{alias}::{name}"), target(name)));
                    renames.push((name.clone(), target(name)));
                }
            }
            ImportKind::Module { alias: Some(_) } => {
                for name in module_info.exports.keys() {
                    renames.push((format!("{alias}::{name}"), target(name)));
                }
            }
            ImportKind::Symbols(symbols) => {
                for name in symbols {
                    if module_info.exports.contains_key(name) {
                        renames.push((name.clone(), target(name)));
                    }
                }
            }
        }
        renames
    }

    pub(crate) fn import_mutability(
        &self,
        needs: &aelys_syntax::NeedsStmt,
        kind: &ImportKind,
        module_key: &str,
    ) -> (Vec<String>, Vec<String>) {
        let Some(module_info) = self.loaded_modules.get(module_key) else {
            return (Vec::new(), Vec::new());
        };
        let alias = self.get_module_alias(needs);
        let mut immutable = Vec::new();
        let mut mutable = Vec::new();
        let bare: Vec<&String> = match kind {
            ImportKind::Module { alias: None } | ImportKind::Wildcard => {
                module_info.exports.keys().collect()
            }
            ImportKind::Module { alias: Some(_) } => Vec::new(),
            ImportKind::Symbols(symbols) => symbols.iter().collect(),
        };
        for name in bare {
            match module_info.exports.get(name) {
                Some(export) if export.is_mutable => mutable.push(name.clone()),
                Some(_) => immutable.push(name.clone()),
                None => {}
            }
        }
        if !matches!(kind, ImportKind::Symbols(_)) {
            for (name, export) in &module_info.exports {
                if !export.is_mutable {
                    immutable.push(format!("{alias}::{name}"));
                }
            }
        }
        (immutable, mutable)
    }

    pub fn get_module(&self, module_path: &str) -> Option<&ModuleInfo> {
        self.loaded_modules.get(module_path)
    }
}
