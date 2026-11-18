use aelys_modules::loader::FileFingerprint;
use aelys_modules::manifest::Manifest;
use aelys_native::AelysNativeType;
use aelys_sema::InferType;
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String, // e.g. "utils.helpers"
    pub file_path: PathBuf,
    pub version: Option<String>, // native modules only
    pub exports: HashMap<String, ExportInfo>,
    pub native_functions: Vec<String>,
    pub native_signatures: HashMap<String, InferType>,
    pub exported_types: super::exported_types::ExportedTypes,
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub is_function: bool,
    pub is_mutable: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedNativeInfo {
    pub file_path: PathBuf,
    pub name: String,
}

pub struct ModuleImports {
    pub module_aliases: HashSet<String>, // "utils" from `needs utils`
    pub known_globals: HashSet<String>,  // direct imports
    pub known_native_globals: HashSet<String>, // native funcs for codegen opt
    pub native_signatures: HashMap<String, InferType>,
    pub symbol_origins: HashMap<String, String>, // symbol -> module_path
    pub imported_types: aelys_sema::infer::imports::ImportedTypes,
    pub imported_impl_stmts: Vec<aelys_syntax::Stmt>,
    pub impl_body_globals: HashSet<String>,
    pub module_sources: HashMap<String, Arc<Source>>,
    pub global_renames: HashMap<String, String>,
    pub generic_targets: HashMap<String, String>,
}

impl ModuleImports {
    // the entry file's spellings of an imported generic function name the carried copy
    pub fn program_stmts(&self, stmts: Vec<aelys_syntax::Stmt>) -> Vec<aelys_syntax::Stmt> {
        let mut own: Vec<_> = stmts
            .into_iter()
            .filter(|stmt| !matches!(stmt.kind, aelys_syntax::StmtKind::Needs(_)))
            .collect();
        super::rewrite_generic_references(&mut own, &self.generic_targets);
        self.imported_impl_stmts
            .iter()
            .cloned()
            .chain(own)
            .collect()
    }
}

pub enum LoadResult {
    Module(String), // qualified access: mod.func
    Symbol(String), // direct access
}

pub struct ModuleLoader {
    pub(crate) base_dir: PathBuf,
    pub(crate) base_root: PathBuf,
    pub(crate) loaded_modules: HashMap<String, ModuleInfo>,
    pub(crate) loading_stack: Vec<String>,
    pub(crate) source: Arc<Source>,
    pub(crate) native_fingerprints: HashMap<String, FileFingerprint>,
    pub(crate) manifest: Option<Manifest>,
    pub(crate) loaded_native_modules: HashMap<String, LoadedNativeInfo>,
    pub(crate) host_modules: HashSet<String>,
    pub(crate) project_dir: PathBuf,
    pub(crate) file_keys: HashMap<PathBuf, String>,
    pub(crate) resolved_needs: HashMap<(PathBuf, String), ResolvedNeeds>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedNeeds {
    pub(crate) key: String,
    pub(crate) symbol: Option<String>,
}

pub fn native_type_to_infer_type(native_type: AelysNativeType) -> InferType {
    match native_type {
        AelysNativeType::Int => InferType::I64,
        AelysNativeType::Float => InferType::F64,
        AelysNativeType::Bool => InferType::Bool,
        AelysNativeType::String => InferType::String,
        AelysNativeType::Unit => InferType::Unit,
        AelysNativeType::Dynamic => InferType::Dynamic,
        AelysNativeType::OptionInt => InferType::Option(Box::new(InferType::I64)),
        AelysNativeType::OptionFloat => InferType::Option(Box::new(InferType::F64)),
        AelysNativeType::OptionBool => InferType::Option(Box::new(InferType::Bool)),
        AelysNativeType::OptionString => InferType::Option(Box::new(InferType::String)),
        AelysNativeType::OptionUnit => InferType::Option(Box::new(InferType::Unit)),
        AelysNativeType::ResultIntString => {
            InferType::Result(Box::new(InferType::I64), Box::new(InferType::String))
        }
        AelysNativeType::ResultFloatString => {
            InferType::Result(Box::new(InferType::F64), Box::new(InferType::String))
        }
        AelysNativeType::ResultBoolString => {
            InferType::Result(Box::new(InferType::Bool), Box::new(InferType::String))
        }
        AelysNativeType::ResultStringString => {
            InferType::Result(Box::new(InferType::String), Box::new(InferType::String))
        }
        AelysNativeType::ResultUnitString => {
            InferType::Result(Box::new(InferType::Unit), Box::new(InferType::String))
        }
    }
}

// a bare name two whole-module imports share is refused only at a use; a named import wins
#[derive(Debug, Default)]
pub struct BareImports {
    bare: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    explicit: HashMap<String, String>,
}

impl BareImports {
    pub fn note_bare(&mut self, name: &str, module: &str, alias: &str) {
        self.bare
            .entry(name.to_string())
            .or_default()
            .insert(module.to_string(), alias.to_string());
    }

    pub fn is_explicit(&self, name: &str) -> bool {
        self.explicit.contains_key(name)
    }

    pub fn note_explicit(&mut self, name: &str, module: &str) -> Option<String> {
        match self.explicit.get(name) {
            Some(previous) if previous != module => Some(previous.clone()),
            Some(_) => None,
            None => {
                self.explicit.insert(name.to_string(), module.to_string());
                None
            }
        }
    }

    pub fn ambiguous(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        self.bare
            .iter()
            .filter(|(name, modules)| modules.len() > 1 && !self.explicit.contains_key(*name))
            .map(|(name, modules)| {
                let aliases: std::collections::BTreeSet<String> =
                    modules.values().cloned().collect();
                (name.clone(), aliases.into_iter().collect())
            })
            .collect()
    }
}
