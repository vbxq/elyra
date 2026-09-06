pub mod loader;
mod needs;

pub use loader::{
    ExportInfo, LoadResult, LoadedNativeInfo, ModuleImports, ModuleInfo, ModuleLoader,
};
pub use needs::{load_modules_for_program, load_modules_in_dir, load_modules_with_loader};

use aelys_runtime::VM;
use std::collections::{HashMap, HashSet};

pub struct ResolvedGlobals {
    pub module_aliases: HashSet<String>,
    pub known_globals: HashSet<String>,
    pub known_native_globals: HashSet<String>,
    pub symbol_origins: HashMap<String, String>,
    pub codegen_globals: HashSet<String>,
}

pub fn diagnostic_source(
    imports: Option<&ModuleImports>,
    module: Option<&str>,
    fallback: &std::sync::Arc<aelys_syntax::Source>,
) -> std::sync::Arc<aelys_syntax::Source> {
    imports
        .zip(module)
        .and_then(|(imports, module)| imports.module_sources.get(module).cloned())
        .unwrap_or_else(|| fallback.clone())
}

pub fn resolve_globals(imports: Option<&ModuleImports>, vm: &VM) -> ResolvedGlobals {
    let mut resolved = ResolvedGlobals {
        module_aliases: vm.repl_module_aliases().clone(),
        known_globals: vm.repl_known_globals().clone(),
        known_native_globals: vm.repl_known_native_globals().clone(),
        symbol_origins: vm.repl_symbol_origins().clone(),
        codegen_globals: HashSet::new(),
    };

    if let Some(imports) = imports {
        resolved
            .module_aliases
            .extend(imports.module_aliases.iter().cloned());
        resolved
            .known_globals
            .extend(imports.known_globals.iter().cloned());
        resolved
            .known_native_globals
            .extend(imports.known_native_globals.iter().cloned());
        for (symbol, origin) in &imports.symbol_origins {
            resolved
                .symbol_origins
                .insert(symbol.clone(), origin.clone());
        }
        resolved
            .codegen_globals
            .extend(imports.impl_body_globals.iter().cloned());
    }

    resolved
        .codegen_globals
        .extend(resolved.known_globals.iter().cloned());
    resolved
}
