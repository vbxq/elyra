use super::types::{LoadResult, ModuleLoader};
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_runtime::VM;
use aelys_runtime::stdlib;
use aelys_syntax::{ImportKind, NeedsStmt};

impl ModuleLoader {
    pub fn load_module(&mut self, needs: &NeedsStmt, vm: &mut VM) -> Result<LoadResult> {
        if needs.path.is_empty() {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: "<empty>".to_string(),
                    searched_paths: vec![],
                },
                needs.span,
                self.source.clone(),
            )));
        }

        let module_path_str = needs.path.join(".");

        if let Some(replacement) = stdlib::removed_std_module(&needs.path) {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::RemovedStdModule {
                    module: needs.path.join("::"),
                    replacement,
                },
                needs.span,
                self.source.clone(),
            )));
        }

        if stdlib::is_std_module(&needs.path) {
            return self.load_std_module(needs, vm);
        }

        if self.host_modules.contains(&module_path_str) {
            return Ok(self.get_load_result(needs));
        }

        let (resolution, root, actual_path, symbol) =
            self.resolve_path_with_fallback(&needs.path, needs)?;
        let key = self.module_key(&resolution.path, root.as_deref(), &actual_path);
        self.resolved_needs.insert(
            (self.base_dir.clone(), module_path_str.clone()),
            super::types::ResolvedNeeds {
                key: key.clone(),
                symbol: symbol.clone(),
            },
        );

        if self.loading_stack.contains(&key) {
            let mut chain = self.loading_stack.clone();
            chain.push(key.clone());
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::CircularDependency { chain },
                needs.span,
                self.source.clone(),
            )));
        }

        let effective_needs = if let Some(sym) = &symbol {
            NeedsStmt {
                path: actual_path.clone(),
                kind: ImportKind::Symbols(vec![sym.clone()]),
                span: needs.span,
            }
        } else {
            needs.clone()
        };

        if self.loaded_modules.contains_key(&key) {
            if self.native_fingerprints.contains_key(&key)
                && let Some(current) = self.current_native_fingerprint(&resolution.path)
            {
                self.native_fingerprints.insert(key.clone(), current);
            }
            let exports = self
                .loaded_modules
                .get(&key)
                .map(|module_info| module_info.exports.clone())
                .unwrap_or_default();
            self.register_exports(&effective_needs, &key, &exports, vm)?;
        } else {
            self.loading_stack.push(key.clone());
            let result = match resolution.kind {
                aelys_modules::resolution::ModuleKind::Script => {
                    self.compile_module(&resolution.path, &key, &effective_needs, vm)
                }
                aelys_modules::resolution::ModuleKind::Native => {
                    self.load_native_module(&resolution.path, &key, &effective_needs, vm)
                }
            };
            self.loading_stack.pop();
            result?;
        }

        if let Some(sym) = symbol {
            Ok(LoadResult::Symbol(sym))
        } else {
            Ok(self.get_load_result(needs))
        }
    }
}
