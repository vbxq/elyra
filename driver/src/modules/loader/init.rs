use super::types::{LoadedNativeInfo, ModuleLoader};
use aelys_modules::manifest::Manifest;
use aelys_syntax::Source;
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl ModuleLoader {
    pub fn new(entry_file: &Path, source: Arc<Source>) -> Self {
        let manifest = Manifest::for_source_file(entry_file);
        Self::with_manifest(entry_file, source, manifest)
    }

    pub fn with_manifest(
        entry_file: &Path,
        source: Arc<Source>,
        manifest: Option<Manifest>,
    ) -> Self {
        let base_dir = entry_file
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| ".".into());
        Self::rooted(base_dir, source, manifest)
    }

    pub fn for_base_dir(base_dir: &Path, source: Arc<Source>) -> Self {
        let manifest = Manifest::from_file(&base_dir.join("aelys.toml")).ok();
        Self::rooted(base_dir.to_path_buf(), source, manifest)
    }

    fn rooted(base_dir: PathBuf, source: Arc<Source>, manifest: Option<Manifest>) -> Self {
        let base_root = base_dir.canonicalize().unwrap_or_else(|_| base_dir.clone());
        Self {
            base_dir,
            base_root,
            loaded_modules: std::collections::HashMap::new(),
            loading_stack: Vec::new(),
            source,
            native_fingerprints: std::collections::HashMap::new(),
            manifest,
            loaded_native_modules: std::collections::HashMap::new(),
            host_modules: std::collections::HashSet::new(),
        }
    }

    pub fn set_host_modules(&mut self, modules: std::collections::HashSet<String>) {
        self.host_modules = modules;
    }

    pub fn manifest(&self) -> Option<&Manifest> {
        self.manifest.as_ref()
    }
    pub fn loaded_native_modules(&self) -> &std::collections::HashMap<String, LoadedNativeInfo> {
        &self.loaded_native_modules
    }
}
