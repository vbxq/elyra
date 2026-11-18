use super::types::ModuleLoader;
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_modules::resolution::{
    ExtensionPattern, ModuleKind, ModuleResolution, full_search_patterns,
    native_only_search_patterns,
};
use aelys_runtime::stdlib;
use aelys_syntax::{NeedsStmt, Span};
use std::path::{Path, PathBuf};

impl ModuleLoader {
    pub fn check_resolvable(&self, needs: &NeedsStmt) -> Result<()> {
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

        if stdlib::is_std_module(&needs.path) {
            return Ok(());
        }

        let module_path_str = needs.path.join(".");
        if self.host_modules.contains(&module_path_str) {
            return Ok(());
        }

        self.resolve_path_with_fallback(&needs.path, needs)
            .map(|_| ())
    }

    pub fn resolve_path(&self, module_path: &[String]) -> Result<PathBuf> {
        match self.resolve_module_path(module_path)? {
            ModuleResolution {
                kind: ModuleKind::Script,
                path,
            } => Ok(path),
            ModuleResolution { .. } => Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: module_path.join("."),
                    searched_paths: vec![],
                },
                Span::dummy(),
                self.source.clone(),
            ))),
        }
    }

    pub(crate) fn resolve_module_path(&self, module_path: &[String]) -> Result<ModuleResolution> {
        self.resolve_rooted(module_path)
            .map(|(resolution, _)| resolution)
    }

    // the search root is part of the module key: one dotted path is two modules iff two files
    pub(crate) fn resolve_rooted(
        &self,
        module_path: &[String],
    ) -> Result<(ModuleResolution, Option<PathBuf>)> {
        if is_std_module(module_path) {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::StdlibNotAvailable {
                    module: module_path.join("."),
                },
                Span::dummy(),
                self.source.clone(),
            )));
        }

        let module_name_str = module_path.join(".");

        if let Some(manifest) = &self.manifest
            && let Some(policy) = manifest.module(&module_name_str)
        {
            if let Some(explicit_path) = &policy.path {
                let resolved_path = self.project_dir.join(explicit_path);
                if let Some(canonical) = self.canonicalize_if_exists(&resolved_path, module_path)? {
                    let kind = if policy.is_native() {
                        ModuleKind::Native
                    } else if policy.is_script() {
                        ModuleKind::Script
                    } else {
                        let ext = resolved_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        if ext == "so" || ext == "dll" || ext == "dylib" || ext == "aelys-lib" {
                            ModuleKind::Native
                        } else {
                            ModuleKind::Script
                        }
                    };
                    return Ok((
                        ModuleResolution {
                            path: canonical,
                            kind,
                        },
                        None,
                    ));
                }
            }

            if policy.is_native() {
                return self.resolve_native_only(module_path);
            }
        }

        self.search_with_patterns(module_path, full_search_patterns())
    }

    fn canonicalize_if_exists(
        &self,
        path: &Path,
        module_path: &[String],
    ) -> Result<Option<PathBuf>> {
        if !path.exists() {
            return Ok(None);
        }
        let canonical = path.canonicalize().map_err(|_| {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: module_path.join("."),
                    searched_paths: vec![path.display().to_string()],
                },
                Span::dummy(),
                self.source.clone(),
            ))
        })?;
        if !canonical.starts_with(&self.base_root) {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: module_path.join("."),
                    searched_paths: vec![path.display().to_string()],
                },
                Span::dummy(),
                self.source.clone(),
            )));
        }
        Ok(Some(canonical))
    }

    // validates path segments (no .., no slashes) and builds filesystem path
    fn search_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.base_dir.clone()];
        let same = |left: &Path, right: &Path| match (left.canonicalize(), right.canonicalize()) {
            (Ok(left), Ok(right)) => left == right,
            _ => left == right,
        };
        if !same(&self.base_dir, &self.project_dir) {
            roots.push(self.project_dir.clone());
        }
        roots
    }

    fn validate_and_build_path(&self, root: &Path, module_path: &[String]) -> Result<PathBuf> {
        if module_path.is_empty() {
            return Err(AelysError::Compile(CompileError::new(
                CompileErrorKind::ModuleNotFound {
                    module_path: "<empty>".to_string(),
                    searched_paths: vec![],
                },
                Span::dummy(),
                self.source.clone(),
            )));
        }
        let mut path = root.to_path_buf();
        for segment in module_path {
            if segment == ".." || segment == "." || segment.contains('/') || segment.contains('\\')
            {
                return Err(AelysError::Compile(CompileError::new(
                    CompileErrorKind::ModuleNotFound {
                        module_path: module_path.join("."),
                        searched_paths: vec![],
                    },
                    Span::dummy(),
                    self.source.clone(),
                )));
            }
            path.push(segment);
        }
        Ok(path)
    }

    fn search_with_patterns(
        &self,
        module_path: &[String],
        patterns: &[ExtensionPattern],
    ) -> Result<(ModuleResolution, Option<PathBuf>)> {
        let mut searched_paths = Vec::new();
        for root in self.search_roots() {
            let base_path = self.validate_and_build_path(&root, module_path)?;
            let module_name = module_path
                .last()
                .expect("module_path validated as non-empty");
            for pattern in patterns {
                let candidate = pattern.to_path(&base_path, module_name);
                searched_paths.push(candidate.display().to_string());
                if let Some(canonical) = self.canonicalize_if_exists(&candidate, module_path)? {
                    return Ok((
                        ModuleResolution {
                            path: canonical,
                            kind: pattern.kind(),
                        },
                        Some(root),
                    ));
                }
            }
        }

        Err(AelysError::Compile(CompileError::new(
            CompileErrorKind::ModuleNotFound {
                module_path: module_path.join("."),
                searched_paths,
            },
            Span::dummy(),
            self.source.clone(),
        )))
    }

    fn resolve_native_only(
        &self,
        module_path: &[String],
    ) -> Result<(ModuleResolution, Option<PathBuf>)> {
        self.search_with_patterns(module_path, native_only_search_patterns())
    }

    pub(crate) fn search_paths_for(&self, module_path: &[String]) -> Vec<String> {
        if module_path.is_empty() {
            return Vec::new();
        }

        let module_name = module_path
            .last()
            .expect("module_path verified as non-empty");

        self.search_roots()
            .into_iter()
            .flat_map(|root| {
                let mut base_path = root;
                for segment in module_path {
                    base_path.push(segment);
                }
                full_search_patterns()
                    .iter()
                    .map(|pattern| {
                        pattern
                            .to_path(&base_path, module_name)
                            .display()
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

fn is_std_module(module_path: &[String]) -> bool {
    matches!(module_path.first().map(|s| s.as_str()), Some("std"))
}

impl ModuleLoader {
    pub(crate) fn module_key(
        &mut self,
        file: &Path,
        root: Option<&Path>,
        module_path: &[String],
    ) -> String {
        if let Some(key) = self.file_keys.get(file) {
            return key.clone();
        }
        let mut segments: Vec<String> = root
            .and_then(|root| root.canonicalize().ok())
            .and_then(|root| {
                let project = self.project_dir.canonicalize().ok()?;
                root.strip_prefix(&project).ok().map(|relative| {
                    relative
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy().into_owned())
                        .collect()
                })
            })
            .unwrap_or_default();
        segments.extend(module_path.iter().cloned());
        let mut key = segments.join(".");
        let taken = |key: &str, keys: &std::collections::HashMap<PathBuf, String>| {
            keys.values().any(|existing| existing == key)
        };
        let base = key.clone();
        let mut suffix = 2;
        while taken(&key, &self.file_keys) {
            key = format!("{base}.v{suffix}");
            suffix += 1;
        }
        self.file_keys.insert(file.to_path_buf(), key.clone());
        key
    }

    pub(crate) fn resolved_needs(&self, needs: &NeedsStmt) -> Option<&super::types::ResolvedNeeds> {
        self.resolved_needs
            .get(&(self.base_dir.clone(), needs.path.join(".")))
    }
}
